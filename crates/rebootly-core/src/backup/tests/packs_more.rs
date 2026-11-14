//! Engine tests for the last M9 v1 packs: Minecraft, My Games, QuickBooks,
//! and the `{each:…}` / `backup_step` / list_only filter features they use.

use super::packs_apps::{backup_pack, write};
use super::*;
use crate::backup::manifest::PackPathStatus;

fn text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn minecraft_copies_worlds_and_settings_not_versions_or_sign_in() {
    let mc = "AppData/Roaming/.minecraft";
    let profiles = r#"{"profiles":{"a":{"name":"Latest"}},"clientToken":"ct",
        "authenticationDatabase":{"u1":{"accessToken":"secret-tok","username":"al"}}}"#;
    let bedrock = "AppData/Roaming/Minecraft Bedrock/Users";
    let uwp =
        "AppData/Local/Packages/Microsoft.MinecraftUWP_8wekyb3d8bbwe/LocalState/games/com.mojang";
    let f = |base: &str, rel: &str| format!("{base}/{rel}");
    let files: Vec<(String, &str)> = vec![
        (f(mc, "saves/New World/level.dat"), "w"),
        (f(mc, "saves/New World/region/r.0.0.mca"), "r"),
        (f(mc, "screenshots/2026-11-14.png"), "p"),
        (f(mc, "resourcepacks/Faithful.zip"), "z"),
        (f(mc, "mods/sodium.jar"), "j"),
        (f(mc, "options.txt"), "fov:0.5"),
        (f(mc, "servers.dat"), "s"),
        (f(mc, "launcher_profiles.json"), profiles),
        (f(mc, "launcher_accounts.json"), "{\"accounts\":{}}"),
        (f(mc, "versions/1.21.4/1.21.4.jar"), "v"),
        (f(mc, "libraries/x.jar"), "l"),
        (f(mc, "assets/indexes/19.json"), "a"),
        (f(mc, "logs/latest.log"), "log"),
        (
            f(
                bedrock,
                "123456/games/com.mojang/minecraftWorlds/abc/level.dat",
            ),
            "b",
        ),
        (
            f(bedrock, "123456/games/com.mojang/minecraftpe/options.txt"),
            "o",
        ),
        (
            f(bedrock, "123456/games/com.mojang/premium_cache/x.zip"),
            "m",
        ),
        (
            f(
                bedrock,
                "Shared/games/com.mojang/resource_packs/p/manifest.json",
            ),
            "{}",
        ),
        (f(uwp, "minecraftWorlds/old/level.dat"), "u"),
        (f(uwp, "premium_cache/y.zip"), "m"),
    ];
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_bytes()))
        .collect();
    let (_t, dir, m) = backup_pack(&files, "minecraft");
    let j = dir.join(".minecraft");
    for kept in [
        "saves/New World/level.dat",
        "saves/New World/region/r.0.0.mca",
        "screenshots/2026-11-14.png",
        "resourcepacks/Faithful.zip",
        "mods/sodium.jar",
        "options.txt",
        "servers.dat",
    ] {
        assert!(j.join(kept).is_file(), "{kept} missing");
    }
    for gone in [
        "launcher_accounts.json",
        "versions",
        "libraries",
        "assets",
        "logs",
    ] {
        assert!(!j.join(gone).exists(), "{gone} copied");
    }
    let lp = text(&j.join("launcher_profiles.json"));
    assert!(!lp.contains("secret-tok") && !lp.contains("\"ct\""), "{lp}");
    assert!(
        lp.contains("\"accessToken\":\"\"") && lp.contains("Latest"),
        "{lp}"
    );
    let b = dir.join("Users");
    assert!(
        b.join("123456/games/com.mojang/minecraftWorlds/abc/level.dat")
            .is_file()
    );
    assert!(
        b.join("Shared/games/com.mojang/resource_packs/p/manifest.json")
            .is_file()
    );
    assert!(!b.join("123456/games/com.mojang/premium_cache").exists());
    let u = dir.join("com.mojang");
    assert!(u.join("minecraftWorlds/old/level.dat").is_file());
    assert!(!u.join("premium_cache").exists());
    let r = m.packs.iter().find(|r| r.id == "minecraft").unwrap();
    assert_eq!(r.winget_id.as_deref(), Some("Mojang.MinecraftLauncher"));
    assert!(r.secrets.is_some(), "the sign-in token was stripped");
}

#[test]
fn my_games_lists_games_in_the_notes_and_documents_still_copies_them() {
    let files: Vec<(&str, &[u8])> = vec![
        (
            "Documents/My Games/Skyrim Special Edition/Saves/s1.ess",
            b"s",
        ),
        ("Documents/My Games/Fallout4/Fallout4Prefs.ini", b"i"),
        ("Documents/My Games/desktop.ini", b"d"),
        ("Saved Games/Hades/Profile1.sav", b"h"),
        ("Saved Games/desktop.ini", b"d"),
    ];
    let (t, _dir, m) = backup_pack(&files, "my-games");
    let r = m.packs.iter().find(|r| r.id == "my-games").unwrap();
    let notes = r.notes.as_deref().unwrap();
    assert!(
        notes.contains("Documents\\My Games: Fallout4, Skyrim Special Edition;"),
        "{notes}"
    );
    assert!(notes.contains("Saved Games: Hades."), "{notes}");
    assert!(!notes.contains("{each:") && !notes.contains("desktop.ini"));
    assert!(r.paths.iter().all(|p| p.status == PackPathStatus::Listed));
    let copied = |rel: &str| {
        walk_files(t.path())
            .iter()
            .any(|p| p.to_string_lossy().replace('\\', "/").ends_with(rel))
    };
    assert!(copied("My Games/Skyrim Special Edition/Saves/s1.ess"));
    assert!(copied("Saved Games/Hades/Profile1.sav"));
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn quickbooks_lists_company_files_and_adds_a_backup_step_for_its_user() {
    let (t, root) = volume();
    let cf = root.join("Users/Public/Documents/Intuit/QuickBooks/Company Files");
    for rel in [
        "Acme Ltd.QBW",
        "Acme Ltd.QBW.TLG",
        "Acme Ltd.QBW.ND",
        "Acme Ltd (Backup Nov 14,2026).QBB",
        "Acme Ltd Tax Form History/f.pdf",
        "Sample.qbm",
    ] {
        write(&cf, rel, b"q");
    }
    let alice = root.join("Users/alice");
    write(
        &alice,
        "AppData/Local/Intuit/QuickBooks/log/34.0/QBWin.log",
        b"l",
    );
    let src = OfflineVolume::open(&root).unwrap();
    let opts = PlanOptions {
        users: vec!["Alice".into(), "Bob".into()],
        packs: crate::packs::load::load_all(None).0,
        ..Default::default()
    };
    let p = plan(&src, &opts).unwrap();
    let qb: Vec<_> = p.packs.iter().filter(|r| r.id == "quickbooks").collect();
    assert_eq!(qb.len(), 1, "only Alice ran QuickBooks");
    let files = &qb[0].paths[0];
    assert_eq!(files.status, PackPathStatus::Listed);
    assert_eq!(
        files.listed,
        [
            "Acme Ltd (Backup Nov 14,2026).QBB",
            "Acme Ltd.QBW",
            "Sample.qbm"
        ]
    );
    assert_eq!(qb[0].paths[1].listed, ["34.0"]);
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    let step = m
        .manual_steps
        .iter()
        .find(|s| s.title == "QuickBooks Desktop (Alice): before you leave the old PC")
        .expect("backup step");
    assert!(
        step.detail.contains(
            "Company Files): Acme Ltd (Backup Nov 14,2026).QBB, Acme Ltd.QBW, Sample.qbm."
        ),
        "{}",
        step.detail
    );
    assert!(!m.manual_steps.iter().any(|s| s.title.contains("(Bob)")));
    assert!(walk_files(&job.dir).iter().all(|p| {
        let s = p.to_string_lossy().to_lowercase();
        !s.ends_with(".qbw") && !s.ends_with(".qbb")
    }));
}

#[test]
fn each_placeholders_must_name_a_list_only_path() {
    let pack = |notes: &str| {
        format!(
            "schema: 1\nid: t\nname: T\ndetect:\n  any:\n    - always: true\n\
             paths:\n  - src: \"%APPDATA%\\\\T\\\\Items\"\n    mode: list_only\n    \
             include: [\"*.x\"]\n  - src: \"%APPDATA%\\\\T\\\\Data\"\n\
             notes: \"{notes}\"\n"
        )
    };
    assert!(crate::packs::load::parse(&pack("Items: {each:items}"), "t.yaml").is_ok());
    let e = crate::packs::load::parse(&pack("{each:Data} {each:Nope}"), "t.yaml")
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("{each:Data}") && e.contains("{each:Nope}"),
        "{e}"
    );
}
