//! After a restore: is each file of the backup on this PC? Uses the same
//! file plan as the restore (mapping, packs, `--map-drive`), then looks at
//! each destination the way the restore decides (identical, rewritten
//! profile paths, keep-both copies, merges, shortcuts, files it leaves out).
//! Settings (registry, Wi-Fi, power plan, certificates, apps) aren't compared.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::Progress;
use crate::backup::copy::{hash_file, hex};
use crate::backup::time::parse_rfc3339;
use crate::restore::RestorePlan;
use crate::restore::conflict::keep_both_name;
use crate::restore::fit::{is_shortcut, unfit};
use crate::restore::map::FileAction;
use crate::restore::rewrite::{MAX_REWRITE, rewrite_bytes};

/// Where one backed-up file stands on this PC. Ordered roughly from
/// "fine" to "needs a look".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The backup's content.
    Same,
    /// The backup's content with the old profile path rewritten.
    Rewritten,
    /// A different file has the name; the backup's is beside it as
    /// `name (migrated).ext`.
    Beside,
    /// Merged into this PC's file (hosts, VPN phonebook): there, not compared.
    Merged,
    /// This PC's own shortcut or `desktop.ini` is kept, by design.
    KeptOwn,
    /// The restore leaves it out by design (see the detail).
    LeftOut,
    /// Different, and changed on this PC after the backup was made.
    Newer,
    /// Different, not newer than the backup.
    Different,
    Missing,
    /// A folder or a link is where the file should be.
    Blocked,
    Unreadable,
}

impl State {
    /// States that mean the PC doesn't have the backup's file.
    pub fn is_problem(self) -> bool {
        matches!(
            self,
            State::Different | State::Missing | State::Blocked | State::Unreadable
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// Destination on this PC (Windows form).
    pub path: String,
    /// Backup-relative path.
    pub from: String,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareReport {
    pub backup: String,
    pub job_id: String,
    /// Backup user compared, and the account on this PC.
    pub user: String,
    pub target_user: String,
    pub target_profile: String,
    pub bytes: u64,
    pub entries: Vec<Entry>,
    /// Left out of the plan already (what, reason).
    pub not_planned: Vec<(String, String)>,
    /// Other backup users and their file counts (not compared in this run).
    pub other_users: Vec<(String, usize)>,
}

impl CompareReport {
    pub fn count(&self, s: State) -> usize {
        self.entries.iter().filter(|e| e.state == s).count()
    }

    pub fn is_good(&self) -> bool {
        !self.entries.iter().any(|e| e.state.is_problem())
    }
}

/// FAT/exFAT keep times to 2 s; anything closer counts as the same time.
const SLACK: Duration = Duration::from_secs(2);

pub fn compare(plan: &RestorePlan, progress: &mut dyn FnMut(&Progress)) -> CompareReport {
    let mut prog = Progress {
        total_files: plan.files.len(),
        total_bytes: plan.bytes,
        ..Progress::default()
    };
    let mut entries = Vec::with_capacity(plan.files.len());
    for f in &plan.files {
        let (state, detail) = check_one(plan, f);
        entries.push(Entry {
            path: f.dst.clone(),
            from: f.from.clone(),
            state,
            detail,
        });
        prog.step(f.size, progress);
    }
    CompareReport {
        backup: plan.backup_dir.display().to_string(),
        job_id: plan.job_id.clone(),
        user: plan.user.username.clone(),
        target_user: plan.target.username.clone(),
        target_profile: plan.target.profile.clone(),
        bytes: plan.bytes,
        entries,
        not_planned: plan
            .skipped
            .iter()
            .map(|s| (s.what.clone(), s.reason.clone()))
            .collect(),
        other_users: plan
            .other_users
            .iter()
            .map(|(u, n)| (u.clone(), *n))
            .collect(),
    }
}

/// What the restore writes for this file: (SHA-256, size, rewritten).
fn expected(plan: &RestorePlan, f: &FileAction) -> Result<(Option<String>, u64, bool), String> {
    if !f.rewrite || f.size > MAX_REWRITE {
        return Ok((f.sha256.clone(), f.size, false));
    }
    let bytes = std::fs::read(&f.src).map_err(|e| format!("can't read the backup copy: {e}"))?;
    let new = rewrite_bytes(&bytes, &plan.user.profile_path, &plan.target.profile);
    Ok(match new {
        Some((b, _)) => (Some(hex(&Sha256::digest(&b))), b.len() as u64, true),
        None => (f.sha256.clone(), f.size, false),
    })
}

fn holds(local: &Path, size: u64, sha: Option<&str>) -> Result<bool, String> {
    let meta = std::fs::metadata(local).map_err(|e| e.to_string())?;
    if meta.len() != size {
        return Ok(false);
    }
    match sha {
        Some(s) => hash_file(local)
            .map(|h| h.eq_ignore_ascii_case(s))
            .map_err(|e| e.to_string()),
        None => Ok(true), // no hash recorded: the size is all we have
    }
}

fn check_one(plan: &RestorePlan, f: &FileAction) -> (State, Option<String>) {
    let t = &plan.target;
    let (sha, size, rewritten) = match expected(plan, f) {
        Ok(x) => x,
        Err(e) => return (State::Unreadable, Some(e)),
    };
    let local = t.local(&f.dst);
    let meta = std::fs::symlink_metadata(&local).ok();
    if let Some(m) = &meta {
        if m.file_type().is_symlink() || m.is_dir() {
            return (State::Blocked, None);
        }
        match holds(&local, size, sha.as_deref()) {
            Ok(true) if rewritten => return (State::Rewritten, None),
            Ok(true) => return (State::Same, None),
            Ok(false) => {}
            Err(e) => return (State::Unreadable, Some(e)),
        }
    }
    let exists = meta.is_some();
    let name = f.dst.rsplit('\\').next().unwrap_or(&f.dst);
    if exists && (is_shortcut(name) || name.eq_ignore_ascii_case("desktop.ini")) {
        return (State::KeptOwn, None);
    }
    if let Some(why) = unfit(f, exists, rewritten, &plan.user, t) {
        return (State::LeftOut, Some(why));
    }
    let Some(m) = meta else {
        return (State::Missing, None);
    };
    if f.merge.is_some() {
        return (State::Merged, None);
    }
    for n in 1..1000 {
        let cand = keep_both_name(&f.dst, n);
        let l = t.local(&cand);
        if !l.exists() {
            break;
        }
        if holds(&l, size, sha.as_deref()).unwrap_or(false) {
            return (State::Beside, Some(cand));
        }
    }
    let theirs = f.mtime.as_deref().and_then(parse_rfc3339);
    match (theirs, m.modified().ok()) {
        (Some(b), Some(here)) if here > b + SLACK => (State::Newer, None),
        _ => (State::Different, None),
    }
}
