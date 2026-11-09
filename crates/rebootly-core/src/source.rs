//! The `Source` abstraction (SPEC §3.2) and its offline implementation.
//!
//! Packs, manifest and copy engine talk only to `dyn Source`. `LiveWindows`
//! (rebootly-win, M4) will implement the same trait. Installed-app listing
//! joins the trait in M3/M4 together with its first real implementation.

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::env::Environment;
use crate::expand::VarSet;
use crate::folders::{FolderPath, resolve_user_folders, user_vars};
use crate::hive::dirty::{self, HiveState};
use crate::hive::{Hive, HiveError};
use crate::install::{OsInfo, WindowsInstall, read_os_info};
use crate::profiles::{
    AccountKind, ProfileDirs, SkippedProfile, has_microsoft_account, read_profile_list, system_vars,
};
use crate::winpath::{Mapped, map_to_volume, resolve_ci};

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

pub trait Source {
    fn environment(&self) -> Environment;
    fn os_info(&self) -> &OsInfo;
    fn profile_dirs(&self) -> &ProfileDirs;
    /// Profiles from `ProfileList`, minus system accounts.
    fn profiles(&self) -> Result<Vec<Profile>, SourceError>;
    fn skipped_profiles(&self) -> &[SkippedProfile];
    fn resolve_folders(&self, profile: &Profile) -> Result<Vec<ResolvedFolder>, SourceError>;
    /// Open a local file for reading. Never opens for write.
    fn open_read(&self, path: &Path) -> std::io::Result<File>;
    fn open_user_hive(&self, profile: &Profile) -> Result<Option<Hive>, SourceError>;
}

/// Windows installed on a mounted volume (WinPE or Linux; SPEC §7).
#[derive(Debug)]
pub struct OfflineVolume {
    install: WindowsInstall,
    environment: Environment,
    os: OsInfo,
    vars: VarSet,
    entries: Vec<crate::profiles::ProfileEntry>,
    skipped: Vec<SkippedProfile>,
    dirs: ProfileDirs,
}

impl OfflineVolume {
    pub fn open(root: &Path) -> Result<Self, SourceError> {
        let install =
            WindowsInstall::from_path(root).ok_or_else(|| SourceError::NotWindows(root.into()))?;
        let sw_path = install.config_hive("SOFTWARE");
        let software = Hive::open(&sw_path).map_err(hive_err(&sw_path))?;
        // SYSTEM is optional: we only use it for computer name and env vars.
        let system = Hive::open(&install.config_hive("SYSTEM")).ok();
        let os = read_os_info(&software, system.as_ref()).map_err(hive_err(&sw_path))?;
        let vars = system_vars(&software, system.as_ref()).map_err(hive_err(&sw_path))?;
        let list = read_profile_list(&software, &vars).map_err(hive_err(&sw_path))?;
        Ok(OfflineVolume {
            install,
            environment: Environment::detect(),
            os,
            vars,
            entries: list.entries,
            skipped: list.skipped,
            dirs: list.dirs,
        })
    }

    pub fn install(&self) -> &WindowsInstall {
        &self.install
    }

    pub fn map(&self, win: &str) -> Mapped {
        map_to_volume(win, self.os.system_drive(), &self.install.volume_root)
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
        match Hive::open(&hp) {
            Ok(h) => {
                let state = dirty::assess(&hp, h.header());
                if state.is_dirty() {
                    p.warnings.push(
                        "NTUSER.DAT is dirty (unapplied transaction logs); read the base hive, \
                         recent settings may be missing"
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
                p.hive = Some(UserHive { path: hp, state });
            }
            Err(err) => p.warnings.push(format!("cannot read NTUSER.DAT: {err}")),
        }
        p
    }
}

impl Source for OfflineVolume {
    fn environment(&self) -> Environment {
        self.environment
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
        File::open(path) // read-only by construction
    }

    fn open_user_hive(&self, profile: &Profile) -> Result<Option<Hive>, SourceError> {
        match &profile.hive {
            Some(h) => Hive::open(&h.path).map(Some).map_err(hive_err(&h.path)),
            None => Ok(None),
        }
    }
}
