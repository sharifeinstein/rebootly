//! `LiveWindows`: the running system as a `Source` (SPEC §3.2).
//!
//! Same resolution logic as offline (`OfflineVolume` with a live
//! [`access::LiveAccess`]), plus the `SHGetKnownFolderPath` cross-check for
//! the user the tool runs as, VSS for locked files and alternate data
//! streams. Requires an elevated process: SeBackupPrivilege
//! is needed both for `RegSaveKeyExW` and to read other users' profiles.

mod access;
mod attrs;
mod capture;
mod cmd;
mod knownfolder;
mod power;
mod regsave;
mod restore;
mod streams;
mod token;
mod volume;
mod vss;
mod wide;

use std::fs::File;
use std::path::{Path, PathBuf};

use rebootly_core::apps::InstalledApp;
use rebootly_core::env::Environment;
use rebootly_core::expand::VarSet;
use rebootly_core::folders::cross_check;
use rebootly_core::install::OsInfo;
use rebootly_core::profiles::{ProfileDirs, SkippedProfile};
use rebootly_core::source::{Profile, ResolvedFolder};
use rebootly_core::winpath::Mapped;
use rebootly_core::{Captures, Hive, HiveRef, OfflineVolume, Source, SourceError, StreamInfo};

pub use power::{SleepGuard, on_battery};
pub use restore::{WinRestoreHost, live_target};
pub use token::is_elevated;
pub use volume::filesystem_type;

#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error(
        "live mode needs an elevated prompt (right-click the terminal → Run as administrator): \
         reading the registry hives and other users' files requires SeBackupPrivilege"
    )]
    NotElevated,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Source(#[from] SourceError),
}

#[derive(Debug)]
pub struct LiveWindows {
    inner: OfflineVolume,
    /// SID the process runs as: the only user `SHGetKnownFolderPath` can answer for.
    me: Option<String>,
    captures: capture::Capturer,
    vss: vss::Vss,
}

impl LiveWindows {
    /// Open the running system. Fails unless elevated.
    pub fn open() -> Result<Self, LiveError> {
        if !token::is_elevated()? || !token::enable_privilege("SeBackupPrivilege")? {
            return Err(LiveError::NotElevated);
        }
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let root = PathBuf::from(format!("{}\\", drive.trim_end_matches('\\')));
        let inner = OfflineVolume::open_with(&root, Box::new(access::LiveAccess::new()?))?;
        Ok(LiveWindows {
            inner,
            me: token::current_user_sid().ok(),
            captures: capture::Capturer::new(),
            vss: vss::Vss::new(),
        })
    }
}

impl Source for LiveWindows {
    fn environment(&self) -> Environment {
        Environment::LiveWindows
    }
    fn source_root(&self) -> &Path {
        self.inner.source_root()
    }
    fn os_info(&self) -> &OsInfo {
        self.inner.os_info()
    }
    fn profile_dirs(&self) -> &ProfileDirs {
        self.inner.profile_dirs()
    }
    fn profiles(&self) -> Result<Vec<Profile>, SourceError> {
        let mut out = self.inner.profiles()?;
        for p in out.iter_mut() {
            if self.me.as_deref() != Some(p.sid.as_str()) {
                continue;
            }
            let resolved: Vec<_> = self
                .inner
                .resolve_folders(p)?
                .into_iter()
                .map(|f| f.path)
                .collect();
            p.warnings
                .extend(cross_check(&resolved, knownfolder::known_folder_path));
        }
        Ok(out)
    }
    fn skipped_profiles(&self) -> &[SkippedProfile] {
        self.inner.skipped_profiles()
    }
    fn resolve_folders(&self, profile: &Profile) -> Result<Vec<ResolvedFolder>, SourceError> {
        self.inner.resolve_folders(profile)
    }
    fn open_read(&self, path: &Path) -> std::io::Result<File> {
        self.inner.open_read(path)
    }
    fn open_user_hive(&self, profile: &Profile) -> Result<Option<Hive>, SourceError> {
        self.inner.open_user_hive(profile)
    }
    fn machine_apps(&self) -> &[InstalledApp] {
        self.inner.machine_apps()
    }
    fn user_vars(&self, profile: &Profile) -> Result<VarSet, SourceError> {
        self.inner.user_vars(profile)
    }
    fn map_path(&self, win: &str) -> Mapped {
        self.inner.map_path(win)
    }
    fn hive_snapshot(
        &self,
        file: &Path,
        which: HiveRef<'_>,
    ) -> Result<Option<PathBuf>, SourceError> {
        self.inner.hive_snapshot(file, which)
    }
    fn is_live(&self) -> bool {
        true
    }
    fn system_captures(&self, sensitive: bool) -> Captures {
        self.captures.run(sensitive)
    }
    fn open_snapshot(&self, path: &Path) -> std::io::Result<File> {
        access::open_backup(&self.vss.path_in_snapshot(path)?)
    }
    fn alt_streams(&self, path: &Path, snapshot: bool) -> std::io::Result<Vec<StreamInfo>> {
        if snapshot {
            streams::list(&self.vss.path_in_snapshot(path)?)
        } else {
            streams::list(path)
        }
    }
    fn open_stream(&self, path: &Path, name: &str, snapshot: bool) -> std::io::Result<File> {
        if snapshot {
            streams::open(&self.vss.path_in_snapshot(path)?, name)
        } else {
            streams::open(path, name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::knownfolder::parse_guid;

    #[test]
    fn guid_parsing() {
        let g = parse_guid("{FDD39AD0-238F-46AF-ADB4-6C85480369C7}").unwrap();
        assert_eq!(g.data1, 0xFDD39AD0);
        assert_eq!(g.data4, [0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7]);
        assert!(parse_guid("{nope}").is_none());
    }
}
