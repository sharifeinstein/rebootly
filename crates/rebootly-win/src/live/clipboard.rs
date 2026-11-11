//! Reading text from the Windows clipboard (paste into the CLI key prompt
//! and the GUI, which is built without eframe's clipboard crates).
#![allow(unsafe_code)]

use std::ptr::null_mut;

use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

const CF_UNICODETEXT: u32 = 13;
/// More than any key, password or path anyone pastes.
const MAX_UNITS: usize = 32 * 1024;

/// The clipboard's text, if it holds any.
pub fn clipboard_text() -> Option<String> {
    // SAFETY: plain Win32 calls; the clipboard is closed after a
    // successful open whatever `read_open` found.
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return None;
        }
        let text = read_open();
        CloseClipboard();
        text
    }
}

/// # Safety
/// The clipboard must be open (by this thread).
unsafe fn read_open() -> Option<String> {
    // SAFETY: the handle comes from the open clipboard; the locked memory
    // is only read within GlobalSize bytes, then unlocked.
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT);
        if h.is_null() {
            return None;
        }
        let p = GlobalLock(h).cast::<u16>();
        if p.is_null() {
            return None;
        }
        let units = (GlobalSize(h) / 2).min(MAX_UNITS);
        let buf = std::slice::from_raw_parts(p, units);
        let n = buf.iter().position(|&c| c == 0).unwrap_or(units);
        let s = String::from_utf16_lossy(&buf[..n]);
        GlobalUnlock(h);
        Some(s)
    }
}
