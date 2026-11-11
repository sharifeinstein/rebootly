//! Backup targets on Linux (SPEC §8): the only place Rebootly mounts
//! anything read-write.
//!
//! - [`find_stick`]: the partition holding `.rebootly-root` (the Rebootly
//!   USB stick's data partition, e.g. Ventoy's exFAT one).
//! - [`TargetMounter::mount`]: mount a volume read-write under `/media`.
//!   Every candidate is first probed read-only; a volume with Windows on it
//!   is refused (a rescue system never writes to a Windows disk), and so is
//!   a locked one.
//!
//! Mounts are listed in `<base>/targets` so `rebootly-cli close` can flush
//! and unmount them later (also after a killed run).

use std::path::{Path, PathBuf};

use rebootly_core::install::WindowsInstall;
use rebootly_core::volume::{Volume, VolumeState};

use crate::cmd::{Runner, last_line};
use crate::mount::MountError;

/// Marker file at the root of a Rebootly stick.
pub const MARKER: &str = ".rebootly-root";
/// Where targets are mounted (desktop systems use it too, so the drive
/// list finds them).
pub const MEDIA: &str = "/media";

pub struct TargetMounter<'a> {
    run: &'a dyn Runner,
    /// Rebootly's own folder for probes and the list of targets.
    base: PathBuf,
    media: PathBuf,
}

fn short(device: &str) -> String {
    Path::new(device)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| device.replace('/', "_"))
}

/// A folder name from a label: letters, digits, `-`, `_` and `.`; spaces
/// become `_`.
fn folder_name(v: &Volume) -> String {
    let l: String = v
        .label
        .as_deref()
        .unwrap_or("")
        .chars()
        .map(|c| if c == ' ' { '_' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let l = l.trim_matches('.').to_string();
    if l.is_empty() { short(&v.device) } else { l }
}

/// The commands to try for `fs`, in order.
fn tries<'s>(
    fs: &str,
    opts: &'s str,
    dev: &'s str,
    dir: &'s str,
) -> Vec<(&'static str, Vec<&'s str>)> {
    match fs.to_ascii_lowercase().as_str() {
        "exfat" => vec![
            ("mount", vec!["-t", "exfat", "-o", opts, dev, dir]),
            ("mount.exfat-fuse", vec!["-o", opts, dev, dir]),
        ],
        "ntfs" => vec![
            ("mount", vec!["-t", "ntfs3", "-o", opts, dev, dir]),
            ("mount", vec!["-t", "ntfs-3g", "-o", opts, dev, dir]),
            ("ntfs-3g", vec!["-o", opts, dev, dir]),
        ],
        _ => Vec::new(),
    }
}

impl<'a> TargetMounter<'a> {
    pub fn new(run: &'a dyn Runner, base: impl Into<PathBuf>, media: impl Into<PathBuf>) -> Self {
        TargetMounter {
            run,
            base: base.into(),
            media: media.into(),
        }
    }

    /// The real system: probes under [`crate::BASE`], targets under [`MEDIA`].
    pub fn system(run: &'a dyn Runner) -> Self {
        Self::new(run, crate::BASE, MEDIA)
    }

    fn fs_mount(&self, v: &Volume, dir: &Path, rw: bool) -> Result<(), MountError> {
        let fs = v.fs.as_deref().unwrap_or("");
        let opts = if rw { "rw,noatime" } else { "ro" };
        let d = dir.to_string_lossy();
        let list = tries(fs, opts, &v.device, &d);
        if list.is_empty() {
            return Err(MountError::Failed {
                step: "mounting",
                detail: format!("{} isn't exFAT or NTFS", v.device),
            });
        }
        let mut last = String::new();
        for (prog, args) in &list {
            if !self.run.has(prog) {
                continue;
            }
            match self.run.run(prog, args, None) {
                Ok(o) if o.ok() => return Ok(()),
                Ok(o) => last = last_line(&o.stderr),
                Err(e) => last = e.to_string(),
            }
        }
        if last.to_ascii_lowercase().contains("busy") {
            last.push_str(
                " (booted from Ventoy? Its data partition can only be mounted with \
                 VTOY_LINUX_REMOUNT in ventoy/ventoy.json, which make-stick writes)",
            );
        }
        Err(MountError::Failed {
            step: if rw {
                "mounting for writing"
            } else {
                "mounting read-only"
            },
            detail: if last.is_empty() {
                format!("no {fs} driver found")
            } else {
                last
            },
        })
    }

    fn umount(&self, dir: &Path) {
        let _ = self.run.run("umount", &[&dir.to_string_lossy()], None);
        let _ = std::fs::remove_dir(dir);
    }

    /// Mount `v` read-only for a moment and look at it: `f(root)`.
    fn probe<T>(&self, v: &Volume, f: impl FnOnce(&Path) -> T) -> Result<T, MountError> {
        if let VolumeState::Mounted { root, .. } = &v.state {
            return Ok(f(root));
        }
        let dir = self.base.join(format!("probe-{}", short(&v.device)));
        std::fs::create_dir_all(&dir)?;
        if let Err(e) = self.fs_mount(v, &dir, false) {
            let _ = std::fs::remove_dir(&dir);
            return Err(e);
        }
        let out = f(&dir);
        self.umount(&dir);
        Ok(out)
    }

    /// Mount `v` read-write under the media folder; returns where.
    pub fn mount(&self, v: &Volume) -> Result<PathBuf, MountError> {
        let refuse = |why: String| MountError::Failed {
            step: "choosing a target",
            detail: why,
        };
        match &v.state {
            VolumeState::Locked => {
                return Err(refuse(format!(
                    "{} is locked with BitLocker; backups go to an unencrypted drive",
                    v.device
                )));
            }
            VolumeState::Mounted {
                root,
                read_only: false,
            } => return Ok(root.clone()),
            VolumeState::Mounted { root, .. } => {
                return Err(refuse(format!(
                    "{} is mounted read-only at {}; unmount it first (rebootly-cli close)",
                    v.device,
                    root.display()
                )));
            }
            VolumeState::Unmounted => {}
        }
        if self.probe(v, |r| WindowsInstall::from_path(r).is_some())? {
            return Err(refuse(format!(
                "{} has Windows on it; Rebootly never writes to a Windows disk from a rescue system",
                v.device
            )));
        }
        let name = folder_name(v);
        let mut dir = self.media.join(&name);
        let mut n = 2;
        while dir.exists()
            && std::fs::read_dir(&dir)
                .map(|mut r| r.next().is_some())
                .unwrap_or(true)
        {
            dir = self.media.join(format!("{name}-{n}"));
            n += 1;
        }
        std::fs::create_dir_all(&dir)?;
        if let Err(e) = self.fs_mount(v, &dir, true) {
            let _ = std::fs::remove_dir(&dir);
            return Err(e);
        }
        self.remember(&dir)?;
        Ok(dir)
    }

    fn list_file(&self) -> PathBuf {
        self.base.join("targets")
    }

    fn remember(&self, dir: &Path) -> std::io::Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(&self.base)?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.list_file())?;
        writeln!(f, "{}", dir.display())
    }

    /// Flush and unmount every target mounted by Rebootly. Returns errors.
    pub fn close_all(&self) -> Vec<String> {
        let Ok(list) = std::fs::read_to_string(self.list_file()) else {
            return Vec::new();
        };
        let _ = self.run.run("sync", &[], None);
        let mut errs = Vec::new();
        for d in list.lines().filter(|l| !l.is_empty()).rev() {
            match self.run.run("umount", &[d], None) {
                Ok(o) if o.ok() => {
                    let _ = std::fs::remove_dir(d);
                }
                Ok(o) => errs.push(format!("umount {d}: {}", last_line(&o.stderr))),
                Err(e) => errs.push(format!("umount {d}: {e}")),
            }
        }
        if errs.is_empty() {
            let _ = std::fs::remove_file(self.list_file());
        }
        errs
    }

    /// The volume holding [`MARKER`] at its root, if any (mounted ones are
    /// looked at in place, the others probed read-only).
    pub fn find_stick<'v>(&self, vols: &'v [Volume]) -> Option<&'v Volume> {
        vols.iter()
            .filter(|v| !v.is_locked())
            .find(|v| self.probe(v, |r| r.join(MARKER).is_file()).unwrap_or(false))
    }
}

/// Volumes that could be mounted as a target: exFAT or NTFS, not locked,
/// not already mounted read-write (those are in the drive list anyway).
pub fn candidates(vols: &[Volume]) -> Vec<Volume> {
    vols.iter()
        .filter(|v| {
            v.fs.as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case("exfat") || f.eq_ignore_ascii_case("ntfs"))
                && matches!(
                    v.state,
                    VolumeState::Unmounted
                        | VolumeState::Mounted {
                            read_only: true,
                            ..
                        }
                )
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
