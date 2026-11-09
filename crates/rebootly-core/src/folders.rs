//! Known-folder table and per-user folder resolution (SPEC §3.4, §4 Tier 1).
//!
//! Order: `User Shell Folders` (REG_EXPAND_SZ, expanded manually) →
//! `Shell Folders` (legacy, pre-expanded) → the Windows default under the
//! profile. Each result records which source was used.

use serde::Serialize;

use crate::expand::VarSet;
use crate::hive::{Hive, Result as HiveResult};

pub const USER_SHELL_FOLDERS: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders";
pub const SHELL_FOLDERS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownFolder {
    /// Stable name used in the backup layout (`users/<u>/folders/<name>`).
    pub name: &'static str,
    /// KNOWNFOLDERID GUID.
    pub guid: &'static str,
    /// Value names in (User) Shell Folders, preferred first.
    pub reg_names: &'static [&'static str],
    /// Default location relative to the profile.
    pub default_rel: &'static str,
}

const fn kf(
    name: &'static str,
    guid: &'static str,
    reg_names: &'static [&'static str],
    default_rel: &'static str,
) -> KnownFolder {
    KnownFolder {
        name,
        guid,
        reg_names,
        default_rel,
    }
}

/// Tier 1 user folders plus the AppData roots that packs build on.
pub const KNOWN_FOLDERS: &[KnownFolder] = &[
    kf(
        "Desktop",
        "{B4BFCC3A-DB2C-424C-B029-7FE99A87C641}",
        &["Desktop"],
        "Desktop",
    ),
    kf(
        "Documents",
        "{FDD39AD0-238F-46AF-ADB4-6C85480369C7}",
        &["Personal", "{F42EE2D3-909F-4907-8871-4C22FC0BF756}"],
        "Documents",
    ),
    kf(
        "Downloads",
        "{374DE290-123F-4565-9164-39C4925E467B}",
        &[
            "{374DE290-123F-4565-9164-39C4925E467B}",
            "{7D83EE9B-2244-4E70-B1F5-5393042AF1E4}",
        ],
        "Downloads",
    ),
    kf(
        "Pictures",
        "{33E28130-4E1E-4676-835A-98395C3BC3BB}",
        &["My Pictures", "{0DDD015D-B06C-45D5-8C4C-F59713854639}"],
        "Pictures",
    ),
    kf(
        "Music",
        "{4BD8D571-6D19-48D3-BE97-422220080E43}",
        &["My Music", "{A0C69A99-21C8-4671-8703-7934162FCF1D}"],
        "Music",
    ),
    kf(
        "Videos",
        "{18989B1D-99B5-455B-841C-AB7C74E4DDFC}",
        &["My Video", "{35286A68-3C57-41A1-BBB1-0EAE73D76C95}"],
        "Videos",
    ),
    kf(
        "Favorites",
        "{1777F761-68AD-4D8A-87BD-30B759FA33DD}",
        &["Favorites"],
        "Favorites",
    ),
    kf(
        "Links",
        "{BFB9D5E0-C6A9-404C-B2B2-AE6DB6AF4968}",
        &["{BFB9D5E0-C6A9-404C-B2B2-AE6DB6AF4968}"],
        "Links",
    ),
    kf(
        "Contacts",
        "{56784854-C6CB-462B-8169-88E350ACB882}",
        &["{56784854-C6CB-462B-8169-88E350ACB882}"],
        "Contacts",
    ),
    kf(
        "SavedGames",
        "{4C5C32FF-BB9D-43B0-B5B4-2D72E54EAAA4}",
        &["{4C5C32FF-BB9D-43B0-B5B4-2D72E54EAAA4}"],
        "Saved Games",
    ),
    kf(
        "Searches",
        "{7D1D3A04-DEBB-4115-95CF-2F29DA2920DA}",
        &["{7D1D3A04-DEBB-4115-95CF-2F29DA2920DA}"],
        "Searches",
    ),
    kf(
        "3DObjects",
        "{31C0DD25-9439-4F12-BF41-7FF4EDA38722}",
        &["{31C0DD25-9439-4F12-BF41-7FF4EDA38722}"],
        "3D Objects",
    ),
    kf(
        "RoamingAppData",
        "{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}",
        &["AppData"],
        r"AppData\Roaming",
    ),
    kf(
        "LocalAppData",
        "{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}",
        &["Local AppData"],
        r"AppData\Local",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderOrigin {
    UserShellFolders,
    ShellFolders,
    Default,
}

/// A folder resolved to a Windows path (mapping to disk happens in `source`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderPath {
    pub name: &'static str,
    pub guid: &'static str,
    pub win_path: String,
    pub origin: FolderOrigin,
    /// Registry value name the path came from, if any.
    pub reg_name: Option<&'static str>,
    pub unresolved_vars: Vec<String>,
}

/// Resolve every known folder for one user. `ntuser` is `None` when the
/// user's hive is missing or unreadable; defaults are used then.
pub fn resolve_user_folders(
    ntuser: Option<&Hive>,
    vars: &VarSet,
    profile_path: &str,
) -> HiveResult<Vec<FolderPath>> {
    let (usf, sf) = match ntuser {
        Some(h) => (h.open_key(USER_SHELL_FOLDERS)?, h.open_key(SHELL_FOLDERS)?),
        None => (None, None),
    };
    let lookup = |key: &Option<crate::hive::Key<'_>>, kf: &KnownFolder| -> HiveResult<_> {
        let Some(k) = key else { return Ok(None) };
        for name in kf.reg_names {
            if let Some(s) = k.string(name)? {
                if !s.trim().is_empty() {
                    return Ok(Some((*name, s)));
                }
            }
        }
        Ok(None)
    };
    let mut out = Vec::with_capacity(KNOWN_FOLDERS.len());
    for kf in KNOWN_FOLDERS {
        let (origin, reg_name, raw) = if let Some((n, s)) = lookup(&usf, kf)? {
            (FolderOrigin::UserShellFolders, Some(n), s)
        } else if let Some((n, s)) = lookup(&sf, kf)? {
            (FolderOrigin::ShellFolders, Some(n), s)
        } else {
            let base = profile_path.trim_end_matches('\\');
            (
                FolderOrigin::Default,
                None,
                format!(r"{base}\{}", kf.default_rel),
            )
        };
        let e = vars.expand(&raw);
        out.push(FolderPath {
            name: kf.name,
            guid: kf.guid,
            win_path: e.text,
            origin,
            reg_name,
            unresolved_vars: e.unresolved,
        });
    }
    Ok(out)
}

/// Per-user variables, layered on the machine set: user `Environment`
/// values override system ones; profile-derived ones override both.
pub fn user_vars(base: &VarSet, ntuser: Option<&Hive>, profile_path: &str) -> HiveResult<VarSet> {
    let mut v = base.clone();
    if let Some(h) = ntuser {
        if let Some(env) = h.open_key("Environment")? {
            for val in env.values()? {
                if let Some(s) = val.as_string() {
                    v.set(&val.name, s);
                }
            }
        }
    }
    let p = profile_path.trim_end_matches('\\');
    if let Some(user) = crate::winpath::leaf(p) {
        v.set("USERNAME", user);
    }
    if p.len() >= 2 && p.as_bytes()[1] == b':' {
        v.set("HOMEDRIVE", &p[..2]).set("HOMEPATH", &p[2..]);
    }
    v.set("USERPROFILE", p)
        .set("APPDATA", format!(r"{p}\AppData\Roaming"))
        .set("LOCALAPPDATA", format!(r"{p}\AppData\Local"));
    Ok(v)
}

#[cfg(test)]
mod tests;

/// Live-mode cross-check (SPEC §3.4): compare folders resolved from the
/// hive with what Windows reports (`api`, e.g. `SHGetKnownFolderPath` by
/// GUID). The hive result is kept; each disagreement becomes a warning.
pub fn cross_check(resolved: &[FolderPath], api: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let norm = |s: &str| s.trim_end_matches('\\').to_lowercase();
    resolved
        .iter()
        .filter_map(|f| {
            let reported = api(f.guid)?;
            (norm(&reported) != norm(&f.win_path)).then(|| {
                format!(
                    "{}: registry says {} but Windows reports {}; using the registry value",
                    f.name, f.win_path, reported
                )
            })
        })
        .collect()
}
