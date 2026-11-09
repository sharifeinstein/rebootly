//! Planning app packs (SPEC §6) for each user.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::manifest::{ManualStep, PackPathRecord, PackPathStatus, PackRecord};
use super::plan::{PACK_PRIORITY, Plan, PlanOptions};
use super::walk::{FolderJob, PathFilter, rel_components};
use crate::packs::detect::{DetectCtx, detect, os_tag};
use crate::packs::schema::{Mode, hkcu_subpath};
use crate::source::{Profile, ResolvedFolder, Source, SourceError};
use crate::winpath::Mapped;

/// A file Rebootly writes into the backup itself (e.g. `.reg` exports).
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub dest: String,
    pub bytes: Vec<u8>,
    pub source: String,
}

/// Roots already planned (canonical path → who planned it), to copy once.
pub type SeenRoots = BTreeMap<PathBuf, String>;

pub(super) fn claim(seen: &mut SeenRoots, path: &Path, who: String) -> Option<String> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match seen.get(&canon) {
        Some(first) => Some(first.clone()),
        None => {
            seen.insert(canon, who);
            None
        }
    }
}

fn leaf_name(win: &str) -> String {
    crate::winpath::leaf(win).unwrap_or_else(|| "root".into())
}

/// Detect and plan every pack for one user.
#[allow(clippy::too_many_arguments)]
pub fn plan_user_packs(
    src: &dyn Source,
    profile: &Profile,
    uname: &str,
    profile_dir: &Path,
    folders: &[ResolvedFolder],
    opts: &PlanOptions,
    plan: &mut Plan,
    jobs: &mut Vec<FolderJob>,
    seen: &mut SeenRoots,
) -> Result<(), SourceError> {
    if opts.packs.is_empty() {
        return Ok(());
    }
    let mut vars = src.user_vars(profile)?;
    for f in folders {
        vars.set(&format!("KF:{}", f.path.name), f.path.win_path.clone());
    }
    let ntuser = src.open_user_hive(profile)?;
    let mut apps = src.machine_apps().to_vec();
    if let Some(h) = &ntuser {
        apps.extend(crate::apps::user_apps(h).unwrap_or_default());
    }
    let map = |w: &str| src.map_path(w);
    let ctx = DetectCtx {
        vars: &vars,
        map: &map,
        apps: &apps,
        ntuser: ntuser.as_ref(),
        os: os_tag(plan.source.build),
    };
    let mut sensitive = Vec::new();

    for cp in &opts.packs {
        let Ok(reason) = detect(cp, &ctx) else {
            continue;
        };
        let pack = &cp.pack;
        let mut rec = PackRecord {
            id: pack.id.clone(),
            name: pack.name.clone(),
            version: pack.version,
            origin: cp.origin.clone(),
            user: uname.into(),
            detected_by: reason,
            winget_id: pack.winget_id.clone(),
            requires_closed: pack.requires_closed.clone(),
            notes: pack.notes.clone(),
            post_restore: pack.post_restore.clone(),
            paths: Vec::new(),
            registry_file: None,
        };
        let mut used_leaves: Vec<String> = Vec::new();
        for (i, pp) in pack.paths.iter().enumerate() {
            let e = vars.expand(&pp.src);
            let mut prec = PackPathRecord {
                template: pp.src.clone(),
                source_path: e.text.clone(),
                dest: None,
                mode: pp.mode,
                rewrite: pp.rewrite,
                status: PackPathStatus::NotFound,
                listed: Vec::new(),
            };
            let mapped = if e.unresolved.is_empty() {
                src.map_path(&e.text)
            } else {
                Mapped::Unmappable
            };
            let local = match &mapped {
                Mapped::Local { path, exists: true } => path.clone(),
                Mapped::Local { .. } | Mapped::Unmappable => {
                    rec.paths.push(prec);
                    continue;
                }
                _ => {
                    prec.status = PackPathStatus::NotOnVolume;
                    rec.paths.push(prec);
                    continue;
                }
            };
            if pp.sensitive {
                prec.status = PackPathStatus::SensitiveNotCopied;
                sensitive.push(e.text.clone());
            } else if pp.mode == Mode::ListOnly {
                prec.status = PackPathStatus::Listed;
                prec.listed = list_names(&local);
            } else if let Some(first) = claim(seen, &local, format!("{uname}/{}", pack.id)) {
                prec.status = PackPathStatus::Duplicate;
                plan.warnings.push(format!(
                    "{uname}: pack {} path {} is already copied by {first}",
                    pack.id, e.text
                ));
            } else {
                let mut leaf = leaf_name(&e.text);
                if used_leaves.iter().any(|l| l.eq_ignore_ascii_case(&leaf)) {
                    leaf = format!("{leaf}_{i}");
                }
                used_leaves.push(leaf.clone());
                let dest = format!("users/{uname}/packs/{}/{leaf}", pack.id);
                let (include, exclude) = cp.filters[i].clone();
                let filter = (!include.is_empty() || !exclude.is_empty())
                    .then_some(PathFilter { include, exclude });
                jobs.push(FolderJob {
                    user: uname.into(),
                    folder: format!("pack:{}", pack.id),
                    src_root: local.clone(),
                    win_root: e.text.clone(),
                    profile_rel: rel_components(&local, profile_dir),
                    dest_root: dest.clone(),
                    priority: PACK_PRIORITY,
                    filter,
                });
                prec.status = PackPathStatus::Copied;
                prec.dest = Some(dest);
            }
            rec.paths.push(prec);
        }
        if let Some(h) = &ntuser {
            let sections: Vec<String> = pack
                .registry
                .iter()
                .filter_map(|k| hkcu_subpath(k))
                .filter_map(|sub| {
                    crate::regfile::export(h, sub, "HKEY_CURRENT_USER")
                        .ok()
                        .flatten()
                })
                .collect();
            if !sections.is_empty() {
                let dest = format!("users/{uname}/registry/{}.reg", pack.id);
                plan.generated.push(GeneratedFile {
                    dest: dest.clone(),
                    bytes: crate::regfile::to_reg_file(&sections),
                    source: format!(r"HKCU of {uname} ({})", profile.profile_path),
                });
                rec.registry_file = Some(dest);
            }
        }
        // Installed machine-wide but nothing of this user's: not their app.
        let has_data = rec.registry_file.is_some()
            || rec.paths.iter().any(|p| {
                !matches!(
                    p.status,
                    PackPathStatus::NotFound | PackPathStatus::NotOnVolume
                )
            });
        if has_data {
            plan.packs.push(rec);
        }
    }
    if !sensitive.is_empty() {
        plan.manual_steps.push(ManualStep {
            title: format!("Sensitive files of {uname} were not copied"),
            detail: format!(
                "{} hold keys or credentials. They are only ever stored encrypted, and encrypted \
                 storage is not available in this version yet. Copy them yourself to a safe place.",
                sensitive.join(", ")
            ),
        });
    }
    Ok(())
}

fn list_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.retain(|n| !n.starts_with('.'));
    v.sort_by_key(|n| n.to_lowercase());
    v
}
