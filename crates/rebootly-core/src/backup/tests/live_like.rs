//! A live-style source with the hooks only `LiveWindows` has: a locked file
//! read from a "snapshot", alternate data streams, and a command capture.

use super::*;
use crate::source::StreamInfo;
use crate::{Capture, Captures};

struct FakeLive {
    inner: OfflineVolume,
    /// Stream contents per (file name, stream name).
    streams_dir: PathBuf,
    capture: PathBuf,
    snapshots_opened: std::sync::atomic::AtomicUsize,
}

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

impl Source for FakeLive {
    fn environment(&self) -> Environment {
        Environment::LiveWindows
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
        if name(path) == "mail.pst" {
            return Err(std::io::ErrorKind::ResourceBusy.into());
        }
        self.inner.open_read(path)
    }
    fn open_snapshot(&self, path: &Path) -> std::io::Result<File> {
        self.snapshots_opened.fetch_add(1, Ordering::SeqCst);
        self.inner.open_read(path)
    }
    fn alt_streams(&self, path: &Path, _snapshot: bool) -> std::io::Result<Vec<StreamInfo>> {
        let dir = self.streams_dir.join(name(path));
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut v: Vec<_> = rd
            .flatten()
            .map(|e| StreamInfo {
                name: e.file_name().to_string_lossy().into_owned(),
                size: e.metadata().unwrap().len(),
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(v)
    }
    fn open_stream(&self, path: &Path, stream: &str, _snapshot: bool) -> std::io::Result<File> {
        File::open(self.streams_dir.join(name(path)).join(stream))
    }
    fn system_captures(&self, sensitive: bool) -> Captures {
        let mut c = Captures {
            warnings: vec!["printer list not available (test)".into()],
            ..Captures::default()
        };
        c.files.push(Capture {
            dest: "system/winget.json".into(),
            file: self.capture.clone(),
            origin: "winget export".into(),
            sensitive: false,
        });
        if sensitive {
            c.files.push(Capture {
                dest: "system/certs/ab12.pfx".into(),
                file: self.capture.clone(),
                origin: "Export-PfxCertificate".into(),
                sensitive: true,
            });
        }
        c
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

fn fake(t: &tempfile::TempDir, root: &Path) -> FakeLive {
    let docs = root.join("Users/alice/Documents");
    std::fs::write(docs.join("mail.pst"), b"locked mailbox").unwrap();
    std::fs::write(docs.join("setup.exe"), b"MZ").unwrap();
    let sd = t.path().join("streams/setup.exe");
    std::fs::create_dir_all(&sd).unwrap();
    std::fs::write(sd.join("Zone.Identifier"), b"[ZoneTransfer]\r\nZoneId=3").unwrap();
    std::fs::write(sd.join("com.vendor.tag"), b"tag").unwrap();
    let capture = t.path().join("winget.json");
    std::fs::write(&capture, b"{}").unwrap();
    FakeLive {
        inner: OfflineVolume::open(root).unwrap(),
        streams_dir: t.path().join("streams"),
        capture,
        snapshots_opened: Default::default(),
    }
}

#[test]
fn locked_files_streams_and_captures() {
    let (t, root) = volume();
    let src = fake(&t, &root);
    let p = plan(&src, &PlanOptions::default()).unwrap();
    assert!(
        p.warnings
            .iter()
            .any(|w| w == "system: printer list not available (test)")
    );
    assert!(p.files.iter().any(|f| f.dest == "system/winget.json"));
    assert!(
        p.sensitive.is_empty(),
        "no sensitive capture without --sensitive"
    );

    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    let get = |d: &str| m.files.iter().find(|f| f.path == d).unwrap();

    let pst = get("users/Alice/folders/Documents/mail.pst");
    assert_eq!(pst.status, FileStatus::Ok);
    assert!(pst.snapshot);
    assert_eq!(src.snapshots_opened.load(Ordering::SeqCst), 1);
    assert!(!get("users/Alice/folders/Documents/report.txt").snapshot);

    let exe = get("users/Alice/folders/Documents/setup.exe");
    let names: Vec<_> = exe.streams.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["com.vendor.tag"], "Zone.Identifier dropped");
    let stored = exe.streams[0].path.as_deref().unwrap();
    assert_eq!(
        stored,
        "streams/users/Alice/folders/Documents/setup.exe/com.vendor.tag"
    );
    assert_eq!(std::fs::read(job.native(stored)).unwrap(), b"tag");
    assert_eq!(get("system/winget.json").source, "winget export");
    let report = std::fs::read_to_string(job.dir.join("report.txt")).unwrap();
    assert!(
        report.contains("1 file(s) were open in another program"),
        "{report}"
    );
    assert!(
        report.contains("1 alternate data stream(s) kept"),
        "{report}"
    );

    // Kept on request; and the manifest/state round-trip keeps the fields.
    let job2 = Job::create(&t.path().join("usb2"), None).unwrap();
    let opts = RunOptions {
        keep_zone_identifier: true,
        ..quick()
    };
    let m2 = run(&src, &p, &job2, &opts, &mut |_| {}).unwrap();
    let exe2 = m2
        .files
        .iter()
        .find(|f| f.path.ends_with("setup.exe"))
        .unwrap();
    assert_eq!(exe2.streams.len(), 2);
    let resumed = Job::resume(&job2.dir).unwrap();
    assert_eq!(resumed.previous.entries[&exe2.path], *exe2);
}

#[test]
fn sensitive_captures_go_to_the_bundle_only() {
    let (t, root) = volume();
    let src = fake(&t, &root);
    let opts = PlanOptions {
        sensitive_bundle: true,
        ..PlanOptions::default()
    };
    let p = plan(&src, &opts).unwrap();
    assert!(
        p.sensitive
            .iter()
            .any(|f| f.dest == "system/certs/ab12.pfx")
    );
    assert!(!p.files.iter().any(|f| f.dest.starts_with("system/certs")));
}

#[test]
fn hydrate_is_live_only() {
    use crate::backup::exclude::{CloudFiles, ExcludeOptions};
    let (t, root) = volume();
    let opts = PlanOptions {
        exclude: ExcludeOptions {
            cloud_files: CloudFiles::Hydrate,
            ..ExcludeOptions::default()
        },
        ..PlanOptions::default()
    };
    let offline = plan(&OfflineVolume::open(&root).unwrap(), &opts).unwrap();
    assert!(
        offline
            .warnings
            .iter()
            .any(|w| w.contains("only in live mode"))
    );
    let live = plan(&fake(&t, &root), &opts).unwrap();
    assert!(
        !live
            .warnings
            .iter()
            .any(|w| w.contains("only in live mode"))
    );
}
