//! Whole TUI flows with scripted keys over the `win11-basic` fixture.

use std::any::Any;
use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use rebootly_core::Source;
use rebootly_core::env::Environment;
use rebootly_core::packs::schema::CompiledPack;
use rebootly_core::restore::{RestoreHost, SimHost, Target};
use rebootly_tui::app::run_app;
use rebootly_tui::ui::{ScriptUi, ch, k, text};
use rebootly_tui::{Drive, Platform, first_screen};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic")
}

struct Fake {
    env: Environment,
    drive: PathBuf,
    sim: PathBuf,
}

impl Platform for Fake {
    fn environment(&self) -> Environment {
        self.env
    }
    fn version(&self) -> String {
        "test".into()
    }
    fn open_live(&self) -> anyhow::Result<Box<dyn Source>> {
        self.open_offline(&fixture())
    }
    fn install_roots(&self) -> Vec<PathBuf> {
        vec![fixture()]
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
    ) -> anyhow::Result<(Target, Box<dyn RestoreHost>)> {
        Ok((Target::simulated(&self.sim, user), Box::new(SimHost)))
    }
}

struct Run {
    _t: tempfile::TempDir,
    drive: PathBuf,
    sim: PathBuf,
    screen: String,
    code: u8,
    bells: usize,
}

fn run(env: Environment, keys: &[KeyEvent]) -> Run {
    let t = tempfile::tempdir().unwrap();
    run_in(t, env, keys)
}

fn run_in(t: tempfile::TempDir, env: Environment, keys: &[KeyEvent]) -> Run {
    let p = Fake {
        env,
        drive: t.path().join("usb"),
        sim: t.path().join("sim"),
    };
    std::fs::create_dir_all(&p.drive).unwrap();
    let mut ui = ScriptUi::new(keys);
    let code = run_app(&p, &mut ui, first_screen);
    Run {
        drive: p.drive,
        sim: p.sim,
        screen: ui.screen(),
        code,
        bells: ui.bells,
        _t: t,
    }
}

fn backups(drive: &Path) -> Vec<PathBuf> {
    let root = drive.join("Rebootly");
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.join("manifest.json").is_file())
        .collect()
}

fn manifest(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap()
}

fn paths(m: &serde_json::Value) -> Vec<String> {
    m["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect()
}

const ENTER: KeyCode = KeyCode::Enter;
const DOWN: KeyCode = KeyCode::Down;

/// Home → Back up → This PC → all users → Quick → the test drive → start.
fn quick_keys() -> Vec<KeyEvent> {
    vec![k(ENTER), k(ENTER), k(ENTER), k(ENTER), k(ENTER), k(ENTER)]
}

#[test]
fn quick_backup_runs_and_reports() {
    let r = run(Environment::LiveWindows, &quick_keys());
    assert!(r.screen.contains("Backup finished"), "{}", r.screen);
    assert!(r.screen.contains("Things to do by hand"), "{}", r.screen);
    assert_eq!(r.code, 0);
    assert_eq!(r.bells, 1);
    let b = backups(&r.drive);
    assert_eq!(b.len(), 1);
    let m = manifest(&b[0]);
    assert!(m["complete"].as_bool().unwrap());
    let p = paths(&m);
    assert!(
        p.iter()
            .any(|p| p.starts_with("users/Alice/folders/Documents"))
    );
    assert!(p.iter().any(|p| p == "system/apps.json"));
    // Quick leaves out Public and drive-root folders.
    assert!(!p.iter().any(|p| p.starts_with("custom/")));
    let html = std::fs::read_to_string(b[0].join("report.html")).unwrap();
    assert!(html.contains("type=\"checkbox\""));
}

#[test]
fn custom_tree_leaves_out_unticked_items() {
    // Home, source, users, then Custom (third mode).
    let mut keys = vec![k(ENTER), k(ENTER), k(ENTER), k(DOWN), k(DOWN), k(ENTER)];
    // Cursor starts on Desktop: untick Desktop and Documents.
    keys.extend([ch(' '), k(DOWN), ch(' '), k(ENTER), k(ENTER), k(ENTER)]);
    let r = run(Environment::LiveWindows, &keys);
    assert!(r.screen.contains("Backup finished"), "{}", r.screen);
    let p = paths(&manifest(&backups(&r.drive)[0]));
    assert!(
        !p.iter()
            .any(|p| p.contains("/folders/Desktop") || p.contains("/folders/Documents"))
    );
    assert!(p.iter().any(|p| p.contains("/folders/Downloads")));
}

#[test]
fn users_screen_limits_accounts() {
    // Untick every account but the first (Alice).
    let mut keys = vec![k(ENTER), k(ENTER), ch('n'), ch(' '), k(ENTER)];
    keys.extend([k(ENTER), k(ENTER), k(ENTER)]);
    let r = run(Environment::LiveWindows, &keys);
    let p = paths(&manifest(&backups(&r.drive)[0]));
    assert!(p.iter().any(|p| p.starts_with("users/Alice/")));
    assert!(!p.iter().any(|p| p.starts_with("users/bob/")));
}

#[test]
fn rescue_flow_starts_at_find_windows() {
    let r = run(Environment::Linux, &[]);
    assert!(r.screen.contains("Rescue: find Windows"), "{}", r.screen);
    assert!(r.screen.contains("win11-basic"), "{}", r.screen);
    assert!(!r.screen.contains("This PC"));
    // Choose the install → users → Quick → drive → start.
    let r = run(
        Environment::Linux,
        &[k(ENTER), k(ENTER), k(ENTER), k(ENTER), k(ENTER)],
    );
    assert!(r.screen.contains("Backup finished"), "{}", r.screen);
}

#[test]
fn saved_profile_shows_on_home_and_runs() {
    // Up to the pre-flight screen, then save as "techs".
    let mut keys = vec![k(ENTER), k(ENTER), k(ENTER), k(ENTER), k(ENTER), ch('s')];
    keys.extend(text("techs"));
    keys.push(k(ENTER));
    let r = run(Environment::LiveWindows, &keys);
    assert!(r.screen.contains("Saved job profile"), "{}", r.screen);
    let prof = r.drive.join("Rebootly/rebootly-profiles/techs.json");
    assert!(prof.is_file());
    assert!(
        backups(&r.drive).is_empty(),
        "saving must not start a backup"
    );

    // Same drive again: Home lists the job; run it (→ source → pre-flight).
    let t = r._t;
    let r2 = run_in(
        t,
        Environment::LiveWindows,
        &[k(DOWN), k(DOWN), k(ENTER), k(ENTER), k(ENTER), k(ENTER)],
    );
    assert!(r2.screen.contains("Backup finished"), "{}", r2.screen);
    assert_eq!(backups(&r2.drive).len(), 1);
}

#[test]
fn restore_finds_backup_previews_and_restores() {
    let r = run(Environment::LiveWindows, &quick_keys());
    let t = r._t;
    // Home → Restore → newest backup → user Alice → options → preview.
    let to_plan = [k(DOWN), k(ENTER), k(ENTER), k(ENTER), k(ENTER)];
    let mut keys = to_plan.to_vec();
    keys.push(ch('d'));
    let r = run_in(t, Environment::LiveWindows, &keys);
    assert!(r.screen.contains("Dry run done"), "{}", r.screen);
    assert!(
        !r.sim.join("C").exists() || std::fs::read_dir(r.sim.join("C")).unwrap().next().is_none()
    );

    let t = r._t;
    let mut keys = to_plan.to_vec();
    keys.extend([k(ENTER), ch('y')]);
    let r = run_in(t, Environment::LiveWindows, &keys);
    assert!(r.screen.contains("Restore done"), "{}", r.screen);
    let docs = r.sim.join("C/Users/Alice/Documents");
    assert!(docs.is_dir(), "{}", r.screen);
}

#[test]
fn stop_and_back_navigation() {
    // Esc from the mode screen goes back to users; Esc again to source.
    let r = run(
        Environment::LiveWindows,
        &[
            k(ENTER),
            k(ENTER),
            k(ENTER),
            k(KeyCode::Esc),
            k(KeyCode::Esc),
        ],
    );
    assert!(r.screen.contains("what to back up"), "{}", r.screen);
    // q on the start screen quits.
    let r = run(Environment::LiveWindows, &[ch('q')]);
    assert_eq!(r.code, 0);
}

#[test]
fn cancel_then_resume_from_preflight() {
    // Stop straight away (c, y), before any file is copied.
    let mut keys = quick_keys();
    keys.extend([ch('c'), ch('y')]);
    let r = run(Environment::LiveWindows, &keys);
    assert!(r.screen.contains("Stopped"), "{}", r.screen);
    let b = backups(&r.drive);
    assert!(!manifest(&b[0])["complete"].as_bool().unwrap());

    // Back to the same drive: pre-flight offers to continue it (r).
    let t = r._t;
    let mut keys = quick_keys();
    keys.pop();
    keys.push(ch('r'));
    let r = run_in(t, Environment::LiveWindows, &keys);
    assert!(r.screen.contains("Backup finished"), "{}", r.screen);
    let b = backups(&r.drive);
    assert_eq!(b.len(), 1, "resumed into the same folder");
    assert!(manifest(&b[0])["complete"].as_bool().unwrap());
}
