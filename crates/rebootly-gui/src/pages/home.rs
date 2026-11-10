//! Start page and the saved-jobs list.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use rebootly_core::backup::profile::{self, JobProfile};
use rebootly_core::env::Environment;

use super::restore_pick::RestorePickPage;
use super::source::SourcePage;
use super::undo::UndoPage;
use crate::app::{Cx, Nav, Page};
use crate::widgets::intro;

/// Folder made on a chosen drive for backups and job profiles (same as the
/// text screens).
pub const DRIVE_FOLDER: &str = "Rebootly";

/// Saved job profiles on the drives: (profile, target folder).
pub fn saved_jobs(cx: &Cx) -> Vec<(JobProfile, PathBuf)> {
    let mut out = Vec::new();
    for d in cx.platform.drives() {
        for t in [d.path.join(DRIVE_FOLDER), d.path.clone()] {
            out.extend(profile::list(&t).into_iter().map(|(_, p)| (p, t.clone())));
        }
    }
    out
}

pub struct HomePage {
    jobs: usize,
    live: bool,
}

impl HomePage {
    pub fn new(cx: &mut Cx) -> Self {
        HomePage {
            jobs: saved_jobs(cx).len(),
            live: cx.platform.environment() == Environment::LiveWindows,
        }
    }
}

fn big(ui: &mut egui::Ui, title: &str, note: &str) -> bool {
    let b = egui::Button::new(RichText::new(title).size(19.0).strong())
        .min_size(egui::vec2(260.0, 46.0));
    let clicked = ui.add(b).clicked();
    ui.label(RichText::new(note).weak());
    ui.add_space(10.0);
    clicked
}

impl Page for HomePage {
    fn title(&self) -> String {
        "Move to a new PC, or keep a copy".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        intro(ui, "What would you like to do?");
        ui.add_space(6.0);
        if big(
            ui,
            "Back up",
            "This PC or another Windows disk, to a USB drive, another disk or a NAS.",
        ) {
            return Nav::Push(Box::new(SourcePage::new(cx, self.live, None)));
        }
        if self.live
            && big(
                ui,
                "Restore",
                "Put a backup onto this PC. Rebootly looks for backups on your drives.",
            )
        {
            return Nav::Push(Box::new(RestorePickPage::new(cx)));
        }
        if self.jobs > 0
            && big(
                ui,
                "Run a saved job",
                &format!("{} job profile(s) found on your drives.", self.jobs),
            )
        {
            return Nav::Push(Box::new(JobsPage::new(cx)));
        }
        if self.live
            && big(
                ui,
                "Undo a restore",
                "Put back what a restore on this PC replaced.",
            )
        {
            return Nav::Push(Box::new(UndoPage::new(cx)));
        }
        Nav::Stay
    }
}

pub struct JobsPage {
    jobs: Vec<(JobProfile, PathBuf)>,
    chosen: usize,
}

impl JobsPage {
    pub fn new(cx: &mut Cx) -> Self {
        JobsPage {
            jobs: saved_jobs(cx),
            chosen: 0,
        }
    }
}

impl Page for JobsPage {
    fn title(&self) -> String {
        "Run a saved job".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        intro(
            ui,
            "A job profile keeps what to back up; it runs on this PC into the folder it was saved in.",
        );
        for (i, (p, t)) in self.jobs.iter().enumerate() {
            let note = format!("{} · into {}", p.selection.mode.label(), t.display());
            if crate::widgets::choice(ui, self.chosen == i, &p.name, &note) {
                self.chosen = i;
            }
        }
        if crate::widgets::primary(ui, "Next", !self.jobs.is_empty()) {
            let live = cx.platform.environment() == Environment::LiveWindows;
            let job = self.jobs[self.chosen].clone();
            return Nav::Push(Box::new(SourcePage::new(cx, live, Some(job))));
        }
        Nav::Stay
    }
}
