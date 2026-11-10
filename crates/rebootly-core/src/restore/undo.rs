//! Undo (SPEC §5): every change is written to a journal *before* it is
//! made, and every file that gets replaced is copied first, to
//! `%USERPROFILE%\.rebootly-undo\<job>_<time>\`. `undo` walks it backwards.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::host::{RestoreHost, SysOp};
use super::target::Target;
use crate::backup::copy::hash_file;

pub const UNDO_DIR: &str = ".rebootly-undo";
pub const JOURNAL: &str = "journal.jsonl";
/// Written once a run has been undone completely.
pub const UNDONE: &str = "undone.txt";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum UndoEntry {
    Header {
        job_id: String,
        backup: String,
        started: String,
        user: String,
        profile: String,
    },
    /// A folder the restore created (removed on undo if empty).
    Mkdir {
        path: String,
    },
    /// A new file (deleted on undo if unchanged since).
    Create {
        path: String,
        sha256: String,
    },
    /// A replaced file; the old one is at `saved` (relative to the undo folder).
    Overwrite {
        path: String,
        saved: String,
        sha256: String,
    },
    /// A registry key exported before import (`saved` is None if it didn't exist).
    Reg {
        key: String,
        saved: Option<String>,
    },
    Power {
        /// `None` when an existing plan was only made active.
        imported: Option<String>,
        previous: Option<String>,
    },
    Wifi {
        name: String,
    },
    Cert {
        thumbprint: String,
    },
    Task {
        name: String,
    },
}

/// Append-only, fsynced per entry.
#[derive(Debug)]
pub struct Journal {
    pub dir: PathBuf,
    file: File,
    saved: u32,
}

impl Journal {
    pub fn create(dir: &Path, header: UndoEntry) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir.join("files"))?;
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(dir.join(JOURNAL))?;
        let mut j = Journal {
            dir: dir.to_path_buf(),
            file,
            saved: 0,
        };
        j.append(&header)?;
        Ok(j)
    }

    pub fn append(&mut self, e: &UndoEntry) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(e)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.sync_data()
    }

    /// Copy a file that is about to be replaced; returns its name in the
    /// undo folder. Numbered, so long paths don't matter.
    pub fn save(&mut self, local: &Path) -> std::io::Result<String> {
        self.saved += 1;
        let rel = format!("files/{:06}", self.saved);
        let to = self.dir.join(&rel);
        std::fs::copy(local, &to)?;
        // One writable handle for both: Windows refuses to flush (and to
        // set times on) a file opened read-only.
        let f = File::options().write(true).open(&to)?;
        if let Ok(t) = std::fs::metadata(local).and_then(|m| m.modified()) {
            let _ = f.set_modified(t);
        }
        f.sync_all()?;
        Ok(rel)
    }

    /// A new path under `reg/` (exported keys, the checked file imported):
    /// (absolute, relative to the undo folder).
    pub fn slot(&mut self, ext: &str) -> std::io::Result<(PathBuf, String)> {
        self.saved += 1;
        std::fs::create_dir_all(self.dir.join("reg"))?;
        let rel = format!("reg/{:06}.{ext}", self.saved);
        Ok((self.dir.join(&rel), rel))
    }
}

/// `a.txt` → `a.txt<suffix>` (unlike `with_extension`, never collides with `a.<suffix>`).
pub fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

pub fn read_journal(dir: &Path) -> std::io::Result<Vec<UndoEntry>> {
    let f = File::open(dir.join(JOURNAL))?;
    let mut out = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // A torn last line (crash mid-write) is ignored.
        if let Ok(e) = serde_json::from_str(&line) {
            out.push(e);
        }
    }
    Ok(out)
}

/// One restore run recorded in the undo folder.
#[derive(Debug, Clone)]
pub struct UndoRun {
    pub dir: PathBuf,
    pub header: UndoEntry,
    /// When it was undone (contents of `undone.txt`), if it was.
    pub undone: Option<String>,
}

/// Every recorded restore run, newest first.
pub fn list_all(target: &Target) -> Vec<UndoRun> {
    let root = target.local(&format!(r"{}\{UNDO_DIR}", target.profile));
    let mut out: Vec<UndoRun> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let header = read_journal(&dir).ok()?.into_iter().next()?;
            if !matches!(header, UndoEntry::Header { .. }) {
                return None;
            }
            let undone = std::fs::read_to_string(dir.join(UNDONE))
                .ok()
                .map(|t| t.trim().to_owned());
            Some(UndoRun {
                dir,
                header,
                undone,
            })
        })
        .collect();
    out.sort_by(|a, b| b.dir.cmp(&a.dir));
    out
}

/// Restore runs not undone yet, newest first: (folder, header).
pub fn list(target: &Target) -> Vec<(PathBuf, UndoEntry)> {
    list_all(target)
        .into_iter()
        .filter(|r| r.undone.is_none())
        .map(|r| (r.dir, r.header))
        .collect()
}

#[derive(Debug, Default)]
pub struct UndoReport {
    pub done: Vec<String>,
    /// Left alone because they changed after the restore (or `--force` off).
    pub kept: Vec<String>,
    pub failed: Vec<String>,
}

/// Undo one restore run. `dry_run` only reports. A restored file that was
/// changed afterwards is kept unless `force`.
pub fn undo(
    dir: &Path,
    target: &Target,
    host: &dyn RestoreHost,
    dry_run: bool,
    force: bool,
) -> std::io::Result<UndoReport> {
    let entries = read_journal(dir)?;
    let mut r = UndoReport::default();
    let would = if dry_run { "would " } else { "" };
    let unchanged = |local: &Path, sha: &str| hash_file(local).map(|h| h == sha).unwrap_or(false);
    for e in entries.iter().rev() {
        let mut sys = |op: SysOp, what: String| {
            if dry_run || host.simulated() {
                r.done.push(format!(
                    "{would}{what}{}",
                    if host.simulated() {
                        " (simulation: not run)"
                    } else {
                        ""
                    }
                ));
                return;
            }
            match host.sys(&op) {
                Ok(_) => r.done.push(what),
                Err(e) => r.failed.push(format!("{what}: {e}")),
            }
        };
        match e {
            UndoEntry::Header { .. } => {}
            UndoEntry::Mkdir { path } => {
                let l = target.local(path);
                if std::fs::read_dir(&l).is_ok_and(|mut d| d.next().is_none()) && !dry_run {
                    let _ = std::fs::remove_dir(&l);
                }
            }
            UndoEntry::Create { path, sha256 } => {
                let l = target.local(path);
                if !l.exists() {
                    continue;
                }
                if !force && !unchanged(&l, sha256) {
                    r.kept.push(format!("{path} (changed since the restore)"));
                } else if dry_run {
                    r.done.push(format!("would delete {path}"));
                } else {
                    match std::fs::remove_file(&l) {
                        Ok(()) => r.done.push(format!("deleted {path}")),
                        Err(e) => r.failed.push(format!("{path}: {e}")),
                    }
                }
            }
            UndoEntry::Overwrite {
                path,
                saved,
                sha256,
            } => {
                let l = target.local(path);
                if l.exists() && !force && !unchanged(&l, sha256) {
                    r.kept.push(format!(
                        "{path} (changed since the restore; old copy: {})",
                        dir.join(saved).display()
                    ));
                } else if dry_run {
                    r.done.push(format!("would put back {path}"));
                } else {
                    let put = (|| {
                        let tmp = with_suffix(&l, ".rbundo");
                        std::fs::copy(dir.join(saved), &tmp)?;
                        if let Ok(t) = std::fs::metadata(dir.join(saved)).and_then(|m| m.modified())
                        {
                            File::options().write(true).open(&tmp)?.set_modified(t)?;
                        }
                        std::fs::rename(&tmp, &l)
                    })();
                    match put {
                        Ok(()) => r.done.push(format!("put back {path}")),
                        Err(e) => r.failed.push(format!("{path}: {e}")),
                    }
                }
            }
            UndoEntry::Reg {
                key,
                saved: Some(s),
            } => sys(SysOp::RegImport(dir.join(s)), format!("put back {key}")),
            UndoEntry::Reg { key, saved: None } => sys(
                SysOp::RegDelete(key.clone()),
                format!("delete {key} (it didn't exist before)"),
            ),
            UndoEntry::Power { imported, previous } => {
                if let Some(p) = previous {
                    sys(
                        SysOp::PowerSetActive(p.clone()),
                        format!("make power plan {p} active again"),
                    );
                }
                if let Some(g) = imported {
                    sys(
                        SysOp::PowerDelete(g.clone()),
                        format!("delete imported power plan {g}"),
                    );
                }
            }
            UndoEntry::Wifi { name } => sys(
                SysOp::WifiDelete(name.clone()),
                format!("remove Wi-Fi network {name}"),
            ),
            UndoEntry::Cert { thumbprint } => sys(
                SysOp::CertDelete(thumbprint.clone()),
                format!("remove certificate {thumbprint}"),
            ),
            UndoEntry::Task { name } => sys(
                SysOp::TaskDelete(name.clone()),
                format!("delete scheduled task {name}"),
            ),
        }
    }
    if !dry_run && r.failed.is_empty() {
        let _ = std::fs::write(
            dir.join(UNDONE),
            crate::backup::time::rfc3339(std::time::SystemTime::now()),
        );
    }
    Ok(r)
}
