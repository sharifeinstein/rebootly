//! The new PC as the restore sees it: the user being restored into, their
//! known-folder paths (from `SHGetKnownFolderPath` on Windows, so OneDrive
//! folder backup on the new PC is followed), and variables for pack paths.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::expand::VarSet;
use crate::folders::KNOWN_FOLDERS;
use crate::packs::schema::OsTag;

/// How Windows paths map to local paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStyle {
    /// The running Windows: a Windows path is the local path.
    Native,
    /// Simulation and tests: `C:\Users\x` is `<root>/C/Users/x`.
    Under(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Target {
    pub machine: Option<String>,
    pub os: Option<OsTag>,
    pub build: Option<u32>,
    pub username: String,
    pub sid: Option<String>,
    /// Profile folder, Windows form (`C:\Users\name`).
    pub profile: String,
    pub public: String,
    pub program_data: String,
    pub system_root: String,
    /// Known-folder name (as in `folders::KNOWN_FOLDERS`) → path.
    pub folders: BTreeMap<String, String>,
    pub vars: VarSet,
    pub paths: PathStyle,
}

impl Target {
    /// A fresh Windows 11 profile laid out under `root` (simulation/tests).
    pub fn simulated(root: &Path, username: &str) -> Self {
        let profile = format!(r"C:\Users\{username}");
        let folders = KNOWN_FOLDERS
            .iter()
            .map(|k| (k.name.to_owned(), format!(r"{profile}\{}", k.default_rel)))
            .collect();
        Target {
            machine: Some("NEW-PC".into()),
            os: Some(OsTag::Win11),
            build: Some(22631),
            username: username.into(),
            sid: None,
            profile,
            public: r"C:\Users\Public".into(),
            program_data: r"C:\ProgramData".into(),
            system_root: r"C:\Windows".into(),
            folders,
            vars: VarSet::new(),
            paths: PathStyle::Under(root.to_path_buf()),
        }
        .with_vars()
    }

    /// Fill in the variables pack templates use. Values already set (the
    /// process environment on Windows) are kept, except the profile-derived
    /// ones and `KF:*`, which always follow the fields above.
    pub fn with_vars(mut self) -> Self {
        let p = self.profile.trim_end_matches('\\').to_owned();
        let roaming = self.folder("RoamingAppData");
        let local = self.folder("LocalAppData");
        let v = &mut self.vars;
        v.set("USERPROFILE", &p)
            .set("USERNAME", &self.username)
            .set("APPDATA", roaming)
            .set("LOCALAPPDATA", local)
            .set_default("PUBLIC", &self.public)
            .set_default("ProgramData", &self.program_data)
            .set_default("ALLUSERSPROFILE", &self.program_data)
            .set_default("SystemRoot", &self.system_root)
            .set_default("windir", &self.system_root);
        if p.len() >= 2 && p.as_bytes()[1] == b':' {
            v.set("HOMEDRIVE", &p[..2]).set("HOMEPATH", &p[2..]);
            v.set_default("SystemDrive", &p[..2]);
        }
        for k in KNOWN_FOLDERS {
            let path = self
                .folders
                .get(k.name)
                .cloned()
                .unwrap_or_else(|| format!(r"{p}\{}", k.default_rel));
            self.vars.set(&format!("KF:{}", k.name), path);
        }
        self
    }

    /// Path of a known folder on this PC (default location if unknown).
    pub fn folder(&self, name: &str) -> String {
        if let Some(p) = self.folders.get(name) {
            return p.clone();
        }
        let rel = KNOWN_FOLDERS
            .iter()
            .find(|k| k.name.eq_ignore_ascii_case(name))
            .map_or(name, |k| k.default_rel);
        format!(r"{}\{rel}", self.profile.trim_end_matches('\\'))
    }

    /// Local path for a Windows path on this PC.
    pub fn local(&self, win: &str) -> PathBuf {
        match &self.paths {
            PathStyle::Native => PathBuf::from(win),
            PathStyle::Under(root) => {
                let w = win.replace('/', "\\");
                let mut out = root.clone();
                let rest = if w.len() >= 2 && w.as_bytes()[1] == b':' {
                    out.push(w[..1].to_ascii_uppercase());
                    &w[2..]
                } else if let Some(unc) = w.strip_prefix(r"\\") {
                    out.push("UNC");
                    unc
                } else {
                    &w[..]
                };
                for c in rest.split('\\').filter(|c| !c.is_empty() && *c != ".") {
                    out.push(c);
                }
                out
            }
        }
    }

    pub fn os_label(&self) -> &'static str {
        match self.os {
            Some(OsTag::Win10) => "Windows 10",
            Some(OsTag::Win11) => "Windows 11",
            None => "Windows",
        }
    }
}

/// Windows 11 starts at build 22000.
pub fn os_from_build(build: u32) -> OsTag {
    if build >= 22000 {
        OsTag::Win11
    } else {
        OsTag::Win10
    }
}

/// Case-insensitive "is `path` inside `dir`" for Windows paths.
pub fn win_within(path: &str, dir: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let (p, d) = (norm(path), norm(dir));
    p == d || p.starts_with(&format!("{d}\\"))
}

/// If `path` is inside `old`, the same path inside `new`.
pub fn win_rebase(path: &str, old: &str, new: &str) -> Option<String> {
    if !win_within(path, old) {
        return None;
    }
    let old_len = old.trim_end_matches('\\').len();
    let rest = &path[old_len.min(path.len())..];
    Some(format!("{}{rest}", new.trim_end_matches('\\')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_paths_and_vars() {
        let t = Target::simulated(Path::new("/sim"), "Sam");
        assert_eq!(
            t.local(r"C:\Users\Sam\Desktop\a.txt"),
            Path::new("/sim/C/Users/Sam/Desktop/a.txt")
        );
        assert_eq!(t.local(r"d:\x\.\y"), Path::new("/sim/D/x/y"));
        assert_eq!(
            t.vars.expand(r"%APPDATA%\Code").text,
            r"C:\Users\Sam\AppData\Roaming\Code"
        );
        assert_eq!(
            t.vars.expand("%KF:Documents%").text,
            r"C:\Users\Sam\Documents"
        );
        assert_eq!(t.folder("SavedGames"), r"C:\Users\Sam\Saved Games");
    }

    #[test]
    fn rebase_is_case_insensitive_and_whole_component() {
        assert_eq!(
            win_rebase(
                r"c:\users\alice\Projects\x",
                r"C:\Users\Alice",
                r"C:\Users\Sam"
            )
            .as_deref(),
            Some(r"C:\Users\Sam\Projects\x")
        );
        assert_eq!(
            win_rebase(r"C:\Users\Alice2\x", r"C:\Users\Alice", r"C:\Users\Sam"),
            None
        );
        assert!(win_within(r"C:\Users\Alice", r"C:\Users\Alice\"));
        assert_eq!(os_from_build(19045), OsTag::Win10);
        assert_eq!(os_from_build(22631), OsTag::Win11);
    }
}
