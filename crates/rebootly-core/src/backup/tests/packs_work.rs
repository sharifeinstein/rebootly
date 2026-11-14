//! Engine tests for the M9 work-app packs: Outlook, Office, JetBrains.

use super::packs_apps::backup_pack;
use super::*;

fn bodies(names: &[String]) -> Vec<(&str, &[u8])> {
    names.iter().map(|s| (s.as_str(), &b"x"[..])).collect()
}

/// Folder in the job where `pack`'s path with this template went.
fn dest_of(m: &manifest::Manifest, job: &Path, pack: &str, template: &str) -> PathBuf {
    let r = m.packs.iter().find(|r| r.id == pack).unwrap();
    let p = r
        .paths
        .iter()
        .find(|p| p.template == template)
        .unwrap_or_else(|| panic!("{template} not in {pack}"));
    job.join(
        p.dest
            .as_deref()
            .unwrap_or_else(|| panic!("{template} not copied")),
    )
}

/// The job folder from a pack folder `users/<u>/packs/<id>`.
fn job_of(pack_dir: &Path) -> PathBuf {
    pack_dir.ancestors().nth(4).unwrap().to_path_buf()
}

#[test]
fn outlook_copies_signatures_and_psts_not_osts() {
    let files = [
        "AppData/Roaming/Microsoft/Signatures/Work.htm",
        "AppData/Roaming/Microsoft/Signatures/Work.txt",
        "AppData/Roaming/Microsoft/Signatures/Work_files/image001.png",
        "AppData/Local/Microsoft/Outlook/Outlook.pst",
        "AppData/Local/Microsoft/Outlook/Archive.PST",
        "AppData/Local/Microsoft/Outlook/alice@example.org.ost",
        "AppData/Local/Microsoft/Outlook/RoamCache/Stream_x.dat",
        "AppData/Local/Microsoft/Outlook/Offline Address Books/x.oab",
    ]
    .map(String::from);
    let (_t, dir, m) = backup_pack(&bodies(&files), "outlook");
    let job = job_of(&dir);
    let sig = dest_of(&m, &job, "outlook", r"%APPDATA%\Microsoft\Signatures");
    for kept in ["Work.htm", "Work.txt", "Work_files/image001.png"] {
        assert!(sig.join(kept).is_file(), "{kept} missing");
    }
    let ol = dest_of(&m, &job, "outlook", r"%LOCALAPPDATA%\Microsoft\Outlook");
    assert!(ol.join("Outlook.pst").is_file() && ol.join("Archive.PST").is_file());
    for gone in [
        "alice@example.org.ost",
        "RoamCache",
        "Offline Address Books",
    ] {
        assert!(!ol.join(gone).exists(), "{gone} copied");
    }
    let r = m.packs.iter().find(|r| r.id == "outlook").unwrap();
    assert!(
        r.paths.iter().any(|p| p.rewrite),
        "signatures are rewritten"
    );
    assert_eq!(r.requires_closed, ["OUTLOOK.EXE"]);
}

#[test]
fn office_copies_templates_dictionaries_and_ui() {
    let ms = "AppData/Roaming/Microsoft";
    let lms = "AppData/Local/Microsoft";
    let files = [
        format!("{ms}/Templates/Normal.dotm"),
        format!("{ms}/Templates/NormalEmail.dotm"),
        format!("{ms}/Templates/Document Themes/Theme Colors/Mine.xml"),
        format!("{ms}/Templates/LiveContent/16/x.dotx"),
        format!("{ms}/Templates/~$Normal.dotm"),
        format!("{ms}/UProof/CUSTOM.DIC"),
        format!("{ms}/UProof/ExcludeDictionaryEN0409.lex"),
        format!("{ms}/UProof/CUSTOM.DIC.bin"),
        format!("{ms}/Office/MSO1033.acl"),
        format!("{ms}/Office/Recent/x.lnk"),
        format!("{lms}/Office/Word.officeUI"),
        format!("{lms}/Office/Excel.officeSL"),
        format!("{lms}/Office/OTele/x.db"),
        format!("{ms}/Excel/XLSTART/PERSONAL.XLSB"),
        format!("{ms}/Document Building Blocks/1033/16/Building Blocks.dotx"),
    ];
    let (_t, dir, m) = backup_pack(&bodies(&files), "office");
    let job = job_of(&dir);
    let d = |t: &str| dest_of(&m, &job, "office", t);
    let tpl = d(r"%APPDATA%\Microsoft\Templates");
    assert!(tpl.join("Normal.dotm").is_file());
    assert!(tpl.join("NormalEmail.dotm").is_file());
    assert!(tpl.join("Document Themes/Theme Colors/Mine.xml").is_file());
    assert!(!tpl.join("LiveContent").exists() && !tpl.join("~$Normal.dotm").exists());
    let up = d(r"%APPDATA%\Microsoft\UProof");
    assert!(up.join("CUSTOM.DIC").is_file());
    assert!(up.join("ExcludeDictionaryEN0409.lex").is_file());
    assert!(!up.join("CUSTOM.DIC.bin").exists());
    let acl = d(r"%APPDATA%\Microsoft\Office");
    assert!(acl.join("MSO1033.acl").is_file() && !acl.join("Recent").exists());
    let ui = d(r"%LOCALAPPDATA%\Microsoft\Office");
    assert_ne!(acl, ui, "two Office folders, two destinations");
    assert!(ui.join("Word.officeUI").is_file() && ui.join("Excel.officeSL").is_file());
    assert!(!ui.join("OTele").exists());
    assert!(
        d(r"%APPDATA%\Microsoft\Excel\XLSTART")
            .join("PERSONAL.XLSB")
            .is_file()
    );
    let bb = d(r"%APPDATA%\Microsoft\Document Building Blocks");
    assert!(bb.join("1033/16/Building Blocks.dotx").is_file());
    // Not installed in this volume's Word STARTUP / AddIns: nothing there.
    let r = m.packs.iter().find(|r| r.id == "office").unwrap();
    let st = r
        .paths
        .iter()
        .find(|p| p.template.ends_with(r"Word\STARTUP"))
        .unwrap();
    assert_eq!(st.status, manifest::PackPathStatus::NotFound);
}

#[test]
fn jetbrains_copies_settings_not_plugins_or_secrets() {
    let jb = "AppData/Roaming/JetBrains";
    let files = [
        format!("{jb}/IntelliJIdea2026.2/options/editor.xml"),
        format!("{jb}/IntelliJIdea2026.2/keymaps/Mine.xml"),
        format!("{jb}/IntelliJIdea2026.2/codestyles/Team.xml"),
        format!("{jb}/IntelliJIdea2026.2/templates/Java.xml"),
        format!("{jb}/IntelliJIdea2026.2/idea64.exe.vmoptions"),
        format!("{jb}/IntelliJIdea2026.2/plugins/foo/lib/foo.jar"),
        format!("{jb}/IntelliJIdea2026.2/idea.key"),
        format!("{jb}/IntelliJIdea2026.2/c.kdbx"),
        format!("{jb}/IntelliJIdea2026.2/c.pwd"),
        format!("{jb}/IntelliJIdea2026.2/port.lock"),
        format!("{jb}/PyCharm2025.3/options/ide.general.xml"),
        format!("{jb}/PyCharm2025.3/scratches/scratch.py"),
        "AppData/Local/JetBrains/IntelliJIdea2026.2/caches/x".into(),
    ];
    let (_t, dir, m) = backup_pack(&bodies(&files), "jetbrains");
    let cfg = dir.join("JetBrains");
    for kept in [
        "IntelliJIdea2026.2/options/editor.xml",
        "IntelliJIdea2026.2/keymaps/Mine.xml",
        "IntelliJIdea2026.2/codestyles/Team.xml",
        "IntelliJIdea2026.2/templates/Java.xml",
        "IntelliJIdea2026.2/idea64.exe.vmoptions",
        "PyCharm2025.3/options/ide.general.xml",
        "PyCharm2025.3/scratches/scratch.py",
    ] {
        assert!(cfg.join(kept).is_file(), "{kept} missing");
    }
    for gone in ["plugins", "idea.key", "c.kdbx", "c.pwd", "port.lock"] {
        assert!(
            !cfg.join("IntelliJIdea2026.2").join(gone).exists(),
            "{gone} copied"
        );
    }
    assert!(!dir.join("caches").exists());
    let r = m.packs.iter().find(|r| r.id == "jetbrains").unwrap();
    let listed = r
        .paths
        .iter()
        .find(|p| p.status == manifest::PackPathStatus::Listed)
        .unwrap();
    assert_eq!(listed.listed, ["IntelliJIdea2026.2", "PyCharm2025.3"]);
}
