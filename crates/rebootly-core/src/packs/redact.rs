//! Stripping saved passwords out of an app's config files (pack `redact:`).
//! The plain backup gets the file without them; with a passphrase, the
//! untouched file also goes into the encrypted bundle (SPEC §6).
//!
//! Three kinds, all plain byte edits that keep everything else as it was:
//! - `xml`: remove whole elements, selected by name (`Pass`) or by name and
//!   one attribute (`Setting[name=*password*]`, value is a glob).
//! - `ini`: remove `key=value` lines whose key is listed (case-insensitive).
//! - `json`: blank the string value of the listed keys (`redact_json`).
//!
//! `file` may use `*` / `?` within a segment (`basic/profiles/*/service.json`).

use serde::{Deserialize, Serialize};

use super::glob::Glob;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redact {
    /// File relative to the pack path (`/`-separated); omitted when the
    /// pack path is the file itself.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub xml: Vec<String>,
    #[serde(default)]
    pub ini: Vec<String>,
    #[serde(default)]
    pub json: Vec<String>,
}

/// `Name` or `Name[attr=glob]`.
#[derive(Debug, Clone)]
struct XmlSel {
    name: String,
    attr: Option<(String, Glob)>,
}

fn parse_sel(s: &str) -> Result<XmlSel, String> {
    let bad = || format!("xml selector {s:?} must be `Name` or `Name[attr=value]`");
    let ok_name = |n: &str| {
        !n.is_empty()
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    };
    let (name, attr) = match s.split_once('[') {
        None => (s, None),
        Some((n, rest)) => {
            let inner = rest.strip_suffix(']').ok_or_else(bad)?;
            let (a, v) = inner.split_once('=').ok_or_else(bad)?;
            if !ok_name(a) || v.is_empty() || v.contains('/') {
                return Err(bad());
            }
            (n, Some((a.to_owned(), Glob::new(v)?)))
        }
    };
    if !ok_name(name) {
        return Err(bad());
    }
    Ok(XmlSel {
        name: name.to_owned(),
        attr,
    })
}

impl Redact {
    /// Problems with this entry (empty = valid).
    pub fn problems(&self) -> Vec<String> {
        let mut errs = Vec::new();
        if self.xml.is_empty() && self.ini.is_empty() && self.json.is_empty() {
            errs.push("needs `xml`, `ini` or `json` entries".into());
        }
        if let Some(f) = &self.file {
            let f = f.replace('\\', "/");
            if f.is_empty()
                || f.starts_with('/')
                || f.contains(':')
                || f.split('/').any(|p| p == "..")
            {
                errs.push(format!("file {f:?} must be a relative path without '..'"));
            } else if f.contains("**") {
                errs.push(format!("file {f:?}: only `*` and `?` within a segment"));
            }
        }
        for s in &self.xml {
            if let Err(e) = parse_sel(s) {
                errs.push(e);
            }
        }
        if self
            .ini
            .iter()
            .any(|k| k.trim().is_empty() || k.contains('='))
        {
            errs.push("ini keys must be non-empty and contain no '='".into());
        }
        if self
            .json
            .iter()
            .any(|k| k.is_empty() || k.contains(['"', '\\']))
        {
            errs.push("json keys must be non-empty and contain no quotes or backslashes".into());
        }
        errs
    }

    /// The file's text without the secrets, and how many were removed.
    pub fn apply(&self, data: &[u8]) -> (Vec<u8>, usize) {
        let mut out = data.to_vec();
        let mut n = 0;
        for s in self.xml.iter().filter_map(|s| parse_sel(s).ok()) {
            n += strip_xml(&mut out, &s);
        }
        if !self.ini.is_empty() {
            n += strip_ini(&mut out, &self.ini);
        }
        if !self.json.is_empty() {
            n += super::redact_json::blank(&mut out, &self.json);
        }
        (out, n)
    }
}

fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// End of the start tag beginning at `at` (index after `>`), honouring quotes.
fn tag_end(d: &[u8], at: usize) -> Option<usize> {
    let mut quote = None;
    for (i, &c) in d.iter().enumerate().skip(at) {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(c),
            (None, b'>') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// Value of attribute `name` in a start tag's text.
fn attr_value(tag: &[u8], name: &str) -> Option<String> {
    let t = String::from_utf8_lossy(tag);
    let mut rest = t.as_ref();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim_end();
        let key = key.rsplit(|c: char| c.is_whitespace() || c == '<').next()?;
        let after = rest[eq + 1..].trim_start();
        let q = after.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let close = after[1..].find(q)? + 1;
        if key.eq_ignore_ascii_case(name) {
            return Some(after[1..close].to_owned());
        }
        rest = &after[close + 1..];
    }
    None
}

/// Widen `[s, e)` to whole lines when the element sits alone on its line.
fn whole_line(d: &[u8], s: usize, e: usize) -> (usize, usize) {
    let ls = d[..s]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |i| i + 1);
    let le = d[e..].iter().position(|&c| c == b'\n').map(|i| e + i + 1);
    let blank = |r: &[u8]| r.iter().all(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n'));
    match le {
        Some(le) if blank(&d[ls..s]) && blank(&d[e..le]) => (ls, le),
        _ => (s, e),
    }
}

fn strip_xml(d: &mut Vec<u8>, sel: &XmlSel) -> usize {
    let open = format!("<{}", sel.name).into_bytes();
    let close = format!("</{}", sel.name).into_bytes();
    let (mut i, mut n) = (0, 0);
    while i + open.len() < d.len() {
        let boundary = matches!(
            d[i + open.len()],
            b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/'
        );
        if !(boundary && eq_ci(&d[i..i + open.len()], &open)) {
            i += 1;
            continue;
        }
        let Some(te) = tag_end(d, i) else { break };
        let wanted = sel
            .attr
            .as_ref()
            .is_none_or(|(a, g)| attr_value(&d[i..te], a).is_some_and(|v| g.matches_name(&v)));
        if !wanted {
            i = te;
            continue;
        }
        let end = if d[te - 2] == b'/' {
            Some(te)
        } else {
            (te..d.len().saturating_sub(close.len()))
                .find(|&j| eq_ci(&d[j..j + close.len()], &close))
                .and_then(|j| tag_end(d, j))
        };
        let Some(end) = end else { break };
        let (s, e) = whole_line(d, i, end);
        d.drain(s..e);
        n += 1;
        i = s;
    }
    n
}

fn strip_ini(d: &mut Vec<u8>, keys: &[String]) -> usize {
    let mut out = Vec::with_capacity(d.len());
    let mut n = 0;
    for line in d.split_inclusive(|&c| c == b'\n') {
        let t = String::from_utf8_lossy(line);
        let secret = t.split_once('=').is_some_and(|(k, _)| {
            let k = k.trim();
            keys.iter().any(|x| x.trim().eq_ignore_ascii_case(k))
        });
        if secret {
            n += 1;
        } else {
            out.extend_from_slice(line);
        }
    }
    *d = out;
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(xml: &[&str], ini: &[&str]) -> Redact {
        Redact {
            file: None,
            xml: xml.iter().map(|s| s.to_string()).collect(),
            ini: ini.iter().map(|s| s.to_string()).collect(),
            json: Vec::new(),
        }
    }

    #[test]
    fn xml_elements_are_removed_with_their_line() {
        let src = "<Servers>\r\n  <Server>\r\n    <Host>ftp.example.org</Host>\r\n    \
                   <Pass encoding=\"base64\">c2VjcmV0</Pass>\r\n    <Logontype>1</Logontype>\r\n  \
                   </Server>\r\n  <Server><Host>b</Host><Pass/></Server>\r\n</Servers>\r\n";
        let (out, n) = r(&["Pass"], &[]).apply(src.as_bytes());
        let out = String::from_utf8(out).unwrap();
        assert_eq!(n, 2);
        assert!(!out.contains("c2VjcmV0") && !out.contains("<Pass"), "{out}");
        assert!(out.contains("    <Host>ftp.example.org</Host>\r\n    <Logontype>1</Logontype>"));
        assert!(out.contains("<Server><Host>b</Host></Server>"));
        // <Password> is a different element, <PassX> too.
        let (_, n) = r(&["Pass"], &[]).apply(b"<Password>x</Password><PassX>y</PassX>");
        assert_eq!(n, 0);
    }

    #[test]
    fn xml_attribute_selector_uses_a_glob() {
        let src = "<Settings>\n<Setting name=\"Proxy host\">h</Setting>\n\
                   <Setting name=\"Proxy password\">p1</Setting>\n\
                   <Setting name='FTP Proxy password' sensitive=\"1\">p2</Setting>\n</Settings>\n";
        let (out, n) = r(&["Setting[name=*password*]"], &[]).apply(src.as_bytes());
        let out = String::from_utf8(out).unwrap();
        assert_eq!(n, 2, "{out}");
        assert_eq!(
            out,
            "<Settings>\n<Setting name=\"Proxy host\">h</Setting>\n</Settings>\n"
        );
    }

    #[test]
    fn ini_lines_are_removed() {
        let src = "[Sessions\\a]\r\nHostName=h\r\nPassword=A35C\r\n  proxypassword = x\r\nPasswordX=1\r\n";
        let (out, n) = r(&[], &["Password", "ProxyPassword"]).apply(src.as_bytes());
        assert_eq!(n, 2);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "[Sessions\\a]\r\nHostName=h\r\nPasswordX=1\r\n"
        );
    }

    #[test]
    fn bad_entries_are_reported() {
        assert!(r(&[], &[]).problems()[0].contains("needs"));
        assert!(!r(&["Setting[name]"], &[]).problems().is_empty());
        assert!(!r(&["<Pass>"], &[]).problems().is_empty());
        assert!(!r(&[], &["a=b"]).problems().is_empty());
        let mut x = r(&["Pass"], &[]);
        x.file = Some("../x.xml".into());
        assert!(!x.problems().is_empty());
        x.file = Some("sub/sitemanager.xml".into());
        assert!(x.problems().is_empty());
        x.file = Some("profiles/*/service.json".into());
        assert!(x.problems().is_empty());
        x.file = Some("**/service.json".into());
        assert!(!x.problems().is_empty());
        let mut j = r(&[], &[]);
        j.json = vec!["key".into()];
        assert!(j.problems().is_empty());
        j.json = vec!["a\"b".into()];
        assert!(!j.problems().is_empty());
    }
}
