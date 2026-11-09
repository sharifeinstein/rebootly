//! The `Source` abstraction (SPEC §3.2) and `OfflineVolume`, a Windows
//! install read through a [`VolumeAccess`] strategy.
//!
//! Packs, manifest and copy engine talk only to `dyn Source`. Offline,
//! `OfflineVolume::open` reads a mounted volume. `rebootly-win`'s
//! `LiveWindows` wraps `OfflineVolume::open_with` and a live access strategy,
//! so the resolution logic is the same in every mode.

mod access;

pub use access::{HiveRef, LoadedHive, OfflineAccess, VolumeAccess, clean_state};

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::apps::{InstalledApp, machine_apps};
use crate::env::Environment;
use crate::expand::VarSet;
use crate::folders::{FolderPath, resolve_user_folders, user_vars};
use crate::hive::dirty::HiveState;
use crate::hive::{Hive, HiveError};
use crate::install::{OsInfo, WindowsInstall, read_os_info};
use crate::profiles::{
    AccountKind, ProfileDirs, SkippedProfile, has_microsoft_account, read_profile_list, system_vars,
};
use crate::winpath::{Mapped, resolve_ci};

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error(
        "no Windows installation found at {0} (looked for Windows\\System32\\config\\SOFTWARE)"
    )]
    NotWindows(PathBuf),
    #[error("cannot read hive {path}: {source}")]
    Hive { path: PathBuf, source: HiveError },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn hive_err(path: &Path) -> impl FnOnce(HiveError) -> SourceError + '_ {
    move |source| SourceError::Hive {
        path: path.to_path_buf(),
        source,
    }
}

/// The user's registry hive and its health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserHive {
    pub path: PathBuf,
    pub state: HiveState,
    /// Clean copy saved from the live registry (live mode, signed-in user).
    pub snapshot: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Profile {
    pub sid: String,
    pub username: String,
    pub kind: AccountKind,
    /// Windows-style profile path from `ProfileList`.
    pub profile_path: String,
    pub local: Mapped,
    /// `None` when NTUSER.DAT is missing or unreadable (see `warnings`).
    pub hive: Option<UserHive>,
    pub warnings: Vec<String>,
}

/// A known folder resolved to a Windows path and mapped to local disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedFolder {
    #[serde(flatten)]
    pub path: FolderPath,
    pub local: Mapped,
    /// Folder lives in the user's OneDrive (Known Folder Move).
    pub onedrive: bool,
}

pub trait Source: Send + Sync {
    fn environment(&self) -> Environment;
    /// Root of the source system volume (backups must never land inside it).
    fn source_root(&self) -> &Path;
    fn os_info(&self) -> &OsInfo;
    fn profile_dirs(&self) -> &ProfileDirs;
    /// Profiles from `ProfileList`, minus system accounts.
    fn profiles(&self) -> Result<Vec<Profile>, SourceError>;
    fn skipped_profiles(&self) -> &[SkippedProfile];
    fn resolve_folders(&self, profile: &Profile) -> Result<Vec<ResolvedFolder>, SourceError>;
    /// Open a local file for reading. Never opens for write.
    fn open_read(&self, path: &Path) -> std::io::Result<File>;
    fn open_user_hive(&self, profile: &Profile) -> Result<Option<Hive>, SourceError>;
    /// Machine-wide installed apps (Uninstall keys, 64- and 32-bit).
    fn machine_apps(&self) -> &[InstalledApp];
    /// The user's variables (`%APPDATA%`, `%OneDrive%`, …) from the source.
    fn user_vars(&self, profile: &Profile) -> Result<VarSet, SourceError>;
    /// Map a Windows path from the source machine to a local path.
    fn map_path(&self, win: &str) -> Mapped;
    /// A clean copy of hive `file` to back up instead of the file and its
    /// logs (live mode, hive loaded in the registry). Default: none.
    fn hive_snapshot(
        &self,
        _file: &Path,
        _which: HiveRef<'_>,
    ) -> Result<Option<PathBuf>, SourceError> {
        Ok(None)
    }
    /// Live-only Tier 4 captures (winget export, Wi-Fi export, printers),
    /// run now into private temp files. `sensitive`: the bundle is
    /// available, so captures holding secrets may run. Default: none.
    fn system_captures(&self, _sensitive: bool) -> Captures {
        Captures::default()
    }
}

/// A file produced by a live command, to back up like a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    /// Destination in the backup (or in the bundle when `sensitive`).
    pub dest: String,
    pub file: PathBuf,
    /// What produced it, e.g. `winget export`.
    pub origin: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captures {
    pub files: Vec<Capture>,
    /// Commands that failed or were unavailable (plain language).
    pub warnings: Vec<String>,
}

/// Windows installed on a mounted volume (WinPE or Linux; SPEC §7).
#[derive(Debug)]
pub struct OfflineVolume {
    install: WindowsInstall,
    access: Box<dyn VolumeAccess>,
    environment: Environment,
    os: OsInfo,
    vars: VarSet,
    entries: Vec<crate::profiles::ProfileEntry>,
    skipped: Vec<SkippedProfile>,
    dirs: ProfileDirs,
    apps: Vec<InstalledApp>,
}

impl OfflineVolume {
    /// Open a mounted volume offline (read-only).
    pub fn open(root: &Path) -> Result<Self, SourceError> {
        Self::open_with(root, Box::new(OfflineAccess))
    }

    /// Open the install at `root` through `access` (e.g. the live registry).
    pub fn open_with(root: &Path, access: Box<dyn VolumeAccess>) -> Result<Self, SourceError> {
        let install =
            WindowsInstall::from_path(root).ok_or_else(|| SourceError::NotWindows(root.into()))?;
        let sw_path = install.config_hive("SOFTWARE");
        let software = access
            .load_hive(&sw_path, HiveRef::Software)
            .map_err(hive_err(&sw_path))?
            .hive;
        // SYSTEM is optional: we only use it for computer name and env vars.
        let system = access
            .load_hive(&install.config_hive("SYSTEM"), HiveRef::System)
            .ok()
            .map(|l| l.hive);
        let system = system.as_ref();
        let os = read_os_info(&software, system).map_err(hive_err(&sw_path))?;
        let vars = system_vars(&software, system).map_err(hive_err(&sw_path))?;
        let list = read_profile_list(&software, &vars).map_err(hive_err(&sw_path))?;
        let apps = machine_apps(&software).map_err(hive_err(&sw_path))?;
        Ok(OfflineVolume {
            environment: access.environment(),
            install,
            access,
            os,
            vars,
            entries: list.entries,
            skipped: list.skipped,
            dirs: list.dirs,
            apps,
        })
    }

    pub fn install(&self) -> &WindowsInstall {
        &self.install
    }

    pub fn map(&self, win: &str) -> Mapped {
        self.access
            .map(win, self.os.system_drive(), &self.install.volume_root)
    }

    /// The access strategy (live or offline) this volume is read through.
    pub fn access(&self) -> &dyn VolumeAccess {
        self.access.as_ref()
    }

    fn ntuser_path(local_profile: &Path) -> PathBuf {
        resolve_ci(local_profile, &["NTUSER.DAT".to_owned()]).0
    }

    fn build_profile(&self, e: &crate::profiles::ProfileEntry) -> Profile {
        let local = self.map(&e.profile_path);
        let mut p = Profile {
            sid: e.sid.clone(),
            username: crate::winpath::leaf(&e.profile_path).unwrap_or_else(|| e.sid.clone()),
            kind: e.kind,
            profile_path: e.profile_path.clone(),
            local: local.clone(),
            hive: None,
            warnings: Vec::new(),
        };
        let dir = match &local {
            Mapped::Local { path, exists: true } => path.clone(),
            Mapped::Local { .. } => {
                p.warnings
                    .push("profile folder not found on the volume".into());
                return p;
            }
            other => {
                p.warnings
                    .push(format!("profile is not on the system volume ({other:?})"));
                return p;
            }
        };
        let hp = Self::ntuser_path(&dir);
        match self.access.load_hive(&hp, HiveRef::User(&e.sid)) {
            Ok(LoadedHive {
                hive: h,
                state,
                snapshot,
            }) => {
                if state.is_dirty() {
                    p.warnings.push(
                        "NTUSER.DAT is dirty (unapplied transaction logs): recent settings may be \
                         missing from what was read; the hive and its logs are kept raw in backups"
                            .into(),
                    );
                }
                if !state.checksum_ok {
                    p.warnings
                        .push("NTUSER.DAT header checksum mismatch".into());
                }
                if p.kind == AccountKind::LocalOrDomain
                    && has_microsoft_account(&h).unwrap_or(false)
                {
                    p.kind = AccountKind::Microsoft;
                }
                p.hive = Some(UserHive {
                    path: hp,
                    state,
                    snapshot,
                });
            }
            Err(_) if !hp.exists() => p.warnings.push(
                "NTUSER.DAT not found (user never signed in, or the profile is damaged); \
                 default folder locations used"
                    .into(),
            ),
            Err(err) => p.warnings.push(format!("cannot read NTUSER.DAT: {err}")),
        }
        p
    }
}

impl Source for OfflineVolume {
    fn environment(&self) -> Environment {
        self.environment
    }

    fn source_root(&self) -> &Path {
        &self.install.volume_root
    }

    fn os_info(&self) -> &OsInfo {
        &self.os
    }

    fn profile_dirs(&self) -> &ProfileDirs {
        &self.dirs
    }

    fn profiles(&self) -> Result<Vec<Profile>, SourceError> {
        Ok(self.entries.iter().map(|e| self.build_profile(e)).collect())
    }

    fn skipped_profiles(&self) -> &[SkippedProfile] {
        &self.skipped
    }

    fn user_vars(&self, profile: &Profile) -> Result<VarSet, SourceError> {
        let hive = self.open_user_hive(profile)?;
        let hp = profile
            .hive
            .as_ref()
            .map(|h| h.path.clone())
            .unwrap_or_default();
        user_vars(&self.vars, hive.as_ref(), &profile.profile_path).map_err(hive_err(&hp))
    }

    fn map_path(&self, win: &str) -> Mapped {
        self.map(win)
    }

    fn hive_snapshot(
        &self,
        file: &Path,
        which: HiveRef<'_>,
    ) -> Result<Option<PathBuf>, SourceError> {
        self.access
            .hive_snapshot(file, which)
            .map_err(hive_err(file))
    }

    fn resolve_folders(&self, profile: &Profile) -> Result<Vec<ResolvedFolder>, SourceError> {
        let hive = self.open_user_hive(profile)?;
        let hp = profile
            .hive
            .as_ref()
            .map(|h| h.path.clone())
            .unwrap_or_default();
        let vars =
            user_vars(&self.vars, hive.as_ref(), &profile.profile_path).map_err(hive_err(&hp))?;
        let folders = resolve_user_folders(hive.as_ref(), &vars, &profile.profile_path)
            .map_err(hive_err(&hp))?;
        let onedrive_root = vars
            .get("OneDrive")
            .map(|s| s.trim_end_matches('\\').to_uppercase());
        Ok(folders
            .into_iter()
            .map(|f| {
                let up = f.win_path.to_uppercase();
                let onedrive = onedrive_root
                    .as_deref()
                    .is_some_and(|r| up == r || up.starts_with(&format!("{r}\\")));
                ResolvedFolder {
                    local: self.map(&f.win_path),
                    path: f,
                    onedrive,
                }
            })
            .collect())
    }

    fn open_read(&self, path: &Path) -> std::io::Result<File> {
        self.access.open_read(path)
    }

    fn machine_apps(&self) -> &[InstalledApp] {
        &self.apps
    }

    fn open_user_hive(&self, profile: &Profile) -> Result<Option<Hive>, SourceError> {
        match &profile.hive {
            Some(h) => self
                .access
                .load_hive(&h.path, HiveRef::User(&profile.sid))
                .map(|l| Some(l.hive))
                .map_err(hive_err(&h.path)),
            None => Ok(None),
        }
    }
}
