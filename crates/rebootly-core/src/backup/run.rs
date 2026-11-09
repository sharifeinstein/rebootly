//! Execute a plan: bounded parallel copy, one writer for `job.state`, then
//! `manifest.json` and `report.txt`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::SystemTime;

use super::copy::RetryPolicy;
use super::filecopy::{FileCopyOptions, copy_one};
use super::manifest::{ExcludedRecord, FileEntry, FileStatus, Manifest, SCHEMA_VERSION};
use super::plan::Plan;
use super::sensitive::{BUNDLE_FILE, Passphrase, write_bundle};
use super::state::{self, Header, Loaded, STATE_FILE, StateWriter};
use super::time::{rfc3339, stamp};
use super::walk::PlannedFile;
use crate::source::Source;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub retry: RetryPolicy,
    /// Parallel copy workers (bounded I/O).
    pub jobs: usize,
    pub cancel: Arc<AtomicBool>,
    /// Required when the plan has sensitive files (`sensitive.age`).
    pub passphrase: Option<Passphrase>,
    /// Override age's calibrated scrypt cost (tests only).
    pub bundle_work_factor: Option<u8>,
    /// Keep the `Zone.Identifier` stream ("downloaded from the internet").
    pub keep_zone_identifier: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            retry: RetryPolicy::default(),
            jobs: 4,
            cancel: Arc::new(AtomicBool::new(false)),
            passphrase: None,
            bundle_work_factor: None,
            keep_zone_identifier: false,
        }
    }
}

/// An open backup job folder `<target>/<MACHINE>_<DATE>/`.
#[derive(Debug)]
pub struct Job {
    pub dir: PathBuf,
    pub header: Header,
    pub resumed: bool,
    pub previous: Loaded,
}

fn safe_component(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "PC".into() } else { s }
}

impl Job {
    /// Create a fresh job folder under `target`.
    pub fn create(target: &Path, machine: Option<&str>) -> std::io::Result<Self> {
        let now = SystemTime::now();
        let name = format!("{}_{}", safe_component(machine.unwrap_or("PC")), stamp(now));
        let dir = target.join(name);
        std::fs::create_dir_all(target)?;
        std::fs::create_dir(&dir)?;
        let nanos = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let header = Header {
            job_id: format!("{}-{:x}{:08x}", stamp(now), std::process::id(), nanos),
            schema_version: SCHEMA_VERSION,
            started: rfc3339(now),
        };
        Ok(Job {
            dir,
            header,
            resumed: false,
            previous: Loaded::default(),
        })
    }

    /// Reopen an interrupted job folder.
    pub fn resume(dir: &Path) -> std::io::Result<Self> {
        let previous = state::load(dir).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!(
                    "{}: not a resumable job ({e})",
                    dir.join(STATE_FILE).display()
                ),
            )
        })?;
        let header = previous
            .header
            .clone()
            .ok_or_else(|| std::io::Error::other("job.state has no header"))?;
        Ok(Job {
            dir: dir.to_path_buf(),
            header,
            resumed: true,
            previous,
        })
    }

    pub fn native(&self, dest: &str) -> PathBuf {
        dest.split('/').fold(self.dir.clone(), |p, c| p.join(c))
    }

    /// Files from the plan still to copy, and the bytes they hold.
    pub fn todo<'p>(&self, plan: &'p Plan) -> (Vec<&'p PlannedFile>, u64) {
        let todo: Vec<&PlannedFile> = plan
            .files
            .iter()
            .filter(|f| {
                !self
                    .previous
                    .already_done(&self.dir, &f.dest, f.size, &f.mtime.map(rfc3339))
            })
            .collect();
        let bytes = todo.iter().map(|f| f.size).sum();
        (todo, bytes)
    }
}

#[derive(Debug)]
pub enum Event<'a> {
    Start {
        files: usize,
        bytes: u64,
        already_done: usize,
    },
    File(&'a FileEntry),
    /// Writing the encrypted bundle (after the copy).
    Sensitive {
        files: usize,
    },
}

pub fn run(
    src: &dyn Source,
    plan: &Plan,
    job: &Job,
    opts: &RunOptions,
    progress: &mut dyn FnMut(Event<'_>),
) -> std::io::Result<Manifest> {
    let pass = match (&opts.passphrase, plan.sensitive.is_empty()) {
        (Some(p), false) => Some(p),
        (None, false) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the plan has sensitive files but no passphrase was given",
            ));
        }
        (_, true) => None,
    };
    let mut writer = if job.resumed {
        StateWriter::append_to(&job.dir)?
    } else {
        StateWriter::create(&job.dir, &job.header)?
    };
    let (todo, bytes) = job.todo(plan);
    let todo_set: std::collections::HashSet<&str> = todo.iter().map(|f| f.dest.as_str()).collect();
    let mut files: Vec<FileEntry> = plan
        .files
        .iter()
        .filter(|f| !todo_set.contains(f.dest.as_str()))
        .filter_map(|f| job.previous.entries.get(&f.dest).cloned())
        .collect();
    progress(Event::Start {
        files: todo.len() + plan.generated.len(),
        bytes,
        already_done: files.len(),
    });

    // Generated files (.reg exports) are small: rewrite them on every run.
    for g in &plan.generated {
        let e = write_generated(job, g)?;
        writer.record(&e)?;
        progress(Event::File(&e));
        files.retain(|f| f.path != e.path);
        files.push(e);
    }

    let fc = FileCopyOptions {
        retry: opts.retry,
        keep_zone_identifier: opts.keep_zone_identifier,
    };
    let next = AtomicUsize::new(0);
    let cancel = &*opts.cancel;
    let (tx, rx) = mpsc::channel::<FileEntry>();
    let mut write_err = None;
    std::thread::scope(|scope| {
        for _ in 0..opts.jobs.max(1) {
            let tx = tx.clone();
            let (todo, next) = (&todo, &next);
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= todo.len() || cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let Some(e) = copy_one(src, todo[i], job, fc, cancel) else {
                        break;
                    };
                    if tx.send(e).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        for e in rx {
            if write_err.is_none() {
                if let Err(err) = writer.record(&e) {
                    // Target gone (NAS dropout): stop cleanly; resume later.
                    write_err = Some(err);
                    cancel.store(true, Ordering::Relaxed);
                }
            }
            progress(Event::File(&e));
            files.push(e);
        }
    });
    let _ = writer.sync();
    if let Some(err) = write_err {
        return Err(err);
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    let complete = !cancel.load(Ordering::Relaxed);
    // Rebuilt in full on every completed run (small; needs the passphrase).
    let sensitive = match pass.filter(|_| complete) {
        Some(p) => {
            progress(Event::Sensitive {
                files: plan.sensitive.len(),
            });
            let out = job.dir.join(BUNDLE_FILE);
            Some(write_bundle(
                src,
                &plan.sensitive,
                &out,
                p,
                opts.bundle_work_factor,
            )?)
        }
        None => None,
    };
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION"),
        job_id: job.header.job_id.clone(),
        started: job.header.started.clone(),
        finished: complete.then(|| rfc3339(SystemTime::now())),
        complete,
        source: plan.source.clone(),
        users: plan.users.clone(),
        packs: plan.packs.clone(),
        custom: plan.custom.clone(),
        offered_root_folders: plan.offered_root_folders.clone(),
        files,
        reparse_points: plan.reparse_points.clone(),
        excluded: plan
            .excluded
            .iter()
            .map(|(c, (n, b))| ExcludedRecord {
                category: *c,
                description: c.describe(),
                count: *n,
                bytes: *b,
            })
            .collect(),
        issues: plan.issues.clone(),
        encryption: None,
        sensitive,
        manual_steps: plan.manual_steps.clone(),
        warnings: plan.warnings.clone(),
    };
    manifest.write(&job.dir)?;
    super::manifest::write_atomic(
        &job.dir.join("report.txt"),
        super::report::text(&manifest).as_bytes(),
    )?;
    Ok(manifest)
}

fn write_generated(job: &Job, g: &super::extras::GeneratedFile) -> std::io::Result<FileEntry> {
    use sha2::Digest as _;
    let path = job.native(&g.dest);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    super::manifest::write_atomic(&path, &g.bytes)?;
    Ok(FileEntry {
        path: g.dest.clone(),
        source: g.source.clone(),
        size: g.bytes.len() as u64,
        mtime: None,
        attributes: None,
        sha256: Some(super::copy::hex(&sha2::Sha256::digest(&g.bytes))),
        status: FileStatus::Ok,
        reason: None,
        snapshot: false,
        streams: Vec::new(),
    })
}
