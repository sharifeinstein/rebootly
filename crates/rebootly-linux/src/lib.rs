//! Linux recovery backend for Rebootly (SPEC §7.1, §7.2): volume discovery
//! (lsblk), BitLocker unlock (cryptsetup, fallback dislocker) and
//! read-only NTFS mounts (ntfs3, fallback ntfs-3g); read-write mounts only
//! for backup targets (`target`), never for a disk with Windows on it.
//!
//! Platform-neutral code (it only runs system tools), so it builds and is
//! tested everywhere; only Linux front ends use it.

pub mod cmd;
mod close;
pub mod discover;
pub mod mount;
pub mod target;

pub use discover::discover;
pub use mount::{BASE, MountError, Mounted, Mounter};
pub use target::{MARKER, TargetMounter};
