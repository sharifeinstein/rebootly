//! Undo puts the new PC back the way it was.

use super::*;
use crate::restore::undo::{list, undo};

#[test]
fn undo_restores_replaced_files_and_removes_new_ones() {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&make_backup(t.path())).unwrap();
    let target = new_pc(t.path());
    let settings = r"C:\Users\sam\AppData\Roaming\Code\User\settings.json";
    let s = target.local(settings);
    std::fs::create_dir_all(s.parent().unwrap()).unwrap();
    std::fs::write(&s, "{}").unwrap();

    let p = plan_for(&b, &target, &opts(), None);
    let r = execute(&p, &TestHost::default(), &mut Ui::default(), false);
    assert_eq!(r.failed(), 0);
    assert_ne!(read(&target, settings), "{}");
    // The user edits one restored file afterwards: undo must keep it.
    let todo = r"C:\Users\sam\Desktop\todo.txt";
    std::fs::write(target.local(todo), "edited after restore").unwrap();

    let runs = list(&target);
    assert_eq!(runs.len(), 1);
    let dir = &runs[0].0;
    let host = TestHost::default();
    let dry = undo(dir, &target, &host, true, false).unwrap();
    assert!(
        dry.done.iter().all(|d| d.starts_with("would ")),
        "{:?}",
        dry.done
    );
    assert!(target.local(r"C:\Users\sam\Documents\report.txt").exists());

    let u = undo(dir, &target, &host, false, false).unwrap();
    assert!(u.failed.is_empty(), "{:?}", u.failed);
    assert_eq!(read(&target, settings), "{}");
    assert!(!target.local(r"C:\Users\sam\Documents\report.txt").exists());
    assert!(
        !target.local(r"C:\Users\sam\Documents").exists(),
        "created folders removed"
    );
    assert!(!target.local(r"C:\Users\sam\.gitconfig").exists());
    assert_eq!(read(&target, todo), "edited after restore");
    assert!(u.kept.iter().any(|k| k.contains("todo.txt")));
    assert_eq!(
        read(&target, r"C:\Windows\System32\drivers\etc\hosts"),
        "# stock\r\n"
    );
    let ops = host.ops.borrow();
    assert!(
        ops.iter().any(|o| matches!(o, SysOp::RegImport(_))),
        "registry put back"
    );
    assert!(
        ops.iter()
            .any(|o| matches!(o, SysOp::TaskDelete(n) if n == r"\Vendor\Updater"))
    );
    assert!(list(&target).is_empty(), "undone runs aren't offered again");
}
