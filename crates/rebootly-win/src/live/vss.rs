//! VSS shadow copies for files other programs hold locked (SPEC §3.5).
//!
//! Made lazily, once per volume, the first time a file on it turns out to be
//! locked, through WMI `Win32_ShadowCopy.Create` (context ClientAccessible,
//! which client Windows supports; `vssadmin create` is Server-only). Deleted
//! when the backup ends. If the process is killed first, the shadow IDs are
//! still listed in `%TEMP%\rebootly-vss-<pid>.txt`, and the next live run
//! deletes those whose process is gone.

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rebootly_core::LOCKED_HINT;
use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::cmd::{powershell, run_cmd};
use super::volume::volume_root;
use crate::parse::{below_root, is_braced_guid, vss_created};

#[derive(Debug, Default)]
pub struct Vss {
    /// Volume root (lower case) → shadow device, or why there is none.
    shadows: Mutex<HashMap<String, Result<String, String>>>,
    ids: Mutex<Vec<String>>,
}

fn ledger(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("rebootly-vss-{pid}.txt"))
}

fn process_alive(pid: u32) -> bool {
    // SAFETY: plain Win32 calls; the handle is closed before returning.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

fn delete_shadow(id: &str) {
    if is_braced_guid(id) {
        let script = format!(
            "Get-CimInstance -ClassName Win32_ShadowCopy -Filter \"ID='{id}'\" | Remove-CimInstance"
        );
        let _ = run_cmd(&mut powershell(&script));
    }
}

/// Delete shadows left by live runs that were killed.
fn clean_leftovers() {
    let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = name
            .strip_prefix("rebootly-vss-")
            .and_then(|s| s.strip_suffix(".txt"))
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() || process_alive(pid) {
            continue;
        }
        if let Ok(txt) = std::fs::read_to_string(e.path()) {
            txt.lines().for_each(delete_shadow);
        }
        let _ = std::fs::remove_file(e.path());
    }
}

fn create(root: &str) -> Result<(String, String), String> {
    let vol = root.replace('\'', "''");
    let script = format!(
        "$r = Invoke-CimMethod -ClassName Win32_ShadowCopy -MethodName Create \
         -Arguments @{{Volume='{vol}'; Context='ClientAccessible'}}; \
         if ($r.ReturnValue -ne 0) {{ [Console]::Out.Write(\"ERR $($r.ReturnValue)\"); exit 0 }}; \
         $s = Get-CimInstance -ClassName Win32_ShadowCopy -Filter \"ID='$($r.ShadowID)'\"; \
         [Console]::Out.Write(\"OK $($r.ShadowID) $($s.DeviceObject)\")"
    );
    let out = run_cmd(&mut powershell(&script))?;
    vss_created(&String::from_utf8_lossy(&out))
}

impl Vss {
    pub fn new() -> Self {
        clean_leftovers();
        Vss::default()
    }

    fn device(&self, root: &str) -> Result<String, String> {
        let mut g = self.shadows.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = g.get(&root.to_lowercase()) {
            return r.clone();
        }
        let r = create(root).map(|(id, dev)| {
            let mut ids = self.ids.lock().unwrap_or_else(|e| e.into_inner());
            ids.push(id);
            let _ = std::fs::write(ledger(std::process::id()), ids.join("\r\n"));
            dev
        });
        g.insert(root.to_lowercase(), r.clone());
        r
    }

    /// Where `path` is inside a shadow copy of its volume (made on first use).
    /// Errors are `Unsupported`, so the copy engine doesn't retry them.
    pub fn path_in_snapshot(&self, path: &Path) -> io::Result<PathBuf> {
        let fail = |why: String| io::Error::new(io::ErrorKind::Unsupported, why);
        let root = volume_root(path)?;
        let p = path.to_string_lossy();
        let rel = below_root(&p, &root)
            .ok_or_else(|| fail(format!("{LOCKED_HINT} ({p} is not below {root})")))?;
        let dev = self
            .device(&root)
            .map_err(|e| fail(format!("{LOCKED_HINT} (no shadow copy of {root}: {e})")))?;
        Ok(PathBuf::from(format!(r"{dev}\{rel}")))
    }
}

impl Drop for Vss {
    fn drop(&mut self) {
        let ids = std::mem::take(self.ids.get_mut().unwrap_or_else(|e| e.into_inner()));
        if ids.is_empty() {
            return;
        }
        ids.iter().for_each(|id| delete_shadow(id));
        let _ = std::fs::remove_file(ledger(std::process::id()));
    }
}
