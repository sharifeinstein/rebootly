//! Conflict policies against files already on the new PC.

use super::*;

const DOC: &str = r"C:\Users\sam\Documents\report.txt";

fn setup() -> (tempfile::TempDir, Backup, Target) {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&make_backup(t.path())).unwrap();
    let target = new_pc(t.path());
    let p = target.local(DOC);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "mine").unwrap();
    (t, b, target)
}

fn with(policy: &str) -> RestoreOptions {
    let mut o = opts();
    o.policies.apply(&format!("folders={policy}")).unwrap();
    o
}

fn outcome<'a>(r: &'a RestoreResult, what: &str) -> &'a run::Outcome {
    r.outcomes
        .iter()
        .find(|o| o.what == what)
        .unwrap_or_else(|| panic!("no outcome for {what}"))
}

#[test]
fn keep_both_is_the_default_for_your_files() {
    let (_t, b, target) = setup();
    let p = plan_for(&b, &target, &opts(), None);
    let r = execute(&p, &TestHost::default(), &mut Ui::default(), false);
    assert_eq!(read(&target, DOC), "mine");
    let migrated = r"C:\Users\sam\Documents\report (migrated).txt";
    assert_eq!(
        read(&target, migrated),
        read_backup(&b, "users/Alice/folders/Documents/report.txt")
    );
    assert!(outcome(&r, DOC).detail.contains("report (migrated).txt"));
    // A second run finds the migrated copy and writes nothing new.
    let r = execute(&p, &TestHost::default(), &mut Ui::default(), false);
    assert_eq!(outcome(&r, DOC).status, run::Status::Skipped);
    assert!(
        !target
            .local(r"C:\Users\sam\Documents\report (migrated 2).txt")
            .exists()
    );
}

#[test]
fn skip_overwrite_newer_and_ask() {
    let (_t, b, target) = setup();
    let run_with = |o: &RestoreOptions, ui: &mut Ui| {
        let p = plan_for(&b, &target, o, None);
        execute(&p, &TestHost::default(), ui, false)
    };
    let r = run_with(&with("skip"), &mut Ui::default());
    assert_eq!(outcome(&r, DOC).status, run::Status::Skipped);
    assert_eq!(read(&target, DOC), "mine");

    // The file here was just written, so it is newer than the backup's.
    let r = run_with(&with("newer"), &mut Ui::default());
    assert!(outcome(&r, DOC).detail.contains("newer"));
    assert_eq!(read(&target, DOC), "mine");

    let mut ui = Ui {
        answer: (Policy::Overwrite, true),
        asked: 0,
    };
    let r = run_with(&with("ask"), &mut ui);
    assert_eq!(ui.asked, 1);
    assert_eq!(outcome(&r, DOC).status, run::Status::Done);
    assert_ne!(read(&target, DOC), "mine");
    let undo = r.undo_dir.unwrap();
    let saved = std::fs::read_dir(undo.join("files")).unwrap().count();
    assert_eq!(saved, 1, "the replaced file was kept for undo");
}

#[test]
fn dry_run_reports_each_decision() {
    let (_t, b, target) = setup();
    let p = plan_for(&b, &target, &with("ask"), None);
    let mut ui = Ui::default();
    let r = execute(&p, &TestHost::default(), &mut ui, true);
    assert_eq!(ui.asked, 0, "a dry run never asks");
    assert_eq!(outcome(&r, DOC).detail, "ask (exists and differs)");
    assert_eq!(read(&target, DOC), "mine");
}
