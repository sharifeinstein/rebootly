//! Was this Windows shut down with Fast Startup or hibernated? (SPEC §7.2)
//!
//! `hiberfil.sys` starts with `HIBR`/`hibr` while a hibernation image is
//! waiting to be resumed (Fast Startup writes one on every shutdown), and
//! with `WAKE`/zeros otherwise; ntfs-3g uses the same test. Rebootly only
//! reads, so a backup is still safe, but the user is told why the disk was
//! opened read-only and that the last session's changes may be missing.

use std::io::Read;
use std::path::Path;

use crate::winpath::resolve_ci;

/// True when the volume at `root` holds a hibernation image to resume.
pub fn hibernated(root: &Path) -> bool {
    let (path, exists) = resolve_ci(root, &["hiberfil.sys".to_owned()]);
    if !exists {
        return false;
    }
    let mut head = [0u8; 4];
    match std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)) {
        Ok(()) => head.eq_ignore_ascii_case(b"HIBR"),
        Err(_) => false,
    }
}

/// What to tell the user about a hibernated volume.
pub const HIBERNATED_NOTE: &str = "Windows on this disk wasn't fully shut down (Fast Startup or \
     hibernation). Rebootly only reads, so this is safe, but changes from the last session may \
     be missing. For a complete copy, start that Windows, hold Shift while choosing Shut down, and try \
     again.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_header_case_insensitively() {
        let d = tempfile::tempdir().unwrap();
        assert!(!hibernated(d.path()));
        std::fs::write(d.path().join("HIBERFIL.SYS"), b"hibr\0\0\0\0").unwrap();
        assert!(hibernated(d.path()));
        std::fs::write(d.path().join("HIBERFIL.SYS"), b"WAKE\0\0\0\0").unwrap();
        assert!(!hibernated(d.path()));
        std::fs::write(d.path().join("HIBERFIL.SYS"), b"HI").unwrap();
        assert!(!hibernated(d.path()));
    }
}
