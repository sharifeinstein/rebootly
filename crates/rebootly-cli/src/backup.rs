//! `rebootly backup`: Tier 1 folders + raw hives to a target folder.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use rebootly_core::OfflineVolume;
use rebootly_core::backup::exclude::ExcludeOptions;
use rebootly_core::backup::preflight::{Preflight, gib};
use rebootly_core::backup::{self, Event, Job, PlanOptions, RunOptions};

#[derive(clap::Args)]
pub struct BackupArgs {
    /// Volume root containing `Windows\` (or the Windows directory itself).
    #[arg(long, value_name = "PATH")]
    pub windows_root: PathBuf,
    /// Where to write the backup; a `<MACHINE>_<DATE>` folder is created here.
    #[arg(long, value_name = "DIR", required_unless_present = "resume")]
    pub target: Option<PathBuf>,
    /// Continue an interrupted backup in this job folder.
    #[arg(long, value_name = "JOB_DIR", conflicts_with = "target")]
    pub resume: Option<PathBuf>,
    /// Only these users (username or SID). Repeatable. Default: all.
    #[arg(long = "user", value_name = "NAME")]
    pub users: Vec<String>,
    /// Only these folders (Desktop, Documents, Pictures, ...). Repeatable.
    #[arg(long = "folder", value_name = "NAME")]
    pub folders: Vec<String>,
    /// Also skip node_modules and .git/objects.
    #[arg(long)]
    pub skip_dev_artifacts: bool,
    /// Parallel copy workers.
    #[arg(long, default_value_t = 4)]
    pub jobs: usize,
    /// Show the pre-flight check and stop.
    #[arg(long)]
    pub preflight: bool,
    /// Run despite pre-flight blockers (never overrides the source-disk check).
    #[arg(long)]
    pub force: bool,
}

fn print_preflight(p: &Preflight) {
    eprintln!("Pre-flight:");
    for f in &p.folders {
        eprintln!(
            "  {:<22} {:>7} files  {:>12}",
            f.folder,
            f.files,
            gib(f.bytes)
        );
    }
    eprintln!(
        "  {:<22} {:>7} files  {:>12}",
        "Total",
        p.total_files,
        gib(p.total_bytes)
    );
    if p.resumed_bytes > 0 {
        eprintln!("  already on target:     {}", gib(p.resumed_bytes));
    }
    let free = p.target_free.map_or("unknown".into(), gib);
    eprintln!(
        "  target: {} free, filesystem {}",
        free,
        p.target_fs.as_deref().unwrap_or("unknown")
    );
    for w in &p.warnings {
        eprintln!("  ! {w}");
    }
    for b in &p.blockers {
        eprintln!("  X {b}");
    }
}

/// Returns the process exit code: 0 all copied, 2 finished with failures.
pub fn run(a: BackupArgs) -> anyhow::Result<u8> {
    let src = OfflineVolume::open(&a.windows_root)?;
    let opts = PlanOptions {
        users: a.users,
        folders: a.folders,
        exclude: ExcludeOptions {
            dev_artifacts: a.skip_dev_artifacts,
        },
    };
    let plan = backup::plan(&src, &opts)?;
    if plan.users.is_empty() {
        bail!("no matching users to back up");
    }
    let resumed = a.resume.as_deref().map(Job::resume).transpose()?;
    let target: &Path = match (&resumed, &a.target) {
        (Some(j), _) => j.dir.parent().unwrap_or(&j.dir),
        (None, Some(t)) => t,
        (None, None) => unreachable!("clap requires --target or --resume"),
    };
    // Free-space probing needs an existing directory.
    let probe = if target.exists() {
        target.to_path_buf()
    } else {
        nearest_existing(target)
    };
    let pre = backup::preflight(&plan, &probe, resumed.as_ref());
    print_preflight(&pre);
    if a.preflight {
        return Ok(0);
    }
    let source_blocker = pre.blockers.iter().any(|b| b.contains("source disk"));
    if !pre.blockers.is_empty() && (!a.force || source_blocker) {
        bail!(
            "pre-flight failed (see above){}",
            if source_blocker {
                ""
            } else {
                "; use --force to run anyway"
            }
        );
    }

    let job = match resumed {
        Some(j) => j,
        None => Job::create(target, plan.source.machine.as_deref())
            .context("creating the backup folder")?,
    };
    eprintln!("Backing up to {}", job.dir.display());
    let ro = RunOptions {
        jobs: a.jobs,
        ..RunOptions::default()
    };
    let (mut done, mut total, mut failed, mut last) = (0usize, 0usize, 0usize, Instant::now());
    let manifest = backup::run(&src, &plan, &job, &ro, &mut |e| match e {
        Event::Start {
            files,
            already_done,
            ..
        } => {
            total = files;
            if already_done > 0 {
                eprintln!("Resuming: {already_done} file(s) already copied");
            }
        }
        Event::File(f) => {
            done += 1;
            if f.status == rebootly_core::backup::manifest::FileStatus::Failed {
                failed += 1;
                eprintln!(
                    "\r  failed: {} ({})",
                    f.source,
                    f.reason.as_deref().unwrap_or("?")
                );
            }
            if last.elapsed() > Duration::from_millis(500) || done == total {
                eprint!("\r  {done}/{total} files, {failed} failed   ");
                let _ = std::io::stderr().flush();
                last = Instant::now();
            }
        }
    })
    .with_context(|| format!("backup stopped; resume with --resume {}", job.dir.display()))?;
    eprintln!();
    let (ok, _, failed) = manifest.counts();
    println!("{}", job.dir.display());
    eprintln!(
        "{ok} copied, {failed} failed. Report: {}",
        job.dir.join("report.txt").display()
    );
    if !manifest.manual_steps.is_empty() {
        eprintln!(
            "{} manual step(s) listed in the report.",
            manifest.manual_steps.len()
        );
    }
    Ok(if failed > 0 { 2 } else { 0 })
}

fn nearest_existing(p: &Path) -> PathBuf {
    p.ancestors()
        .find(|a| a.exists())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}
