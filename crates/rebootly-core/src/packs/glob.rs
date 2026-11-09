//! Tiny case-insensitive glob matcher for pack `include`/`exclude` and
//! `uninstall_name`. Paths are `/`-separated and relative to the pack path.
//! `*` and `?` match within one segment; `**` matches any number of segments.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    segments: Vec<String>, // uppercase
}

impl Glob {
    pub fn new(pattern: &str) -> Result<Self, String> {
        let p = pattern.trim().replace('\\', "/");
        if p.is_empty() {
            return Err("empty pattern".into());
        }
        if p.starts_with('/') || p.split('/').any(|s| s == "..") {
            return Err(format!(
                "pattern {pattern:?} must be relative and must not contain '..'"
            ));
        }
        let segments: Vec<String> = p
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_uppercase)
            .collect();
        if segments.iter().any(|s| s.contains("**") && s != "**") {
            return Err(format!(
                "pattern {pattern:?}: '**' must be a whole path segment"
            ));
        }
        Ok(Glob { segments })
    }

    /// Match a `/`-separated relative path.
    pub fn matches(&self, path: &str) -> bool {
        let parts: Vec<String> = path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_uppercase)
            .collect();
        match_segs(&self.segments, &parts)
    }

    /// Match a single name (no separators), e.g. an uninstall DisplayName.
    pub fn matches_name(&self, name: &str) -> bool {
        self.segments.len() == 1
            && seg_match(self.segments[0].as_bytes(), name.to_uppercase().as_bytes())
    }
}

fn match_segs(pat: &[String], path: &[String]) -> bool {
    match pat.first() {
        None => path.is_empty(),
        Some(p) if p == "**" => (0..=path.len()).any(|i| match_segs(&pat[1..], &path[i..])),
        Some(p) => {
            !path.is_empty()
                && seg_match(p.as_bytes(), path[0].as_bytes())
                && match_segs(&pat[1..], &path[1..])
        }
    }
}

/// Wildcard match on bytes; iterative with backtracking on the last `*`.
fn seg_match(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool {
        Glob::new(p).unwrap().matches(s)
    }

    #[test]
    fn segments_and_double_star() {
        assert!(m("settings.json", "Settings.JSON"));
        assert!(!m("settings.json", "sub/settings.json"));
        assert!(m("snippets/**", "snippets/a/b.json"));
        assert!(m("snippets/**", "snippets"));
        assert!(m("**/*.xml", "config.xml"));
        assert!(m("**/*.xml", "a/b/c.xml"));
        assert!(m("Profiles/*/prefs.js", "Profiles/x.default/prefs.js"));
        assert!(!m("Profiles/*/prefs.js", "Profiles/x/y/prefs.js"));
        assert!(m("a?c*", "abcdef"));
        assert!(!m("a?c", "ac"));
    }

    #[test]
    fn names_and_errors() {
        let g = Glob::new("Microsoft Visual Studio Code*").unwrap();
        assert!(g.matches_name("Microsoft Visual Studio Code (User)"));
        assert!(!g.matches_name("Visual Studio Code"));
        assert!(Glob::new("").is_err());
        assert!(Glob::new("/abs").is_err());
        assert!(Glob::new("../up").is_err());
        assert!(Glob::new("a**b").is_err());
    }
}
