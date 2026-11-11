//! `OfflineVolume`: a Windows install read through a [`VolumeAccess`]
//! strategy (offline from WinPE/Linux, or live via rebootly-win).

use super::*;
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
    /// Other drive letters of the offline Windows that are mounted here.
    drives: Vec<(char, PathBuf)>,
    /// Offline only: hiberfil.sys holds an image to resume (Fast Startup).
    hibernated: bool,
}

impl OfflineVolume {
    /// Open a mounted volume offline (read-only).
    pub fn open(root: &Path) -> Result<Self, SourceError> {
        Self::open_with(root, Box::new(OfflineAccess))
    }

    /// Open the install at `root` through `access` (e.g. the live registry).
    pub fn open_with(root: &Path, access: Box<dyn VolumeAccess>) -> Result<Self, SourceError> {
        Self::open_full(root, access, &[])
    }

    /// Open offline, also mapping the install's other drive letters (from
    /// `SYSTEM\MountedDevices`) onto whichever of `mounted` is that
    /// partition (SPEC §7).
    pub fn open_with_volumes(root: &Path, mounted: &[MountedVolume]) -> Result<Self, SourceError> {
        Self::open_full(root, Box::new(OfflineAccess), mounted)
    }

    fn open_full(
        root: &Path,
        access: Box<dyn VolumeAccess>,
        mounted: &[MountedVolume],
    ) -> Result<Self, SourceError> {
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
        let drives = match system {
            Some(sys) if !mounted.is_empty() => match_letters(
                &drive_letters(sys).unwrap_or_default(),
                os.system_drive(),
                mounted,
                &install.volume_root,
            ),
            _ => Vec::new(),
        };
        // The live wrapper (rebootly-win) doesn't pass `source_notes` on:
        // the running Windows' own hiberfil.sys means nothing.
        let hibernated = crate::volume::hibernated(&install.volume_root);
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
            drives,
            hibernated,
        })
    }

    pub fn install(&self) -> &WindowsInstall {
        &self.install
    }

    pub fn map(&self, win: &str) -> Mapped {
        if let WinPath::Drive { letter, parts } = WinPath::parse(win) {
            if let Some((_, root)) = self.drives.iter().find(|(l, _)| *l == letter) {
                if parts.iter().any(|p| p == "..") {
                    return Mapped::Unmappable;
                }
                let (path, exists) = resolve_ci(root, &parts);
                return Mapped::Local { path, exists };
            }
        }
        self.access
            .map(win, self.os.system_drive(), &self.install.volume_root)
    }

    /// The install's other drive letters that are mounted here.
    pub fn drives(&self) -> &[(char, PathBuf)] {
        &self.drives
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

    fn source_notes(&self) -> Vec<String> {
        if self.hibernated {
            vec![crate::volume::HIBERNATED_NOTE.into()]
        } else {
            Vec::new()
        }
    }

    fn other_drives(&self) -> Vec<(char, PathBuf)> {
        self.drives.clone()
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
