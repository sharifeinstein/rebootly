//! Tier 4 file items: hosts, non-Microsoft tasks, Wi-Fi (bundle only), apps.json.

use super::*;
use crate::backup::sensitive::Passphrase;

fn add_system_files(root: &Path) {
    let s32 = root.join("Windows/System32");
    std::fs::create_dir_all(s32.join("drivers/etc")).unwrap();
    std::fs::write(s32.join("drivers/etc/hosts"), b"10.0.0.5 nas\n").unwrap();
    std::fs::create_dir_all(s32.join("Tasks/Microsoft/Windows/Defrag")).unwrap();
    std::fs::write(
        s32.join("Tasks/Microsoft/Windows/Defrag/ScheduledDefrag"),
        b"<Task/>",
    )
    .unwrap();
    std::fs::create_dir_all(s32.join("Tasks/Vendor")).unwrap();
    std::fs::write(s32.join("Tasks/Vendor/Updater"), b"<Task/>").unwrap();
    std::fs::write(s32.join("Tasks/NightlyBackup"), b"<Task/>").unwrap();
    let pbk = root.join("ProgramData/Microsoft/Network/Connections/Pbk");
    std::fs::create_dir_all(&pbk).unwrap();
    std::fs::write(pbk.join("rasphone.pbk"), b"[Office VPN]\r\n").unwrap();
    let wifi = root.join("ProgramData/Microsoft/Wlansvc/Profiles/Interfaces/{1234}");
    std::fs::create_dir_all(&wifi).unwrap();
    std::fs::write(wifi.join("{abcd}.xml"), b"<WLANProfile/>").unwrap();
}

#[test]
fn system_items_planned_and_wifi_only_encrypted() {
    let (t, root) = volume();
    add_system_files(&root);
    let src = OfflineVolume::open(&root).unwrap();
    let p = plan(&src, &PlanOptions::default()).unwrap();
    let dests: Vec<&str> = p
        .files
        .iter()
        .filter(|f| f.priority == crate::backup::system::SYSTEM_PRIORITY)
        .map(|f| f.dest.as_str())
        .collect();
    assert_eq!(
        dests,
        [
            "system/hosts",
            "system/tasks/NightlyBackup",
            "system/tasks/Vendor/Updater",
            "system/vpn/rasphone.pbk"
        ]
    );
    assert!(
        !p.files
            .iter()
            .any(|f| f.dest.contains("wifi") || f.dest.contains("Wlansvc"))
    );
    assert!(p.manual_steps.iter().any(|s| s.title == "Wi-Fi networks"));
    assert!(p.generated.iter().any(|g| g.dest == "system/apps.json"));

    let o = PlanOptions {
        sensitive_bundle: true,
        ..Default::default()
    };
    let p = plan(&src, &o).unwrap();
    assert!(
        p.sensitive
            .iter()
            .any(|f| f.dest == "system/wifi/{1234}/{abcd}.xml")
    );
    let job = Job::create(&t.path().join("usb"), None).unwrap();
    let ro = RunOptions {
        passphrase: Some(Passphrase::new("correct horse battery".into()).unwrap()),
        bundle_work_factor: Some(2),
        ..quick()
    };
    let m = run(&src, &p, &job, &ro, &mut |_| {}).unwrap();
    assert_eq!(m.counts().2, 0);
    assert_eq!(
        std::fs::read(job.dir.join("system/hosts")).unwrap(),
        b"10.0.0.5 nas\n"
    );
    let apps = std::fs::read_to_string(job.dir.join("system/apps.json")).unwrap();
    assert!(apps.contains("\"machine\""));

    let none = PlanOptions {
        no_system: true,
        ..Default::default()
    };
    let p = plan(&src, &none).unwrap();
    assert!(
        !p.files
            .iter()
            .any(|f| f.dest.starts_with("system/") && !f.dest.starts_with("system/hives/"))
    );
    assert!(p.generated.is_empty());
}
