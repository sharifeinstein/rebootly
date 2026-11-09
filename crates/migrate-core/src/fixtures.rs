//! Deterministic fake Windows volumes for tests (feature `fixtures`).
//!
//! Regenerate the committed copy with:
//! `cargo run -p migrate-core --example gen_fixtures --features fixtures`
//!
//! These are synthetic hives. Real Win10/Win11/Entra hives from VMs should be
//! added alongside them (see PROGRESS.md known issues).

use std::io;
use std::path::Path;

use crate::folders::{SHELL_FOLDERS, USER_SHELL_FOLDERS};
use crate::hive::writer::HiveWriter;
use crate::profiles::PROFILE_LIST;

pub const DOM: &str = "S-1-5-21-1111111111-2222222222-3333333333";
pub const CAROL_SID: &str = "S-1-12-1-1234567890-1234567890-1234567890-1234567890";

fn put(root: &Path, rel: &str, bytes: &[u8]) -> io::Result<()> {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().expect("relative path has a parent"))?;
    std::fs::write(p, bytes)
}

fn note(root: &Path, rel: &str) -> io::Result<()> {
    put(root, rel, format!("fixture file: {rel}\n").as_bytes())
}

fn software() -> HiveWriter {
    let mut w = HiveWriter::new("ROOT");
    w.key(r"Microsoft\Windows NT\CurrentVersion")
        .sz("ProductName", "Windows 10 Pro")
        .sz("DisplayVersion", "24H2")
        .sz("EditionID", "Professional")
        .sz("CurrentBuildNumber", "26100")
        .dword("UBR", 4351)
        .sz("SystemRoot", r"C:\WINDOWS");
    w.key(r"Microsoft\Windows\CurrentVersion")
        .sz("ProgramFilesDir", r"C:\Program Files")
        .sz("ProgramFilesDir (x86)", r"C:\Program Files (x86)");
    w.key(PROFILE_LIST)
        .expand_sz("ProfilesDirectory", r"%SystemDrive%\Users")
        .expand_sz("Public", r"%SystemDrive%\Users\Public")
        .expand_sz("Default", r"%SystemDrive%\Users\Default")
        .expand_sz("ProgramData", r"%SystemDrive%\ProgramData");
    let profiles: [(&str, &str); 9] = [
        ("S-1-5-18", r"%systemroot%\system32\config\systemprofile"),
        ("S-1-5-19", r"%systemroot%\ServiceProfiles\LocalService"),
        ("S-1-5-20", r"%systemroot%\ServiceProfiles\NetworkService"),
        ("-1001", r"%SystemDrive%\Users\Alice"),
        ("-1001.bak", r"C:\Users\TEMP"),
        ("-1002", r"C:\Users\bob"),
        ("-1003", r"C:\Users\dave"),
        ("-1004", r"C:\Users\erin"),
        (CAROL_SID, r"C:\Users\carol.CONTOSO"),
    ];
    for (sid, path) in profiles {
        let sid = if sid.starts_with('-') {
            format!("{DOM}{sid}")
        } else {
            sid.to_owned()
        };
        w.key(&format!(r"{PROFILE_LIST}\{sid}"))
            .expand_sz("ProfileImagePath", path)
            .dword("Flags", 0);
    }
    w
}

fn system() -> HiveWriter {
    let mut w = HiveWriter::new("ROOT");
    w.key("Select").dword("Current", 1).dword("Default", 1);
    w.key(r"ControlSet001\Control\ComputerName\ComputerName")
        .sz("ComputerName", "OLD-PC");
    w.key(r"ControlSet001\Control\Session Manager\Environment")
        .expand_sz("TEMP", r"%SystemRoot%\TEMP")
        .sz("OS", "Windows_NT");
    w
}

fn alice() -> HiveWriter {
    let mut w = HiveWriter::new("ROOT");
    w.key(USER_SHELL_FOLDERS)
        .expand_sz("Desktop", r"%USERPROFILE%\Desktop")
        .expand_sz("Personal", r"%USERPROFILE%\Documents")
        .expand_sz(
            "{374DE290-123F-4565-9164-39C4925E467B}",
            r"%USERPROFILE%\Downloads",
        )
        .expand_sz("AppData", r"%USERPROFILE%\AppData\Roaming");
    w
}

fn bob() -> HiveWriter {
    let mut w = HiveWriter::new("ROOT");
    w.key("Environment")
        .sz("OneDrive", r"C:\Users\bob\OneDrive");
    w.key(r"Software\Microsoft\IdentityCRL\UserExtendedProperties\bob@outlook.com");
    w.key(USER_SHELL_FOLDERS)
        .expand_sz("Desktop", r"%OneDrive%\Desktop")
        .expand_sz("Personal", r"%OneDrive%\Documents")
        .expand_sz("My Pictures", r"D:\Pictures");
    w
}

fn carol() -> HiveWriter {
    let mut w = HiveWriter::new("ROOT");
    w.key(USER_SHELL_FOLDERS)
        .expand_sz("Desktop", r"%USERPROFILE%\Desktop")
        .expand_sz("Personal", r"\\nas01\home\carol\Documents");
    w.key(SHELL_FOLDERS).sz(
        "{374DE290-123F-4565-9164-39C4925E467B}",
        r"C:\Users\carol.CONTOSO\Downloads",
    );
    w
}

/// Build the `win11-basic` volume under `root`:
/// - alice: local, profile path case differs from disk, clean hive + empty logs
/// - bob: Microsoft account, OneDrive KFM, Pictures on D:
/// - carol: Entra ID, Documents on a NAS, Downloads only in `Shell Folders`
/// - dave: dirty hive (pending `.LOG1` entry)
/// - erin: profile folder without NTUSER.DAT
/// - system SIDs and a `.bak` duplicate, all skipped
pub fn build_win11_basic(root: &Path) -> io::Result<()> {
    let cfg = "Windows/System32/config";
    put(root, &format!("{cfg}/SOFTWARE"), &software().to_bytes())?;
    put(root, &format!("{cfg}/SYSTEM"), &system().to_bytes())?;

    put(root, "Users/alice/NTUSER.DAT", &alice().to_bytes())?;
    put(root, "Users/alice/ntuser.dat.LOG1", &[0u8; 4096])?;
    put(root, "Users/alice/ntuser.dat.LOG2", &[])?;
    for f in [
        "Desktop/todo.txt",
        "Documents/report.txt",
        "Downloads/setup-notes.txt",
    ] {
        note(root, &format!("Users/alice/{f}"))?;
    }

    put(root, "Users/bob/NTUSER.DAT", &bob().to_bytes())?;
    for f in [
        "OneDrive/Desktop/a.txt",
        "OneDrive/Documents/b.txt",
        "Music/song.txt",
    ] {
        note(root, &format!("Users/bob/{f}"))?;
    }

    put(root, "Users/carol.CONTOSO/NTUSER.DAT", &carol().to_bytes())?;
    note(root, "Users/carol.CONTOSO/Downloads/c.txt")?;

    let mut dave = alice();
    dave.sequences(5, 5);
    put(root, "Users/dave/NTUSER.DAT", &dave.to_bytes())?;
    put(
        root,
        "Users/dave/NTUSER.DAT.LOG1",
        &dave.pending_log_bytes(5),
    )?;
    note(root, "Users/dave/Desktop/d.txt")?;

    note(root, "Users/erin/Desktop/e.txt")?;
    note(root, "Users/Public/Documents/shared.txt")?;
    Ok(())
}
