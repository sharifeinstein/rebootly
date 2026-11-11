//! The real machine: drive letters or mount folders, the running Windows,
//! packs, and the restore target (current user, or a simulated one).

use std::any::Any;
use std::path::{Path, PathBuf};

use rebootly_core::Source;
use rebootly_core::backup::preflight::gib;
use rebootly_core::env::Environment;
use rebootly_core::packs::schema::CompiledPack;
use rebootly_core::restore::{RestoreHost, SimHost, Target};
use rebootly_core::volume::{Unlock, Volume};

use crate::{Drive, Platform};

/// Settings of the real machine layer (the CLI's `tui` flags).
#[derive(Debug, Clone, Default)]
pub struct MachineOptions {
    /// Folder with extra or updated pack YAML files.
    pub packs_dir: Option<PathBuf>,
    /// Restore into DIR/C/… instead of this PC (previews off Windows); also
    /// shows the start menu outside Windows.
    pub simulate_root: Option<PathBuf>,
    /// Extra folders offered as targets and searched for backups.
    pub drives: Vec<PathBuf>,
    /// Text of `--version`, e.g. `0.1.0 (commit abc1234)`.
    pub version: String,
}

/// The machine Rebootly runs on.
pub struct Machine {
    pub args: MachineOptions,
    /// Unlocks and read-only mounts made in this run (undone on drop).
    pub vols: crate::volumes::Volumes,
}

impl Machine {
    pub fn new(args: MachineOptions) -> Self {
        Machine {
            args,
            vols: crate::volumes::Volumes::new(),
        }
    }
}

pub fn note(p: &Path) -> String {
    let free = fs4::available_space(p).ok().map(gib);
    let fs = fs_type(p);
    match (free, fs) {
        (Some(f), Some(t)) => format!("{f} free, {t}"),
        (Some(f), None) => format!("{f} free"),
        (None, _) => String::new(),
    }
}

fn fs_type(p: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        rebootly_win::filesystem_type(p)
    }
    #[cfg(not(windows))]
    {
        rebootly_core::backup::preflight::filesystem_type(p)
    }
}

/// Drive letters that exist (Windows), mount folders (Linux).
pub fn mounted() -> Vec<PathBuf> {
    if cfg!(windows) {
        (b'A'..=b'Z')
            .map(|c| PathBuf::from(format!("{}:\\", c as char)))
            .filter(|p| p.is_dir())
            .collect()
    } else {
        let mut out = Vec::new();
        for base in ["/media", "/run/media", "/mnt"] {
            let Ok(rd) = std::fs::read_dir(base) else {
                continue;
            };
            for e in rd.flatten().filter(|e| e.path().is_dir()) {
                let p = e.path();
                // /media/<user>/<label> as well as /mnt/<label>.
                let kids: Vec<PathBuf> = std::fs::read_dir(&p)
                    .map(|r| {
                        r.flatten()
                            .map(|e| e.path())
                            .filter(|p| p.is_dir())
                            .collect()
                    })
                    .unwrap_or_default();
                if base != "/mnt" && !kids.is_empty() {
                    out.extend(kids);
                } else {
                    out.push(p);
                }
            }
        }
        out
    }
}

impl Platform for Machine {
    fn environment(&self) -> Environment {
        if self.args.simulate_root.is_some() {
            return Environment::LiveWindows;
        }
        Environment::detect()
    }

    fn version(&self) -> String {
        self.args.version.clone()
    }

    fn open_live(&self) -> anyhow::Result<Box<dyn Source>> {
        crate::source::live()
    }

    fn install_roots(&self) -> Vec<PathBuf> {
        let mut roots = mounted();
        for v in self.volumes() {
            if let Some(r) = v.root() {
                if !roots.iter().any(|x| x == r) {
                    roots.push(r.to_path_buf());
                }
            }
        }
        roots
    }

    fn open_offline(&self, root: &Path) -> anyhow::Result<Box<dyn Source>> {
        crate::volumes::open_offline(root)
    }

    fn volumes(&self) -> Vec<Volume> {
        crate::volumes::list()
    }

    fn open_volume(
        &self,
        device: &str,
        key: Option<&Unlock>,
    ) -> anyhow::Result<(PathBuf, Vec<String>)> {
        self.vols.open(device, key)
    }

    fn prepare_rescue(&self) -> Vec<String> {
        self.vols.mount_all()
    }

    fn drives(&self) -> Vec<Drive> {
        let system = std::env::var("SystemDrive")
            .ok()
            .map(|d| d.to_ascii_uppercase());
        let live = Environment::detect() == Environment::LiveWindows;
        let mut out: Vec<Drive> = mounted()
            .into_iter()
            .filter(|p| {
                // Never offer the running system's own drive.
                !(live
                    && system
                        .as_deref()
                        .is_some_and(|s| p.to_string_lossy().to_ascii_uppercase().starts_with(s)))
            })
            .map(|p| Drive {
                note: note(&p),
                path: p,
            })
            .collect();
        for d in &self.args.drives {
            out.push(Drive {
                note: note(d),
                path: d.clone(),
            });
        }
        out
    }

    fn target_fs(&self, path: &Path) -> Option<String> {
        if cfg!(windows) { fs_type(path) } else { None }
    }

    fn keep_awake(&self, live: bool) -> Box<dyn Any> {
        Box::new(crate::source::Awake::new(live))
    }

    fn power_warning(&self) -> Option<String> {
        #[cfg(windows)]
        {
            rebootly_win::on_battery()
                .then(|| "Running on battery: plug the laptop in for a long backup.".into())
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    fn packs(&self) -> Vec<CompiledPack> {
        let dir = self
            .args
            .packs_dir
            .clone()
            .or_else(rebootly_core::packs::load::default_dir);
        rebootly_core::packs::load::load_all(dir.as_deref()).0
    }

    fn restore_target(
        &self,
        dry_run: bool,
        backup_user: &str,
    ) -> anyhow::Result<(Target, Box<dyn RestoreHost + Send>)> {
        if let Some(root) = &self.args.simulate_root {
            return Ok((Target::simulated(root, backup_user), Box::new(SimHost)));
        }
        #[cfg(windows)]
        {
            if Environment::detect() != Environment::LiveWindows {
                anyhow::bail!(
                    "restore runs on the new PC's own Windows, not from WinPE or a recovery system"
                );
            }
            if !dry_run && !rebootly_win::is_elevated().unwrap_or(false) {
                anyhow::bail!(
                    "restoring needs Rebootly started as administrator (right-click → Run as administrator); a dry run works without"
                );
            }
            Ok((
                rebootly_win::live_target()?,
                Box::new(rebootly_win::WinRestoreHost),
            ))
        }
        #[cfg(not(windows))]
        {
            let _ = dry_run;
            anyhow::bail!(
                "restore runs on the new PC's Windows; to preview here, start with --simulate-root DIR"
            )
        }
    }
}
