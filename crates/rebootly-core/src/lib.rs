//! Core engine for `rebootly` (see docs/SPEC.md).
//!
//! Everything here is platform-neutral and must build and test on Linux.

pub mod apps;
pub mod backup;
pub mod env;
pub mod expand;
pub mod folders;
pub mod hive;
pub mod inspect;
pub mod install;
pub mod packs;
pub mod profiles;
pub mod regfile;
pub mod restore;
pub mod source;
pub mod winpath;

pub use hive::{Hive, HiveError};

pub use source::{
    Capture, Captures, HiveRef, LOCKED_HINT, LoadedHive, OfflineAccess, OfflineVolume, Source,
    SourceError, StreamInfo, VolumeAccess,
};
#[cfg(feature = "fixtures")]
pub mod fixtures;
