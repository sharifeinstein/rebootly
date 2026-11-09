use super::*;
use crate::packs::schema::{Cond, Mode};

const SPEC_EXAMPLE: &str = r#"
schema: 1
id: vscode
name: Visual Studio Code
detect:
  any:
    - path: "%LOCALAPPDATA%\\Programs\\Microsoft VS Code\\Code.exe"
    - uninstall_name: "Microsoft Visual Studio Code*"
winget_id: Microsoft.VisualStudioCode
paths:
  - src: "%APPDATA%\\Code\\User"
    include: ["settings.json", "keybindings.json", "snippets/**"]
    rewrite: true
  - src: "%USERPROFILE%\\.vscode\\extensions"
    mode: list_only
registry: []
requires_closed: ["Code.exe"]
os: [win10, win11]
notes: "Extensions reinstalled via `code --install-extension`."
post_restore:
  - run: "code --install-extension {each:extensions}"
    live_only: true
"#;

#[test]
fn spec_example_parses() {
    let p = parse(SPEC_EXAMPLE, "spec").unwrap();
    assert_eq!(p.pack.id, "vscode");
    assert_eq!(p.pack.version, 1);
    assert!(matches!(&p.pack.detect.any[1], Cond::UninstallName(g) if g.starts_with("Microsoft")));
    assert_eq!(p.pack.paths[1].mode, Mode::ListOnly);
    assert_eq!(p.filters[0].0.len(), 3);
    assert!(p.uninstall_globs[0].matches_name("Microsoft Visual Studio Code (User)"));
}

#[test]
fn all_builtins_are_valid_and_unique() {
    let (packs, errors) = load_all(None);
    assert!(
        errors.is_empty(),
        "{}",
        errors.iter().map(ToString::to_string).collect::<String>()
    );
    assert_eq!(packs.len(), BUILTIN.len());
}

#[test]
fn errors_are_clear_and_complete() {
    let bad = r#"
schema: 2
id: "Bad Id"
name: ""
detect: {}
paths:
  - src: "relative\\path"
    include: ["../x"]
registry: ["HKLM\\Software\\X"]
"#;
    let e = parse(bad, "bad.yaml").unwrap_err();
    let text = e.to_string();
    for needle in [
        "schema: 2",
        "id:",
        "name:",
        "detect:",
        "paths[0].src",
        "paths[0].include",
        "registry:",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    assert_eq!(e.problems.len(), 7);
}

#[test]
fn yaml_errors_have_positions_and_unknown_fields_fail() {
    let e = parse("schema: 1\nid: x\nname: X\ndetect: {any: [{always: true}]}\npaths: [{src: '%APPDATA%', bogus: 1}]\n", "t")
        .unwrap_err();
    assert!(e.problems[0].contains("bogus"), "{e}");
    let e = parse("schema: [1\n", "t").unwrap_err();
    assert!(e.problems[0].contains("line"), "{e}");
}

#[test]
fn folder_overrides_builtin_and_reports_bad_files() {
    let t = tempfile::tempdir().unwrap();
    let custom = SPEC_EXAMPLE.replace("name: Visual Studio Code", "name: VS Code (custom)");
    std::fs::write(t.path().join("my-vscode.yaml"), custom).unwrap();
    std::fs::write(t.path().join("broken.yml"), "schema: 1\n").unwrap();
    std::fs::write(t.path().join("readme.txt"), "not a pack").unwrap();
    let (packs, errors) = load_all(Some(t.path()));
    let vs = packs.iter().find(|p| p.pack.id == "vscode").unwrap();
    assert_eq!(vs.pack.name, "VS Code (custom)");
    assert!(vs.origin.ends_with("my-vscode.yaml"));
    assert_eq!(errors.len(), 1);
    assert!(errors[0].origin.ends_with("broken.yml"));
}
