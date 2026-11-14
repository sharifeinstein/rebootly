//! End-to-end engine tests on a fresh copy of the `win11-basic` fixture.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::copy::{RetryPolicy, hash_file};
use super::manifest::FileStatus;
use super::*;
use crate::env::Environment;
use crate::install::OsInfo;
use crate::profiles::{ProfileDirs, SkippedProfile};
use crate::source::{Profile, ResolvedFolder, SourceError};
use crate::{Hive, OfflineVolume, Source};

/// Delegates to a real volume but fails reads of chosen files.
struct Flaky {
    inner: OfflineVolume,
    fail_if_contains: &'static str,
}

impl Source for Flaky {
    fn environment(&self) -> Environment {
        self.inner.environment()
    }
    fn source_root(&self) -> &Path {
        self.inner.source_root()
    }
    fn os_info(&self) -> &OsInfo {
        self.inner.os_info()
    }
    fn profile_dirs(&self) -> &ProfileDirs {
        self.inner.profile_dirs()
    }
    fn profiles(&self) -> Result<Vec<Profile>, SourceError> {
        self.inner.profiles()
    }
    fn skipped_profiles(&self) -> &[SkippedProfile] {
        self.inner.skipped_profiles()
    }
    fn resolve_folders(&self, p: &Profile) -> Result<Vec<ResolvedFolder>, SourceError> {
        self.inner.resolve_folders(p)
    }
    fn open_read(&self, path: &Path) -> std::io::Result<File> {
        if path.to_string_lossy().contains(self.fail_if_contains) {
            return Err(std::io::Error::other("CRC error (simulated bad sector)"));
        }
        self.inner.open_read(path)
    }
    fn user_vars(&self, p: &Profile) -> Result<crate::expand::VarSet, SourceError> {
        self.inner.user_vars(p)
    }
    fn map_path(&self, win: &str) -> crate::winpath::Mapped {
        self.inner.map_path(win)
    }
    fn machine_apps(&self) -> &[crate::apps::InstalledApp] {
        self.inner.machine_apps()
    }
    fn open_user_hive(&self, p: &Profile) -> Result<Option<Hive>, SourceError> {
        self.inner.open_user_hive(p)
    }
}

fn volume() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("vol");
    crate::fixtures::build_win11_basic(&root).unwrap();
    let docs = root.join("Users/alice/Documents");
    std::fs::write(docs.join("Thumbs.db"), b"thumbs").unwrap();
    std::fs::create_dir_all(docs.join("Project/sub")).unwrap();
    std::fs::write(docs.join("Project/sub/notes.md"), b"# notes").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&docs, docs.join("Project/My Documents")).unwrap();
    (t, root)
}

fn quick() -> RunOptions {
    RunOptions {
        retry: RetryPolicy {
            attempts: 2,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        },
        jobs: 3,
        ..RunOptions::default()
    }
}

#[test]
fn full_backup_layout_manifest_and_report() {
    let (t, root) = volume();
    let src = OfflineVolume::open(&root).unwrap();
    let p = plan(&src, &PlanOptions::default()).unwrap();
    assert_eq!(p.files[0].priority, plan::HIVES_PRIORITY, "hives first");
    let target = t.path().join("usb");
    let pre = preflight(&p, &t.path().join("."), None, None);
    assert!(pre.blockers.is_empty(), "{:?}", pre.blockers);

    let job = Job::create(&target, p.source.machine.as_deref()).unwrap();
    assert!(
        job.dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("OLD-PC_")
    );
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    assert!(m.complete);
    let (ok, _, failed) = m.counts();
    assert_eq!(failed, 0);
    assert_eq!(ok, p.files.len() + p.generated.len());
    assert!(job.dir.join("system/apps.json").is_file());

    let report = job.dir.join("users/Alice/folders/Documents/report.txt");
    let entry = m
        .files
        .iter()
        .find(|f| f.path == "users/Alice/folders/Documents/report.txt")
        .unwrap();
    assert_eq!(
        entry.sha256.as_deref(),
        Some(hash_file(&report).unwrap().as_str())
    );
    assert_eq!(entry.source, r"C:\Users\Alice\Documents\report.txt");
    assert!(
        job.dir
            .join("users/Alice/folders/Documents/Project/sub/notes.md")
            .exists()
    );
    assert!(
        job.dir.join("users/bob/folders/Desktop/a.txt").exists(),
        "OneDrive KFM folder copied"
    );
    assert!(job.dir.join("system/hives/dave/NTUSER.DAT.LOG1").exists());
    assert!(
        !job.dir
            .join("users/Alice/folders/Documents/Thumbs.db")
            .exists()
    );

    #[cfg(unix)]
    assert_eq!(
        m.reparse_points.len(),
        1,
        "junction loop recorded, not followed"
    );
    assert!(
        m.excluded
            .iter()
            .any(|e| e.category == exclude::Category::ThumbnailCache)
    );
    let bob = m.users.iter().find(|u| u.username == "bob").unwrap();
    assert_eq!(
        bob.folders["Pictures"].note.as_deref(),
        Some("on drive D:, not mounted")
    );
    assert!(bob.folders["Desktop"].included);
    assert!(
        m.manual_steps
            .iter()
            .any(|s| s.title.contains("Pictures of bob is on drive D:"))
    );
    assert!(
        m.manual_steps
            .iter()
            .any(|s| s.title.contains("network share"))
    );
    assert!(m.warnings.iter().any(|w| w.starts_with("dave:")));

    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(job.dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(
        json["users"][0]["folders"]["Desktop"]["guid"],
        "{B4BFCC3A-DB2C-424C-B029-7FE99A87C641}"
    );
    let txt = std::fs::read_to_string(job.dir.join("report.txt")).unwrap();
    assert!(txt.contains("0 failed") && txt.contains("[ ] 1."), "{txt}");

    // Source untouched: no backup layout written under the volume (checked
    // by a path that can't exist there, also on case-insensitive NTFS).
    assert!(!root.join("users/Alice/folders").exists());
}

#[test]
fn bad_file_is_logged_and_the_job_continues() {
    let (t, root) = volume();
    let src = Flaky {
        inner: OfflineVolume::open(&root).unwrap(),
        fail_if_contains: "report.txt",
    };
    let p = plan(&src, &PlanOptions::default()).unwrap();
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    let failed: Vec<_> = m
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Failed)
        .collect();
    assert_eq!(failed.len(), 1);
    assert!(
        failed[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("after 2 attempts")
    );
    assert!(m.complete);
    assert!(m.counts().0 > 5);
    let txt = std::fs::read_to_string(job.dir.join("report.txt")).unwrap();
    assert!(txt.contains("Failed files (1):"), "{txt}");
    assert!(txt.contains(r"Documents\report.txt  -- "), "{txt}");
    let html = std::fs::read_to_string(job.dir.join("report.html")).unwrap();
    assert!(html.contains("Failed files (1)"), "{html}");
}

#[test]
fn cancel_then_resume_copies_only_the_rest() {
    let (t, root) = volume();
    let src = OfflineVolume::open(&root).unwrap();
    let opts = PlanOptions {
        users: vec!["alice".into(), "BOB".into()],
        ..Default::default()
    };
    let p = plan(&src, &opts).unwrap();
    assert_eq!(p.users.len(), 2);
    let job = Job::create(&t.path().join("usb"), Some("OLD-PC")).unwrap();
    let mut ro = quick();
    ro.jobs = 1;
    let cancel = ro.flow.clone();
    let mut seen = 0;
    let m1 = run(&src, &p, &job, &ro, &mut |e| {
        if let Event::File(_) = e {
            seen += 1;
            if seen == 2 + p.generated.len() {
                cancel.stop();
            }
        }
    })
    .unwrap();
    assert!(!m1.complete && m1.finished.is_none());
    // Generated files (apps.json) are rewritten on every run, not resumed.
    let done_first = m1.files.len() - p.generated.len();
    assert!(done_first >= 2 && done_first < p.files.len());

    let job2 = Job::resume(&job.dir).unwrap();
    assert_eq!(job2.header.job_id, job.header.job_id);
    let mut to_copy = usize::MAX;
    let m2 = run(&src, &p, &job2, &quick(), &mut |e| {
        if let Event::Start {
            files,
            already_done,
            ..
        } = e
        {
            to_copy = files;
            assert_eq!(already_done, done_first);
        }
    })
    .unwrap();
    assert_eq!(to_copy, p.files.len() + p.generated.len() - done_first);
    assert!(m2.complete);
    assert_eq!(m2.counts().0, p.files.len() + p.generated.len());
}

#[test]
fn folder_filter_limits_plan() {
    let (_t, root) = volume();
    let src = OfflineVolume::open(&root).unwrap();
    let opts = PlanOptions {
        folders: vec!["desktop".into()],
        ..Default::default()
    };
    let p = plan(&src, &opts).unwrap();
    assert!(
        p.files
            .iter()
            .all(|f| f.dest.starts_with("system/hives/") || f.dest.contains("/folders/Desktop/"))
    );
    let alice = p.users.iter().find(|u| u.username == "Alice").unwrap();
    assert_eq!(
        alice.folders["Documents"].note.as_deref(),
        Some("not part of this backup")
    );
}

mod live_like;
mod packs;
mod packs_apps;
mod packs_media;
mod packs_more;
mod packs_secrets;
mod packs_work;
mod sensitive;
mod snapshot;
mod system;
