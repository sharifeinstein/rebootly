//! Copy one file: stream, hash (SHA-256) while copying, retry with backoff,
//! write to a temporary name and rename into place, keep the mtime.
//!
//! Only the main stream is copied here (bytes are streamed, not
//! `CopyFileEx`); alternate data streams are handled by `filecopy`.
//!
//! A file that another program holds locked (sharing or lock violation), or
//! that can't be opened or read ("Access is denied": a filter driver such as
//! an antivirus's, which often isn't attached to shadow copies), is retried
//! at once from a consistent snapshot (VSS in live mode), via the
//! `open(true)` call. Errors writing the destination never switch.

use super::flow::Flow;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

pub const PARTIAL_SUFFIX: &str = ".rbpart";
const BUF: usize = 1 << 20;

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            attempts: 4,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(5),
        }
    }
}

impl RetryPolicy {
    fn delay(&self, attempt: u32) -> Duration {
        self.base_delay
            .saturating_mul(1 << attempt.min(16))
            .min(self.max_delay)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    pub sha256: String,
    pub bytes: u64,
    pub attempts: u32,
    /// Read from the snapshot because the live file was locked.
    pub from_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyFailed {
    pub reason: String,
    pub attempts: u32,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn partial_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(PARTIAL_SUFFIX);
    PathBuf::from(s)
}

/// Opens the source; `true` = from the snapshot instead of the live file.
pub type Opener<'a> = dyn Fn(bool) -> io::Result<File> + 'a;

/// Another program has the file open without sharing, or holds a byte-range
/// lock (Windows `ERROR_SHARING_VIOLATION` / `ERROR_LOCK_VIOLATION`).
pub fn is_lock_error(e: &io::Error) -> bool {
    (cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)))
        || e.kind() == io::ErrorKind::ResourceBusy
}

/// Reading the source should be tried from the snapshot: locked, or access
/// denied (Windows `ERROR_ACCESS_DENIED`, os error 5, even with backup
/// semantics: usually a filter driver).
pub fn try_snapshot(e: &io::Error) -> bool {
    is_lock_error(e) || e.kind() == io::ErrorKind::PermissionDenied
}

/// Where a copy failed: reading the source or writing the destination.
#[derive(Debug)]
enum Side {
    Source,
    Dest,
}

fn copy_once(
    open: &Opener<'_>,
    snapshot: bool,
    dest: &Path,
    mtime: Option<SystemTime>,
    flow: &Flow,
) -> Result<(String, u64), (Side, io::Error)> {
    let src_err = |e| (Side::Source, e);
    let dst_err = |e| (Side::Dest, e);
    let mut src = open(snapshot).map_err(src_err)?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(dst_err)?;
    }
    let tmp = partial_path(dest);
    let mut out = File::create(&tmp).map_err(dst_err)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF];
    let mut total = 0u64;
    loop {
        if !flow.proceed() {
            let e = io::Error::new(io::ErrorKind::Interrupted, "cancelled");
            return Err(dst_err(e));
        }
        let n = match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(src_err(e)),
        };
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(dst_err)?;
        total += n as u64;
        flow.add_bytes(n as u64);
    }
    if let Some(t) = mtime {
        out.set_modified(t).map_err(dst_err)?;
    }
    out.sync_all().map_err(dst_err)?;
    drop(out);
    std::fs::rename(&tmp, dest).map_err(dst_err)?;
    Ok((hex(&hasher.finalize()), total))
}

/// Copy with retries. Never panics; the caller logs a failure and moves on.
pub fn copy_file(
    open: &Opener<'_>,
    dest: &Path,
    mtime: Option<SystemTime>,
    policy: RetryPolicy,
    flow: &Flow,
) -> Result<Copied, CopyFailed> {
    let attempts = policy.attempts.max(1);
    let mut last = String::new();
    let mut snapshot = false;
    let mut denied: Option<String> = None;
    let mut attempt = 0;
    let mut wait = false;
    while attempt < attempts {
        if wait {
            std::thread::sleep(policy.delay(attempt - 1));
        }
        match copy_once(open, snapshot, dest, mtime, flow) {
            Ok((sha256, bytes)) => {
                return Ok(Copied {
                    sha256,
                    bytes,
                    attempts: attempt + 1,
                    from_snapshot: snapshot,
                });
            }
            Err((_, e)) if e.kind() == io::ErrorKind::Interrupted => {
                let _ = std::fs::remove_file(partial_path(dest));
                return Err(CopyFailed {
                    reason: "cancelled".into(),
                    attempts: attempt + 1,
                });
            }
            // Locked or denied: switch to the snapshot straight away (no
            // backoff, doesn't use up an attempt).
            Err((Side::Source, e)) if !snapshot && try_snapshot(&e) => {
                snapshot = true;
                wait = false;
                denied = (!is_lock_error(&e)).then(|| e.to_string());
                last = e.to_string();
                continue;
            }
            // No snapshot possible (offline, or VSS failed): retrying won't
            // help. For "Access is denied" that error is the useful one (the
            // snapshot's "close the program" hint is about locks).
            Err((_, e)) if snapshot && e.kind() == io::ErrorKind::Unsupported => {
                let _ = std::fs::remove_file(partial_path(dest));
                return Err(CopyFailed {
                    reason: denied.take().unwrap_or_else(|| e.to_string()),
                    attempts: attempt + 1,
                });
            }
            Err((_, e)) => last = e.to_string(),
        }
        attempt += 1;
        wait = true;
    }
    let _ = std::fs::remove_file(partial_path(dest));
    Err(CopyFailed {
        reason: last,
        attempts,
    })
}

/// SHA-256 of a file on disk (used by verify and resume checks).
pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests;
