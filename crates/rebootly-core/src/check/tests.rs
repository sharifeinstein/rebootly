//! `verify` and `compare` on a real backup of the `win11-basic` fixture.

use std::time::{Duration, SystemTime};

use super::text::{compare_text, verify_text};
use super::verify::Problem;
use super::*;
use crate::restore::tests::{TestHost, Ui, make_backup, new_pc, opts, pass, plan_for};
use crate::restore::{Backup, Bundle, execute};

fn quiet(_: &Progress) {}

#[test]
fn a_fresh_backup_verifies() {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&make_backup(t.path())).unwrap();
    let mut last = Progress::default();
    let r = verify(&b, None, &mut |p| last = *p);
    let text = verify_text(&r, 50);
    assert!(r.is_good(), "{text}");
    assert!(r.files_checked > 10, "{text}");
    assert_eq!(last.files, last.total_files);
    assert_eq!(last.bytes, r.bytes_checked);
    assert!(r.extra.is_empty(), "{:?}", r.extra);
    assert!(matches!(r.bundle, BundleCheck::NotOpened { .. }), "{text}");
    assert!(text.contains("add --sensitive"), "{text}");

    let r = verify(&b, Some(&pass()), &mut quiet);
    assert!(matches!(r.bundle, BundleCheck::Ok { files, .. } if files > 0));
    assert!(verify_text(&r, 50).ends_with("Result: OK\n"));
}

#[test]
fn verify_finds_damage() {
    let t = tempfile::tempdir().unwrap();
    let dir = make_backup(t.path());
    let b = Backup::open(&dir).unwrap();
    let ok: Vec<_> = b
        .manifest
        .files
        .iter()
        .filter(|f| f.size > 2 && f.sha256.is_some())
        .map(|f| f.path.clone())
        .collect();
    let at = |rel: &str| b.local(rel).unwrap();
    // Same size, other bytes; shorter; gone.
    let mut bytes = std::fs::read(at(&ok[0])).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(at(&ok[0]), &bytes).unwrap();
    std::fs::write(at(&ok[1]), b"x").unwrap();
    std::fs::remove_file(at(&ok[2])).unwrap();
    std::fs::write(dir.join("users/stray.rbpart"), b"left over").unwrap();

    let r = verify(
        &b,
        Some(&crate::backup::sensitive::Passphrase::new("wrong passphrase".into()).unwrap()),
        &mut quiet,
    );
    let text = verify_text(&r, 50);
    assert!(!r.is_good());
    let p = |rel: &str| {
        r.problems
            .iter()
            .find(|f| f.path == rel)
            .map(|f| &f.problem)
    };
    assert_eq!(p(&ok[0]), Some(&Problem::Hash), "{text}");
    assert!(
        matches!(p(&ok[1]), Some(Problem::Size { found: 1, .. })),
        "{text}"
    );
    assert_eq!(p(&ok[2]), Some(&Problem::Missing), "{text}");
    assert_eq!(r.problems.len(), 3, "{text}");
    assert_eq!(r.extra, vec!["users/stray.rbpart".to_owned()]);
    assert!(matches!(r.bundle, BundleCheck::Failed { .. }), "{text}");
    assert!(text.contains("content changed"), "{text}");
    assert!(text.ends_with("Result: PROBLEMS FOUND\n"), "{text}");
    // The list is cut at the limit.
    assert!(verify_text(&r, 1).contains("… and 2 more"));
}

#[test]
fn compare_before_and_after_a_restore() {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&make_backup(t.path())).unwrap();
    let target = new_pc(t.path());
    let bundle = Bundle::open(&b.bundle_path().unwrap(), &pass()).unwrap();
    let p = plan_for(&b, &target, &opts(), Some(&bundle));

    let before = compare(&p, &mut quiet);
    assert!(!before.is_good());
    assert!(before.count(State::Missing) > 5);
    assert_eq!(before.count(State::Same), 0);

    // A different todo.txt already on the new PC: kept, the backup's beside it.
    let todo = p
        .files
        .iter()
        .find(|f| f.dst.ends_with(r"\Desktop\todo.txt"))
        .expect("todo.txt in the plan");
    let todo_local = target.local(&todo.dst);
    std::fs::create_dir_all(todo_local.parent().unwrap()).unwrap();
    std::fs::write(&todo_local, b"the new PC's own list").unwrap();
    let docs_lnk = target.local(r"C:\Users\sam\Desktop\Docs.lnk");
    std::fs::write(&docs_lnk, b"the new PC's own shortcut").unwrap();
    let r = execute(&p, &TestHost::default(), &mut Ui::default(), false);
    assert_eq!(r.failed(), 0);

    let after = compare(&p, &mut quiet);
    let text = compare_text(&after, 50);
    assert!(after.is_good(), "{text}");
    let state = |dst_end: &str| {
        after
            .entries
            .iter()
            .find(|e| e.path.ends_with(dst_end))
            .map(|e| e.state)
    };
    assert_eq!(state(r"\Desktop\todo.txt"), Some(State::Beside), "{text}");
    assert_eq!(state(r"\Desktop\Docs.lnk"), Some(State::KeptOwn), "{text}");
    assert_eq!(state(r"\.gitconfig"), Some(State::Rewritten), "{text}");
    assert!(after.count(State::Same) > 5, "{text}");
    assert!(
        text.contains("Other users in the backup, not compared: bob"),
        "{text}"
    );
    assert!(text.ends_with("Result: OK\n"), "{text}");

    // Changed later on the PC, replaced by older content, deleted.
    let same: Vec<_> = after
        .entries
        .iter()
        .filter(|e| e.state == State::Same)
        .filter(|e| p.files.iter().any(|f| f.dst == e.path && f.merge.is_none()))
        .map(|e| e.path.clone())
        .collect();
    // The fixture was written moments ago, so "later" is an hour ahead.
    let edit = |win: &str, body: &[u8], when: SystemTime| {
        let l = target.local(win);
        std::fs::write(&l, body).unwrap();
        let f = std::fs::File::options().write(true).open(&l).unwrap();
        f.set_modified(when).unwrap();
    };
    let now = SystemTime::now();
    edit(&same[0], b"edited later", now + Duration::from_secs(3600));
    let long_ago = now - Duration::from_secs(20 * 365 * 86_400);
    edit(&same[1], b"an old copy", long_ago);
    std::fs::remove_file(target.local(&same[2])).unwrap();
    let r = compare(&p, &mut quiet);
    let text = compare_text(&r, 50);
    let state = |win: &str| r.entries.iter().find(|e| e.path == win).unwrap().state;
    assert_eq!(state(&same[0]), State::Newer, "{text}");
    assert_eq!(state(&same[1]), State::Different, "{text}");
    assert_eq!(state(&same[2]), State::Missing, "{text}");
    assert!(!r.is_good());
    assert!(
        text.contains(&format!("Missing (1):\n  {}\n", same[2])),
        "{text}"
    );
    assert!(text.ends_with("Result: DIFFERENCES FOUND\n"), "{text}");
}
