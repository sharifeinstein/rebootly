//! Pick the accounts to back up (all by default).

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Paragraph;

use super::Shared;
use super::mode::ModeScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::widgets::{Check, Checks};

pub struct UsersScreen {
    ctx: Shared,
    checks: Checks,
    error: Option<String>,
}

impl UsersScreen {
    pub fn new(ctx: Shared) -> Self {
        let rows = ctx
            .accounts
            .iter()
            .map(|a| {
                let note = if a.has_hive {
                    a.kind.clone()
                } else {
                    format!("{} · settings unreadable, files only", a.kind)
                };
                Check::item(&format!("sid:{}", a.sid), &a.username, note, true)
            })
            .collect();
        UsersScreen {
            ctx,
            checks: Checks::new(rows),
            error: None,
        }
    }

    /// Chosen SIDs; empty when every account is ticked (= all, which also
    /// suits a saved job used on other PCs).
    fn chosen(&self) -> Vec<String> {
        let t = self.checks.ticked("sid:");
        if t.len() == self.ctx.accounts.len() {
            Vec::new()
        } else {
            t
        }
    }
}

impl Screen for UsersScreen {
    fn title(&self) -> String {
        format!("Back up {}: accounts", self.ctx.label)
    }

    fn hints(&self) -> String {
        "↑↓ move · Space tick · a all · n none · Enter next · Esc back".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body, err] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .areas(area);
        f.render_widget(
            Paragraph::new("Whose files and settings should be backed up?"),
            intro,
        );
        self.checks.draw(f, body);
        if let Some(e) = &self.error {
            f.render_widget(Paragraph::new(e.as_str()), err);
        }
    }

    fn key(&mut self, k: KeyEvent, _cx: &mut Ctx) -> Nav {
        if self.checks.key(&k) {
            self.error = None;
            return Nav::Stay;
        }
        if k.code != KeyCode::Enter {
            return Nav::Ignored;
        }
        if self.checks.ticked("sid:").is_empty() {
            self.error = Some("Tick at least one account.".into());
            return Nav::Stay;
        }
        Nav::Push(Box::new(ModeScreen::new(self.ctx.clone(), self.chosen())))
    }
}
