//! The encrypted sensitive bundle, `sensitive.age` (SPEC §9).
//!
//! Keys and credentials from `sensitive: true` pack paths (SSH keys, cloud
//! CLI credentials, …) are never written in plain. They go into one tar
//! archive, encrypted with a passphrase in the standard age format (scrypt),
//! so `age -d sensitive.age | tar -x` works without Rebootly. The archive
//! ends with `rebootly-sensitive.json`, an index with a SHA-256 per file.
//! The passphrase is held only as a `SecretString` and never logged.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use age::secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::manifest::{SensitiveEntry, SensitiveFailure, SensitiveRecord};
use super::time::rfc3339;
use super::walk::PlannedFile;
use crate::source::Source;

pub const BUNDLE_FILE: &str = "sensitive.age";
pub const INDEX_NAME: &str = "rebootly-sensitive.json";
pub const FORMAT: &str = "age v1, scrypt passphrase; tar inside";
/// Larger files are not what this bundle is for (and would sit in memory).
pub const MAX_FILE: u64 = 64 * 1024 * 1024;
pub const MIN_PASSPHRASE: usize = 8;

/// A bundle passphrase. Debug output never shows it.
#[derive(Clone)]
pub struct Passphrase(SecretString);

impl Passphrase {
    pub fn new(s: String) -> Result<Self, String> {
        if s.chars().count() < MIN_PASSPHRASE {
            return Err(format!(
                "the passphrase must be at least {MIN_PASSPHRASE} characters"
            ));
        }
        Ok(Passphrase(SecretString::from(s)))
    }
}

impl std::fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Passphrase(…)")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundleIndex {
    pub schema: u32,
    pub created: String,
    pub files: Vec<BundleFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundleFile {
    /// Path inside the bundle (`users/<u>/packs/<pack>/…`), same layout as
    /// a plain backup.
    pub path: String,
    /// Windows path it came from.
    pub source: String,
    pub size: u64,
    pub sha256: String,
}

fn bad_pass(e: age::DecryptError) -> io::Error {
    match e {
        age::DecryptError::DecryptionFailed | age::DecryptError::NoMatchingKeys => {
            io::Error::new(io::ErrorKind::PermissionDenied, "wrong passphrase")
        }
        other => io::Error::new(
            io::ErrorKind::InvalidData,
            format!("not a readable sensitive bundle: {other}"),
        ),
    }
}

fn read_capped(src: &dyn Source, f: &PlannedFile) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    src.open_read(&f.src)?
        .take(MAX_FILE + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_FILE {
        return Err(io::Error::other(format!(
            "larger than {} MiB, too big for the sensitive bundle",
            MAX_FILE >> 20
        )));
    }
    Ok(data)
}

fn header_for(size: u64, mtime: Option<SystemTime>) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_size(size);
    h.set_mode(0o600);
    let secs = mtime
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    h.set_mtime(secs);
    h.set_entry_type(tar::EntryType::Regular);
    h // the path is set by append_data, which handles long names
}

/// Write `files` into the encrypted bundle `out` (via `<out>.rbpart`, then
/// rename). Unreadable files are recorded as failed; the bundle still gets
/// written. `work_factor` overrides age's calibrated scrypt cost (tests).
pub fn write_bundle(
    src: &dyn Source,
    files: &[PlannedFile],
    out: &Path,
    pass: &Passphrase,
    work_factor: Option<u8>,
) -> io::Result<SensitiveRecord> {
    let part = out.with_extension("age.rbpart");
    let file = File::create(&part)?;
    let enc = match work_factor {
        None => age::Encryptor::with_user_passphrase(pass.0.clone()),
        Some(n) => {
            let mut r = age::scrypt::Recipient::new(pass.0.clone());
            r.set_work_factor(n);
            age::Encryptor::with_recipients(std::iter::once(&r as &dyn age::Recipient))
                .map_err(io::Error::other)?
        }
    };
    let stream = enc.wrap_output(BufWriter::new(file))?;
    let mut tar = tar::Builder::new(stream);
    let mut record = SensitiveRecord {
        file: BUNDLE_FILE.into(),
        format: FORMAT.into(),
        files: Vec::new(),
        failed: Vec::new(),
    };
    let mut index = BundleIndex {
        schema: 1,
        created: rfc3339(SystemTime::now()),
        files: Vec::new(),
    };
    for f in files {
        match read_capped(src, f) {
            Ok(data) => {
                let mut h = header_for(data.len() as u64, f.mtime);
                tar.append_data(&mut h, &f.dest, data.as_slice())?;
                index.files.push(BundleFile {
                    path: f.dest.clone(),
                    source: f.src_win.clone(),
                    size: data.len() as u64,
                    sha256: hex(&Sha256::digest(&data)),
                });
                record.files.push(SensitiveEntry {
                    path: f.dest.clone(),
                    source: f.src_win.clone(),
                    size: data.len() as u64,
                });
            }
            Err(e) => record.failed.push(SensitiveFailure {
                path: f.dest.clone(),
                source: f.src_win.clone(),
                reason: e.to_string(),
            }),
        }
    }
    let idx = serde_json::to_vec_pretty(&index)?;
    let mut h = header_for(idx.len() as u64, Some(SystemTime::now()));
    tar.append_data(&mut h, INDEX_NAME, idx.as_slice())?;
    let mut buffered = tar.into_inner()?.finish()?;
    buffered.flush()?;
    let file = buffered.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&part, out)?;
    Ok(record)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn open_plain(bundle: &Path, pass: &Passphrase) -> io::Result<impl Read> {
    let dec =
        age::Decryptor::new_buffered(BufReader::new(File::open(bundle)?)).map_err(bad_pass)?;
    if !dec.is_scrypt() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bundle is not passphrase-encrypted",
        ));
    }
    let id = age::scrypt::Identity::new(pass.0.clone());
    dec.decrypt(std::iter::once(&id as &dyn age::Identity))
        .map_err(bad_pass)
}

/// Decrypt and return the index (contents are read but not kept).
pub fn list(bundle: &Path, pass: &Passphrase) -> io::Result<BundleIndex> {
    let mut ar = tar::Archive::new(open_plain(bundle, pass)?);
    let mut index = None;
    for e in ar.entries()? {
        let mut e = e?;
        if e.path()?.as_os_str() == INDEX_NAME {
            let mut s = Vec::new();
            e.read_to_end(&mut s)?;
            index = Some(serde_json::from_slice(&s)?);
        }
    }
    index.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bundle has no index"))
}

/// Decrypt into `dest` (paths can't escape it) and check every file against
/// the index. Returns the index; a hash mismatch is an error.
pub fn extract(bundle: &Path, pass: &Passphrase, dest: &Path) -> io::Result<BundleIndex> {
    std::fs::create_dir_all(dest)?;
    let mut ar = tar::Archive::new(open_plain(bundle, pass)?);
    let mut index: Option<BundleIndex> = None;
    let mut hashes = std::collections::HashMap::new();
    for e in ar.entries()? {
        let mut e = e?;
        let path = e.path()?.to_string_lossy().into_owned();
        let mut data = Vec::new();
        e.read_to_end(&mut data)?;
        if path == INDEX_NAME {
            index = Some(serde_json::from_slice(&data)?);
            continue;
        }
        let target = safe_join(dest, &path)?;
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&target, &data)?;
        hashes.insert(path, hex(&Sha256::digest(&data)));
    }
    let index =
        index.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bundle has no index"))?;
    for f in &index.files {
        if hashes.get(&f.path) != Some(&f.sha256) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is missing or damaged in the bundle", f.path),
            ));
        }
    }
    Ok(index)
}

/// `dest/rel` with only normal components (no `..`, no absolute paths).
fn safe_join(dest: &Path, rel: &str) -> io::Result<std::path::PathBuf> {
    let mut out = dest.to_path_buf();
    for c in rel.split(['/', '\\']) {
        if c.is_empty() || c == "." || c == ".." || c.contains(':') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsafe path in bundle: {rel}"),
            ));
        }
        out.push(c);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::safe_join;
    use std::path::Path;

    #[test]
    fn bundle_paths_cannot_escape_the_extract_folder() {
        let d = Path::new("out");
        assert_eq!(
            safe_join(d, "users/A/.ssh/id").unwrap(),
            Path::new("out/users/A/.ssh/id")
        );
        for bad in [
            "../x",
            "users/../../x",
            "/etc/passwd",
            r"C:\x",
            "a//b",
            r"a\..\..\b",
        ] {
            assert!(safe_join(d, bad).is_err(), "{bad}");
        }
    }
}
