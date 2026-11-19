//! The passphrase prompt with piped input (no terminal): one line per
//! question, line breaks not part of the passphrase.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

/// Runs the CLI with `stdin` piped in; (exit code, stdout, stderr).
fn piped(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rebootly-cli"))
        .args(args)
        .env_remove("REBOOTLY_PASSPHRASE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn backup(target: &Path, stdin: &str) -> (i32, String, String) {
    let fx = fixture();
    piped(
        &[
            "backup",
            "--windows-root",
            fx.to_str().unwrap(),
            "--target",
            target.to_str().unwrap(),
            "--user",
            "Alice",
            "--sensitive",
        ],
        stdin,
    )
}

#[test]
fn piped_passphrase_is_asked_twice_and_opens_the_bundle() {
    let t = tempfile::tempdir().unwrap();
    let (code, job_dir, err) = backup(
        &t.path().join("usb"),
        "correct horse battery\r\ncorrect horse battery\n",
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("Keep this passphrase safe"), "{err}");
    assert!(!err.contains("correct horse"), "{err}");
    let bundle = PathBuf::from(&job_dir).join("sensitive.age");
    let b = bundle.to_str().unwrap();

    // The line break isn't part of it: the same words open the bundle.
    let (code, out, err) = piped(&["sensitive", "list", b], "correct horse battery\n");
    assert_eq!(code, 0, "{err}");
    assert!(!out.is_empty());
    let (code, _, err) = piped(&["sensitive", "list", b], "correct horse battery!\n");
    assert_ne!(code, 0, "a wrong passphrase must fail: {err}");
}

#[test]
fn piped_passphrase_too_short_or_different_stops_before_copying() {
    let t = tempfile::tempdir().unwrap();
    let target = t.path().join("usb");
    let (code, _, err) = backup(&target, "short\n");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("the prompt received 5"), "{err}");

    let (code, _, err) = backup(&target, "correct horse battery\ncorrect horse\n");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("don't match"), "{err}");

    let (code, _, err) = backup(&target, "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("the prompt received 0"), "{err}");
    let copied = walk(&target).any(|p| p.ends_with("manifest.json") || p.ends_with("job.state"));
    assert!(
        !copied,
        "nothing may be written before the passphrase is known"
    );
}

fn walk(dir: &Path) -> impl Iterator<Item = PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p.clone());
            }
            out.push(p);
        }
    }
    out.into_iter()
}
