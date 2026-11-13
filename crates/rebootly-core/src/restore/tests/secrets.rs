//! Pack files and registry keys with saved passwords: restored from the
//! bundle when it is open, else the stripped plain copies.

use super::*;

const SITES: &str = "<Servers><Server><Host>ftp.example.org</Host>\
                     <Pass encoding=\"base64\">c2VjcmV0</Pass></Server></Servers>";

fn backup_with_secrets(t: &Path) -> PathBuf {
    let root = t.join("vol");
    crate::fixtures::build_win11_basic(&root).unwrap();
    let home = root.join("Users/alice");
    let fz = home.join("AppData/Roaming/FileZilla");
    std::fs::create_dir_all(&fz).unwrap();
    std::fs::write(fz.join("sitemanager.xml"), SITES).unwrap();
    let mut w = crate::fixtures::alice();
    w.key(r"Software\Martin Prikryl\WinSCP 2\Sessions\nas")
        .sz("HostName", "nas.local")
        .sz("Password", "A35C4E");
    std::fs::write(home.join("NTUSER.DAT"), w.to_bytes()).unwrap();
    let src = OfflineVolume::open(&root).unwrap();
    let opts = backup::PlanOptions {
        users: vec!["Alice".into()],
        packs: load_all(None).0,
        sensitive_bundle: true,
        ..Default::default()
    };
    let p = backup::plan(&src, &opts).unwrap();
    let job = Job::create(&t.join("usb"), None).unwrap();
    let ro = RunOptions {
        passphrase: Some(pass()),
        bundle_work_factor: Some(2),
        ..RunOptions::default()
    };
    assert!(
        backup::run(&src, &p, &job, &ro, &mut |_| {})
            .unwrap()
            .complete
    );
    job.dir
}

const DST: &str = r"C:\Users\sam\AppData\Roaming\FileZilla\sitemanager.xml";

fn sites(p: &RestorePlan) -> Vec<String> {
    p.files
        .iter()
        .filter(|f| f.dst.eq_ignore_ascii_case(DST))
        .map(|f| std::fs::read_to_string(&f.src).unwrap())
        .collect()
}

fn winscp(p: &RestorePlan) -> &str {
    &p.registry.iter().find(|r| r.pack == "winscp").unwrap().text
}

#[test]
fn the_bundle_copy_replaces_the_stripped_one() {
    let t = tempfile::tempdir().unwrap();
    let b = Backup::open(&backup_with_secrets(t.path())).unwrap();
    let target = new_pc(t.path());

    let plain = plan_for(&b, &target, &opts(), None);
    let s = sites(&plain);
    assert!(s.len() == 1 && !s[0].contains("<Pass"), "{s:?}");
    assert!(winscp(&plain).contains("nas.local") && !winscp(&plain).contains("A35C4E"));

    let bundle = Bundle::open(&b.bundle_path().unwrap(), &pass()).unwrap();
    let full = plan_for(&b, &target, &opts(), Some(&bundle));
    assert_eq!(sites(&full), [SITES], "one action, from the bundle");
    assert!(
        winscp(&full).contains("\"Password\"=\"A35C4E\""),
        "{}",
        winscp(&full)
    );

    let r = execute(&full, &TestHost::default(), &mut Ui::default(), false);
    assert_eq!(r.failed(), 0, "{}", report::result_text(&full, &r));
    assert_eq!(read(&target, DST), SITES);
}
