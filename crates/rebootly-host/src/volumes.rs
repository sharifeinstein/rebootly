//! Volumes on this machine (SPEC §3.3, §7): listing, BitLocker unlock,
//! read-only mounts on Linux, and opening an offline Windows with its other
//! drive letters mapped.
//!
//! Windows/WinPE: drive letters (`rebootly-win`), unlocked with
//! `manage-bde`. Linux: `rebootly-linux` (lsblk/blkid, cryptsetup or
//! dislocker, ntfs3 or ntfs-3g), mounts under `/run/rebootly`, undone when
//! [`Volumes`] is dropped.

use std::path::{Path, PathBuf};

use rebootly_core::volume::{Unlock, Volume};
use rebootly_core::{OfflineVolume, Source};

/// Every volume Windows could use, as this system sees it now.
pub fn list() -> Vec<Volume> {
    #[cfg(windows)]
    {
        rebootly_win::volumes()
    }
    #[cfg(not(windows))]
    {
        rebootly_linux::discover(&rebootly_linux::cmd::System).unwrap_or_default()
    }
}

/// Open the Windows at `root`, mapping its other drive letters onto the
/// volumes mounted here.
pub fn open_offline(root: &Path) -> anyhow::Result<Box<dyn Source>> {
    let mounted = Volume::mounted(&list());
    Ok(Box::new(OfflineVolume::open_with_volumes(root, &mounted)?))
}

/// Unlocks and mounts made for this run.
pub struct Volumes {
    #[cfg(not(windows))]
    mounter: rebootly_linux::Mounter,
}

impl Default for Volumes {
    fn default() -> Self {
        Self::new()
    }
}

impl Volumes {
    pub fn new() -> Self {
        Volumes {
            #[cfg(not(windows))]
            mounter: rebootly_linux::Mounter::system(),
        }
    }

    /// Leave what was mounted in place after this run (CLI `mount`).
    pub fn keep(&mut self) {
        #[cfg(not(windows))]
        self.mounter.keep();
    }

    /// Unlock (with the recovery key or password) and mount `device`
    /// read-only.
    pub fn open(
        &self,
        device: &str,
        key: Option<&Unlock>,
    ) -> anyhow::Result<(PathBuf, Vec<String>)> {
        let vols = list();
        let v = vols
            .iter()
            .find(|v| v.device.eq_ignore_ascii_case(device))
            .ok_or_else(|| anyhow::anyhow!("{device}: no such volume (see `rebootly-cli volumes`)"))?;
        #[cfg(windows)]
        {
            if v.is_locked() {
                let key = key.ok_or_else(|| {
                    anyhow::anyhow!("{device} needs its recovery key or password")
                })?;
                rebootly_win::unlock_bitlocker(device, key).map_err(anyhow::Error::msg)?;
            }
            let root = PathBuf::from(format!("{}\\", v.device.trim_end_matches('\\')));
            Ok(with_hibernation(root, Vec::new()))
        }
        #[cfg(not(windows))]
        {
            let m = self.mounter.open(v, key)?;
            Ok(with_hibernation(m.root, m.notes))
        }
    }

    /// Mount every unmounted Windows filesystem read-only (Linux); on
    /// Windows letters are mounted already. Returns notes and failures.
    pub fn mount_all(&self) -> Vec<String> {
        let mut notes = Vec::new();
        #[cfg(not(windows))]
        for v in list().iter().filter(|v| {
            v.state == rebootly_core::volume::VolumeState::Unmounted
                && v.fs.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("ntfs"))
        }) {
            match self.mounter.open(v, None) {
                Ok(m) => notes.extend(with_hibernation(m.root, m.notes).1),
                Err(e) => notes.push(format!("{}: {e}", v.device)),
            }
        }
        notes.dedup();
        notes
    }
}

fn with_hibernation(root: PathBuf, mut notes: Vec<String>) -> (PathBuf, Vec<String>) {
    let note = rebootly_core::volume::HIBERNATED_NOTE;
    if rebootly_core::volume::hibernated(&root) && !notes.iter().any(|n| n == note) {
        notes.push(note.to_string());
    }
    (root, notes)
}
