//! Engine tests for app packs and custom folders.

use super::*;

fn with_packs() -> PlanOptions {
    PlanOptions {
        packs: crate::packs::load::load_all(None).0,
        ..Default::default()
    }
}

#[test]
fn packs_detect_copy_list_and_export_registry() {
    let (t, root) = volume();
    let src = OfflineVolume::open(&root).unwrap();
    let opts = PlanOptions {
        users: vec!["Alice".into(), "bob".into()],
        ..with_packs()
    };
    let p = plan(&src, &opts).unwrap();
    let ids = |u: &str| -> Vec<String> {
        p.packs
            .iter()
            .filter(|r| r.user == u)
            .map(|r| r.id.clone())
            .collect()
    };
    let alice = ids("Alice");
    for id in ["chrome", "dev-dotfiles", "vscode", "windows-shell"] {
        assert!(
            alice.contains(&id.to_owned()),
            "{id} not detected: {alice:?}"
        );
    }
    assert!(!alice.contains(&"firefox".to_owned()));
    assert!(
        !alice.contains(&"notepadpp".to_owned()),
        "installed machine-wide, no user data"
    );
    let bob = ids("bob");
    assert!(
        bob.contains(&"powershell".to_owned()),
        "KF:Documents follows OneDrive"
    );
    assert!(
        !bob.contains(&"chrome".to_owned()),
        "machine-wide app without user data: {bob:?}"
    );

    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    assert_eq!(m.counts().2, 0);
    let d = &job.dir;
    let code = d.join("users/Alice/packs/vscode/User");
    assert!(code.join("settings.json").exists() && code.join("snippets/python.json").exists());
    assert!(
        !code.join("workspaceStorage").exists(),
        "include filter applied"
    );
    let chrome = d.join("users/Alice/packs/chrome/User Data");
    assert!(chrome.join("Default/Bookmarks").exists());
    assert!(
        !chrome.join("Default/Login Data").exists() && !chrome.join("Local State").exists(),
        "secrets never copied"
    );
    assert!(!chrome.join("Default/Cache").exists() && !chrome.join("Crashpad").exists());
    assert!(
        d.join("users/Alice/packs/dev-dotfiles/.gitconfig")
            .is_file()
    );
    assert!(
        !d.join("users/Alice/packs/dev-dotfiles/.ssh").exists(),
        "sensitive not copied in plain"
    );
    assert!(
        d.join("users/bob/packs/powershell/WindowsPowerShell/Microsoft.PowerShell_profile.ps1")
            .exists()
    );
    assert!(
        !d.join("users/bob/folders/Documents/WindowsPowerShell")
            .exists(),
        "pack root inside Documents is copied once, by the pack"
    );

    let vs = m.packs.iter().find(|r| r.id == "vscode").unwrap();
    let ext = &vs.paths[1];
    assert_eq!(ext.status, manifest::PackPathStatus::Listed);
    assert_eq!(
        ext.listed,
        [
            "ms-python.python-2026.1.0",
            "rust-lang.rust-analyzer-0.3.2600"
        ]
    );
    assert!(vs.paths[0].rewrite);
    let dot = m.packs.iter().find(|r| r.id == "dev-dotfiles").unwrap();
    assert!(
        dot.paths
            .iter()
            .any(|p| p.status == manifest::PackPathStatus::SensitiveNotCopied)
    );
    assert!(
        m.manual_steps
            .iter()
            .any(|s| s.title == "Sensitive files of Alice were not copied")
    );

    let shell = m
        .packs
        .iter()
        .find(|r| r.id == "windows-shell" && r.user == "Alice")
        .unwrap();
    let reg = shell.registry_file.as_deref().unwrap();
    let bytes = std::fs::read(d.join(reg)).unwrap();
    let text = String::from_utf16_lossy(
        &bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>(),
    );
    assert!(text.contains("[HKEY_CURRENT_USER\\Control Panel\\Desktop]"));
    assert!(text.contains("\"HideFileExt\"=dword:00000000"));
    assert!(m.files.iter().any(|f| f.path == reg && f.sha256.is_some()));
}

#[test]
fn public_root_and_custom_folders() {
    let (t, root) = volume();
    let src = OfflineVolume::open(&root).unwrap();
    let opts = PlanOptions {
        folders: vec!["Desktop".into()],
        public: true,
        root_folders: vec!["Projects".into()],
        custom: vec![
            ("Old stuff".into(), r"C:\Users\erin".into()),
            ("Nas".into(), r"\\nas\x".into()),
        ],
        ..Default::default()
    };
    let p = plan(&src, &opts).unwrap();
    assert_eq!(p.offered_root_folders, ["Projects"]);
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    assert!(job.dir.join("custom/Public/Documents/shared.txt").exists());
    assert!(job.dir.join("custom/Projects/rebootly/README.md").exists());
    let old = m.custom.iter().find(|c| c.label == "Old stuff").unwrap();
    assert_eq!(old.dest.as_deref(), Some("custom/Old stuff"));
    // erin's Desktop sits inside it but is its own job: copied once, there.
    assert!(job.dir.join("users/erin/folders/Desktop/e.txt").exists());
    assert!(!job.dir.join("custom/Old stuff/Desktop").exists());
    assert!(
        m.custom
            .iter()
            .any(|c| c.label == "Nas" && c.dest.is_none())
    );
}
