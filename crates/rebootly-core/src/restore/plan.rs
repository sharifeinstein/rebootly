//! Phase 1: everything the restore will do, worked out before anything is
//! written (the dry run prints this plus the per-file decisions).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::RestoreError;
use super::conflict::Policies;
use super::input::{Backup, Bundle};
use super::map::{FileAction, MapCtx, Skip, map_files};
use super::notes::pack_steps;
use super::regplan::registry;
use super::system::{SysAction, name_list, plan_system};
use super::target::Target;
use crate::backup::manifest::{FileStatus, ManualStep, PackRecord, UserRecord};
use crate::packs::schema::CompiledPack;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppsMode {
    /// Run `winget import` first, then restore the rest.
    Install,
    Skip,
    /// Only `winget import`, then stop.
    Only,
}

#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Backup user to restore (into the user running the restore).
    pub user: Option<String>,
    pub policies: Policies,
    /// `--map-drive D=E`: custom folders from old D: go to E:.
    pub drive_map: Vec<(char, char)>,
    pub apps: AppsMode,
    pub packs: bool,
    pub registry: bool,
    pub system: bool,
    /// Re-create scheduled tasks (opt-in).
    pub tasks: bool,
    pub restart_explorer: bool,
    /// Restore an incomplete backup / with too little free space.
    pub force: bool,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        RestoreOptions {
            user: None,
            policies: Policies::default(),
            drive_map: Vec::new(),
            apps: AppsMode::Install,
            packs: true,
            registry: true,
            system: true,
            tasks: false,
            restart_explorer: true,
            force: false,
        }
    }
}

/// A checked `.reg` file, ready to import.
#[derive(Debug, Clone)]
pub struct RegAction {
    pub pack: String,
    pub from: String,
    /// Validated text with the profile path rewritten.
    pub text: String,
    pub keys: Vec<String>,
    /// Keys exported before the import so undo can put them back.
    pub top: Vec<String>,
    pub review: Vec<String>,
    pub rewrites: usize,
}

#[derive(Debug, Clone)]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub requires_closed: Vec<String>,
    pub files: usize,
}

#[derive(Debug)]
pub struct RestorePlan {
    pub job_id: String,
    pub backup_dir: PathBuf,
    pub source_machine: Option<String>,
    pub user: UserRecord,
    pub target: Target,
    pub policies: Policies,
    pub files: Vec<FileAction>,
    pub registry: Vec<RegAction>,
    pub apps: Option<SysAction>,
    pub apps_only: bool,
    pub system: Vec<SysAction>,
    pub packs: Vec<PackInfo>,
    pub skipped: Vec<Skip>,
    pub other_users: BTreeMap<String, usize>,
    /// Known folder → (old path, new path) for included folders.
    pub folder_map: Vec<(String, String, String)>,
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    pub manual_steps: Vec<ManualStep>,
    pub restart_explorer: bool,
}

/// Which backup user this run restores.
pub fn choose_user(
    backup: &Backup,
    target: &Target,
    wanted: Option<&str>,
) -> Result<UserRecord, RestoreError> {
    let users = &backup.manifest.users;
    let names = || {
        users
            .iter()
            .map(|u| u.username.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Some(w) = wanted {
        return users
            .iter()
            .find(|u| u.username.eq_ignore_ascii_case(w))
            .cloned()
            .ok_or_else(|| {
                RestoreError::User(format!("no user {w} in this backup (it has: {})", names()))
            });
    }
    let with_files: Vec<&UserRecord> = users
        .iter()
        .filter(|u| {
            let pre = format!("users/{}/", u.username);
            backup
                .manifest
                .files
                .iter()
                .any(|f| f.status == FileStatus::Ok && f.path.starts_with(&pre))
        })
        .collect();
    match with_files.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(RestoreError::User(
            "this backup has no user files to restore".into(),
        )),
        many => many
            .iter()
            .find(|u| u.username.eq_ignore_ascii_case(&target.username))
            .map(|u| (*u).clone())
            .ok_or_else(|| {
                RestoreError::User(format!(
                    "this backup has several users ({}); choose one with --user NAME \
                     (it is restored into {}, the user running Rebootly)",
                    names(),
                    target.username
                ))
            }),
    }
}

pub fn plan(
    backup: &Backup,
    target: &Target,
    packs: &[CompiledPack],
    opts: &RestoreOptions,
    bundle: Option<&Bundle>,
) -> Result<RestorePlan, RestoreError> {
    let m = &backup.manifest;
    let user = choose_user(backup, target, opts.user.as_deref())?;
    let ctx = MapCtx {
        backup,
        target,
        user: &user,
        packs,
        opts,
    };
    let mapped = map_files(&ctx, bundle);
    let sys = plan_system(backup, bundle, opts, &user.sid);
    let mut warnings = Vec::new();
    let mut blockers = Vec::new();
    let mut skipped = mapped.skipped;
    let registry = if opts.registry {
        registry(&ctx, &mut skipped)
    } else {
        Vec::new()
    };
    let my_packs: Vec<&PackRecord> = m.packs.iter().filter(|p| p.user == user.username).collect();
    let pack_info: Vec<PackInfo> = my_packs
        .iter()
        .map(|p| PackInfo {
            id: p.id.clone(),
            name: p.name.clone(),
            requires_closed: p.requires_closed.clone(),
            files: mapped
                .files
                .iter()
                .filter(|f| f.pack.as_deref() == Some(&p.id))
                .count(),
        })
        .filter(|p| p.files > 0)
        .collect();
    let bytes = mapped.files.iter().map(|f| f.size).sum();
    if !m.complete {
        let msg = "the backup is incomplete (it was interrupted); finish it with `rebootly backup --resume` first";
        if opts.force {
            warnings.push(msg.into())
        } else {
            blockers.push(format!("{msg}, or use --force"))
        }
    }
    if let Some(free) = free_space(&target.local(&target.profile)) {
        if bytes > free {
            let msg = format!(
                "needs {} but {} has only {} free",
                crate::backup::preflight::gib(bytes),
                target.profile,
                crate::backup::preflight::gib(free)
            );
            if opts.force {
                warnings.push(msg)
            } else {
                blockers.push(format!("{msg} (--force to try anyway)"))
            }
        }
    }
    let mut manual = sys.manual;
    for p in &pack_info {
        let rec = my_packs
            .iter()
            .find(|r| r.id == p.id)
            .expect("from my_packs");
        manual.extend(pack_steps(rec));
    }
    if !mapped.other_users.is_empty() {
        let names: Vec<String> = mapped.other_users.keys().cloned().collect();
        manual.push(ManualStep {
            title: "Other users in this backup".into(),
            detail: format!(
                "Not restored in this run: {}. Sign in as each user on this PC and run the restore with --user NAME.",
                name_list(&names, 10)
            ),
        });
    }
    // Backup-time steps about other users are noise here.
    let mentions = |s: &ManualStep, name: &str| {
        let words = |t: &str| {
            t.split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_' || c == '-'))
                .any(|w| w.eq_ignore_ascii_case(name))
        };
        words(&s.title) || words(&s.detail)
    };
    manual.extend(
        m.manual_steps
            .iter()
            .filter(|s| {
                mentions(s, &user.username)
                    || !m
                        .users
                        .iter()
                        .any(|u| u.username != user.username && mentions(s, &u.username))
            })
            .map(|s| ManualStep {
                title: format!("From the backup: {}", s.title.trim_end_matches(':')),
                detail: s.detail.clone(),
            }),
    );
    let folder_map = user
        .folders
        .iter()
        .filter(|(_, f)| f.included)
        .map(|(n, f)| (n.clone(), f.source_path.clone(), target.folder(n)))
        .collect();
    Ok(RestorePlan {
        job_id: m.job_id.clone(),
        backup_dir: backup.dir.clone(),
        source_machine: m.source.machine.clone(),
        target: target.clone(),
        policies: opts.policies,
        files: mapped.files,
        registry,
        apps: sys.apps,
        apps_only: opts.apps == super::AppsMode::Only,
        system: sys.actions,
        packs: pack_info,
        skipped,
        other_users: mapped.other_users,
        folder_map,
        bytes,
        warnings,
        blockers,
        manual_steps: manual,
        restart_explorer: opts.restart_explorer,
        user,
    })
}

fn free_space(path: &Path) -> Option<u64> {
    let mut p = Some(path);
    while let Some(x) = p {
        if x.exists() {
            return fs4::available_space(x).ok();
        }
        p = x.parent();
    }
    None
}
