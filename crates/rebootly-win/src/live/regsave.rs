//! Snapshots of loaded hives via `RegSaveKeyExW` (needs SeBackupPrivilege).
//!
//! Loaded hive files are locked by the kernel and their on-disk copy can lag
//! behind (lazy flush), so a live read saves the key instead: the result is
//! a clean, current hive file. Snapshots live in a private temp folder under
//! the elevated user's `%TEMP%` and are removed when the source is dropped.
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, REG_LATEST_FORMAT, REG_OPTION_BACKUP_RESTORE,
    RegCloseKey, RegOpenKeyExW, RegSaveKeyExW,
};

use super::wide::{wide, wide_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Root {
    Machine,
    Users,
}

impl Root {
    fn hkey(self) -> HKEY {
        match self {
            Root::Machine => HKEY_LOCAL_MACHINE,
            Root::Users => HKEY_USERS,
        }
    }
}

#[derive(Debug)]
pub struct Snapshots {
    dir: PathBuf,
    saved: Mutex<HashMap<(Root, String), Option<PathBuf>>>,
}

impl Snapshots {
    pub fn new() -> io::Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir =
            std::env::temp_dir().join(format!("rebootly-hives-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir)?;
        Ok(Snapshots {
            dir,
            saved: Mutex::new(HashMap::new()),
        })
    }

    /// Save `root\subkey` once and return the file. `Ok(None)`: the key is
    /// not loaded (e.g. that user isn't signed in), so read the file instead.
    pub fn get(&self, root: Root, subkey: &str) -> io::Result<Option<PathBuf>> {
        let key = (root, subkey.to_ascii_uppercase());
        let mut saved = self.saved.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = saved.get(&key) {
            return Ok(hit.clone());
        }
        let name: String = subkey
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let file = self.dir.join(format!("{root:?}-{name}.hiv"));
        let out = save_key(root.hkey(), subkey, &file)?.then_some(file);
        saved.insert(key, out.clone());
        Ok(out)
    }
}

impl Drop for Snapshots {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `Ok(false)` when the key doesn't exist (hive not loaded).
fn save_key(root: HKEY, subkey: &str, file: &std::path::Path) -> io::Result<bool> {
    let wsub = wide(subkey);
    let mut h: HKEY = null_mut();
    // SAFETY: valid predefined root, NUL-terminated subkey, valid out-pointer.
    let rc = unsafe {
        RegOpenKeyExW(
            root,
            wsub.as_ptr(),
            REG_OPTION_BACKUP_RESTORE,
            KEY_READ,
            &mut h,
        )
    };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if rc != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let wfile = wide_path(file);
    // SAFETY: `h` is an open key; `wfile` is NUL-terminated and doesn't exist yet.
    let rc = unsafe { RegSaveKeyExW(h, wfile.as_ptr(), null(), REG_LATEST_FORMAT) };
    // SAFETY: we opened `h` above.
    unsafe { RegCloseKey(h) };
    if rc != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    Ok(true)
}
