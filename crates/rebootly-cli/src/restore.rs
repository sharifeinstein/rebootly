//! `rebootly restore` and `rebootly undo` (SPEC §5).

use std::io::{BufRead, IsTerminal, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use rebootly_core::restore::conflict::Category;
use rebootly_core::restore::map::FileAction;
use rebootly_core::restore::plan::choose_user;
use rebootly_core::restore::{
    self, AppsMode, Backup, Bundle, Event, Interact, Policies, Policy, RestoreHost, RestoreOptions,
    SimHost, Target, report,
};

#[derive(clap::Args)]
pub struct RestoreArgs {
    /// The backup folder (the one with manifest.json).
    #[arg(value_name = "BACKUP")]
    pub backup: PathBuf,
    /// Backup user to restore. It goes into the user running Rebootly.
    #[arg(long, value_name = "NAME")]
    pub user: Option<String>,
    /// Show every action (and every conflict decision) without changing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Don't ask before starting or between steps.
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// When a file is already there: skip, overwrite, overwrite-if-newer,
    /// keep-both or ask (all categories). Default: keep-both for your files,
    /// overwrite for app and system settings (replaced files are kept for undo).
    #[arg(long, value_name = "POLICY")]
    pub conflict: Option<String>,
    /// Per category: folders, custom, packs, system, e.g. `packs=ask,folders=skip`.
    #[arg(long, value_name = "CAT=POLICY,...")]
    pub conflict_for: Option<String>,
    /// Put custom folders from old drive D: on E:, e.g. `--map-drive D=E`. Repeatable.
    #[arg(long, value_name = "OLD=NEW")]
    pub map_drive: Vec<String>,
    /// install: run `winget import` first; skip; only (install apps, then stop).
    #[arg(long, value_name = "MODE", default_value = "install")]
    pub apps: String,
    /// Also re-create scheduled tasks from the old PC.
    #[arg(long)]
    pub tasks: bool,
    /// Leave out system settings (hosts, VPN, Wi-Fi, power plan, certificates, tasks).
    #[arg(long)]
    pub no_system: bool,
    /// Don't import registry settings.
    #[arg(long)]
    pub no_registry: bool,
    /// Don't restore app settings (packs).
    #[arg(long)]
    pub no_packs: bool,
    /// Don't restart Explorer after registry or shell changes.
    #[arg(long)]
    pub no_explorer_restart: bool,
    /// Open sensitive.age (SSH keys, Wi-Fi passwords, certificates); asks for the passphrase.
    #[arg(long)]
    pub sensitive: bool,
    /// Read the passphrase from this file instead of asking.
    #[arg(long, value_name = "FILE")]
    pub passphrase_file: Option<PathBuf>,
    /// Folder with extra or updated pack YAML files.
    #[arg(long, value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
    /// Restore an incomplete backup, or with too little free space.
    #[arg(long)]
    pub force: bool,
    /// Restore files under DIR as if it were the new PC's drives (C:\ is DIR/C).
    /// For previews and tests; system steps are only listed.
    #[arg(long, value_name = "DIR")]
    pub simulate_root: Option<PathBuf>,
    /// Profile name for --simulate-root (default: the backup user's).
    #[arg(long, value_name = "NAME", requires = "simulate_root")]
    pub simulate_user: Option<String>,
}

fn options(a: &RestoreArgs) -> anyhow::Result<RestoreOptions> {
    let mut policies = match &a.conflict {
        Some(p) => Policies::all(p.parse::<Policy>().map_err(anyhow::Error::msg)?),
        None => Policies::default(),
    };
    if let Some(spec) = &a.conflict_for {
        policies.apply(spec).map_err(anyhow::Error::msg)?;
    }
    let mut drive_map = Vec::new();
    for m in &a.map_drive {
        let (from, to) = m
            .split_once('=')
            .context("--map-drive takes OLD=NEW, e.g. D=E")?;
        let letter = |s: &str| {
            let s = s.trim().trim_end_matches([':', '\\']);
            (s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic())
                .then(|| s.as_bytes()[0].to_ascii_uppercase() as char)
        };
        match (letter(from), letter(to)) {
            (Some(f), Some(t)) => drive_map.push((f, t)),
            _ => bail!("--map-drive takes drive letters, e.g. D=E (got {m})"),
        }
    }
    let apps = match a.apps.to_ascii_lowercase().as_str() {
        "install" => AppsMode::Install,
        "skip" => AppsMode::Skip,
        "only" => AppsMode::Only,
        other => bail!("--apps takes install, skip or only (got {other})"),
    };
    Ok(RestoreOptions {
        user: a.user.clone(),
        policies,
        drive_map,
        apps,
        packs: !a.no_packs,
        registry: !a.no_registry,
        system: !a.no_system,
        tasks: a.tasks,
        restart_explorer: !a.no_explorer_restart,
        force: a.force,
    })
}

/// The new PC: simulated under a folder, or the running Windows.
pub(crate) fn target(
    sim: Option<&PathBuf>,
    name: impl FnOnce() -> anyhow::Result<String>,
    dry_run: bool,
) -> anyhow::Result<(Target, Box<dyn RestoreHost>)> {
    if let Some(root) = sim {
        return Ok((Target::simulated(root, &name()?), Box::new(SimHost)));
    }
    #[cfg(windows)]
    {
        if rebootly_core::env::Environment::detect() != rebootly_core::env::Environment::LiveWindows
        {
            bail!("restore runs on the new PC's own Windows, not from WinPE or a recovery system");
        }
        let elevated = rebootly_win::is_elevated().unwrap_or(false);
        if !elevated && !dry_run {
            bail!(
                "restore needs an elevated prompt (right-click the terminal → Run as administrator); --dry-run works without"
            );
        }
        if !elevated {
            eprintln!(
                "note: not elevated; this dry run can't check Wi-Fi, tasks and certificates already present"
            );
        }
        Ok((
            rebootly_win::live_target()?,
            Box::new(rebootly_win::WinRestoreHost),
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = (name, dry_run);
        bail!("restore runs on the new PC's Windows; to preview here, add --simulate-root DIR")
    }
}

pub fn run(a: RestoreArgs) -> anyhow::Result<u8> {
    let backup = Backup::open(&a.backup)?;
    let opts = options(&a)?;
    let sim_name = || -> anyhow::Result<String> {
        if let Some(n) = &a.simulate_user {
            return Ok(n.clone());
        }
        let probe = Target::simulated(Path::new("."), "User");
        Ok(choose_user(&backup, &probe, opts.user.as_deref())?.username)
    };
    let (target, host) = target(a.simulate_root.as_ref(), sim_name, a.dry_run)?;
    if let Some(root) = &a.simulate_root {
        eprintln!(
            "Simulation: files go under {}; system steps are only listed.",
            root.display()
        );
    }
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
    eprint!("{}", report::plan_text(&plan));
    if !plan.blockers.is_empty() {
        bail!("can't restore: {}", plan.blockers.join("; "));
    }
    let interactive = std::io::stdin().is_terminal();
    if !a.dry_run && !a.yes {
        if !interactive {
            bail!("add --yes to restore without being asked (or --dry-run to preview)");
        }
        if !confirm(&format!(
            "Restore into {} ({})? [y/N] ",
            target.username, target.profile
        ))? {
            eprintln!("Nothing was changed.");
            return Ok(0);
        }
    }
    let mut ui = CliUi {
        interactive,
        yes: a.yes,
        progress: false,
    };
    let result = restore::execute(&plan, host.as_ref(), &mut ui, a.dry_run);
    ui.end_progress();
    eprint!("{}", report::result_text(&plan, &result));
    if let Some(d) = &result.undo_dir {
        println!("{}", d.display());
    }
    drop(bundle);
    Ok(if result.failed() > 0 { 2 } else { 0 })
}

pub(crate) fn confirm(q: &str) -> anyhow::Result<bool> {
    eprint!("{q}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "Yes"))
}

struct CliUi {
    interactive: bool,
    yes: bool,
    progress: bool,
}

impl CliUi {
    fn end_progress(&mut self) {
        if self.progress {
            eprintln!();
            self.progress = false;
        }
    }
}

impl Interact for CliUi {
    fn conflict(&mut self, f: &FileAction, _: &Path) -> (Policy, bool) {
        self.end_progress();
        if !self.interactive {
            return (Policy::Skip, false);
        }
        eprintln!("{} is already there and differs (from {}).", f.dst, f.from);
        let cat = Category::as_str(f.category);
        loop {
            eprint!(
                "  [s]kip, [o]verwrite, [n]ewer wins, [k]eep both; capital letter = for all {cat}: "
            );
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
                return (Policy::Skip, true);
            }
            let c = line.trim().chars().next().unwrap_or(' ');
            let p = match c.to_ascii_lowercase() {
                's' => Policy::Skip,
                'o' => Policy::Overwrite,
                'n' => Policy::OverwriteIfNewer,
                'k' => Policy::KeepBoth,
                _ => continue,
            };
            return (p, c.is_ascii_uppercase());
        }
    }

    fn apps_done(&mut self) -> bool {
        if !self.interactive || self.yes {
            return true;
        }
        eprintln!(
            "Install any other apps you need now (the list is under Manual steps in the plan)."
        );
        eprint!("Press Enter to continue with files and settings, or q then Enter to stop here: ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        !line.trim().eq_ignore_ascii_case("q")
    }

    fn event(&mut self, e: Event<'_>) {
        match e {
            Event::Phase(p) => {
                self.end_progress();
                eprintln!("{}…", p.label());
            }
            Event::Progress { done, total } => {
                eprint!("\r  {done}/{total}   ");
                self.progress = done < total;
                if done == total {
                    eprintln!();
                }
            }
            Event::Note(n) => {
                self.end_progress();
                eprintln!("  {n}");
            }
        }
    }
}
