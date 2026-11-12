//! Tier 4 on restore (SPEC §4): `winget import`, Wi-Fi profiles, power
//! plan, certificates and (opt-in) scheduled tasks become actions; printers,
//! default apps, drivers and the app list become manual steps.

use std::path::PathBuf;

use super::input::{Backup, Bundle};
use super::plan::{AppsMode, RestoreOptions};
use crate::backup::manifest::{FileStatus, ManualStep};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysAction {
    Winget {
        file: PathBuf,
    },
    Wifi {
        file: PathBuf,
        name: String,
    },
    Power {
        file: PathBuf,
        /// The old PC's active scheme (from `active-scheme.txt`), if known.
        guid: Option<String>,
        name: Option<String>,
    },
    Cert {
        file: PathBuf,
        thumbprint: String,
        password: Option<PathBuf>,
    },
    Task {
        name: String,
        file: PathBuf,
    },
}

impl SysAction {
    pub fn describe(&self) -> String {
        match self {
            SysAction::Winget { .. } => {
                "install apps with winget import (system/winget.json)".into()
            }
            SysAction::Wifi { name, .. } => format!("add Wi-Fi network {name}"),
            SysAction::Power { name, .. } => match name {
                Some(n) => format!("power plan {n}: make it the active plan"),
                None => "power plan: import it and make it active".into(),
            },
            SysAction::Cert {
                thumbprint,
                password,
                ..
            } => format!(
                "import certificate {thumbprint}{}",
                if password.is_some() {
                    " (with its private key)"
                } else {
                    ""
                }
            ),
            SysAction::Task { name, .. } => format!("create scheduled task {name}"),
        }
    }
}

#[derive(Debug, Default)]
pub struct SystemPlan {
    pub apps: Option<SysAction>,
    pub actions: Vec<SysAction>,
    pub manual: Vec<ManualStep>,
}

fn step(title: impl Into<String>, detail: impl Into<String>) -> ManualStep {
    ManualStep {
        title: title.into(),
        detail: detail.into(),
    }
}

/// Up to `max` names, then "and N more".
pub fn name_list(names: &[String], max: usize) -> String {
    let mut s = names
        .iter()
        .take(max)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > max {
        s.push_str(&format!(", and {} more", names.len() - max));
    }
    s
}

/// `user_sid`: SID of the backup user being restored (certificates are
/// imported only for their owner).
pub fn plan_system(
    backup: &Backup,
    bundle: Option<&Bundle>,
    opts: &RestoreOptions,
    user_sid: &str,
) -> SystemPlan {
    let mut p = SystemPlan::default();
    let has = |rel: &str| backup.file(rel).and_then(|_| backup.local(rel));
    let read_json = |rel: &str| -> Option<serde_json::Value> {
        serde_json::from_slice(&std::fs::read(has(rel)?).ok()?).ok()
    };
    match (has("system/winget.json"), opts.apps) {
        (Some(file), AppsMode::Install | AppsMode::Only) => {
            p.apps = Some(SysAction::Winget { file })
        }
        (Some(_), AppsMode::Skip) => {}
        (None, _) => p.manual.push(step(
            "Install your apps",
            "This backup has no winget list (it was made offline, or winget wasn't installed); \
             reinstall apps from the list below.",
        )),
    }
    if let Some(v) = read_json("system/apps.json") {
        let names: Vec<String> = v["machine"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a["name"].as_str().map(String::from))
            .collect();
        if !names.is_empty() {
            p.manual.push(step(
                "Apps that were on the old PC",
                format!(
                    "Check that you have what you need (winget can't install everything): {}. \
                     Full list: system/apps.json in the backup.",
                    name_list(&names, 40)
                ),
            ));
        }
    }
    if let Some(v) = read_json("system/printers.json") {
        let list = match v {
            serde_json::Value::Array(a) => a,
            one => vec![one],
        };
        let names: Vec<String> = list
            .iter()
            .filter_map(|x| x["Name"].as_str().map(String::from))
            .filter(|n| !is_builtin_printer(n))
            .collect();
        if !names.is_empty() {
            p.manual.push(step(
                "Add your printers",
                format!(
                    "Settings › Bluetooth & devices › Printers & scanners: {}. Drivers and ports \
                     are in system/printers.json.",
                    name_list(&names, 20)
                ),
            ));
        }
    }
    if has("system/default-apps.xml").is_some() {
        p.manual.push(step(
            "Choose default apps",
            "Windows doesn't let tools set default apps silently. Set them in Settings › Apps › \
             Default apps; the old choices are in system/default-apps.xml.",
        ));
    }
    if has("system/drivers.txt").is_some() {
        p.manual.push(step(
            "Drivers (reference only)",
            "The old PC's third-party drivers are listed in system/drivers.txt. Rebootly never \
             installs drivers; get current ones from the device maker if something is missing.",
        ));
    }
    if opts.system {
        if let Some(file) = has("system/power/active-scheme.pow") {
            let txt = has("system/power/active-scheme.txt")
                .and_then(|t| std::fs::read(t).ok())
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            p.actions.push(SysAction::Power {
                file,
                guid: find_guid(&txt).map(str::to_ascii_lowercase),
                name: scheme_name(&txt),
            });
        }
    }
    tasks(backup, opts, &mut p);
    sensitive(backup, bundle, opts, user_sid, &mut p);
    p
}

fn is_builtin_printer(n: &str) -> bool {
    [
        "Microsoft Print to PDF",
        "Microsoft XPS Document Writer",
        "Fax",
        "OneNote",
    ]
    .iter()
    .any(|b| n.starts_with(b))
}

fn tasks(backup: &Backup, opts: &RestoreOptions, p: &mut SystemPlan) {
    let found: Vec<(String, PathBuf)> = backup
        .manifest
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Ok)
        .filter_map(|f| {
            let rest = f.path.strip_prefix("system/tasks/")?;
            Some((
                format!(r"\{}", rest.replace('/', "\\")),
                backup.local(&f.path)?,
            ))
        })
        .collect();
    if found.is_empty() {
        return;
    }
    if opts.tasks && opts.system {
        p.actions.extend(
            found
                .into_iter()
                .map(|(name, file)| SysAction::Task { name, file }),
        );
    } else {
        let names: Vec<String> = found.into_iter().map(|t| t.0).collect();
        p.manual.push(step(
            "Scheduled tasks (not restored)",
            format!(
                "Apps usually re-create their own tasks when installed. To restore these as \
                 well, run the restore again with --tasks: {}.",
                name_list(&names, 20)
            ),
        ));
    }
}

/// The first bare GUID in `text` (`powercfg` wording is localized, the GUID isn't).
pub fn find_guid(text: &str) -> Option<&str> {
    let b = text.as_bytes();
    (0..b.len().saturating_sub(35)).find_map(|i| {
        let c = text.get(i..i + 36)?;
        let ok = c.bytes().enumerate().all(|(j, x)| {
            if matches!(j, 8 | 13 | 18 | 23) {
                x == b'-'
            } else {
                x.is_ascii_hexdigit()
            }
        });
        ok.then_some(c)
    })
}

/// `… GUID: 381b…  (Balanced)` → `Balanced`.
pub fn scheme_name(text: &str) -> Option<String> {
    let t = text.trim();
    let inner = t.strip_suffix(')')?.rsplit_once('(')?.1.trim();
    (!inner.is_empty()).then(|| inner.to_owned())
}

/// `<name>` of a WLAN profile XML.
pub fn wifi_name(xml: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(xml);
    let start = s.find("<name>")? + "<name>".len();
    let end = s[start..].find("</name>")? + start;
    let n = s[start..end].trim();
    (!n.is_empty()).then(|| {
        n.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    })
}

/// Certificates are exported only for the user who ran the backup
/// (`system/certs/OWNER.txt`: SID, then name). `Err`: why they aren't
/// imported for `user_sid`.
pub fn cert_owner_check(owner: Option<&str>, user_sid: &str) -> Result<(), String> {
    let Some(text) = owner else {
        return Err(
            "this backup doesn't record whose certificates they are (made by an older \
             Rebootly), so they weren't imported. If they're yours, get the .pfx \
                    files and PFX-PASSWORD.txt with `rebootly-cli sensitive extract` and import \
                    them in certmgr.msc"
                .into(),
        );
    };
    let mut lines = text.lines().map(str::trim);
    let sid = lines.next().unwrap_or("");
    let name = lines.next().filter(|n| !n.is_empty()).unwrap_or(sid);
    if sid.eq_ignore_ascii_case(user_sid) {
        Ok(())
    } else {
        Err(format!(
            "they belong to {name}, who made the backup; restore that user (--user, signed \
             in as the user they're for) to import them"
        ))
    }
}

fn sensitive(
    backup: &Backup,
    bundle: Option<&Bundle>,
    opts: &RestoreOptions,
    user_sid: &str,
    p: &mut SystemPlan,
) {
    let Some(rec) = &backup.manifest.sensitive else {
        return;
    };
    let Some(b) = bundle else {
        let names: Vec<String> = rec.files.iter().map(|f| f.path.clone()).collect();
        p.manual.push(step(
            "Encrypted items not restored",
            format!(
                "sensitive.age was not opened. Run the restore again with --sensitive and the \
                 backup passphrase to restore: {}.",
                name_list(&names, 15)
            ),
        ));
        return;
    };
    let password = b
        .local("system/certs/PFX-PASSWORD.txt")
        .filter(|p| p.is_file());
    let owner = b
        .local("system/certs/OWNER.txt")
        .and_then(|f| std::fs::read(f).ok())
        .map(|x| String::from_utf8_lossy(&x).into_owned());
    let certs_ok = cert_owner_check(owner.as_deref(), user_sid);
    let mut certs_skipped = Vec::new();
    let mut offline_wifi = Vec::new();
    for f in &b.index.files {
        let Some(local) = b.local(&f.path) else {
            continue;
        };
        let lower = f.path.to_ascii_lowercase();
        if let Some(name) = lower.strip_prefix("system/wifi-export/") {
            if name.ends_with(".xml") && opts.system {
                let name = std::fs::read(&local).ok().and_then(|x| wifi_name(&x));
                p.actions.push(SysAction::Wifi {
                    file: local,
                    name: name.unwrap_or_else(|| f.path.rsplit('/').next().unwrap_or("").into()),
                });
            }
        } else if lower.starts_with("system/wifi/") && lower.ends_with(".xml") {
            if let Some(n) = std::fs::read(&local).ok().and_then(|x| wifi_name(&x)) {
                offline_wifi.push(n);
            }
        } else if let Some(name) = lower.strip_prefix("system/certs/") {
            let is_pfx = name.ends_with(".pfx");
            if (is_pfx || name.ends_with(".cer")) && opts.system {
                let stem = f.path.rsplit('/').next().unwrap_or("");
                let thumbprint = stem.rsplit_once('.').map_or(stem, |s| s.0).to_owned();
                if certs_ok.is_err() {
                    certs_skipped.push(thumbprint);
                    continue;
                }
                p.actions.push(SysAction::Cert {
                    file: local,
                    thumbprint,
                    password: if is_pfx { password.clone() } else { None },
                });
            }
        }
    }
    if let (Err(why), false) = (&certs_ok, certs_skipped.is_empty()) {
        p.manual.push(step(
            "Certificates not imported",
            format!("{}: {why}.", name_list(&certs_skipped, 10)),
        ));
    }
    if !offline_wifi.is_empty() {
        p.manual.push(step(
            "Re-join Wi-Fi networks",
            format!(
                "Their passwords can't be recovered from an offline backup: {}.",
                name_list(&offline_wifi, 20)
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::cert_owner_check;

    #[test]
    fn certificates_only_for_their_owner() {
        let owner = "S-1-5-21-1-2-3-1001\r\nPicard\r\n";
        assert!(cert_owner_check(Some(owner), "s-1-5-21-1-2-3-1001").is_ok());
        let e = cert_owner_check(Some(owner), "S-1-5-21-1-2-3-1002").unwrap_err();
        assert!(e.contains("belong to Picard"), "{e}");
        assert!(cert_owner_check(None, "S-1-5-21-1-2-3-1001").is_err());
    }
}
