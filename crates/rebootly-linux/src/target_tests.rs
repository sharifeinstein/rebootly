use super::*;
use crate::cmd::fake::Fake;

fn vol(dev: &str, fs: &str, label: Option<&str>, state: VolumeState) -> Volume {
    Volume {
        device: dev.into(),
        label: label.map(str::to_owned),
        size: Some(64 << 30),
        fs: Some(fs.into()),
        state,
        id: None,
        removable: true,
    }
}

struct Dirs {
    base: tempfile::TempDir,
    media: tempfile::TempDir,
}

fn dirs() -> Dirs {
    Dirs {
        base: tempfile::tempdir().unwrap(),
        media: tempfile::tempdir().unwrap(),
    }
}

#[test]
fn exfat_is_probed_then_mounted_read_write_under_media_and_closed_later() {
    let d = dirs();
    let fake = Fake::default();
    fake.expect("mount -t exfat -o ro /dev/sdb1", 0, "", "")
        .expect("umount", 0, "", "")
        .expect("mount -t exfat -o rw,noatime /dev/sdb1", 0, "", "");
    let t = TargetMounter::new(&fake, d.base.path(), d.media.path());
    let v = vol(
        "/dev/sdb1",
        "exfat",
        Some("My Stick"),
        VolumeState::Unmounted,
    );
    let root = t.mount(&v).unwrap();
    assert_eq!(root, d.media.path().join("My_Stick"));
    assert!(root.is_dir());
    let lines = fake.lines();
    assert!(lines[2].ends_with(&root.display().to_string()), "{lines:?}");

    fake.expect("sync", 0, "", "").expect("umount", 0, "", "");
    assert!(t.close_all().is_empty());
    assert_eq!(fake.lines()[4], format!("umount {}", root.display()));
    assert!(!root.exists());
    assert!(!d.base.path().join("targets").exists());
}

#[test]
fn ntfs_falls_back_to_ntfs_3g_and_a_busy_name_gets_a_suffix() {
    let d = dirs();
    let taken = d.media.path().join("sdc1");
    std::fs::create_dir_all(taken.join("x")).unwrap();
    let fake = Fake::default();
    fake.expect("mount -t ntfs3 -o ro", 0, "", "")
        .expect("umount", 0, "", "")
        .expect(
            "mount -t ntfs3 -o rw,noatime",
            32,
            "",
            "unknown filesystem type 'ntfs3'",
        )
        .expect("mount -t ntfs-3g -o rw,noatime /dev/sdc1", 0, "", "");
    let t = TargetMounter::new(&fake, d.base.path(), d.media.path());
    let root = t
        .mount(&vol("/dev/sdc1", "ntfs", None, VolumeState::Unmounted))
        .unwrap();
    assert_eq!(root, d.media.path().join("sdc1-2"));
}

#[test]
fn a_disk_with_windows_on_it_is_refused() {
    let d = dirs();
    let probe = d.base.path().join("probe-sda3");
    let cfg = probe.join("Windows/System32/config");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("SOFTWARE"), b"x").unwrap();
    let fake = Fake::default();
    fake.expect("mount -t ntfs3 -o ro /dev/sda3", 0, "", "")
        .expect("umount", 0, "", "");
    let t = TargetMounter::new(&fake, d.base.path(), d.media.path());
    let e = t
        .mount(&vol(
            "/dev/sda3",
            "ntfs",
            Some("OS"),
            VolumeState::Unmounted,
        ))
        .unwrap_err();
    assert!(e.to_string().contains("has Windows on it"), "{e}");
    assert_eq!(fake.lines().len(), 2, "never mounted for writing");
    assert!(std::fs::read_dir(d.media.path()).unwrap().next().is_none());
}

#[test]
fn locked_and_read_only_mounts_are_refused_and_rw_mounts_used_as_they_are() {
    let d = dirs();
    let fake = Fake::default();
    let t = TargetMounter::new(&fake, d.base.path(), d.media.path());
    let e = t
        .mount(&vol("/dev/sda3", "BitLocker", None, VolumeState::Locked))
        .unwrap_err();
    assert!(e.to_string().contains("locked"));
    let ro = VolumeState::Mounted {
        root: "/run/rebootly/sdb1".into(),
        read_only: true,
    };
    assert!(
        t.mount(&vol("/dev/sdb1", "ntfs", None, ro))
            .unwrap_err()
            .to_string()
            .contains("read-only")
    );
    let rw = VolumeState::Mounted {
        root: "/media/usb".into(),
        read_only: false,
    };
    assert_eq!(
        t.mount(&vol("/dev/sdb1", "exfat", None, rw)).unwrap(),
        PathBuf::from("/media/usb")
    );
    assert!(fake.lines().is_empty());
}

#[test]
fn a_busy_ventoy_partition_says_what_to_do() {
    let d = dirs();
    let fake = Fake::default();
    fake.expect(
        "mount -t exfat -o ro",
        32,
        "",
        "mount: /x: /dev/sdb1 already mounted or mount point busy.",
    );
    let t = TargetMounter::new(&fake, d.base.path(), d.media.path());
    let e = t
        .mount(&vol(
            "/dev/sdb1",
            "exfat",
            Some("Ventoy"),
            VolumeState::Unmounted,
        ))
        .unwrap_err();
    assert!(e.to_string().contains("VTOY_LINUX_REMOUNT"), "{e}");
}

#[test]
fn the_stick_is_found_by_its_marker() {
    let d = dirs();
    let mounted = tempfile::tempdir().unwrap();
    // sdb1: unmounted, no marker. sdc1: unmounted, has the marker (the
    // fake mount doesn't mount, so the marker is put where the probe looks).
    let probe = d.base.path().join("probe-sdc1");
    std::fs::create_dir_all(&probe).unwrap();
    std::fs::write(probe.join(MARKER), b"").unwrap();
    let fake = Fake::default();
    fake.expect("mount -t ntfs3 -o ro /dev/sdb1", 0, "", "")
        .expect("umount", 0, "", "")
        .expect("mount -t exfat -o ro /dev/sdc1", 0, "", "")
        .expect("umount", 0, "", "");
    let vols = vec![
        vol("/dev/sda3", "BitLocker", None, VolumeState::Locked),
        vol(
            "/dev/sda4",
            "ntfs",
            None,
            VolumeState::Mounted {
                root: mounted.path().into(),
                read_only: false,
            },
        ),
        vol("/dev/sdb1", "ntfs", None, VolumeState::Unmounted),
        vol("/dev/sdc1", "exfat", Some("Ventoy"), VolumeState::Unmounted),
    ];
    let t = TargetMounter::new(&fake, d.base.path(), d.media.path());
    assert_eq!(
        t.find_stick(&vols).map(|v| v.device.as_str()),
        Some("/dev/sdc1")
    );

    // A mounted stick is looked at in place.
    std::fs::write(mounted.path().join(MARKER), b"").unwrap();
    assert_eq!(
        t.find_stick(&vols).map(|v| v.device.as_str()),
        Some("/dev/sda4")
    );
}

#[test]
fn candidates_skip_locked_and_read_write_volumes() {
    let vols = vec![
        vol("/dev/sda3", "BitLocker", None, VolumeState::Locked),
        vol("/dev/sdb1", "exfat", None, VolumeState::Unmounted),
        vol(
            "/dev/sdc1",
            "ntfs",
            None,
            VolumeState::Mounted {
                root: "/media/x".into(),
                read_only: false,
            },
        ),
        vol(
            "/dev/sdd1",
            "ntfs",
            None,
            VolumeState::Mounted {
                root: "/run/rebootly/sdd1".into(),
                read_only: true,
            },
        ),
    ];
    let c: Vec<_> = candidates(&vols).into_iter().map(|v| v.device).collect();
    assert_eq!(c, ["/dev/sdb1", "/dev/sdd1"]);
}
