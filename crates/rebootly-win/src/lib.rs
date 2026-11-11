//! Live-Windows backend for `rebootly` (SPEC §3.2 `LiveWindows`, §3.5, §4 Tier 4).
//!
//! All Win32 code lives behind `cfg(windows)` so the workspace still builds
//! and tests on Linux. `unsafe` is allowed only in the small FFI modules.
//!
//! M4: SeBackupPrivilege, live hive snapshots (`RegSaveKeyExW`),
//! backup-semantics reads, VSS for locked files, alternate data streams,
//! `SHGetKnownFolderPath` cross-check, Tier 4 command captures, sleep
//! prevention, battery check, target filesystem type.
//! M5: the restore target (current user's known folders) and `RestoreHost`.
//! M7: drive letters as volumes, BitLocker lock state, `manage-bde` unlock.

#[cfg(windows)]
mod live;
#[cfg_attr(not(windows), allow(dead_code))]
mod parse;

#[cfg(windows)]
pub use live::{
    LiveError, LiveWindows, SleepGuard, WinRestoreHost, filesystem_type, is_elevated, live_target,
    on_battery,
};
#[cfg(windows)]
pub use live::clipboard::clipboard_text;
#[cfg(windows)]
pub use live::volumes::{letter_identity, unlock as unlock_bitlocker, volumes};
