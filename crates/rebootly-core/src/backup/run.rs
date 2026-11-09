//! Execute a plan: bounded parallel copy, one writer for `job.state`, then
//! `manifest.json` and `report.txt`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::SystemTime;

use super::copy::{RetryPolicy, copy_file};
use super::manifest::{ExcludedRecord, FileEntry, FileStatus, Manifest, SCHEMA_VERSION};
use super::plan::Plan;
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
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            retry: RetryPolicy::default(),
            jobs: 4,
            cancel: Arc::new(AtomicBool::new(false)),
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
}

fn entry(
    f: &PlannedFile,
    status: FileStatus,
    sha256: Option<String>,
    reason: Option<String>,
) -> FileEntry {
    FileEntry {
        path: f.dest.clone(),
        source: f.src_win.clone(),
        size: f.size,
        mtime: f.mtime.map(rfc3339),
        attributes: f.attributes,
        sha256,
        status,
        reason,
    }
}

pub fn run(
    src: &dyn Source,
    plan: &Plan,
    job: &Job,
    opts: &RunOptions,
    progress: &mut dyn FnMut(Event<'_>),
) -> std::io::Result<Manifest> {
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
                    let f = todo[i];
                    let open = || src.open_read(&f.src);
                    let e =
                        match copy_file(&open, &job.native(&f.dest), f.mtime, opts.retry, cancel) {
                            Ok(c) => entry(f, FileStatus::Ok, Some(c.sha256), None),
                            Err(fail) if fail.reason == "cancelled" => break,
                            Err(fail) => entry(
                                f,
                                FileStatus::Failed,
                                None,
                                Some(format!(
                                    "{} (after {} attempts)",
                                    fail.reason, fail.attempts
                                )),
                            ),
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
    })
}
