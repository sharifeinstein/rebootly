//! Volume of a path and its filesystem type (`GetVolumePathNameW`,
//! `GetVolumeInformationW`): the FAT32 4 GiB check on Windows targets, and
//! which volume VSS should snapshot.
#![allow(unsafe_code)]

use std::io;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};

use super::wide::{wide, wide_path};

/// Mount point holding `path`, e.g. `C:\`, `D:\Mount\Data\`, `\\nas\share\`.
pub fn volume_root(path: &Path) -> io::Result<String> {
    let w = wide_path(path);
    let mut buf = vec![0u16; 1024];
    // SAFETY: `w` is NUL-terminated; `buf` has the length we pass.
    let ok = unsafe { GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf16_lossy(&buf[..n]))
}

/// Filesystem name of the volume holding `path` (`NTFS`, `exFAT`, `FAT32`,
/// `ReFS`, …); for a network share, what the server reports.
pub fn filesystem_type(path: &Path) -> Option<String> {
    let root = wide(&volume_root(path).ok()?);
    let mut name = [0u16; 64];
    // SAFETY: `root` is NUL-terminated; optional outputs are null; `name` has
    // the length we pass.
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..n])).filter(|s| !s.is_empty())
}
