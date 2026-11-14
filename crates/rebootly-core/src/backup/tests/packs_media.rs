//! Engine tests for the M9 media/gaming packs: OBS, Adobe presets, Steam.

use super::packs_apps::{backup_pack, write};
use super::*;
use crate::backup::manifest::PackPathStatus;

fn text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn obs_copies_scenes_and_profiles_without_keys_or_tokens() {
    let obs = "AppData/Roaming/obs-studio";
    let service = "{\r\n  \"settings\": {\r\n    \"key\": \"live_123_secret\",\r\n    \
                   \"server\": \"auto\"\r\n  },\r\n  \"type\": \"rtmp_common\"\r\n}\r\n";
    let basic =
        "[General]\r\nName=Main\r\n[Twitch]\r\nName=al\r\nToken=tok\r\nRefreshToken=ref\r\n";
    let ws = r#"{"server_enabled":true,"server_password":"wspw","server_port":4455}"#;
    let global = "[General]\r\nLanguage=en-US\r\n[OBSWebSocket]\r\nServerPassword=old\r\n";
    let f = |rel: &str| format!("{obs}/{rel}");
    let files: Vec<(String, &str)> = vec![
        (f("global.ini"), global),
        (f("user.ini"), "[General]\r\n"),
        (f("basic/scenes/Main.json"), "{\"name\":\"Main\"}"),
        (f("basic/scenes/Main.json.bak"), "{}"),
        (f("basic/profiles/Main/basic.ini"), basic),
        (f("basic/profiles/Main/service.json"), service),
        (f("basic/profiles/Main/recordEncoder.json"), "{}"),
        (
            f("basic/profiles/Spare/basic.ini"),
            "[General]\r\nName=Spare\r\n",
        ),
        (f("plugin_config/obs-websocket/config.json"), ws),
        (f("plugin_config/obs-browser/Cache/data_0"), "c"),
        (f("plugin_config/rtmp-services/services.json"), "{}"),
        (f("logs/2026-11-14.txt"), "log"),
        (f("crashes/x.txt"), "crash"),
    ];
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_bytes()))
        .collect();
    let (_t, dir, m) = backup_pack(&files, "obs");
    let o = dir.join("obs-studio");
    let svc = text(&o.join("basic/profiles/Main/service.json"));
    assert!(
        svc.contains("\"key\": \"\"") && !svc.contains("live_123"),
        "{svc}"
    );
    assert!(svc.contains("\"server\": \"auto\""));
    let ini = text(&o.join("basic/profiles/Main/basic.ini"));
    assert_eq!(ini, "[General]\r\nName=Main\r\n[Twitch]\r\nName=al\r\n");
    assert_eq!(
        text(&o.join("basic/profiles/Spare/basic.ini")),
        "[General]\r\nName=Spare\r\n",
        "a profile without tokens is copied as it is"
    );
    let wsc = text(&o.join("plugin_config/obs-websocket/config.json"));
    assert!(
        wsc.contains(r#""server_password":"""#) && wsc.contains("4455"),
        "{wsc}"
    );
    assert!(!text(&o.join("global.ini")).contains("ServerPassword"));
    for kept in [
        "user.ini",
        "basic/scenes/Main.json",
        "basic/profiles/Main/recordEncoder.json",
    ] {
        assert!(o.join(kept).is_file(), "{kept} missing");
    }
    for gone in [
        "basic/scenes/Main.json.bak",
        "plugin_config/obs-browser",
        "plugin_config/rtmp-services",
        "logs",
        "crashes",
    ] {
        assert!(!o.join(gone).exists(), "{gone} copied");
    }
    let r = m.packs.iter().find(|r| r.id == "obs").unwrap();
    let s = r.secrets.as_ref().expect("secrets recorded");
    assert_eq!(s.status, PackPathStatus::SensitiveNotCopied);
    assert_eq!(s.files.len(), 4, "{:?}", s.files);
    assert!(
        m.manual_steps
            .iter()
            .any(|st| st.detail.contains("OBS Studio") || st.title.contains("OBS Studio"))
    );
}

#[test]
fn adobe_presets_copies_presets_and_settings_only() {
    let a = "AppData/Roaming/Adobe";
    let files = [
        format!("{a}/CameraRaw/Settings/My Look.xmp"),
        format!("{a}/CameraRaw/CameraProfiles/Imported/cam.dcp"),
        format!("{a}/CameraRaw/Logs/x.log"),
        format!("{a}/CameraRaw/GPU/Adobe Photoshop Lightroom Classic/cache.bin"),
        format!("{a}/Lightroom/Export Presets/User Presets/Web.lrtemplate"),
        format!("{a}/Lightroom/Preferences/Lightroom Classic CC 7 Preferences.agprefs"),
        format!("{a}/Adobe Photoshop 2025/Presets/Brushes/Mine.abr"),
        format!("{a}/Adobe Photoshop 2025/Adobe Photoshop 2025 Settings/Brushes.psp"),
        format!("{a}/Adobe Photoshop 2025/Adobe Photoshop 2025 Settings/WorkSpaces/Paint.psw"),
        format!("{a}/Adobe Photoshop 2025/Adobe Photoshop 2025 Settings/web-cache-temp/x"),
        format!("{a}/Adobe Photoshop 2025/Logs/x.log"),
        format!("{a}/Acrobat/DC/Security/x"),
        format!("{a}/OOBE/x.db"),
    ];
    let files: Vec<(&str, &[u8])> = files.iter().map(|s| (s.as_str(), &b"x"[..])).collect();
    let (_t, dir, _m) = backup_pack(&files, "adobe-presets");
    let a = dir.join("Adobe");
    for kept in [
        "CameraRaw/Settings/My Look.xmp",
        "CameraRaw/CameraProfiles/Imported/cam.dcp",
        "Lightroom/Export Presets/User Presets/Web.lrtemplate",
        "Lightroom/Preferences/Lightroom Classic CC 7 Preferences.agprefs",
        "Adobe Photoshop 2025/Presets/Brushes/Mine.abr",
        "Adobe Photoshop 2025/Adobe Photoshop 2025 Settings/Brushes.psp",
        "Adobe Photoshop 2025/Adobe Photoshop 2025 Settings/WorkSpaces/Paint.psw",
    ] {
        assert!(a.join(kept).is_file(), "{kept} missing");
    }
    for gone in [
        "CameraRaw/Logs",
        "CameraRaw/GPU",
        "Adobe Photoshop 2025/Adobe Photoshop 2025 Settings/web-cache-temp",
        "Adobe Photoshop 2025/Logs",
        "Acrobat",
        "OOBE",
    ] {
        assert!(!a.join(gone).exists(), "{gone} copied");
    }
}

#[test]
fn steam_copies_userdata_for_users_who_ran_it_and_lists_games() {
    let (t, root) = volume();
    let st = root.join("Program Files (x86)/Steam");
    for rel in [
        "userdata/12345/config/localconfig.vdf",
        "userdata/12345/config/shortcuts.vdf",
        "userdata/12345/760/remote/440/screenshots/a.jpg",
        "userdata/12345/7/remote/sharedconfig.vdf",
        "config/config.vdf",
        "config/loginusers.vdf",
        "steamapps/common/Portal 2/portal2.exe",
        "steamapps/common/Hades/Hades.exe",
    ] {
        write(&st, rel, b"x");
    }
    let mut w = crate::fixtures::alice();
    w.key(r"Software\Valve\Steam").sz("Language", "english");
    std::fs::write(root.join("Users/alice/NTUSER.DAT"), w.to_bytes()).unwrap();
    let src = OfflineVolume::open(&root).unwrap();
    let opts = PlanOptions {
        users: vec!["Alice".into(), "Bob".into()],
        packs: crate::packs::load::load_all(None).0,
        ..Default::default()
    };
    let p = plan(&src, &opts).unwrap();
    let ids = |u: &str| -> Vec<&str> {
        p.packs
            .iter()
            .filter(|r| r.user == u)
            .map(|r| r.id.as_str())
            .collect()
    };
    assert!(ids("Alice").contains(&"steam"));
    assert!(!ids("Bob").contains(&"steam"), "Bob never ran Steam");
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let m = run(&src, &p, &job, &quick(), &mut |_| {}).unwrap();
    assert_eq!(m.counts().2, 0, "no failed files");
    let ud = job.dir.join("users/Alice/packs/steam/userdata/12345");
    for kept in [
        "config/localconfig.vdf",
        "config/shortcuts.vdf",
        "760/remote/440/screenshots/a.jpg",
        "7/remote/sharedconfig.vdf",
    ] {
        assert!(ud.join(kept).is_file(), "{kept} missing");
    }
    assert!(!job.dir.join("users/Alice/packs/steam/config").exists());
    let r = m.packs.iter().find(|r| r.id == "steam").unwrap();
    let games = r
        .paths
        .iter()
        .find(|p| p.status == PackPathStatus::Listed)
        .unwrap();
    assert_eq!(games.listed, ["Hades", "Portal 2"]);
    assert_eq!(r.winget_id.as_deref(), Some("Valve.Steam"));
}
