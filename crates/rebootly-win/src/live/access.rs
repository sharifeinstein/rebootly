//! `VolumeAccess` for the running system (SPEC §3.5 live bits).
//!
//! Files are opened with `FILE_FLAG_BACKUP_SEMANTICS` (with SeBackupPrivilege
//! enabled this reads other users' profiles past their ACLs) and full sharing,
//! so files other programs have open for writing can still be read. Loaded
//! hives are snapshotted with `RegSaveKeyExW`; hives that aren't loaded
//! (users not signed in) are read from their files like offline.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rebootly_core::env::Environment;
use rebootly_core::hive::dirty;
use rebootly_core::source::{HiveRef, LoadedHive, VolumeAccess, clean_state, cloud};
use rebootly_core::winpath::{Mapped, map_live};
use rebootly_core::{Hive, HiveError};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::regsave::{Root, Snapshots};

#[derive(Debug)]
pub struct LiveAccess {
    snaps: Snapshots,
}

impl LiveAccess {
    pub fn new() -> std::io::Result<Self> {
        Ok(LiveAccess {
            snaps: Snapshots::new()?,
        })
    }

    fn snapshot_for(&self, which: HiveRef<'_>) -> std::io::Result<Option<PathBuf>> {
        let (root, key) = match which {
            HiveRef::Software => (Root::Machine, "SOFTWARE".to_owned()),
            HiveRef::System => (Root::Machine, "SYSTEM".to_owned()),
            HiveRef::User(sid) => (Root::Users, sid.to_owned()),
            HiveRef::UserClasses(sid) => (Root::Users, format!("{sid}_Classes")),
        };
        self.snaps.get(root, &key)
    }
}

/// Open for reading with backup semantics and full sharing. In a VSS
/// shadow copy OneDrive's filter isn't attached: downloaded OneDrive files
/// are read through their reparse point (`source::cloud`).
pub fn open_backup(path: &Path) -> std::io::Result<File> {
    cloud::open_or_bypass(
        |extra| {
            OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | extra)
                .open(path)
        },
        || cloud::own_attributes(path),
    )
}

impl VolumeAccess for LiveAccess {
    fn environment(&self) -> Environment {
        Environment::LiveWindows
    }

    fn load_hive(&self, file: &Path, which: HiveRef<'_>) -> Result<LoadedHive, HiveError> {
        if let Some(snap) = self.snapshot_for(which)? {
            return Ok(LoadedHive {
                hive: Hive::open(&snap)?,
                state: clean_state(),
                snapshot: Some(snap),
            });
        }
        let mut data = Vec::new();
        open_backup(file)?.read_to_end(&mut data)?;
        let hive = Hive::from_bytes(data)?;
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
        which: HiveRef<'_>,
    ) -> Result<Option<PathBuf>, HiveError> {
        Ok(self.snapshot_for(which)?)
    }

    fn open_read(&self, path: &Path) -> std::io::Result<File> {
        open_backup(path)
    }

    fn map(&self, win: &str, _system_drive: char, _volume_root: &Path) -> Mapped {
        map_live(win)
    }
}
