//! Find backups on the stick or NAS (SPEC §11: "on restore, backups are
//! found automatically and the newest is offered").

use std::path::{Path, PathBuf};

use super::input::Backup;

/// How deep below each root to look for a `manifest.json`
/// (`E:\` → `E:\rb\OLD-PC_20261110-131615` is depth 2).
pub const MAX_DEPTH: usize = 3;

#[derive(Debug, Clone)]
pub struct FoundBackup {
    pub dir: PathBuf,
    pub job_id: String,
    pub machine: String,
    /// RFC 3339; `None` for an unfinished job.
    pub finished: Option<String>,
    pub complete: bool,
    pub users: Vec<String>,
    pub files: usize,
    pub bytes: u64,
}

/// Backups under `roots`, newest first (finished before unfinished).
/// Folders that can't be read are skipped; links are not followed.
pub fn find_backups(roots: &[PathBuf]) -> Vec<FoundBackup> {
    let mut out = Vec::new();
    for r in roots {
        walk(r, 0, &mut out);
    }
    out.sort_by(|a, b| {
        (b.complete, &b.finished, &b.job_id).cmp(&(a.complete, &a.finished, &a.job_id))
    });
    out.dedup_by(|a, b| a.dir == b.dir);
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<FoundBackup>) {
    if dir.join("manifest.json").is_file() {
        if let Ok(b) = Backup::open(dir) {
            out.push(describe(b));
        }
        return; // never look inside a backup
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| !skip(p))
        .collect();
    subdirs.sort();
    for d in subdirs {
        walk(&d, depth + 1, out);
    }
}

/// System folders on a drive that never hold backups (and are slow).
fn skip(p: &Path) -> bool {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name.starts_with('$')
        || matches!(
            name.as_str(),
            "windows"
                | "program files"
                | "program files (x86)"
                | "programdata"
                | "users"
                | "system volume information"
                | "recovery"
                | "proc"
                | "sys"
                | "dev"
        )
}

fn describe(b: Backup) -> FoundBackup {
    let m = &b.manifest;
    let ok = m
        .files
        .iter()
        .filter(|f| f.status == crate::backup::manifest::FileStatus::Ok);
    let (files, bytes) = ok.fold((0, 0), |a, f| (a.0 + 1, a.1 + f.size));
    FoundBackup {
        job_id: m.job_id.clone(),
        machine: m.source.machine.clone().unwrap_or_else(|| "?".into()),
        finished: m.finished.clone(),
        complete: m.complete,
        users: m.users.iter().map(|u| u.username.clone()).collect(),
        files,
        bytes,
        dir: b.dir,
    }
}

/// Files and bytes backed up for one user (`users/<name>/…`), e.g. for
/// "Alice · 12.3 GiB · 4,567 files" when choosing whose files to restore.
pub fn user_size(b: &Backup, username: &str) -> (usize, u64) {
    let prefix = format!("users/{username}/");
    b.manifest
        .files
        .iter()
        .filter(|f| {
            f.status == crate::backup::manifest::FileStatus::Ok && f.path.starts_with(&prefix)
        })
        .fold((0, 0), |a, f| (a.0 + 1, a.1 + f.size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(dir: &Path, job: &str, finished: Option<&str>) {
        std::fs::create_dir_all(dir).unwrap();
        let fin = finished.map_or("null".to_string(), |f| format!("\"{f}\""));
        std::fs::write(
            dir.join("manifest.json"),
            format!(
                r#"{{"schema_version":1,"job_id":"{job}","finished":{fin},"complete":{},
                "source":{{"machine":"OLD-PC","os_name":"Windows 11","environment":"live-windows"}},
                "users":[],"files":[]}}"#,
                finished.is_some()
            ),
        )
        .unwrap();
    }

    #[test]
    fn newest_complete_first() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        fake(&r.join("rb/OLD-PC_1"), "j1", Some("2026-11-01T10:00:00Z"));
        fake(&r.join("rb/OLD-PC_2"), "j2", Some("2026-11-09T10:00:00Z"));
        fake(&r.join("OLD-PC_3"), "j3", None);
        fake(
            &r.join("a/b/c/too-deep"),
            "j4",
            Some("2026-11-10T10:00:00Z"),
        );
        std::fs::create_dir_all(r.join("Windows/rb")).unwrap();
        let f = find_backups(&[r.to_path_buf()]);
        let ids: Vec<&str> = f.iter().map(|b| b.job_id.as_str()).collect();
        assert_eq!(ids, ["j2", "j1", "j3"]);
        assert_eq!(f[0].machine, "OLD-PC");
    }
}
