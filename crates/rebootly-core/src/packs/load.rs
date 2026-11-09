//! Loading packs: built-in copies compiled into the binary, overridden by
//! `*.yaml` files in a packs folder (by default `packs/` beside the binary),
//! so packs can be updated without rebuilding.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::schema::{CompiledPack, Pack};

/// Built-in packs: (file name, YAML).
pub const BUILTIN: &[(&str, &str)] = &[
    ("chrome.yaml", include_str!("../../../../packs/chrome.yaml")),
    ("edge.yaml", include_str!("../../../../packs/edge.yaml")),
    (
        "firefox.yaml",
        include_str!("../../../../packs/firefox.yaml"),
    ),
    ("vscode.yaml", include_str!("../../../../packs/vscode.yaml")),
    (
        "windows-terminal.yaml",
        include_str!("../../../../packs/windows-terminal.yaml"),
    ),
    (
        "powershell.yaml",
        include_str!("../../../../packs/powershell.yaml"),
    ),
    (
        "dev-dotfiles.yaml",
        include_str!("../../../../packs/dev-dotfiles.yaml"),
    ),
    (
        "notepadpp.yaml",
        include_str!("../../../../packs/notepadpp.yaml"),
    ),
    (
        "windows-shell.yaml",
        include_str!("../../../../packs/windows-shell.yaml"),
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackError {
    pub origin: String,
    pub problems: Vec<String>,
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "invalid pack {}:", self.origin)?;
        for p in &self.problems {
            writeln!(f, "  - {p}")?;
        }
        Ok(())
    }
}

/// Parse and validate one pack. YAML syntax errors carry line/column.
pub fn parse(text: &str, origin: &str) -> Result<CompiledPack, PackError> {
    let err = |problems| PackError {
        origin: origin.into(),
        problems,
    };
    let pack: Pack = serde_norway::from_str(text).map_err(|e| err(vec![e.to_string()]))?;
    pack.compile(origin).map_err(err)
}

pub fn parse_file(path: &Path) -> Result<CompiledPack, PackError> {
    let origin = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|e| PackError {
        origin: origin.clone(),
        problems: vec![e.to_string()],
    })?;
    parse(&text, &origin)
}

/// `packs/` next to the running executable, if it exists.
pub fn default_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.join("packs");
    dir.is_dir().then_some(dir)
}

/// Built-ins, then every `*.yaml`/`*.yml` in `dir` (same id replaces the
/// built-in). Bad files are reported, never fatal. Sorted by id.
pub fn load_all(dir: Option<&Path>) -> (Vec<CompiledPack>, Vec<PackError>) {
    let mut packs: BTreeMap<String, CompiledPack> = BTreeMap::new();
    let mut errors = Vec::new();
    for (name, text) in BUILTIN {
        match parse(text, &format!("built-in {name}")) {
            Ok(p) => {
                packs.insert(p.pack.id.clone(), p);
            }
            Err(e) => errors.push(e),
        }
    }
    let Some(dir) = dir else {
        return (packs.into_values().collect(), errors);
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| {
        p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
    });
    files.sort();
    let mut from_dir: BTreeMap<String, String> = BTreeMap::new();
    for f in files {
        match parse_file(&f) {
            Ok(p) => {
                if let Some(prev) = from_dir.insert(p.pack.id.clone(), p.origin.clone()) {
                    errors.push(PackError {
                        origin: p.origin.clone(),
                        problems: vec![format!(
                            "duplicate id {:?} (also in {prev}); this file wins",
                            p.pack.id
                        )],
                    });
                }
                packs.insert(p.pack.id.clone(), p);
            }
            Err(e) => errors.push(e),
        }
    }
    (packs.into_values().collect(), errors)
}

#[cfg(test)]
mod tests;
