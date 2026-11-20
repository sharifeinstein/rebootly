//! `rebootly-cli verify` (re-hash a backup) and `rebootly-cli compare`
//! (check this PC against a backup after a restore). SPEC §11.

use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::bail;
use rebootly_core::backup::preflight::gib;
use rebootly_core::check::{self, Progress, text};
use rebootly_core::restore::plan::choose_user;
use rebootly_core::restore::{self, AppsMode, Backup, Bundle, RestoreOptions, Target};

#[derive(clap::Args)]
pub struct VerifyArgs {
    /// The backup folder (the one with manifest.json).
    #[arg(value_name = "BACKUP")]
    pub backup: PathBuf,
    /// Also open sensitive.age and check it (asks for the passphrase).
    #[arg(long)]
    pub sensitive: bool,
    /// Read the passphrase from this file instead of asking.
    #[arg(long, value_name = "FILE")]
    pub passphrase_file: Option<PathBuf>,
    /// List every problem (default: the first 50 of each kind).
    #[arg(long)]
    pub all: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct CompareArgs {
    /// The backup folder that was restored.
    #[arg(value_name = "BACKUP")]
    pub backup: PathBuf,
    /// Backup user to compare (with the user running Rebootly).
    #[arg(long, value_name = "NAME")]
    pub user: Option<String>,
    /// As given to the restore: custom folders from old D: are on E:.
    #[arg(long, value_name = "OLD=NEW")]
    pub map_drive: Vec<String>,
    /// As given to the restore: app settings (packs) weren't restored.
    #[arg(long)]
    pub no_packs: bool,
    /// As given to the restore: hosts and the VPN phonebook weren't restored.
    #[arg(long)]
    pub no_system: bool,
    /// Also compare the files from sensitive.age (SSH keys and similar).
    #[arg(long)]
    pub sensitive: bool,
    /// Read the passphrase from this file instead of asking.
    #[arg(long, value_name = "FILE")]
    pub passphrase_file: Option<PathBuf>,
    /// Folder with extra or updated pack YAML files.
    #[arg(long, value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
    /// Compare a restore made with --simulate-root DIR.
    #[arg(long, value_name = "DIR")]
    pub simulate_root: Option<PathBuf>,
    /// Profile name for --simulate-root (default: the backup user's).
    #[arg(long, value_name = "NAME", requires = "simulate_root")]
    pub simulate_user: Option<String>,
    /// List every file in each list (default: the first 50).
    #[arg(long)]
    pub all: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

fn limit(all: bool) -> usize {
    if all { usize::MAX } else { text::DEFAULT_LIMIT }
}

pub fn verify(a: VerifyArgs) -> anyhow::Result<u8> {
    eprintln!("Rebootly {}", crate::VERSION);
    let backup = Backup::open(&a.backup)?;
    let pass = if a.sensitive || a.passphrase_file.is_some() {
        if backup.manifest.sensitive.is_none() {
            eprintln!("note: this backup has no sensitive.age");
            None
        } else {
            Some(crate::sensitive::passphrase(
                a.passphrase_file.as_deref(),
                false,
            )?)
        }
    } else {
        None
    };
    let mut bar = Bar::new("Verifying");
    let r = check::verify(&backup, pass.as_ref(), &mut |p| bar.show(p));
    bar.done();
    if a.json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        print!("{}", text::verify_text(&r, limit(a.all)));
    }
    Ok(if r.is_good() { 0 } else { 2 })
}

pub fn compare(a: CompareArgs) -> anyhow::Result<u8> {
    eprintln!("Rebootly {}", crate::VERSION);
    let backup = Backup::open(&a.backup)?;
    let opts = RestoreOptions {
        user: a.user.clone(),
        drive_map: crate::restore::drive_map(&a.map_drive)?,
        apps: AppsMode::Skip,
        packs: !a.no_packs,
        // Hosts and phonebooks are files of the plan; the other system
        // steps and the registry aren't compared.
        registry: false,
        system: !a.no_system,
        restart_explorer: false,
        force: true,
        ..RestoreOptions::default()
    };
    let target = target(&a, &backup, &opts)?;
    let bundle = match (
        a.sensitive || a.passphrase_file.is_some(),
        backup.bundle_path(),
    ) {
        (true, Some(path)) => {
            let pass = crate::sensitive::passphrase(a.passphrase_file.as_deref(), false)?;
            Some(Bundle::open(&path, &pass)?)
        }
        (true, None) => {
            eprintln!("note: this backup has no sensitive.age");
            None
        }
        (false, _) => None,
    };
    let (packs, errors) = rebootly_core::packs::load::load_all(
        a.packs_dir
            .as_deref()
            .or(rebootly_core::packs::load::default_dir().as_deref()),
    );
    for e in errors {
        eprintln!("warning: {e}");
    }
    let plan = restore::plan(&backup, &target, &packs, &opts, bundle.as_ref())?;
    let mut bar = Bar::new("Comparing");
    let r = check::compare(&plan, &mut |p| bar.show(p));
    bar.done();
    drop(bundle);
    if a.json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        print!("{}", text::compare_text(&r, limit(a.all)));
    }
    Ok(if r.is_good() { 0 } else { 2 })
}

/// This PC as the restore saw it: simulated, or the running Windows.
fn target(a: &CompareArgs, backup: &Backup, opts: &RestoreOptions) -> anyhow::Result<Target> {
    if let Some(root) = &a.simulate_root {
        let name = match &a.simulate_user {
            Some(n) => n.clone(),
            None => {
                let probe = Target::simulated(Path::new("."), "User");
                choose_user(backup, &probe, opts.user.as_deref())?.username
            }
        };
        return Ok(Target::simulated(root, &name));
    }
    #[cfg(windows)]
    {
        if rebootly_core::env::Environment::detect() != rebootly_core::env::Environment::LiveWindows
        {
            bail!("compare checks the new PC's own Windows, not WinPE or a recovery system");
        }
        Ok(rebootly_win::live_target()?)
    }
    #[cfg(not(windows))]
    {
        bail!(
            "compare checks the new PC's Windows; for a simulated restore add --simulate-root DIR"
        )
    }
}

/// One progress line on stderr, redrawn at most 4 times a second; only on
/// a terminal. It ends with a final "done" line and a line break instead of
/// erasing itself: on the owner's Windows console the erased line's spaces
/// stayed in front of the report.
struct Bar {
    what: &'static str,
    on: bool,
    last: Option<Instant>,
    seen: Progress,
}

impl Bar {
    fn new(what: &'static str) -> Self {
        Bar {
            what,
            on: std::io::stderr().is_terminal(),
            last: None,
            seen: Progress::default(),
        }
    }

    fn show(&mut self, p: &Progress) {
        self.seen = *p;
        if !self.on
            || self
                .last
                .is_some_and(|t| t.elapsed() < Duration::from_millis(250))
        {
            return;
        }
        self.last = Some(Instant::now());
        let mut e = std::io::stderr();
        let _ = write!(e, "\r{}   ", self.line(p));
        let _ = e.flush();
    }

    fn line(&self, p: &Progress) -> String {
        format!(
            "{}: {}/{} files, {} of {}",
            self.what,
            p.files,
            p.total_files,
            gib(p.bytes),
            gib(p.total_bytes)
        )
    }

    fn done(&mut self) {
        if self.on && self.last.is_some() {
            eprintln!("\r{}, done.   ", self.line(&self.seen));
        }
    }
}
