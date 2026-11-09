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
    assert!(err.contains("Resuming: 16 file(s) already copied"), "{err}");
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
