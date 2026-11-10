//! Accounts to back up, then Quick / Full / Custom.

use eframe::egui::{self, RichText};
use rebootly_core::backup::profile::{Mode, Selection};
use rebootly_host::session::Shared;

use super::custom::CustomPage;
use super::target::TargetPage;
use crate::app::{Cx, Nav, Page};
use crate::widgets::{choice, error, intro, primary};

pub struct AccountsPage {
    ctx: Shared,
    on: Vec<bool>,
}

impl AccountsPage {
    pub fn new(ctx: Shared) -> Self {
        let on = vec![true; ctx.accounts.len()];
        AccountsPage { ctx, on }
    }

    /// Chosen SIDs; empty when all are ticked (= every user).
    fn chosen(&self) -> Vec<String> {
        if self.on.iter().all(|o| *o) {
            return Vec::new();
        }
        self.ctx
            .accounts
            .iter()
            .zip(&self.on)
            .filter(|(_, on)| **on)
            .map(|(a, _)| a.sid.clone())
            .collect()
    }
}

impl Page for AccountsPage {
    fn title(&self) -> String {
        format!("Back up {}: accounts", self.ctx.label)
    }

    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx) -> Nav {
        intro(ui, "Whose files and settings should be backed up?");
        for (a, on) in self.ctx.accounts.iter().zip(self.on.iter_mut()) {
            ui.horizontal(|ui| {
                ui.checkbox(on, RichText::new(&a.username).strong());
                let note = if a.has_hive {
                    a.kind.clone()
                } else {
                    format!("{} · settings unreadable, files only", a.kind)
                };
                ui.label(RichText::new(note).weak());
            });
        }
        let any = self.on.iter().any(|o| *o);
        if !any {
            error(ui, "Tick at least one account.");
        }
        if primary(ui, "Next", any) {
            return Nav::Push(Box::new(ModePage::new(self.ctx.clone(), self.chosen())));
        }
        Nav::Stay
    }
}

pub struct ModePage {
    ctx: Shared,
    users: Vec<String>,
    mode: Mode,
}

impl ModePage {
    pub fn new(ctx: Shared, users: Vec<String>) -> Self {
        ModePage {
            ctx,
            users,
            mode: Mode::Quick,
        }
    }
}

impl Page for ModePage {
    fn title(&self) -> String {
        format!("Back up {}: how much", self.ctx.label)
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        intro(ui, "How much should be backed up?");
        for m in [Mode::Quick, Mode::Full, Mode::Custom] {
            if choice(ui, self.mode == m, m.label(), m.describe()) {
                self.mode = m;
            }
        }
        if primary(ui, "Next", true) {
            let mut sel = Selection::new(self.mode);
            sel.users = self.users.clone();
            return match self.mode {
                Mode::Custom => Nav::Push(Box::new(CustomPage::new(cx, self.ctx.clone(), sel))),
                _ => Nav::Push(Box::new(TargetPage::new(cx, self.ctx.clone(), sel))),
            };
        }
        Nav::Stay
    }
}
