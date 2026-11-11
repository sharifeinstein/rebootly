//! Where to put the backup: a listed drive (into its `Rebootly\` folder), a
//! drive to mount for writing first (Linux rescue systems: a USB disk), or a
//! typed folder (NAS share, another directory).

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::profile::Selection;
use rebootly_core::volume::Volume;

use super::Shared;
use super::preflight::PreflightScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::ui::BAD;
use crate::widgets::{Menu, TextField};
use rebootly_host::Drive;

/// Folder created on a chosen drive for backups and job profiles.
pub const DRIVE_FOLDER: &str = "Rebootly";

pub struct TargetScreen {
    ctx: Shared,
    sel: Selection,
    drives: Vec<Drive>,
    /// Drives that need mounting for writing first (listed after `drives`).
    mountable: Vec<Volume>,
    menu: Menu,
    typing: Option<TextField>,
    error: Option<String>,
}

impl TargetScreen {
    pub fn new(cx: &mut Ctx, ctx: Shared, sel: Selection) -> Self {
        let drives = cx.platform.drives();
        let mountable = cx.platform.target_volumes();
        let mut items: Vec<(String, String)> = drives
            .iter()
            .map(|d| (d.path.display().to_string(), d.note.clone()))
            .collect();
        items.extend(mountable.iter().map(|v| {
            (
                format!("Use {}", v.describe()),
                "mounts it for writing".into(),
            )
        }));
        items.push(("Type a folder…".into(), "a NAS share or any folder".into()));
        TargetScreen {
            ctx,
            sel,
            drives,
            mountable,
            menu: Menu::new(items),
            typing: None,
            error: None,
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
        let [intro, body, foot] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(if self.error.is_some() { 3 } else { 0 }),
        ])
        .areas(area);
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
        if let Some(e) = &self.error {
            f.render_widget(
                Paragraph::new(Line::styled(e.clone(), ratatui::style::Style::default().fg(BAD)))
                    .wrap(Wrap { trim: true }),
                foot,
            );
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
        let pick = self.menu.key(&k);
        if pick.is_some() {
            self.error = None;
        }
        let n = self.drives.len();
        match pick {
            Some(i) if i < n => {
                let p = self.drives[i].path.join(DRIVE_FOLDER);
                self.next(cx, p)
            }
            Some(i) if i < n + self.mountable.len() => {
                let dev = self.mountable[i - n].device.clone();
                match cx.platform.mount_target(&dev) {
                    Ok(root) => self.next(cx, root.join(DRIVE_FOLDER)),
                    Err(e) => {
                        self.error = Some(format!("{e:#}"));
                        Nav::Stay
                    }
                }
            }
            Some(_) => {
                self.typing = Some(TextField::new("", false));
                Nav::Stay
            }
            None => Nav::Ignored,
        }
    }
}
