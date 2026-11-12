//! Backup engine (SPEC §3.5, §3.6, §10): plan → pre-flight → run.
//!
//! Reads the source only through `Source::open_read`; writes only into the
//! job folder on the target.

pub mod copy;
pub mod custom;
pub mod exclude;
pub mod extras;
pub mod failures;
pub mod filecopy;
pub mod flow;
pub mod manifest;
pub mod plan;
pub mod preflight;
pub mod profile;
pub mod report;
pub mod report_html;
pub mod run;
pub mod sensitive;
pub mod sizes;
pub mod state;
pub mod summary;
pub mod system;
pub mod time;
pub mod walk;

pub use flow::Flow;
pub use plan::{Plan, PlanOptions, plan};
pub use run::{Event, Job, RunOptions, run};

/// Pre-flight for a plan against a job (accounts for resumed files).
/// `target_fs`: the target's filesystem type when the platform layer knows
/// it (Windows); otherwise it is looked up here (Linux).
pub fn preflight(
    plan: &Plan,
    job_target: &std::path::Path,
    job: Option<&Job>,
    target_fs: Option<String>,
) -> preflight::Preflight {
    let todo_bytes = match job {
        Some(j) => j.todo(plan).1,
        None => plan.files.iter().map(|f| f.size).sum(),
    };
    let mut p = preflight::check(
        &plan.files,
        todo_bytes,
        &plan.labels,
        &plan.issues,
        job_target,
        target_fs,
        &plan.source_root,
    );
    p.warnings.extend(plan.source_notes.iter().cloned());
    if let Ok(t) = job_target.canonicalize() {
        for (letter, root) in &plan.other_drives {
            if root.canonicalize().is_ok_and(|r| t.starts_with(r)) {
                p.warnings.push(format!(
                    "the target is on {letter}:, a drive of the Windows being backed up; \
                     fine if that disk is healthy, but use another disk to rescue a failing one"
                ));
            }
        }
    }
    let (n, b) = plan.to_download;
    if n > 0 {
        p.warnings.push(format!(
            "{n} online-only OneDrive file(s), {}, will be downloaded during the backup \
             (needs internet and free space on this PC)",
            preflight::gib(b)
        ));
    }
    p
}

#[cfg(test)]
mod tests;
