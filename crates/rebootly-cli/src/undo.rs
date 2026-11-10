//! `rebootly undo`: put a restore back (SPEC §5 undo).

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use rebootly_core::restore::undo::{self, UNDONE, UndoEntry};

use crate::restore::{confirm, target};

#[derive(clap::Args)]
pub struct UndoArgs {
    /// Undo folder of a restore (default: the latest one of this user).
    #[arg(value_name = "UNDO_DIR")]
    pub dir: Option<PathBuf>,
    /// List restores that can be undone.
    #[arg(long, conflicts_with = "dir")]
    pub list: bool,
    #[arg(long)]
    pub dry_run: bool,
    /// Also undo files that were changed after the restore.
    #[arg(long)]
    pub force: bool,
    #[arg(long, short = 'y')]
    pub yes: bool,
    #[arg(long, value_name = "DIR")]
    pub simulate_root: Option<PathBuf>,
    #[arg(long, value_name = "NAME", requires = "simulate_root")]
    pub simulate_user: Option<String>,
}

pub fn run(a: UndoArgs) -> anyhow::Result<u8> {
    let header_profile = |d: &Path| -> Option<String> {
        match undo::read_journal(d).ok()?.into_iter().next()? {
            UndoEntry::Header { profile, .. } => profile.rsplit('\\').next().map(String::from),
            _ => None,
        }
    };
    let name = || -> anyhow::Result<String> {
        a.simulate_user
            .clone()
            .or_else(|| a.dir.as_deref().and_then(header_profile))
            .context("give --simulate-user NAME")
    };
    let (target, host) = target(a.simulate_root.as_ref(), name, a.dry_run)?;
    let runs = undo::list(&target);
    if a.list {
        if runs.is_empty() {
            eprintln!("No restores to undo for {}.", target.username);
        }
        for (dir, h) in &runs {
            if let UndoEntry::Header {
                job_id,
                started,
                user,
                ..
            } = h
            {
                println!(
                    "{}  {started}  backup {job_id} (user {user})",
                    dir.display()
                );
            }
        }
        return Ok(0);
    }
    let dir = match &a.dir {
        Some(d) => d.clone(),
        None => runs
            .first()
            .map(|r| r.0.clone())
            .context("no restore to undo for this user")?,
    };
    if dir.join(UNDONE).exists() && !a.force {
        bail!(
            "{} was already undone (--force to run it again)",
            dir.display()
        );
    }
    if !a.dry_run && !a.yes {
        if !std::io::stdin().is_terminal() {
            bail!("add --yes to undo without being asked (or --dry-run to preview)");
        }
        if !confirm(&format!(
            "Undo the restore recorded in {}? [y/N] ",
            dir.display()
        ))? {
            return Ok(0);
        }
    }
    let r = undo::undo(&dir, &target, host.as_ref(), a.dry_run, a.force)?;
    for d in &r.done {
        eprintln!("  {d}");
    }
    for k in &r.kept {
        eprintln!("  kept {k}");
    }
    for f in &r.failed {
        eprintln!("  FAILED {f}");
    }
    eprintln!(
        "{}: {} done, {} kept, {} failed",
        if a.dry_run {
            "Dry run"
        } else {
            "Undo finished"
        },
        r.done.len(),
        r.kept.len(),
        r.failed.len()
    );
    Ok(if r.failed.is_empty() { 0 } else { 2 })
}
