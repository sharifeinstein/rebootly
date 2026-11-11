//! Undoing read-only mounts and unlocks left by an earlier run.

use std::path::Path;

use crate::cmd::{Runner, last_line};
use crate::mount::BASE;

/// Undo what an earlier run left open (`rebootly-cli open`, or a run that
/// was killed): unmount everything under [`BASE`], deepest first, then close
/// `rebootly-*` mappings and remove the empty folders.
pub fn close_all(run: &dyn Runner) -> Vec<String> {
    let info = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let mapper: Vec<String> = std::fs::read_dir("/dev/mapper")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    close_all_in(run, Path::new(BASE), &info, &mapper)
}

pub fn close_all_in(run: &dyn Runner, base: &Path, mountinfo: &str, mapper: &[String]) -> Vec<String> {
    let mut errs = Vec::new();
    let mut mounts: Vec<String> = mountinfo
        .lines()
        .filter_map(|l| l.split(' ').nth(4))
        .map(crate::discover::unescape)
        .filter(|m| Path::new(m).starts_with(base))
        .collect();
    mounts.sort_by_key(|m| std::cmp::Reverse(m.len()));
    mounts.dedup();
    for m in &mounts {
        match run.run("umount", &[m], None) {
            Ok(o) if o.ok() => {}
            Ok(o) => errs.push(format!("umount {m}: {}", last_line(&o.stderr))),
            Err(e) => errs.push(format!("umount {m}: {e}")),
        }
    }
    for n in mapper.iter().filter(|n| n.starts_with("rebootly-")) {
        match run.run("cryptsetup", &["close", n], None) {
            Ok(o) if o.ok() => {}
            Ok(o) => errs.push(format!("cryptsetup close {n}: {}", last_line(&o.stderr))),
            Err(e) => errs.push(format!("cryptsetup close {n}: {e}")),
        }
    }
    if let Ok(rd) = std::fs::read_dir(base) {
        for e in rd.flatten() {
            let _ = std::fs::remove_dir(e.path()); // only empty ones go
        }
    }
    errs
}
