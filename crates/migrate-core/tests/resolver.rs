//! End-to-end resolver tests against `tests/fixtures/win11-basic`.

use std::path::{Path, PathBuf};

use migrate_core::folders::FolderOrigin;
use migrate_core::inspect::{UserReport, inspect};
use migrate_core::install::WindowsInstall;
use migrate_core::profiles::AccountKind;
use migrate_core::winpath::Mapped;
use migrate_core::{OfflineVolume, Source};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

fn user<'a>(users: &'a [UserReport], name: &str) -> &'a UserReport {
    users
        .iter()
        .find(|u| u.profile.username.eq_ignore_ascii_case(name))
        .unwrap()
}

fn folder<'a>(u: &'a UserReport, name: &str) -> &'a migrate_core::source::ResolvedFolder {
    u.folders.iter().find(|f| f.path.name == name).unwrap()
}

#[test]
fn committed_fixture_matches_generator() {
    let tmp = tempfile::tempdir().unwrap();
    migrate_core::fixtures::build_win11_basic(tmp.path()).unwrap();
    let mut checked = 0;
    for rel in walk(tmp.path()) {
        let fresh = std::fs::read(tmp.path().join(&rel)).unwrap();
        let committed = std::fs::read(fixture().join(&rel)).unwrap_or_else(|_| {
            panic!(
                "missing committed fixture {}; run gen_fixtures",
                rel.display()
            )
        });
        assert_eq!(
            fresh,
            committed,
            "{} is stale; run gen_fixtures",
            rel.display()
        );
        checked += 1;
    }
    assert!(checked > 10);
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    out
}

#[test]
fn locates_install_from_root_or_windows_dir() {
    let a = WindowsInstall::from_path(&fixture()).unwrap();
    let b = WindowsInstall::from_path(&fixture().join("Windows")).unwrap();
    assert_eq!(a.windows_dir, b.windows_dir);
    assert!(WindowsInstall::from_path(&fixture().join("Users")).is_none());
    let found = WindowsInstall::scan(&[fixture().join("..")]);
    let canon = |p: &Path| p.canonicalize().unwrap();
    assert!(
        found
            .iter()
            .any(|i| canon(&i.windows_dir) == canon(&a.windows_dir))
    );
}

#[test]
fn os_info_and_skipped_profiles() {
    let src = OfflineVolume::open(&fixture()).unwrap();
    let os = src.os_info();
    assert_eq!(os.build, Some(26100));
    assert_eq!(os.computer_name.as_deref(), Some("OLD-PC"));
    assert_eq!(os.friendly_name(), "Windows 11 Pro");
    assert_eq!(
        src.profile_dirs().public.as_deref(),
        Some(r"C:\Users\Public")
    );
    let skipped: Vec<_> = src
        .skipped_profiles()
        .iter()
        .map(|s| s.sid.as_str())
        .collect();
    assert_eq!(skipped.len(), 4, "{skipped:?}");
    assert!(skipped.iter().any(|s| s.ends_with(".bak")));
}

#[test]
fn resolves_users_and_folders() {
    let src = OfflineVolume::open(&fixture()).unwrap();
    let r = inspect(&src).unwrap();
    assert_eq!(r.users.len(), 5);

    let alice = user(&r.users, "alice");
    assert_eq!(alice.profile.profile_path, r"C:\Users\Alice");
    assert_eq!(alice.profile.kind, AccountKind::LocalOrDomain);
    assert!(alice.profile.local.exists());
    assert!(
        alice.profile.warnings.is_empty(),
        "{:?}",
        alice.profile.warnings
    );
    let docs = folder(alice, "Documents");
    assert_eq!(docs.path.origin, FolderOrigin::UserShellFolders);
    assert_eq!(
        docs.local.local_path().unwrap(),
        fixture().join("Users/alice/Documents")
    );
    assert!(docs.local.exists());
    assert_eq!(folder(alice, "Music").path.origin, FolderOrigin::Default);
    assert!(!folder(alice, "Music").local.exists());

    let bob = user(&r.users, "bob");
    assert_eq!(bob.profile.kind, AccountKind::Microsoft);
    let desk = folder(bob, "Desktop");
    assert_eq!(desk.path.win_path, r"C:\Users\bob\OneDrive\Desktop");
    assert!(desk.onedrive && desk.local.exists());
    assert_eq!(
        folder(bob, "Pictures").local,
        Mapped::OtherDrive { drive: 'D' }
    );
    assert!(!folder(bob, "Music").onedrive);

    let carol = user(&r.users, "carol.CONTOSO");
    assert_eq!(carol.profile.kind, AccountKind::EntraId);
    assert_eq!(folder(carol, "Documents").local, Mapped::Network);
    let dl = folder(carol, "Downloads");
    assert_eq!(dl.path.origin, FolderOrigin::ShellFolders);
    assert!(dl.local.exists());

    let dave = user(&r.users, "dave");
    assert!(dave.profile.hive.as_ref().unwrap().state.is_dirty());
    assert!(dave.profile.warnings.iter().any(|w| w.contains("dirty")));

    let erin = user(&r.users, "erin");
    assert!(erin.profile.hive.is_none());
    assert!(
        erin.profile
            .warnings
            .iter()
            .any(|w| w.contains("NTUSER.DAT"))
    );
    assert!(folder(erin, "Desktop").local.exists());
}

#[test]
fn report_serialises_to_json() {
    let src = OfflineVolume::open(&fixture()).unwrap();
    let json = serde_json::to_value(inspect(&src).unwrap()).unwrap();
    assert_eq!(json["os"]["computer_name"], "OLD-PC");
    let f = &json["users"][0]["folders"][0];
    assert!(f["guid"].as_str().unwrap().starts_with('{'));
    assert!(f["local"]["kind"].is_string());
}

#[test]
fn not_windows_is_a_clear_error() {
    let t = tempfile::tempdir().unwrap();
    let err = OfflineVolume::open(t.path()).unwrap_err();
    assert!(err.to_string().contains("no Windows installation"));
}
