//! The window: a header with the page title, the page in a scroll area, and
//! a footer with Back. Pages form a stack like a wizard.

use std::sync::Arc;

use eframe::egui::{self, Color32, RichText};
use rebootly_host::Platform;

use crate::pages::home::HomePage;

pub enum Nav {
    Stay,
    Push(Box<dyn Page>),
    /// Replace the current page (no way back to it).
    Replace(Box<dyn Page>),
    Back,
    Home,
}

/// What every page gets.
pub struct Cx {
    pub platform: Arc<dyn Platform>,
    pub ctx: egui::Context,
}

pub trait Page {
    fn title(&self) -> String;
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav;
    /// False while a job runs.
    fn can_go_back(&self) -> bool {
        true
    }
}

pub struct RebootlyApp {
    stack: Vec<Box<dyn Page>>,
    cx: Cx,
    version: String,
}

impl RebootlyApp {
    pub fn new(ctx: &egui::Context, platform: Arc<dyn Platform>) -> Self {
        style(ctx);
        fonts(ctx);
        let mut cx = Cx {
            platform,
            ctx: ctx.clone(),
        };
        let version = cx.platform.version();
        let home = HomePage::new(&mut cx);
        RebootlyApp {
            stack: vec![Box::new(home)],
            cx,
            version,
        }
    }

    fn apply(&mut self, nav: Nav) {
        match nav {
            Nav::Stay => {}
            Nav::Push(p) => self.stack.push(p),
            Nav::Replace(p) => {
                self.stack.pop();
                self.stack.push(p);
            }
            Nav::Back => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                }
            }
            Nav::Home => self.stack.truncate(1),
        }
    }
}

impl eframe::App for RebootlyApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // eframe's shortcuts are off on Windows (main.rs): paste comes from
        // the Windows clipboard here.
        if cfg!(windows) {
            crate::clipboard::translate(&mut raw.events, rebootly_host::clipboard_text);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let back_ok = self.stack.len() > 1 && self.stack.last().is_some_and(|p| p.can_go_back());
        let title = self.stack.last().map(|p| p.title()).unwrap_or_default();
        let mut nav = Nav::Stay;
        let side =
            egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(24, 0));
        egui::Panel::top("header").frame(side).show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Rebootly").strong().color(ACCENT).size(20.0));
                ui.separator();
                ui.label(RichText::new(&title).size(20.0));
            });
            ui.add_space(6.0);
        });
        egui::Panel::bottom("footer").frame(side).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if back_ok && ui.button("Back").clicked() {
                    nav = Nav::Back;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("Rebootly {}", self.version))
                            .weak()
                            .small(),
                    );
                });
            });
            ui.add_space(6.0);
        });
        let body =
            egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(24, 12));
        egui::CentralPanel::default().frame(body).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    // Readable line length on wide windows.
                    ui.set_max_width(ui.available_width().min(820.0));
                    if let Some(page) = self.stack.last_mut() {
                        let n = page.ui(ui, &mut self.cx);
                        if !matches!(n, Nav::Stay) {
                            nav = n;
                        }
                    }
                });
        });
        self.apply(nav);
    }
}

pub const ACCENT: Color32 = Color32::from_rgb(0x1f, 0x7a, 0xc4);
pub const WARN: Color32 = Color32::from_rgb(0xc2, 0x7c, 0x0e);
pub const BAD: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);
pub const GOOD: Color32 = Color32::from_rgb(0x2e, 0x8b, 0x3e);

/// Segoe UI on Windows (looks native), with symbol and CJK fallbacks;
/// characters no font has are drawn as boxes (file names can hold anything).
fn fonts(ctx: &egui::Context) {
    ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
    let Some(dir) = std::env::var_os("WINDIR").map(|w| std::path::PathBuf::from(w).join("Fonts"))
    else {
        return;
    };
    let mut defs = egui::FontDefinitions::default();
    let mut primary = Vec::new();
    for (name, file) in [
        ("segoe-ui", "segoeui.ttf"),
        ("segoe-symbol", "seguisym.ttf"),
        ("yahei", "msyh.ttc"),
    ] {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            defs.font_data
                .insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            primary.push(name.to_string());
        }
    }
    if primary.is_empty() {
        return;
    }
    let fam = defs
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    // Segoe UI first; egui's own fonts stay as the last fallbacks.
    for (i, n) in primary.iter().enumerate() {
        fam.insert(i, n.clone());
    }
    if let Some(mono) = defs.families.get_mut(&egui::FontFamily::Monospace) {
        mono.extend(primary.into_iter().skip(1));
    }
    ctx.set_fonts(defs);
}

fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        use egui::{FontId, TextStyle};
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(15.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(15.0));
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(22.0));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.5));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.0));
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(14.0, 6.0);
        s.spacing.interact_size.y = 28.0;
    });
}
