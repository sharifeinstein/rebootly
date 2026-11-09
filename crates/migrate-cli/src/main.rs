//! `migrate` command-line interface (thin layer over migrate-core).

mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use migrate_core::OfflineVolume;
use migrate_core::install::WindowsInstall;

#[derive(Parser)]
#[command(name = "migrate", version, about = "Portable PC migration and backup")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show users and their real folder paths for a Windows installation.
    Inspect {
        /// Volume root containing `Windows\` (or the Windows directory itself).
        #[arg(long, value_name = "PATH", conflicts_with = "scan")]
        windows_root: Option<PathBuf>,
        /// Scan these directories (and their subdirectories) for installs.
        #[arg(long, value_name = "DIR", num_args = 1..)]
        scan: Vec<PathBuf>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.cmd {
        Cmd::Inspect {
            windows_root,
            scan,
            json,
        } => {
            let root = match (windows_root, scan.is_empty()) {
                (Some(r), _) => r,
                (None, false) => pick_install(&scan)?,
                (None, true) => anyhow::bail!("give --windows-root <PATH> or --scan <DIR>..."),
            };
            let src = OfflineVolume::open(&root)?;
            let report = migrate_core::inspect::inspect(&src)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render::text(&report, &src.install().volume_root));
            }
            Ok(())
        }
    }
}

/// Non-interactive: exactly one install must be found (the TUI will offer a
/// choice when there are several; SPEC §3.3).
fn pick_install(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
    let found = WindowsInstall::scan(roots);
    match found.as_slice() {
        [] => anyhow::bail!("no Windows installation found under the given directories"),
        [one] => Ok(one.volume_root.clone()),
        many => {
            let list: Vec<String> = many
                .iter()
                .map(|i| format!("  {}", i.volume_root.display()))
                .collect();
            anyhow::bail!(
                "several Windows installations found; choose one with --windows-root:\n{}",
                list.join("\n")
            )
        }
    }
}
