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
    /// Read from a VSS snapshot because the file was open in another program.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snapshot: bool,
    /// Alternate data streams kept beside the file (not `Zone.Identifier`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub streams: Vec<StreamEntry>,
}

/// One alternate data stream of a backed-up file, stored as a separate file
/// under `streams/` (targets like exFAT or a NAS can't hold streams).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamEntry {
    pub name: String,
    pub size: u64,
    /// Path inside the backup folder, when stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub sha256: Option<String>,
    pub status: FileStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceInfo {
    pub machine: Option<String>,
    pub os_name: String,
    pub edition: Option<String>,
    pub display_version: Option<String>,
    pub build: Option<u32>,
    pub ubr: Option<u32>,
    pub environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderRecord {
    /// KNOWNFOLDERID
    pub guid: String,
    /// Resolved path on the source machine.
    pub source_path: String,
    pub included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackPathRecord {
    /// Template from the pack, e.g. `%APPDATA%\Code\User`.
    pub template: String,
    /// Expanded path on the source machine.
    pub source_path: String,
    /// Where it is in the backup (`None` if not copied).
    pub dest: Option<String>,
    pub mode: crate::packs::schema::Mode,
    /// Rewrite old profile paths inside these files on restore (M5).
    pub rewrite: bool,
    pub status: PackPathStatus,
    /// For `list_only`: names found in the folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackPathStatus {
    Copied,
    Listed,
    /// Sensitive and no passphrase given: not copied at all.
    SensitiveNotCopied,
    /// Sensitive: stored only in the encrypted bundle (`dest` is its path there).
    Encrypted,
    NotFound,
    NotOnVolume,
    Duplicate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackRecord {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub origin: String,
    pub user: String,
    pub detected_by: String,
    pub winget_id: Option<String>,
    pub requires_closed: Vec<String>,
    pub notes: Option<String>,
    pub post_restore: Vec<crate::packs::schema::PostRestore>,
    pub paths: Vec<PackPathRecord>,
    /// `.reg` file with the pack's HKCU keys, if any were present.
    pub registry_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRecord {
    pub label: String,
    pub source_path: String,
    pub dest: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// App packs used, per user.
    pub packs: Vec<PackRecord>,
    /// Custom, Public and drive-root folders.
    pub custom: Vec<CustomRecord>,
    /// Non-system folders found at the drive root (offered, not forced).
    pub offered_root_folders: Vec<String>,
    pub files: Vec<FileEntry>,
    pub reparse_points: Vec<ReparsePoint>,
    pub excluded: Vec<ExcludedRecord>,
    pub issues: Vec<PathIssue>,
    /// Whole-backup encryption metadata, never secrets (not implemented yet).
    pub encryption: Option<String>,
    /// The encrypted sensitive bundle, if one was written (SPEC §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive: Option<SensitiveRecord>,
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

/// What went into `sensitive.age`: names and sizes only, no hashes or
/// contents (those are inside the encrypted bundle).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SensitiveRecord {
    pub file: String,
    pub format: String,
    pub files: Vec<SensitiveEntry>,
    pub failed: Vec<SensitiveFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SensitiveEntry {
    pub path: String,
    pub source: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SensitiveFailure {
    pub path: String,
    pub source: String,
    pub reason: String,
}
