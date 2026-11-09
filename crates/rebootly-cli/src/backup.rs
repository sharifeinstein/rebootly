//! `rebootly backup`: Tier 1 folders + raw hives to a target folder.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use rebootly_core::backup::exclude::{CloudFiles, ExcludeOptions};
use rebootly_core::backup::preflight::{Preflight, gib};
use rebootly_core::backup::{self, Event, Job, PlanOptions, RunOptions};

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum CloudArg {
    Skip,
    Hydrate,
}

#[derive(clap::Args)]
pub struct BackupArgs {
    /// Volume root containing `Windows\` (or the Windows directory itself).
    #[arg(
        long,
        value_name = "PATH",
        required_unless_present = "live",
        conflicts_with = "live"
    )]
    pub windows_root: Option<PathBuf>,
    /// Back up the running Windows (needs an elevated prompt). Loaded hives
    /// are saved from the live registry; the PC is kept awake.
    #[arg(long)]
    pub live: bool,
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
    /// Folder with extra or updated pack YAML files (default: `packs/` beside
    /// the binary). Built-in packs are always available.
    #[arg(long, value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
    /// Only these packs (by id). Repeatable.
    #[arg(long = "pack", value_name = "ID", conflicts_with = "no_packs")]
    pub pack_ids: Vec<String>,
    /// Don't back up app settings (packs).
    #[arg(long)]
    pub no_packs: bool,
    /// Include C:\Users\Public.
    #[arg(long)]
    pub public: bool,
    /// Include a folder at the drive root, e.g. `--root-folder Projects`.
    #[arg(long = "root-folder", value_name = "NAME")]
    pub root_folders: Vec<String>,
    /// Extra folder by Windows path: `--custom "Label=C:\Data"`. Repeatable.
    #[arg(long = "custom", value_name = "LABEL=PATH", value_parser = parse_custom)]
    pub custom: Vec<(String, String)>,
    /// Parallel copy workers.
    #[arg(long, default_value_t = 4)]
    pub jobs: usize,
    /// Store keys and credentials found by packs (SSH keys, cloud CLI
    /// credentials, …) in the encrypted `sensitive.age`. Asks for a
    /// passphrase (or reads `--passphrase-file` / $REBOOTLY_PASSPHRASE).
    #[arg(long)]
    pub sensitive: bool,
    /// Passphrase for `--sensitive`, read from the first line of this file.
    #[arg(long, value_name = "FILE", requires = "sensitive")]
    pub passphrase_file: Option<PathBuf>,
    /// Leave out system items (hosts, scheduled tasks, Wi-Fi, apps.json).
    #[arg(long)]
    pub no_system: bool,
    /// Online-only OneDrive files: `skip` them (they stay in the cloud), or
    /// `hydrate`: download and back them up (live only; fills this PC's disk).
    #[arg(long, value_enum, default_value = "skip")]
    pub cloud_files: CloudArg,
    /// Keep the "downloaded from the internet" mark (Zone.Identifier stream).
    #[arg(long)]
    pub keep_zone_identifier: bool,
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
    if matches!(a.cloud_files, CloudArg::Hydrate) && !a.live {
        bail!(
            "--cloud-files hydrate works only with --live (offline, the files can't be downloaded)"
        );
    }
    let src = crate::source::open(a.live, a.windows_root.as_ref())?;
    let src = src.as_ref();
    let packs = if a.no_packs {
        Vec::new()
    } else {
        crate::pack::load(a.packs_dir.as_deref(), &a.pack_ids)?
    };
    let opts = PlanOptions {
        users: a.users,
        folders: a.folders,
        exclude: ExcludeOptions {
            dev_artifacts: a.skip_dev_artifacts,
            cloud_files: match a.cloud_files {
                CloudArg::Skip => CloudFiles::Skip,
                CloudArg::Hydrate => CloudFiles::Hydrate,
            },
        },
        packs,
        public: a.public,
        root_folders: a.root_folders,
        custom: a.custom,
        sensitive_bundle: a.sensitive,
        no_system: a.no_system,
    };
    let plan = backup::plan(src, &opts)?;
    print_detected(&plan);
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
    let pre = backup::preflight(&plan, &probe, resumed.as_ref(), target_fs(&probe));
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

    let passphrase = (a.sensitive && !plan.sensitive.is_empty())
        .then(|| crate::sensitive::passphrase(a.passphrase_file.as_deref(), true))
        .transpose()?;
    let job = match resumed {
        Some(j) => j,
        None => Job::create(target, plan.source.machine.as_deref())
            .context("creating the backup folder")?,
    };
    eprintln!("Backing up to {}", job.dir.display());
    let _awake = crate::source::Awake::new(a.live);
    let ro = RunOptions {
        jobs: a.jobs,
        passphrase,
        keep_zone_identifier: a.keep_zone_identifier,
        ..RunOptions::default()
    };
    let (mut done, mut total, mut failed, mut last) = (0usize, 0usize, 0usize, Instant::now());
    let manifest = backup::run(src, &plan, &job, &ro, &mut |e| match e {
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
        Event::Sensitive { files } => {
            eprintln!("\nEncrypting {files} sensitive file(s) into sensitive.age…");
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

fn print_detected(plan: &backup::Plan) {
    let mut by_user: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for p in &plan.packs {
        by_user.entry(&p.user).or_default().push(&p.id);
    }
    if !by_user.is_empty() {
        eprintln!("App packs:");
        for (u, ids) in by_user {
            eprintln!("  {u:<20} {}", ids.join(", "));
        }
    }
    if !plan.sensitive.is_empty() {
        eprintln!(
            "Sensitive: {} file(s) (keys, credentials) go into the encrypted sensitive.age",
            plan.sensitive.len()
        );
    } else if !plan.sensitive_jobs.is_empty() {
        eprintln!(
            "Sensitive: {} key/credential location(s) found and left out; add --sensitive to \
             store them encrypted",
            plan.sensitive_jobs.len()
        );
    }
    let unused: Vec<&str> = plan
        .offered_root_folders
        .iter()
        .filter(|n| !plan.custom.iter().any(|c| c.label.eq_ignore_ascii_case(n)))
        .map(String::as_str)
        .collect();
    if !unused.is_empty() {
        eprintln!(
            "Folders at the drive root (add with --root-folder NAME): {}",
            unused.join(", ")
        );
    }
}

fn parse_custom(s: &str) -> Result<(String, String), String> {
    let (label, path) = s.split_once('=').ok_or("expected LABEL=PATH")?;
    if label.trim().is_empty() || path.trim().is_empty() {
        return Err("expected LABEL=PATH".into());
    }
    Ok((label.trim().to_owned(), path.trim().to_owned()))
}

fn nearest_existing(p: &Path) -> PathBuf {
    p.ancestors()
        .find(|a| a.exists())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Target filesystem type from the platform layer (Windows); `None` lets
/// the core look it up (Linux).
fn target_fs(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        rebootly_win::filesystem_type(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}
