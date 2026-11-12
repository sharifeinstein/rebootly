//! The `Source` abstraction (SPEC §3.2) and `OfflineVolume`, a Windows
//! install read through a [`VolumeAccess`] strategy.
//!
//! Packs, manifest and copy engine talk only to `dyn Source`. Offline,
//! `OfflineVolume::open` reads a mounted volume. `rebootly-win`'s
//! `LiveWindows` wraps `OfflineVolume::open_with` and a live access strategy,
//! so the resolution logic is the same in every mode.

mod access;
pub mod cloud;
mod offline;

pub use access::{HiveRef, LoadedHive, OfflineAccess, VolumeAccess, clean_state};
pub use offline::OfflineVolume;

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::apps::{InstalledApp, machine_apps};
use crate::env::Environment;
use crate::expand::VarSet;
use crate::folders::{FolderPath, resolve_user_folders, user_vars};
use crate::hive::dirty::HiveState;
use crate::hive::{Hive, HiveError};
use crate::install::{OsInfo, WindowsInstall, read_os_info};
use crate::profiles::{
    AccountKind, ProfileDirs, SkippedProfile, has_microsoft_account, read_profile_list, system_vars,
};
use crate::volume::{MountedVolume, drive_letters, match_letters};
use crate::winpath::{Mapped, WinPath, resolve_ci};

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error(
        "no Windows installation found at {0} (looked for Windows\\System32\\config\\SOFTWARE)"
    )]
    NotWindows(PathBuf),
    #[error("cannot read hive {path}: {source}")]
    Hive { path: PathBuf, source: HiveError },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub(crate) fn hive_err(path: &Path) -> impl FnOnce(HiveError) -> SourceError + '_ {
    move |source| SourceError::Hive {
        path: path.to_path_buf(),
        source,
    }
}

/// The user's registry hive and its health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserHive {
    pub path: PathBuf,
    pub state: HiveState,
    /// Clean copy saved from the live registry (live mode, signed-in user).
    pub snapshot: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Profile {
    pub sid: String,
    pub username: String,
    pub kind: AccountKind,
    /// Windows-style profile path from `ProfileList`.
    pub profile_path: String,
    pub local: Mapped,
    /// `None` when NTUSER.DAT is missing or unreadable (see `warnings`).
    pub hive: Option<UserHive>,
    pub warnings: Vec<String>,
}

/// A known folder resolved to a Windows path and mapped to local disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedFolder {
    #[serde(flatten)]
    pub path: FolderPath,
    pub local: Mapped,
    /// Folder lives in the user's OneDrive (Known Folder Move).
    pub onedrive: bool,
}

pub trait Source: Send + Sync {
    fn environment(&self) -> Environment;
    /// Root of the source system volume (backups must never land inside it).
    fn source_root(&self) -> &Path;
    fn os_info(&self) -> &OsInfo;
    fn profile_dirs(&self) -> &ProfileDirs;
    /// Profiles from `ProfileList`, minus system accounts.
    fn profiles(&self) -> Result<Vec<Profile>, SourceError>;
    fn skipped_profiles(&self) -> &[SkippedProfile];
    fn resolve_folders(&self, profile: &Profile) -> Result<Vec<ResolvedFolder>, SourceError>;
    /// Open a local file for reading. Never opens for write.
    fn open_read(&self, path: &Path) -> std::io::Result<File>;
    fn open_user_hive(&self, profile: &Profile) -> Result<Option<Hive>, SourceError>;
    /// Machine-wide installed apps (Uninstall keys, 64- and 32-bit).
    fn machine_apps(&self) -> &[InstalledApp];
    /// The user's variables (`%APPDATA%`, `%OneDrive%`, …) from the source.
    fn user_vars(&self, profile: &Profile) -> Result<VarSet, SourceError>;
    /// Map a Windows path from the source machine to a local path.
    fn map_path(&self, win: &str) -> Mapped;
    /// A clean copy of hive `file` to back up instead of the file and its
    /// logs (live mode, hive loaded in the registry). Default: none.
    fn hive_snapshot(
        &self,
        _file: &Path,
        _which: HiveRef<'_>,
    ) -> Result<Option<PathBuf>, SourceError> {
        Ok(None)
    }
    /// Live-only Tier 4 captures (winget export, Wi-Fi export, printers),
    /// run now into private temp files. `sensitive`: the bundle is
    /// available, so captures holding secrets may run. Default: none.
    fn system_captures(&self, _sensitive: bool) -> Captures {
        Captures::default()
    }
    /// The source is the running Windows itself (`--live`). Not the same as
    /// `environment()`, which says where the process runs: an offline backup
    /// of another disk made from Windows runs on live Windows too.
    fn is_live(&self) -> bool {
        false
    }
    /// Things to tell the user before a backup of this source, e.g. that
    /// the offline Windows was hibernated. Default: none.
    fn source_notes(&self) -> Vec<String> {
        Vec::new()
    }
    /// The Windows licence (key and channel) from the SOFTWARE hive.
    /// Default: none.
    fn windows_licence(&self) -> Option<&crate::licence::WindowsLicence> {
        None
    }
    /// The source's other drive letters that are read from here (offline:
    /// matched through `MountedDevices`). Default: none.
    fn other_drives(&self) -> Vec<(char, PathBuf)> {
        Vec::new()
    }
    /// Open a point-in-time copy of `path` (VSS in live mode), for a file
    /// another program holds locked. Default: unsupported.
    fn open_snapshot(&self, _path: &Path) -> std::io::Result<File> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            LOCKED_HINT,
        ))
    }
    /// Named alternate data streams of `path` (never the main stream), read
    /// from the snapshot when `snapshot`. Default: none (offline mounts
    /// don't expose them).
    fn alt_streams(&self, _path: &Path, _snapshot: bool) -> std::io::Result<Vec<StreamInfo>> {
        Ok(Vec::new())
    }
    /// Open alternate data stream `name` of `path`.
    fn open_stream(&self, _path: &Path, _name: &str, _snapshot: bool) -> std::io::Result<File> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "alternate data streams can't be read here",
        ))
    }
}

/// Why a locked file couldn't be read, in plain language.
pub const LOCKED_HINT: &str =
    "the file is open in another program; close it and run the backup again with --resume";

/// An alternate data stream (`file.txt:name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub name: String,
    pub size: u64,
}

/// A file produced by a live command, to back up like a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    /// Destination in the backup (or in the bundle when `sensitive`).
    pub dest: String,
    pub file: PathBuf,
    /// What produced it, e.g. `winget export`.
    pub origin: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captures {
    pub files: Vec<Capture>,
    /// Commands that failed or were unavailable (plain language).
    pub warnings: Vec<String>,
}
