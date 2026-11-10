//! Restore plan preview, then a dry run or the restore itself.

use std::sync::Arc;

use eframe::egui::{self, RichText};
use rebootly_core::restore::{Backup, Bundle, RestoreOptions, RestorePlan, report};

use super::restore_run::RestoreRunPage;
use crate::app::{Cx, Nav, Page};
use crate::widgets::{error, primary};

/// A plan made against this PC (dry-run target), with what made it.
pub struct Planned {
    pub backup: Backup,
    pub opts: RestoreOptions,
    pub bundle: Option<Arc<Bundle>>,
    pub plan: RestorePlan,
}

pub struct PlanPage {
    p: Arc<Planned>,
    text: String,
    confirm: bool,
}

impl PlanPage {
    pub fn new(p: Planned) -> Self {
        let text = report::plan_text(&p.plan);
        PlanPage {
            p: Arc::new(p),
            text,
            confirm: false,
        }
    }
}

impl Page for PlanPage {
    fn title(&self) -> String {
        let t = &self.p.plan.target;
        format!("Restore into {} ({}): preview", t.username, t.profile)
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        let plan = &self.p.plan;
        for b in &plan.blockers {
            error(ui, &format!("× {b}"));
        }
        ui.label("This is what the restore will do. Nothing has been changed yet.");
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(&self.text).monospace());
                });
        });
        let ok = plan.blockers.is_empty();
        let mut nav = Nav::Stay;
        ui.horizontal(|ui| {
            if primary(ui, "Restore…", ok) {
                self.confirm = true;
            }
            ui.add_space(8.0);
            if ui
                .add_enabled(ok, egui::Button::new("Dry run"))
                .on_hover_text("Goes through every step without changing anything")
                .clicked()
            {
                nav = Nav::Push(Box::new(RestoreRunPage::new(cx, self.p.clone(), true)));
            }
        });
        if self.confirm {
            let user = plan.target.username.clone();
            egui::Modal::new(egui::Id::unique("confirm-restore")).show(ui.ctx(), |ui| {
                ui.heading(format!("Restore into {user} now?"));
                ui.label("Everything it replaces is kept, so you can undo it later (Home, then Undo a restore).");
                ui.horizontal(|ui| {
                    if ui.button("Restore now").clicked() {
                        self.confirm = false;
                        nav = Nav::Push(Box::new(RestoreRunPage::new(cx, self.p.clone(), false)));
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm = false;
                    }
                });
            });
        }
        nav
    }
}
