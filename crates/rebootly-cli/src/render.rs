//! Plain-text rendering of an `InspectReport`.

use std::fmt::Write as _;
use std::path::Path;

use rebootly_core::folders::FolderOrigin;
use rebootly_core::inspect::{InspectReport, UserReport};
use rebootly_core::winpath::Mapped;

fn local(m: &Mapped, root: &Path) -> String {
    match m {
        Mapped::Local { path, exists } => {
            let shown = path
                .strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string();
            if *exists {
                shown
            } else {
                format!("{shown}  (not found)")
            }
        }
        Mapped::OtherDrive { drive } => format!("(on drive {drive}:, not mounted)"),
        Mapped::Network => "(network path, not followed)".into(),
        Mapped::Unmappable => "(cannot map path)".into(),
    }
}

fn origin(o: FolderOrigin) -> &'static str {
    match o {
        FolderOrigin::UserShellFolders => "registry",
        FolderOrigin::ShellFolders => "legacy key",
        FolderOrigin::Default => "default",
    }
}

pub fn text(r: &InspectReport, root: &Path) -> String {
    let mut s = String::new();
    let os = &r.os;
    let build = match (os.build, os.ubr) {
        (Some(b), Some(u)) => format!(" (build {b}.{u})"),
        (Some(b), None) => format!(" (build {b})"),
        _ => String::new(),
    };
    let _ = writeln!(
        s,
        "{} {}{}  computer {}",
        r.os_name,
        os.display_version.as_deref().unwrap_or(""),
        build,
        os.computer_name.as_deref().unwrap_or("?")
    );
    let _ = writeln!(
        s,
        "Volume: {}   running in: {}",
        root.display(),
        r.environment.as_str()
    );
    for (letter, at) in &r.other_drives {
        let _ = writeln!(s, "Drive {letter}: of that Windows is mounted at {}", at.display());
    }
    for n in &r.notes {
        let _ = writeln!(s, "Note: {n}");
    }
    for u in &r.users {
        user(&mut s, u, root);
    }
    if !r.skipped.is_empty() {
        let _ = writeln!(s, "\nSkipped profiles:");
        for k in &r.skipped {
            let _ = writeln!(s, "  {}  ({})", k.sid, k.reason);
        }
    }
    s
}

fn user(s: &mut String, u: &UserReport, root: &Path) {
    let p = &u.profile;
    let _ = writeln!(s, "\nUser {}  [{}]  {}", p.username, p.kind.label(), p.sid);
    let _ = writeln!(
        s,
        "  Profile  {}  ->  {}",
        p.profile_path,
        local(&p.local, root)
    );
    let hive = match &p.hive {
        Some(h) if h.state.is_dirty() => "DIRTY (unapplied logs; base hive read)",
        Some(_) => "clean",
        None => "not available",
    };
    let _ = writeln!(s, "  Hive     {hive}");
    let w = u
        .folders
        .iter()
        .map(|f| f.path.win_path.len())
        .max()
        .unwrap_or(0);
    for f in &u.folders {
        let od = if f.onedrive { " [OneDrive]" } else { "" };
        let _ = writeln!(
            s,
            "  {:<15}{:<11}{:<w$}  {}{}",
            f.path.name,
            origin(f.path.origin),
            f.path.win_path,
            local(&f.local, root),
            od,
        );
        if !f.path.unresolved_vars.is_empty() {
            let _ = writeln!(
                s,
                "      ! unresolved variables: {}",
                f.path.unresolved_vars.join(", ")
            );
        }
    }
    for warn in &p.warnings {
        let _ = writeln!(s, "  ! {warn}");
    }
}
