use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

fn rebootly(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rebootly"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn backup_then_resume_is_a_no_op() {
    let t = tempfile::tempdir().unwrap();
    let fx = fixture();
    let target = t.path().join("usb");
    let (code, job_dir, err) = rebootly(&[
        "backup",
        "--windows-root",
        fx.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let job = PathBuf::from(&job_dir);
    assert!(job.join("manifest.json").is_file() && job.join("report.txt").is_file());
    assert!(
        job.join("users/Alice/folders/Documents/report.txt")
            .is_file()
    );
    assert!(err.contains("Pre-flight:") && err.contains("0 failed"));

    let (code, _, err) = rebootly(&[
        "backup",
        "--windows-root",
        fx.to_str().unwrap(),
        "--resume",
        &job_dir,
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("Resuming: ") && err.contains(" file(s) already copied"),
        "{err}"
    );
    assert!(
        err.contains("0/0 files") || !err.contains("failed:"),
        "nothing recopied: {err}"
    );
}

#[test]
fn preflight_only_writes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let fx = fixture();
    let target = t.path().join("usb");
    let (code, _, err) = rebootly(&[
        "backup",
        "--windows-root",
        fx.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--user",
        "bob",
        "--folder",
        "Desktop",
        "--preflight",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("Desktop") && !err.contains("Downloads"),
        "{err}"
    );
    assert!(!target.exists());
}

#[test]
fn refuses_to_write_into_the_source_volume_even_with_force() {
    let t = tempfile::tempdir().unwrap();
    let vol = t.path().join("vol");
    let status = Command::new("cp")
        .args(["-r", fixture().to_str().unwrap(), vol.to_str().unwrap()])
        .status();
    if !status.is_ok_and(|s| s.success()) {
        return; // no cp (Windows CI): the core unit test covers this
    }
    let inside = vol.join("backups");
    let (code, _, err) = rebootly(&[
        "backup",
        "--windows-root",
        vol.to_str().unwrap(),
        "--target",
        inside.to_str().unwrap(),
        "--force",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("never writes to the source disk"), "{err}");
    assert!(!inside.exists());
}

#[test]
fn sensitive_backup_then_extract() {
    let t = tempfile::tempdir().unwrap();
    let fx = fixture();
    let pf = t.path().join("pass.txt");
    std::fs::write(&pf, "correct horse battery\r\n").unwrap();
    let (code, job_dir, err) = rebootly(&[
        "backup",
        "--windows-root",
        fx.to_str().unwrap(),
        "--target",
        t.path().join("usb").to_str().unwrap(),
        "--user",
        "Alice",
        "--sensitive",
        "--passphrase-file",
        pf.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("go into the encrypted sensitive.age"), "{err}");
    let bundle = PathBuf::from(&job_dir).join("sensitive.age");
    assert!(bundle.is_file());
    assert!(!err.contains("correct horse"));

    let b = bundle.to_str().unwrap();
    let (code, out, err) = rebootly(&[
        "sensitive",
        "list",
        b,
        "--passphrase-file",
        pf.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("users/Alice/packs/dev-dotfiles/.ssh/id_ed25519"),
        "{out}"
    );

    let to = t.path().join("keys");
    let (code, _, err) = rebootly(&[
        "sensitive",
        "extract",
        b,
        "--to",
        to.to_str().unwrap(),
        "--passphrase-file",
        pf.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        to.join("users/Alice/packs/dev-dotfiles/.ssh/id_ed25519")
            .is_file()
    );

    std::fs::write(&pf, "wrong passphrase!").unwrap();
    let (code, _, err) = rebootly(&[
        "sensitive",
        "list",
        b,
        "--passphrase-file",
        pf.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("wrong passphrase"), "{err}");
}
