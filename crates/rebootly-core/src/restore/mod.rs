//! Restore engine (SPEC §5): backup folder → the new PC.
//!
//! Phases, in order: (1) plan + pre-flight → (2) `winget import` → (3) the
//! user installs other apps → (4) files → (5) app packs (app closed) →
//! (6) allowlisted `.reg` import → (7) Wi-Fi, power plan, certificates,
//! tasks, hosts, VPN → (8) Explorer restart → (9) verify → (10) manual steps.
//!
//! Everything platform-specific (commands, streams, running processes) goes
//! through [`RestoreHost`]; this module only reads the backup and writes
//! files, so it builds and tests on Linux. A dry run takes the same code
//! path and stops before each change.

pub mod conflict;
mod files;
pub mod find;
pub(crate) mod fit;
pub mod host;
pub mod input;
pub mod map;
pub mod merge;
mod notes;
mod phases;
pub mod plan;
pub mod regcheck;
mod regplan;
pub mod report;
pub mod rewrite;
pub mod run;
pub mod system;
pub mod target;
pub mod undo;

pub use conflict::{Policies, Policy};
pub use host::{RestoreHost, SimHost, SysOk, SysOp};
pub use input::{Backup, Bundle};
pub use plan::{AppsMode, RestoreOptions, RestorePlan, plan};
pub use run::{Event, Interact, RestoreResult, execute};
pub use target::{PathStyle, Target};

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A file that couldn't be read, and why (not a source: `{:#}` would
    /// print the OS error twice).
    #[error("can't read {0}: {1}")]
    Read(String, std::io::Error),
    #[error("{0} has no manifest.json (is it a Rebootly backup folder?)")]
    NoManifest(String),
    #[error("manifest.json can't be read: {0}")]
    BadManifest(String),
    #[error("this backup was made by a newer Rebootly (schema {0}); update Rebootly first")]
    NewerSchema(u32),
    #[error("{0}")]
    User(String),
    #[error("sensitive.age: {0}")]
    Bundle(String),
}

#[cfg(test)]
pub(crate) mod tests;
