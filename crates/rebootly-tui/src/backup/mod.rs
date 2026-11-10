//! Backup flow: source → users → Quick/Full/Custom → target → pre-flight →
//! progress → done. In WinPE/Linux the same flow is the Rescue flow.

pub mod custom;
pub mod done;
pub mod mode;
pub mod preflight;
pub mod progress;
pub mod source;
pub mod target;
pub mod users;

use std::path::PathBuf;
use std::rc::Rc;

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

pub type Shared = Rc<BackupCtx>;
