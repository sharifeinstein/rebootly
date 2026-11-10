//! `.reg` files from the backup are checked before import (SPEC §5): every
//! key must be under the pack's HKCU allowlist, and nothing may be deleted.
//! A backup is only data; this keeps a tampered one from writing elsewhere.

/// Keys whose import changes things the user should look at first.
pub const REVIEW_KEYS: &[(&str, &str)] = &[
    (
        "HKEY_CURRENT_USER\\Environment",
        "user environment variables (PATH and others)",
    ),
    ("HKEY_CURRENT_USER\\Network", "mapped network drives"),
];

/// Decode a `.reg` file (UTF-16LE with BOM as regedit writes, or UTF-8).
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&units).map_err(|_| "not valid UTF-16".into());
    }
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(body.to_vec()).map_err(|_| "not a text .reg file".into())
}

/// UTF-16LE with BOM, CRLF line ends (what `reg import` expects).
pub fn encode(text: &str) -> Vec<u8> {
    let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let mut b = vec![0xFF, 0xFE];
    b.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    b
}

/// `HKCU\x` / `HKEY_CURRENT_USER\x` → `hkey_current_user\x` (for comparing).
fn norm_key(k: &str) -> String {
    let k = k.trim().trim_end_matches('\\');
    let k = match k.get(..5) {
        Some(h) if h.eq_ignore_ascii_case("HKCU\\") => format!("HKEY_CURRENT_USER\\{}", &k[5..]),
        _ => k.to_owned(),
    };
    k.to_lowercase()
}

/// Checked `.reg` contents: the keys it writes, and the review notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub keys: Vec<String>,
    pub review: Vec<String>,
}

/// Check the text against `allow` (pack registry entries, `HKCU\…`).
pub fn check(text: &str, allow: &[String]) -> Result<Checked, String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    match lines.next() {
        Some(h) if h.trim_start_matches('\u{feff}') == "Windows Registry Editor Version 5.00" => {}
        _ => return Err("missing the `Windows Registry Editor Version 5.00` header".into()),
    }
    let allow: Vec<String> = allow.iter().map(|a| norm_key(a)).collect();
    let mut keys = Vec::new();
    let mut continued = false;
    for line in lines {
        let cont = continued;
        continued = line.ends_with('\\');
        if cont || line.starts_with(';') {
            continue;
        }
        if let Some(key) = line.strip_prefix('[') {
            let key = key
                .strip_suffix(']')
                .ok_or_else(|| format!("bad key line: {line}"))?;
            if key.starts_with('-') {
                return Err(format!("deletes a key ({key}); not allowed"));
            }
            let n = norm_key(key);
            if !n.starts_with("hkey_current_user\\") {
                return Err(format!("{key} is outside HKEY_CURRENT_USER"));
            }
            if !allow
                .iter()
                .any(|a| n == *a || n.starts_with(&format!("{a}\\")))
            {
                return Err(format!("{key} is not on this pack's allowlist"));
            }
            keys.push(key.to_owned());
        } else if line.ends_with("=-") {
            return Err(format!("deletes a value ({line}); not allowed"));
        } else if keys.is_empty() {
            return Err(format!("value before any key: {line}"));
        }
    }
    let mut review = Vec::new();
    for (k, what) in REVIEW_KEYS {
        let nk = norm_key(k);
        if keys.iter().any(|x| {
            let n = norm_key(x);
            n == nk || n.starts_with(&format!("{nk}\\"))
        }) {
            review.push(format!("{k}: {what}"));
        }
    }
    Ok(Checked { keys, review })
}

/// Top-level keys of the file (keys not inside another key of the file),
/// i.e. what to export first so an undo can put them back.
pub fn top_keys(keys: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut sorted: Vec<&String> = keys.iter().collect();
    sorted.sort_by_key(|k| norm_key(k));
    for k in sorted {
        let n = norm_key(k);
        if !out.iter().any(|o| {
            let o = norm_key(o);
            n == o || n.starts_with(&format!("{o}\\"))
        }) {
            out.push(k.clone());
        }
    }
    out
}

/// Rewrite the old profile path inside string values (`.reg` escapes
/// backslashes, so the stored form is `C:\\Users\\old`).
pub fn rewrite_profile(text: &str, old: &str, new: &str) -> (String, usize) {
    let esc = |s: &str| s.trim_end_matches('\\').replace('\\', r"\\");
    super::rewrite::replace_path_ci(text, &esc(old), &esc(new))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "Windows Registry Editor Version 5.00\r\n\r\n";

    fn allow() -> Vec<String> {
        vec![r"HKCU\Control Panel".into(), r"HKCU\Environment".into()]
    }

    #[test]
    fn accepts_allowlisted_keys_and_flags_review() {
        let t = format!(
            "{HEAD}[HKEY_CURRENT_USER\\Control Panel\\Desktop]\r\n\"Wallpaper\"=\"C:\\\\Users\\\\Alice\\\\bg.jpg\"\r\n\"Bin\"=hex:01,02,\\\r\n  03\r\n\r\n[HKEY_CURRENT_USER\\Environment]\r\n\"Path\"=\"C:\\\\Users\\\\Alice\\\\bin\"\r\n"
        );
        let c = check(&t, &allow()).unwrap();
        assert_eq!(c.keys.len(), 2);
        assert_eq!(c.review.len(), 1);
        let (out, n) = rewrite_profile(&t, r"C:\Users\Alice", r"C:\Users\sam");
        assert_eq!(n, 2);
        assert!(out.contains(r#""Wallpaper"="C:\\Users\\sam\\bg.jpg""#));
        assert_eq!(
            decode(&encode(&out)).unwrap(),
            out.replace("\r\n", "\n").replace('\n', "\r\n")
        );
    }

    #[test]
    fn rejects_everything_else() {
        for body in [
            "[HKEY_LOCAL_MACHINE\\SOFTWARE\\Run]\r\n",
            "[HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n",
            "[-HKEY_CURRENT_USER\\Control Panel\\Desktop]\r\n",
            "[HKEY_CURRENT_USER\\Control Panel\\Desktop]\r\n\"Wallpaper\"=-\r\n",
            "[HKEY_CURRENT_USER\\Control PanelX]\r\n",
            "\"x\"=\"y\"\r\n",
        ] {
            assert!(check(&format!("{HEAD}{body}"), &allow()).is_err(), "{body}");
        }
        assert!(check("[HKEY_CURRENT_USER\\Control Panel]", &allow()).is_err());
    }

    #[test]
    fn top_level_keys() {
        let keys: Vec<String> = [
            r"HKEY_CURRENT_USER\Control Panel",
            r"HKEY_CURRENT_USER\Control Panel\Desktop",
            r"HKEY_CURRENT_USER\Environment",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(top_keys(&keys), [keys[0].clone(), keys[2].clone()]);
    }
}
