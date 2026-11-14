//! Engine tests for the app packs added in M9 (one fixture test per pack).

use super::*;

pub(super) fn write(root: &Path, rel: &str, body: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// Plan and run a backup of Alice with every built-in pack; returns the
/// job folder of Alice's copy of `pack` and the manifest.
pub(super) fn backup_pack(
    files: &[(&str, &[u8])],
    pack: &str,
) -> (tempfile::TempDir, PathBuf, manifest::Manifest) {
    let (t, root) = volume();
    let home = root.join("Users/alice");
    for (rel, body) in files {
        write(&home, rel, body);
    }
    let src = OfflineVolume::open(&root).unwrap();
    let opts = PlanOptions {
        users: vec!["Alice".into()],
        packs: crate::packs::load::load_all(None).0,
        ..Default::default()
    };
    let p = plan(&src, &opts).unwrap();
    assert!(
        p.packs.iter().any(|r| r.user == "Alice" && r.id == pack),
        "{pack} not detected"
    );
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    assert_eq!(m.counts().2, 0, "no failed files");
    let dir = job.dir.join("users/Alice/packs").join(pack);
    (t, dir, m)
}

#[test]
fn brave_copies_the_allowlist_only() {
    let ud = "AppData/Local/BraveSoftware/Brave-Browser/User Data";
    let f = |rel: &str| format!("{ud}/{rel}");
    let files = [
        f("Local State"),
        f("Default/Bookmarks"),
        f("Default/Preferences"),
        f("Default/Login Data"),
        f("Default/Cookies"),
        f("Default/Cache/Cache_Data/data_0"),
        f("Profile 1/History"),
        f("Profile 1/Local Extension Settings/abc/000003.log"),
        f("Profile 1/Local Extension Settings/abc/LOCK"),
        f("BraveWallet/x.json"),
    ];
    let files: Vec<(&str, &[u8])> = files.iter().map(|s| (s.as_str(), &b"x"[..])).collect();
    let (_t, dir, m) = backup_pack(&files, "brave");
    let ud = dir.join("User Data");
    assert!(ud.join("Default/Bookmarks").is_file());
    assert!(ud.join("Default/Preferences").is_file());
    assert!(ud.join("Profile 1/History").is_file());
    assert!(
        ud.join("Profile 1/Local Extension Settings/abc/000003.log")
            .is_file()
    );
    for gone in [
        "Local State",
        "Default/Login Data",
        "Default/Cookies",
        "Default/Cache",
        "Profile 1/Local Extension Settings/abc/LOCK",
        "BraveWallet",
    ] {
        assert!(!ud.join(gone).exists(), "{gone} copied");
    }
    let r = m.packs.iter().find(|r| r.id == "brave").unwrap();
    assert_eq!(r.winget_id.as_deref(), Some("Brave.Brave"));
}

#[test]
fn thunderbird_copies_profiles_without_secrets() {
    let tb = "AppData/Roaming/Thunderbird";
    let p = "Profiles/ab12cd34.default-release";
    let f = |rel: &str| format!("{tb}/{rel}");
    let files = [
        f("profiles.ini"),
        f("installs.ini"),
        f(&format!("{p}/prefs.js")),
        f(&format!("{p}/abook.sqlite")),
        f(&format!("{p}/Mail/Local Folders/Inbox")),
        f(&format!("{p}/ImapMail/imap.example.org/INBOX.msf")),
        f(&format!("{p}/calendar-data/local.sqlite")),
        f(&format!("{p}/pubring.gpg")),
        f(&format!("{p}/logins.json")),
        f(&format!("{p}/key4.db")),
        f(&format!("{p}/cookies.sqlite")),
        f(&format!("{p}/cookies.sqlite-wal")),
        f(&format!("{p}/secring.gpg")),
        f(&format!("{p}/encrypted-openpgp-passphrase.txt")),
        f(&format!("{p}/parent.lock")),
        f(&format!("{p}/crashes/events/x")),
        f("Crash Reports/InstallTime"),
    ];
    let files: Vec<(&str, &[u8])> = files.iter().map(|s| (s.as_str(), &b"x"[..])).collect();
    let (_t, dir, _m) = backup_pack(&files, "thunderbird");
    let tb = dir.join("Thunderbird");
    assert!(tb.join("profiles.ini").is_file() && tb.join("installs.ini").is_file());
    let prof = tb.join(p);
    for kept in [
        "prefs.js",
        "abook.sqlite",
        "Mail/Local Folders/Inbox",
        "ImapMail/imap.example.org/INBOX.msf",
        "calendar-data/local.sqlite",
        "pubring.gpg",
    ] {
        assert!(prof.join(kept).is_file(), "{kept} missing");
    }
    for gone in [
        "logins.json",
        "key4.db",
        "cookies.sqlite",
        "cookies.sqlite-wal",
        "secring.gpg",
        "encrypted-openpgp-passphrase.txt",
        "parent.lock",
        "crashes",
    ] {
        assert!(!prof.join(gone).exists(), "{gone} copied");
    }
    assert!(!tb.join("Crash Reports").exists());
}

#[test]
fn sticky_notes_copies_the_database_set() {
    let ls = "AppData/Local/Packages/Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe/LocalState";
    let f = |rel: &str| format!("{ls}/{rel}");
    let files = [
        f("plum.sqlite"),
        f("plum.sqlite-wal"),
        f("plum.sqlite-shm"),
        f("Legacy/ThresholdNotes.snt"),
        f("ink/cache.bin"),
    ];
    let files: Vec<(&str, &[u8])> = files.iter().map(|s| (s.as_str(), &b"x"[..])).collect();
    let (_t, dir, m) = backup_pack(&files, "sticky-notes");
    let ls = dir.join("LocalState");
    for kept in [
        "plum.sqlite",
        "plum.sqlite-wal",
        "plum.sqlite-shm",
        "Legacy/ThresholdNotes.snt",
    ] {
        assert!(ls.join(kept).is_file(), "{kept} missing");
    }
    assert!(!ls.join("ink").exists());
    let r = m.packs.iter().find(|r| r.id == "sticky-notes").unwrap();
    assert_eq!(r.requires_closed, ["Microsoft.Notes.exe"]);
}
