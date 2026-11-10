//! What the front ends (TUI, GUI) need from the machine they run on.
//! [`Machine`] is the real one (rebootly-win on Windows, mount folders on
//! Linux); tests use fakes over the fixtures.

pub mod custom;
pub mod machine;
pub mod session;
pub mod source;

pub use machine::{Machine, MachineOptions};

/// Donation link shown (optionally, closable) at the end of a backup or
/// restore. Set at build time: `REBOOTLY_DONATE_URL=https://… cargo build`;
/// without it nothing is shown.
pub const DONATE_URL: Option<&str> = option_env!("REBOOTLY_DONATE_URL");

use std::any::Any;
use std::path::{Path, PathBuf};

use rebootly_core::Source;
use rebootly_core::env::Environment;
use rebootly_core::packs::schema::CompiledPack;
use rebootly_core::restore::{RestoreHost, Target};

/// A drive or folder offered as a backup target (or searched for backups).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    pub path: PathBuf,
    /// Shown beside the path, e.g. "USB stick, 58.2 GiB free".
    pub note: String,
}

/// `Send + Sync` so the GUI can plan and copy on worker threads.
pub trait Platform: Send + Sync {
    /// Where Rebootly runs; WinPE and Linux start the Rescue flow.
    fn environment(&self) -> Environment;
    /// `0.1.0 (commit abc1234)`.
    fn version(&self) -> String;
    /// The running Windows (elevated). Errors say what to do.
    fn open_live(&self) -> anyhow::Result<Box<dyn Source>>;
    /// A Windows installation on a mounted volume.
    fn open_offline(&self, root: &Path) -> anyhow::Result<Box<dyn Source>> {
        Ok(Box::new(rebootly_core::OfflineVolume::open(root)?))
    }
    /// Directories to scan for Windows installations (Rescue flow).
    fn install_roots(&self) -> Vec<PathBuf>;
    /// Drives offered as backup targets and searched for backups. Never
    /// the running system's own drive.
    fn drives(&self) -> Vec<Drive>;
    /// Filesystem type of the target (None: core looks it up on Linux).
    fn target_fs(&self, path: &Path) -> Option<String>;
    /// Keeps the PC awake while the guard lives (live backups).
    fn keep_awake(&self, live: bool) -> Box<dyn Any>;
    /// Warning to show before a long live job, e.g. running on battery.
    fn power_warning(&self) -> Option<String> {
        None
    }
    /// Installed packs (built-in and the `packs/` folder).
    fn packs(&self) -> Vec<CompiledPack>;
    /// The new PC (current user) to restore into. `backup_user` is the
    /// backup user chosen (only used for simulated targets).
    fn restore_target(
        &self,
        dry_run: bool,
        backup_user: &str,
    ) -> anyhow::Result<(Target, Box<dyn RestoreHost + Send>)>;
}
