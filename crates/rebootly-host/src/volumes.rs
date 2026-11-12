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
            .ok_or_else(|| {
                anyhow::anyhow!("{device}: no such volume (see `rebootly-cli volumes`)")
            })?;
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
                && v.fs
                    .as_deref()
                    .is_some_and(|f| f.eq_ignore_ascii_case("ntfs"))
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

impl Volumes {
    /// Volumes that could be mounted read-write as a backup target (Linux:
    /// exFAT/NTFS, not BitLocker; Windows: none, letters are mounted).
    pub fn target_candidates(&self) -> Vec<Volume> {
        #[cfg(not(windows))]
        {
            rebootly_linux::target::candidates(&list())
                .into_iter()
                .filter(|v| !unlocked_bitlocker(&v.device))
                // Mounted already (for the Rescue flow) and holding Windows:
                // never a target, so not offered.
                .filter(|v| {
                    v.root().is_none_or(|r| {
                        rebootly_core::install::WindowsInstall::from_path(r).is_none()
                    })
                })
                .collect()
        }
        #[cfg(windows)]
        {
            Vec::new()
        }
    }

    /// Mount `device` read-write as a backup target (Linux). A read-only
    /// mount made by Rebootly is replaced; a disk with Windows is refused.
    pub fn mount_target(&self, device: &str) -> anyhow::Result<PathBuf> {
        #[cfg(not(windows))]
        {
            use rebootly_core::volume::VolumeState;
            let mut v = list()
                .into_iter()
                .find(|v| v.device.eq_ignore_ascii_case(device))
                .ok_or_else(|| {
                    anyhow::anyhow!("{device}: no such volume (see `rebootly-cli volumes`)")
                })?;
            if unlocked_bitlocker(&v.device) {
                anyhow::bail!("{device} is a BitLocker drive; backups go to an unencrypted drive");
            }
            if let VolumeState::Mounted {
                root,
                read_only: true,
            } = &v.state
            {
                if root.starts_with(rebootly_linux::BASE) {
                    if rebootly_core::install::WindowsInstall::from_path(root).is_some() {
                        anyhow::bail!(
                            "{device} has Windows on it; Rebootly never writes to a Windows disk \
                             from a rescue system"
                        );
                    }
                    self.mounter.unmount(root)?;
                    v.state = VolumeState::Unmounted;
                }
            }
            let t = rebootly_linux::TargetMounter::system(self.mounter.runner());
            Ok(t.mount(&v)?)
        }
        #[cfg(windows)]
        {
            anyhow::bail!("{device}: Windows mounts drives itself; pick its letter")
        }
    }
}

/// Mount the Rebootly stick (the partition with `.rebootly-root`)
/// read-write; `None` when there is none (Linux only).
pub fn mount_stick() -> anyhow::Result<Option<PathBuf>> {
    #[cfg(not(windows))]
    {
        let vols = list();
        let run = rebootly_linux::cmd::System;
        let t = rebootly_linux::TargetMounter::system(&run);
        match t.find_stick(&vols) {
            Some(v) => Ok(Some(t.mount(v)?)),
            None => Ok(None),
        }
    }
    #[cfg(windows)]
    {
        Ok(None)
    }
}

/// Was `device` unlocked by Rebootly (cryptsetup or dislocker)?
#[cfg(not(windows))]
fn unlocked_bitlocker(device: &str) -> bool {
    let s = std::path::Path::new(device)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Path::new("/dev/mapper")
        .join(format!("rebootly-{s}"))
        .exists()
        || Path::new(rebootly_linux::BASE)
            .join(format!("fve-{s}"))
            .exists()
}

fn with_hibernation(root: PathBuf, mut notes: Vec<String>) -> (PathBuf, Vec<String>) {
    let note = rebootly_core::volume::HIBERNATED_NOTE;
    if rebootly_core::volume::hibernated(&root) && !notes.iter().any(|n| n == note) {
        notes.push(note.to_string());
    }
    (root, notes)
}
