//! Live-Windows backend for `rebootly` (SPEC §3.2 `LiveWindows`, §3.5, §4 Tier 4).
//!
//! All Win32 code lives behind `cfg(windows)` so the workspace still builds
//! and tests on Linux. `unsafe` is allowed only in the small FFI modules.
//!
//! Done (M4): SeBackupPrivilege, live hive snapshots (`RegSaveKeyExW`),
//! backup-semantics reads, `SHGetKnownFolderPath` cross-check, sleep
//! prevention and battery check. Later in M4: VSS, OneDrive placeholder
//! policy, winget/netsh/printers/tasks capture.

#[cfg(windows)]
mod live;

#[cfg(windows)]
pub use live::{LiveError, LiveWindows, SleepGuard, on_battery};
