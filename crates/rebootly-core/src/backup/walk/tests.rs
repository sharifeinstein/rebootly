use super::*;

fn job(root: &Path, profile_rel: Option<Vec<String>>) -> FolderJob {
    FolderJob {
        user: "u".into(),
        folder: "Documents".into(),
        src_root: root.to_path_buf(),
        win_root: r"C:\Users\u\Documents".into(),
        profile_rel,
        dest_root: "users/u/folders/Documents".into(),
        priority: 1,
        filter: None,
    }
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

#[test]
fn walks_sorted_and_excludes() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(&r.join("b.txt"));
    touch(&r.join("a/z.txt"));
    touch(&r.join("a/Thumbs.db"));
    touch(&r.join("a/desktop.ini"));
    touch(&r.join("Cache/kept.txt")); // not under AppData: kept
    let w = walk(
        &job(r, Some(vec!["Documents".into()])),
        &SkipSet::default(),
        ExcludeOptions::default(),
    );
    let dests: Vec<_> = w.files.iter().map(|f| f.dest.as_str()).collect();
    assert_eq!(
        dests,
        [
            "users/u/folders/Documents/b.txt",
            "users/u/folders/Documents/Cache/kept.txt",
            "users/u/folders/Documents/a/desktop.ini",
            "users/u/folders/Documents/a/z.txt",
        ]
    );
    assert_eq!(w.excluded.get(&Category::ThumbnailCache), Some(&(1, 1)));
    assert_eq!(w.files[3].src_win, r"C:\Users\u\Documents\a\z.txt");
}

#[test]
fn appdata_context_applies_from_profile_rel() {
    let t = tempfile::tempdir().unwrap();
    touch(&t.path().join("Google/Chrome/User Data/Default/Login Data"));
    touch(&t.path().join("Google/Chrome/User Data/Default/Bookmarks"));
    let rel = Some(vec!["AppData".into(), "Local".into()]);
    let w = walk(
        &job(t.path(), rel),
        &SkipSet::default(),
        ExcludeOptions::default(),
    );
    assert_eq!(w.files.len(), 1);
    assert!(w.files[0].dest.ends_with("Bookmarks"));
    assert_eq!(w.excluded.get(&Category::Secrets), Some(&(1, 1)));
}

#[test]
fn skips_nested_planned_roots() {
    let t = tempfile::tempdir().unwrap();
    touch(&t.path().join("Pictures/p.jpg"));
    touch(&t.path().join("d.txt"));
    let skip: SkipSet = [t.path().join("pictures")].into_iter().collect(); // other casing
    let w = walk(&job(t.path(), None), &skip, ExcludeOptions::default());
    assert_eq!(w.files.len(), 1);
}

#[cfg(unix)]
#[test]
fn records_but_never_follows_links() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(&r.join("real/f.txt"));
    // Junction-style loop: "My Music" -> its own parent.
    std::os::unix::fs::symlink(r, r.join("real/My Music")).unwrap();
    std::os::unix::fs::symlink(r.join("real/f.txt"), r.join("link.txt")).unwrap();
    let w = walk(
        &job(r, None),
        &SkipSet::default(),
        ExcludeOptions::default(),
    );
    assert_eq!(w.files.len(), 1);
    assert_eq!(w.reparse_points.len(), 2);
    assert!(w.reparse_points.iter().all(|p| p.target.is_some()));
}

#[cfg(unix)]
#[test]
fn reports_name_issues() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(&r.join("Report.txt"));
    touch(&r.join("report.TXT"));
    touch(&r.join("bad:name"));
    let w = walk(
        &job(r, None),
        &SkipSet::default(),
        ExcludeOptions::default(),
    );
    let kinds: Vec<_> = w.issues.iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&IssueKind::CaseCollision));
    assert!(kinds.contains(&IssueKind::InvalidWindowsName));
    assert_eq!(w.files.len(), 3, "issues are reported, files still planned");
    // 230 characters, 260 bytes in UTF-8: valid on NTFS, too long for ext4.
    assert!(name_too_long(&("x".repeat(200) + &"é".repeat(30))));
    assert!(!name_too_long(&"x".repeat(255)));
}

#[test]
fn unreadable_root_is_an_issue_not_a_panic() {
    let w = walk(
        &job(Path::new("/nonexistent/rebootly"), None),
        &SkipSet::default(),
        ExcludeOptions::default(),
    );
    assert!(w.files.is_empty());
    assert_eq!(w.issues[0].kind, IssueKind::Unreadable);
}

#[test]
fn pack_filters_and_single_file_roots() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(&r.join("settings.json"));
    touch(&r.join("workspaceStorage/x/state.vscdb"));
    touch(&r.join("snippets/a.json"));
    touch(&r.join("snippets/skip/b.json"));
    let g = |p: &str| Glob::new(p).unwrap();
    let mut j = job(r, None);
    j.filter = Some(PathFilter {
        include: vec![g("settings.json"), g("snippets/**")],
        exclude: vec![g("snippets/skip")],
    });
    let w = walk(&j, &SkipSet::default(), ExcludeOptions::default());
    let rels: Vec<_> = w
        .files
        .iter()
        .map(|f| f.dest.rsplit("Documents/").next().unwrap())
        .collect();
    assert_eq!(rels, ["settings.json", "snippets/a.json"]);

    let mut one = job(&r.join("settings.json"), None);
    one.dest_root = "users/u/packs/x/settings.json".into();
    let w = walk(&one, &SkipSet::default(), ExcludeOptions::default());
    assert_eq!(w.files.len(), 1);
    assert_eq!(w.files[0].dest, "users/u/packs/x/settings.json");
}

#[test]
fn cloud_placeholder_attributes() {
    assert!(is_cloud_placeholder(0x0040_0020)); // RECALL_ON_DATA_ACCESS | ARCHIVE
    assert!(is_cloud_placeholder(0x0000_1000)); // OFFLINE
    assert!(is_cloud_placeholder(0x0004_0010)); // RECALL_ON_OPEN | DIRECTORY
    assert!(!is_cloud_placeholder(0x0008_0020)); // PINNED | ARCHIVE: on the device
    assert!(!is_cloud_placeholder(0x20));
}
