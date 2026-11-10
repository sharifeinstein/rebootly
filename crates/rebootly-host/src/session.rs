//! A backup session once the source is open: accounts, packs, drive-root
//! folders, and planning a [`Selection`]. Shared by the TUI and the GUI
//! (`Send + Sync`, so the GUI can plan on a worker thread).

use std::path::PathBuf;
use std::sync::Arc;

use rebootly_core::Source;
use rebootly_core::backup::profile::Selection;
use rebootly_core::backup::{self, Plan};
use rebootly_core::packs::schema::CompiledPack;

/// One account on the source, for the users screen.
#[derive(Debug, Clone)]
pub struct Account {
    pub sid: String,
    pub username: String,
    pub kind: String,
    pub has_hive: bool,
}

/// What every backup screen shares once the source is open.
pub struct BackupCtx {
    pub src: Box<dyn Source>,
    pub live: bool,
    /// "This PC" or the volume path, for titles.
    pub label: String,
    pub packs: Vec<CompiledPack>,
    pub accounts: Vec<Account>,
    pub offered_roots: Vec<String>,
    /// Target from a saved job profile (skips the target screen).
    pub fixed_target: Option<PathBuf>,
}

impl BackupCtx {
    pub fn new(
        src: Box<dyn Source>,
        label: String,
        packs: Vec<CompiledPack>,
    ) -> anyhow::Result<Self> {
        let mut accounts: Vec<Account> = src
            .profiles()?
            .into_iter()
            .map(|p| Account {
                kind: format!("{:?}", p.kind),
                has_hive: p.hive.is_some(),
                sid: p.sid,
                username: p.username,
            })
            .collect();
        accounts.sort_by_key(|a| a.username.to_lowercase());
        let offered_roots = backup::custom::root_folders(src.source_root());
        Ok(BackupCtx {
            live: src.is_live(),
            src,
            label,
            packs,
            accounts,
            offered_roots,
            fixed_target: None,
        })
    }

    /// Plan for a selection (Full mode adds every drive-root folder).
    pub fn plan(&self, sel: &Selection) -> anyhow::Result<Plan> {
        let mut sel = sel.clone();
        sel.resolve(&self.offered_roots);
        Ok(backup::plan(
            self.src.as_ref(),
            &sel.plan_options(&self.packs),
        )?)
    }
}

pub type Shared = Arc<BackupCtx>;

/// Everything the pre-flight screen shows.
pub struct Prepared {
    pub plan: Plan,
    pub pre: rebootly_core::backup::preflight::Preflight,
    pub summary: rebootly_core::backup::summary::Summary,
    /// An unfinished backup of the same machine on this target.
    pub resumable: Option<PathBuf>,
}

fn nearest_existing(p: &std::path::Path) -> PathBuf {
    p.ancestors()
        .find(|a| a.is_dir())
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

/// Plan, pre-flight checks, the target's write speed and the summary
/// sentence. Slow (walks the folders, writes a 32 MiB test file).
pub fn prepare(
    ctx: &BackupCtx,
    sel: &Selection,
    target: &std::path::Path,
    platform: &dyn crate::Platform,
) -> anyhow::Result<Prepared> {
    use rebootly_core::backup::summary::{speed_probe, summarize};
    let probe = nearest_existing(target);
    let plan = ctx.plan(sel)?;
    let pre = backup::preflight(&plan, &probe, None, platform.target_fs(&probe));
    let summary = summarize(&plan, &pre, speed_probe(&probe));
    let machine = plan.source.machine.clone();
    let resumable = rebootly_core::restore::find::find_backups(&[target.to_path_buf()])
        .into_iter()
        .find(|b| !b.complete && Some(&b.machine) == machine.as_ref())
        .map(|b| b.dir);
    Ok(Prepared {
        plan,
        pre,
        summary,
        resumable,
    })
}
