//! Installed-app inventory from the Uninstall keys (machine, 32-bit machine,
//! and per-user). Read from hives in every mode, so it also works offline.

use serde::Serialize;

use crate::hive::{Hive, Result as HiveResult};

pub const UNINSTALL: &str = r"Microsoft\Windows\CurrentVersion\Uninstall";
pub const UNINSTALL_WOW64: &str = r"WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
pub const USER_UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppScope {
    Machine,
    Machine32,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub install_location: Option<String>,
    pub scope: AppScope,
    /// Uninstall subkey name (often a product GUID).
    pub key: String,
}

/// Read one Uninstall key. Skips entries without a DisplayName, system
/// components and updates (`ParentKeyName`), like Programs and Features.
pub fn read_uninstall(hive: &Hive, path: &str, scope: AppScope) -> HiveResult<Vec<InstalledApp>> {
    let Some(root) = hive.open_key(path)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for k in root.subkeys()? {
        let Some(name) = k.string("DisplayName")?.filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        let system = k.value("SystemComponent")?.and_then(|v| v.as_dword()) == Some(1);
        if system || k.value("ParentKeyName")?.is_some() {
            continue;
        }
        out.push(InstalledApp {
            name: name.trim().to_owned(),
            version: k.string("DisplayVersion")?,
            publisher: k.string("Publisher")?,
            install_location: k.string("InstallLocation")?.filter(|s| !s.is_empty()),
            scope,
            key: k.name().to_owned(),
        });
    }
    Ok(out)
}

/// Machine-wide apps from the SOFTWARE hive (64-bit and 32-bit views).
pub fn machine_apps(software: &Hive) -> HiveResult<Vec<InstalledApp>> {
    let mut v = read_uninstall(software, UNINSTALL, AppScope::Machine)?;
    v.extend(read_uninstall(
        software,
        UNINSTALL_WOW64,
        AppScope::Machine32,
    )?);
    v.sort_by_key(|a| a.name.to_lowercase());
    Ok(v)
}

pub fn user_apps(ntuser: &Hive) -> HiveResult<Vec<InstalledApp>> {
    read_uninstall(ntuser, USER_UNINSTALL, AppScope::User)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hive::writer::HiveWriter;

    #[test]
    fn reads_and_filters_entries() {
        let mut w = HiveWriter::new("ROOT");
        w.key(&format!(r"{UNINSTALL}\Google Chrome"))
            .sz("DisplayName", "Google Chrome")
            .sz("DisplayVersion", "131.0")
            .sz("InstallLocation", "");
        w.key(&format!(r"{UNINSTALL}\{{GUID-1}}"))
            .sz("DisplayName", "Runtime")
            .dword("SystemComponent", 1);
        w.key(&format!(r"{UNINSTALL}\KB123"))
            .sz("DisplayName", "Update")
            .sz("ParentKeyName", "Office");
        w.key(&format!(r"{UNINSTALL}\NoName"));
        w.key(&format!(r"{UNINSTALL_WOW64}\Notepad++"))
            .sz("DisplayName", "Notepad++ (32-bit x86)");
        let h = Hive::from_bytes(w.to_bytes()).unwrap();
        let apps = machine_apps(&h).unwrap();
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Google Chrome", "Notepad++ (32-bit x86)"]);
        assert_eq!(apps[0].install_location, None);
        assert_eq!(apps[1].scope, AppScope::Machine32);
    }
}
