//! App pack schema (SPEC §6) and validation.
//!
//! Path templates use the user's variables (`%APPDATA%`, `%LOCALAPPDATA%`,
//! `%USERPROFILE%`, …) plus `%KF:<Folder>%` for a resolved known folder,
//! e.g. `%KF:Documents%\PowerShell` (Documents may be redirected).

use serde::{Deserialize, Serialize};

use super::glob::Glob;

pub const PACK_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// Content version of this pack (bump when paths change).
    #[serde(default = "one")]
    pub version: u32,
    pub detect: Detect,
    #[serde(default)]
    pub winget_id: Option<String>,
    #[serde(default)]
    pub paths: Vec<PackPath>,
    /// HKCU keys exported (recursively) to `.reg`.
    #[serde(default)]
    pub registry: Vec<String>,
    #[serde(default)]
    pub requires_closed: Vec<String>,
    /// Empty = any Windows version.
    #[serde(default)]
    pub os: Vec<OsTag>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub post_restore: Vec<PostRestore>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detect {
    #[serde(default)]
    pub any: Vec<Cond>,
    #[serde(default)]
    pub all: Vec<Cond>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "CondMap")]
pub enum Cond {
    /// File or folder exists (template expanded per user).
    Path(String),
    /// Glob on DisplayName under the Uninstall keys (machine + user).
    UninstallName(String),
    /// HKCU key exists in the user's hive.
    Registry(String),
    /// Always matches (for Windows settings packs).
    Always(bool),
}

/// YAML form of a condition: a map with exactly one key, as in SPEC §6
/// (`- path: "..."`). serde_norway would otherwise expect `!path` tags.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CondMap {
    path: Option<String>,
    uninstall_name: Option<String>,
    registry: Option<String>,
    always: Option<bool>,
}

impl TryFrom<CondMap> for Cond {
    type Error = String;
    fn try_from(m: CondMap) -> Result<Self, String> {
        let mut found: Vec<Cond> = Vec::new();
        found.extend(m.path.map(Cond::Path));
        found.extend(m.uninstall_name.map(Cond::UninstallName));
        found.extend(m.registry.map(Cond::Registry));
        found.extend(m.always.map(Cond::Always));
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err("condition needs one of: path, uninstall_name, registry, always".into()),
            _ => Err("each condition must have exactly one key (use separate list items)".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Copy,
    /// Record what is there (e.g. extensions) instead of copying it.
    ListOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackPath {
    pub src: String,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub mode: Mode,
    /// Rewrite old profile paths inside these files on restore (M5).
    #[serde(default)]
    pub rewrite: bool,
    /// Goes to the encrypted sensitive bundle (SPEC §9), never plain.
    #[serde(default)]
    pub sensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsTag {
    Win10,
    Win11,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostRestore {
    pub run: String,
    #[serde(default)]
    pub live_only: bool,
}

/// A validated pack with compiled globs.
#[derive(Debug, Clone)]
pub struct CompiledPack {
    pub pack: Pack,
    /// Per `paths[i]`: (include, exclude).
    pub filters: Vec<(Vec<Glob>, Vec<Glob>)>,
    pub uninstall_globs: Vec<Glob>,
    /// Where it came from: "built-in" or a file path.
    pub origin: String,
}

fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_' | b'.')
        })
}

fn valid_template(s: &str) -> bool {
    let s = s.trim();
    let rooted = s.starts_with('%')
        || (s.len() > 2 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic());
    rooted && !s.split(['\\', '/']).any(|c| c == "..")
}

pub fn hkcu_subpath(key: &str) -> Option<&str> {
    let k = key.trim();
    ["HKCU\\", "HKEY_CURRENT_USER\\"]
        .iter()
        .find_map(|p| {
            k.get(..p.len())
                .filter(|h| h.eq_ignore_ascii_case(p))
                .map(|_| &k[p.len()..])
        })
        .filter(|rest| !rest.is_empty() && !rest.split('\\').any(|c| c == ".."))
}

impl Pack {
    /// Validate and compile. Returns every problem found, not just the first.
    pub fn compile(self, origin: &str) -> Result<CompiledPack, Vec<String>> {
        let mut errs = Vec::new();
        if self.schema != PACK_SCHEMA {
            errs.push(format!(
                "schema: {} is not supported (this Rebootly reads schema {PACK_SCHEMA})",
                self.schema
            ));
        }
        if !valid_id(&self.id) {
            errs.push(format!(
                "id: {:?} must be 1-64 chars of a-z, 0-9, '-', '_', '.'",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            errs.push("name: must not be empty".into());
        }
        if self.detect.any.is_empty() && self.detect.all.is_empty() {
            errs.push("detect: needs at least one condition under `any` or `all`".into());
        }
        if self.paths.is_empty() && self.registry.is_empty() {
            errs.push("pack has no `paths` and no `registry` keys: nothing to back up".into());
        }
        let mut uninstall_globs = Vec::new();
        for c in self.detect.any.iter().chain(&self.detect.all) {
            match c {
                Cond::Path(p) if !valid_template(p) => errs.push(format!(
                    "detect.path: {p:?} must start with %VAR% or a drive letter"
                )),
                Cond::Registry(k) if hkcu_subpath(k).is_none() => {
                    errs.push(format!("detect.registry: {k:?} must be an HKCU\\ key"))
                }
                Cond::UninstallName(g) => match Glob::new(g) {
                    Ok(g) => uninstall_globs.push(g),
                    Err(e) => errs.push(format!("detect.uninstall_name: {e}")),
                },
                _ => {}
            }
        }
        let mut filters = Vec::new();
        for (i, p) in self.paths.iter().enumerate() {
            if !valid_template(&p.src) {
                errs.push(format!(
                    "paths[{i}].src: {:?} must start with %VAR% or a drive letter, without '..'",
                    p.src
                ));
            }
            if p.mode == Mode::ListOnly && (!p.include.is_empty() || p.rewrite) {
                errs.push(format!(
                    "paths[{i}]: `include` and `rewrite` have no effect with mode: list_only"
                ));
            }
            let mut compile = |list: &[String], what: &str| -> Vec<Glob> {
                list.iter()
                    .filter_map(|g| {
                        Glob::new(g)
                            .map_err(|e| errs.push(format!("paths[{i}].{what}: {e}")))
                            .ok()
                    })
                    .collect()
            };
            let inc = compile(&p.include, "include");
            let exc = compile(&p.exclude, "exclude");
            filters.push((inc, exc));
        }
        for k in &self.registry {
            if hkcu_subpath(k).is_none() {
                errs.push(format!(
                    "registry: {k:?} must be an HKCU\\ key (HKLM is not supported in packs)"
                ));
            }
        }
        if errs.is_empty() {
            Ok(CompiledPack {
                pack: self,
                filters,
                uninstall_globs,
                origin: origin.into(),
            })
        } else {
            Err(errs)
        }
    }
}
