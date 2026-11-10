//! Interactive ratatui front end for Rebootly (SPEC §11): Quick / Full /
//! Custom backups, the Rescue flow in WinPE/Linux, progress with pause and
//! safe cancel, restore with conflict questions, and tick-box checklists.
//! Every action has a CLI equivalent (`rebootly backup|restore|undo`).

pub mod app;
pub mod backup;
pub mod checklist;
pub mod home;
pub mod platform;
pub mod restore;
pub mod textview;
pub mod ui;
pub mod widgets;

use rebootly_core::env::Environment;

pub use platform::{Drive, Platform};

/// The first screen: the start menu on a running Windows, the Rescue flow
/// (find Windows) in WinPE or Linux.
pub fn first_screen(cx: &mut app::Ctx) -> Box<dyn app::Screen> {
    match cx.platform.environment() {
        Environment::LiveWindows => Box::new(home::HomeScreen::new(cx)),
        Environment::WinPe | Environment::Linux => {
            Box::new(backup::source::SourceScreen::new(cx, false, None))
        }
    }
}

/// Runs the TUI on the terminal; returns the process exit code (0, or 2
/// if a job finished with failed files, 1 if one stopped on an error).
pub fn run(platform: &dyn Platform) -> anyhow::Result<u8> {
    let term = ratatui::init();
    let mut ui = ui::TermUi { term };
    let code = app::run_app(platform, &mut ui, first_screen);
    ratatui::restore();
    Ok(code)
}
