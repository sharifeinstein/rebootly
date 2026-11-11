//! Drive letters as volumes (WinPE, or another disk seen from Windows):
//! filesystem and label, BitLocker lock state, and the partition identity
//! from this system's own `HKLM\SYSTEM\MountedDevices` (same format as the
//! offline hive's, so the offline Windows' letters can be matched).
//!
//! A locked BitLocker volume has a letter but no filesystem: its boot
//! sector (read through `\\.\X:`, needs admin) starts with `-FVE-FS-`.
//! Unlocking runs `manage-bde -unlock X: -RecoveryPassword <key>` (WinPE
//! needs the WinPE-SecureStartup component). Its output is never shown: the
//! key isn't echoed, but nothing from the command is trusted with it.
#![allow(unsafe_code)]

use std::io::Read;
use std::process::Command;
use std::ptr::null_mut;

use rebootly_core::volume::{RecoveryKey, Volume, VolumeId, VolumeState};
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RegGetValueW};

use super::cmd::run_cmd;
use super::wide::wide;
use crate::parse::is_bitlocker_boot_sector;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

fn utf16(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// (label, filesystem) of `X:\`; `None` when the volume can't be read
/// (locked, no media).
fn info(root: &str) -> Option<(String, String)> {
    let w = wide(root);
    let mut label = [0u16; 261];
    let mut fs = [0u16; 64];
    // SAFETY: `w` is NUL-terminated; buffers have the lengths we pass;
    // unused outputs are null.
    let ok = unsafe {
        GetVolumeInformationW(
            w.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            null_mut(),
            null_mut(),
            null_mut(),
            fs.as_mut_ptr(),
            fs.len() as u32,
        )
    };
    (ok != 0).then(|| (utf16(&label), utf16(&fs)))
}

fn total_size(root: &str) -> Option<u64> {
    let w = wide(root);
    let mut total = 0u64;
    // SAFETY: `w` is NUL-terminated; `total` is a valid out pointer.
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), null_mut(), &mut total, null_mut()) };
    (ok != 0).then_some(total)
}

/// The volume's boot sector says BitLocker (raw read; needs admin).
fn bitlocker_header(letter: char) -> bool {
    let mut sector = [0u8; 512];
    std::fs::File::open(format!(r"\\.\{letter}:"))
        .and_then(|mut f| f.read_exact(&mut sector))
        .is_ok_and(|()| is_bitlocker_boot_sector(&sector))
}

/// This system's `MountedDevices` entry for `X:`.
pub fn letter_identity(letter: char) -> Option<VolumeId> {
    let key = wide(r"SYSTEM\MountedDevices");
    let name = wide(&format!(r"\DosDevices\{letter}:"));
    let mut buf = [0u8; 256];
    let mut len = buf.len() as u32;
    // SAFETY: strings are NUL-terminated; `buf`/`len` describe the buffer.
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_BINARY,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return None;
    }
    VolumeId::from_mounted_devices(&buf[..(len as usize).min(buf.len())])
}

/// Fixed and removable drive letters holding NTFS or exFAT, or locked.
pub fn volumes() -> Vec<Volume> {
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!(r"{letter}:\");
        let w = wide(&root);
        // SAFETY: `w` is NUL-terminated.
        let kind = unsafe { GetDriveTypeW(w.as_ptr()) };
        if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
            continue;
        }
        let (label, fs, state) = match info(&root) {
            Some((label, fs)) => {
                let wanted = fs.eq_ignore_ascii_case("ntfs") || fs.eq_ignore_ascii_case("exfat");
                if !wanted {
                    continue;
                }
                let state = VolumeState::Mounted {
                    root: root.clone().into(),
                    read_only: false,
                };
                (Some(label), Some(fs), state)
            }
            None if bitlocker_header(letter) => {
                (None, Some("BitLocker".into()), VolumeState::Locked)
            }
            None => continue,
        };
        out.push(Volume {
            device: format!("{letter}:"),
            label: label.filter(|l| !l.is_empty()),
            size: total_size(&root),
            fs,
            state,
            id: letter_identity(letter),
            removable: kind == DRIVE_REMOVABLE,
        });
    }
    out
}

/// Unlock a BitLocker drive letter with its recovery key.
pub fn unlock(device: &str, key: &RecoveryKey) -> Result<(), String> {
    let letter = device.trim_end_matches(['\\', ':']);
    if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!("{device} is not a drive letter"));
    }
    let drive = format!("{}:", letter.to_ascii_uppercase());
    let res = run_cmd(Command::new("manage-bde").args([
        "-unlock",
        &drive,
        "-RecoveryPassword",
        key.expose(),
    ]));
    match res {
        Ok(_) => Ok(()),
        Err(e) if e.starts_with("not available") => Err(
            "manage-bde isn't available here; a WinPE image needs the WinPE-SecureStartup \
             component (and WinPE-WMI) for BitLocker"
                .into(),
        ),
        Err(e) => Err(format!(
            "{drive} wasn't unlocked (manage-bde {e}); check the recovery key and try again"
        )),
    }
}
