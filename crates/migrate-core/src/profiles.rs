//! `ProfileList` parsing and account classification (SPEC §3.4).

use serde::Serialize;

use crate::expand::VarSet;
use crate::hive::{Hive, Result as HiveResult};
use crate::install::CURRENT_VERSION;

/// Well-known service SIDs that are never migrated.
const SYSTEM_SIDS: [&str; 3] = ["S-1-5-18", "S-1-5-19", "S-1-5-20"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    /// Entra ID (Azure AD) account: SID prefix `S-1-12-1-`.
    EntraId,
    /// Microsoft account: NTUSER has `IdentityCRL\UserExtendedProperties`.
    Microsoft,
    /// `S-1-5-21-…`: a local or an AD domain account. Telling these apart
    /// offline needs the machine SID (SECURITY/SAM hives); deferred.
    LocalOrDomain,
    Unknown,
}

impl AccountKind {
    pub fn from_sid(sid: &str) -> Self {
        let s = sid.to_ascii_uppercase();
        if s.starts_with("S-1-12-1-") {
            AccountKind::EntraId
        } else if s.starts_with("S-1-5-21-") {
            AccountKind::LocalOrDomain
        } else {
            AccountKind::Unknown
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AccountKind::EntraId => "Entra ID",
            AccountKind::Microsoft => "Microsoft account",
            AccountKind::LocalOrDomain => "local/domain",
            AccountKind::Unknown => "unknown",
        }
    }
}

/// One `ProfileList\<SID>` entry, paths still Windows-style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileEntry {
    pub sid: String,
    /// `ProfileImagePath` as stored (may contain `%SystemDrive%`).
    pub raw_path: String,
    /// `ProfileImagePath` expanded with the source machine's variables.
    pub profile_path: String,
    pub kind: AccountKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedProfile {
    pub sid: String,
    pub reason: String,
}

/// Machine-wide profile locations stored on the `ProfileList` key itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProfileDirs {
    pub profiles_directory: Option<String>,
    pub public: Option<String>,
    pub default: Option<String>,
    pub program_data: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProfileList {
    pub entries: Vec<ProfileEntry>,
    pub skipped: Vec<SkippedProfile>,
    pub dirs: ProfileDirs,
}

pub const PROFILE_LIST: &str = r"Microsoft\Windows NT\CurrentVersion\ProfileList";

pub fn read_profile_list(software: &Hive, vars: &VarSet) -> HiveResult<ProfileList> {
    let mut out = ProfileList::default();
    let Some(list) = software.open_key(PROFILE_LIST)? else {
        return Ok(out);
    };
    let exp = |v: Option<String>| v.map(|s| vars.expand(&s).text);
    out.dirs = ProfileDirs {
        profiles_directory: exp(list.string("ProfilesDirectory")?),
        public: exp(list.string("Public")?),
        default: exp(list.string("Default")?),
        program_data: exp(list.string("ProgramData")?),
    };
    for key in list.subkeys()? {
        let sid = key.name().to_owned();
        let skip = |reason: &str| SkippedProfile {
            sid: sid.clone(),
            reason: reason.into(),
        };
        if SYSTEM_SIDS.iter().any(|s| s.eq_ignore_ascii_case(&sid)) {
            out.skipped.push(skip("system account"));
            continue;
        }
        if sid.to_ascii_lowercase().ends_with(".bak") {
            out.skipped.push(skip(
                "'.bak' duplicate entry (profile was re-created by Windows)",
            ));
            continue;
        }
        let Some(raw) = key.string("ProfileImagePath")? else {
            out.skipped.push(skip("no ProfileImagePath"));
            continue;
        };
        let expanded = vars.expand(&raw);
        out.entries.push(ProfileEntry {
            kind: AccountKind::from_sid(&sid),
            profile_path: expanded.text,
            raw_path: raw,
            sid,
        });
    }
    Ok(out)
}

/// True if the user's NTUSER.DAT shows a signed-in Microsoft account.
pub fn has_microsoft_account(ntuser: &Hive) -> HiveResult<bool> {
    let props = ntuser.open_key(r"Software\Microsoft\IdentityCRL\UserExtendedProperties")?;
    Ok(match props {
        Some(k) => !k.subkeys()?.is_empty(),
        None => false,
    })
}

/// Machine-level variables from SOFTWARE (and SYSTEM's environment).
pub fn system_vars(software: &Hive, system: Option<&Hive>) -> HiveResult<VarSet> {
    let mut v = VarSet::new();
    if let Some(cv) = software.open_key(CURRENT_VERSION)? {
        if let Some(root) = cv.string("SystemRoot")? {
            if let Some(d) = crate::winpath::WinPath::parse(&root).drive() {
                v.set("SystemDrive", format!("{d}:"));
            }
            v.set("windir", root.clone()).set("SystemRoot", root);
        }
    }
    if let Some(w) = software.open_key(r"Microsoft\Windows\CurrentVersion")? {
        for (var, val) in [
            ("ProgramFiles", "ProgramFilesDir"),
            ("ProgramFiles(x86)", "ProgramFilesDir (x86)"),
            ("CommonProgramFiles", "CommonFilesDir"),
            ("CommonProgramFiles(x86)", "CommonFilesDir (x86)"),
        ] {
            if let Some(s) = w.string(val)? {
                v.set(var, s);
            }
        }
    }
    if let Some(list) = software.open_key(PROFILE_LIST)? {
        if let Some(pd) = list.string("ProgramData")? {
            v.set("ProgramData", pd.clone()).set("ALLUSERSPROFILE", pd);
        }
        if let Some(p) = list.string("Public")? {
            v.set("PUBLIC", p);
        }
    }
    if let Some(sys) = system {
        if let Some(cs) = crate::install::current_control_set(sys)? {
            let path = format!(r"{cs}\Control\Session Manager\Environment");
            if let Some(env) = sys.open_key(&path)? {
                for val in env.values()? {
                    if let Some(s) = val.as_string() {
                        v.set_default(&val.name, s);
                    }
                }
            }
        }
    }
    v.set_default("SystemDrive", "C:");
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_sids() {
        assert_eq!(
            AccountKind::from_sid("S-1-12-1-111-222-333-444"),
            AccountKind::EntraId
        );
        assert_eq!(
            AccountKind::from_sid("s-1-5-21-1-2-3-1001"),
            AccountKind::LocalOrDomain
        );
        assert_eq!(AccountKind::from_sid("S-1-5-80-1"), AccountKind::Unknown);
    }
}
