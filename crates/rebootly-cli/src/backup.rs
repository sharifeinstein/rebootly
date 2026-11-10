//! `rebootly backup`: Tier 1 folders + raw hives to a target folder.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use rebootly_core::backup::exclude::CloudFiles;
use rebootly_core::backup::profile::{self, JobProfile, Mode, Selection};
use rebootly_core::backup::{self, Event, Job, RunOptions};

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
        required_unless_present_any = ["live", "save_profile"],
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
    /// (Implies --sensitive.)
    #[arg(long, value_name = "FILE")]
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
    /// Everything detected, like the TUI's Full mode: also Public, every
    /// drive-root folder, and keys/credentials (implies --sensitive).
    #[arg(long, conflicts_with_all = ["profile", "folders", "pack_ids", "no_packs", "no_system"])]
    pub full: bool,
    /// Use the choices of a saved job profile (a TUI "save as job profile",
    /// or --save-profile): users, folders, packs, extras.
    #[arg(long, value_name = "FILE", conflicts_with_all = [
        "users", "folders", "pack_ids", "no_packs", "public", "root_folders", "custom",
        "no_system", "skip_dev_artifacts",
    ])]
    pub profile: Option<PathBuf>,
    /// Save these choices as a job profile in the target's
    /// `rebootly-profiles/` folder, and stop (nothing is copied).
    #[arg(
        long,
        value_name = "NAME",
        requires = "target",
        conflicts_with = "profile"
    )]
    pub save_profile: Option<String>,
}

fn selection(a: &BackupArgs) -> anyhow::Result<Selection> {
    if let Some(p) = &a.profile {
        let job = profile::load(p).with_context(|| format!("reading {}", p.display()))?;
        eprintln!("Job profile: {}", job.name);
        return Ok(job.selection);
    }
    let mut sel = Selection::new(if a.full { Mode::Full } else { Mode::Quick });
    sel.users = a.users.clone();
    sel.folders = a.folders.clone();
    sel.packs = (a.no_packs || !a.pack_ids.is_empty()).then(|| a.pack_ids.clone());
    sel.public |= a.public;
    sel.root_folders = a.root_folders.clone();
    sel.custom = a.custom.clone();
    sel.sensitive |= a.sensitive || a.passphrase_file.is_some();
    sel.system = !a.no_system;
    sel.skip_dev_artifacts = a.skip_dev_artifacts;
    if sel != Selection::new(sel.mode) {
        sel.mode = Mode::Custom;
    }
    Ok(sel)
}

/// Returns the process exit code: 0 all copied, 2 finished with failures.
pub fn run(a: BackupArgs) -> anyhow::Result<u8> {
    eprintln!("Rebootly {}", crate::VERSION);
    if matches!(a.cloud_files, CloudArg::Hydrate) && !a.live {
        bail!(
            "--cloud-files hydrate works only with --live (offline, the files can't be downloaded)"
        );
    }
    let mut sel = selection(&a)?;
    if let Some(name) = &a.save_profile {
        let job = JobProfile {
            schema: profile::PROFILE_SCHEMA,
            name: name.clone(),
            selection: sel,
            jobs: (a.jobs != 4).then_some(a.jobs),
        };
        let target = a.target.as_deref().expect("clap: requires target");
        let path = profile::save(target, &job).context("saving the job profile")?;
        println!("{}", path.display());
        eprintln!(
            "Saved; run it with: rebootly backup --profile \"{}\" --target …",
            path.display()
        );
        return Ok(0);
    }
    let src = crate::source::open(a.live, a.windows_root.as_ref())?;
    let src = src.as_ref();
    // Validates --pack ids; the selection then filters by id.
    let packs = crate::pack::load(a.packs_dir.as_deref(), &a.pack_ids)?;
    sel.resolve(&backup::custom::root_folders(src.source_root()));
    let mut opts = sel.plan_options(&packs);
    opts.exclude.cloud_files = match a.cloud_files {
        CloudArg::Skip => CloudFiles::Skip,
        CloudArg::Hydrate => CloudFiles::Hydrate,
    };
    let sensitive = sel.sensitive;
    let plan = backup::plan(src, &opts)?;
    crate::backup_text::print_detected(&plan);
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
    crate::backup_text::print_preflight(&pre);
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

    let passphrase = (sensitive && !plan.sensitive.is_empty())
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
        Event::Tick => {}
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
