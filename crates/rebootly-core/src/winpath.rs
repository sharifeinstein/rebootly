//! Windows path parsing and mapping onto a mounted volume.
//!
//! Registry paths are Windows-style (`C:\Users\bob`). Offline, the system
//! drive is a directory such as `/mnt/win`. We map `<SystemDrive>:\...` onto
//! that root and resolve each component case-insensitively, because NTFS is
//! case-insensitive but Linux mounts may not be.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WinPath {
    /// `X:\a\b` → drive `X`, components `[a, b]`.
    Drive { letter: char, parts: Vec<String> },
    /// `\\server\share\...`
    Unc(String),
    /// Anything else (relative, empty, still contains `%VAR%`, ...).
    Other(String),
}

impl Mapped {
    pub fn local_path(&self) -> Option<&Path> {
        match self {
            Mapped::Local { path, .. } => Some(path),
            _ => None,
        }
    }

    pub fn exists(&self) -> bool {
        matches!(self, Mapped::Local { exists: true, .. })
    }
}

impl WinPath {
    pub fn parse(s: &str) -> Self {
        let s = s.trim().trim_matches('"');
        let s = s.strip_prefix(r"\\?\").unwrap_or(s);
        if s.starts_with(r"\\") {
            return WinPath::Unc(s.to_owned());
        }
        let mut chars = s.chars();
        match (chars.next(), chars.next(), chars.next()) {
            (Some(l), Some(':'), Some('\\') | Some('/') | None) if l.is_ascii_alphabetic() => {
                let parts = s[2..]
                    .split(['\\', '/'])
                    .filter(|p| !p.is_empty() && *p != ".")
                    .map(str::to_owned)
                    .collect();
                WinPath::Drive {
                    letter: l.to_ascii_uppercase(),
                    parts,
                }
            }
            _ => WinPath::Other(s.to_owned()),
        }
    }

    pub fn drive(&self) -> Option<char> {
        match self {
            WinPath::Drive { letter, .. } => Some(*letter),
            _ => None,
        }
    }
}

/// Where a Windows path lands on the local filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mapped {
    /// Mapped onto the volume; `exists` reflects a case-insensitive lookup.
    Local { path: PathBuf, exists: bool },
    /// On a different drive letter than the mounted system volume.
    OtherDrive { drive: char },
    /// Network path; never followed offline.
    Network,
    /// Could not be interpreted.
    Unmappable,
}

/// Map a Windows path onto `volume_root`, which holds drive `system_drive`.
pub fn map_to_volume(win: &str, system_drive: char, volume_root: &Path) -> Mapped {
    match WinPath::parse(win) {
        WinPath::Drive { letter, parts } if letter == system_drive.to_ascii_uppercase() => {
            if parts.iter().any(|p| p == "..") {
                return Mapped::Unmappable; // never escape the volume root
            }
            let (path, exists) = resolve_ci(volume_root, &parts);
            Mapped::Local { path, exists }
        }
        WinPath::Drive { letter, .. } => Mapped::OtherDrive { drive: letter },
        WinPath::Unc(_) => Mapped::Network,
        WinPath::Other(_) => Mapped::Unmappable,
    }
}

/// Map a Windows path on the running system itself (live mode): every drive
/// letter is real, so the path is used as it is (normalised to `X:\a\b`).
/// Network paths are not followed; `..` is refused.
pub fn map_live(win: &str) -> Mapped {
    match WinPath::parse(win) {
        WinPath::Drive { letter, parts } => {
            if parts.iter().any(|p| p == "..") {
                return Mapped::Unmappable;
            }
            let path = PathBuf::from(format!("{letter}:\\{}", parts.join("\\")));
            let exists = path.exists();
            Mapped::Local { path, exists }
        }
        WinPath::Unc(_) => Mapped::Network,
        WinPath::Other(_) => Mapped::Unmappable,
    }
}

/// Resolve `parts` under `root`, matching each component case-insensitively.
/// Returns the best path (real casing where found) and whether it exists.
/// Symlinked components are not followed (reparse points: SPEC §3.5).
pub fn resolve_ci(root: &Path, parts: &[String]) -> (PathBuf, bool) {
    let mut cur = root.to_path_buf();
    let mut found = true;
    for (i, part) in parts.iter().enumerate() {
        let exact = cur.join(part);
        let is_last = i + 1 == parts.len();
        if found {
            if let Some(real) = find_entry_ci(&cur, part, is_last) {
                cur = real;
                continue;
            }
        }
        found = false;
        cur = exact;
    }
    (cur, found)
}

fn find_entry_ci(dir: &Path, name: &str, allow_file: bool) -> Option<PathBuf> {
    let ok = |p: &Path| match std::fs::symlink_metadata(p) {
        Ok(m) => !m.file_type().is_symlink() && (allow_file || m.is_dir()),
        Err(_) => false,
    };
    let exact = dir.join(name);
    if ok(&exact) {
        return Some(exact);
    }
    let want = name.to_uppercase();
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().to_uppercase() == want)
                && ok(p)
        })
}

/// Last path component of a Windows path (e.g. user folder name).
pub fn leaf(win: &str) -> Option<String> {
    match WinPath::parse(win) {
        WinPath::Drive { parts, .. } => parts.last().cloned(),
        WinPath::Unc(s) | WinPath::Other(s) => s
            .rsplit(['\\', '/'])
            .find(|p| !p.is_empty())
            .map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_mapping_keeps_every_drive() {
        let m = map_live(r"d:\Data\Work\");
        assert_eq!(m.local_path(), Some(Path::new(r"D:\Data\Work")));
        assert!(matches!(
            map_live(r"C:\Users\..\Windows"),
            Mapped::Unmappable
        ));
        assert!(matches!(map_live(r"\\nas\home"), Mapped::Network));
    }

    #[test]
    fn parses_forms() {
        assert_eq!(
            WinPath::parse(r"c:\Users\Bob\"),
            WinPath::Drive {
                letter: 'C',
                parts: vec!["Users".into(), "Bob".into()]
            }
        );
        assert_eq!(WinPath::parse(r"\\?\D:\x").drive(), Some('D'));
        assert!(matches!(WinPath::parse(r"\\nas\home"), WinPath::Unc(_)));
        assert!(matches!(
            WinPath::parse(r"%USERPROFILE%\x"),
            WinPath::Other(_)
        ));
        assert_eq!(WinPath::parse("C:").drive(), Some('C'));
    }

    #[test]
    fn maps_case_insensitively() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("Users/alice/Documents")).unwrap();
        let m = map_to_volume(r"C:\USERS\Alice\documents", 'c', t.path());
        assert_eq!(
            m,
            Mapped::Local {
                path: t.path().join("Users/alice/Documents"),
                exists: true
            }
        );
        let m = map_to_volume(r"C:\Users\alice\Nope\x", 'C', t.path());
        assert_eq!(
            m,
            Mapped::Local {
                path: t.path().join("Users/alice/Nope/x"),
                exists: false
            }
        );
        assert_eq!(
            map_to_volume(r"D:\Pics", 'C', t.path()),
            Mapped::OtherDrive { drive: 'D' }
        );
        assert_eq!(map_to_volume(r"\\nas\x", 'C', t.path()), Mapped::Network);
        assert_eq!(
            map_to_volume(r"C:\..\etc", 'C', t.path()),
            Mapped::Unmappable
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("real")).unwrap();
        std::os::unix::fs::symlink(t.path().join("real"), t.path().join("link")).unwrap();
        let (_, exists) = resolve_ci(t.path(), &["LINK".into()]);
        assert!(!exists);
    }

    #[test]
    fn leaf_names() {
        assert_eq!(
            leaf(r"C:\Users\carol.CONTOSO").as_deref(),
            Some("carol.CONTOSO")
        );
        assert_eq!(leaf(r"%SystemDrive%\Users\x\").as_deref(), Some("x"));
    }
}
