//! Phases 2–10: carry out a [`RestorePlan`]. A dry run goes through the
//! same steps (including the conflict checks) and stops before each change.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::conflict::{Category, Policies, Policy};
use super::files::restore_file;
use super::host::{RestoreHost, SysOk, SysOp};
use super::map::FileAction;
use super::phases::{explorer, packs_phase, registry_phase, system_phase};
use super::plan::RestorePlan;
use super::system::SysAction;
use super::undo::{Journal, UNDO_DIR, UndoEntry};
use crate::backup::copy::hash_file;
use crate::backup::manifest::ManualStep;
use crate::backup::time::{rfc3339, stamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Apps,
    Files,
    Packs,
    Registry,
    System,
    Explorer,
    Verify,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Apps => "Apps (winget import)",
            Phase::Files => "Files",
            Phase::Packs => "App settings",
            Phase::Registry => "Registry",
            Phase::System => "System settings",
            Phase::Explorer => "Explorer restart",
            Phase::Verify => "Verify",
        }
    }
}

pub enum Event<'a> {
    Phase(Phase),
    Progress { done: usize, total: usize },
    Note(&'a str),
}

/// The user's side of a restore (the CLI prompts; tests answer directly).
pub trait Interact {
    /// A file exists and the policy is `ask`: the choice (never `Ask`) and
    /// whether to use it for the rest of this category.
    fn conflict(&mut self, f: &FileAction, existing: &Path) -> (Policy, bool);
    /// Phase 3, after `winget import`: `false` stops the restore here.
    fn apps_done(&mut self) -> bool;
    fn event(&mut self, e: Event<'_>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Done,
    Skipped,
    Failed,
    /// Dry run: what would happen.
    Would,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub phase: Phase,
    pub what: String,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Default, Serialize)]
pub struct RestoreResult {
    pub dry_run: bool,
    pub undo_dir: Option<PathBuf>,
    pub outcomes: Vec<Outcome>,
    pub verified: usize,
    pub verify_failed: Vec<String>,
    /// Why the run stopped early, if it did.
    pub stopped: Option<String>,
    pub manual_steps: Vec<ManualStep>,
}

impl RestoreResult {
    pub fn count(&self, s: Status) -> usize {
        self.outcomes.iter().filter(|o| o.status == s).count()
    }
    pub fn failed(&self) -> usize {
        self.count(Status::Failed) + self.verify_failed.len()
    }
}

pub(super) struct Exec<'a> {
    pub plan: &'a RestorePlan,
    pub host: &'a dyn RestoreHost,
    pub ui: &'a mut dyn Interact,
    pub dry_run: bool,
    pub policies: Policies,
    journal: Option<Journal>,
    undo_dir: PathBuf,
    pub res: RestoreResult,
    /// (local path, Windows path, expected SHA-256) of files written.
    pub written: Vec<(PathBuf, String, String)>,
    /// A `windows-shell` pack file was written (Explorer restart needed).
    pub shell_changed: bool,
}

impl Exec<'_> {
    /// The journal, created on the first change.
    pub fn journal(&mut self) -> std::io::Result<&mut Journal> {
        if self.journal.is_none() {
            let p = self.plan;
            let header = UndoEntry::Header {
                job_id: p.job_id.clone(),
                backup: p.backup_dir.display().to_string(),
                started: rfc3339(std::time::SystemTime::now()),
                user: p.user.username.clone(),
                profile: p.target.profile.clone(),
            };
            // Two runs in the same second get `-2`, `-3`… (never share a journal).
            let base = self.undo_dir.clone();
            let mut n = 1;
            while self.undo_dir.exists() {
                n += 1;
                self.undo_dir = super::undo::with_suffix(&base, &format!("-{n}"));
            }
            self.journal = Some(Journal::create(&self.undo_dir, header)?);
            self.res.undo_dir = Some(self.undo_dir.clone());
        }
        Ok(self.journal.as_mut().expect("just set"))
    }

    pub fn push(
        &mut self,
        phase: Phase,
        what: impl Into<String>,
        status: Status,
        detail: impl Into<String>,
    ) {
        self.res.outcomes.push(Outcome {
            phase,
            what: what.into(),
            status,
            detail: detail.into(),
        });
    }

    /// Run a system step unless this is a dry run or simulation.
    pub fn sys(&mut self, phase: Phase, what: &str, op: SysOp) -> Option<SysOk> {
        if self.dry_run {
            self.push(phase, what, Status::Would, op.describe());
            return None;
        }
        if self.host.simulated() {
            self.push(phase, what, Status::Skipped, "simulation: not run");
            return None;
        }
        match self.host.sys(&op) {
            Ok(ok) => Some(ok),
            Err(e) => {
                self.push(phase, what, Status::Failed, e);
                None
            }
        }
    }

    /// A query that must run even in a dry run (it changes nothing).
    pub fn query(&mut self, op: SysOp) -> Option<SysOk> {
        if self.host.simulated() {
            None
        } else {
            self.host.sys(&op).ok()
        }
    }
}

pub fn execute(
    plan: &RestorePlan,
    host: &dyn RestoreHost,
    ui: &mut dyn Interact,
    dry_run: bool,
) -> RestoreResult {
    let t = &plan.target;
    let safe: String = plan
        .job_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let undo_dir = t.local(&format!(
        r"{}\{UNDO_DIR}\{safe}_{}",
        t.profile,
        stamp(std::time::SystemTime::now())
    ));
    let mut ex = Exec {
        plan,
        host,
        ui,
        dry_run,
        policies: plan.policies,
        journal: None,
        undo_dir,
        res: RestoreResult {
            dry_run,
            ..Default::default()
        },
        written: Vec::new(),
        shell_changed: false,
    };
    if !apps(&mut ex) {
        return finish(ex);
    }
    files_phase(
        &mut ex,
        Phase::Files,
        &[Category::Folders, Category::Custom],
    );
    packs_phase(&mut ex);
    registry_phase(&mut ex);
    system_phase(&mut ex);
    explorer(&mut ex);
    verify(&mut ex);
    finish(ex)
}

fn finish(mut ex: Exec<'_>) -> RestoreResult {
    ex.res.manual_steps = ex.plan.manual_steps.clone();
    if let Some(dir) = &ex.res.undo_dir {
        let text = super::report::result_text(ex.plan, &ex.res);
        let _ = std::fs::write(dir.join("report.txt"), text);
    }
    ex.res
}

/// Phases 2–3. `false`: stop here.
fn apps(ex: &mut Exec<'_>) -> bool {
    if let Some(SysAction::Winget { file }) = &ex.plan.apps {
        ex.ui.event(Event::Phase(Phase::Apps));
        let what = "winget import";
        if ex
            .sys(Phase::Apps, what, SysOp::WingetImport(file.clone()))
            .is_some()
        {
            ex.push(Phase::Apps, what, Status::Done, "apps installed");
        }
        if !ex.plan.apps_only && !ex.dry_run && !ex.ui.apps_done() {
            ex.res.stopped =
                Some("stopped after the apps step; run the restore again to continue".into());
            return false;
        }
    }
    if ex.plan.apps_only {
        ex.res.stopped = Some("stopped after the apps step (--apps only)".into());
        return false;
    }
    true
}

fn files_phase(ex: &mut Exec<'_>, phase: Phase, cats: &[Category]) {
    let todo: Vec<&FileAction> = ex
        .plan
        .files
        .iter()
        .filter(|f| cats.contains(&f.category))
        .collect();
    if todo.is_empty() {
        return;
    }
    ex.ui.event(Event::Phase(phase));
    for (i, f) in todo.iter().enumerate() {
        restore_file(ex, phase, f);
        ex.ui.event(Event::Progress {
            done: i + 1,
            total: todo.len(),
        });
    }
}

/// Phase 9: read every written file back and compare hashes.
fn verify(ex: &mut Exec<'_>) {
    if ex.written.is_empty() {
        return;
    }
    ex.ui.event(Event::Phase(Phase::Verify));
    for (local, dst, sha) in std::mem::take(&mut ex.written) {
        match hash_file(&local) {
            Ok(h) if h == sha => ex.res.verified += 1,
            Ok(_) => ex
                .res
                .verify_failed
                .push(format!("{dst}: contents differ from the backup")),
            Err(e) => ex.res.verify_failed.push(format!("{dst}: {e}")),
        }
    }
}
