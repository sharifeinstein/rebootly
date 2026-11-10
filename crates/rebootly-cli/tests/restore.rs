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
fn restore_preview_run_and_undo_in_a_simulated_pc() {
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
    let sim = t.path().join("new");
    let sim_s = sim.to_str().unwrap();
    let base = [
        "restore",
        job.as_str(),
        "--simulate-root",
        sim_s,
        "--simulate-user",
        "sam",
    ];

    let (code, _, err) = rebootly(&base);
    assert_eq!(code, 1);
    assert!(err.contains("--user NAME"), "{err}");

    let with = |extra: &[&str]| {
        let mut a = base.to_vec();
        a.extend_from_slice(&["--user", "Alice"]);
        a.extend_from_slice(extra);
        rebootly(&a)
    };
    let (code, out, err) = with(&["--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.is_empty() && err.contains("Dry run: nothing was changed"),
        "{err}"
    );
    assert!(!sim.join("C/Users/sam/Documents").exists());

    let (code, _, err) = with(&[]);
    assert_eq!(code, 1);
    assert!(err.contains("--yes"), "{err}");

    let (code, undo_dir, err) = with(&["--yes", "--conflict", "skip"]);
    assert_eq!(code, 0, "{err}");
    assert!(sim.join("C/Users/sam/Documents/report.txt").is_file());
    assert!(Path::new(&undo_dir).join("journal.jsonl").is_file());

    let (code, list, _) = rebootly(&[
        "undo",
        "--list",
        "--simulate-root",
        sim_s,
        "--simulate-user",
        "sam",
    ]);
    assert_eq!(code, 0);
    assert!(list.starts_with(&undo_dir), "{list}");
    let (code, _, err) = rebootly(&["undo", &undo_dir, "--simulate-root", sim_s, "--yes"]);
    assert_eq!(code, 0, "{err}");
    assert!(!sim.join("C/Users/sam/Documents/report.txt").exists());
    let (code, _, err) = rebootly(&["undo", &undo_dir, "--simulate-root", sim_s, "--yes"]);
    assert_eq!(code, 1);
    assert!(err.contains("already undone"), "{err}");
}

#[test]
fn restore_needs_windows_or_a_simulation() {
    let t = tempfile::tempdir().unwrap();
    let (code, _, err) = rebootly(&["restore", t.path().to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("no manifest.json"), "{err}");
}

#[test]
fn version_names_the_commit() {
    let (code, out, _) = rebootly(&["--version"]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with("rebootly 0.") && out.contains("(commit "),
        "{out}"
    );
}
