//! Real tools, real NTFS: set `REBOOTLY_TEST_NTFS_DEV` to a block device
//! holding a copy of `tests/fixtures/win11-basic` (and a `HIBR` hiberfil.sys),
//! run as root. Skipped otherwise. Setup used in the cloud workspace:
//! `truncate -s 64M t.img; mkntfs -F -f -L Windows t.img; losetup -f --show t.img`,
//! mount it with ntfs-3g, copy the fixture in, unmount.

use rebootly_core::volume::{HIBERNATED_NOTE, VolumeState};
use rebootly_core::{OfflineVolume, Source};
use rebootly_linux::{Mounter, cmd::System, discover};

#[test]
fn discover_mount_read_release() {
    let Ok(dev) = std::env::var("REBOOTLY_TEST_NTFS_DEV") else {
        return;
    };
    let vols = discover(&System).unwrap();
    let v = vols
        .iter()
        .find(|v| v.device == dev)
        .unwrap_or_else(|| panic!("{dev} not listed: {vols:?}"));
    assert_eq!(v.fs.as_deref(), Some("ntfs"));
    assert_eq!(v.label.as_deref(), Some("Windows"));
    assert_eq!(v.state, VolumeState::Unmounted);

    let base = tempfile::tempdir().unwrap();
    let m = Mounter::new(Box::new(System), base.path());
    let got = m.open(v, None).unwrap();
    // Read-only for real: writing must fail.
    assert!(std::fs::write(got.root.join("x.txt"), b"x").is_err());
    let src = OfflineVolume::open(&got.root).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/win11-basic");
    let want = OfflineVolume::open(&fixture).unwrap().profiles().unwrap();
    let got_users: Vec<String> = src.profiles().unwrap().into_iter().map(|p| p.username).collect();
    let want_users: Vec<String> = want.into_iter().map(|p| p.username).collect();
    assert_eq!(got_users, want_users);
    assert_eq!(src.source_notes(), vec![HIBERNATED_NOTE.to_string()]);
    drop(src);

    let after = discover(&System).unwrap();
    let mounted = after.iter().find(|v| v.device == dev).unwrap();
    assert!(
        matches!(mounted.state, VolumeState::Mounted { read_only: true, .. }),
        "{mounted:?}"
    );
    assert_eq!(m.release(), Vec::<String>::new());
    assert!(!got.root.exists());
}
