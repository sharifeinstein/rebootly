//! Reading a backup folder: the manifest (tolerant of unknown fields) and,
//! with a passphrase, the encrypted sensitive bundle.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::RestoreError;
use crate::backup::manifest::{
    CustomRecord, FileEntry, ManualStep, PackRecord, SCHEMA_VERSION, SensitiveRecord, SourceInfo,
    UserRecord,
};
use crate::backup::sensitive::{self, BundleIndex, Passphrase};

/// The parts of `manifest.json` a restore needs.
#[derive(Debug, Clone, Deserialize)]
pub struct BackupManifest {
    pub schema_version: u32,
    #[serde(default)]
    pub tool_version: String,
    pub job_id: String,
    #[serde(default)]
    pub finished: Option<String>,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub source: SourceInfo,
    #[serde(default)]
    pub users: Vec<UserRecord>,
    #[serde(default)]
    pub packs: Vec<PackRecord>,
    #[serde(default)]
    pub custom: Vec<CustomRecord>,
    #[serde(default)]
    pub files: Vec<FileEntry>,
    #[serde(default)]
    pub sensitive: Option<SensitiveRecord>,
    #[serde(default)]
    pub manual_steps: Vec<ManualStep>,
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub dir: PathBuf,
    pub manifest: BackupManifest,
}

impl Backup {
    pub fn open(dir: &Path) -> Result<Self, RestoreError> {
        let path = dir.join("manifest.json");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(RestoreError::NoManifest(dir.display().to_string()));
            }
            Err(e) => return Err(e.into()),
        };
        let manifest: BackupManifest =
            serde_json::from_slice(&bytes).map_err(|e| RestoreError::BadManifest(e.to_string()))?;
        if manifest.schema_version > SCHEMA_VERSION {
            return Err(RestoreError::NewerSchema(manifest.schema_version));
        }
        Ok(Backup {
            dir: dir.to_path_buf(),
            manifest,
        })
    }

    /// Local path of a backup-relative path (`/`-separated). `None` if the
    /// manifest names something outside the backup folder.
    pub fn local(&self, rel: &str) -> Option<PathBuf> {
        safe_rel(&self.dir, rel)
    }

    /// A plain (not encrypted) file of the backup that was copied fine.
    pub fn file(&self, rel: &str) -> Option<&FileEntry> {
        self.manifest
            .files
            .iter()
            .find(|f| f.path == rel && f.status == crate::backup::manifest::FileStatus::Ok)
    }

    pub fn bundle_path(&self) -> Option<PathBuf> {
        let s = self.manifest.sensitive.as_ref()?;
        self.local(&s.file).filter(|p| p.is_file())
    }
}

/// `base/rel` with normal components only (no `..`, roots or drive letters).
pub fn safe_rel(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for c in rel.split(['/', '\\']) {
        if c.is_empty() || c == "." || c == ".." || c.contains(':') {
            return None;
        }
        out.push(c);
    }
    Some(out)
}

/// The decrypted sensitive bundle, in a private temp folder that is deleted
/// when this is dropped (it holds clear-text keys).
#[derive(Debug)]
pub struct Bundle {
    pub dir: PathBuf,
    pub index: BundleIndex,
}

impl Bundle {
    pub fn open(bundle: &Path, pass: &Passphrase) -> Result<Self, RestoreError> {
        let dir = private_temp_dir()?;
        match sensitive::extract(bundle, pass, &dir) {
            Ok(index) => Ok(Bundle { dir, index }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                Err(RestoreError::Bundle(e.to_string()))
            }
        }
    }

    pub fn local(&self, rel: &str) -> Option<PathBuf> {
        safe_rel(&self.dir, rel)
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A new folder under the system temp folder (per-user on Windows), made
/// with `create_dir` so an existing folder is never reused.
fn private_temp_dir() -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for i in 0..100u32 {
        let p = base.join(format!(
            "rebootly-restore-{}-{:x}",
            std::process::id(),
            seed.wrapping_add(u128::from(i) * 7919)
        ));
        match std::fs::create_dir(&p) {
            Ok(()) => {
                restrict(&p);
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free temp folder name"))
}

#[cfg(unix)]
fn restrict(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict(_: &Path) {}
