//! Manual `%VAR%` expansion (SPEC §3.4): values come from the *source*
//! machine's hives and the user's profile path, never the running system.

use std::collections::BTreeMap;

const MAX_PASSES: usize = 8;

/// Case-insensitive variable set.
#[derive(Debug, Clone, Default)]
pub struct VarSet {
    vars: BTreeMap<String, String>, // UPPERCASE name -> raw value
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    /// Variable names that had no value (left as `%NAME%` in `text`).
    pub unresolved: Vec<String>,
}

impl VarSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) -> &mut Self {
        self.vars.insert(name.to_uppercase(), value.into());
        self
    }

    /// Set only if not already present.
    pub fn set_default(&mut self, name: &str, value: impl Into<String>) -> &mut Self {
        self.vars
            .entry(name.to_uppercase())
            .or_insert_with(|| value.into());
        self
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(&name.to_uppercase()).map(String::as_str)
    }

    /// Expand repeatedly (values may reference other variables), bounded so
    /// self-referencing values cannot loop.
    pub fn expand(&self, input: &str) -> Expanded {
        let mut text = input.to_owned();
        let mut unresolved = Vec::new();
        for _ in 0..MAX_PASSES {
            let (next, missing, changed) = self.expand_once(&text);
            text = next;
            unresolved = missing;
            if !changed {
                break;
            }
        }
        Expanded { text, unresolved }
    }

    fn expand_once(&self, s: &str) -> (String, Vec<String>, bool) {
        let mut out = String::with_capacity(s.len());
        let mut missing = Vec::new();
        let mut changed = false;
        let mut rest = s;
        while let Some(start) = rest.find('%') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('%') {
                Some(end) if end > 0 && !after[..end].contains(char::is_whitespace) => {
                    let name = &after[..end];
                    match self.get(name) {
                        Some(v) => {
                            out.push_str(v);
                            changed = true;
                        }
                        None => {
                            out.push('%');
                            out.push_str(name);
                            out.push('%');
                            if !missing
                                .iter()
                                .any(|m: &String| m.eq_ignore_ascii_case(name))
                            {
                                missing.push(name.to_owned());
                            }
                        }
                    }
                    rest = &after[end + 1..];
                }
                _ => {
                    out.push('%');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        (out, missing, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> VarSet {
        let mut v = VarSet::new();
        v.set("USERPROFILE", r"C:\Users\bob")
            .set("SystemDrive", "C:")
            .set("OneDrive", r"%UserProfile%\OneDrive")
            .set("LOOP", "%loop%");
        v
    }

    #[test]
    fn expands_case_insensitively() {
        let e = vars().expand(r"%userprofile%\Documents");
        assert_eq!(e.text, r"C:\Users\bob\Documents");
        assert!(e.unresolved.is_empty());
    }

    #[test]
    fn nested_and_unknown() {
        let e = vars().expand(r"%ONEDRIVE%\Desktop;%NOPE%\x;%nope%");
        assert_eq!(e.text, r"C:\Users\bob\OneDrive\Desktop;%NOPE%\x;%nope%");
        assert_eq!(e.unresolved, ["NOPE"]);
    }

    #[test]
    fn literal_percents_and_loops_terminate() {
        assert_eq!(vars().expand("100% done").text, "100% done");
        assert_eq!(vars().expand("50% and 60%").text, "50% and 60%");
        assert_eq!(vars().expand("%LOOP%").text, "%loop%");
        assert_eq!(vars().expand("%%").text, "%%");
    }

    #[test]
    fn set_default_keeps_existing() {
        let mut v = vars();
        v.set_default("systemdrive", "D:").set_default("NEW", "1");
        assert_eq!(v.get("SYSTEMDRIVE"), Some("C:"));
        assert_eq!(v.get("new"), Some("1"));
    }
}
