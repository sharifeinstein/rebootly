//! Screen stack and the main loop. A screen draws itself and handles keys;
//! long steps (planning, copying, restoring) run inside `key` and use
//! `Ctx::ui` to keep the screen alive.

use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::ui::{self, Ui, chrome, popup};
use rebootly_host::Platform;

pub enum Nav {
    Stay,
    /// The screen didn't use the key (Esc then goes back).
    Ignored,
    Push(Box<dyn Screen>),
    /// Replace the current screen (no way back to it).
    Replace(Box<dyn Screen>),
    Pop,
    /// Back to the first screen.
    Home,
    Quit,
}

pub struct Ctx<'a> {
    pub platform: &'a dyn Platform,
    pub ui: &'a mut dyn Ui,
    /// Process exit code: 0, or 2 if a job finished with failures.
    pub exit: u8,
}

impl Ctx<'_> {
    /// Shows a "working" box until the next draw (scans, planning).
    pub fn busy(&mut self, title: &str, msg: &str) {
        let _ = self.ui.draw(&mut |f: &mut Frame| {
            let _ = chrome(f, title, "");
            popup(f, "Please wait", vec![Line::from(msg.to_owned())]);
        });
    }
}

pub trait Screen {
    fn title(&self) -> String;
    /// Key hints for the footer.
    fn hints(&self) -> String {
        "↑↓ move · Enter choose · Esc back".into()
    }
    fn draw(&mut self, f: &mut Frame, area: Rect);
    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav;
    /// Shown as a box over the screen (errors, confirmations).
    fn overlay(&self) -> Option<(String, Vec<Line<'static>>)> {
        None
    }
}

/// Draw one screen with its chrome.
pub fn draw_screen(f: &mut Frame, s: &mut dyn Screen) {
    let area = chrome(f, &s.title(), &s.hints());
    s.draw(f, area);
    if let Some((title, lines)) = s.overlay() {
        popup(f, &title, lines);
    }
}

/// Runs until the user quits; returns the exit code.
pub fn run_app(
    platform: &dyn Platform,
    ui: &mut dyn Ui,
    first: impl FnOnce(&mut Ctx) -> Box<dyn Screen>,
) -> u8 {
    let mut cx = Ctx {
        platform,
        ui,
        exit: 0,
    };
    let mut stack: Vec<Box<dyn Screen>> = vec![first(&mut cx)];
    loop {
        let Some(top) = stack.last_mut() else {
            return cx.exit;
        };
        if cx.ui.draw(&mut |f| draw_screen(f, top.as_mut())).is_err() || cx.ui.finished() {
            return cx.exit;
        }
        let key = match cx.ui.key(Duration::from_millis(250)) {
            Ok(Some(k)) => k,
            Ok(None) => continue,
            Err(_) => return cx.exit,
        };
        if ui::is_ctrl_c(&key) {
            return cx.exit;
        }
        let nav = match key.code {
            // Esc is "back" everywhere unless a screen uses it.
            KeyCode::Esc => match top.key(key, &mut cx) {
                Nav::Ignored => Nav::Pop,
                n => n,
            },
            _ => top.key(key, &mut cx),
        };
        match nav {
            Nav::Stay | Nav::Ignored => {}
            Nav::Push(s) => stack.push(s),
            Nav::Replace(s) => {
                stack.pop();
                stack.push(s);
            }
            Nav::Pop => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            Nav::Home => stack.truncate(1),
            Nav::Quit => return cx.exit,
        }
    }
}
