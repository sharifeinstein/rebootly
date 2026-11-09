//! Alternate data streams of a file (SPEC §3.5): listed from a handle opened
//! with backup semantics (`GetFileInformationByHandleEx(FileStreamInfo)`),
//! so other users' files work too, and opened as `path:name`.
#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use rebootly_core::StreamInfo;
use windows_sys::Win32::Storage::FileSystem::{FileStreamInfo, GetFileInformationByHandleEx};

use super::access::open_backup;
use crate::parse::{data_stream_name, stream_infos};

const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
const ERROR_HANDLE_EOF: i32 = 38;
const ERROR_MORE_DATA: i32 = 234;
/// Filesystems without streams (FAT, some network shares).
const ERROR_INVALID_PARAMETER: i32 = 87;
const ERROR_NOT_SUPPORTED: i32 = 50;

fn query(f: &File) -> io::Result<Vec<u8>> {
    // u64 storage keeps the records 8-byte aligned, as Windows expects.
    let mut buf = vec![0u64; 8 << 10];
    loop {
        let bytes = (buf.len() * 8) as u32;
        // SAFETY: the handle is open for the call; `buf` holds `bytes` bytes.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                f.as_raw_handle(),
                FileStreamInfo,
                buf.as_mut_ptr().cast(),
                bytes,
            )
        };
        if ok != 0 {
            return Ok(buf.iter().flat_map(|w| w.to_le_bytes()).collect());
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(ERROR_HANDLE_EOF | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED) => {
                return Ok(Vec::new());
            }
            Some(ERROR_MORE_DATA | ERROR_INSUFFICIENT_BUFFER) if buf.len() < (1 << 20) => {
                buf.resize(buf.len() * 4, 0);
            }
            _ => return Err(e),
        }
    }
}

/// Named data streams of `path` (not the main stream).
pub fn list(path: &Path) -> io::Result<Vec<StreamInfo>> {
    let buf = query(&open_backup(path)?)?;
    Ok(stream_infos(&buf)
        .into_iter()
        .filter_map(|(raw, size)| {
            data_stream_name(&raw).map(|n| StreamInfo {
                name: n.to_owned(),
                size,
            })
        })
        .collect())
}

/// Open stream `name` of `path`.
pub fn open(path: &Path, name: &str) -> io::Result<File> {
    let mut s = path.as_os_str().to_owned();
    s.push(":");
    s.push(name);
    open_backup(&PathBuf::from(s))
}
