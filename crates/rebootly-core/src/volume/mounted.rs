//! Drive letters of the offline Windows, from `SYSTEM\MountedDevices`.
//!
//! Each `\DosDevices\X:` value names the partition that had letter X:
//! - 12 bytes: MBR disk signature (u32 LE) + partition byte offset (u64 LE);
//! - 24 bytes: `DMIO:ID:` + the GPT partition GUID (Windows byte order).
//!
//! Other values (USB sticks, CD drives: UTF-16 device paths) can't be
//! matched to a partition and are left out. The running system reports the
//! same identity for each mounted volume (lsblk on Linux, the live
//! `MountedDevices` in WinPE), so letters are matched by identity, never by
//! the letter WinPE happened to assign.

use std::path::PathBuf;

use serde::Serialize;

use crate::hive::{Hive, HiveError};

/// Which partition a volume is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "style", rename_all = "snake_case")]
pub enum VolumeId {
    /// MBR disk: disk signature and the partition's offset in bytes.
    Mbr { signature: u32, offset: u64 },
    /// GPT partition GUID, lowercase `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
    Gpt { partition: String },
}

impl VolumeId {
    /// Decode a `MountedDevices` value (also used for the live registry).
    pub fn from_mounted_devices(data: &[u8]) -> Option<Self> {
        if data.len() == 12 {
            let signature = u32::from_le_bytes(data[0..4].try_into().ok()?);
            let offset = u64::from_le_bytes(data[4..12].try_into().ok()?);
            return Some(VolumeId::Mbr { signature, offset });
        }
        if data.len() == 24 && &data[..8] == b"DMIO:ID:" {
            return Some(VolumeId::Gpt {
                partition: guid_string(&data[8..24]),
            });
        }
        None
    }

    /// A GPT partition GUID as text (lsblk's PARTUUID), any case.
    pub fn gpt(text: &str) -> Self {
        VolumeId::Gpt {
            partition: text.trim().trim_matches(['{', '}']).to_ascii_lowercase(),
        }
    }
}

/// Windows GUID bytes (Data1/2/3 little-endian) → canonical text.
pub fn guid_string(b: &[u8]) -> String {
    let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let d2 = u16::from_le_bytes([b[4], b[5]]);
    let d3 = u16::from_le_bytes([b[6], b[7]]);
    let tail: String = b[10..16].iter().map(|x| format!("{x:02x}")).collect();
    format!("{d1:08x}-{d2:04x}-{d3:04x}-{:02x}{:02x}-{tail}", b[8], b[9])
}

/// Canonical GUID text → Windows bytes (inverse of [`guid_string`]).
pub fn guid_bytes(text: &str) -> Option<[u8; 16]> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut b = [0u8; 16];
    for (i, x) in b.iter_mut().enumerate() {
        *x = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    b[0..4].reverse();
    b[4..6].reverse();
    b[6..8].reverse();
    Some(b)
}

/// The `MountedDevices` value for a GPT partition (fixtures, tests).
pub fn gpt_value(text: &str) -> Option<Vec<u8>> {
    let mut v = b"DMIO:ID:".to_vec();
    v.extend_from_slice(&guid_bytes(text)?);
    Some(v)
}

/// `X: → partition` for every letter of the offline Windows.
pub fn drive_letters(system: &Hive) -> Result<Vec<(char, VolumeId)>, HiveError> {
    let Some(key) = system.open_key("MountedDevices")? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in key.values()? {
        let Some(rest) = v.name.strip_prefix(r"\DosDevices\") else {
            continue;
        };
        let mut c = rest.chars();
        let (Some(l), Some(':'), None) = (c.next(), c.next(), c.next()) else {
            continue;
        };
        if let Some(id) = VolumeId::from_mounted_devices(&v.data) {
            out.push((l.to_ascii_uppercase(), id));
        }
    }
    out.sort_by_key(|(l, _)| *l);
    Ok(out)
}

/// A volume that is readable here, and which partition it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedVolume {
    pub id: VolumeId,
    pub root: PathBuf,
}

/// Letters (other than `system_drive`) whose partition is mounted here.
pub fn match_letters(
    letters: &[(char, VolumeId)],
    system_drive: char,
    mounted: &[MountedVolume],
    system_root: &std::path::Path,
) -> Vec<(char, PathBuf)> {
    letters
        .iter()
        .filter(|(l, _)| *l != system_drive.to_ascii_uppercase())
        .filter_map(|(l, id)| {
            mounted
                .iter()
                .find(|m| &m.id == id && m.root != system_root)
                .map(|m| (*l, m.root.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hive::ValueType;
    use crate::hive::writer::HiveWriter;

    fn gpt_blob(guid_bytes: [u8; 16]) -> Vec<u8> {
        let mut v = b"DMIO:ID:".to_vec();
        v.extend_from_slice(&guid_bytes);
        v
    }

    // {01234567-89ab-cdef-0011-223344556677} in Windows byte order.
    const GUID: [u8; 16] = [
        0x67, 0x45, 0x23, 0x01, 0xab, 0x89, 0xef, 0xcd, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
        0x77,
    ];

    #[test]
    fn decodes_mbr_gpt_and_skips_device_paths() {
        let mut mbr = 0x1234_abcdu32.to_le_bytes().to_vec();
        mbr.extend_from_slice(&(1_048_576u64).to_le_bytes());
        let usb: Vec<u8> = r"\??\USBSTOR#Disk"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut w = HiveWriter::new("SYSTEM");
        w.key("MountedDevices")
            .set(r"\DosDevices\C:", ValueType::Binary, mbr)
            .set(r"\DosDevices\d:", ValueType::Binary, gpt_blob(GUID))
            .set(r"\DosDevices\E:", ValueType::Binary, usb)
            .set(r"\??\Volume{x}", ValueType::Binary, gpt_blob(GUID));
        let hive = Hive::from_bytes(w.to_bytes()).unwrap();
        let got = drive_letters(&hive).unwrap();
        assert_eq!(
            guid_bytes("01234567-89ab-cdef-0011-223344556677"),
            Some(GUID)
        );
        assert_eq!(
            got,
            vec![
                (
                    'C',
                    VolumeId::Mbr {
                        signature: 0x1234_abcd,
                        offset: 1_048_576
                    }
                ),
                ('D', VolumeId::gpt("{01234567-89AB-CDEF-0011-223344556677}")),
            ]
        );
    }

    #[test]
    fn letters_match_by_partition_not_by_letter() {
        let letters = vec![
            ('C', VolumeId::gpt("aa")),
            ('D', VolumeId::gpt("bb")),
            ('F', VolumeId::gpt("cc")),
        ];
        let mounted = vec![
            MountedVolume {
                id: VolumeId::gpt("aa"),
                root: "/run/rebootly/sda3".into(),
            },
            MountedVolume {
                id: VolumeId::gpt("bb"),
                root: "/run/rebootly/sdb1".into(),
            },
        ];
        let got = match_letters(&letters, 'c', &mounted, "/run/rebootly/sda3".as_ref());
        assert_eq!(got, vec![('D', PathBuf::from("/run/rebootly/sdb1"))]);
    }
}
