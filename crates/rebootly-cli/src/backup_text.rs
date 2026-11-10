//! Text output of `rebootly-cli backup`: detected packs and the pre-flight table.

use rebootly_core::backup;
use rebootly_core::backup::preflight::{Preflight, gib};

pub fn print_preflight(p: &Preflight) {
    eprintln!("Pre-flight:");
    for f in &p.folders {
        eprintln!(
            "  {:<22} {:>7} files  {:>12}",
            f.folder,
            f.files,
            gib(f.bytes)
        );
    }
    eprintln!(
        "  {:<22} {:>7} files  {:>12}",
        "Total",
        p.total_files,
        gib(p.total_bytes)
    );
    if p.resumed_bytes > 0 {
        eprintln!("  already on target:     {}", gib(p.resumed_bytes));
    }
    let free = p.target_free.map_or("unknown".into(), gib);
    eprintln!(
        "  target: {} free, filesystem {}",
        free,
        p.target_fs.as_deref().unwrap_or("unknown")
    );
    for w in &p.warnings {
        eprintln!("  ! {w}");
    }
    for b in &p.blockers {
        eprintln!("  X {b}");
    }
}

pub fn print_detected(plan: &backup::Plan) {
    let mut by_user: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for p in &plan.packs {
        by_user.entry(&p.user).or_default().push(&p.id);
    }
    if !by_user.is_empty() {
        eprintln!("App packs:");
        for (u, ids) in by_user {
            eprintln!("  {u:<20} {}", ids.join(", "));
        }
    }
    if !plan.sensitive.is_empty() {
        eprintln!(
            "Sensitive: {} file(s) (keys, credentials) go into the encrypted sensitive.age",
            plan.sensitive.len()
        );
    } else if !plan.sensitive_jobs.is_empty() {
        eprintln!(
            "Sensitive: {} key/credential location(s) found and left out; add --sensitive to \
             store them encrypted",
            plan.sensitive_jobs.len()
        );
    }
    let unused: Vec<&str> = plan
        .offered_root_folders
        .iter()
        .filter(|n| !plan.custom.iter().any(|c| c.label.eq_ignore_ascii_case(n)))
        .map(String::as_str)
        .collect();
    if !unused.is_empty() {
        eprintln!(
            "Folders at the drive root (add with --root-folder NAME): {}",
            unused.join(", ")
        );
    }
}
