//! Quick / Full / Custom (SPEC §11).

use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Paragraph;
use rebootly_core::backup::profile::{Mode, Selection};

use super::Shared;
use super::custom::CustomScreen;
use super::target::TargetScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::widgets::Menu;

const MODES: [Mode; 3] = [Mode::Quick, Mode::Full, Mode::Custom];

pub struct ModeScreen {
    ctx: Shared,
    users: Vec<String>,
    menu: Menu,
}

impl ModeScreen {
    pub fn new(ctx: Shared, users: Vec<String>) -> Self {
        let items = MODES
            .iter()
            .map(|m| (m.label().to_string(), m.describe().to_string()))
            .collect();
        ModeScreen {
            ctx,
            users,
            menu: Menu::new(items),
        }
    }
}

impl Screen for ModeScreen {
    fn title(&self) -> String {
        format!("Back up {}: how much", self.ctx.label)
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).areas(area);
        f.render_widget(Paragraph::new("How much should be backed up?"), intro);
        self.menu.draw(f, body);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        let Some(i) = self.menu.key(&k) else {
            return Nav::Ignored;
        };
        let mut sel = Selection::new(MODES[i]);
        sel.users = self.users.clone();
        match MODES[i] {
            Mode::Custom => match CustomScreen::new(cx, self.ctx.clone(), sel) {
                Ok(s) => Nav::Push(Box::new(s)),
                Err(e) => {
                    self.menu.items[i].1 = format!("couldn't scan: {e:#}");
                    Nav::Stay
                }
            },
            _ => Nav::Push(Box::new(TargetScreen::new(cx, self.ctx.clone(), sel))),
        }
    }
}
