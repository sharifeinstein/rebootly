//! Restore end to end: a real backup of the `win11-basic` fixture restored
//! into a simulated new profile, with a recording host.

mod conflicts;
mod secrets;
mod undo;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::*;
use crate::backup::run::{Job, RunOptions};
use crate::backup::sensitive::Passphrase;
use crate::packs::load::load_all;
use crate::{OfflineVolume, backup};

pub(crate) fn pass() -> Passphrase {
    Passphrase::new("correct horse battery".into()).unwrap()
}

/// Back up the fixture (Alice and the others) with packs, the bundle,
/// hosts, a task, a phonebook and the `Projects` root folder.
pub(crate) fn make_backup(t: &Path) -> PathBuf {
    let root = t.join("vol");
    crate::fixtures::build_win11_basic(&root).unwrap();
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w(
        "Users/alice/.gitconfig",
        "[core]\n\texcludesfile = C:/Users/Alice/.gitignore_global\n",
    );
    w(
        "Users/alice/AppData/Roaming/Code/User/settings.json",
        r#"{"python.venvPath": "C:\\Users\\Alice\\venvs"}"#,
    );
    w(
        "Windows/System32/drivers/etc/hosts",
        "127.0.0.1 localhost\r\n10.0.0.5 nas\r\n",
    );
    w("Windows/System32/Tasks/Vendor/Updater", "<Task/>");
    w(
        "ProgramData/Microsoft/Network/Connections/Pbk/rasphone.pbk",
        "[Office]\r\nType=2\r\n",
    );
    let mut lnk = b"L\0\0\0".to_vec();
    lnk.extend(
        r"C:\Users\Alice\Documents"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    let sm = root.join("Users/alice/AppData/Roaming/Microsoft/Windows/Start Menu/Programs");
    std::fs::create_dir_all(&sm).unwrap();
    std::fs::write(sm.join("Old docs.lnk"), &lnk).unwrap();
    std::fs::write(root.join("Users/alice/Desktop/Docs.lnk"), &lnk).unwrap();
    let src = OfflineVolume::open(&root).unwrap();
    let opts = backup::PlanOptions {
        packs: load_all(None).0,
        sensitive_bundle: true,
        root_folders: vec!["Projects".into()],
        ..Default::default()
    };
    let p = backup::plan(&src, &opts).unwrap();
    let job = Job::create(&t.join("usb"), p.source.machine.as_deref()).unwrap();
    let ro = RunOptions {
        passphrase: Some(pass()),
        bundle_work_factor: Some(2),
        jobs: 2,
        ..RunOptions::default()
    };
    let m = backup::run(&src, &p, &job, &ro, &mut |_| {}).unwrap();
    assert!(m.complete);
    job.dir
}

#[derive(Default)]
pub(crate) struct TestHost {
    pub ops: RefCell<Vec<SysOp>>,
    pub running: Vec<String>,
}

impl RestoreHost for TestHost {
    fn sys(&self, op: &SysOp) -> Result<SysOk, String> {
        self.ops.borrow_mut().push(op.clone());
        Ok(match op {
            SysOp::RegExport { out, .. } => {
                std::fs::write(out, b"saved").unwrap();
                SysOk::Yes
            }
            SysOp::WifiExists(_) | SysOp::CertExists(_) | SysOp::TaskExists(_) => SysOk::No,
            SysOp::PowerActive => SysOk::Value("old-guid".into()),
            SysOp::PowerExists(g) => {
                if g == "381b4222-f694-41f0-9685-ff5bb260df2e" {
                    SysOk::Yes
                } else {
                    SysOk::No
                }
            }
            SysOp::PowerImport(_) => SysOk::Value("new-guid".into()),
            _ => SysOk::Done,
        })
    }
    fn running(&self) -> Vec<String> {
        self.running.clone()
    }
    fn write_stream(&self, _: &Path, _: &str, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) struct Ui {
    pub answer: (Policy, bool),
    pub asked: usize,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            answer: (Policy::Skip, false),
            asked: 0,
        }
    }
}

impl Interact for Ui {
    fn conflict(&mut self, _: &map::FileAction, _: &Path) -> (Policy, bool) {
        self.asked += 1;
        self.answer
    }
    fn apps_done(&mut self) -> bool {
        true
    }
    fn event(&mut self, _: Event<'_>) {}
}

pub(crate) fn opts() -> RestoreOptions {
    RestoreOptions {
        user: Some("Alice".into()),
        tasks: true,
        ..Default::default()
    }
}

/// New PC under `t/new`, user `sam`, with a stock hosts file.
pub(crate) fn new_pc(t: &Path) -> Target {
    let target = Target::simulated(&t.join("new"), "sam");
    let hosts = target.local(r"C:\Windows\System32\drivers\etc\hosts");
    std::fs::create_dir_all(hosts.parent().unwrap()).unwrap();
    std::fs::write(&hosts, "# stock\r\n").unwrap();
    target
}

pub(crate) fn plan_for(
    b: &Backup,
    target: &Target,
    o: &RestoreOptions,
    bundle: Option<&Bundle>,
) -> RestorePlan {
    plan(b, target, &load_all(None).0, o, bundle).unwrap()
}

pub(crate) fn read(target: &Target, win: &str) -> String {
    std::fs::read_to_string(target.local(win)).unwrap_or_else(|e| panic!("{win}: {e}"))
}

#[test]
fn restores_into_a_new_user_with_remapping() {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&make_backup(t.path())).unwrap();
    let target = new_pc(t.path());
    let bundle = Bundle::open(&b.bundle_path().unwrap(), &pass()).unwrap();
    let p = plan_for(&b, &target, &opts(), Some(&bundle));
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    assert!(p.other_users.contains_key("bob"));
    assert_eq!(p.registry.len(), 1);
    assert!(
        p.registry[0]
            .text
            .contains(r"C:\\Users\\sam\\Pictures\\beach.jpg")
    );
    let text = report::plan_text(&p);
    assert!(
        text.contains(r"old profile C:\Users\Alice → C:\Users\sam"),
        "{text}"
    );

    let docs_lnk = target.local(r"C:\Users\sam\Desktop\Docs.lnk");
    std::fs::create_dir_all(docs_lnk.parent().unwrap()).unwrap();
    std::fs::write(&docs_lnk, b"the new PC's own shortcut").unwrap();
    let host = TestHost::default();
    let r = execute(&p, &host, &mut Ui::default(), false);
    assert_eq!(r.failed(), 0, "{}", report::result_text(&p, &r));
    assert!(r.verified > 5);
    let why = |win: &str| {
        r.outcomes
            .iter()
            .find(|o| o.what == win)
            .map(|o| (o.status, o.detail.clone()))
    };
    let (st, d) = why(r"C:\Users\sam\Desktop\Docs.lnk").unwrap();
    assert!(st == run::Status::Skipped && d.contains("shortcut"), "{d}");
    assert!(
        !target
            .local(r"C:\Users\sam\Desktop\Docs (migrated).lnk")
            .exists()
    );
    let (st, d) =
        why(r"C:\Users\sam\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Old docs.lnk")
            .unwrap();
    assert!(
        st == run::Status::Skipped && d.contains("old profile"),
        "{d}"
    );
    assert_eq!(
        read(&target, r"C:\Users\sam\Documents\report.txt"),
        read_backup(&b, "users/Alice/folders/Documents/report.txt")
    );
    assert!(read(&target, r"C:\Users\sam\.gitconfig").contains("C:/Users/sam/.gitignore_global"));
    assert!(
        read(
            &target,
            r"C:\Users\sam\AppData\Roaming\Code\User\settings.json"
        )
        .contains(r"C:\\Users\\sam\\venvs")
    );
    assert!(
        target.local(r"C:\Users\sam\.ssh\id_ed25519").is_file(),
        "key from the bundle"
    );
    assert!(target.local(r"C:\Projects").is_dir(), "root folder");
    let hosts = read(&target, r"C:\Windows\System32\drivers\etc\hosts");
    assert!(
        hosts.starts_with("# stock\r\n") && hosts.contains("10.0.0.5 nas"),
        "{hosts}"
    );
    assert!(
        target
            .local(r"C:\ProgramData\Microsoft\Network\Connections\Pbk\rasphone.pbk")
            .is_file()
    );

    let ops = host.ops.borrow();
    assert!(ops.iter().any(|o| matches!(o, SysOp::RegImport(_))));
    assert!(
        ops.iter()
            .any(|o| matches!(o, SysOp::TaskCreate { name, .. } if name == r"\Vendor\Updater"))
    );
    assert_eq!(ops.last(), Some(&SysOp::RestartExplorer));
    let undo = r.undo_dir.as_ref().unwrap();
    assert!(undo.join("journal.jsonl").is_file() && undo.join("report.txt").is_file());

    // Again: everything is already there.
    let r2 = execute(&p, &TestHost::default(), &mut Ui::default(), false);
    let files_done = r2
        .outcomes
        .iter()
        .filter(|o| o.status == run::Status::Done && o.phase == run::Phase::Files)
        .count();
    assert_eq!(files_done, 0, "{}", report::result_text(&p, &r2));
}

pub(crate) fn read_backup(b: &Backup, rel: &str) -> String {
    std::fs::read_to_string(b.local(rel).unwrap()).unwrap()
}

#[test]
fn dry_run_and_simulation_change_nothing_on_the_system() {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&make_backup(t.path())).unwrap();
    let target = new_pc(t.path());
    let p = plan_for(&b, &target, &opts(), None);
    assert!(
        p.manual_steps
            .iter()
            .any(|s| s.title == "Encrypted items not restored")
    );
    let host = TestHost::default();
    let r = execute(&p, &host, &mut Ui::default(), true);
    assert!(r.undo_dir.is_none() && r.count(run::Status::Done) == 0);
    assert!(r.count(run::Status::Would) > 5);
    assert!(!target.local(r"C:\Users\sam\Documents").exists());
    assert!(
        host.ops
            .borrow()
            .iter()
            .all(|o| matches!(o, SysOp::TaskExists(_))),
        "only queries"
    );

    let r = execute(&p, &SimHost, &mut Ui::default(), false);
    assert_eq!(r.failed(), 0);
    assert!(target.local(r"C:\Users\sam\Documents\report.txt").is_file());
    assert!(
        r.outcomes
            .iter()
            .any(|o| o.phase == run::Phase::Registry && o.detail.contains("simulation"))
    );
}

#[test]
fn chooses_the_user_and_refuses_tampered_registry_files() {
    let t = tempfile::tempdir().unwrap();
    let dir = make_backup(t.path());
    let b = Backup::open(&dir).unwrap();
    let target = new_pc(t.path());
    let none = RestoreOptions {
        user: None,
        ..opts()
    };
    let e = plan(&b, &target, &load_all(None).0, &none, None).unwrap_err();
    assert!(e.to_string().contains("--user"), "{e}");
    let alice = Target::simulated(&t.path().join("n2"), "alice");
    assert_eq!(
        plan(&b, &alice, &load_all(None).0, &none, None)
            .unwrap()
            .user
            .username,
        "Alice"
    );

    let reg = dir.join("users/Alice/registry/windows-shell.reg");
    let bad = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\"x\"=\"evil.exe\"\r\n";
    std::fs::write(&reg, regcheck::encode(bad)).unwrap();
    let p = plan_for(&b, &target, &opts(), None);
    assert!(p.registry.is_empty());
    assert!(
        p.skipped.iter().any(|s| s.reason.contains("allowlist")),
        "{:?}",
        p.skipped
    );
}

#[test]
fn packs_wait_for_their_app_to_close() {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&make_backup(t.path())).unwrap();
    let target = new_pc(t.path());
    let p = plan_for(&b, &target, &opts(), None);
    let host = TestHost {
        running: vec!["CHROME.EXE".into()],
        ..Default::default()
    };
    let r = execute(&p, &host, &mut Ui::default(), false);
    let chrome = r
        .outcomes
        .iter()
        .find(|o| o.what == "Google Chrome")
        .unwrap();
    assert_eq!(chrome.status, run::Status::Skipped);
    assert!(chrome.detail.contains("chrome.exe is running"));
    assert!(
        !target
            .local(r"C:\Users\sam\AppData\Local\Google\Chrome")
            .exists()
    );
    assert!(target.local(r"C:\Users\sam\.gitconfig").exists());
}

#[test]
fn extension_folder_names_lose_their_version() {
    use super::notes::strip_version;
    assert_eq!(
        strip_version("ms-python.python-2026.1.0"),
        "ms-python.python"
    );
    assert_eq!(
        strip_version("rust-lang.rust-analyzer-0.3.2600-win32-x64"),
        "rust-lang.rust-analyzer"
    );
    assert_eq!(strip_version("plain"), "plain");
}

/// Add a plain file to a finished backup (and its manifest).
pub(crate) fn add_to_backup(dir: &Path, rel: &str, body: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, body).unwrap();
    let mpath = dir.join("manifest.json");
    let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&mpath).unwrap()).unwrap();
    m["files"].as_array_mut().unwrap().push(serde_json::json!({
        "path": rel, "source": "test", "size": body.len(), "mtime": null,
        "sha256": crate::backup::copy::hex(&sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(body))),
        "status": "ok"
    }));
    std::fs::write(&mpath, serde_json::to_vec(&m).unwrap()).unwrap();
}

#[test]
fn power_plan_existing_is_activated_custom_is_imported() {
    use super::system::{find_guid, scheme_name};
    let txt = "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)";
    assert_eq!(find_guid(txt), Some("381b4222-f694-41f0-9685-ff5bb260df2e"));
    assert_eq!(scheme_name(txt).as_deref(), Some("Balanced"));
    assert_eq!(scheme_name("GUID: x"), None);

    for (guid, imports) in [
        ("381b4222-f694-41f0-9685-ff5bb260df2e", false),
        ("11111111-2222-3333-4444-555555555555", true),
    ] {
        let t = tempfile::tempdir().unwrap();
        let dir = make_backup(t.path());
        add_to_backup(&dir, "system/power/active-scheme.pow", b"pow");
        add_to_backup(
            &dir,
            "system/power/active-scheme.txt",
            format!("Power Scheme GUID: {guid}  (Mine)").as_bytes(),
        );
        let b = Backup::open(&dir).unwrap();
        let target = new_pc(t.path());
        let p = plan_for(&b, &target, &opts(), None);
        let host = TestHost::default();
        let r = execute(&p, &host, &mut Ui::default(), false);
        assert_eq!(r.failed(), 0, "{}", report::result_text(&p, &r));
        let ops = host.ops.borrow();
        assert_eq!(
            ops.iter().any(|o| matches!(o, SysOp::PowerImport(_))),
            imports,
            "{guid}"
        );
        let activated = if imports { "new-guid" } else { guid };
        assert!(
            ops.contains(&SysOp::PowerSetActive(activated.into())),
            "{ops:?}"
        );
        drop(ops);
        let host = TestHost::default();
        super::undo::undo(r.undo_dir.as_ref().unwrap(), &target, &host, false, false).unwrap();
        let ops = host.ops.borrow();
        assert!(ops.contains(&SysOp::PowerSetActive("old-guid".into())));
        assert_eq!(
            ops.iter().any(|o| matches!(o, SysOp::PowerDelete(_))),
            imports
        );
    }
}
