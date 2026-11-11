//! Pick what to back up: the running Windows, or a Windows installation on
//! another disk (the Rescue flow's "find Windows" step).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::profile::JobProfile;
use rebootly_core::install::WindowsInstall;
use rebootly_core::volume::RecoveryKey;

use super::{BackupCtx, preflight::PreflightScreen, users::UsersScreen};
use crate::app::{Ctx, Nav, Screen};
use crate::ui::{BAD, dim};
use crate::widgets::{Menu, TextField};

enum Choice {
    Live,
    Install(PathBuf),
    /// A BitLocker-locked volume (device), unlocked with a recovery key.
    Locked(String),
    Type,
}

use super::key::{KeyEntry, draw_key};

pub struct SourceScreen {
    rescue: bool,
    choices: Vec<Choice>,
    menu: Menu,
    typing: Option<TextField>,
    key: Option<KeyEntry>,
    error: Option<String>,
    /// Notes from mounting/unlocking (hibernated, not shut down cleanly).
    notes: Vec<String>,
    /// Run this saved job once the source is open.
    job: Option<(JobProfile, PathBuf)>,
}

impl SourceScreen {
    pub fn new(cx: &mut Ctx, live_ok: bool, job: Option<(JobProfile, PathBuf)>) -> Self {
        let rescue = !live_ok;
        let notes = if rescue {
            cx.busy("Find Windows", "Opening the disks (read-only)…");
            cx.platform.prepare_rescue()
        } else {
            Vec::new()
        };
        let mut s = SourceScreen {
            rescue,
            choices: Vec::new(),
            menu: Menu::new(Vec::new()),
            typing: None,
            key: None,
            error: None,
            notes,
            job,
        };
        s.scan(cx, None);
        s
    }

    /// List installations and locked volumes; select `prefer` if listed.
    fn scan(&mut self, cx: &mut Ctx, prefer: Option<&Path>) {
        cx.busy("Find Windows", "Looking for Windows installations…");
        let live_ok = !self.rescue;
        let mut choices = Vec::new();
        let mut items = Vec::new();
        if live_ok {
            choices.push(Choice::Live);
            items.push(("This PC".into(), "the Windows that is running now".into()));
        }
        for i in WindowsInstall::scan(&cx.platform.install_roots()) {
            items.push((
                format!("Windows on {}", i.volume_root.display()),
                if live_ok {
                    "another disk (offline)".into()
                } else {
                    String::new()
                },
            ));
            choices.push(Choice::Install(i.volume_root));
        }
        for v in cx.platform.volumes().into_iter().filter(|v| v.is_locked()) {
            items.push((
                format!("Unlock {}", v.describe()),
                "BitLocker: needs the recovery key".into(),
            ));
            choices.push(Choice::Locked(v.device));
        }
        choices.push(Choice::Type);
        items.push(("Type a path…".into(), "a mounted Windows volume".into()));
        let pick = prefer.and_then(|p| {
            choices
                .iter()
                .position(|c| matches!(c, Choice::Install(r) if r == p))
        });
        self.choices = choices;
        self.menu = Menu::new(items);
        if let Some(i) = pick {
            self.menu.cursor = i;
        }
    }

    fn unlock(&mut self, cx: &mut Ctx) {
        let Some(entry) = self.key.take() else {
            return;
        };
        let key = match RecoveryKey::parse(&entry.field.value) {
            Ok(k) => k,
            Err(e) => {
                self.error = Some(e.to_string());
                self.key = Some(entry); // keep what was typed, to fix it
                return;
            }
        };
        cx.busy("Unlock", "Unlocking the disk (this can take a minute)…");
        match cx.platform.open_volume(&entry.device, Some(&key)) {
            Ok((root, notes)) => {
                self.notes.extend(notes);
                self.notes.dedup();
                self.scan(cx, Some(&root));
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                self.key = Some(entry); // try again, or Esc
            }
        }
    }

    fn open(&mut self, c: usize, cx: &mut Ctx) -> Nav {
        let (res, label) = match &self.choices[c] {
            Choice::Live => {
                cx.busy("Find Windows", "Reading this PC's accounts and folders…");
                (cx.platform.open_live(), "This PC".to_string())
            }
            Choice::Install(p) => {
                cx.busy(
                    "Find Windows",
                    "Reading the accounts and folders on that disk…",
                );
                (cx.platform.open_offline(p), p.display().to_string())
            }
            Choice::Locked(dev) => {
                self.key = Some(KeyEntry {
                    device: dev.clone(),
                    field: TextField::new("", true),
                });
                return Nav::Stay;
            }
            Choice::Type => {
                self.typing = Some(TextField::new("", false));
                return Nav::Stay;
            }
        };
        self.go(res, label, cx)
    }

    fn go(
        &mut self,
        res: anyhow::Result<Box<dyn rebootly_core::Source>>,
        label: String,
        cx: &mut Ctx,
    ) -> Nav {
        let ctx = res.and_then(|src| BackupCtx::new(src, label, cx.platform.packs()));
        match ctx {
            Ok(mut ctx) => match self.job.clone() {
                Some((job, target)) => {
                    ctx.fixed_target = Some(target.clone());
                    Nav::Push(Box::new(PreflightScreen::new(
                        cx,
                        Arc::new(ctx),
                        job.selection,
                        target,
                    )))
                }
                None => Nav::Push(Box::new(UsersScreen::new(Arc::new(ctx)))),
            },
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                Nav::Stay
            }
        }
    }
}

impl Screen for SourceScreen {
    fn title(&self) -> String {
        match (&self.job, self.rescue) {
            (Some((j, _)), _) => format!("Saved job “{}”: what to back up", j.name),
            (None, true) => "Rescue: find Windows".into(),
            (None, false) => "Back up: what to back up".into(),
        }
    }

    fn hints(&self) -> String {
        if self.key.is_some() {
            "type the 48-digit recovery key · Enter unlock · Esc cancel".into()
        } else if self.typing.is_some() {
            "type the folder that contains Windows\\ · Enter open · Esc cancel".into()
        } else if self.rescue {
            "↑↓ move · Enter choose · q quit".into()
        } else {
            "↑↓ move · Enter choose · Esc back".into()
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body, foot] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .areas(area);
        let text = if self.rescue {
            "Rebootly started outside Windows, so it can copy files from a Windows disk \
             without starting it. Pick the Windows installation to rescue."
        } else {
            "Pick the Windows to back up. Usually that's this PC."
        };
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), intro);
        match (&self.key, &self.typing) {
            (Some(k), _) => draw_key(f, body, k),
            (None, Some(t)) => t.draw(f, body, true),
            (None, None) => self.menu.draw(f, body),
        }
        let mut lines: Vec<Line> = Vec::new();
        if let Some(e) = &self.error {
            lines.push(Line::styled(
                e.clone(),
                ratatui::style::Style::default().fg(BAD),
            ));
        }
        lines.extend(self.notes.iter().map(|n| Line::styled(n.clone(), dim())));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), foot);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if let Some(entry) = &mut self.key {
            match k.code {
                KeyCode::Esc => {
                    self.key = None;
                    self.error = None;
                }
                KeyCode::Enter => {
                    self.error = None;
                    self.unlock(cx);
                }
                _ => {
                    entry.field.key(&k);
                }
            }
            return Nav::Stay;
        }
        if let Some(t) = &mut self.typing {
            match k.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Enter => {
                    let p = PathBuf::from(t.value.trim());
                    self.typing = None;
                    cx.busy("Find Windows", "Reading that Windows installation…");
                    let res = cx.platform.open_offline(&p);
                    return self.go(res, p.display().to_string(), cx);
                }
                _ => {
                    t.key(&k);
                }
            }
            return Nav::Stay;
        }
        if self.rescue && k.code == KeyCode::Char('q') {
            return Nav::Quit;
        }
        self.error = None;
        match self.menu.key(&k) {
            Some(c) => self.open(c, cx),
            None => Nav::Ignored,
        }
    }
}

