//! Signed-in users other than the one running Rebootly (WTS API). Any
//! failure means "none found": this only feeds a pre-flight note.
#![allow(unsafe_code)]

use std::ptr;

use windows_sys::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW, WTSDisconnected,
    WTSEnumerateSessionsW, WTSFreeMemory, WTSQuerySessionInformationW, WTSUserName,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use super::wide::from_wide_ptr;
use crate::sessions::OtherSession;

/// Sessions with a signed-in user, except this process's own session.
pub fn others() -> Vec<OtherSession> {
    let mut own = u32::MAX;
    // SAFETY: plain out-parameter.
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut own) } == 0 {
        return Vec::new();
    }
    let mut info: *mut WTS_SESSION_INFOW = ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: out-parameters; the buffer is freed below with WTSFreeMemory.
    if unsafe { WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut info, &mut count) } == 0
        || info.is_null()
    {
        return Vec::new();
    }
    // SAFETY: WTSEnumerateSessionsW returned `count` entries at `info`.
    let sessions = unsafe { std::slice::from_raw_parts(info, count as usize) };
    let out = sessions
        .iter()
        .filter(|s| s.SessionId != own)
        .filter_map(|s| {
            let user = user_name(s.SessionId)?;
            Some(OtherSession {
                user,
                disconnected: s.State == WTSDisconnected,
            })
        })
        .collect();
    // SAFETY: allocated by WTSEnumerateSessionsW.
    unsafe { WTSFreeMemory(info.cast()) };
    out
}

fn user_name(session: u32) -> Option<String> {
    let mut buf: *mut u16 = ptr::null_mut();
    let mut bytes = 0u32;
    // SAFETY: out-parameters; the buffer is freed below with WTSFreeMemory.
    let ok = unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session,
            WTSUserName,
            &mut buf,
            &mut bytes,
        )
    };
    if ok == 0 || buf.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated string from WTSQuerySessionInformationW.
    let name = unsafe { from_wide_ptr(buf) };
    // SAFETY: allocated by WTSQuerySessionInformationW.
    unsafe { WTSFreeMemory(buf.cast()) };
    (!name.is_empty()).then_some(name)
}
