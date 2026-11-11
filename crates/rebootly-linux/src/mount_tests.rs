use super::*;
use rebootly_core::volume::{Password, RecoveryKey};
use crate::cmd::fake::Fake;
use std::sync::Arc;

const KEY: &str = "000000-000011-123453-720885-111111-222222-333333-444444";

/// A `Runner` handle that shares the fake with the test.
struct Shared(Arc<Fake>);
impl Runner for Shared {
    fn run(&self, p: &str, a: &[&str], s: Option<&[u8]>) -> std::io::Result<crate::cmd::Output> {
        self.0.run(p, a, s)
    }
    fn has(&self, p: &str) -> bool {
        self.0.has(p)
    }
}

fn setup(missing: Vec<&'static str>) -> (Arc<Fake>, Mounter, tempfile::TempDir) {
    let fake = Arc::new(Fake {
        missing,
        ..Fake::default()
    });
    let base = tempfile::tempdir().unwrap();
    let m = Mounter::new(Box::new(Shared(fake.clone())), base.path());
    (fake, m, base)
}

fn locked() -> Volume {
    Volume {
        device: "/dev/sda3".into(),
        label: None,
        size: None,
        fs: Some("BitLocker".into()),
        state: VolumeState::Locked,
        id: None,
        removable: false,
    }
}

#[test]
fn cryptsetup_gets_the_key_on_stdin_and_everything_is_undone() {
    let (fake, m, base) = setup(vec![]);
    fake.expect("cryptsetup open --type bitlk --readonly /dev/sda3 rebootly-sda3", 0, "", "")
        .expect("mount -t ntfs3 -o ro /dev/mapper/rebootly-sda3", 0, "", "")
        .expect("umount", 0, "", "")
        .expect("cryptsetup close rebootly-sda3", 0, "", "");
    let key: Unlock = RecoveryKey::parse(KEY).unwrap().into();
    let got = m.open(&locked(), Some(&key)).unwrap();
    assert_eq!(got.root, base.path().join("sda3"));
    assert!(got.root.is_dir());

    let calls = fake.calls.lock().unwrap().clone();
    assert!(calls.iter().all(|c| !c.line().contains("123453")));
    assert_eq!(calls[0].stdin.as_deref(), Some(format!("{KEY}\n").as_bytes()));

    assert!(m.release().is_empty());
    let lines = fake.lines();
    assert!(lines[2].starts_with("umount"), "{lines:?}");
    assert_eq!(lines[3], "cryptsetup close rebootly-sda3");
    assert!(!base.path().join("sda3").exists());
}

#[test]
fn a_wrong_key_says_so_without_the_key() {
    let (fake, m, _b) = setup(vec![]);
    fake.expect("cryptsetup open", 2, "", "No key available with this passphrase.");
    let key: Unlock = RecoveryKey::parse(KEY).unwrap().into();
    let e = m.open(&locked(), Some(&key)).unwrap_err();
    assert!(matches!(e, MountError::WrongKey("recovery key")), "{e}");
}

#[test]
fn a_password_goes_to_cryptsetup_on_stdin_and_to_dislocker_as_user_password() {
    let (fake, m, _b) = setup(vec![]);
    fake.expect("cryptsetup open --type bitlk --readonly /dev/sda3", 2, "", "No key available.");
    let pw: Unlock = Password::new("Sommer 2026!").unwrap().into();
    let e = m.unlock("/dev/sda3", &pw).unwrap_err();
    assert_eq!(e.to_string(), "the password doesn't unlock this disk; check it and try again");
    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(calls[0].stdin.as_deref(), Some(&b"Sommer 2026!\n"[..]));

    let (fake, m, _b) = setup(vec!["cryptsetup"]);
    fake.expect("dislocker -r -V /dev/sda3 -uSommer 2026!", 1, "", "Sommer 2026! wrong user password");
    let e = m.unlock("/dev/sda3", &pw).unwrap_err().to_string();
    assert!(e.contains("password doesn't unlock") && !e.contains("Sommer"), "{e}");
}

#[test]
fn falls_back_to_dislocker_and_loop_mounts_its_file() {
    let (fake, m, base) = setup(vec![]);
    fake.expect("cryptsetup open", 1, "", &format!("Unknown type bitlk {KEY}"))
        .expect("dislocker -r -V /dev/sda3 -p", 0, "", "")
        .expect("mount -t ntfs3 -o ro,loop", 0, "", "")
        .expect("umount", 0, "", "")
        .expect("umount", 0, "", "");
    let key: Unlock = RecoveryKey::parse(KEY).unwrap().into();
    let got = m.open(&locked(), Some(&key)).unwrap();
    assert_eq!(got.root, base.path().join("sda3"));
    let lines = fake.lines();
    // Built with join, not "/": the tests also run on Windows (`\`).
    let file = base.path().join("fve-sda3").join("dislocker-file");
    assert!(lines[2].contains(&file.display().to_string()), "{lines:?}");
    assert!(m.release().is_empty());
}

#[test]
fn no_unlock_tool_and_tool_errors_are_scrubbed() {
    let (_f, m, _b) = setup(vec!["cryptsetup", "dislocker"]);
    let key: Unlock = RecoveryKey::parse(KEY).unwrap().into();
    assert!(matches!(m.unlock("/dev/sda3", &key), Err(MountError::NoUnlockTool)));

    let (fake, m, _b) = setup(vec!["dislocker"]);
    fake.expect("cryptsetup open", 1, "", &format!("device busy {KEY}"));
    let e = m.unlock("/dev/sda3", &key).unwrap_err().to_string();
    assert!(e.contains("device busy") && !e.contains("123453"), "{e}");
}

#[test]
fn mount_falls_back_to_ntfs3g_and_reports_hibernation() {
    let (fake, m, base) = setup(vec![]);
    let mut v = locked();
    v.state = VolumeState::Unmounted;
    v.device = "/dev/sdb1".into();
    fake.expect("mount -t ntfs3", 32, "", "mount: unknown filesystem type 'ntfs3'.")
        .expect("mount -t ntfs-3g -o ro /dev/sdb1", 0, "", "Windows is hibernated, mounted read-only")
        .expect("umount", 0, "", "");
    let got = m.open(&v, None).unwrap();
    assert_eq!(got.root, base.path().join("sdb1"));
    assert_eq!(got.notes, vec![rebootly_core::volume::HIBERNATED_NOTE.to_string()]);
    drop(m); // released on drop
    assert!(fake.lines().last().unwrap().starts_with("umount"));
}

#[test]
fn nothing_mounts_is_a_clear_error_and_keep_leaves_mounts() {
    let (fake, m, _b) = setup(vec!["ntfs-3g"]);
    let mut v = locked();
    v.state = VolumeState::Unmounted;
    fake.expect("mount -t ntfs3", 32, "", "wrong fs type")
        .expect("mount -t ntfs-3g", 32, "", "unknown filesystem type 'ntfs-3g'");
    let e = m.open(&v, None).unwrap_err().to_string();
    assert!(e.contains("mounting read-only failed"), "{e}");

    let (fake, mut m, _b) = setup(vec![]);
    fake.expect("mount -t ntfs3", 0, "", "");
    m.open(&v, None).unwrap();
    m.keep();
    drop(m);
    assert_eq!(fake.lines().len(), 1);
}

#[test]
fn close_all_unmounts_deepest_first_and_closes_mappings() {
    let (fake, m, base) = setup(vec![]);
    std::fs::create_dir_all(base.path().join("fve-sda3")).unwrap();
    std::fs::create_dir_all(base.path().join("sda3")).unwrap();
    let b = base.path().display();
    let info = format!(
        "1 0 8:1 / / rw - ext4 /dev/vda rw\n\
         2 1 0:5 / {b}/fve-sda3 ro - fuse x ro\n\
         3 1 7:0 / {b}/sda3 ro - ntfs3 /dev/loop0 ro\n"
    );
    fake.expect("umount", 0, "", "")
        .expect("umount", 0, "", "")
        .expect("cryptsetup close rebootly-sdb2", 0, "", "");
    let mapper = vec!["control".to_string(), "rebootly-sdb2".to_string()];
    let errs = close_all_in(m.runner(), base.path(), &info, &mapper);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = fake.lines();
    assert_eq!(lines[0], format!("umount {b}/fve-sda3"));
    assert_eq!(lines[1], format!("umount {b}/sda3"));
    assert!(!base.path().join("sda3").exists());
}
