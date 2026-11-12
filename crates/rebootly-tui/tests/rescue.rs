//! Rescue flow with a BitLocker-locked disk: unlock with the recovery key,
//! then the Windows on it is listed and chosen.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use rebootly_core::Source;
use rebootly_core::env::Environment;
use rebootly_core::packs::schema::CompiledPack;
use rebootly_core::restore::{RestoreHost, Target};
use rebootly_core::volume::{Unlock, Volume, VolumeState};
use rebootly_tui::app::run_app;
use rebootly_tui::ui::{ScriptUi, k, text};
use rebootly_tui::{Drive, Platform, first_screen};

/// The drive's BitLocker password (the fake accepts it too).
const PASSWORD: &str = "Sommer 2026!";
const KEY: &str = "000000-000011-123453-720885-111111-222222-333333-444444";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

#[derive(Default)]
struct Fake {
    unlocked: Mutex<bool>,
    prepared: Mutex<bool>,
    /// Where the USB disk was mounted for writing.
    target: Mutex<Option<PathBuf>>,
}

impl Platform for Fake {
    fn environment(&self) -> Environment {
        Environment::Linux
    }
    fn version(&self) -> String {
        "test".into()
    }
    fn open_live(&self) -> anyhow::Result<Box<dyn Source>> {
        anyhow::bail!("not live")
    }
    fn install_roots(&self) -> Vec<PathBuf> {
        if *self.unlocked.lock().unwrap() {
            vec![fixture()]
        } else {
            Vec::new()
        }
    }
    fn volumes(&self) -> Vec<Volume> {
        let state = if *self.unlocked.lock().unwrap() {
            VolumeState::Mounted {
                root: fixture(),
                read_only: true,
            }
        } else {
            VolumeState::Locked
        };
        vec![Volume {
            device: "/dev/sda3".into(),
            label: None,
            size: Some(256 << 30),
            fs: Some("BitLocker".into()),
            state,
            id: None,
            removable: false,
        }]
    }
    fn open_volume(
        &self,
        device: &str,
        key: Option<&Unlock>,
    ) -> anyhow::Result<(PathBuf, Vec<String>)> {
        assert_eq!(device, "/dev/sda3");
        if !matches!(key.map(Unlock::expose), Some(KEY | PASSWORD)) {
            anyhow::bail!("the recovery key doesn't unlock this disk");
        }
        *self.unlocked.lock().unwrap() = true;
        Ok((
            fixture(),
            vec!["Windows on this disk wasn't fully shut down.".into()],
        ))
    }
    fn prepare_rescue(&self) -> Vec<String> {
        *self.prepared.lock().unwrap() = true;
        Vec::new()
    }
    fn drives(&self) -> Vec<Drive> {
        Vec::new()
    }
    fn target_volumes(&self) -> Vec<Volume> {
        let usb = |dev: &str, label: &str| Volume {
            device: dev.into(),
            label: Some(label.into()),
            size: Some(1 << 40),
            fs: Some("ntfs".into()),
            state: VolumeState::Unmounted,
            id: None,
            removable: true,
        };
        vec![usb("/dev/sdb1", "OldPC"), usb("/dev/sdc1", "Backup HDD")]
    }
    fn mount_target(&self, device: &str) -> anyhow::Result<PathBuf> {
        if device == "/dev/sdb1" {
            anyhow::bail!("{device} has Windows on it");
        }
        let dir = std::env::temp_dir().join(format!("rebootly-tui-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        *self.target.lock().unwrap() = Some(dir.clone());
        Ok(dir)
    }
    fn target_fs(&self, _: &Path) -> Option<String> {
        None
    }
    fn keep_awake(&self, _: bool) -> Box<dyn Any> {
        Box::new(())
    }
    fn packs(&self) -> Vec<CompiledPack> {
        Vec::new()
    }
    fn restore_target(
        &self,
        _: bool,
        _: &str,
    ) -> anyhow::Result<(Target, Box<dyn RestoreHost + Send>)> {
        anyhow::bail!("no restore here")
    }
}

fn screen(keys: &[KeyEvent]) -> (String, Fake) {
    let p = Fake::default();
    let mut ui = ScriptUi::new(keys);
    run_app(&p, &mut ui, first_screen);
    (ui.screen(), p)
}

#[test]
fn locked_disk_is_listed_and_unlocks_with_the_key() {
    let (s, p) = screen(&[]);
    assert!(*p.prepared.lock().unwrap(), "disks mounted before the scan");
    assert!(s.contains("Unlock /dev/sda3"), "{s}");
    assert!(s.contains("locked with BitLocker"), "{s}");
    assert!(!s.contains("Windows on"), "{s}");

    // A short key: plain error, digit count, the field keeps what was typed
    // and shows it (owner: digits, grouped, so it can be checked).
    let mut keys = vec![k(KeyCode::Enter)];
    keys.extend(text("12345"));
    keys.push(k(KeyCode::Enter));
    let (s, _) = screen(&keys);
    assert!(s.contains("48 digits"), "{s}");
    assert!(s.contains("5 of 48 digits"), "{s}");
    assert!(s.contains("> 12345"), "typed digits shown: {s}");

    // Grouped as typed; a mistyped group is named at once.
    let mut keys = vec![k(KeyCode::Enter)];
    keys.extend(text("000000123454"));
    let (s, _) = screen(&keys);
    assert!(s.contains("> 000000-123454"), "{s}");
    assert!(s.contains("group 2 isn't right"), "{s}");

    // The right key: unlocked, the install is listed and selected, the note
    // shown; Enter opens it (the accounts screen).
    let mut keys = vec![k(KeyCode::Enter)];
    keys.extend(text(KEY));
    keys.push(k(KeyCode::Enter));
    let (s, p) = screen(&keys);
    assert!(*p.unlocked.lock().unwrap());
    assert!(s.contains("Windows on"), "{s}");
    assert!(s.contains("wasn't fully shut down"), "{s}");
    assert!(!s.contains("Unlock /dev/sda3"), "{s}");
    assert!(
        !s.contains("123453"),
        "key left on screen after unlock: {s}"
    );
    keys.push(k(KeyCode::Enter));
    let (s, _) = screen(&keys);
    assert!(s.contains("Alice"), "{s}");
}

#[test]
fn tab_switches_to_the_password_which_shows_as_dots() {
    let mut keys = vec![k(KeyCode::Enter), k(KeyCode::Tab)];
    keys.extend(text(PASSWORD));
    let (s, _) = screen(&keys);
    assert!(s.contains("Password for /dev/sda3"), "{s}");
    assert!(s.contains("12 characters"), "{s}");
    assert!(s.contains(&"•".repeat(12)), "{s}");
    assert!(!s.contains("Sommer"), "password on screen: {s}");
    keys.push(k(KeyCode::Enter));
    let (s, p) = screen(&keys);
    assert!(*p.unlocked.lock().unwrap());
    assert!(s.contains("Windows on"), "{s}");
}

#[test]
fn a_wrong_key_says_so_and_esc_goes_back() {
    let bad = KEY.replace("444444", "555555");
    let mut keys = vec![k(KeyCode::Enter)];
    keys.extend(text(&bad));
    keys.push(k(KeyCode::Enter));
    let (s, p) = screen(&keys);
    assert!(!*p.unlocked.lock().unwrap());
    assert!(s.contains("doesn't unlock this disk"), "{s}");
    keys.push(k(KeyCode::Esc));
    let (s, _) = screen(&keys);
    assert!(s.contains("Unlock /dev/sda3"), "{s}");
}

#[test]
fn a_usb_disk_is_mounted_for_writing_from_the_target_screen() {
    let mut keys = vec![k(KeyCode::Enter)];
    keys.extend(text(KEY));
    keys.extend([
        k(KeyCode::Enter),
        k(KeyCode::Enter),
        k(KeyCode::Enter),
        k(KeyCode::Enter),
    ]);
    let (s, _) = screen(&keys);
    assert!(s.contains("Where should the backup go"), "{s}");
    assert!(s.contains("Use /dev/sdc1 · “Backup HDD”"), "{s}");
    assert!(s.contains("mounts it for writing"), "{s}");

    // The disk with Windows: refused, the screen stays.
    keys.push(k(KeyCode::Enter));
    let (s, p) = screen(&keys);
    assert!(s.contains("has Windows on it"), "{s}");
    assert!(p.target.lock().unwrap().is_none());

    // The backup disk: mounted, on to the pre-flight.
    keys.extend([k(KeyCode::Down), k(KeyCode::Enter)]);
    let (s, p) = screen(&keys);
    let dir = p.target.lock().unwrap().clone().expect("mounted");
    assert!(!s.contains("Where should the backup go"), "{s}");
    assert!(
        s.contains("Target: ") && s.contains("rebootly-tui-target"),
        "pre-flight on the mounted disk: {s}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
