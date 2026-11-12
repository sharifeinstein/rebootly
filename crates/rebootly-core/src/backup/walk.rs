//! Folder walker (SPEC §3.5): never follows reparse points, applies §3.6
//! exclusions, never aborts on an unreadable directory.
//!
//! On Windows, std reports junctions and symlinks as `is_symlink()`; on Linux
//! ntfs3/ntfs-3g expose them as symlinks too. Either way they are recorded,
//! not traversed. OneDrive files carry other reparse tags (`attrs`,
//! `source::cloud`); ntfs-3g can't read them and shows them as links: those
//! are reported as issues, not recorded as links.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use super::exclude::{Category, CloudFiles, ExcludeOptions, classify};
use crate::packs::glob::Glob;

mod attrs;
use attrs::attributes;
pub use attrs::is_cloud_placeholder;

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
    /// Pack include/exclude globs, relative to `src_root`.
    pub filter: Option<PathFilter>,
}

#[derive(Debug, Clone, Default)]
pub struct PathFilter {
    /// Empty = everything.
    pub include: Vec<Glob>,
    pub exclude: Vec<Glob>,
}

impl PathFilter {
    fn skip_dir(&self, rel: &str) -> bool {
        self.exclude.iter().any(|g| g.matches(rel))
    }
    fn skip_file(&self, rel: &str) -> bool {
        self.skip_dir(rel)
            || (!self.include.is_empty() && !self.include.iter().any(|g| g.matches(rel)))
    }
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
    /// A file the NTFS driver can't read (ntfs-3g: OneDrive files); detail
    /// is the reparse tag.
    UnsupportedReparse,
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
    /// Online-only files planned for download (`CloudFiles::Hydrate`): (count, bytes).
    pub to_download: (u64, u64),
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

/// Planned roots a walk must not enter (nested planned folders, sensitive
/// paths). Compared case-insensitively: NTFS names are, and live paths keep
/// the registry's casing while the walker sees the disk's.
#[derive(Debug, Default, Clone)]
pub struct SkipSet(HashSet<String>);

impl SkipSet {
    fn key(p: &Path) -> String {
        p.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    }
    pub fn insert(&mut self, p: &Path) {
        self.0.insert(Self::key(p));
    }
    pub fn contains(&self, p: &Path) -> bool {
        self.0.contains(&Self::key(p))
    }
}

impl FromIterator<PathBuf> for SkipSet {
    fn from_iter<I: IntoIterator<Item = PathBuf>>(it: I) -> Self {
        let mut s = SkipSet::default();
        for p in it {
            s.insert(&p);
        }
        s
    }
}

/// Walk `job`, skipping any directory or file listed in `skip_roots` (other planned
/// folders nested inside this one, so nothing is copied twice).
pub fn walk(job: &FolderJob, skip_roots: &SkipSet, opts: ExcludeOptions) -> WalkResult {
    let mut out = WalkResult::default();
    if let Ok(m) = std::fs::symlink_metadata(&job.src_root) {
        if m.is_file() {
            walk_single_file(job, &m, opts, &mut out);
            return out;
        }
    }
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
                if let Some(tag) = target.as_deref().and_then(attrs::unsupported_reparse_tag) {
                    out.issues
                        .push(issue(IssueKind::UnsupportedReparse, Some(tag.into())));
                    continue;
                }
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
            let rel_str = child_rel.join("/");
            if let Some(f) = &job.filter {
                if (is_dir && f.skip_dir(&rel_str)) || (!is_dir && f.skip_file(&rel_str)) {
                    continue;
                }
            }
            if is_dir {
                if !skip_roots.contains(&path) {
                    subdirs.push((path, child_rel));
                }
            } else if ft.is_file() && !skip_roots.contains(&path) {
                let meta = entry.metadata().ok();
                if let Some(m) = meta
                    .as_ref()
                    .filter(|m| attributes(&path, m).is_some_and(is_cloud_placeholder))
                {
                    let slot = if opts.cloud_files == CloudFiles::Hydrate {
                        &mut out.to_download
                    } else {
                        out.excluded.entry(Category::CloudPlaceholder).or_default()
                    };
                    slot.0 += 1;
                    slot.1 += m.len();
                    if opts.cloud_files == CloudFiles::Skip {
                        continue;
                    }
                }
                out.files.push(PlannedFile {
                    src_win: format!(
                        "{}\\{}",
                        job.win_root.trim_end_matches('\\'),
                        child_rel.join("\\")
                    ),
                    dest: dest_of(&child_rel),
                    size: meta.as_ref().map_or(0, |m| m.len()),
                    mtime: meta.as_ref().and_then(|m| m.modified().ok()),
                    attributes: meta.as_ref().and_then(|m| attributes(&path, m)),
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

/// A pack path that is a single file (e.g. `.gitconfig`): `dest_root` is the
/// destination file itself.
fn walk_single_file(
    job: &FolderJob,
    meta: &std::fs::Metadata,
    opts: ExcludeOptions,
    out: &mut WalkResult,
) {
    if let Some(p) = &job.profile_rel {
        if let Some(cat) = classify(p, false, true, opts) {
            let slot = out.excluded.entry(cat).or_default();
            slot.0 += 1;
            slot.1 += meta.len();
            return;
        }
    }
    out.files.push(PlannedFile {
        src: job.src_root.clone(),
        src_win: job.win_root.clone(),
        dest: job.dest_root.clone(),
        size: meta.len(),
        mtime: meta.modified().ok(),
        priority: job.priority,
        attributes: attributes(&job.src_root, meta),
    });
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
