//! The encrypted sensitive bundle end to end.

use super::*;
use crate::backup::manifest::PackPathStatus;
use crate::backup::sensitive::{self, Passphrase};

fn pass() -> Passphrase {
    Passphrase::new("correct horse battery".into()).unwrap()
}

fn opts(bundle: bool) -> PlanOptions {
    PlanOptions {
        users: vec!["Alice".into()],
        packs: crate::packs::load::load_all(None).0,
        sensitive_bundle: bundle,
        ..Default::default()
    }
}

fn fast() -> RunOptions {
    RunOptions {
        passphrase: Some(pass()),
        bundle_work_factor: Some(2),
        ..quick()
    }
}

#[test]
fn keys_go_only_into_the_encrypted_bundle() {
    let (t, root) = volume();
    let ssh = root.join("Users/alice/.ssh");
    std::fs::write(ssh.join("config"), b"Host x\n").unwrap();
    let src = OfflineVolume::open(&root).unwrap();
    let p = plan(&src, &opts(true)).unwrap();
    let dests: Vec<&str> = p.sensitive.iter().map(|f| f.dest.as_str()).collect();
    assert_eq!(
        dests,
        [
            "users/Alice/packs/dev-dotfiles/.ssh/config",
            "users/Alice/packs/dev-dotfiles/.ssh/id_ed25519"
        ]
    );
    assert!(!p.files.iter().any(|f| f.dest.contains(".ssh")));
    let dot = p.packs.iter().find(|r| r.id == "dev-dotfiles").unwrap();
    let rec = dot
        .paths
        .iter()
        .find(|r| r.template.ends_with(".ssh"))
        .unwrap();
    assert_eq!(rec.status, PackPathStatus::Encrypted);
    assert!(
        !p.manual_steps
            .iter()
            .any(|s| s.title.starts_with("Sensitive files"))
    );

    // No passphrase: refused before anything is copied.
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let err = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

    let mut saw = false;
    let m = run(&src, &p, &job, &fast(), &mut |e| {
        saw |= matches!(e, Event::Sensitive { files: 2 })
    })
    .unwrap();
    assert!(saw && m.complete);
    let sb = m.sensitive.as_ref().unwrap();
    assert_eq!(sb.files.len(), 2);
    assert!(sb.failed.is_empty());
    let bundle = job.dir.join(sensitive::BUNDLE_FILE);
    assert!(!job.dir.join("users/Alice/packs/dev-dotfiles/.ssh").exists());
    assert!(!job.dir.join("sensitive.age.rbpart").exists());

    // Nothing of the key is readable in plain anywhere in the backup.
    let key = std::fs::read(ssh.join("id_ed25519")).unwrap();
    let raw = std::fs::read(&bundle).unwrap();
    assert!(raw.starts_with(b"age-encryption.org/v1"));
    assert!(!raw.windows(key.len()).any(|w| w == key.as_slice()));
    let mj = std::fs::read_to_string(job.dir.join("manifest.json")).unwrap();
    assert!(!mj.contains("correct horse") && mj.contains("\"sensitive\""));
    let report = std::fs::read_to_string(job.dir.join("report.txt")).unwrap();
    assert!(report.contains("Encrypted sensitive bundle: sensitive.age (2 file(s)"));

    let idx = sensitive::list(&bundle, &pass()).unwrap();
    assert_eq!(idx.files.len(), 2);
    let out = t.path().join("restored");
    sensitive::extract(&bundle, &pass(), &out).unwrap();
    assert_eq!(
        std::fs::read(out.join("users/Alice/packs/dev-dotfiles/.ssh/id_ed25519")).unwrap(),
        key
    );

    let wrong = Passphrase::new("not the passphrase".into()).unwrap();
    let err = sensitive::list(&bundle, &wrong).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn without_a_passphrase_keys_are_left_out_and_never_copied_plain() {
    let (t, root) = volume();
    // A key inside a folder that IS copied: the parent must not pick it up.
    let src = OfflineVolume::open(&root).unwrap();
    let p = plan(&src, &opts(false)).unwrap();
    assert!(p.sensitive.is_empty() && !p.sensitive_jobs.is_empty());
    assert!(!p.files.iter().any(|f| f.dest.contains(".ssh")));
    assert!(
        p.manual_steps
            .iter()
            .any(|s| s.title.starts_with("Sensitive files") && s.detail.contains("--sensitive"))
    );
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    assert!(m.sensitive.is_none());
    assert!(!job.dir.join(sensitive::BUNDLE_FILE).exists());
}

#[test]
fn sensitive_path_nested_in_a_copied_folder_is_not_copied_plain() {
    let (_t, root) = volume();
    // A custom folder that contains the user's .ssh.
    let src = OfflineVolume::open(&root).unwrap();
    let o = PlanOptions {
        custom: vec![("Profile".into(), r"C:\Users\alice".into())],
        ..opts(false)
    };
    let p = plan(&src, &o).unwrap();
    assert!(
        !p.files.iter().any(|f| f.dest.contains(".ssh")),
        "{:?}",
        p.files
            .iter()
            .filter(|f| f.dest.contains("ssh"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn passphrase_rules_and_no_leak_in_debug() {
    assert!(Passphrase::new("short".into()).is_err());
    assert_eq!(format!("{:?}", pass()), "Passphrase(…)");
}
