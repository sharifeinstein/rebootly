//! Linux recovery backend for `migrate` (SPEC §7.1, §7.2).
//!
//! Planned for M7: volume discovery, BitLocker unlock via cryptsetup/dislocker,
//! read-only NTFS mounting. Never writes to the source disk.
