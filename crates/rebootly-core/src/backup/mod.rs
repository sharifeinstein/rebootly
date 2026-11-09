//! Backup engine (SPEC §3.5, §3.6, §10): plan → pre-flight → run.
//!
//! Reads the source only through `Source::open_read`; writes only into the
//! job folder on the target.

pub mod copy;
pub mod custom;
pub mod exclude;
pub mod extras;
pub mod filecopy;
pub mod manifest;
pub mod plan;
pub mod preflight;
pub mod report;
pub mod run;
pub mod sensitive;
pub mod state;
pub mod system;
pub mod time;
pub mod walk;

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
