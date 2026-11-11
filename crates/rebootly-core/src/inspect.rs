//! `rebootly-cli inspect`: a serialisable snapshot of what the resolver found.

use serde::Serialize;

use crate::env::Environment;
use crate::install::OsInfo;
use crate::profiles::{ProfileDirs, SkippedProfile};
use crate::source::{Profile, ResolvedFolder, Source, SourceError};

#[derive(Debug, Clone, Serialize)]
pub struct UserReport {
    #[serde(flatten)]
    pub profile: Profile,
    pub folders: Vec<ResolvedFolder>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectReport {
    pub tool_version: &'static str,
    pub environment: Environment,
    pub os: OsInfo,
    pub os_name: String,
    pub profile_dirs: ProfileDirs,
    pub users: Vec<UserReport>,
    pub skipped: Vec<SkippedProfile>,
    /// Other drive letters of the install that are mounted here (offline).
    pub other_drives: Vec<(char, std::path::PathBuf)>,
    /// e.g. the install was hibernated.
    pub notes: Vec<String>,
}

pub fn inspect(src: &dyn Source) -> Result<InspectReport, SourceError> {
    let mut users = Vec::new();
    for profile in src.profiles()? {
        let folders = src.resolve_folders(&profile)?;
        users.push(UserReport { profile, folders });
    }
    Ok(InspectReport {
        tool_version: env!("CARGO_PKG_VERSION"),
        environment: src.environment(),
        os: src.os_info().clone(),
        os_name: src.os_info().friendly_name(),
        profile_dirs: src.profile_dirs().clone(),
        users,
        skipped: src.skipped_profiles().to_vec(),
        other_drives: src.other_drives(),
        notes: src.source_notes(),
    })
}
