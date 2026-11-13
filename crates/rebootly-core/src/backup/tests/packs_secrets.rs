//! Saved passwords in pack files and registry keys (FileZilla, PuTTY,
//! WinSCP): stripped from the plain backup, whole in the encrypted bundle.

use super::*;
use crate::backup::manifest::PackPathStatus;
use crate::backup::sensitive::{self, Passphrase};

const FZ: &str = "AppData/Roaming/FileZilla";
const SITES: &str = "<FileZilla3>\r\n<Servers>\r\n<Server>\r\n<Host>ftp.example.org</Host>\r\n\
                     <User>al</User>\r\n<Pass encoding=\"base64\">c2VjcmV0</Pass>\r\n\
                     </Server>\r\n</Servers>\r\n</FileZilla3>\r\n";
const SETTINGS: &str = "<FileZilla3><Settings>\r\n<Setting name=\"Proxy host\">p</Setting>\r\n\
                        <Setting name=\"Proxy password\">pw</Setting>\r\n</Settings></FileZilla3>\r\n";
const RECENT: &str =
    "<FileZilla3><RecentServers><Server><Host>h</Host></Server></RecentServers></FileZilla3>";

/// The fixture with FileZilla files and PuTTY/WinSCP keys for Alice.
pub(super) fn volume_with_secrets() -> (tempfile::TempDir, PathBuf) {
    let (t, root) = volume();
    let home = root.join("Users/alice");
    for (name, body) in [
        ("sitemanager.xml", SITES),
        ("filezilla.xml", SETTINGS),
        ("recentservers.xml", RECENT),
        ("lockfile", ""),
    ] {
        let p = home.join(FZ).join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    let mut w = crate::fixtures::alice();
    w.key(r"Software\Martin Prikryl\WinSCP 2\Sessions\nas")
        .sz("HostName", "nas.local")
        .sz("Password", "A35C4E");
    w.key(r"Software\SimonTatham\PuTTY\Sessions\router")
        .sz("HostName", "10.0.0.1")
        .sz("ProxyPassword", "pp");
    std::fs::write(home.join("NTUSER.DAT"), w.to_bytes()).unwrap();
    (t, root)
}

fn opts(bundle: bool) -> PlanOptions {
    PlanOptions {
        users: vec!["Alice".into()],
        packs: crate::packs::load::load_all(None).0,
        sensitive_bundle: bundle,
        ..Default::default()
    }
}

fn pass() -> Passphrase {
    Passphrase::new("correct horse battery".into()).unwrap()
}

fn utf16(b: &[u8]) -> String {
    let u: Vec<u16> = b[2..]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u).unwrap()
}

#[test]
fn without_a_passphrase_the_passwords_are_stripped() {
    let (t, root) = volume_with_secrets();
    let src = OfflineVolume::open(&root).unwrap();
    let p = plan(&src, &opts(false)).unwrap();
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    assert_eq!(m.counts().2, 0);
    let fz = job.dir.join("users/Alice/packs/filezilla/FileZilla");
    let sites = std::fs::read_to_string(fz.join("sitemanager.xml")).unwrap();
    assert!(
        sites.contains("<Host>ftp.example.org</Host>") && !sites.contains("<Pass"),
        "{sites}"
    );
    let settings = std::fs::read_to_string(fz.join("filezilla.xml")).unwrap();
    assert!(settings.contains("Proxy host") && !settings.contains("Proxy password"));
    assert_eq!(
        std::fs::read_to_string(fz.join("recentservers.xml")).unwrap(),
        RECENT
    );
    assert!(!fz.join("lockfile").exists());
    assert_eq!(
        m.files
            .iter()
            .filter(|f| f.path.ends_with("FileZilla/sitemanager.xml"))
            .count(),
        1,
        "copied once, stripped"
    );

    let rec = |id: &str| m.packs.iter().find(|r| r.id == id).unwrap();
    let s = rec("filezilla").secrets.as_ref().unwrap();
    assert_eq!(s.status, PackPathStatus::SensitiveNotCopied);
    assert_eq!(s.files.len(), 2, "{:?}", s.files);
    let reg = |id: &str| {
        utf16(&std::fs::read(job.dir.join(rec(id).registry_file.as_ref().unwrap())).unwrap())
    };
    let winscp = reg("winscp");
    assert!(
        winscp.contains("\"HostName\"=\"nas.local\"") && !winscp.contains("A35C4E"),
        "{winscp}"
    );
    assert_eq!(rec("winscp").secrets.as_ref().unwrap().registry_values, 1);
    let putty = reg("putty");
    assert!(putty.contains("10.0.0.1") && !putty.contains("ProxyPassword"));

    let step = m
        .manual_steps
        .iter()
        .find(|s| s.title == "Saved passwords of Alice were not copied")
        .unwrap();
    for name in ["FileZilla", "PuTTY", "WinSCP"] {
        assert!(step.detail.contains(name), "{}", step.detail);
    }
    assert!(m.sensitive.is_none());
}

#[test]
fn with_a_passphrase_the_bundle_has_the_whole_copies() {
    let (t, root) = volume_with_secrets();
    let src = OfflineVolume::open(&root).unwrap();
    let p = plan(&src, &opts(true)).unwrap();
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let ro = RunOptions {
        passphrase: Some(pass()),
        bundle_work_factor: Some(2),
        ..quick()
    };
    let m = run(&src, &p, &job, &ro, &mut |_| {}).unwrap();
    assert_eq!(m.counts().2, 0);
    let fz = job.dir.join("users/Alice/packs/filezilla/FileZilla");
    assert!(
        !std::fs::read_to_string(fz.join("sitemanager.xml"))
            .unwrap()
            .contains("<Pass")
    );
    let s = m.packs.iter().find(|r| r.id == "winscp").unwrap();
    assert_eq!(
        s.secrets.as_ref().unwrap().status,
        PackPathStatus::Encrypted
    );
    assert!(
        !m.manual_steps
            .iter()
            .any(|s| s.title.starts_with("Saved passwords"))
    );

    let out = t.path().join("open");
    let index = sensitive::extract(&job.dir.join("sensitive.age"), &pass(), &out).unwrap();
    let inside = |rel: &str| {
        assert!(
            index.files.iter().any(|f| f.path == rel),
            "{rel} not in the bundle"
        );
        std::fs::read_to_string(out.join(rel)).unwrap()
    };
    assert_eq!(
        inside("users/Alice/packs/filezilla/FileZilla/sitemanager.xml"),
        SITES
    );
    assert_eq!(
        inside("users/Alice/packs/filezilla/FileZilla/filezilla.xml"),
        SETTINGS
    );
    assert!(
        !index
            .files
            .iter()
            .any(|f| f.path.ends_with("recentservers.xml")),
        "no passwords in it: plain only"
    );
    let full = inside("users/Alice/registry/winscp.reg");
    assert!(
        full.contains("\"Password\"=\"A35C4E\"") && full.contains("nas.local"),
        "{full}"
    );
    assert!(inside("users/Alice/registry/putty.reg").contains("\"ProxyPassword\"=\"pp\""));
}
