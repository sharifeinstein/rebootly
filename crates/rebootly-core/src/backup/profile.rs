//! What to back up, as the TUI's Quick / Full / Custom modes describe it,
//! and saved **job profiles** on the stick (SPEC §11) for repeat backups
//! and for IT techs doing many PCs. `rebootly-cli backup --profile FILE` runs
//! the same selection from the CLI.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::exclude::ExcludeOptions;
use super::plan::{PlanOptions, TIER1};
use crate::packs::schema::CompiledPack;

/// Folder beside the backups where profiles are kept.
pub const PROFILE_DIR: &str = "rebootly-profiles";
pub const PROFILE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// User folders (Tier 1), detected app settings, system items.
    Quick,
    /// Everything detected: also Public, every drive-root folder, and keys
    /// and credentials into the encrypted bundle.
    Full,
    /// Picked by hand.
    Custom,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Quick => "Quick",
            Mode::Full => "Full",
            Mode::Custom => "Custom",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Mode::Quick => {
                "Your folders (Desktop, Documents, Pictures, …), app settings and the app list"
            }
            Mode::Full => {
                "Everything Rebootly finds: also Public, folders at the drive root, and keys/credentials (encrypted)"
            }
            Mode::Custom => "Choose folders, apps and extras yourself, with sizes",
        }
    }
}

/// The choices behind one backup. Empty `users`/`folders` mean all;
/// `packs: None` means every detected pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub mode: Mode,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub packs: Option<Vec<String>>,
    #[serde(default)]
    pub public: bool,
    /// Drive-root folder names. In Full mode, every offered one is added
    /// when planning (see [`Selection::resolve`]).
    #[serde(default)]
    pub root_folders: Vec<String>,
    #[serde(default)]
    pub custom: Vec<(String, String)>,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default = "yes")]
    pub system: bool,
    #[serde(default)]
    pub skip_dev_artifacts: bool,
}

fn yes() -> bool {
    true
}

impl Selection {
    pub fn new(mode: Mode) -> Self {
        let full = mode == Mode::Full;
        Selection {
            mode,
            users: Vec::new(),
            folders: Vec::new(),
            packs: None,
            public: full,
            root_folders: Vec::new(),
            custom: Vec::new(),
            sensitive: full,
            system: true,
            skip_dev_artifacts: false,
        }
    }

    /// Full mode takes every drive-root folder the source offers.
    pub fn resolve(&mut self, offered_root_folders: &[String]) {
        if self.mode == Mode::Full {
            for f in offered_root_folders {
                if !self.root_folders.iter().any(|r| r.eq_ignore_ascii_case(f)) {
                    self.root_folders.push(f.clone());
                }
            }
        }
    }

    /// Plan options; `all_packs` are the installed packs (built-in and
    /// `packs/`), filtered by `packs` here.
    pub fn plan_options(&self, all_packs: &[CompiledPack]) -> PlanOptions {
        let packs = all_packs
            .iter()
            .filter(|p| {
                self.packs
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&p.pack.id))
            })
            .cloned()
            .collect();
        PlanOptions {
            users: self.users.clone(),
            folders: self.folders.clone(),
            exclude: ExcludeOptions {
                dev_artifacts: self.skip_dev_artifacts,
                ..ExcludeOptions::default()
            },
            packs,
            public: self.public,
            root_folders: self.root_folders.clone(),
            custom: self.custom.clone(),
            sensitive_bundle: self.sensitive,
            no_system: !self.system,
        }
    }

    /// Tier 1 folder names in priority order (for the Custom tree).
    pub fn tier1() -> impl Iterator<Item = &'static str> {
        TIER1.iter().map(|(n, _)| *n)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProfile {
    pub schema: u32,
    pub name: String,
    pub selection: Selection,
    /// Parallel copy workers.
    #[serde(default)]
    pub jobs: Option<usize>,
}

/// File name for a profile name: letters, digits, `-` and `_` kept.
pub fn file_name(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.json", if s.is_empty() { "job" } else { &s })
}

/// Saves into `<target>/rebootly-profiles/<name>.json`.
pub fn save(target: &Path, p: &JobProfile) -> std::io::Result<PathBuf> {
    let dir = target.join(PROFILE_DIR);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(file_name(&p.name));
    let json = serde_json::to_vec_pretty(p).map_err(std::io::Error::other)?;
    super::manifest::write_atomic(&path, &json)?;
    Ok(path)
}

pub fn load(path: &Path) -> std::io::Result<JobProfile> {
    let bytes = std::fs::read(path)?;
    let p: JobProfile = serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if p.schema > PROFILE_SCHEMA {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("profile schema {} is newer than this Rebootly", p.schema),
        ));
    }
    Ok(p)
}

/// Profiles under `<target>/rebootly-profiles/`, by name. Bad files are
/// left out.
pub fn list(target: &Path) -> Vec<(PathBuf, JobProfile)> {
    let Ok(rd) = std::fs::read_dir(target.join(PROFILE_DIR)) else {
        return Vec::new();
    };
    let mut out: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| load(&p).ok().map(|j| (p, j)))
        .collect();
    out.sort_by_key(|p| p.1.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes() {
        let q = Selection::new(Mode::Quick);
        assert!(!q.public && !q.sensitive && q.system && q.packs.is_none());
        let mut f = Selection::new(Mode::Full);
        f.resolve(&["Projects".into(), "Data".into()]);
        f.resolve(&["projects".into()]);
        assert_eq!(f.root_folders, ["Projects", "Data"]);
        assert!(f.public && f.sensitive);
        let mut q2 = q.clone();
        q2.resolve(&["Projects".into()]);
        assert!(q2.root_folders.is_empty());
    }

    #[test]
    fn save_list_load() {
        let t = tempfile::tempdir().unwrap();
        let mut sel = Selection::new(Mode::Custom);
        sel.folders = vec!["Documents".into()];
        sel.packs = Some(vec!["vscode".into()]);
        let p = JobProfile {
            schema: PROFILE_SCHEMA,
            name: "Office PCs / docs".into(),
            selection: sel,
            jobs: Some(2),
        };
        let path = save(t.path(), &p).unwrap();
        assert!(path.ends_with("rebootly-profiles/Office_PCs___docs.json"));
        let all = list(t.path());
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].1, p);
        std::fs::write(t.path().join(PROFILE_DIR).join("bad.json"), "{").unwrap();
        assert_eq!(list(t.path()).len(), 1);
    }

    #[test]
    fn old_profile_defaults() {
        let p: JobProfile =
            serde_json::from_str(r#"{"schema":1,"name":"a","selection":{"mode":"quick"}}"#)
                .unwrap();
        assert!(p.selection.system && p.selection.packs.is_none());
    }
}
