//! How a [`super::OfflineVolume`] reads hives and files (SPEC §3.2).
//!
//! Offline, hives are plain files on a read-only mount. On a running Windows
//! the loaded hives (SOFTWARE, SYSTEM, signed-in users' NTUSER.DAT and
//! UsrClass.dat) are locked by the kernel, so `rebootly-win` supplies an
//! access strategy that snapshots them from the live registry and opens other
//! files with backup semantics. Everything above this layer (profiles,
//! folders, packs, copy engine) is shared.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::env::Environment;
use crate::hive::dirty::{self, HiveState};
use crate::hive::{Hive, HiveError};
use crate::winpath::{Mapped, map_to_volume};

/// Which hive a file is, so a live strategy can find its loaded key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiveRef<'a> {
    /// `HKLM\SOFTWARE` (`Windows\System32\config\SOFTWARE`).
    Software,
    /// `HKLM\SYSTEM` (`Windows\System32\config\SYSTEM`).
    System,
    /// A user's NTUSER.DAT (`HKU\<sid>` when signed in).
    User(&'a str),
    /// A user's UsrClass.dat (`HKU\<sid>_Classes` when signed in).
    UserClasses(&'a str),
}

/// A parsed hive plus what we know about its health.
pub struct LoadedHive {
    pub hive: Hive,
    pub state: HiveState,
    /// Set when the hive was saved from the live registry: this file is a
    /// clean, current copy and is what a backup keeps as the raw hive.
    pub snapshot: Option<PathBuf>,
}

/// Strategy for reading a Windows install. Must never write to the source
/// volume in offline mode (SPEC §1.6).
pub trait VolumeAccess: Send + Sync + std::fmt::Debug {
    fn environment(&self) -> Environment;

    /// Load and parse a hive whose file on the volume is `file`.
    fn load_hive(&self, file: &Path, which: HiveRef<'_>) -> Result<LoadedHive, HiveError>;

    /// A clean copy to back up instead of `file` and its transaction logs,
    /// when the file itself can't be read (a hive loaded in the live
    /// registry). `None`: copy `file` and its logs as they are.
    fn hive_snapshot(&self, file: &Path, which: HiveRef<'_>) -> Result<Option<PathBuf>, HiveError>;

    /// Open a source file for reading.
    fn open_read(&self, path: &Path) -> std::io::Result<File>;

    /// Map a Windows path (as stored in the registry) to a local path.
    fn map(&self, win: &str, system_drive: char, volume_root: &Path) -> Mapped;
}

/// Offline access: plain read-only file opens, hives read from disk with
/// dirty-log detection, only the system drive mapped.
#[derive(Debug, Default, Clone, Copy)]
pub struct OfflineAccess;

impl VolumeAccess for OfflineAccess {
    fn environment(&self) -> Environment {
        Environment::detect()
    }

    fn load_hive(&self, file: &Path, _which: HiveRef<'_>) -> Result<LoadedHive, HiveError> {
        let hive = Hive::open(file)?;
        let state = dirty::assess(file, hive.header());
        Ok(LoadedHive {
            hive,
            state,
            snapshot: None,
        })
    }

    fn hive_snapshot(
        &self,
        _file: &Path,
        _which: HiveRef<'_>,
    ) -> Result<Option<PathBuf>, HiveError> {
        Ok(None)
    }

    fn open_read(&self, path: &Path) -> std::io::Result<File> {
        File::open(path) // read-only by construction
    }

    fn map(&self, win: &str, system_drive: char, volume_root: &Path) -> Mapped {
        map_to_volume(win, system_drive, volume_root)
    }
}

/// `HiveState` for a hive saved from the live registry: consistent by
/// construction, no pending logs.
pub fn clean_state() -> HiveState {
    HiveState {
        seq_mismatch: false,
        checksum_ok: true,
        logs: Vec::new(),
    }
}
