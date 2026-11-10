//! Pick a backup (found on the drives, newest first) and the backup user.

use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::preflight::gib;
use rebootly_core::restore::find::{FoundBackup, find_backups};

use super::Backup;
use super::options::OptionsScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::backup::target::DRIVE_FOLDER;
use crate::ui::{BAD, dim};
use crate::widgets::{Menu, TextField};

/// "2026-11-10T13:16:15Z" → "2026-11-10 13:16".
fn when(s: &str) -> String {
    s.get(..16).unwrap_or(s).replace('T', " ")
}

fn describe(b: &FoundBackup) -> (String, String) {
    let state = match &b.finished {
        Some(f) if b.complete => format!("finished {}", when(f)),
        _ => "not finished".into(),
    };
    (
        format!("{} · {state}", b.machine),
        format!(
            "{} · {} · {}",
            b.users.join(", "),
            gib(b.bytes),
            b.dir.display()
        ),
    )
}

pub struct PickScreen {
    found: Vec<FoundBackup>,
    menu: Menu,
    typing: Option<TextField>,
    error: Option<String>,
}

impl PickScreen {
    pub fn new(cx: &mut Ctx) -> Self {
        cx.busy("Restore", "Looking for backups on your drives…");
        let roots: Vec<PathBuf> = cx
            .platform
            .drives()
            .into_iter()
            .flat_map(|d| [d.path.join(DRIVE_FOLDER), d.path])
            .collect();
        let found = find_backups(&roots);
        let mut items: Vec<_> = found.iter().map(describe).collect();
        items.push((
            "Type a folder…".into(),
            "a backup folder (contains manifest.json)".into(),
        ));
        PickScreen {
            found,
            menu: Menu::new(items),
            typing: None,
            error: None,
        }
    }

    fn open(&mut self, dir: &Path) -> Nav {
        match Backup::open(dir) {
            Ok(b) if b.manifest.users.len() > 1 => Nav::Push(Box::new(UserScreen::new(b))),
            Ok(b) => match b.manifest.users.first().map(|u| u.username.clone()) {
                Some(u) => Nav::Push(Box::new(OptionsScreen::new(b, u))),
                None => {
                    self.error = Some("That backup has no user accounts in it.".into());
                    Nav::Stay
                }
            },
            Err(e) => {
                self.error = Some(e.to_string());
                Nav::Stay
            }
        }
    }
}

impl Screen for PickScreen {
    fn title(&self) -> String {
        "Restore: choose a backup".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body, foot] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .areas(area);
        let intro_text = if self.found.is_empty() {
            Line::raw("No backups found on your drives. Plug in the drive, or type the folder.")
        } else {
            Line::raw("The newest backup is at the top.")
        };
        f.render_widget(Paragraph::new(intro_text), intro);
        match &self.typing {
            Some(t) => t.draw(f, body, true),
            None => self.menu.draw(f, body),
        }
        let note = match &self.error {
            Some(e) => Line::styled(e.clone(), Style::default().fg(BAD)),
            None => Line::styled(
                "Restore runs as the account you're signed in with now.",
                dim(),
            ),
        };
        f.render_widget(Paragraph::new(note).wrap(Wrap { trim: true }), foot);
    }

    fn key(&mut self, k: KeyEvent, _cx: &mut Ctx) -> Nav {
        if let Some(t) = &mut self.typing {
            match k.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Enter => {
                    let p = PathBuf::from(t.value.trim());
                    self.typing = None;
                    return self.open(&p);
                }
                _ => {
                    t.key(&k);
                }
            }
            return Nav::Stay;
        }
        self.error = None;
        match self.menu.key(&k) {
            Some(i) if i < self.found.len() => {
                let dir = self.found[i].dir.clone();
                self.open(&dir)
            }
            Some(_) => {
                self.typing = Some(TextField::new("", false));
                Nav::Stay
            }
            None => Nav::Ignored,
        }
    }
}

/// Several accounts in the backup: one restore run restores one of them
/// into the account signed in now.
pub struct UserScreen {
    backup: Backup,
    /// Manifest user indexes, by name.
    order: Vec<usize>,
    menu: Menu,
}

impl UserScreen {
    pub fn new(backup: Backup) -> Self {
        let users = &backup.manifest.users;
        let mut order: Vec<usize> = (0..users.len()).collect();
        order.sort_by_key(|&i| users[i].username.to_lowercase());
        let items = order
            .iter()
            .map(|&i| {
                (
                    users[i].username.clone(),
                    format!("{} folder(s)", users[i].folders.len()),
                )
            })
            .collect();
        UserScreen {
            backup,
            order,
            menu: Menu::new(items),
        }
    }
}

impl Screen for UserScreen {
    fn title(&self) -> String {
        "Restore: whose files".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
        f.render_widget(
            Paragraph::new(
                "Which old account goes into the account you're signed in with? \
                 For the others, sign in as each of them and run Restore again.",
            )
            .wrap(Wrap { trim: true }),
            intro,
        );
        self.menu.draw(f, body);
    }

    fn key(&mut self, k: KeyEvent, _cx: &mut Ctx) -> Nav {
        match self.menu.key(&k) {
            Some(i) => {
                let user = self.backup.manifest.users[self.order[i]].username.clone();
                Nav::Push(Box::new(OptionsScreen::new(self.backup.clone(), user)))
            }
            None => Nav::Ignored,
        }
    }
}
