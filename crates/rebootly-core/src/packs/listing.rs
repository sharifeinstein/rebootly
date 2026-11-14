//! `{each:<leaf>}` in pack texts (`notes`, `backup_step`, `post_restore`):
//! the names that the `list_only` path ending in `<leaf>` listed.

use crate::backup::manifest::PackPathRecord;

use super::schema::Mode;

/// Most names written into a note; the rest are counted.
const MAX_NAMES: usize = 30;

/// Last segment of a path template (`%APPDATA%\JetBrains` → `JetBrains`;
/// a bare known folder `%KF:SavedGames%` → `SavedGames`).
pub fn leaf(template: &str) -> &str {
    let last = template
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(template);
    last.strip_prefix("%KF:")
        .and_then(|s| s.strip_suffix('%'))
        .unwrap_or(last)
}

/// The `<leaf>` of every `{each:<leaf>}` in `text`.
pub fn placeholders(text: &str) -> Vec<&str> {
    text.split("{each:")
        .skip(1)
        .filter_map(|s| s.split_once('}').map(|(v, _)| v))
        .collect()
}

/// Names listed by the `list_only` paths whose template ends in `var`
/// (case-insensitive), sorted, without duplicates (the first spelling wins).
pub fn names(paths: &[PackPathRecord], var: &str) -> Vec<String> {
    let mut set = std::collections::BTreeMap::new();
    for p in paths
        .iter()
        .filter(|p| p.mode == Mode::ListOnly && leaf(&p.template).eq_ignore_ascii_case(var))
    {
        for n in &p.listed {
            set.entry(n.to_lowercase()).or_insert_with(|| n.clone());
        }
    }
    set.into_values().collect()
}

/// `text` with each `{each:<leaf>}` replaced by the listed names
/// ("none found" when the path listed nothing or wasn't there).
pub fn fill(text: &str, paths: &[PackPathRecord]) -> String {
    let mut out = text.to_string();
    for var in placeholders(text) {
        let n = names(paths, var);
        let list = if n.is_empty() {
            "none found".to_string()
        } else {
            let mut s = n[..n.len().min(MAX_NAMES)].join(", ");
            if n.len() > MAX_NAMES {
                s.push_str(&format!(", and {} more", n.len() - MAX_NAMES));
            }
            s
        };
        out = out.replace(&format!("{{each:{var}}}"), &list);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::manifest::PackPathStatus;

    fn rec(template: &str, mode: Mode, listed: &[&str]) -> PackPathRecord {
        PackPathRecord {
            template: template.into(),
            source_path: String::new(),
            dest: None,
            mode,
            rewrite: false,
            status: PackPathStatus::Listed,
            listed: listed.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn leaf_and_placeholders() {
        assert_eq!(leaf(r"%PUBLIC%\Documents\Company Files"), "Company Files");
        assert_eq!(leaf(r"%APPDATA%\JetBrains\"), "JetBrains");
        assert_eq!(leaf("%KF:SavedGames%"), "SavedGames");
        assert_eq!(
            placeholders("a {each:My Games} b {each:x} {each:open"),
            ["My Games", "x"]
        );
    }

    #[test]
    fn fill_names_merges_paths_and_says_none_found() {
        let paths = [
            rec(
                r"%KF:Documents%\My Games",
                Mode::ListOnly,
                &["Skyrim", "Fallout 4"],
            ),
            rec(r"D:\My Games", Mode::ListOnly, &["skyrim", "Anno 1800"]),
            rec(r"%APPDATA%\My Games", Mode::Copy, &["Copied"]),
            rec(r"%KF:SavedGames%", Mode::ListOnly, &[]),
        ];
        assert_eq!(
            fill("Games: {each:my games}. Saves: {each:SavedGames}.", &paths),
            "Games: Anno 1800, Fallout 4, Skyrim. Saves: none found."
        );
        let many: Vec<String> = (0..32).map(|i| format!("g{i:02}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let text = fill("{each:x}", &[rec(r"C:\x", Mode::ListOnly, &many)]);
        assert!(text.ends_with("g29, and 2 more"), "{text}");
    }
}
