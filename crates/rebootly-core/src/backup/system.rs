//! Tier 4 system-level items that are files on the volume, so they work
//! offline and live alike (SPEC §4 Tier 4). Live-only command captures
//! (winget, netsh, printers) come from rebootly-win.
//!
//! - `hosts` → `system/hosts`
//! - scheduled tasks outside `Tasks\Microsoft` → `system/tasks/…`
//! - Wi-Fi profile XMLs → the encrypted bundle (`system/wifi/…`), never plain
//! - all-users VPN phonebook → `system/vpn/rasphone.pbk` (per-user ones come
//!   with the `windows-shell` pack)
//! - machine-wide installed apps → generated `system/apps.json`

use serde::Serialize;

use super::extras::GeneratedFile;
use super::manifest::ManualStep;
use super::plan::{Plan, PlanOptions};
use super::walk::{FolderJob, PathFilter, PlannedFile};
use crate::apps::InstalledApp;
use crate::packs::glob::Glob;
use crate::source::Source;
use crate::winpath::Mapped;

pub const SYSTEM_PRIORITY: u32 = 5;
pub const SYSTEM_LABEL: &str = "System settings";

#[derive(Serialize)]
struct AppsFile<'a> {
    schema: u32,
    note: &'static str,
    machine: &'a [InstalledApp],
}

fn local_dir(src: &dyn Source, win: &str) -> Option<std::path::PathBuf> {
    match src.map_path(win) {
        Mapped::Local { path, exists: true } => Some(path),
        _ => None,
    }
}

fn job(
    win: String,
    local: std::path::PathBuf,
    dest: &str,
    filter: Option<PathFilter>,
) -> FolderJob {
    FolderJob {
        user: String::new(),
        folder: "system".into(),
        src_root: local,
        win_root: win,
        profile_rel: None,
        dest_root: dest.into(),
        priority: SYSTEM_PRIORITY,
        filter,
    }
}

/// Add Tier 4 file items to the plan; walk jobs go to `jobs` (or to the
/// sensitive jobs for Wi-Fi).
pub fn plan_system(
    src: &dyn Source,
    opts: &PlanOptions,
    plan: &mut Plan,
    jobs: &mut Vec<FolderJob>,
) {
    if opts.no_system {
        return;
    }
    plan.labels.insert(SYSTEM_PRIORITY, SYSTEM_LABEL.into());
    let sysroot = src
        .os_info()
        .system_root
        .clone()
        .unwrap_or_else(|| format!("{}:\\Windows", src.os_info().system_drive()));
    let sysroot = sysroot.trim_end_matches('\\');

    let hosts = format!(r"{sysroot}\System32\drivers\etc\hosts");
    if let Some(p) = local_dir(src, &hosts) {
        jobs.push(job(hosts, p, "system/hosts", None));
    }

    let tasks = format!(r"{sysroot}\System32\Tasks");
    if let Some(p) = local_dir(src, &tasks) {
        let filter = Glob::new("Microsoft").ok().map(|g| PathFilter {
            include: Vec::new(),
            exclude: vec![g],
        });
        jobs.push(job(tasks, p, "system/tasks", filter));
    }

    let pd = src
        .profile_dirs()
        .program_data
        .clone()
        .unwrap_or_else(|| format!("{}:\\ProgramData", src.os_info().system_drive()));
    let pd = pd.trim_end_matches('\\');
    let pbk = format!(r"{pd}\Microsoft\Network\Connections\Pbk\rasphone.pbk");
    if let Some(p) = local_dir(src, &pbk) {
        jobs.push(job(pbk, p, "system/vpn/rasphone.pbk", None));
    }
    let wifi = format!(r"{pd}\Microsoft\Wlansvc\Profiles\Interfaces");
    if let Some(p) = local_dir(src, &wifi) {
        let names = wifi_names(&p);
        plan.sensitive_jobs.push(job(wifi, p, "system/wifi", None));
        let how = if opts.sensitive_bundle {
            "Wi-Fi profiles are in sensitive.age. Saved Wi-Fi passwords are protected by this \
             PC's Windows and can't be moved from an offline backup: have them at hand."
        } else {
            "Wi-Fi profiles were not copied (they're only stored encrypted; use --sensitive). \
             Have your Wi-Fi passwords at hand for the new PC."
        };
        let detail = match names_line(&names) {
            Some(line) => format!("{line} {how}"),
            None => how.to_owned(),
        };
        plan.manual_steps.push(ManualStep {
            title: "Wi-Fi networks".into(),
            detail,
        });
    }

    let caps = src.system_captures(opts.sensitive_bundle);
    for w in caps.warnings {
        plan.warnings.push(format!("system: {w}"));
    }
    for c in caps.files {
        let Ok(meta) = std::fs::metadata(&c.file) else {
            continue;
        };
        let f = PlannedFile {
            src_win: c.origin,
            dest: c.dest,
            size: meta.len(),
            mtime: meta.modified().ok(),
            priority: SYSTEM_PRIORITY,
            attributes: None,
            src: c.file,
        };
        match (c.sensitive, opts.sensitive_bundle) {
            (false, _) => plan.files.push(f),
            (true, true) => plan.sensitive.push(f),
            (true, false) => {} // never plain; captures shouldn't produce these
        }
    }

    let apps = AppsFile {
        schema: 1,
        note: "Machine-wide apps from the Uninstall keys (64- and 32-bit). Per-user apps are \
               listed by the app packs; winget export comes with live mode.",
        machine: src.machine_apps(),
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&apps) {
        plan.generated.push(GeneratedFile {
            dest: "system/apps.json".into(),
            bytes,
            source: "Uninstall keys (HKLM SOFTWARE)".into(),
        });
    }
}

/// Names of the saved Wi-Fi networks (`Interfaces\{GUID}\{GUID}.xml`), sorted,
/// without duplicates. The name is plain text in the profile; only the key is
/// protected. Unreadable files are skipped.
pub fn wifi_names(interfaces: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(interfaces)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|i| std::fs::read_dir(i.path()).ok())
        .flatten()
        .flatten()
        .filter(|f| {
            f.path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
        })
        .filter_map(|f| std::fs::read(f.path()).ok())
        .filter_map(|x| crate::restore::system::wifi_name(&x))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// "Saved networks: A, B and C." for the checklist (at most 20 names).
fn names_line(names: &[String]) -> Option<String> {
    const MAX: usize = 20;
    let (shown, rest) = names.split_at(names.len().min(MAX));
    let mut line = match shown {
        [] => return None,
        [one] => one.clone(),
        [init @ .., last] if rest.is_empty() => format!("{} and {last}", init.join(", ")),
        all => all.join(", "),
    };
    if !rest.is_empty() {
        line.push_str(&format!(" and {} more", rest.len()));
    }
    let what = if names.len() == 1 {
        "network"
    } else {
        "networks"
    };
    Some(format!("Saved Wi-Fi {what}: {line}."))
}

#[cfg(test)]
mod names_tests {
    use super::names_line;

    fn v(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lists_read_naturally() {
        assert_eq!(names_line(&[]), None);
        assert_eq!(
            names_line(&v(&["Home"])).unwrap(),
            "Saved Wi-Fi network: Home."
        );
        assert_eq!(
            names_line(&v(&["A", "B", "C"])).unwrap(),
            "Saved Wi-Fi networks: A, B and C."
        );
        let many: Vec<String> = (0..23).map(|i| format!("N{i}")).collect();
        let l = names_line(&many).unwrap();
        assert!(l.ends_with("N19 and 3 more."), "{l}");
    }
}
