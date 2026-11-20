//! Checks after the fact (SPEC §11): `verify` re-hashes a backup against its
//! manifest; `compare` checks the new PC against the backup after a restore.
//! Both only read.

pub mod compare;
pub mod text;
pub mod verify;

#[cfg(test)]
mod tests;

pub use compare::{CompareReport, State, compare};
pub use verify::{BundleCheck, VerifyReport, verify};

/// Files and bytes done so far, sent after each file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub files: usize,
    pub total_files: usize,
    pub bytes: u64,
    pub total_bytes: u64,
}

impl Progress {
    pub(crate) fn step(&mut self, bytes: u64, cb: &mut dyn FnMut(&Progress)) {
        self.files += 1;
        self.bytes += bytes;
        cb(self);
    }
}
