//! Custom mode's rows (folders, app settings, extras, with sizes) and the
//! selection made from the ticked ones. Shared by the TUI and the GUI.

use std::collections::BTreeSet;

use rebootly_core::backup::preflight::gib;
use rebootly_core::backup::profile::{Mode, Selection};
use rebootly_core::backup::sizes::sizes;

use crate::session::BackupCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Folders,
    Apps,
    Extras,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::Folders => "Folders",
            Section::Apps => "App settings",
            Section::Extras => "Extras",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    /// `folder:Documents`, `pack:vscode`, `system`, `public`,
    /// `root:Projects`, `sensitive`.
    pub id: String,
    pub label: String,
    pub section: Section,
    /// (files, bytes); `None` = nothing there.
    pub size: Option<(u64, u64)>,
    pub on: bool,
}

impl Row {
    /// "1.2 GiB · 345 files" or "empty".
    pub fn size_text(&self) -> String {
        match self.size {
            Some((n, b)) => format!("{} · {n} files", gib(b)),
            None => "empty".into(),
        }
    }
}

/// Rows for the chosen accounts, sized from a Full plan (slow: walks the
/// folders). Defaults: folders with something in them, every detected
/// pack, system items on; Public, drive-root folders and keys off.
pub fn rows(ctx: &BackupCtx, users: &[String]) -> anyhow::Result<Vec<Row>> {
    let mut full = Selection::new(Mode::Full);
    full.users = users.to_vec();
    let plan = ctx.plan(&full)?;
    let sz = sizes(&plan);
    let row = |id: String, label: &str, section, on: bool| {
        let size = sz.get(&id).copied();
        Row {
            label: label.to_string(),
            section,
            on,
            size,
            id,
        }
    };
    let mut out = Vec::new();
    for kf in Selection::tier1() {
        let id = format!("folder:{kf}");
        let on = sz.contains_key(&id);
        out.push(row(id, kf, Section::Folders, on));
    }
    let mut seen = BTreeSet::new();
    for p in &plan.packs {
        if seen.insert(p.id.clone()) {
            out.push(row(format!("pack:{}", p.id), &p.name, Section::Apps, true));
        }
    }
    out.push(row(
        "system".into(),
        "System items (app list, hosts, Wi-Fi, printers…)",
        Section::Extras,
        true,
    ));
    if sz.contains_key("public") {
        out.push(row(
            "public".into(),
            "Public folder (shared by all users)",
            Section::Extras,
            false,
        ));
    }
    for r in &ctx.offered_roots {
        out.push(row(
            format!("root:{r}"),
            &format!("Drive root: {r}"),
            Section::Extras,
            false,
        ));
    }
    if sz.contains_key("sensitive") {
        out.push(row(
            "sensitive".into(),
            "Keys and credentials (encrypted, needs a passphrase)",
            Section::Extras,
            false,
        ));
    }
    Ok(out)
}

/// The selection for the ticked rows.
pub fn selection(users: &[String], rows: &[Row]) -> Selection {
    let ticked = |prefix: &str| -> Vec<String> {
        rows.iter()
            .filter(|r| r.on)
            .filter_map(|r| r.id.strip_prefix(prefix).map(str::to_owned))
            .collect()
    };
    let all = |prefix: &str| rows.iter().filter(|r| r.id.starts_with(prefix)).count();
    let on = |id: &str| rows.iter().any(|r| r.id == id && r.on);
    let mut s = Selection::new(Mode::Custom);
    s.users = users.to_vec();
    let folders = ticked("folder:");
    // Nothing ticked must not mean "all": a name that matches nothing.
    s.folders = match folders.len() {
        0 => vec!["-".into()],
        n if n == all("folder:") => Vec::new(),
        _ => folders,
    };
    let packs = ticked("pack:");
    s.packs = (packs.len() != all("pack:")).then_some(packs);
    s.public = on("public");
    s.root_folders = ticked("root:");
    s.system = on("system");
    s.sensitive = on("sensitive");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, on: bool) -> Row {
        Row {
            id: id.into(),
            label: id.into(),
            section: Section::Folders,
            size: None,
            on,
        }
    }

    #[test]
    fn selection_from_rows() {
        let rows = vec![
            r("folder:Desktop", true),
            r("folder:Documents", true),
            r("pack:vscode", true),
            r("pack:chrome", false),
            r("system", false),
            r("root:Projects", true),
        ];
        let s = selection(&[], &rows);
        assert!(s.folders.is_empty(), "all folders ticked = all");
        assert_eq!(s.packs.as_deref(), Some(&["vscode".to_string()][..]));
        assert!(!s.system && !s.public);
        assert_eq!(s.root_folders, ["Projects"]);
        let none = selection(&[], &[r("folder:Desktop", false), r("folder:Music", true)]);
        assert_eq!(none.folders, ["Music"]);
        let zero = selection(&[], &[r("folder:Desktop", false)]);
        assert_eq!(zero.folders, ["-"]);
    }
}
