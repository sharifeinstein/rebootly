//! Path rewriting inside text configs (SPEC §5, packs with `rewrite: true`):
//! the old profile path becomes the new one, in the spellings configs use:
//! `C:\Users\old`, JSON-escaped `C:\\Users\\old`, and `C:/Users/old`.

/// Files above this are left as they are.
pub const MAX_REWRITE: u64 = 16 * 1024 * 1024;

/// Replace `old` with `new` in every spelling. `None` if the bytes aren't
/// text (UTF-8 without NULs, or UTF-16LE with a BOM) or nothing matched.
pub fn rewrite_bytes(bytes: &[u8], old: &str, new: &str) -> Option<(Vec<u8>, usize)> {
    if bytes.len() as u64 > MAX_REWRITE {
        return None;
    }
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        if body.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16(&units).ok()?;
        let (out, n) = rewrite_text(&text, old, new);
        if n == 0 {
            return None;
        }
        let mut b = vec![0xFF, 0xFE];
        b.extend(out.encode_utf16().flat_map(u16::to_le_bytes));
        return Some((b, n));
    }
    let (bom, body) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(b) => (true, b),
        None => (false, bytes),
    };
    if body.contains(&0) {
        return None;
    }
    let text = std::str::from_utf8(body).ok()?;
    let (out, n) = rewrite_text(text, old, new);
    if n == 0 {
        return None;
    }
    let mut b = if bom {
        vec![0xEF, 0xBB, 0xBF]
    } else {
        Vec::new()
    };
    b.extend_from_slice(out.as_bytes());
    Some((b, n))
}

/// Rewrite all three spellings; returns the text and the number of hits.
pub fn rewrite_text(text: &str, old: &str, new: &str) -> (String, usize) {
    let old = old.trim_end_matches(['\\', '/']);
    let new = new.trim_end_matches(['\\', '/']);
    if old.is_empty() || old.eq_ignore_ascii_case(new) {
        return (text.to_owned(), 0);
    }
    let mut total = 0;
    let mut out = text.to_owned();
    for sep in [r"\\", r"\", "/"] {
        let o = old.replace('\\', sep);
        let n = new.replace('\\', sep);
        let (next, hits) = replace_path_ci(&out, &o, &n);
        out = next;
        total += hits;
    }
    (out, total)
}

/// Replace `old` (ASCII case-insensitive) where it is a whole path prefix:
/// the next character must end a path component, so `C:\Users\Al` never
/// matches inside `C:\Users\Alice`.
pub fn replace_path_ci(text: &str, old: &str, new: &str) -> (String, usize) {
    let (t, o) = (text.as_bytes(), old.as_bytes());
    if o.is_empty() || t.len() < o.len() {
        return (text.to_owned(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let (mut i, mut last, mut hits) = (0, 0, 0);
    while i + o.len() <= t.len() {
        if t[i..i + o.len()].eq_ignore_ascii_case(o)
            && t.get(i + o.len()).is_none_or(|&c| ends_component(c))
            && (i == 0 || !is_word(t[i - 1]))
        {
            out.push_str(&text[last..i]);
            out.push_str(new);
            i += o.len();
            last = i;
            hits += 1;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[last..]);
    (out, hits)
}

fn ends_component(c: u8) -> bool {
    matches!(
        c,
        b'\\' | b'/' | b'"' | b'\'' | b';' | b',' | b'|' | b'<' | b'>' | b']' | b')' | b'}' | b'='
    ) || c.is_ascii_whitespace()
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = r"C:\Users\Alice";
    const NEW: &str = r"C:\Users\sam";

    #[test]
    fn rewrites_json_gitconfig_and_plain() {
        let json = r#"{"python.defaultInterpreterPath": "c:\\users\\alice\\venv\\python.exe", "x": "C:\\Users\\Alice2"}"#;
        let (out, n) = rewrite_text(json, OLD, NEW);
        assert_eq!(n, 1);
        assert!(
            out.contains(r#""C:\\Users\\sam\\venv\\python.exe""#),
            "{out}"
        );
        assert!(out.contains(r"C:\\Users\\Alice2"));
        let git = "[core]\n\texcludesfile = C:/Users/Alice/.gitignore_global\n";
        assert_eq!(
            rewrite_text(git, OLD, NEW).0,
            "[core]\n\texcludesfile = C:/Users/sam/.gitignore_global\n"
        );
        assert_eq!(
            rewrite_text(r"cd C:\Users\Alice", OLD, NEW).0,
            r"cd C:\Users\sam"
        );
        assert_eq!(rewrite_text(r"XC:\Users\Alice\a", OLD, NEW).1, 0);
    }

    #[test]
    fn keeps_encoding_and_ignores_binary() {
        let mut u16le = vec![0xFF, 0xFE];
        u16le.extend(
            r"p=C:\Users\Alice\x"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        let (out, n) = rewrite_bytes(&u16le, OLD, NEW).unwrap();
        assert_eq!(n, 1);
        assert_eq!(&out[..2], &[0xFF, 0xFE]);
        let bom = [&[0xEF, 0xBB, 0xBF][..], br"C:\Users\Alice"].concat();
        assert_eq!(
            rewrite_bytes(&bom, OLD, NEW).unwrap().0,
            [&[0xEF, 0xBB, 0xBF][..], br"C:\Users\sam"].concat()
        );
        assert!(rewrite_bytes(b"C:\\Users\\Alice\0\x01", OLD, NEW).is_none());
        assert!(rewrite_bytes(b"nothing here", OLD, NEW).is_none());
    }
}
