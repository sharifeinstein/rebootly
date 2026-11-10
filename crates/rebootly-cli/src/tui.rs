//! `rebootly-cli` with no arguments (or `rebootly-cli tui`): the text
//! screens, with this machine behind them.

use std::io::IsTerminal;
use std::path::PathBuf;

use rebootly_host::{Machine, MachineOptions};

#[derive(clap::Args, Default)]
pub struct TuiArgs {
    /// Folder with extra or updated pack YAML files.
    #[arg(long, value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
    /// Restore into DIR/C/… instead of this PC (preview off Windows);
    /// also shows the start menu outside Windows.
    #[arg(long, value_name = "DIR")]
    pub simulate_root: Option<PathBuf>,
    /// Extra folder offered as a backup target and searched for backups.
    #[arg(long = "drive", value_name = "DIR")]
    pub drives: Vec<PathBuf>,
}

pub fn run(args: TuiArgs) -> anyhow::Result<u8> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "the interactive screens need a terminal; see `rebootly-cli --help` for the commands"
        );
    }
    let m = Machine::new(MachineOptions {
        packs_dir: args.packs_dir,
        simulate_root: args.simulate_root,
        drives: args.drives,
        version: crate::VERSION.to_string(),
    });
    rebootly_tui::run(&m)
}
