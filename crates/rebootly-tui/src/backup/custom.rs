//! Custom mode: a tree with checkboxes and sizes (SPEC §11). Sizes come
//! from a Full plan of the chosen accounts.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Paragraph;
use rebootly_core::backup::profile::Selection;
use rebootly_host::custom::{self, Row, Section};

use super::Shared;
use super::target::TargetScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::widgets::{Check, Checks};

pub struct CustomScreen {
    ctx: Shared,
    users: Vec<String>,
    rows: Vec<Row>,
    checks: Checks,
}

impl CustomScreen {
    pub fn new(cx: &mut Ctx, ctx: Shared, sel: Selection) -> anyhow::Result<Self> {
        cx.busy("Custom", "Scanning folders to show their sizes…");
        let rows = custom::rows(&ctx, &sel.users)?;
        let mut checks = Vec::new();
        let mut last = None;
        for r in &rows {
            if last != Some(r.section) {
                checks.push(Check::heading(r.section.label()));
                last = Some(r.section);
            }
            let note = if r.section == Section::Folders || r.size.is_some() {
                format!("{:>24}", r.size_text())
            } else {
                String::new()
            };
            checks.push(Check::item(&r.id, &r.label, note, r.on));
        }
        Ok(CustomScreen {
            ctx,
            users: sel.users,
            rows,
            checks: Checks::new(checks),
        })
    }

    pub fn selection(&self) -> Selection {
        let mut rows = self.rows.clone();
        for r in &mut rows {
            r.on = self.checks.on(&r.id);
        }
        custom::selection(&self.users, &rows)
    }
}

impl Screen for CustomScreen {
    fn title(&self) -> String {
        format!("Back up {}: choose", self.ctx.label)
    }

    fn hints(&self) -> String {
        "↑↓ move · Space tick · a all · n none · Enter next · Esc back".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).areas(area);
        f.render_widget(
            Paragraph::new("Tick what to back up. Registry hives are always included (small)."),
            intro,
        );
        self.checks.draw(f, body);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if self.checks.key(&k) {
            return Nav::Stay;
        }
        if k.code != KeyCode::Enter {
            return Nav::Ignored;
        }
        Nav::Push(Box::new(TargetScreen::new(
            cx,
            self.ctx.clone(),
            self.selection(),
        )))
    }
}
