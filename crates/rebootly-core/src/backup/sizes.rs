//! Sizes per selectable item (Custom mode): Tier 1 folder, app pack,
//! Public, drive-root folder, system items, keys/credentials.

use std::collections::BTreeMap;

use super::Plan;

/// Row id for a planned file's destination (`folder:Documents`,
/// `pack:vscode`, `public`, `root:Projects`, `system`); raw hives have none
/// (always included).
pub fn bucket(dest: &str) -> Option<String> {
    let p: Vec<&str> = dest.splitn(5, '/').collect();
    match p.as_slice() {
        ["users", _, "folders", kf, ..] => Some(format!("folder:{kf}")),
        ["users", _, "packs", id, ..] => Some(format!("pack:{id}")),
        ["users", _, "registry", f] => f.strip_suffix(".reg").map(|id| format!("pack:{id}")),
        ["custom", "Public", ..] => Some("public".into()),
        ["custom", label, ..] => Some(format!("root:{label}")),
        ["system", "hives", ..] => None,
        ["system", ..] => Some("system".into()),
        _ => None,
    }
}

/// (files, bytes) per row id.
pub fn sizes(plan: &Plan) -> BTreeMap<String, (u64, u64)> {
    let mut m: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut add = |id: Option<String>, size: u64| {
        if let Some(id) = id {
            let e = m.entry(id).or_default();
            e.0 += 1;
            e.1 += size;
        }
    };
    for f in &plan.files {
        add(bucket(&f.dest), f.size);
    }
    for g in &plan.generated {
        add(bucket(&g.dest), g.bytes.len() as u64);
    }
    for f in &plan.sensitive {
        add(Some("sensitive".into()), f.size);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::bucket;

    #[test]
    fn buckets() {
        assert_eq!(
            bucket("users/A/folders/Documents/x.txt").unwrap(),
            "folder:Documents"
        );
        assert_eq!(
            bucket("users/A/packs/vscode/settings.json").unwrap(),
            "pack:vscode"
        );
        assert_eq!(
            bucket("users/A/registry/windows-shell.reg").unwrap(),
            "pack:windows-shell"
        );
        assert_eq!(bucket("custom/Public/Documents/a").unwrap(), "public");
        assert_eq!(bucket("custom/Projects/a").unwrap(), "root:Projects");
        assert_eq!(bucket("system/apps.json").unwrap(), "system");
        assert!(bucket("system/hives/A/NTUSER.DAT").is_none());
    }
}
