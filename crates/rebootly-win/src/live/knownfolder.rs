//! `SHGetKnownFolderPath` for the current user (SPEC §3.4 live cross-check).
#![allow(unsafe_code)]

use std::ptr::null_mut;

use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath};
use windows_sys::core::GUID;

use super::wide::from_wide_ptr;

/// Parse `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}` (braces optional).
pub fn parse_guid(s: &str) -> Option<GUID> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let groups: Vec<&str> = s.trim_matches(['{', '}']).split('-').collect();
    if hex.len() != 32 || groups.len() != 5 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok().map(GUID::from_u128)
}

/// Path of a known folder for the current user, without creating it.
pub fn known_folder_path(guid: &str) -> Option<String> {
    let id = parse_guid(guid)?;
    let mut p = null_mut();
    // SAFETY: valid GUID pointer and out-pointer; null token = current user.
    let hr = unsafe { SHGetKnownFolderPath(&id, KF_FLAG_DONT_VERIFY as _, null_mut(), &mut p) };
    // SAFETY: on success `p` is a NUL-terminated string; it must be freed either way.
    let out = (hr >= 0).then(|| unsafe { from_wide_ptr(p) });
    unsafe { CoTaskMemFree(p.cast()) };
    out
}
