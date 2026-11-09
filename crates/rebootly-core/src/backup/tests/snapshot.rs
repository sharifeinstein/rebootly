//! A live-style access strategy that "snapshots" loaded hives: the plan must
//! copy the snapshot in place of the hive file and skip its logs.

use std::fs::File;
use std::path::{Path, PathBuf};

use super::volume;
use crate::backup::{PlanOptions, plan};
use crate::env::Environment;
use crate::hive::HiveError;
use crate::source::{HiveRef, LoadedHive, OfflineAccess, VolumeAccess, clean_state};
use crate::winpath::Mapped;
use crate::{OfflineVolume, Source};

/// Pretends Alice is signed in: her hives come from snapshot files.
#[derive(Debug)]
struct FakeLive {
    snaps: PathBuf,
}

impl FakeLive {
    fn snap_for(&self, file: &Path, which: HiveRef<'_>) -> Option<PathBuf> {
        let alice = file.to_string_lossy().contains("alice");
        let name = match which {
            HiveRef::User(_) if alice => "alice-ntuser.snap",
            HiveRef::UserClasses(_) if alice => "alice-classes.snap",
            _ => return None,
        };
        let p = self.snaps.join(name);
        if !p.exists() {
            std::fs::copy(file, &p).unwrap();
        }
        Some(p)
    }
}

impl VolumeAccess for FakeLive {
    fn environment(&self) -> Environment {
        Environment::LiveWindows
    }
    fn load_hive(&self, file: &Path, which: HiveRef<'_>) -> Result<LoadedHive, HiveError> {
        match self.snap_for(file, which) {
            Some(snap) => Ok(LoadedHive {
                hive: crate::Hive::open(&snap)?,
                state: clean_state(),
                snapshot: Some(snap),
            }),
            None => OfflineAccess.load_hive(file, which),
        }
    }
    fn hive_snapshot(&self, file: &Path, which: HiveRef<'_>) -> Result<Option<PathBuf>, HiveError> {
        Ok(self.snap_for(file, which))
    }
    fn open_read(&self, path: &Path) -> std::io::Result<File> {
        File::open(path)
    }
    fn map(&self, win: &str, drive: char, root: &Path) -> Mapped {
        OfflineAccess.map(win, drive, root)
    }
}

#[test]
fn signed_in_user_hives_come_from_snapshots_without_logs() {
    let (t, root) = volume();
    let classes = root.join("Users/alice/AppData/Local/Microsoft/Windows");
    std::fs::create_dir_all(&classes).unwrap();
    std::fs::copy(
        root.join("Users/bob/NTUSER.DAT"),
        classes.join("UsrClass.dat"),
    )
    .unwrap();
    std::fs::write(classes.join("UsrClass.dat.LOG1"), b"log").unwrap();
    let snaps = t.path().join("snaps");
    std::fs::create_dir_all(&snaps).unwrap();

    let src = OfflineVolume::open_with(
        &root,
        Box::new(FakeLive {
            snaps: snaps.clone(),
        }),
    )
    .unwrap();
    assert_eq!(src.environment(), Environment::LiveWindows);
    let alice = src
        .profiles()
        .unwrap()
        .into_iter()
        .find(|p| p.username.eq_ignore_ascii_case("alice"))
        .unwrap();
    let hive = alice.hive.as_ref().unwrap();
    assert!(!hive.state.is_dirty(), "snapshot is clean");
    assert_eq!(
        hive.snapshot.as_deref(),
        Some(snaps.join("alice-ntuser.snap").as_path())
    );

    let p = plan(&src, &PlanOptions::default()).unwrap();
    let hives: Vec<_> = p
        .files
        .iter()
        .filter(|f| f.dest.starts_with("system/hives/Alice/"))
        .collect();
    let dests: Vec<&str> = hives.iter().map(|f| f.dest.as_str()).collect();
    assert_eq!(
        dests,
        [
            "system/hives/Alice/NTUSER.DAT",
            "system/hives/Alice/UsrClass.dat"
        ]
    );
    assert_eq!(hives[0].src, snaps.join("alice-ntuser.snap"));
    assert_eq!(hives[1].src, snaps.join("alice-classes.snap"));
    assert!(
        hives[1]
            .src_win
            .ends_with(r"\AppData\Local\Microsoft\Windows\UsrClass.dat")
    );
    // Dave isn't "signed in": his hive file and log are copied as they are.
    assert!(p.files.iter().any(|f| {
        f.dest
            .eq_ignore_ascii_case("system/hives/Dave/NTUSER.DAT.LOG1")
    }));
}
