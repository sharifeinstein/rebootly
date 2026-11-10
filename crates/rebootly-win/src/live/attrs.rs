//! Adding file attributes (hidden, system) to restored files.
#![allow(unsafe_code)]

use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_NORMAL, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
};

use super::wide::wide_path;

pub fn add_attributes(p: &Path, attrs: u32) -> std::io::Result<()> {
    let w = wide_path(p);
    // SAFETY: `w` is a NUL-terminated UTF-16 path that outlives both calls.
    let cur = unsafe { GetFileAttributesW(w.as_ptr()) };
    if cur == INVALID_FILE_ATTRIBUTES {
        return Err(std::io::Error::last_os_error());
    }
    let new = (cur | attrs) & !FILE_ATTRIBUTE_NORMAL;
    // SAFETY: as above.
    if unsafe { SetFileAttributesW(w.as_ptr(), new) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
