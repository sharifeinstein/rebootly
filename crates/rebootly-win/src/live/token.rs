//! Process token: elevation, privileges, current user SID.
#![allow(unsafe_code)]

use std::io;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
    SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION, TOKEN_PRIVILEGES, TOKEN_QUERY,
    TOKEN_USER, TokenElevation, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::wide::{from_wide_ptr, wide};

struct Token(HANDLE);

impl Token {
    fn open(access: u32) -> io::Result<Self> {
        let mut h: HANDLE = null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle; `h` is a valid out-pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut h) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Token(h))
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: we own this handle.
        unsafe { CloseHandle(self.0) };
    }
}

/// Whether the process runs elevated (UAC "Run as administrator").
pub fn is_elevated() -> io::Result<bool> {
    let t = Token::open(TOKEN_QUERY)?;
    let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut len = 0u32;
    // SAFETY: buffer is a TOKEN_ELEVATION of the size we pass.
    let ok = unsafe {
        GetTokenInformation(
            t.0,
            TokenElevation,
            (&mut e as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(e.TokenIsElevated != 0)
}

/// Enable a privilege (e.g. `SeBackupPrivilege`) on the process token.
/// `Ok(false)`: the token doesn't hold it (not elevated).
pub fn enable_privilege(name: &str) -> io::Result<bool> {
    let t = Token::open(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY)?;
    let wname = wide(name);
    let mut tp = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: Default::default(),
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: `wname` is NUL-terminated; the LUID out-pointer is valid.
    if unsafe { LookupPrivilegeValueW(null(), wname.as_ptr(), &mut tp.Privileges[0].Luid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `tp` is a valid TOKEN_PRIVILEGES with one entry.
    let ok = unsafe { AdjustTokenPrivileges(t.0, 0, &tp, 0, null_mut(), null_mut()) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: plain thread-local error read.
    Ok(unsafe { GetLastError() } != ERROR_NOT_ALL_ASSIGNED)
}

/// String SID of the user the process runs as (`S-1-5-21-…`).
pub fn current_user_sid() -> io::Result<String> {
    let t = Token::open(TOKEN_QUERY)?;
    let mut len = 0u32;
    // SAFETY: size query with a null buffer.
    unsafe { GetTokenInformation(t.0, TokenUser, null_mut(), 0, &mut len) };
    // u64 storage keeps the buffer aligned for TOKEN_USER.
    let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
    // SAFETY: buffer is at least `len` bytes and aligned.
    let ok = unsafe { GetTokenInformation(t.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the API filled the buffer with a TOKEN_USER.
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    let mut s = null_mut();
    // SAFETY: the SID pointer comes from the token info above.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut s) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `s` is a NUL-terminated LocalAlloc'd string we must free.
    let sid = unsafe { from_wide_ptr(s) };
    unsafe { LocalFree(s.cast()) };
    Ok(sid)
}
