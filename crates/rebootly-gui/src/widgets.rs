//! Small building blocks shared by the pages.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};
use rebootly_core::backup::manifest::ManualStep;

use crate::app::{ACCENT, BAD, WARN};

pub fn intro(ui: &mut egui::Ui, text: &str) {
    ui.label(text);
    ui.add_space(4.0);
}

pub fn error(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(BAD));
}

pub fn warning(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(WARN));
}

pub fn busy(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(text);
    });
}

/// A large choice with a description underneath; true when clicked.
pub fn choice(ui: &mut egui::Ui, selected: bool, title: &str, note: &str) -> bool {
    let frame = egui::Frame::group(ui.style())
        .stroke(egui::Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected {
                ACCENT
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            },
        ))
        .inner_margin(egui::Margin::symmetric(12, 8));
    let mut clicked = false;
    let r = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            clicked |= ui.radio(selected, RichText::new(title).strong()).clicked();
            if !note.is_empty() {
                ui.label(RichText::new(note).weak());
            }
        })
        .response;
    clicked
        || ui
            .interact(r.rect, r.id.with("choice"), egui::Sense::click())
            .clicked()
}

/// The main button of a page, at the end of its content.
pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    if !ui.layout().is_horizontal() {
        ui.add_space(10.0);
    }
    let b = egui::Button::new(RichText::new(text).strong().color(Color32::WHITE)).fill(ACCENT);
    ui.add_enabled(enabled, b).clicked()
}

/// A path field with a "Browse…" folder picker (Windows).
pub fn folder_field(ui: &mut egui::Ui, value: &mut String, hint: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::TextEdit::singleline(value)
                    .hint_text(hint)
                    .desired_width(420.0),
            )
            .changed();
        if browse_available()
            && ui.button("Browse…").clicked()
            && let Some(p) = pick_folder()
        {
            *value = p.display().to_string();
            changed = true;
        }
    });
    changed
}

fn browse_available() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().pick_folder()
}

#[cfg(not(windows))]
fn pick_folder() -> Option<PathBuf> {
    None
}

/// Opens a file or folder with the system's default app.
pub fn open_path(p: &std::path::Path) {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(p).spawn();
    let _ = r;
}

/// Manual steps with tick boxes (end of backup and restore).
pub struct Checklist {
    pub steps: Vec<ManualStep>,
    pub done: Vec<bool>,
}

impl Checklist {
    pub fn new(steps: Vec<ManualStep>) -> Self {
        let done = vec![false; steps.len()];
        Checklist { steps, done }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let n = self.done.iter().filter(|d| **d).count();
        ui.heading(format!(
            "Things to do by hand ({n} of {} done)",
            self.steps.len()
        ));
        if self.steps.is_empty() {
            ui.label(RichText::new("Nothing to do by hand.").weak());
        }
        for (i, s) in self.steps.iter().enumerate() {
            ui.checkbox(
                &mut self.done[i],
                RichText::new(format!("{}. {}", i + 1, s.title)).strong(),
            );
            ui.indent(("step", i), |ui| {
                ui.label(RichText::new(&s.detail).weak());
            });
        }
    }
}
