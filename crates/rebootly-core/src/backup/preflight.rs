//! Pre-flight checks (SPEC §3.5): sizes per folder, free space, FAT32 4 GiB
//! limit, name problems, and never writing to the source volume.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::walk::{IssueKind, PathIssue, PlannedFile};

pub const FAT32_MAX: u64 = (4 << 30) - 1;

#[derive(Debug, Clone, Serialize)]
pub struct FolderSize {
    pub folder: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub folders: Vec<FolderSize>,
    pub total_files: u64,
    pub total_bytes: u64,
    /// Bytes already on the target from a previous run of this job.
    pub resumed_bytes: u64,
    pub target_free: Option<u64>,
    pub target_fs: Option<String>,
    pub warnings: Vec<String>,
    /// Problems that stop the backup unless forced.
    pub blockers: Vec<String>,
}

/// Filesystem type of the mount holding `path` (Linux: /proc/self/mountinfo).
/// Elsewhere returns `None`; on Windows the caller passes rebootly-win's
/// answer to [`check`].
pub fn filesystem_type(path: &Path) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let path = path.canonicalize().ok()?;
        let info = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
        let mut best: Option<(usize, String)> = None;
        for line in info.lines() {
            let Some((left, right)) = line.split_once(" - ") else {
                continue;
            };
            let Some(mount) = left.split(' ').nth(4) else {
                continue;
            };
            let mount = mount.replace("\\040", " ");
            let fstype = right.split(' ').next().unwrap_or_default().to_owned();
            if path.starts_with(&mount) && best.as_ref().is_none_or(|(l, _)| mount.len() >= *l) {
                best = Some((mount.len(), fstype));
            }
        }
        best.map(|(_, t)| t)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        None
    }
}

fn is_fat32(fs: &str) -> bool {
    matches!(
        fs.to_ascii_lowercase().as_str(),
        "vfat" | "msdos" | "fat" | "fat32"
    )
}

/// Human-readable size: B, KiB, MiB, GiB, TiB.
pub fn gib(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// `todo` are the files still to copy; `labels` maps priority → folder name.
pub fn check(
    files: &[PlannedFile],
    todo_bytes: u64,
    labels: &BTreeMap<u32, String>,
    issues: &[PathIssue],
    target: &Path,
    target_fs: Option<String>,
    source_root: &Path,
) -> Preflight {
    let mut by_folder: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    for f in files {
        let e = by_folder.entry(f.priority).or_default();
        e.0 += 1;
        e.1 += f.size;
    }
    let folders = by_folder
        .iter()
        .map(|(p, (n, b))| FolderSize {
            folder: labels.get(p).cloned().unwrap_or_else(|| format!("#{p}")),
            files: *n,
            bytes: *b,
        })
        .collect();
    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let target_free = fs4::available_space(target).ok();
    let target_fs = target_fs.or_else(|| filesystem_type(target));
    let mut warnings = Vec::new();
    let mut blockers = Vec::new();

    if let (Ok(t), Ok(s)) = (target.canonicalize(), source_root.canonicalize()) {
        if t.starts_with(&s) {
            blockers.push(format!(
                "the target {} is on the source volume; Rebootly never writes to the source disk",
                t.display()
            ));
        }
    }
    if let Some(free) = target_free {
        if free < todo_bytes {
            blockers.push(format!(
                "not enough free space on the target: need {}, have {}",
                gib(todo_bytes),
                gib(free)
            ));
        }
    }
    let big: Vec<&PlannedFile> = files.iter().filter(|f| f.size > FAT32_MAX).collect();
    if !big.is_empty() {
        let largest = big.iter().map(|f| f.size).max().unwrap_or(0);
        match target_fs.as_deref() {
            Some(fs) if is_fat32(fs) => warnings.push(format!(
                "target is FAT32: {} file(s) over 4 GiB (largest {}) will fail. Reformat the target as exFAT or NTFS.",
                big.len(),
                gib(largest)
            )),
            Some(_) => {}
            None => warnings.push(format!(
                "{} file(s) over 4 GiB; if the target is FAT32 they will fail (use exFAT or NTFS)",
                big.len()
            )),
        }
    }
    let count = |k| issues.iter().filter(|i| i.kind == k).count();
    for (kind, text) in [
        (
            IssueKind::Unreadable,
            "folder(s) could not be read and will be missing",
        ),
        (
            IssueKind::NameTooLong,
            "name(s) longer than 255 bytes may fail on Linux/NAS targets",
        ),
        (
            IssueKind::InvalidWindowsName,
            "name(s) Windows can't store; they may not restore to Windows",
        ),
        (
            IssueKind::CaseCollision,
            "name(s) differ only by case; one may overwrite the other on a case-insensitive target",
        ),
    ] {
        let n = count(kind);
        if n > 0 {
            warnings.push(format!("{n} {text}"));
        }
    }
    Preflight {
        folders,
        total_files: files.len() as u64,
        total_bytes,
        resumed_bytes: total_bytes.saturating_sub(todo_bytes),
        target_free,
        target_fs,
        warnings,
        blockers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pf(size: u64) -> PlannedFile {
        PlannedFile {
            src: PathBuf::new(),
            src_win: String::new(),
            dest: "x".into(),
            size,
            mtime: None,
            priority: 10,
            attributes: None,
        }
    }

    #[test]
    fn sizes_and_blockers() {
        let t = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let labels = BTreeMap::from([(10, "Desktop".to_owned())]);
        let files = [pf(10), pf(FAT32_MAX + 1)];
        let p = check(&files, 10, &labels, &[], t.path(), None, src.path());
        assert_eq!(p.folders[0].folder, "Desktop");
        assert_eq!(p.total_files, 2);
        assert_eq!(p.resumed_bytes, FAT32_MAX + 1);
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.target_free.is_some());
        let huge = check(&files, u64::MAX, &labels, &[], t.path(), None, src.path());
        assert!(huge.blockers.iter().any(|b| b.contains("free space")));
    }

    #[test]
    fn refuses_target_on_source_volume() {
        let src = tempfile::tempdir().unwrap();
        let inside = src.path().join("backup");
        std::fs::create_dir(&inside).unwrap();
        let p = check(&[], 0, &BTreeMap::new(), &[], &inside, None, src.path());
        assert!(
            p.blockers
                .iter()
                .any(|b| b.contains("never writes to the source"))
        );
    }

    #[test]
    fn human_sizes() {
        assert_eq!(gib(512), "512 B");
        assert_eq!(gib(1536), "1.5 KiB");
        assert_eq!(gib(FAT32_MAX + 1), "4.0 GiB");
    }

    #[test]
    fn fat32_detection() {
        assert!(is_fat32("vfat") && is_fat32("FAT32") && !is_fat32("exfat"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_some_filesystem_type() {
        assert!(filesystem_type(Path::new("/")).is_some());
    }
}
