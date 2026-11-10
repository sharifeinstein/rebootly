//! Copy one file: stream, hash (SHA-256) while copying, retry with backoff,
//! write to a temporary name and rename into place, keep the mtime.
//!
//! Only the main stream is copied here (bytes are streamed, not
//! `CopyFileEx`); alternate data streams are handled by `filecopy`.
//!
//! A file that another program holds locked (sharing or lock violation) is
//! retried once at once from a consistent snapshot (VSS in live mode), via
//! the `open(true)` call.

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

fn copy_once(
    open: &Opener<'_>,
    snapshot: bool,
    dest: &Path,
    mtime: Option<SystemTime>,
    flow: &Flow,
) -> io::Result<(String, u64)> {
    let mut src = open(snapshot)?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = partial_path(dest);
    let mut out = File::create(&tmp)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF];
    let mut total = 0u64;
    loop {
        if !flow.proceed() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        total += n as u64;
        flow.add_bytes(n as u64);
    }
    if let Some(t) = mtime {
        out.set_modified(t)?;
    }
    out.sync_all()?;
    drop(out);
    std::fs::rename(&tmp, dest)?;
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
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                let _ = std::fs::remove_file(partial_path(dest));
                return Err(CopyFailed {
                    reason: "cancelled".into(),
                    attempts: attempt + 1,
                });
            }
            // Locked: switch to the snapshot straight away (no backoff,
            // doesn't use up an attempt).
            Err(e) if !snapshot && is_lock_error(&e) => {
                snapshot = true;
                wait = false;
                last = e.to_string();
                continue;
            }
            // No snapshot possible (offline, or VSS failed): retrying won't help.
            Err(e) if snapshot && e.kind() == io::ErrorKind::Unsupported => {
                let _ = std::fs::remove_file(partial_path(dest));
                return Err(CopyFailed {
                    reason: e.to_string(),
                    attempts: attempt + 1,
                });
            }
            Err(e) => last = e.to_string(),
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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn cancelled() -> Flow {
        let f = Flow::default();
        f.stop();
        f
    }

    const NO_WAIT: RetryPolicy = RetryPolicy {
        attempts: 3,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    };

    #[test]
    fn copies_hashes_and_keeps_mtime() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src.bin");
        std::fs::write(&src, b"abc").unwrap();
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let dest = t.path().join("out/deep/src.bin");
        let open = |_| File::open(&src);
        let ok = copy_file(&open, &dest, Some(when), NO_WAIT, &Flow::default()).unwrap();
        // SHA-256("abc")
        assert_eq!(
            ok.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!((ok.bytes, ok.attempts), (3, 1));
        assert_eq!(std::fs::metadata(&dest).unwrap().modified().unwrap(), when);
        assert!(!partial_path(&dest).exists());
        assert_eq!(hash_file(&dest).unwrap(), ok.sha256);
    }

    #[test]
    fn retries_then_succeeds_or_gives_up() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("s");
        std::fs::write(&src, b"data").unwrap();
        let fails = AtomicU32::new(2);
        let flaky = |_| {
            if fails.load(Ordering::SeqCst) > 0 {
                fails.fetch_sub(1, Ordering::SeqCst);
                Err(io::Error::other("bad sector"))
            } else {
                File::open(&src)
            }
        };
        let ok = copy_file(
            &flaky,
            &t.path().join("d1"),
            None,
            NO_WAIT,
            &Flow::default(),
        );
        assert_eq!(ok.unwrap().attempts, 3);

        let never = |_| -> io::Result<File> { Err(io::Error::other("bad sector")) };
        let err = copy_file(
            &never,
            &t.path().join("d2"),
            None,
            NO_WAIT,
            &Flow::default(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            CopyFailed {
                reason: "bad sector".into(),
                attempts: 3
            }
        );
        assert!(!t.path().join("d2").exists());
    }

    #[test]
    fn cancel_stops_without_leaving_partials() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("s");
        std::fs::write(&src, b"data").unwrap();
        let dest = t.path().join("d");
        let open = |_| File::open(&src);
        let err = copy_file(&open, &dest, None, NO_WAIT, &cancelled()).unwrap_err();
        assert_eq!(err.reason, "cancelled");
        assert!(!dest.exists() && !partial_path(&dest).exists());
    }

    #[test]
    fn locked_file_is_read_from_the_snapshot() {
        let t = tempfile::tempdir().unwrap();
        let snap = t.path().join("snap");
        std::fs::write(&snap, b"abc").unwrap();
        let busy = || io::Error::from(io::ErrorKind::ResourceBusy);
        let open = |s: bool| if s { File::open(&snap) } else { Err(busy()) };
        let ok = copy_file(&open, &t.path().join("d"), None, NO_WAIT, &Flow::default()).unwrap();
        assert!(ok.from_snapshot);
        assert_eq!(ok.attempts, 1);

        // No snapshot available: one clear failure, no pointless retries.
        let none = |s: bool| -> io::Result<File> {
            if s {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "close the program",
                ))
            } else {
                Err(busy())
            }
        };
        let err =
            copy_file(&none, &t.path().join("e"), None, NO_WAIT, &Flow::default()).unwrap_err();
        assert_eq!(
            (err.reason.as_str(), err.attempts),
            ("close the program", 1)
        );
    }
}
