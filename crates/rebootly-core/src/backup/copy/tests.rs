//! Tests for `backup::copy`.

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
    let err = copy_file(&none, &t.path().join("e"), None, NO_WAIT, &Flow::default()).unwrap_err();
    assert_eq!(
        (err.reason.as_str(), err.attempts),
        ("close the program", 1)
    );
}

#[test]
fn access_denied_is_read_from_the_snapshot() {
    let t = tempfile::tempdir().unwrap();
    let snap = t.path().join("snap");
    std::fs::write(&snap, b"abc").unwrap();
    let denied = || io::Error::from(io::ErrorKind::PermissionDenied);
    let open = |s: bool| if s { File::open(&snap) } else { Err(denied()) };
    let ok = copy_file(&open, &t.path().join("d"), None, NO_WAIT, &Flow::default()).unwrap();
    assert!(ok.from_snapshot);
    assert_eq!(ok.attempts, 1);

    // Offline (no snapshot): the access-denied error is reported, once.
    let none = |s: bool| -> io::Result<File> {
        if s {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "close the program",
            ))
        } else {
            Err(denied())
        }
    };
    let err = copy_file(&none, &t.path().join("e"), None, NO_WAIT, &Flow::default()).unwrap_err();
    assert_eq!(err.attempts, 1);
    assert_eq!(err.reason, denied().to_string());

    // Denied in the snapshot too: the usual retries, then that error.
    let calls = AtomicU32::new(0);
    let both = |_s: bool| -> io::Result<File> {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(denied())
    };
    let err = copy_file(&both, &t.path().join("f"), None, NO_WAIT, &Flow::default()).unwrap_err();
    assert_eq!((err.attempts, calls.load(Ordering::SeqCst)), (3, 4));
    assert!(!t.path().join("f").exists());
}
