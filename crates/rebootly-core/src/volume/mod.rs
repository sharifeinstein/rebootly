//! Volumes seen from WinPE or a Linux rescue system (SPEC §3.3, §7):
//! what the platform backends (rebootly-linux, rebootly-win) report, the
//! BitLocker recovery key or password, hibernation, and the offline drive letters.

pub mod hiber;
pub mod key;
pub mod mounted;
pub mod unlock;

pub use hiber::{HIBERNATED_NOTE, hibernated};
pub use key::{
    KeyError, RecoveryKey, find_key, grouped as grouped_key, pasted_digits as pasted_key_digits,
    progress as key_progress, tidy as tidy_key, typing_status as key_status,
};
pub use mounted::{MountedVolume, VolumeId, drive_letters, match_letters};
pub use unlock::{Password, PasswordError, Unlock};

use std::path::PathBuf;

use serde::Serialize;

use crate::backup::preflight::gib;

/// Whether a volume can be read here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VolumeState {
    /// Readable at `root`. `read_only`: mounted read-only (always, when
    /// Rebootly mounted it on Linux).
    Mounted { root: PathBuf, read_only: bool },
    /// Encrypted with BitLocker and not unlocked yet.
    Locked,
    /// A filesystem Windows could use, not mounted (Linux).
    Unmounted,
}

/// A partition or drive letter that might hold Windows or user data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Volume {
    /// How the backend names it: `/dev/sda3` (Linux), `D:` (Windows).
    pub device: String,
    pub label: Option<String>,
    pub size: Option<u64>,
    /// `ntfs`, `BitLocker`, `vfat`, …
    pub fs: Option<String>,
    #[serde(flatten)]
    pub state: VolumeState,
    /// Partition identity, to match the offline Windows' drive letters.
    pub id: Option<VolumeId>,
    pub removable: bool,
}

impl Volume {
    pub fn root(&self) -> Option<&std::path::Path> {
        match &self.state {
            VolumeState::Mounted { root, .. } => Some(root),
            _ => None,
        }
    }

    pub fn is_locked(&self) -> bool {
        self.state == VolumeState::Locked
    }

    /// `sda3 · "Data" · 476.9 GiB · NTFS`, for lists.
    pub fn describe(&self) -> String {
        let mut parts = vec![self.device.clone()];
        if let Some(l) = self.label.as_deref().filter(|l| !l.is_empty()) {
            parts.push(format!("“{l}”"));
        }
        if let Some(s) = self.size {
            parts.push(gib(s));
        }
        match (&self.state, self.fs.as_deref()) {
            (VolumeState::Locked, _) => parts.push("locked with BitLocker".into()),
            (_, Some(fs)) => parts.push(fs.to_ascii_uppercase()),
            _ => {}
        }
        if self.removable {
            parts.push("removable".into());
        }
        parts.join(" · ")
    }

    /// Mounted volumes with a known identity, for the drive-letter match.
    pub fn mounted(volumes: &[Volume]) -> Vec<MountedVolume> {
        volumes
            .iter()
            .filter_map(|v| {
                Some(MountedVolume {
                    id: v.id.clone()?,
                    root: v.root()?.to_path_buf(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_and_mounted() {
        let mut v = Volume {
            device: "/dev/sda3".into(),
            label: Some("Windows".into()),
            size: Some(512 << 30),
            fs: Some("BitLocker".into()),
            state: VolumeState::Locked,
            id: Some(VolumeId::gpt("aa")),
            removable: false,
        };
        assert_eq!(
            v.describe(),
            "/dev/sda3 · “Windows” · 512.0 GiB · locked with BitLocker"
        );
        assert!(Volume::mounted(std::slice::from_ref(&v)).is_empty());
        v.state = VolumeState::Mounted {
            root: "/run/rebootly/sda3".into(),
            read_only: true,
        };
        v.fs = Some("ntfs".into());
        assert!(v.describe().ends_with("· NTFS"));
        assert_eq!(Volume::mounted(&[v]).len(), 1);
    }
}

#[cfg(test)]
mod drive_map_tests {
    use super::*;
    use crate::fixtures::{C_PARTITION, D_PARTITION, build_win11_basic};
    use crate::winpath::Mapped;
    use crate::{OfflineVolume, Source};

    #[test]
    fn other_drive_letters_map_onto_their_partitions() {
        let c = tempfile::tempdir().unwrap();
        let d = tempfile::tempdir().unwrap();
        build_win11_basic(c.path()).unwrap();
        std::fs::create_dir_all(d.path().join("pictures")).unwrap();
        let mounted = [
            MountedVolume {
                id: VolumeId::gpt(C_PARTITION),
                root: c.path().into(),
            },
            MountedVolume {
                id: VolumeId::gpt(&D_PARTITION.to_uppercase()),
                root: d.path().into(),
            },
        ];
        let pics = |src: &OfflineVolume| {
            let bob = src
                .profiles()
                .unwrap()
                .into_iter()
                .find(|p| p.username == "bob")
                .unwrap();
            let f = src.resolve_folders(&bob).unwrap();
            f.into_iter()
                .find(|f| f.path.name == "Pictures")
                .unwrap()
                .local
        };

        let plain = OfflineVolume::open(c.path()).unwrap();
        assert_eq!(pics(&plain), Mapped::OtherDrive { drive: 'D' });

        let src = OfflineVolume::open_with_volumes(c.path(), &mounted).unwrap();
        assert_eq!(src.drives(), &[('D', d.path().to_path_buf())]);
        // Case-insensitive, real casing kept.
        assert_eq!(
            pics(&src),
            Mapped::Local {
                path: d.path().join("pictures"),
                exists: true
            }
        );
        assert_eq!(src.map(r"D:\..\x"), Mapped::Unmappable);

        // Hibernated source and a target on the source's D: → warnings.
        assert!(src.source_notes().is_empty());
        std::fs::write(c.path().join("hiberfil.sys"), b"HIBR....").unwrap();
        let src = OfflineVolume::open_with_volumes(c.path(), &mounted).unwrap();
        let plan = crate::backup::plan::plan(&src, &Default::default()).unwrap();
        assert_eq!(plan.source_notes, vec![HIBERNATED_NOTE.to_string()]);
        let target = d.path().join("backups");
        std::fs::create_dir_all(&target).unwrap();
        let pre = crate::backup::preflight(&plan, &target, None, None);
        assert!(pre.warnings.iter().any(|w| w == HIBERNATED_NOTE));
        assert!(
            pre.warnings
                .iter()
                .any(|w| w.starts_with("the target is on D:"))
        );
        assert!(pre.blockers.is_empty(), "{:?}", pre.blockers);
    }
}
