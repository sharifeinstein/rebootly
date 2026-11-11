//! Linux recovery backend for Rebootly (SPEC §7.1, §7.2): volume discovery
//! (lsblk), BitLocker unlock (cryptsetup, fallback dislocker) and
//! read-only NTFS mounts (ntfs3, fallback ntfs-3g). Never writes to a
//! source disk.
//!
//! Platform-neutral code (it only runs system tools), so it builds and is
//! tested everywhere; only Linux front ends use it.

pub mod cmd;
pub mod discover;
pub mod mount;

pub use discover::discover;
pub use mount::{BASE, MountError, Mounted, Mounter};
