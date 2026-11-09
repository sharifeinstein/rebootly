//! Decide whether a pack applies to a user (SPEC §6 `detect`, `os`).

use super::schema::{CompiledPack, Cond, OsTag, hkcu_subpath};
use crate::apps::InstalledApp;
use crate::expand::VarSet;
use crate::hive::Hive;
use crate::winpath::Mapped;

pub struct DetectCtx<'a> {
    /// User variables, including `KF:<Folder>` entries.
    pub vars: &'a VarSet,
    pub map: &'a dyn Fn(&str) -> Mapped,
    /// Machine apps plus this user's apps.
    pub apps: &'a [InstalledApp],
    pub ntuser: Option<&'a Hive>,
    pub os: Option<OsTag>,
}

/// Windows 11 from build 22000 on.
pub fn os_tag(build: Option<u32>) -> Option<OsTag> {
    build.map(|b| {
        if b >= 22000 {
            OsTag::Win11
        } else {
            OsTag::Win10
        }
    })
}

fn cond_true(c: &Cond, ctx: &DetectCtx<'_>) -> bool {
    match c {
        Cond::Always(b) => *b,
        Cond::Path(t) => {
            let e = ctx.vars.expand(t);
            e.unresolved.is_empty() && (ctx.map)(&e.text).exists()
        }
        // Already validated at load time, so compiling here cannot fail.
        Cond::UninstallName(pattern) => super::glob::Glob::new(pattern)
            .is_ok_and(|g| ctx.apps.iter().any(|a| g.matches_name(&a.name))),
        Cond::Registry(k) => match (hkcu_subpath(k), ctx.ntuser) {
            (Some(sub), Some(h)) => h.open_key(sub).ok().flatten().is_some(),
            _ => false,
        },
    }
}

/// `Ok(reason)` if detected, `Err(reason)` if not.
pub fn detect(pack: &CompiledPack, ctx: &DetectCtx<'_>) -> Result<String, String> {
    let p = &pack.pack;
    if let Some(os) = ctx.os {
        if !p.os.is_empty() && !p.os.contains(&os) {
            return Err(format!("not for {os:?}"));
        }
    }
    let any_hit = p.detect.any.iter().find(|c| cond_true(c, ctx));
    let any_ok = p.detect.any.is_empty() || any_hit.is_some();
    let all_ok = p.detect.all.iter().all(|c| cond_true(c, ctx));
    if any_ok && all_ok {
        Ok(match any_hit.or(p.detect.all.first()) {
            Some(Cond::Path(t)) => format!("found {t}"),
            Some(Cond::UninstallName(n)) => format!("installed: {n}"),
            Some(Cond::Registry(k)) => format!("registry key {k}"),
            Some(Cond::Always(_)) | None => "always".into(),
        })
    } else {
        Err("not detected".into())
    }
}
