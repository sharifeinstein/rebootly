//! Custom folders, `Users\Public` and non-system drive-root folders
//! (SPEC §4 Tier 1, §6 "Add custom folder").

use std::path::Path;

use super::extras::{SeenRoots, claim};
use super::manifest::CustomRecord;
use super::plan::{CUSTOM_PRIORITY, PUBLIC_PRIORITY, Plan, PlanOptions};
use super::walk::FolderJob;
use crate::source::Source;
use crate::winpath::Mapped;

/// Stock folders at the root of a Windows system drive (never offered).
const STOCK_ROOT: &[&str] = &[
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "Users",
    "PerfLogs",
    "Recovery",
    "Config.Msi",
    "MSOCache",
    "Documents and Settings",
    "OneDriveTemp",
    "Intel",
    "AMD",
    "NVIDIA",
    "Drivers",
    "inetpub",
    "ESD",
    "Boot",
    "EFI",
    "swsetup",
    "Windows.old",
    "XboxGames",
    "System Volume Information",
];

/// Non-system folders at the volume root, e.g. `C:\Projects`.
pub fn root_folders(volume_root: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(volume_root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.retain(|n| !n.starts_with('$') && !STOCK_ROOT.iter().any(|s| s.eq_ignore_ascii_case(n)));
    v.sort_by_key(|n| n.to_lowercase());
    v
}

/// Public, chosen root folders, and custom folders (Windows paths).
pub fn plan_custom(
    src: &dyn Source,
    opts: &PlanOptions,
    plan: &mut Plan,
    jobs: &mut Vec<FolderJob>,
    seen: &mut SeenRoots,
) {
    plan.offered_root_folders = root_folders(src.source_root());
    let drive = src.os_info().system_drive();
    let mut wanted: Vec<(String, String, u32)> = Vec::new();
    if opts.public {
        if let Some(p) = &src.profile_dirs().public {
            wanted.push(("Public".into(), p.clone(), PUBLIC_PRIORITY));
        }
    }
    for name in &opts.root_folders {
        wanted.push((name.clone(), format!(r"{drive}:\{name}"), CUSTOM_PRIORITY));
    }
    for (label, path) in &opts.custom {
        wanted.push((label.clone(), path.clone(), CUSTOM_PRIORITY));
    }
    for (label, win, priority) in wanted {
        let label = label.replace(['/', '\\', ':'], "_");
        let mut rec = CustomRecord {
            label: label.clone(),
            source_path: win.clone(),
            dest: None,
            note: None,
        };
        match src.map_path(&win) {
            Mapped::Local { path, exists: true } => {
                if let Some(first) = claim(seen, &path, format!("custom/{label}")) {
                    rec.note = Some(format!("already copied by {first}"));
                } else {
                    let dest = format!("custom/{label}");
                    jobs.push(FolderJob {
                        user: String::new(),
                        folder: format!("custom:{label}"),
                        src_root: path,
                        win_root: win.clone(),
                        profile_rel: None,
                        dest_root: dest.clone(),
                        priority,
                        filter: None,
                    });
                    rec.dest = Some(dest);
                }
            }
            Mapped::Local { .. } => rec.note = Some("not found".into()),
            other => rec.note = Some(format!("not on the scanned volume ({other:?})")),
        }
        if rec.dest.is_none() {
            plan.warnings.push(format!(
                "custom folder {label} ({win}): {}",
                rec.note.clone().unwrap_or_default()
            ));
        }
        plan.custom.push(rec);
    }
}
