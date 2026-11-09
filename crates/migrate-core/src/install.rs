//! Locating a Windows installation on a mounted volume (SPEC §3.3) and
//! reading basic OS facts from its SOFTWARE/SYSTEM hives.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::hive::{Hive, Result as HiveResult};
use crate::winpath::resolve_ci;

const SOFTWARE_REL: [&str; 3] = ["System32", "config", "SOFTWARE"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowsInstall {
    /// Directory that represents the system drive root (e.g. `/mnt/win`).
    pub volume_root: PathBuf,
    /// The `Windows` directory with its on-disk casing.
    pub windows_dir: PathBuf,
}

fn parts(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| (*s).to_owned()).collect()
}

fn has_software_hive(windows_dir: &Path) -> bool {
    let (p, exists) = resolve_ci(windows_dir, &parts(&SOFTWARE_REL));
    exists && p.is_file()
}

impl WindowsInstall {
    /// Accept either a volume root (containing `Windows\`) or the Windows
    /// directory itself. Never assumes a drive letter.
    pub fn from_path(path: &Path) -> Option<Self> {
        let (win, exists) = resolve_ci(path, &parts(&["Windows"]));
        if exists && has_software_hive(&win) {
            return Some(WindowsInstall {
                volume_root: path.to_path_buf(),
                windows_dir: win,
            });
        }
        if has_software_hive(path) {
            let root = path.parent()?.to_path_buf();
            return Some(WindowsInstall {
                volume_root: root,
                windows_dir: path.to_path_buf(),
            });
        }
        None
    }

    /// Scan each root and its immediate subdirectories (e.g. `/mnt/*` or
    /// every drive letter in WinPE). May return several installs.
    pub fn scan(roots: &[PathBuf]) -> Vec<Self> {
        let mut found = Vec::new();
        for root in roots {
            let mut candidates = vec![root.clone()];
            if let Ok(rd) = std::fs::read_dir(root) {
                candidates.extend(
                    rd.filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.is_dir()),
                );
            }
            for c in candidates {
                if let Some(inst) = Self::from_path(&c) {
                    if !found.contains(&inst) {
                        found.push(inst);
                    }
                }
            }
        }
        found
    }

    /// Path of a hive in `Windows\System32\config` (case-insensitive).
    pub fn config_hive(&self, name: &str) -> PathBuf {
        resolve_ci(&self.windows_dir, &parts(&["System32", "config", name])).0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OsInfo {
    pub product_name: Option<String>,
    pub display_version: Option<String>,
    pub edition_id: Option<String>,
    pub build: Option<u32>,
    pub ubr: Option<u32>,
    pub system_root: Option<String>,
    pub computer_name: Option<String>,
}

impl OsInfo {
    /// Windows 11 still reports "Windows 10" in ProductName; fix by build.
    pub fn friendly_name(&self) -> String {
        let name = self
            .product_name
            .clone()
            .unwrap_or_else(|| "Windows (unknown)".into());
        match self.build {
            Some(b) if b >= 22000 && name.contains("Windows 10") => {
                name.replace("Windows 10", "Windows 11")
            }
            _ => name,
        }
    }

    /// System drive letter from `SystemRoot` (e.g. `C:\WINDOWS` → `C`).
    pub fn system_drive(&self) -> char {
        self.system_root
            .as_deref()
            .and_then(|s| crate::winpath::WinPath::parse(s).drive())
            .unwrap_or('C')
    }
}

pub(crate) const CURRENT_VERSION: &str = r"Microsoft\Windows NT\CurrentVersion";

pub fn read_os_info(software: &Hive, system: Option<&Hive>) -> HiveResult<OsInfo> {
    let mut info = OsInfo::default();
    if let Some(cv) = software.open_key(CURRENT_VERSION)? {
        info.product_name = cv.string("ProductName")?;
        info.display_version = match cv.string("DisplayVersion")? {
            Some(v) => Some(v),
            None => cv.string("ReleaseId")?,
        };
        info.edition_id = cv.string("EditionID")?;
        info.build = cv
            .string("CurrentBuildNumber")?
            .or(cv.string("CurrentBuild")?)
            .and_then(|s| s.trim().parse().ok());
        info.ubr = cv.value("UBR")?.and_then(|v| v.as_dword());
        info.system_root = cv.string("SystemRoot")?;
    }
    if let Some(sys) = system {
        if let Some(cs) = current_control_set(sys)? {
            info.computer_name = sys
                .open_key(&format!(r"{cs}\Control\ComputerName\ComputerName"))?
                .map(|k| k.string("ComputerName"))
                .transpose()?
                .flatten();
        }
    }
    Ok(info)
}

/// `ControlSet00N` named by `SYSTEM\Select\Current` (offline there is no
/// `CurrentControlSet` link).
pub fn current_control_set(system: &Hive) -> HiveResult<Option<String>> {
    let n = system
        .open_key("Select")?
        .map(|k| k.value("Current"))
        .transpose()?
        .flatten()
        .and_then(|v| v.as_dword());
    Ok(n.map(|n| format!("ControlSet{n:03}")))
}
