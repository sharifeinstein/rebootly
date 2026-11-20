//! `rebootly-cli verify` and `compare` end to end on the fixture.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

fn rebootly(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rebootly-cli"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn verify_then_compare_a_simulated_restore() {
    let t = tempfile::tempdir().unwrap();
    let usb = t.path().join("usb");
    let (code, job, err) = rebootly(&[
        "backup",
        "--windows-root",
        fixture().to_str().unwrap(),
        "--target",
        usb.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let job = job.trim().to_owned();

    let (code, out, err) = rebootly(&["verify", &job]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("Every copied file matches") && out.ends_with("Result: OK\n"),
        "{out}"
    );

    let sim = t.path().join("new");
    let sim_s = sim.to_str().unwrap();
    let sim_args = [
        "--simulate-root",
        sim_s,
        "--simulate-user",
        "sam",
        "--user",
        "Alice",
    ];
    let (code, out, _) = rebootly(&[&["compare", job.as_str()], &sim_args[..]].concat());
    assert_eq!(code, 2, "{out}");
    assert!(
        out.contains("MISSING") && out.ends_with("Result: DIFFERENCES FOUND\n"),
        "{out}"
    );

    let (code, _, err) = rebootly(&[&["restore", job.as_str(), "--yes"], &sim_args[..]].concat());
    assert_eq!(code, 0, "{err}");
    let (code, out, _) = rebootly(&[&["compare", job.as_str()], &sim_args[..]].concat());
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("same as in the backup") && out.contains("Settings (registry"),
        "{out}"
    );

    let report = sim.join("C/Users/sam/Documents/report.txt");
    std::fs::remove_file(&report).unwrap();
    let (code, out, _) = rebootly(&[&["compare", job.as_str(), "--json"], &sim_args[..]].concat());
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let missing: Vec<&str> = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["state"] == "missing")
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert_eq!(missing, [r"C:\Users\sam\Documents\report.txt"]);

    // Damage in the backup itself.
    let copy = Path::new(&job).join("users/Alice/folders/Documents/report.txt");
    std::fs::write(&copy, b"changed").unwrap();
    let (code, out, _) = rebootly(&["verify", &job, "--json"]);
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["problems"][0]["path"],
        "users/Alice/folders/Documents/report.txt"
    );
    assert_eq!(v["problems"][0]["problem"], "size");

    let (code, _, err) = rebootly(&["verify", t.path().to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("no manifest.json"), "{err}");
}

#[test]
fn a_bad_path_is_named_once() {
    // A folder named manifest.json: a read error on Linux (EISDIR) and on
    // Windows (access denied). A path through a file isn't: Windows calls
    // that "not found".
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir(t.path().join("manifest.json")).unwrap();
    let (code, _, err) = rebootly(&["verify", t.path().to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(
        err.contains("can't read") && err.contains("manifest.json"),
        "{err}"
    );
    assert_eq!(err.matches("os error").count(), 1, "{err}");
}
