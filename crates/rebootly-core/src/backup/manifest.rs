//! `manifest.json` (SPEC §10). Schema-versioned; bump `SCHEMA_VERSION` on any
//! breaking change and keep readers tolerant of unknown fields.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::exclude::Category;
use super::walk::{PathIssue, ReparsePoint};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Ok,
    Skipped,
    Failed,
}

/// One file in the backup. Also the record type of `job.state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path inside the backup folder, `/`-separated.
    pub path: String,
    /// Path on the source machine (Windows form).
    pub source: String,
    pub size: u64,
    pub mtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributes: Option<u32>,
    pub sha256: Option<String>,
    pub status: FileStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SourceInfo {
    pub machine: Option<String>,
    pub os_name: String,
    pub edition: Option<String>,
    pub display_version: Option<String>,
    pub build: Option<u32>,
    pub ubr: Option<u32>,
    pub environment: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderRecord {
    /// KNOWNFOLDERID
    pub guid: String,
    /// Resolved path on the source machine.
    pub source_path: String,
    pub included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserRecord {
    pub sid: String,
    pub username: String,
    pub account_kind: String,
    pub profile_path: String,
    pub hive_dirty: Option<bool>,
    /// Known-folder name → record (KNOWNFOLDERID → path map).
    pub folders: BTreeMap<String, FolderRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExcludedRecord {
    pub category: Category,
    pub description: &'static str,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManualStep {
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub tool_version: &'static str,
    pub job_id: String,
    pub started: String,
    pub finished: Option<String>,
    /// False if the job was cancelled or interrupted (resume with `--resume`).
    pub complete: bool,
    pub source: SourceInfo,
    pub users: Vec<UserRecord>,
    /// App packs used (M3).
    pub packs: Vec<String>,
    pub files: Vec<FileEntry>,
    pub reparse_points: Vec<ReparsePoint>,
    pub excluded: Vec<ExcludedRecord>,
    pub issues: Vec<PathIssue>,
    /// Encryption metadata, never secrets (M4).
    pub encryption: Option<String>,
    pub manual_steps: Vec<ManualStep>,
    pub warnings: Vec<String>,
}

impl Manifest {
    pub fn counts(&self) -> (usize, usize, usize) {
        let n = |s| self.files.iter().filter(|f| f.status == s).count();
        (
            n(FileStatus::Ok),
            n(FileStatus::Skipped),
            n(FileStatus::Failed),
        )
    }

    /// Write via a temp file + rename so a crash never leaves half a manifest.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        write_atomic(
            &dir.join("manifest.json"),
            &serde_json::to_vec_pretty(self)?,
        )
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(tmp, path)
}
