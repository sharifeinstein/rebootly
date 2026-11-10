//! Where to put the backup: a listed drive (into its `Rebootly\` folder) or
//! a typed folder (NAS share, another directory).

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::profile::Selection;

use super::Shared;
use super::preflight::PreflightScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::platform::Drive;
use crate::widgets::{Menu, TextField};

/// Folder created on a chosen drive for backups and job profiles.
pub const DRIVE_FOLDER: &str = "Rebootly";

pub struct TargetScreen {
    ctx: Shared,
    sel: Selection,
    drives: Vec<Drive>,
    menu: Menu,
    typing: Option<TextField>,
}

impl TargetScreen {
    pub fn new(cx: &mut Ctx, ctx: Shared, sel: Selection) -> Self {
        let drives = cx.platform.drives();
        let mut items: Vec<(String, String)> = drives
            .iter()
            .map(|d| (d.path.display().to_string(), d.note.clone()))
            .collect();
        items.push(("Type a folder…".into(), "a NAS share or any folder".into()));
        TargetScreen {
            ctx,
            sel,
            drives,
            menu: Menu::new(items),
            typing: None,
        }
    }

    fn next(&self, cx: &mut Ctx, target: PathBuf) -> Nav {
        Nav::Push(Box::new(PreflightScreen::new(
            cx,
            self.ctx.clone(),
            self.sel.clone(),
            target,
        )))
    }
}

impl Screen for TargetScreen {
    fn title(&self) -> String {
        format!("Back up {}: where to", self.ctx.label)
    }

    fn hints(&self) -> String {
        if self.typing.is_some() {
            "type a folder · Enter next · Esc cancel".into()
        } else {
            "↑↓ move · Enter choose · Esc back".into()
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
        f.render_widget(
            Paragraph::new(format!(
                "Where should the backup go? On a drive it goes into a {DRIVE_FOLDER} folder; \
                 each backup gets its own folder named after the PC and the time."
            ))
            .wrap(Wrap { trim: true }),
            intro,
        );
        match &self.typing {
            Some(t) => t.draw(f, body, true),
            None => self.menu.draw(f, body),
        }
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if let Some(t) = &mut self.typing {
            match k.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Enter if !t.value.trim().is_empty() => {
                    let p = PathBuf::from(t.value.trim());
                    self.typing = None;
                    return self.next(cx, p);
                }
                _ => {
                    t.key(&k);
                }
            }
            return Nav::Stay;
        }
        match self.menu.key(&k) {
            Some(i) if i < self.drives.len() => {
                let p = self.drives[i].path.join(DRIVE_FOLDER);
                self.next(cx, p)
            }
            Some(_) => {
                self.typing = Some(TextField::new("", false));
                Nav::Stay
            }
            None => Nav::Ignored,
        }
    }
}
