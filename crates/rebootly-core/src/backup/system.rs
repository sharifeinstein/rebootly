//! Tier 4 system-level items that are files on the volume, so they work
//! offline and live alike (SPEC §4 Tier 4). Live-only command captures
//! (winget, netsh, printers) come from rebootly-win.
//!
//! - `hosts` → `system/hosts`
//! - scheduled tasks outside `Tasks\Microsoft` → `system/tasks/…`
//! - Wi-Fi profile XMLs → the encrypted bundle (`system/wifi/…`), never plain
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
    let wifi = format!(
        r"{}\Microsoft\Wlansvc\Profiles\Interfaces",
        pd.trim_end_matches('\\')
    );
    if let Some(p) = local_dir(src, &wifi) {
        plan.sensitive_jobs.push(job(wifi, p, "system/wifi", None));
        let detail = if opts.sensitive_bundle {
            "Wi-Fi profiles are in sensitive.age. Saved Wi-Fi passwords are protected by this \
             PC's Windows and can't be moved from an offline backup: have them at hand."
        } else {
            "Wi-Fi profiles were not copied (they're only stored encrypted; use --sensitive). \
             Have your Wi-Fi passwords at hand for the new PC."
        };
        plan.manual_steps.push(ManualStep {
            title: "Wi-Fi networks".into(),
            detail: detail.into(),
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
