use super::*;
use crate::hive::writer::HiveWriter;

fn base() -> VarSet {
    let mut v = VarSet::new();
    v.set("SystemDrive", "C:").set("TEMP", r"%SystemRoot%\TEMP");
    v
}

fn find<'a>(f: &'a [FolderPath], name: &str) -> &'a FolderPath {
    f.iter().find(|x| x.name == name).unwrap()
}

#[test]
fn table_is_consistent() {
    let mut names: Vec<_> = KNOWN_FOLDERS.iter().map(|k| k.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), KNOWN_FOLDERS.len());
    for k in KNOWN_FOLDERS {
        assert!(k.guid.len() == 38 && k.guid.starts_with('{'), "{}", k.name);
        assert!(!k.reg_names.is_empty());
    }
}

#[test]
fn precedence_usf_then_sf_then_default() {
    let mut w = HiveWriter::new("ROOT");
    w.key(USER_SHELL_FOLDERS)
        .expand_sz("Desktop", r"%OneDrive%\Desktop")
        .expand_sz("Personal", "") // blank → fall through
        .expand_sz("My Music", r"%NOSUCH%\Music");
    w.key(SHELL_FOLDERS)
        .sz("Personal", r"C:\Users\bob\Docs")
        .sz("Desktop", r"C:\stale");
    w.key("Environment")
        .sz("OneDrive", r"%USERPROFILE%\OneDrive");
    let h = crate::hive::Hive::from_bytes(w.to_bytes()).unwrap();

    let vars = user_vars(&base(), Some(&h), r"C:\Users\bob").unwrap();
    let f = resolve_user_folders(Some(&h), &vars, r"C:\Users\bob").unwrap();

    let d = find(&f, "Desktop");
    assert_eq!(d.win_path, r"C:\Users\bob\OneDrive\Desktop");
    assert_eq!(d.origin, FolderOrigin::UserShellFolders);

    let docs = find(&f, "Documents");
    assert_eq!(docs.win_path, r"C:\Users\bob\Docs");
    assert_eq!(docs.origin, FolderOrigin::ShellFolders);

    let pics = find(&f, "Pictures");
    assert_eq!(pics.win_path, r"C:\Users\bob\Pictures");
    assert_eq!(pics.origin, FolderOrigin::Default);

    assert_eq!(find(&f, "Music").unresolved_vars, ["NOSUCH"]);
    assert_eq!(
        find(&f, "LocalAppData").win_path,
        r"C:\Users\bob\AppData\Local"
    );
}

#[test]
fn guid_alias_used_when_canonical_missing() {
    let mut w = HiveWriter::new("ROOT");
    w.key(USER_SHELL_FOLDERS).expand_sz(
        "{F42EE2D3-909F-4907-8871-4C22FC0BF756}",
        r"%USERPROFILE%\LocalDocs",
    );
    let h = crate::hive::Hive::from_bytes(w.to_bytes()).unwrap();
    let vars = user_vars(&base(), Some(&h), r"C:\Users\x").unwrap();
    let f = resolve_user_folders(Some(&h), &vars, r"C:\Users\x").unwrap();
    let docs = find(&f, "Documents");
    assert_eq!(docs.win_path, r"C:\Users\x\LocalDocs");
    assert_eq!(
        docs.reg_name,
        Some("{F42EE2D3-909F-4907-8871-4C22FC0BF756}")
    );
}

#[test]
fn missing_hive_gives_defaults_and_user_vars() {
    let vars = user_vars(&base(), None, r"C:\Users\eve\").unwrap();
    assert_eq!(vars.get("HOMEPATH"), Some(r"\Users\eve"));
    assert_eq!(vars.get("username"), Some("eve"));
    let f = resolve_user_folders(None, &vars, r"C:\Users\eve").unwrap();
    assert!(f.iter().all(|x| x.origin == FolderOrigin::Default));
    assert_eq!(find(&f, "SavedGames").win_path, r"C:\Users\eve\Saved Games");
}

#[test]
fn profile_vars_override_user_environment() {
    let mut w = HiveWriter::new("ROOT");
    w.key("Environment")
        .sz("USERPROFILE", r"C:\evil")
        .sz("TEMP", r"%USERPROFILE%\T");
    let h = crate::hive::Hive::from_bytes(w.to_bytes()).unwrap();
    let vars = user_vars(&base(), Some(&h), r"C:\Users\bob").unwrap();
    assert_eq!(vars.get("USERPROFILE"), Some(r"C:\Users\bob"));
    assert_eq!(vars.expand("%TEMP%").text, r"C:\Users\bob\T");
}

#[test]
fn cross_check_reports_only_real_differences() {
    let f = resolve_user_folders(None, &base(), r"C:\Users\bob").unwrap();
    let docs = find(&f, "Documents").guid;
    let desk = find(&f, "Desktop").guid;
    let warn = cross_check(&f, |g| {
        if g == docs {
            Some(r"C:\Users\bob\OneDrive\Documents".into())
        } else if g == desk {
            Some(r"c:\users\BOB\Desktop\".into()) // same, other casing
        } else {
            None
        }
    });
    assert_eq!(warn.len(), 1, "{warn:?}");
    assert!(warn[0].starts_with("Documents: registry says"));
}
