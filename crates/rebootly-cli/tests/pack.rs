use std::path::{Path, PathBuf};
use std::process::Command;

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

fn repo_packs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

#[test]
fn list_shows_builtins() {
    let (code, out, err) = rebootly(&["pack", "list"]);
    assert_eq!(code, 0, "{err}");
    for id in ["chrome", "vscode", "windows-shell", "dev-dotfiles"] {
        assert!(
            out.lines().any(|l| l.starts_with(id)),
            "{id} missing:\n{out}"
        );
    }
}

#[test]
fn validate_good_and_bad_files() {
    let good = repo_packs().join("vscode.yaml");
    let t = tempfile::tempdir().unwrap();
    let bad = t.path().join("bad.yaml");
    std::fs::write(
        &bad,
        "schema: 1\nid: Bad\nname: x\ndetect: {any: [{path: rel}]}\npaths: []\n",
    )
    .unwrap();
    let (code, out, _) = rebootly(&[
        "pack",
        "validate",
        good.to_str().unwrap(),
        bad.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(
        out.contains("ok   ") && out.contains("(vscode v1)"),
        "{out}"
    );
    assert!(
        out.contains("invalid pack") && out.contains("id:") && out.contains("detect.path"),
        "{out}"
    );
}

#[test]
fn backup_with_packs_public_root_and_custom() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic");
    let t = tempfile::tempdir().unwrap();
    let target = t.path().join("usb");
    let (code, job, err) = rebootly(&[
        "backup",
        "--windows-root",
        fx.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--user",
        "alice",
        "--public",
        "--root-folder",
        "Projects",
        "--custom",
        r"Erin=C:\Users\erin",
    ]);
    assert_eq!(code, 0, "{err}");
    let job = PathBuf::from(job.trim());
    assert!(
        err.contains("App packs:") && err.contains("vscode"),
        "{err}"
    );
    assert!(
        job.join("users/Alice/packs/vscode/User/settings.json")
            .exists()
    );
    assert!(job.join("users/Alice/registry/windows-shell.reg").exists());
    assert!(job.join("custom/Public/Documents/shared.txt").exists());
    assert!(job.join("custom/Projects/rebootly/README.md").exists());
    assert!(job.join("custom/Erin/Desktop/e.txt").exists());
    let report = std::fs::read_to_string(job.join("report.txt")).unwrap();
    assert!(
        report.contains("Sensitive files of Alice were not copied"),
        "{report}"
    );

    let (code, _, err) = rebootly(&[
        "backup",
        "--windows-root",
        fx.to_str().unwrap(),
        "--target",
        "x",
        "--pack",
        "nope",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("unknown pack id"), "{err}");
}
