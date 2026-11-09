//! Export hive keys as `.reg` text (regedit "Version 5.00" format, UTF-16LE
//! with BOM). Used for the Tier 3 HKCU allowlist and per-pack keys. Values
//! are exported as-is; nothing is decrypted.

use std::fmt::Write as _;

use crate::hive::{Hive, Key, Result as HiveResult, ValueType};

const MAX_KEYS: usize = 100_000;

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn hex_list(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// True if UTF-16 data is text with at most a trailing NUL (safe as "...").
fn plain_sz(data: &[u8]) -> bool {
    if data.len() % 2 != 0 {
        return false;
    }
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let body = match units.split_last() {
        Some((0, rest)) => rest,
        _ => &units[..],
    };
    !body.contains(&0) && !body.contains(&u16::from(b'\n')) && !body.contains(&u16::from(b'\r'))
}

fn value_line(v: &crate::hive::Value) -> String {
    let name = if v.name.is_empty() {
        "@".to_owned()
    } else {
        quote(&v.name)
    };
    let data = match v.kind {
        ValueType::Sz if plain_sz(&v.data) => quote(&v.as_string().unwrap_or_default()),
        ValueType::Dword if v.data.len() == 4 => format!("dword:{:08x}", v.as_dword().unwrap_or(0)),
        ValueType::Binary => format!("hex:{}", hex_list(&v.data)),
        other => format!("hex({:x}):{}", u32::from(other), hex_list(&v.data)),
    };
    format!("{name}={data}")
}

fn export_key(k: &Key<'_>, path: &str, out: &mut String, count: &mut usize) -> HiveResult<()> {
    *count += 1;
    if *count > MAX_KEYS {
        return Err(crate::hive::HiveError::TooDeep);
    }
    let _ = write!(out, "\r\n[{path}]\r\n");
    for v in k.values()? {
        let _ = write!(out, "{}\r\n", value_line(&v));
    }
    for sub in k.subkeys()? {
        let child = format!("{path}\\{}", sub.name());
        export_key(&sub, &child, out, count)?;
    }
    Ok(())
}

/// Export `subpath` (relative to the hive root) and everything below it.
/// `root_label` is e.g. `HKEY_CURRENT_USER`. `None` if the key is absent.
pub fn export(hive: &Hive, subpath: &str, root_label: &str) -> HiveResult<Option<String>> {
    let Some(key) = hive.open_key(subpath)? else {
        return Ok(None);
    };
    // Use the hive's own casing for every path component.
    let mut canonical = Vec::new();
    let mut cur = hive.root()?;
    for part in subpath.split('\\').filter(|p| !p.is_empty()) {
        cur = cur
            .subkey(part)?
            .ok_or(crate::hive::HiveError::Malformed("key vanished".into()))?;
        canonical.push(cur.name().to_owned());
    }
    let mut out = String::new();
    export_key(
        &key,
        &format!("{root_label}\\{}", canonical.join("\\")),
        &mut out,
        &mut 0,
    )?;
    Ok(Some(out))
}

/// Wrap exported sections into a complete `.reg` file (UTF-16LE + BOM).
pub fn to_reg_file(sections: &[String]) -> Vec<u8> {
    let mut text = String::from("Windows Registry Editor Version 5.00\r\n");
    for s in sections {
        text.push_str(s);
    }
    text.push_str("\r\n");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hive::writer::HiveWriter;

    #[test]
    fn exports_types_recursively() {
        let mut w = HiveWriter::new("ROOT");
        w.key(r"Control Panel\Desktop")
            .sz("Wallpaper", r#"C:\Users\bob\Pictures\a "b".jpg"#)
            .dword("WheelScrollLines", 3)
            .expand_sz("", "%USERPROFILE%")
            .set("Mask", ValueType::Binary, vec![0x9e, 0x1e]);
        w.key(r"Control Panel\Mouse").sz("MouseSpeed", "1");
        let h = Hive::from_bytes(w.to_bytes()).unwrap();
        let s = export(&h, r"control panel", "HKEY_CURRENT_USER")
            .unwrap()
            .unwrap();
        assert!(s.contains("[HKEY_CURRENT_USER\\Control Panel]\r\n"));
        assert!(s.contains("[HKEY_CURRENT_USER\\Control Panel\\Desktop]\r\n"));
        assert!(
            s.contains(r#""Wallpaper"="C:\\Users\\bob\\Pictures\\a \"b\".jpg""#),
            "{s}"
        );
        assert!(s.contains("\"WheelScrollLines\"=dword:00000003"));
        assert!(s.contains("@=hex(2):25,00,55,00"));
        assert!(s.contains("\"Mask\"=hex:9e,1e"));
        assert!(s.find("Desktop]").unwrap() < s.find("Mouse]").unwrap());
        assert!(export(&h, "Nope", "HKEY_CURRENT_USER").unwrap().is_none());

        let file = to_reg_file(&[s]);
        assert_eq!(&file[..2], &[0xFF, 0xFE]);
        let text = String::from_utf16(
            &file[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(text.starts_with("Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER"));
    }

    #[test]
    fn odd_strings_become_hex() {
        assert!(plain_sz(&[b'a', 0, 0, 0]));
        assert!(!plain_sz(&[b'a', 0, 0, 0, b'b', 0]));
        assert!(!plain_sz(b"a"));
    }
}
