//! Core engine for `rebootly` (see docs/SPEC.md).
//!
//! Everything here is platform-neutral and must build and test on Linux.

pub mod backup;
pub mod env;
pub mod expand;
pub mod folders;
pub mod hive;
pub mod inspect;
pub mod install;
pub mod profiles;
pub mod source;
pub mod winpath;

pub use hive::{Hive, HiveError};

pub use source::{OfflineVolume, Source, SourceError};
#[cfg(feature = "fixtures")]
pub mod fixtures;
