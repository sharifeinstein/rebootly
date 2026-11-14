//! `json:` redaction: blank the string value of the listed keys, at any
//! depth (`"key": "secret"` → `"key": ""`). Removing the pair would need
//! comma handling; an empty value keeps the file valid JSON and the app
//! simply asks for it again. Plain byte edit, everything else is kept.

/// Blank the non-empty string values of `keys` (case-insensitive); returns
/// how many were blanked. Text that isn't JSON is left as it is.
pub(super) fn blank(d: &mut Vec<u8>, keys: &[String]) -> usize {
    let mut out = Vec::with_capacity(d.len());
    let mut n = 0;
    let mut i = 0;
    while i < d.len() {
        if d[i] != b'"' {
            out.push(d[i]);
            i += 1;
            continue;
        }
        let Some(end) = string_end(d, i) else {
            break; // unterminated string: keep the rest as it is
        };
        out.extend_from_slice(&d[i..end]);
        let key = &d[i + 1..end - 1];
        i = end;
        let colon = skip_ws(d, i);
        if d.get(colon) != Some(&b':')
            || !keys.iter().any(|k| k.as_bytes().eq_ignore_ascii_case(key))
        {
            continue;
        }
        let v = skip_ws(d, colon + 1);
        if d.get(v) != Some(&b'"') {
            continue; // number, object, null…: not a saved secret string
        }
        let Some(vend) = string_end(d, v) else {
            break;
        };
        out.extend_from_slice(&d[i..=v]);
        if vend - v > 2 {
            n += 1;
        } else {
            out.extend_from_slice(&d[v + 1..vend - 1]);
        }
        out.push(b'"');
        i = vend;
    }
    out.extend_from_slice(&d[i.min(d.len())..]);
    *d = out;
    n
}

/// Index after the closing quote of the string starting at `at`.
fn string_end(d: &[u8], at: usize) -> Option<usize> {
    let mut i = at + 1;
    while i < d.len() {
        match d[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn skip_ws(d: &[u8], mut i: usize) -> usize {
    while d.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, keys: &[&str]) -> (String, usize) {
        let mut d = src.as_bytes().to_vec();
        let keys: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
        let n = blank(&mut d, &keys);
        (String::from_utf8(d).unwrap(), n)
    }

    #[test]
    fn string_values_of_listed_keys_are_blanked_at_any_depth() {
        let src = "{\r\n  \"settings\": {\r\n    \"bwtest\": false,\r\n    \"key\": \"live_123\\\"x\",\r\n    \
                   \"server\": \"rtmp://a/key\",\r\n    \"Key\" : \"two\"\r\n  },\r\n  \"type\": \"rtmp_common\"\r\n}\r\n";
        let (out, n) = run(src, &["key"]);
        assert_eq!(n, 2, "{out}");
        assert!(!out.contains("live_123") && !out.contains("two"), "{out}");
        assert!(out.contains("\"key\": \"\",\r\n") && out.contains("\"Key\" : \"\"\r\n"));
        // A value that merely contains the key's name, and other keys, stay.
        assert!(out.contains("\"server\": \"rtmp://a/key\""));
        assert!(out.contains("\"type\": \"rtmp_common\""));
        assert_eq!(out.len(), src.len() - "live_123\\\"x".len() - "two".len());
    }

    #[test]
    fn empty_and_non_string_values_and_lookalikes_are_left_alone() {
        let src = r#"{"key":"","server_password":null,"x":"key","keys":"a","n":{"key":1}}"#;
        let (out, n) = run(src, &["key", "server_password"]);
        assert_eq!((out.as_str(), n), (src, 0));
        // Unterminated input: nothing lost.
        let (out, _) = run(r#"{"key": "abc"#, &["key"]);
        assert_eq!(out, r#"{"key": "abc"#);
    }
}
