//! Saved passwords inside pack files and registry keys (pack `redact:` and
//! `secret_values:`, SPEC §6). The plain backup gets the files and the `.reg`
//! without them; with a passphrase, the untouched files and the full `.reg`
//! also go into the encrypted bundle, at the same paths.

use std::path::{Path, PathBuf};

use super::extras::GeneratedFile;
use super::manifest::{PackPathStatus, PackSecrets};
use super::plan::{PACK_PRIORITY, Plan, PlanOptions, SecretText};
use super::walk::{FolderJob, PathFilter, rel_components};
use crate::hive::Hive;
use crate::packs::schema::{Pack, PackPath, hkcu_subpath};

/// Files larger than this aren't read to strip passwords; they are treated
/// as holding them (bundle only).
const MAX_REDACT: u64 = 16 << 20;

/// Where one plain pack path is planned.
pub(super) struct PlainPath<'a> {
    pub uname: &'a str,
    pub pack_id: &'a str,
    pub local: &'a Path,
    pub win: &'a str,
    pub dest: &'a str,
    pub profile_dir: &'a Path,
    /// The path's include/exclude filter: files it leaves out are ignored.
    pub filter: Option<&'a PathFilter>,
}

/// For each `redact` file holding a password: keep it out of the plain walk
/// (it becomes a sensitive job), write the stripped copy as a generated file.
pub(super) fn redact_path(pp: &PackPath, at: &PlainPath<'_>, plan: &mut Plan, s: &mut PackSecrets) {
    for r in &pp.redact {
        let targets: Vec<(PathBuf, String, String)> = match r.file.as_deref() {
            None => vec![(
                at.local.to_path_buf(),
                at.win.to_owned(),
                at.dest.to_owned(),
            )],
            Some(rel) => matching_files(at.local, &rel.replace('\\', "/"))
                .into_iter()
                .filter(|parts| !at.filter.is_some_and(|f| f.skip_file(&parts.join("/"))))
                .map(|parts| {
                    (
                        parts.iter().fold(at.local.to_path_buf(), |p, n| p.join(n)),
                        format!(r"{}\{}", at.win.trim_end_matches('\\'), parts.join("\\")),
                        format!("{}/{}", at.dest, parts.join("/")),
                    )
                })
                .collect(),
        };
        for (local, win, dest) in targets {
            redact_file(r, at, &local, &win, dest, plan, s);
        }
    }
}

/// On-disk names of the files under `root` matching `rel` (`/`-separated;
/// a segment with `*` or `?` matches names in that folder, case-insensitive;
/// others are looked up case-insensitively). Sorted, links not followed.
fn matching_files(root: &Path, rel: &str) -> Vec<Vec<String>> {
    let mut found: Vec<(PathBuf, Vec<String>)> = vec![(root.to_path_buf(), Vec::new())];
    for seg in rel.split('/').filter(|p| !p.is_empty()) {
        let mut next = Vec::new();
        for (dir, parts) in found {
            if seg.contains(['*', '?']) {
                let Ok(glob) = crate::packs::glob::Glob::new(seg) else {
                    continue;
                };
                let Ok(rd) = std::fs::read_dir(&dir) else {
                    continue;
                };
                let mut names: Vec<String> = rd
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| !t.is_symlink()))
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| glob.matches_name(n))
                    .collect();
                names.sort();
                for n in names {
                    let mut p = parts.clone();
                    p.push(n.clone());
                    next.push((dir.join(n), p));
                }
            } else {
                let (local, _) = crate::winpath::resolve_ci(&dir, &[seg.to_owned()]);
                let name = local
                    .file_name()
                    .map_or_else(|| seg.to_owned(), |n| n.to_string_lossy().into_owned());
                let mut p = parts;
                p.push(name);
                next.push((local, p));
            }
        }
        found = next;
    }
    found.into_iter().map(|(_, p)| p).collect()
}

#[allow(clippy::too_many_arguments)]
fn redact_file(
    r: &crate::packs::redact::Redact,
    at: &PlainPath<'_>,
    local: &Path,
    win: &str,
    dest: String,
    plan: &mut Plan,
    s: &mut PackSecrets,
) {
    let Ok(meta) = std::fs::metadata(local) else {
        return;
    };
    if !meta.is_file() {
        return;
    }
    let read = (meta.len() <= MAX_REDACT)
        .then(|| std::fs::read(local).ok())
        .flatten();
    let stripped = match &read {
        Some(data) => match r.apply(data) {
            (_, 0) => return, // no saved passwords: copied as usual
            (bytes, _) => Some(bytes),
        },
        None => {
            plan.warnings.push(format!(
                "{}: couldn't read {win} to remove its saved passwords, so it is only \
                 copied into the encrypted bundle",
                at.uname
            ));
            None
        }
    };
    plan.sensitive_jobs.push(FolderJob {
        user: at.uname.into(),
        folder: format!("pack:{}", at.pack_id),
        src_root: local.to_path_buf(),
        win_root: win.to_owned(),
        profile_rel: rel_components(local, at.profile_dir),
        dest_root: dest.clone(),
        priority: PACK_PRIORITY,
        filter: None,
    });
    if let Some(bytes) = stripped {
        plan.generated.push(GeneratedFile {
            dest: dest.clone(),
            bytes,
            source: format!("{win} (saved passwords removed)"),
        });
    }
    s.files.push(dest);
}

/// The pack's registry keys as `.reg` sections without `secret_values`;
/// with a passphrase, the full export goes into the bundle at `dest`.
pub(super) fn registry(
    pack: &Pack,
    h: &Hive,
    dest: &str,
    source: &str,
    opts: &PlanOptions,
    plan: &mut Plan,
    s: &mut PackSecrets,
) -> Vec<String> {
    let secret = |n: &str| {
        pack.secret_values
            .iter()
            .any(|v| v.trim().eq_ignore_ascii_case(n))
    };
    let mut plain = Vec::new();
    let mut full = Vec::new();
    for sub in pack.registry.iter().filter_map(|k| hkcu_subpath(k)) {
        let Ok(Some((section, dropped))) =
            crate::regfile::export_without(h, sub, "HKEY_CURRENT_USER", &secret)
        else {
            continue;
        };
        plain.push(section);
        s.registry_values += dropped;
        if dropped > 0 && opts.sensitive_bundle {
            if let Ok(Some(all)) = crate::regfile::export(h, sub, "HKEY_CURRENT_USER") {
                full.push(all);
            }
        }
    }
    if !full.is_empty() {
        plan.sensitive_text.push(SecretText {
            dest: dest.into(),
            source: format!("{source}, with saved passwords"),
            text: crate::regfile::to_reg_text(&full).into(),
        });
    }
    plain
}

/// The pack's secrets record, or `None` if nothing held a password.
pub(super) fn finish(mut s: PackSecrets, opts: &PlanOptions) -> Option<PackSecrets> {
    if s.files.is_empty() && s.registry_values == 0 {
        return None;
    }
    s.status = if opts.sensitive_bundle {
        PackPathStatus::Encrypted
    } else {
        PackPathStatus::SensitiveNotCopied
    };
    Some(s)
}
