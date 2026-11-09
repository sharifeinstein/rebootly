use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

fn migrate(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_migrate"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn inspect_text() {
    let root = fixture();
    let (ok, out, err) = migrate(&["inspect", "--windows-root", root.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("Windows 11 Pro 24H2 (build 26100.4351)  computer OLD-PC"));
    assert!(out.contains("User bob  [Microsoft account]"));
    assert!(out.contains("[OneDrive]"));
    assert!(out.contains("(on drive D:, not mounted)"));
    assert!(out.contains("(network path, not followed)"));
    assert!(out.contains("DIRTY"));
    assert!(out.contains("S-1-5-18  (system account)"));
}

#[test]
fn inspect_json_and_scan() {
    let parent = fixture().join("..");
    let (ok, out, err) = migrate(&["inspect", "--scan", parent.to_str().unwrap(), "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["users"].as_array().unwrap().len(), 5);
}

#[test]
fn inspect_errors_cleanly() {
    let t = tempfile::tempdir().unwrap();
    let (ok, _, err) = migrate(&["inspect", "--windows-root", t.path().to_str().unwrap()]);
    assert!(!ok);
    assert!(
        err.starts_with("error: no Windows installation found"),
        "{err}"
    );
    let (ok, _, err) = migrate(&["inspect"]);
    assert!(!ok && err.contains("--windows-root"));
}
