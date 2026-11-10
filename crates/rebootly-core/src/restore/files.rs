//! Restoring one file: identical → leave it; merge (hosts, phonebooks);
//! otherwise the conflict policy. The old file is saved and journalled
//! before it is replaced; new content goes to `<name>.rbpart`, is checked
//! against the backup's SHA-256, then renamed into place.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::conflict::{Policy, keep_both_name};
use super::map::FileAction;
use super::rewrite::{MAX_REWRITE, rewrite_bytes};
use super::run::{Exec, Phase, Status};
use super::undo::{UndoEntry, with_suffix};
use crate::backup::copy::{hash_file, hex};
use crate::backup::time::parse_rfc3339;

/// Hidden and system: what makes `desktop.ini` and similar files behave.
/// Read-only isn't restored (it would block a later restore or undo).
const KEEP_ATTRS: u32 = 0x2 | 0x4;

enum How {
    Create(String),
    Replace,
}

pub(super) fn restore_file(ex: &mut Exec<'_>, phase: Phase, f: &FileAction) {
    let (status, detail) = match decide_and_write(ex, f) {
        Ok(r) => r,
        Err(e) => (Status::Failed, e),
    };
    if status == Status::Done && f.pack.as_deref() == Some("windows-shell") {
        ex.shell_changed = true;
    }
    ex.push(phase, &f.dst, status, detail);
}

fn decide_and_write(ex: &mut Exec<'_>, f: &FileAction) -> Result<(Status, String), String> {
    let t = &ex.plan.target;
    let local = t.local(&f.dst);
    let content = content(ex, f)?;
    let expected = match &content {
        Some(b) => Some(hex(&Sha256::digest(b))),
        None => f.sha256.clone(),
    };
    let size = content.as_ref().map_or(f.size, |b| b.len() as u64);
    let meta = std::fs::symlink_metadata(&local).ok();
    if let Some(m) = &meta {
        if m.file_type().is_symlink() {
            return Ok((Status::Skipped, "a link is in the way; not followed".into()));
        }
        if m.is_dir() {
            return Ok((
                Status::Skipped,
                "a folder with this name is already there".into(),
            ));
        }
        if m.len() == size
            && expected
                .as_deref()
                .is_some_and(|e| hash_file(&local).is_ok_and(|h| h == e))
        {
            return Ok((Status::Skipped, "already there".into()));
        }
    }
    if let Some(why) = super::fit::unfit(f, meta.is_some(), content.is_some(), &ex.plan.user, t) {
        return Ok((Status::Skipped, why));
    }
    if meta.is_some() && leaf(&f.dst).eq_ignore_ascii_case("desktop.ini") {
        // Windows writes its own; two of them would only add clutter.
        return Ok((Status::Skipped, "the folder has its own desktop.ini".into()));
    }
    if let (Some(kind), Some(_)) = (f.merge, &meta) {
        let cur = std::fs::read(&local).map_err(|e| e.to_string())?;
        let old = match &content {
            Some(b) => b.clone(),
            None => std::fs::read(&f.src).map_err(|e| e.to_string())?,
        };
        let note = ex
            .plan
            .source_machine
            .clone()
            .unwrap_or_else(|| ex.plan.job_id.clone());
        return match kind.merge(&cur, &old, &note) {
            None => Ok((
                Status::Skipped,
                "already has every entry from the old PC".into(),
            )),
            Some((_, n)) if ex.dry_run => Ok((Status::Would, format!("merge: add {n} entries"))),
            Some((bytes, n)) => {
                write(ex, f, &local, &f.dst, Some(&bytes), How::Replace, true)?;
                Ok((
                    Status::Done,
                    format!("merged: added {n} entries (old file kept for undo)"),
                ))
            }
        };
    }
    let how = match meta {
        None => How::Create(f.dst.clone()),
        Some(m) => {
            let mut policy = ex.policies.get(f.category);
            if policy == Policy::Ask {
                if ex.dry_run {
                    return Ok((Status::Would, "ask (exists and differs)".into()));
                }
                let (p, all) = ex.ui.conflict(f, &local);
                if all {
                    ex.policies.set(f.category, p);
                }
                policy = p;
            }
            match policy {
                Policy::Skip | Policy::Ask => {
                    return Ok((
                        Status::Skipped,
                        "a different file is there; kept it (policy: skip)".into(),
                    ));
                }
                Policy::OverwriteIfNewer => {
                    let theirs = f.mtime.as_deref().and_then(parse_rfc3339);
                    match (theirs, m.modified().ok()) {
                        (Some(b), Some(here)) if b > here => How::Replace,
                        _ => {
                            return Ok((
                                Status::Skipped,
                                "the file on this PC is newer (policy: overwrite-if-newer)".into(),
                            ));
                        }
                    }
                }
                Policy::Overwrite => How::Replace,
                Policy::KeepBoth => match free_name(ex, &f.dst, expected.as_deref()) {
                    Some(n) => How::Create(n),
                    None => {
                        return Ok((Status::Skipped, "already there as a (migrated) copy".into()));
                    }
                },
            }
        }
    };
    let (dst, replace) = match how {
        How::Replace => (f.dst.clone(), true),
        How::Create(d) => (d, false),
    };
    let rewritten = if content.is_some() {
        " (profile paths rewritten)"
    } else {
        ""
    };
    let detail = match (replace, dst == f.dst) {
        (true, _) => format!("replaced{rewritten}; old file kept for undo"),
        (false, true) => format!("created{rewritten}"),
        (false, false) => format!("kept both: written as {}{rewritten}", leaf(&dst)),
    };
    if ex.dry_run {
        return Ok((Status::Would, detail));
    }
    let local = t.local(&dst);
    write(
        ex,
        f,
        &local,
        &dst,
        content.as_deref(),
        if replace {
            How::Replace
        } else {
            How::Create(dst.clone())
        },
        false,
    )?;
    let mut detail = detail;
    let attrs = f
        .attributes
        .map(|a| a & KEEP_ATTRS)
        .filter(|a| *a != 0)
        .or_else(|| {
            leaf(&dst)
                .eq_ignore_ascii_case("desktop.ini")
                .then_some(KEEP_ATTRS)
        });
    if let Some(a) = attrs {
        if let Err(e) = ex.host.set_attributes(&local, a) {
            detail.push_str(&format!("; attributes not set: {e}"));
        }
    }
    for (name, src) in &f.streams {
        if let Err(e) = ex.host.write_stream(&local, name, src) {
            detail.push_str(&format!("; stream {name} not written: {e}"));
        }
    }
    Ok((Status::Done, detail))
}

fn leaf(win: &str) -> &str {
    win.rsplit('\\').next().unwrap_or(win)
}

/// Rewritten bytes for `rewrite: true` files (verified against the backup
/// hash first); `None` to copy the file as it is.
fn content(ex: &Exec<'_>, f: &FileAction) -> Result<Option<Vec<u8>>, String> {
    if !f.rewrite || f.size > MAX_REWRITE {
        return Ok(None);
    }
    let bytes = std::fs::read(&f.src).map_err(|e| format!("can't read the backup copy: {e}"))?;
    if f.sha256
        .as_deref()
        .is_some_and(|s| s != hex(&Sha256::digest(&bytes)))
    {
        return Err("the backup copy is damaged (SHA-256 mismatch)".into());
    }
    let (old, new) = (&ex.plan.user.profile_path, &ex.plan.target.profile);
    Ok(rewrite_bytes(&bytes, old, new).map(|r| r.0))
}

/// First `name (migrated N).ext` that is free; `None` if one already holds
/// the same content.
fn free_name(ex: &Exec<'_>, dst: &str, sha: Option<&str>) -> Option<String> {
    for n in 1..1000 {
        let cand = keep_both_name(dst, n);
        let l = ex.plan.target.local(&cand);
        if !l.exists() {
            return Some(cand);
        }
        if sha.is_some_and(|s| hash_file(&l).is_ok_and(|h| h == s)) {
            return None;
        }
    }
    None
}

/// Journal, then write via `.rbpart` + rename.
fn write(
    ex: &mut Exec<'_>,
    f: &FileAction,
    local: &Path,
    dst: &str,
    bytes: Option<&[u8]>,
    how: How,
    merged: bool,
) -> Result<(), String> {
    let err = |e: std::io::Error| e.to_string();
    let parent = local.parent().ok_or("no parent folder")?;
    let mut missing: Vec<PathBuf> = parent
        .ancestors()
        .take_while(|a| !a.exists())
        .map(Path::to_path_buf)
        .collect();
    missing.reverse();
    let expected = match bytes {
        Some(b) => hex(&Sha256::digest(b)),
        None => f.sha256.clone().unwrap_or_default(),
    };
    {
        let t = ex.plan.target.clone();
        let j = ex.journal().map_err(err)?;
        for m in &missing {
            j.append(&UndoEntry::Mkdir {
                path: win_of(&t, m, dst, local),
            })
            .map_err(err)?;
        }
        match how {
            How::Replace => {
                let saved = j.save(local).map_err(err)?;
                j.append(&UndoEntry::Overwrite {
                    path: dst.into(),
                    saved,
                    sha256: expected.clone(),
                })
                .map_err(err)?;
            }
            How::Create(_) => j
                .append(&UndoEntry::Create {
                    path: dst.into(),
                    sha256: expected.clone(),
                })
                .map_err(err)?,
        }
    }
    std::fs::create_dir_all(parent).map_err(err)?;
    let tmp = with_suffix(local, crate::backup::copy::PARTIAL_SUFFIX);
    let sha = (|| -> std::io::Result<String> {
        let mut out = File::create(&tmp)?;
        let mut h = Sha256::new();
        match bytes {
            Some(b) => {
                h.update(b);
                out.write_all(b)?;
            }
            None => {
                let mut src = File::open(&f.src)?;
                let mut buf = vec![0u8; 1 << 20];
                loop {
                    let n = src.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                    out.write_all(&buf[..n])?;
                }
            }
        }
        if !merged {
            if let Some(t) = f.mtime.as_deref().and_then(parse_rfc3339) {
                out.set_modified(t)?;
            }
        }
        out.sync_all()?;
        Ok(hex(&h.finalize()))
    })();
    let sha = match sha {
        Ok(s) => s,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("write failed: {e}"));
        }
    };
    if !expected.is_empty() && sha != expected {
        let _ = std::fs::remove_file(&tmp);
        return Err("the backup copy is damaged (SHA-256 mismatch)".into());
    }
    if let Ok(m) = std::fs::metadata(local) {
        let mut p = m.permissions();
        if p.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(false);
            let _ = std::fs::set_permissions(local, p);
        }
    }
    if let Err(e) = std::fs::rename(&tmp, local) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("can't put the file in place: {e}"));
    }
    ex.written.push((local.to_path_buf(), dst.to_owned(), sha));
    Ok(())
}

/// Windows path of a folder `m` that is an ancestor of `local` (= `dst`).
fn win_of(_t: &super::target::Target, m: &Path, dst: &str, local: &Path) -> String {
    let depth = local.ancestors().position(|a| a == m).unwrap_or(0);
    let mut w = dst;
    for _ in 0..depth {
        w = w.rsplit_once('\\').map_or(w, |x| x.0);
    }
    w.to_owned()
}
