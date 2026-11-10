//! Restore flow (SPEC §5, §11): backups found automatically (newest first)
//! → backup user → options → plan preview → restore → result checklist.

pub mod options;
pub mod pick;
pub mod plan;
pub mod run;

use rebootly_core::restore::Backup;
