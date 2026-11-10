//! Pick what to back up: the running Windows, or a Windows installation on
//! another disk (the Rescue flow's "find Windows" step).

use std::path::PathBuf;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::profile::JobProfile;
use rebootly_core::install::WindowsInstall;

use super::{BackupCtx, preflight::PreflightScreen, users::UsersScreen};
use crate::app::{Ctx, Nav, Screen};
use crate::ui::{BAD, dim};
use crate::widgets::{Menu, TextField};

enum Choice {
    Live,
    Install(PathBuf),
    Type,
}

pub struct SourceScreen {
    rescue: bool,
    choices: Vec<Choice>,
    menu: Menu,
    typing: Option<TextField>,
    error: Option<String>,
    /// Run this saved job once the source is open.
    job: Option<(JobProfile, PathBuf)>,
}

impl SourceScreen {
    pub fn new(cx: &mut Ctx, live_ok: bool, job: Option<(JobProfile, PathBuf)>) -> Self {
        cx.busy("Find Windows", "Looking for Windows installations…");
        let rescue = !live_ok;
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
        choices.push(Choice::Type);
        items.push(("Type a path…".into(), "a mounted Windows volume".into()));
        SourceScreen {
            rescue,
            choices,
            menu: Menu::new(items),
            typing: None,
            error: None,
            job,
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
        if self.typing.is_some() {
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
        match &self.typing {
            Some(t) => t.draw(f, body, true),
            None => self.menu.draw(f, body),
        }
        let note = match &self.error {
            Some(e) => Line::styled(e.clone(), ratatui::style::Style::default().fg(BAD)),
            None if self.rescue => Line::styled(
                "Disks locked with BitLocker aren't listed yet (unlocking comes in a later version).",
                dim(),
            ),
            None => Line::default(),
        };
        f.render_widget(Paragraph::new(note).wrap(Wrap { trim: true }), foot);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
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
