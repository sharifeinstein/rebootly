//! Where each backed-up file goes on the new PC (SPEC §5 remapping):
//! known folders by KNOWNFOLDERID name, pack data by re-expanding the pack
//! template for the new user, custom folders by their old path with the
//! profile and drive letters remapped.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::conflict::Category;
use super::input::{Backup, Bundle};
use super::merge::MergeKind;
use super::plan::RestoreOptions;
use super::target::{Target, win_rebase, win_within};
use crate::backup::manifest::{FileStatus, PackPathRecord, PackRecord, UserRecord};
use crate::packs::schema::CompiledPack;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAction {
    pub category: Category,
    /// The file to restore (in the backup, or in the decrypted bundle).
    pub src: PathBuf,
    /// Backup-relative path, for reports.
    pub from: String,
    /// Destination on this PC (Windows form).
    pub dst: String,
    pub size: u64,
    pub mtime: Option<String>,
    pub sha256: Option<String>,
    /// Windows attributes recorded at backup (live backups).
    pub attributes: Option<u32>,
    /// Rewrite the old profile path inside (pack `rewrite: true`).
    pub rewrite: bool,
    pub pack: Option<String>,
    pub merge: Option<MergeKind>,
    /// Alternate data streams: (name, stored file).
    pub streams: Vec<(String, PathBuf)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub what: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Mapped {
    pub files: Vec<FileAction>,
    pub skipped: Vec<Skip>,
    /// Backup user → number of files not restored in this run.
    pub other_users: BTreeMap<String, usize>,
}

pub struct MapCtx<'a> {
    pub backup: &'a Backup,
    pub target: &'a Target,
    pub user: &'a UserRecord,
    pub packs: &'a [CompiledPack],
    pub opts: &'a RestoreOptions,
}

/// One file to place, from the plain backup or the bundle.
struct Item<'a> {
    rel: &'a str,
    src: PathBuf,
    size: u64,
    mtime: Option<String>,
    sha256: Option<String>,
    attributes: Option<u32>,
    streams: Vec<(String, PathBuf)>,
}

pub fn map_files(ctx: &MapCtx<'_>, bundle: Option<&Bundle>) -> Mapped {
    let mut out = Mapped::default();
    for f in &ctx.backup.manifest.files {
        if f.status != FileStatus::Ok {
            continue;
        }
        let Some(src) = ctx.backup.local(&f.path) else {
            skip(&mut out, &f.path, "unsafe path in the manifest");
            continue;
        };
        let streams = f
            .streams
            .iter()
            .filter(|s| s.status == FileStatus::Ok)
            .filter_map(|s| Some((s.name.clone(), ctx.backup.local(s.path.as_deref()?)?)))
            .collect();
        let item = Item {
            rel: &f.path,
            src,
            size: f.size,
            mtime: f.mtime.clone(),
            sha256: f.sha256.clone(),
            attributes: f.attributes,
            streams,
        };
        place(ctx, item, &mut out);
    }
    if let Some(b) = bundle {
        for f in &b.index.files {
            if !f.path.starts_with("users/") {
                continue; // system/ items of the bundle: see `system`
            }
            let Some(src) = b.local(&f.path) else {
                continue;
            };
            let item = Item {
                rel: &f.path,
                src,
                size: f.size,
                mtime: None,
                sha256: Some(f.sha256.clone()),
                attributes: None,
                streams: Vec::new(),
            };
            place(ctx, item, &mut out);
        }
    }
    out
}

fn skip(out: &mut Mapped, what: &str, reason: impl Into<String>) {
    out.skipped.push(Skip {
        what: what.into(),
        reason: reason.into(),
    });
}

/// (category, destination, pack, rewrite)
type Placed<'a> = (Category, String, Option<&'a PackRecord>, bool);

fn place(ctx: &MapCtx<'_>, it: Item<'_>, out: &mut Mapped) {
    let parts: Vec<&str> = it.rel.split('/').collect();
    let t = ctx.target;
    let placed: Result<Option<Placed<'_>>, String> = match parts.as_slice() {
        ["users", u, rest @ ..] if *u != ctx.user.username => {
            if !matches!(rest.first(), Some(&"registry")) {
                *out.other_users.entry((*u).to_owned()).or_default() += 1;
            }
            Ok(None)
        }
        ["users", _, "folders", kf, rest @ ..] if !rest.is_empty() => {
            join(&t.folder(kf), rest).map(|d| Some((Category::Folders, d, None, false)))
        }
        ["users", _, "packs", ..] if !ctx.opts.packs => {
            Err("app packs not restored (--no-packs)".into())
        }
        ["users", _, "packs", ..] => pack_dst(ctx, it.rel)
            .map(|(d, rec, path)| Some((Category::Packs, d, Some(rec), path.rewrite))),
        ["users", _, "registry", ..] => Ok(None),
        ["custom", label, rest @ ..] if !rest.is_empty() => custom_dst(ctx, label)
            .and_then(|base| join(&base, rest))
            .map(|d| Some((Category::Custom, d, None, false))),
        ["system", "hosts"] | ["system", "vpn", "rasphone.pbk"] if !ctx.opts.system => {
            Err("system settings not restored (--no-system)".into())
        }
        ["system", "hosts"] => Ok(Some((
            Category::System,
            format!(r"{}\System32\drivers\etc\hosts", t.system_root),
            None,
            false,
        ))),
        ["system", "vpn", "rasphone.pbk"] => Ok(Some((
            Category::System,
            format!(
                r"{}\Microsoft\Network\Connections\Pbk\rasphone.pbk",
                t.program_data
            ),
            None,
            false,
        ))),
        _ => Ok(None), // system/… handled by `system`; streams/ ride along
    };
    match placed {
        Ok(Some((category, dst, pack, rewrite))) => {
            let leaf = dst.rsplit('\\').next().unwrap_or("");
            let merge = if it.rel == "system/hosts" {
                Some(MergeKind::Hosts)
            } else if leaf.eq_ignore_ascii_case("rasphone.pbk") {
                Some(MergeKind::Phonebook)
            } else {
                None
            };
            out.files.push(FileAction {
                category,
                src: it.src,
                from: it.rel.to_owned(),
                dst,
                size: it.size,
                mtime: it.mtime,
                sha256: it.sha256,
                attributes: it.attributes,
                rewrite,
                pack: pack.map(|p| p.id.clone()),
                merge,
                streams: it.streams,
            });
        }
        Ok(None) => {}
        Err(reason) => skip(out, it.rel, reason),
    }
}

/// `base\a\b` from backup components, refusing anything that could step
/// outside `base` or name a stream.
fn join(base: &str, rest: &[&str]) -> Result<String, String> {
    let mut s = base.trim_end_matches('\\').to_owned();
    for c in rest {
        if c.is_empty() || *c == "." || *c == ".." || c.contains([':', '\\']) {
            return Err("unsafe path in the manifest".into());
        }
        s.push('\\');
        s.push_str(c);
    }
    Ok(s)
}

/// Pack file: the record whose `dest` is the longest prefix of `rel`.
fn pack_dst<'a>(
    ctx: &MapCtx<'a>,
    rel: &str,
) -> Result<(String, &'a PackRecord, &'a PackPathRecord), String> {
    let mut best: Option<(&PackRecord, &PackPathRecord, &str)> = None;
    for r in ctx
        .backup
        .manifest
        .packs
        .iter()
        .filter(|r| r.user == ctx.user.username)
    {
        for p in &r.paths {
            let Some(d) = p.dest.as_deref() else { continue };
            let rest = if rel == d {
                Some("")
            } else {
                rel.strip_prefix(d).and_then(|x| x.strip_prefix('/'))
            };
            if let Some(rest) = rest {
                if best.is_none_or(|b| b.1.dest.as_deref().map_or(0, str::len) < d.len()) {
                    best = Some((r, p, rest));
                }
            }
        }
    }
    let (rec, path, rest) = best.ok_or("not listed under any app pack in the manifest")?;
    let known = ctx.packs.iter().find(|c| c.pack.id == rec.id);
    if let (Some(k), Some(os)) = (known, ctx.target.os) {
        if !k.pack.os.is_empty() && !k.pack.os.contains(&os) {
            return Err(format!(
                "{} doesn't apply to {}",
                rec.name,
                ctx.target.os_label()
            ));
        }
    }
    let e = ctx.target.vars.expand(&path.template);
    if !e.unresolved.is_empty() {
        return Err(format!(
            "%{}% isn't known on this PC",
            e.unresolved.join("%, %")
        ));
    }
    let base = e.text;
    let from_pack = known.is_some_and(|k| {
        k.pack
            .paths
            .iter()
            .any(|p| p.src.eq_ignore_ascii_case(&path.template))
    });
    if !from_pack && !win_within(&base, &ctx.target.profile) {
        return Err(format!(
            "{base} is outside the profile and not a path of the installed {} pack",
            rec.id
        ));
    }
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    Ok((join(&base, &parts)?, rec, path))
}

/// New location of a custom folder: old profile → new profile, old Public
/// → new Public, then `--map-drive`. System folders are refused.
fn custom_dst(ctx: &MapCtx<'_>, label: &str) -> Result<String, String> {
    let rec = ctx
        .backup
        .manifest
        .custom
        .iter()
        .find(|c| c.dest.as_deref() == Some(&format!("custom/{label}")))
        .ok_or("not listed under custom folders in the manifest")?;
    let (t, old) = (ctx.target, &ctx.user.profile_path);
    let old_public = match old.trim_end_matches('\\').rsplit_once('\\') {
        Some((users, _)) => format!(r"{users}\Public"),
        None => r"C:\Users\Public".into(),
    };
    let mut p = win_rebase(&rec.source_path, old, &t.profile)
        .or_else(|| win_rebase(&rec.source_path, &old_public, &t.public))
        .unwrap_or_else(|| rec.source_path.clone());
    if p.len() >= 2 && p.as_bytes()[1] == b':' {
        let d = p.as_bytes()[0].to_ascii_uppercase() as char;
        if let Some((_, to)) = ctx.opts.drive_map.iter().find(|(from, _)| *from == d) {
            p.replace_range(..1, &to.to_string());
        }
        let root = format!("{}:\\", &p[..1]);
        if !t.local(&root).exists() {
            return Err(format!(
                "drive {}: isn't on this PC (use --map-drive {}=<letter>)",
                &p[..1],
                &p[..1]
            ));
        }
    }
    let drive = &t.system_root[..2.min(t.system_root.len())];
    let system = [
        t.system_root.clone(),
        format!(r"{drive}\Program Files"),
        format!(r"{drive}\Program Files (x86)"),
    ];
    if system.iter().any(|s| win_within(&p, s)) {
        return Err(format!("{p} is a system folder; not restored"));
    }
    Ok(p)
}
