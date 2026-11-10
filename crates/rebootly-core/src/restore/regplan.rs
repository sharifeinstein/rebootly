//! Phase 6 planning: each pack's `.reg` file is checked against that pack's
//! allowlist (as installed in this Rebootly, not as the backup claims) and
//! the old profile path inside it is rewritten.

use super::map::{MapCtx, Skip};
use super::plan::RegAction;
use super::regcheck;

pub(super) fn registry(ctx: &MapCtx<'_>, skipped: &mut Vec<Skip>) -> Vec<RegAction> {
    let mut out = Vec::new();
    for r in ctx
        .backup
        .manifest
        .packs
        .iter()
        .filter(|r| r.user == ctx.user.username)
    {
        let Some(rel) = r.registry_file.as_deref() else {
            continue;
        };
        let mut skip = |reason: String| {
            skipped.push(Skip {
                what: rel.into(),
                reason,
            })
        };
        let Some(known) = ctx.packs.iter().find(|c| c.pack.id == r.id) else {
            skip(format!(
                "pack {} isn't installed in this Rebootly, so its registry allowlist is unknown",
                r.id
            ));
            continue;
        };
        if let (false, Some(os)) = (known.pack.os.is_empty(), ctx.target.os) {
            if !known.pack.os.contains(&os) {
                skip(format!(
                    "{} doesn't apply to {}",
                    r.name,
                    ctx.target.os_label()
                ));
                continue;
            }
        }
        let Some(path) = ctx.backup.file(rel).and_then(|_| ctx.backup.local(rel)) else {
            skip("missing from the backup".into());
            continue;
        };
        let text = match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| regcheck::decode(&b))
        {
            Ok(t) => t,
            Err(e) => {
                skip(e);
                continue;
            }
        };
        match regcheck::check(&text, &known.pack.registry) {
            Ok(c) => {
                let (text, rewrites) =
                    regcheck::rewrite_profile(&text, &ctx.user.profile_path, &ctx.target.profile);
                out.push(RegAction {
                    pack: r.id.clone(),
                    from: rel.into(),
                    text,
                    top: regcheck::top_keys(&c.keys),
                    keys: c.keys,
                    review: c.review,
                    rewrites,
                });
            }
            Err(e) => skip(format!("refused: {e}")),
        }
    }
    out
}
