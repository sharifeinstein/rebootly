//! Custom mode: folders, app settings and extras with sizes.

use eframe::egui::{self, RichText};
use rebootly_core::backup::profile::Selection;
use rebootly_host::custom::{self, Row, Section};
use rebootly_host::session::Shared;

use super::target::TargetPage;
use crate::app::{Cx, Nav, Page};
use crate::task::Task;
use crate::widgets::{busy, error, intro, primary};

pub struct CustomPage {
    ctx: Shared,
    users: Vec<String>,
    rows: Task<Result<Vec<Row>, String>>,
}

impl CustomPage {
    pub fn new(cx: &mut Cx, ctx: Shared, sel: Selection) -> Self {
        let (c, users) = (ctx.clone(), sel.users.clone());
        let rows = Task::spawn(&cx.ctx, move || {
            custom::rows(&c, &users).map_err(|e| format!("{e:#}"))
        });
        CustomPage {
            ctx,
            users: sel.users,
            rows,
        }
    }
}

impl Page for CustomPage {
    fn title(&self) -> String {
        format!("Back up {}: choose", self.ctx.label)
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        intro(
            ui,
            "Tick what to back up. Registry hives are always included (they're small).",
        );
        let rows = match self.rows.get() {
            None => {
                busy(ui, "Scanning folders to show their sizes…");
                return Nav::Stay;
            }
            Some(Err(e)) => {
                error(ui, &format!("Couldn't scan: {e}"));
                return Nav::Stay;
            }
            Some(Ok(rows)) => rows,
        };
        let mut last = None;
        egui::Grid::new("custom")
            .num_columns(2)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                for r in rows.iter_mut() {
                    if last != Some(r.section) {
                        ui.label(RichText::new(r.section.label()).heading());
                        ui.end_row();
                        last = Some(r.section);
                    }
                    ui.checkbox(&mut r.on, &r.label);
                    if r.section == Section::Folders || r.size.is_some() {
                        ui.label(RichText::new(r.size_text()).weak());
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        let sel = custom::selection(&self.users, rows);
        if primary(ui, "Next", true) {
            return Nav::Push(Box::new(TargetPage::new(cx, self.ctx.clone(), sel)));
        }
        Nav::Stay
    }
}
