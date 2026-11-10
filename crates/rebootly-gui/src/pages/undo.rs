//! Undo a restore on this PC (same as `rebootly-cli undo`).

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use rebootly_core::restore::undo::{self, UndoEntry, UndoReport, UndoRun};

use crate::app::{Cx, Nav, Page};
use crate::task::Task;
use crate::widgets::{busy, choice, error, intro, primary};

/// User name for the restore target. On the real PC the target is always
/// the signed-in user; the name only matters for simulated targets.
fn user_hint() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "User".into())
}

pub struct UndoPage {
    runs: Task<Result<Vec<UndoRun>, String>>,
    pick: usize,
    confirm: bool,
    running: Option<Task<Result<UndoReport, String>>>,
    report: Option<Result<UndoReport, String>>,
}

impl UndoPage {
    pub fn new(cx: &mut Cx) -> Self {
        let p = cx.platform.clone();
        let runs = Task::spawn(&cx.ctx, move || {
            let (t, _) = p
                .restore_target(true, &user_hint())
                .map_err(|e| format!("{e:#}"))?;
            Ok(undo::list_all(&t))
        });
        UndoPage {
            runs,
            pick: 0,
            confirm: false,
            running: None,
            report: None,
        }
    }

    fn start(&mut self, cx: &Cx, dir: PathBuf) {
        let p = cx.platform.clone();
        self.running = Some(Task::spawn(&cx.ctx, move || {
            let (t, host) = p
                .restore_target(false, &user_hint())
                .map_err(|e| format!("{e:#}"))?;
            undo::undo(&dir, &t, host.as_ref(), false, false).map_err(|e| e.to_string())
        }));
    }
}

impl Page for UndoPage {
    fn title(&self) -> String {
        "Undo a restore".into()
    }

    fn can_go_back(&self) -> bool {
        self.running.is_none()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        if let Some(t) = &mut self.running {
            match t.take() {
                None => {
                    busy(ui, "Putting things back…");
                    return Nav::Stay;
                }
                Some(r) => {
                    self.running = None;
                    self.report = Some(r);
                }
            }
        }
        if let Some(r) = &self.report {
            match r {
                Err(e) => error(ui, e),
                Ok(r) => {
                    ui.label(
                        RichText::new(format!(
                            "{} put back, {} kept, {} failed.",
                            r.done.len(),
                            r.kept.len(),
                            r.failed.len()
                        ))
                        .strong(),
                    );
                    if !r.kept.is_empty() {
                        ui.label("Kept because they changed after the restore:");
                        for k in &r.kept {
                            ui.label(RichText::new(k).weak());
                        }
                    }
                    for f in &r.failed {
                        error(ui, f);
                    }
                }
            }
            return if primary(ui, "Done", true) {
                Nav::Home
            } else {
                Nav::Stay
            };
        }
        let runs = match self.runs.get() {
            None => {
                busy(ui, "Looking for restores on this PC…");
                return Nav::Stay;
            }
            Some(Err(e)) => {
                error(ui, e);
                return Nav::Stay;
            }
            Some(Ok(r)) => r,
        };
        if runs.is_empty() {
            intro(ui, "No restores are recorded for this account.");
            return Nav::Stay;
        }
        intro(
            ui,
            "Pick the restore to undo (newest first). Files you changed since then are kept.",
        );
        for (i, r) in runs.iter().enumerate() {
            let UndoEntry::Header {
                job_id,
                started,
                user,
                ..
            } = &r.header
            else {
                continue;
            };
            let state = match &r.undone {
                Some(t) => format!("already undone {t}"),
                None => "can be undone".into(),
            };
            if choice(
                ui,
                self.pick == i,
                &format!("{started} · from {user}"),
                &format!("backup {job_id} · {state}"),
            ) {
                self.pick = i;
            }
        }
        let ok = runs.get(self.pick).is_some_and(|r| r.undone.is_none());
        let dir = runs.get(self.pick).map(|r| r.dir.clone());
        if primary(ui, "Undo…", ok) {
            self.confirm = true;
        }
        if self.confirm {
            egui::Modal::new(egui::Id::unique("confirm-undo")).show(ui.ctx(), |ui| {
                ui.heading("Undo this restore?");
                ui.label("Replaced files and settings are put back; files and apps it added are removed.");
                ui.horizontal(|ui| {
                    if ui.button("Undo now").clicked() {
                        self.confirm = false;
                        if let Some(d) = dir.clone() {
                            self.start(cx, d);
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm = false;
                    }
                });
            });
        }
        Nav::Stay
    }
}
