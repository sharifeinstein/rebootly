//! Live-Windows backend for `rebootly` (SPEC §3.2 `LiveWindows`, §3.5, §4 Tier 4).
//!
//! Planned for M4: SeBackupPrivilege, VSS snapshots, `SHGetKnownFolderPath`
//! cross-check, OneDrive placeholder handling, winget/netsh capture, sleep
//! prevention. All Win32 code lives behind `cfg(windows)` so the workspace
//! still builds and tests on Linux.
