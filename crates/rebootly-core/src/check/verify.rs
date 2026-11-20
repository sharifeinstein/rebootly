//! Re-hash a backup against its manifest: every file and alternate data
//! stream the manifest says was copied, plus (with the passphrase) the
//! encrypted bundle, whose extraction checks each file's SHA-256.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use super::Progress;
use crate::backup::copy::hash_file;
use crate::backup::manifest::FileStatus;
use crate::backup::sensitive::Passphrase;
use crate::restore::{Backup, Bundle};

/// What is wrong with one file of the backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "problem")]
pub enum Problem {
    Missing,
    Size {
        expected: u64,
        found: u64,
    },
    Hash,
    Unreadable {
        error: String,
    },
    /// The manifest names a path outside the backup folder.
    UnsafePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Backup-relative path (`/`-separated).
    pub path: String,
    #[serde(flatten)]
    pub problem: Problem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum BundleCheck {
    /// The backup has no encrypted bundle.
    None,
    Missing {
        file: String,
    },
    /// There, but not opened (no passphrase given).
    NotOpened {
        file: String,
        size: u64,
    },
    /// Decrypted; every file matched its SHA-256.
    Ok {
        file: String,
        files: usize,
    },
    Failed {
        file: String,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub backup: String,
    pub job_id: String,
    pub complete: bool,
    pub files_checked: usize,
    pub streams_checked: usize,
    pub bytes_checked: u64,
    /// Copied files without a recorded hash (only the size is checked).
    pub not_hashed: usize,
    /// Files the backup itself couldn't copy (listed in its report).
    pub failed_in_backup: usize,
    pub problems: Vec<Finding>,
    /// Files in the backup folder that the manifest doesn't list.
    pub extra: Vec<String>,
    pub bundle: BundleCheck,
}

impl VerifyReport {
    /// Complete, nothing missing or changed, bundle not broken.
    pub fn is_good(&self) -> bool {
        self.complete
            && self.problems.is_empty()
            && !matches!(
                self.bundle,
                BundleCheck::Missing { .. } | BundleCheck::Failed { .. }
            )
    }
}

struct Item<'a> {
    rel: &'a str,
    size: u64,
    sha256: Option<&'a str>,
}

/// Files the backup writes besides those in the manifest.
const OWN_FILES: [&str; 4] = ["manifest.json", "report.txt", "report.html", "job.state"];

pub fn verify(
    backup: &Backup,
    pass: Option<&Passphrase>,
    progress: &mut dyn FnMut(&Progress),
) -> VerifyReport {
    let m = &backup.manifest;
    let mut items = Vec::new();
    let mut streams = 0;
    let mut failed_in_backup = 0;
    for f in &m.files {
        if f.status != FileStatus::Ok {
            failed_in_backup += usize::from(f.status == FileStatus::Failed);
            continue;
        }
        items.push(Item {
            rel: &f.path,
            size: f.size,
            sha256: f.sha256.as_deref(),
        });
        for s in f.streams.iter().filter(|s| s.status == FileStatus::Ok) {
            if let Some(p) = &s.path {
                streams += 1;
                items.push(Item {
                    rel: p,
                    size: s.size,
                    sha256: s.sha256.as_deref(),
                });
            }
        }
    }
    let mut prog = Progress {
        total_files: items.len(),
        total_bytes: items.iter().map(|i| i.size).sum(),
        ..Progress::default()
    };
    let mut problems = Vec::new();
    let mut not_hashed = 0;
    for it in &items {
        not_hashed += usize::from(it.sha256.is_none());
        if let Some(p) = check_one(backup, it) {
            problems.push(Finding {
                path: it.rel.to_owned(),
                problem: p,
            });
        }
        prog.step(it.size, progress);
    }
    let mut known: HashSet<&str> = items.iter().map(|i| i.rel).collect();
    known.extend(OWN_FILES);
    if let Some(s) = &m.sensitive {
        known.insert(&s.file);
    }
    VerifyReport {
        backup: backup.dir.display().to_string(),
        job_id: m.job_id.clone(),
        complete: m.complete,
        files_checked: items.len() - streams,
        streams_checked: streams,
        bytes_checked: prog.bytes,
        not_hashed,
        failed_in_backup,
        problems,
        extra: extra_files(&backup.dir, &known),
        bundle: check_bundle(backup, pass),
    }
}

fn check_one(backup: &Backup, it: &Item<'_>) -> Option<Problem> {
    let Some(local) = backup.local(it.rel) else {
        return Some(Problem::UnsafePath);
    };
    let meta = match std::fs::metadata(&local) {
        Ok(m) if m.is_file() => m,
        Ok(_) => return Some(Problem::Missing),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Problem::Missing),
        Err(e) => {
            return Some(Problem::Unreadable {
                error: e.to_string(),
            });
        }
    };
    if meta.len() != it.size {
        return Some(Problem::Size {
            expected: it.size,
            found: meta.len(),
        });
    }
    let want = it.sha256?;
    match hash_file(&local) {
        Ok(h) if h.eq_ignore_ascii_case(want) => None,
        Ok(_) => Some(Problem::Hash),
        Err(e) => Some(Problem::Unreadable {
            error: e.to_string(),
        }),
    }
}

fn check_bundle(backup: &Backup, pass: Option<&Passphrase>) -> BundleCheck {
    let Some(rec) = &backup.manifest.sensitive else {
        return BundleCheck::None;
    };
    let file = rec.file.clone();
    let Some(path) = backup.bundle_path() else {
        return BundleCheck::Missing { file };
    };
    let Some(pass) = pass else {
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        return BundleCheck::NotOpened { file, size };
    };
    match Bundle::open(&path, pass) {
        Ok(b) => {
            let inside: HashSet<&str> = b.index.files.iter().map(|f| f.path.as_str()).collect();
            let lost: Vec<&str> = rec
                .files
                .iter()
                .map(|f| f.path.as_str())
                .filter(|p| !inside.contains(p))
                .collect();
            if lost.is_empty() {
                BundleCheck::Ok {
                    file,
                    files: b.index.files.len(),
                }
            } else {
                BundleCheck::Failed {
                    file,
                    error: format!(
                        "{} file(s) listed in the manifest aren't inside, e.g. {}",
                        lost.len(),
                        lost[0]
                    ),
                }
            }
        }
        Err(e) => BundleCheck::Failed {
            file,
            error: e.to_string(),
        },
    }
}

/// Backup-relative paths of files under `dir` not in `known` (sorted; links
/// are not followed).
fn extra_files(dir: &Path, known: &HashSet<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push((e.path(), rel)),
                Ok(_) if !known.contains(rel.as_str()) => out.push(rel),
                _ => {}
            }
        }
    }
    out.sort();
    out
}
