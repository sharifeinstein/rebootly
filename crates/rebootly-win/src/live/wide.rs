//! UTF-16 helpers for Win32 calls.
#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

/// NUL-terminated UTF-16 copy of `s`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// NUL-terminated UTF-16 copy of a path (no `\\?\` added).
pub fn wide_path(p: &Path) -> Vec<u16> {
    OsStr::new(p).encode_wide().chain(Some(0)).collect()
}

/// Read a NUL-terminated UTF-16 string.
///
/// # Safety
/// `p` must be null or point to a NUL-terminated UTF-16 string.
pub unsafe fn from_wide_ptr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    // SAFETY: caller guarantees NUL termination.
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    // SAFETY: `n` u16s were just read from `p`.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) })
}
