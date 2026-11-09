//! Folder walker (SPEC §3.5): never follows reparse points, applies §3.6
//! exclusions, never aborts on an unreadable directory.
//!
//! On Windows, std reports junctions and symlinks as `is_symlink()`; on Linux
//! ntfs3/ntfs-3g expose them as symlinks too. Either way they are recorded,
//! not traversed. (Cloud placeholders are other reparse tags: M4.)

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use super::exclude::{Category, ExcludeOptions, classify};

/// One folder to back up.
#[derive(Debug, Clone)]
pub struct FolderJob {
    pub user: String,
    /// Known-folder name, or a custom label.
    pub folder: String,
    pub src_root: PathBuf,
    /// Windows path of `src_root` on the source machine (for restore).
    pub win_root: String,
    /// Path of `src_root` relative to the profile, for AppData-aware rules.
    pub profile_rel: Option<Vec<String>>,
    /// Destination root inside the backup, `/`-separated.
    pub dest_root: String,
    /// Sort key from the priority order (lower first).
    pub priority: u32,
}

#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub src: PathBuf,
    /// Windows path on the source machine.
    pub src_win: String,
    pub dest: String,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub priority: u32,
    /// Windows file attributes when the platform exposes them.
    pub attributes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReparsePoint {
    pub dest: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// A path component longer than 255 bytes (fails on Linux/most NAS).
    NameTooLong,
    /// Characters Windows can't store (`<>:"|?*`, control, trailing dot/space).
    InvalidWindowsName,
    /// Names in one directory that differ only by case.
    CaseCollision,
    /// Directory could not be listed.
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathIssue {
    pub path: String,
    pub kind: IssueKind,
    pub detail: Option<String>,
}

#[derive(Debug, Default)]
pub struct WalkResult {
    pub files: Vec<PlannedFile>,
    pub reparse_points: Vec<ReparsePoint>,
    /// category -> (count, bytes)
    pub excluded: BTreeMap<Category, (u64, u64)>,
    pub issues: Vec<PathIssue>,
}

fn invalid_on_windows(name: &str) -> bool {
    name.chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\') || (c as u32) < 32)
        || name.ends_with('.')
        || name.ends_with(' ')
}

/// Linux and most NAS filesystems cap a name at 255 *bytes*; a 255-character
/// Windows name in UTF-8 can be up to 765 bytes.
pub fn name_too_long(name: &str) -> bool {
    name.len() > 255
}

#[cfg(windows)]
fn attributes(m: &std::fs::Metadata) -> Option<u32> {
    use std::os::windows::fs::MetadataExt;
    Some(m.file_attributes())
}
#[cfg(not(windows))]
fn attributes(_: &std::fs::Metadata) -> Option<u32> {
    None
}

/// Walk `job`, skipping any directory listed in `skip_roots` (other planned
/// folders nested inside this one, so nothing is copied twice).
pub fn walk(job: &FolderJob, skip_roots: &HashSet<PathBuf>, opts: ExcludeOptions) -> WalkResult {
    let mut out = WalkResult::default();
    // (dir on disk, components relative to src_root)
    let mut stack: Vec<(PathBuf, Vec<String>)> = vec![(job.src_root.clone(), Vec::new())];
    while let Some((dir, rel)) = stack.pop() {
        let dest_of = |rel: &[String]| {
            if rel.is_empty() {
                job.dest_root.clone()
            } else {
                format!("{}/{}", job.dest_root, rel.join("/"))
            }
        };
        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
            Err(e) => {
                out.issues.push(PathIssue {
                    path: dir.display().to_string(),
                    kind: IssueKind::Unreadable,
                    detail: Some(e.to_string()),
                });
                continue;
            }
        };
        entries.sort_by_key(|e| e.file_name());

        let mut seen_upper: HashSet<String> = HashSet::new();
        let mut subdirs = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let mut child_rel = rel.clone();
            child_rel.push(name.clone());
            let issue = |kind, detail: Option<String>| PathIssue {
                path: path.display().to_string(),
                kind,
                detail,
            };
            if !seen_upper.insert(name.to_uppercase()) {
                out.issues.push(issue(IssueKind::CaseCollision, None));
            }
            if name_too_long(&name) {
                out.issues.push(issue(
                    IssueKind::NameTooLong,
                    Some(format!("{} bytes", name.len())),
                ));
            }
            if invalid_on_windows(&name) {
                out.issues.push(issue(IssueKind::InvalidWindowsName, None));
            }

            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                let target = std::fs::read_link(&path)
                    .ok()
                    .map(|t| t.display().to_string());
                out.reparse_points.push(ReparsePoint {
                    dest: dest_of(&child_rel),
                    target,
                });
                continue;
            }
            let is_dir = ft.is_dir();
            let ctx: Vec<String> = match &job.profile_rel {
                Some(p) => p.iter().chain(child_rel.iter()).cloned().collect(),
                None => child_rel.clone(),
            };
            if let Some(cat) = classify(&ctx, is_dir, job.profile_rel.is_some(), opts) {
                let size = if is_dir {
                    0
                } else {
                    entry.metadata().map(|m| m.len()).unwrap_or(0)
                };
                let slot = out.excluded.entry(cat).or_default();
                slot.0 += 1;
                slot.1 += size;
                continue;
            }
            if is_dir {
                if !skip_roots.contains(&path) {
                    subdirs.push((path, child_rel));
                }
            } else if ft.is_file() {
                let meta = entry.metadata().ok();
                out.files.push(PlannedFile {
                    src_win: format!(
                        "{}\\{}",
                        job.win_root.trim_end_matches('\\'),
                        child_rel.join("\\")
                    ),
                    dest: dest_of(&child_rel),
                    size: meta.as_ref().map_or(0, |m| m.len()),
                    mtime: meta.as_ref().and_then(|m| m.modified().ok()),
                    attributes: meta.as_ref().and_then(attributes),
                    priority: job.priority,
                    src: path,
                });
            }
        }
        // Reverse so the stack pops in sorted order (deterministic output).
        stack.extend(subdirs.into_iter().rev());
    }
    out
}

/// Components of `path` relative to `base`, if it is inside it.
pub fn rel_components(path: &Path, base: &Path) -> Option<Vec<String>> {
    let rel = path.strip_prefix(base).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect(),
    )
}

#[cfg(test)]
mod tests;
