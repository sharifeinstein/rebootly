//! Platform-neutral parsing for the live backend, so it is tested on Linux:
//! `FILE_STREAM_INFO` buffers, stream names, VSS and powercfg output, and
//! the random PFX password.

/// `FILE_STREAM_INFO` records in a `GetFileInformationByHandleEx` buffer:
/// (raw name such as `:tag:$DATA`, size).
pub fn stream_infos(buf: &[u8]) -> Vec<(String, u64)> {
    let rd32 = |o: usize| {
        buf.get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let rd64 = |o: usize| {
        buf.get(o..o + 8)
            .map(|b| i64::from_le_bytes(b.try_into().unwrap()))
    };
    let mut out = Vec::new();
    let mut off = 0usize;
    while let (Some(next), Some(len), Some(size)) = (rd32(off), rd32(off + 4), rd64(off + 8)) {
        let start = off + 24;
        let Some(name) = buf.get(start..start + len as usize) else {
            break;
        };
        let units: Vec<u16> = name
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        out.push((String::from_utf16_lossy(&units), size.max(0) as u64));
        if next == 0 {
            break;
        }
        off += next as usize;
    }
    out
}

/// `:name:$DATA` → `name`. The main stream (`::$DATA`) and other stream
/// types give `None`.
pub fn data_stream_name(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix(':')?;
    let (name, kind) = rest.rsplit_once(':')?;
    (kind.eq_ignore_ascii_case("$DATA") && !name.is_empty()).then_some(name)
}

/// A `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}` GUID (safe to put in a WMI filter).
pub fn is_braced_guid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 38
        && b[0] == b'{'
        && b[37] == b'}'
        && b[1..37].iter().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// The first bare GUID in `text` (e.g. `powercfg /getactivescheme`, whose
/// wording is localized but whose GUID isn't).
pub fn find_guid(text: &str) -> Option<&str> {
    (0..text.len().saturating_sub(35)).find_map(|i| {
        let cand = text.get(i..i + 36)?;
        is_braced_guid(&format!("{{{cand}}}")).then_some(cand)
    })
}

/// Output of the shadow-copy script: `OK <id> <device>` or `ERR <code>`.
pub fn vss_created(out: &str) -> Result<(String, String), String> {
    let out = out.trim();
    if let Some(code) = out.strip_prefix("ERR ") {
        let why = match code.trim() {
            "1" => "access denied",
            "3" | "2" => "the volume isn't supported",
            "4" | "5" => "shadow copies aren't supported on this volume",
            "6" => "not enough free space on the volume for a shadow copy",
            "7" | "9" => "another shadow copy is being made; try again shortly",
            "8" => "the maximum number of shadow copies is reached",
            "10" | "11" | "12" => "the shadow copy provider failed (is the VSS service disabled?)",
            _ => "unknown error",
        };
        return Err(format!("{why} (code {})", code.trim()));
    }
    let mut parts = out
        .strip_prefix("OK ")
        .unwrap_or_default()
        .split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(id), Some(dev))
            if is_braced_guid(id) && dev.starts_with(r"\\?\GLOBALROOT\Device\") =>
        {
            Ok((id.to_owned(), dev.trim_end_matches('\\').to_owned()))
        }
        _ => Err("unexpected answer from WMI".into()),
    }
}

/// `path` below volume root `root`, compared case-insensitively; both in
/// Windows form (`C:\`, `C:\Users\x`), with or without a `\\?\` prefix.
pub fn below_root<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let root = root.strip_prefix(r"\\?\").unwrap_or(root);
    let root = root.trim_end_matches('\\');
    let head = path.get(..root.len())?;
    let rest = path.get(root.len()..)?;
    (head.eq_ignore_ascii_case(root) && (rest.is_empty() || rest.starts_with('\\')))
        .then(|| rest.trim_start_matches('\\'))
}

/// A 24-character password from random bytes (letters and digits, no
/// look-alikes), for the PFX files inside the encrypted bundle.
pub fn password(random: &[u8; 24]) -> String {
    const SET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    random
        .iter()
        .map(|b| SET[*b as usize % SET.len()] as char)
        .collect()
}

/// PowerShell `-EncodedCommand` form: base64 of the UTF-16LE script.
pub fn encode_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64_encode(&bytes)
}

/// Standard base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Image names from `tasklist /fo csv /nh` (`"chrome.exe","1234",...`).
pub fn tasklist_images(csv: &str) -> Vec<String> {
    csv.lines()
        .filter_map(|l| {
            l.strip_prefix('"')?
                .split_once('"')
                .map(|(n, _)| n.to_owned())
        })
        .filter(|n| !n.is_empty())
        .collect()
}

/// `CurrentBuildNumber    REG_SZ    22631` → 22631 (`reg query` output).
pub fn reg_query_number(out: &str, value: &str) -> Option<u32> {
    out.lines()
        .find(|l| {
            l.split_whitespace()
                .next()
                .is_some_and(|n| n.eq_ignore_ascii_case(value))
        })
        .and_then(|l| l.split_whitespace().last())
        .and_then(|n| n.parse().ok())
}

/// A PowerShell single-quoted string literal.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A certificate thumbprint (40 hex digits for SHA-1).
pub fn is_thumbprint(s: &str) -> bool {
    (40..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Can be passed as `name="…"` to netsh / schtasks without breaking quoting.
pub fn quotable(s: &str) -> bool {
    !s.is_empty() && !s.contains(['"', '\0', '\r', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(next: u32, name: &str, size: i64) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = Vec::new();
        v.extend(next.to_le_bytes());
        v.extend(((units.len() * 2) as u32).to_le_bytes());
        v.extend(size.to_le_bytes());
        v.extend(size.to_le_bytes());
        for u in units {
            v.extend(u.to_le_bytes());
        }
        v
    }

    #[test]
    fn parses_stream_info_buffers() {
        let mut a = record(0, "::$DATA", 10);
        let pad = a.len().div_ceil(8) * 8;
        let next = pad as u32;
        a = record(next, "::$DATA", 10);
        a.resize(pad, 0);
        a.extend(record(0, ":Zone.Identifier:$DATA", 26));
        let got = stream_infos(&a);
        assert_eq!(
            got,
            [
                ("::$DATA".into(), 10),
                (":Zone.Identifier:$DATA".into(), 26)
            ]
        );
        assert!(stream_infos(&a[..10]).is_empty(), "truncated buffer");
        assert_eq!(data_stream_name("::$DATA"), None);
        assert_eq!(
            data_stream_name(":Zone.Identifier:$DATA"),
            Some("Zone.Identifier")
        );
        assert_eq!(data_stream_name(":x:$INDEX_ALLOCATION"), None);
    }

    #[test]
    fn guids_and_vss_answers() {
        let g = "{3E2B1A4C-1111-2222-3333-444455556666}";
        assert!(is_braced_guid(g) && !is_braced_guid("{x' OR 1=1}"));
        let txt =
            "GUID du mode de gestion : 381b4222-f694-41f0-9685-ff5bb260df2e  (Utilisation normale)";
        assert_eq!(find_guid(txt), Some("381b4222-f694-41f0-9685-ff5bb260df2e"));
        assert_eq!(find_guid("none"), None);
        let ok = format!(r"OK {g} \\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy7");
        assert_eq!(
            vss_created(&ok).unwrap().1,
            r"\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy7"
        );
        assert!(vss_created("ERR 6").unwrap_err().contains("free space"));
        assert!(vss_created("garbage").is_err());
    }

    #[test]
    fn paths_below_a_volume_root() {
        assert_eq!(
            below_root(r"C:\Users\a\x.pst", r"C:\"),
            Some(r"Users\a\x.pst")
        );
        assert_eq!(below_root(r"\\?\c:\Users\a", r"C:\"), Some(r"Users\a"));
        assert_eq!(below_root(r"D:\Data\x", r"D:\Data\"), Some("x"));
        assert_eq!(below_root(r"D:\Database\x", r"D:\Data\"), None);
        assert_eq!(below_root(r"E:\x", r"C:\"), None);
    }

    #[test]
    fn encoded_command_is_utf16le_base64() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('dir'))
        assert_eq!(encode_command("dir"), "ZABpAHIA");
        assert_eq!(encode_command("a"), "YQA=");
        assert_eq!(encode_command(""), "");
    }

    #[test]
    fn password_shape() {
        let p = password(&[7; 24]);
        assert_eq!(p.len(), 24);
        assert!(p.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn restore_helpers() {
        let csv =
            "\"System Idle Process\",\"0\",\"Services\"\r\n\"chrome.exe\",\"4242\",\"Console\"\r\n";
        assert_eq!(tasklist_images(csv), ["System Idle Process", "chrome.exe"]);
        let reg = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\r\n    CurrentBuildNumber    REG_SZ    22631\r\n";
        assert_eq!(reg_query_number(reg, "CurrentBuildNumber"), Some(22631));
        assert_eq!(ps_quote("C:\\it's"), "'C:\\it''s'");
        assert!(is_thumbprint("0123456789ABCDEF0123456789abcdef01234567"));
        assert!(!is_thumbprint("../../x"));
        assert!(quotable("Home Wi-Fi") && !quotable("a\"b"));
    }

    #[test]
    fn base64_pads() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
        assert_eq!(base64_encode("é".as_bytes()), "w6k=");
    }
}
