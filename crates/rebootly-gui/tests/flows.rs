//! Whole GUI flows over the `win11-basic` fixture, driven through the
//! accessibility tree (egui_kittest): find by label, click, wait.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rebootly_core::Source;
use rebootly_core::env::Environment;
use rebootly_core::packs::schema::CompiledPack;
use rebootly_core::restore::{RestoreHost, SimHost, Target};
use rebootly_core::volume::{RecoveryKey, Volume, VolumeState};
use rebootly_gui::RebootlyApp;
use rebootly_host::{Drive, Platform};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

struct Fake {
    drive: PathBuf,
    sim: PathBuf,
    /// A BitLocker-locked disk holding the fixture, until unlocked.
    locked_disk: bool,
    unlocked: std::sync::Mutex<bool>,
}

const KEY: &str = "000000-000011-123453-720885-111111-222222-333333-444444";

impl Platform for Fake {
    fn environment(&self) -> Environment {
        Environment::LiveWindows
    }
    fn version(&self) -> String {
        "test".into()
    }
    fn open_live(&self) -> anyhow::Result<Box<dyn Source>> {
        self.open_offline(&fixture())
    }
    fn install_roots(&self) -> Vec<PathBuf> {
        if *self.unlocked.lock().unwrap() {
            vec![fixture()]
        } else {
            Vec::new()
        }
    }
    fn volumes(&self) -> Vec<Volume> {
        if !self.locked_disk || *self.unlocked.lock().unwrap() {
            return Vec::new();
        }
        vec![Volume {
            device: "E:".into(),
            label: None,
            size: Some(256 << 30),
            fs: Some("BitLocker".into()),
            state: VolumeState::Locked,
            id: None,
            removable: true,
        }]
    }
    fn open_volume(
        &self,
        _: &str,
        key: Option<&RecoveryKey>,
    ) -> anyhow::Result<(PathBuf, Vec<String>)> {
        if key.map(RecoveryKey::expose) != Some(KEY) {
            anyhow::bail!("E: wasn't unlocked; check the recovery key");
        }
        *self.unlocked.lock().unwrap() = true;
        Ok((fixture(), vec!["It was hibernated.".into()]))
    }
    fn drives(&self) -> Vec<Drive> {
        vec![Drive {
            path: self.drive.clone(),
            note: "test drive".into(),
        }]
    }
    fn target_fs(&self, _: &Path) -> Option<String> {
        None
    }
    fn keep_awake(&self, _: bool) -> Box<dyn Any> {
        Box::new(())
    }
    fn packs(&self) -> Vec<CompiledPack> {
        rebootly_core::packs::load::load_all(None).0
    }
    fn restore_target(
        &self,
        _: bool,
        user: &str,
    ) -> anyhow::Result<(Target, Box<dyn RestoreHost + Send>)> {
        Ok((Target::simulated(&self.sim, user), Box::new(SimHost)))
    }
}

struct Gui {
    h: Harness<'static, RebootlyApp>,
    drive: PathBuf,
    sim: PathBuf,
}

impl Gui {
    fn new(t: &Path) -> Self {
        Self::with(t, false)
    }

    fn with(t: &Path, locked_disk: bool) -> Self {
        let drive = t.join("usb");
        let sim = t.join("sim");
        std::fs::create_dir_all(&drive).unwrap();
        let p: Arc<dyn Platform> = Arc::new(Fake {
            drive: drive.clone(),
            sim: sim.clone(),
            locked_disk,
            unlocked: Default::default(),
        });
        let h = Harness::builder()
            .with_size([960.0, 2400.0])
            .build_eframe(move |cc| RebootlyApp::new(&cc.egui_ctx, p));
        Gui { h, drive, sim }
    }

    /// Steps until a widget whose label contains `text` shows (workers run
    /// on their own threads).
    fn wait(&mut self, text: &str) {
        for _ in 0..1500 {
            self.h.step();
            if self.h.query_all_by_label_contains(text).next().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("“{text}” never showed");
    }

    fn click(&mut self, label: &str) {
        self.wait(label);
        self.h.get_by_label(label).click();
        self.h.step();
        self.h.step();
    }

    fn backups(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.drive.join("Rebootly"))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("manifest.json").is_file())
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn paths(dir: &Path) -> Vec<String> {
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    m["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect()
}

/// Home → Back up → This PC → all accounts → Quick → the drive → pre-flight.
fn to_preflight(g: &mut Gui) {
    g.click("Back up");
    g.click("Next"); // This PC
    g.wait("Alice");
    g.click("Next"); // accounts
    g.click("Next"); // Quick
    g.click("Next"); // the drive
    g.wait("to do by hand");
}

#[test]
fn quick_backup_from_the_window() {
    let t = tempfile::tempdir().unwrap();
    let mut g = Gui::new(t.path());
    to_preflight(&mut g);
    g.click("Start backup");
    g.wait("Backup finished.");
    g.wait("Things to do by hand");
    let b = g.backups();
    assert_eq!(b.len(), 1);
    let p = paths(&b[0]);
    assert!(
        p.iter()
            .any(|p| p.starts_with("users/Alice/folders/Documents"))
    );
    assert!(!p.iter().any(|p| p.starts_with("custom/")));
    assert!(b[0].join("report.html").is_file());
    g.click("Done");
    g.wait("What would you like to do?");
}

#[test]
fn custom_leaves_out_unticked_folders() {
    let t = tempfile::tempdir().unwrap();
    let mut g = Gui::new(t.path());
    g.click("Back up");
    g.click("Next");
    g.wait("Alice");
    g.click("Next");
    g.click("Custom");
    g.click("Next");
    g.wait("Downloads");
    g.click("Desktop");
    g.click("Documents");
    g.click("Next"); // custom
    g.click("Next"); // drive
    g.click("Start backup");
    g.wait("Backup finished.");
    let p = paths(&g.backups()[0]);
    assert!(
        !p.iter()
            .any(|p| p.contains("/folders/Desktop") || p.contains("/folders/Documents"))
    );
    assert!(p.iter().any(|p| p.contains("/folders/Downloads")));
}

#[test]
fn saved_profile_then_restore_dry_run_and_real() {
    let t = tempfile::tempdir().unwrap();
    let mut g = Gui::new(t.path());
    to_preflight(&mut g);
    g.click("Save as a job profile");
    g.wait("Profile name");
    g.h.get_by_role_and_label(eframe::egui::Role::TextInput, "Profile name")
        .click();
    g.h.step();
    g.h.get_by_role_and_label(eframe::egui::Role::TextInput, "Profile name")
        .type_text("techs");
    g.h.step();
    g.h.step();
    g.click("Save profile");
    g.wait("Saved job profile");
    assert!(
        g.drive
            .join("Rebootly/rebootly-profiles/techs.json")
            .is_file()
    );
    g.click("Start backup");
    g.wait("Backup finished.");
    g.click("Done");

    // Restore: newest backup → Alice → options → preview → dry run.
    g.click("Restore");
    g.wait("The newest backup is at the top");
    g.click("Next");
    g.click("Alice");
    g.click("Next");
    g.click("Preview");
    g.wait("Nothing has been changed yet");
    g.click("Dry run");
    g.wait("Dry run done");
    assert!(!g.sim.join("C/Users/Alice/Documents").exists());
    g.click("Back");
    g.click("Restore…");
    g.click("Restore now");
    g.wait("Restore done");
    assert!(g.sim.join("C/Users/Alice/Documents").is_dir());
}

#[test]
fn locked_disk_unlocks_with_the_recovery_key() {
    let t = tempfile::tempdir().unwrap();
    let mut g = Gui::with(t.path(), true);
    g.click("Back up");
    g.click("Unlock E: · 256.0 GiB · locked with BitLocker · removable");
    g.wait("0 of 48 digits");
    // Shown as typed (owner), not as a password field. Typed without
    // dashes, in two goes: the dashes are added and the cursor stays at
    // the end, so the second batch lands after the first.
    let field = || eframe::egui::Role::TextInput;
    let plain: String = KEY.chars().filter(char::is_ascii_digit).collect();
    g.h.get_by_role_and_label(field(), "Recovery key").click();
    g.h.step();
    for part in [&plain[..10], &plain[10..47]] {
        g.h.get_by_role_and_label(field(), "Recovery key").type_text(part);
        g.h.step();
    }
    g.wait("47 of 48 digits");
    g.click("Unlock");
    g.wait("this has 47");
    g.h.get_by_role_and_label(field(), "Recovery key").click();
    g.h.step();
    g.h.key_press(eframe::egui::Key::End);
    g.h.step();
    g.h.get_by_role_and_label(field(), "Recovery key").type_text(&plain[47..]);
    g.h.step();
    g.wait("48 of 48 digits");
    g.click("Unlock");
    g.wait("It was hibernated.");
    g.wait("Windows on");
    assert!(g.h.query_all_by_label_contains("Unlock E:").next().is_none());
    g.click("Next"); // the unlocked install is picked
    g.wait("Alice");
}
