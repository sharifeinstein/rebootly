//! Merging instead of overwriting, for files where both PCs' entries matter:
//! the hosts file (SPEC §4 Tier 4: "diff and merge") and VPN phonebooks
//! (`rasphone.pbk`, INI sections, one per connection). Works on bytes so a
//! file in a legacy code page comes out unchanged apart from the additions.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeKind {
    Hosts,
    Phonebook,
}

impl MergeKind {
    /// Merged contents, or `None` when nothing from `old` is missing.
    pub fn merge(self, current: &[u8], old: &[u8], note: &str) -> Option<(Vec<u8>, usize)> {
        match self {
            MergeKind::Hosts => merge_hosts(current, old, note),
            MergeKind::Phonebook => merge_phonebook(current, old),
        }
    }
}

fn lines(b: &[u8]) -> impl Iterator<Item = &[u8]> {
    b.split(|&c| c == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
}

fn lower(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_lowercase()
}

/// (address, name) pairs of a hosts line, ignoring comments.
fn host_pairs(line: &[u8]) -> Vec<(String, String)> {
    let l = lower(line);
    let l = l.split('#').next().unwrap_or("");
    let mut it = l.split_whitespace();
    let Some(addr) = it.next() else {
        return Vec::new();
    };
    it.map(|n| (addr.to_owned(), n.to_owned())).collect()
}

fn line_end(b: &[u8]) -> &'static [u8] {
    if b.windows(2).any(|w| w == b"\r\n") || b.is_empty() {
        b"\r\n"
    } else {
        b"\n"
    }
}

fn merge_hosts(current: &[u8], old: &[u8], note: &str) -> Option<(Vec<u8>, usize)> {
    let have: std::collections::HashSet<(String, String)> =
        lines(current).flat_map(host_pairs).collect();
    let missing: Vec<&[u8]> = lines(old)
        .filter(|l| {
            let p = host_pairs(l);
            !p.is_empty() && p.iter().any(|x| !have.contains(x))
        })
        .collect();
    if missing.is_empty() {
        return None;
    }
    let nl = line_end(current);
    let mut out = current.to_vec();
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(nl);
    }
    out.extend_from_slice(nl);
    out.extend_from_slice(format!("# Added by Rebootly restore ({note})").as_bytes());
    out.extend_from_slice(nl);
    for l in &missing {
        out.extend_from_slice(l);
        out.extend_from_slice(nl);
    }
    Some((out, missing.len()))
}

/// Sections of an INI-style file: (lowercased name, raw bytes incl. header).
fn sections(b: &[u8]) -> Vec<(String, Vec<u8>)> {
    let nl = line_end(b);
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for l in lines(b) {
        let t = lower(l);
        let t = t.trim();
        if t.starts_with('[') && t.ends_with(']') {
            out.push((t[1..t.len() - 1].trim().to_owned(), Vec::new()));
        }
        if let Some((_, body)) = out.last_mut() {
            body.extend_from_slice(l);
            body.extend_from_slice(nl);
        }
    }
    out
}

fn merge_phonebook(current: &[u8], old: &[u8]) -> Option<(Vec<u8>, usize)> {
    let have: Vec<String> = sections(current).into_iter().map(|s| s.0).collect();
    let add: Vec<Vec<u8>> = sections(old)
        .into_iter()
        .filter(|(n, _)| !have.contains(n))
        .map(|s| s.1)
        .collect();
    if add.is_empty() {
        return None;
    }
    let nl = line_end(current);
    let mut out = current.to_vec();
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(nl);
    }
    for s in &add {
        if !out.is_empty() {
            out.extend_from_slice(nl);
        }
        out.extend_from_slice(s.trim_ascii_end());
        out.extend_from_slice(nl);
    }
    Some((out, add.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_adds_only_missing_entries() {
        let cur = b"# default\r\n127.0.0.1 localhost\r\n";
        let old = b"127.0.0.1 localhost\r\n10.0.0.5 nas nas.lan # home\r\n# 1.2.3.4 old\r\n";
        let (out, n) = MergeKind::Hosts.merge(cur, old, "OLD-PC").unwrap();
        assert_eq!(n, 1);
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(
            s.starts_with("# default\r\n127.0.0.1 localhost\r\n\r\n# Added by Rebootly"),
            "{s}"
        );
        assert!(s.ends_with("10.0.0.5 nas nas.lan # home\r\n"));
        assert!(MergeKind::Hosts.merge(&out, old, "x").is_none());
    }

    #[test]
    fn phonebook_adds_missing_connections() {
        let cur = b"[Work VPN]\r\nType=2\r\n";
        let old =
            b"[work vpn]\r\nType=2\r\n\r\n[Home]\r\nType=2\r\nPhoneNumber=vpn.example.org\r\n";
        let (out, n) = MergeKind::Phonebook.merge(cur, old, "").unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            String::from_utf8(out.clone()).unwrap(),
            "[Work VPN]\r\nType=2\r\n\r\n[Home]\r\nType=2\r\nPhoneNumber=vpn.example.org\r\n"
        );
        assert!(MergeKind::Phonebook.merge(&out, old, "").is_none());
        assert_eq!(
            MergeKind::Phonebook
                .merge(b"", b"[A]\nx=1\n", "")
                .unwrap()
                .0,
            b"[A]\nx=1\r\n"
        );
    }
}
