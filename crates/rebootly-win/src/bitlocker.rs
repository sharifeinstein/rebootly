//! BitLocker bits of the Windows backend that are plain data and text, so
//! they are tested on Linux: lock detection, and the PowerShell/WMI password
//! unlock (script and its answer).

/// A BitLocker volume's boot sector carries the OEM ID `-FVE-FS-` (bytes
/// 3..11) where NTFS has `NTFS    `.
pub fn is_bitlocker_boot_sector(sector: &[u8]) -> bool {
    sector.get(3..11) == Some(b"-FVE-FS-")
}

/// `FVE_E_LOCKED_VOLUME`: what `GetVolumeInformationW` (and `dir`) fail
/// with on a locked BitLocker volume. Raw reads of `\\.\X:` fail the same
/// way, so the boot sector can't be relied on while it is locked.
pub const FVE_E_LOCKED_VOLUME: u32 = 0x8031_0000;
/// The same as an NTSTATUS (`STATUS_FVE_LOCKED_VOLUME`), in case it's
/// passed through unmapped.
const STATUS_FVE_LOCKED_VOLUME: u32 = 0xC021_0000;

/// A failed volume call's last error says "locked by BitLocker".
pub fn is_locked_volume_error(code: u32) -> bool {
    code == FVE_E_LOCKED_VOLUME || code == STATUS_FVE_LOCKED_VOLUME
}

/// PowerShell that reads the password (base64 of UTF-8, one line) from
/// stdin and unlocks `drive` (an already checked `X:`) through WMI.
/// Prints one `rebootly:` line; never the password.
pub fn unlock_script(drive: &str) -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
try {{
  $b = [Console]::In.ReadLine()
  $p = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($b))
  $v = Get-CimInstance -Namespace 'root/cimv2/Security/MicrosoftVolumeEncryption' -ClassName Win32_EncryptableVolume -Filter "DriveLetter='{drive}'"
  if (-not $v) {{ 'rebootly:novolume'; exit 0 }}
  $r = Invoke-CimMethod -InputObject $v -MethodName UnlockWithPassphrase -Arguments @{{ Passphrase = $p }}
  'rebootly:rc=' + $r.ReturnValue
}} catch {{
  'rebootly:nowmi'
}}"#
    )
}

/// What the unlock script printed, as a result with a plain message.
pub fn parse_unlock_answer(out: &str, drive: &str) -> Result<(), String> {
    let line = out
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("rebootly:"))
        .unwrap_or("");
    if let Some(rc) = line.strip_prefix("rc=") {
        return match rc.trim().parse::<u32>() {
            Ok(0) => Ok(()),
            Ok(code) => Err(format!(
                "{drive} wasn't unlocked (BitLocker error 0x{code:08X}): check the password. A \
                 drive without a password (an OS drive with a TPM never has one) opens only \
                 with its recovery key"
            )),
            Err(_) => Err(format!(
                "{drive} wasn't unlocked (unexpected answer from WMI)"
            )),
        };
    }
    Err(match line {
        "novolume" => format!("{drive} isn't a BitLocker drive (WMI doesn't list it)"),
        "nowmi" => "BitLocker's WMI interface isn't available here (WinPE needs WinPE-WMI and \
                    WinPE-SecureStartup); use the recovery key instead"
            .into(),
        _ => format!("{drive} wasn't unlocked (no answer from PowerShell)"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitlocker_boot_sector() {
        let mut s = [0u8; 512];
        s[3..11].copy_from_slice(b"-FVE-FS-");
        assert!(is_bitlocker_boot_sector(&s));
        s[3..11].copy_from_slice(b"NTFS    ");
        assert!(!is_bitlocker_boot_sector(&s));
        assert!(!is_bitlocker_boot_sector(b"abc"));
    }

    #[test]
    fn locked_volume_error() {
        assert!(is_locked_volume_error(0x8031_0000));
        assert!(is_locked_volume_error(0xC021_0000));
        // ERROR_NOT_READY (card reader without media), ERROR_ACCESS_DENIED
        assert!(!is_locked_volume_error(21));
        assert!(!is_locked_volume_error(5));
    }

    #[test]
    fn unlock_script_names_the_drive_and_reads_stdin() {
        let s = unlock_script("R:");
        assert!(s.contains("DriveLetter='R:'"), "{s}");
        assert!(s.contains("[Console]::In.ReadLine()"));
        assert!(s.contains("UnlockWithPassphrase"));
        // format! braces came out single.
        assert!(s.contains("try {") && !s.contains("{{"));
    }

    #[test]
    fn unlock_answers() {
        assert_eq!(parse_unlock_answer("rebootly:rc=0\r\n", "R:"), Ok(()));
        let e = parse_unlock_answer("rebootly:rc=2150694951", "R:").unwrap_err();
        assert!(
            e.contains("0x80310027") && e.contains("check the password"),
            "{e}"
        );
        let e = parse_unlock_answer("rebootly:novolume", "R:").unwrap_err();
        assert!(e.contains("isn't a BitLocker drive"), "{e}");
        let e = parse_unlock_answer("rebootly:nowmi", "R:").unwrap_err();
        assert!(e.contains("WinPE-WMI"), "{e}");
        let e = parse_unlock_answer("", "R:").unwrap_err();
        assert!(e.contains("no answer"), "{e}");
    }
}
