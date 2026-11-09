//! Build a backup plan from a `Source`: which users and folders, in which
//! order, and what can't be backed up (manual steps).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use super::custom::plan_custom;
use super::exclude::{Category, ExcludeOptions};
use super::extras::{GeneratedFile, plan_user_packs};
use super::manifest::{CustomRecord, FolderRecord, ManualStep, PackRecord, SourceInfo, UserRecord};
use super::walk::{FolderJob, PathIssue, PlannedFile, ReparsePoint, rel_components, walk};
use crate::hive::dirty;
use crate::packs::schema::CompiledPack;
use crate::source::{Profile, Source, SourceError};
use crate::winpath::{Mapped, resolve_ci};

/// Tier 1 folders and their priority (SPEC §3.5: Desktop, Documents,
/// Pictures → settings → Music, Videos → Downloads → custom).
pub const TIER1: &[(&str, u32)] = &[
    ("Desktop", 10),
    ("Documents", 11),
    ("Pictures", 12),
    ("Favorites", 20),
    ("Links", 21),
    ("Contacts", 22),
    ("Searches", 23),
    ("SavedGames", 24),
    ("3DObjects", 25),
    ("Music", 30),
    ("Videos", 31),
    ("Downloads", 40),
];
pub const HIVES_PRIORITY: u32 = 0;
/// App packs: after Pictures, before the small Tier 1 folders.
pub const PACK_PRIORITY: u32 = 15;
pub const PUBLIC_PRIORITY: u32 = 45;
pub const CUSTOM_PRIORITY: u32 = 50;
pub const HIVES_LABEL: &str = "Registry hives (raw)";

#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    /// Usernames or SIDs; empty = every user.
    pub users: Vec<String>,
    /// Tier 1 folder names; empty = all of them.
    pub folders: Vec<String>,
    pub exclude: ExcludeOptions,
    /// Packs to detect (empty = none).
    pub packs: Vec<CompiledPack>,
    /// Include `Users\Public`.
    pub public: bool,
    /// Drive-root folder names to include (e.g. `Projects`).
    pub root_folders: Vec<String>,
    /// (label, Windows path) custom folders.
    pub custom: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub source: SourceInfo,
    pub source_root: PathBuf,
    pub users: Vec<UserRecord>,
    /// Sorted by (priority, destination path).
    pub files: Vec<PlannedFile>,
    pub reparse_points: Vec<ReparsePoint>,
    pub excluded: BTreeMap<Category, (u64, u64)>,
    pub issues: Vec<PathIssue>,
    pub manual_steps: Vec<ManualStep>,
    pub warnings: Vec<String>,
    /// priority → label, for per-folder sizes.
    pub labels: BTreeMap<u32, String>,
    pub packs: Vec<PackRecord>,
    pub custom: Vec<CustomRecord>,
    pub offered_root_folders: Vec<String>,
    /// Files Rebootly generates (e.g. `.reg` exports).
    pub generated: Vec<GeneratedFile>,
}

fn wanted(list: &[String], candidates: &[&str]) -> bool {
    list.is_empty()
        || list
            .iter()
            .any(|w| candidates.iter().any(|c| w.eq_ignore_ascii_case(c)))
}

/// Folder name used in the backup for a user, unique within the job.
fn unique_name(p: &Profile, used: &mut HashSet<String>) -> String {
    let mut name = p.username.clone();
    if !used.insert(name.to_lowercase()) {
        let rid = p.sid.rsplit('-').next().unwrap_or("x");
        name = format!("{}_{rid}", p.username);
        used.insert(name.to_lowercase());
    }
    name
}

pub fn plan(src: &dyn Source, opts: &PlanOptions) -> Result<Plan, SourceError> {
    let os = src.os_info();
    let mut plan = Plan {
        source: SourceInfo {
            machine: os.computer_name.clone(),
            os_name: os.friendly_name(),
            edition: os.edition_id.clone(),
            display_version: os.display_version.clone(),
            build: os.build,
            ubr: os.ubr,
            environment: src.environment().as_str().into(),
        },
        source_root: src.source_root().to_path_buf(),
        ..Plan::default()
    };
    plan.labels.insert(HIVES_PRIORITY, HIVES_LABEL.into());
    plan.labels.insert(PACK_PRIORITY, "App packs".into());
    plan.labels.insert(PUBLIC_PRIORITY, "Public".into());
    plan.labels.insert(CUSTOM_PRIORITY, "Custom folders".into());
    for (name, prio) in TIER1 {
        plan.labels.insert(*prio, (*name).into());
    }

    let mut jobs: Vec<FolderJob> = Vec::new();
    let mut seen_roots: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut used_names = HashSet::new();
    for profile in src.profiles()? {
        if !wanted(&opts.users, &[&profile.username, &profile.sid]) {
            continue;
        }
        let uname = unique_name(&profile, &mut used_names);
        for w in &profile.warnings {
            plan.warnings.push(format!("{uname}: {w}"));
        }
        let mut record = UserRecord {
            sid: profile.sid.clone(),
            username: uname.clone(),
            account_kind: profile.kind.label().into(),
            profile_path: profile.profile_path.clone(),
            hive_dirty: profile.hive.as_ref().map(|h| h.state.is_dirty()),
            folders: BTreeMap::new(),
        };
        let Some(profile_dir) = profile
            .local
            .local_path()
            .filter(|_| profile.local.exists())
        else {
            plan.manual_steps.push(ManualStep {
                title: format!("Profile of {uname} not backed up"),
                detail: format!("{} is not on the scanned volume.", profile.profile_path),
            });
            plan.users.push(record);
            continue;
        };
        add_hives(&mut plan, &profile, profile_dir, &uname);

        let resolved = src.resolve_folders(&profile)?;
        for f in &resolved {
            let prio = TIER1
                .iter()
                .find(|(n, _)| *n == f.path.name)
                .map(|(_, p)| *p);
            let mut rec = FolderRecord {
                guid: f.path.guid.into(),
                source_path: f.path.win_path.clone(),
                included: false,
                note: f.onedrive.then(|| "OneDrive (Known Folder Move)".into()),
            };
            let Some(prio) = prio.filter(|_| wanted(&opts.folders, &[f.path.name])) else {
                rec.note = Some("not part of this backup".into());
                record.folders.insert(f.path.name.into(), rec);
                continue;
            };
            match &f.local {
                Mapped::Local { path, exists: true } => {
                    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if let Some(first) = seen_roots.get(&canon) {
                        rec.note = Some(format!("same folder as {first}; copied once"));
                    } else {
                        seen_roots.insert(canon, format!("{uname}/{}", f.path.name));
                        rec.included = true;
                        jobs.push(FolderJob {
                            user: uname.clone(),
                            folder: f.path.name.into(),
                            src_root: path.clone(),
                            win_root: f.path.win_path.clone(),
                            profile_rel: rel_components(path, profile_dir),
                            dest_root: format!("users/{uname}/folders/{}", f.path.name),
                            priority: prio,
                            filter: None,
                        });
                    }
                }
                Mapped::Local { .. } => rec.note = Some("not found on disk".into()),
                Mapped::OtherDrive { drive } => {
                    rec.note = Some(format!("on drive {drive}:, not mounted"));
                    plan.manual_steps.push(ManualStep {
                        title: format!("{} of {uname} is on drive {drive}:", f.path.name),
                        detail: format!(
                            "{} was not backed up. Back up drive {drive}: separately, or mount it and run again.",
                            f.path.win_path
                        ),
                    });
                }
                Mapped::Network => {
                    rec.note = Some("network share, not copied".into());
                    plan.manual_steps.push(ManualStep {
                        title: format!("{} of {uname} is on a network share", f.path.name),
                        detail: format!(
                            "{} stays on the share; reconnect it on the new PC.",
                            f.path.win_path
                        ),
                    });
                }
                Mapped::Unmappable => rec.note = Some("path could not be interpreted".into()),
            }
            record.folders.insert(f.path.name.into(), rec);
        }
        plan.users.push(record);
        plan_user_packs(
            src,
            &profile,
            &uname,
            profile_dir,
            &resolved,
            opts,
            &mut plan,
            &mut jobs,
            &mut seen_roots,
        )?;
    }
    plan_custom(src, opts, &mut plan, &mut jobs, &mut seen_roots);

    // Nested planned roots are copied once, by their own job. Filtered (pack)
    // jobs copy only part of their root, so the parent still walks into them.
    let skip: HashSet<PathBuf> = jobs
        .iter()
        .filter(|j| j.filter.is_none())
        .map(|j| j.src_root.clone())
        .collect();
    for job in &jobs {
        let w = walk(job, &skip, opts.exclude);
        plan.files.extend(w.files);
        plan.reparse_points.extend(w.reparse_points);
        plan.issues.extend(w.issues);
        for (cat, (n, b)) in w.excluded {
            let e = plan.excluded.entry(cat).or_default();
            e.0 += n;
            e.1 += b;
        }
    }
    plan.files
        .sort_by(|a, b| (a.priority, &a.dest).cmp(&(b.priority, &b.dest)));
    Ok(plan)
}

/// Raw copies of NTUSER.DAT, UsrClass.dat and their logs (SPEC §3.4, §3.6).
fn add_hives(plan: &mut Plan, profile: &Profile, profile_dir: &Path, uname: &str) {
    let Some(hive) = &profile.hive else { return };
    let usrclass_rel: Vec<String> = ["AppData", "Local", "Microsoft", "Windows", "UsrClass.dat"]
        .map(String::from)
        .to_vec();
    let (usrclass, has_usrclass) = resolve_ci(profile_dir, &usrclass_rel);
    let mut paths = vec![hive.path.clone()];
    paths.extend(dirty::log_paths(&hive.path));
    if has_usrclass {
        paths.push(usrclass.clone());
        paths.extend(dirty::log_paths(&usrclass));
    }
    for p in paths {
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rel = rel_components(&p, profile_dir).unwrap_or_else(|| vec![name.clone()]);
        plan.files.push(PlannedFile {
            src_win: format!(
                r"{}\{}",
                profile.profile_path.trim_end_matches('\\'),
                rel.join("\\")
            ),
            dest: format!("system/hives/{uname}/{name}"),
            size: meta.len(),
            mtime: meta.modified().ok(),
            priority: HIVES_PRIORITY,
            attributes: None,
            src: p,
        });
    }
}
