//! BitLocker unlock and read-only mounts (SPEC §7.1, §7.2).
//!
//! - Unlock: `cryptsetup open --type bitlk --readonly` (≥ 2.3) with the
//!   recovery key on stdin; fallback `dislocker -r` (the key goes in its
//!   arguments: dislocker has no other non-interactive way).
//! - Mount: `mount -t ntfs3 -o ro`, then `ntfs-3g -o ro`. Never read-write.
//!
//! Everything done is recorded and undone in reverse order by
//! [`Mounter::release`] (also on drop), unless [`Mounter::keep`] was called
//! (the CLI's `mount` command leaves the volumes mounted for later commands).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rebootly_core::volume::{RecoveryKey, Volume, VolumeState};

use crate::cmd::{Runner, last_line};

/// Where Rebootly mounts volumes (a tmpfs, never on a source disk).
pub const BASE: &str = "/run/rebootly";

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("the recovery key doesn't unlock this disk; check it and try again")]
    WrongKey,
    #[error(
        "unlocking BitLocker needs cryptsetup (2.3 or newer) or dislocker, and neither is installed"
    )]
    NoUnlockTool,
    #[error("{step} failed: {detail}")]
    Failed { step: &'static str, detail: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Undo {
    Umount(PathBuf),
    Close(String),
    Rmdir(PathBuf),
}

pub struct Mounter {
    run: Box<dyn Runner>,
    base: PathBuf,
    done: Mutex<Vec<Undo>>,
    keep: bool,
}

/// A successful mount, and what to tell the user about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mounted {
    pub root: PathBuf,
    /// e.g. "Windows wasn't shut down cleanly…" from the driver.
    pub notes: Vec<String>,
}

fn short(device: &str) -> String {
    Path::new(device)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| device.replace('/', "_"))
}

impl Mounter {
    pub fn new(run: Box<dyn Runner>, base: impl Into<PathBuf>) -> Self {
        Mounter {
            run,
            base: base.into(),
            done: Mutex::new(Vec::new()),
            keep: false,
        }
    }

    /// The real system, mounting under [`BASE`].
    pub fn system() -> Self {
        Self::new(Box::new(crate::cmd::System), BASE)
    }

    /// Leave everything mounted when this is dropped.
    pub fn keep(&mut self) {
        self.keep = true;
    }

    pub fn runner(&self) -> &dyn Runner {
        self.run.as_ref()
    }

    fn record(&self, u: Undo) {
        self.done.lock().expect("mount log").push(u);
    }

    fn dir(&self, name: &str) -> Result<PathBuf, MountError> {
        let d = self.base.join(name);
        if !d.is_dir() {
            std::fs::create_dir_all(&d)?;
            self.record(Undo::Rmdir(d.clone()));
        }
        Ok(d)
    }

    /// Unlock a BitLocker partition read-only; returns the device to mount.
    pub fn unlock(&self, device: &str, key: &RecoveryKey) -> Result<String, MountError> {
        let name = format!("rebootly-{}", short(device));
        if self.run.has("cryptsetup") {
            let mut stdin = key.expose().as_bytes().to_vec();
            stdin.push(b'\n');
            let out = self.run.run(
                "cryptsetup",
                &["open", "--type", "bitlk", "--readonly", device, &name],
                Some(&stdin),
            )?;
            if out.ok() {
                self.record(Undo::Close(name.clone()));
                return Ok(format!("/dev/mapper/{name}"));
            }
            // Exit code 2: no key slot accepted the passphrase.
            if out.code == Some(2) {
                return Err(MountError::WrongKey);
            }
            if !self.run.has("dislocker") {
                return Err(MountError::Failed {
                    step: "cryptsetup",
                    detail: scrub(&last_line(&out.stderr), key),
                });
            }
            // Old cryptsetup or an unsupported BitLocker variant: try dislocker.
        } else if !self.run.has("dislocker") {
            return Err(MountError::NoUnlockTool);
        }
        let fve = self.dir(&format!("fve-{}", short(device)))?;
        let fve_s = fve.to_string_lossy().into_owned();
        let p = format!("-p{}", key.expose());
        let out = self
            .run
            .run("dislocker", &["-r", "-V", device, &p, "--", &fve_s], None)?;
        if !out.ok() {
            let detail = scrub(&last_line(&out.stderr), key);
            if detail.to_ascii_lowercase().contains("password")
                || detail.to_ascii_lowercase().contains("vmk")
            {
                return Err(MountError::WrongKey);
            }
            return Err(MountError::Failed {
                step: "dislocker",
                detail,
            });
        }
        self.record(Undo::Umount(fve.clone()));
        Ok(fve.join("dislocker-file").to_string_lossy().into_owned())
    }

    /// Mount `dev` read-only at `<base>/<name>`.
    pub fn mount(&self, dev: &str, name: &str) -> Result<Mounted, MountError> {
        let dir = self.dir(&short(name))?;
        let d = dir.to_string_lossy().into_owned();
        let file = !dev.starts_with("/dev/");
        let opts = if file { "ro,loop" } else { "ro" };
        let tries: [(&str, Vec<&str>); 3] = [
            ("mount", vec!["-t", "ntfs3", "-o", opts, dev, &d]),
            ("mount", vec!["-t", "ntfs-3g", "-o", opts, dev, &d]),
            ("ntfs-3g", vec!["-o", opts, dev, &d]),
        ];
        let mut last = String::new();
        for (prog, args) in &tries {
            if !self.run.has(prog) {
                continue;
            }
            match self.run.run(prog, args, None) {
                Ok(out) if out.ok() => {
                    self.record(Undo::Umount(dir.clone()));
                    return Ok(Mounted {
                        root: dir,
                        notes: notes(&format!("{}\n{}", out.stdout, out.stderr)),
                    });
                }
                Ok(out) => last = last_line(&out.stderr),
                Err(e) => last = e.to_string(),
            }
        }
        Err(MountError::Failed {
            step: "mounting read-only",
            detail: if last.is_empty() {
                "no NTFS driver (ntfs3 or ntfs-3g) found".into()
            } else {
                last
            },
        })
    }

    /// Unlock (if needed) and mount `v`; already mounted volumes are left.
    pub fn open(&self, v: &Volume, key: Option<&RecoveryKey>) -> Result<Mounted, MountError> {
        match (&v.state, key) {
            (VolumeState::Mounted { root, .. }, _) => Ok(Mounted {
                root: root.clone(),
                notes: Vec::new(),
            }),
            (VolumeState::Locked, Some(k)) => {
                let dev = self.unlock(&v.device, k)?;
                self.mount(&dev, &v.device)
            }
            (VolumeState::Locked, None) => Err(MountError::Failed {
                step: "opening",
                detail: "the disk is locked with BitLocker; a recovery key is needed".into(),
            }),
            (VolumeState::Unmounted, _) => self.mount(&v.device, &v.device),
        }
    }

    /// Undo every mount and unlock, newest first. Errors are returned as
    /// text (nothing more can be done about them).
    pub fn release(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let steps: Vec<Undo> = std::mem::take(&mut *self.done.lock().expect("mount log"));
        for u in steps.into_iter().rev() {
            let r = match &u {
                Undo::Umount(d) => self
                    .run
                    .run("umount", &[&d.to_string_lossy()], None)
                    .map(|o| (o.ok(), o.stderr)),
                Undo::Close(n) => self
                    .run
                    .run("cryptsetup", &["close", n], None)
                    .map(|o| (o.ok(), o.stderr)),
                Undo::Rmdir(d) => std::fs::remove_dir(d).map(|_| (true, String::new())),
            };
            match r {
                Ok((true, _)) => {}
                Ok((false, e)) => errs.push(format!("{u:?}: {}", last_line(&e))),
                Err(e) => errs.push(format!("{u:?}: {e}")),
            }
        }
        errs
    }
}

impl Drop for Mounter {
    fn drop(&mut self) {
        if !self.keep {
            self.release();
        }
    }
}

/// Undo what an earlier run left open (`rebootly-cli open`, or a run that
/// was killed): unmount everything under [`BASE`], deepest first, then close
/// `rebootly-*` mappings and remove the empty folders.
pub fn close_all(run: &dyn Runner) -> Vec<String> {
    let info = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let mapper: Vec<String> = std::fs::read_dir("/dev/mapper")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    close_all_in(run, Path::new(BASE), &info, &mapper)
}

pub fn close_all_in(run: &dyn Runner, base: &Path, mountinfo: &str, mapper: &[String]) -> Vec<String> {
    let mut errs = Vec::new();
    let mut mounts: Vec<String> = mountinfo
        .lines()
        .filter_map(|l| l.split(' ').nth(4))
        .map(crate::discover::unescape)
        .filter(|m| Path::new(m).starts_with(base))
        .collect();
    mounts.sort_by_key(|m| std::cmp::Reverse(m.len()));
    mounts.dedup();
    for m in &mounts {
        match run.run("umount", &[m], None) {
            Ok(o) if o.ok() => {}
            Ok(o) => errs.push(format!("umount {m}: {}", last_line(&o.stderr))),
            Err(e) => errs.push(format!("umount {m}: {e}")),
        }
    }
    for n in mapper.iter().filter(|n| n.starts_with("rebootly-")) {
        match run.run("cryptsetup", &["close", n], None) {
            Ok(o) if o.ok() => {}
            Ok(o) => errs.push(format!("cryptsetup close {n}: {}", last_line(&o.stderr))),
            Err(e) => errs.push(format!("cryptsetup close {n}: {e}")),
        }
    }
    if let Ok(rd) = std::fs::read_dir(base) {
        for e in rd.flatten() {
            let _ = std::fs::remove_dir(e.path()); // only empty ones go
        }
    }
    errs
}

/// Never let the key reach a message, even if a tool echoes it.
fn scrub(s: &str, key: &RecoveryKey) -> String {
    let k = key.expose();
    let plain: String = k.chars().filter(|c| *c != '-').collect();
    s.replace(k, "…").replace(&plain, "…")
}

/// Plain-language notes from the NTFS drivers' messages.
pub fn notes(output: &str) -> Vec<String> {
    let o = output.to_ascii_lowercase();
    let mut n = Vec::new();
    if o.contains("hibernat") || o.contains("fast restart") {
        n.push(rebootly_core::volume::HIBERNATED_NOTE.to_string());
    } else if o.contains("unclean") || o.contains("dirty") {
        n.push(
            "Windows on this disk wasn't shut down cleanly; it is opened read-only, so nothing \
             changes, but a few files from the last session may be incomplete."
                .into(),
        );
    }
    n
}

#[cfg(test)]
#[path = "mount_tests.rs"]
mod tests;
