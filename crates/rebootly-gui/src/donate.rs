//! Optional donation card on the "done" pages: a QR code and a link. Never
//! required: "Close" hides it for this run, "Don't show this again" for
//! good (a marker file in the user's app-data folder). Hidden entirely
//! when the build has no donation link (`rebootly_host::DONATE_URL`).

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};
use qrcode::{Color, QrCode};

/// `%LOCALAPPDATA%\Rebootly\no-donation-card` (Windows), or
/// `~/.config/rebootly/no-donation-card`.
pub fn marker_path() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(d).join("Rebootly").join("no-donation-card"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/rebootly/no-donation-card"))
}

pub struct Donate {
    url: Option<String>,
    qr: Option<QrCode>,
    marker: Option<PathBuf>,
    never: bool,
}

impl Donate {
    pub fn new(url: Option<&str>, marker: Option<PathBuf>) -> Self {
        let hidden = marker.as_ref().is_some_and(|m| m.exists());
        let url = url.filter(|u| !u.is_empty() && !hidden).map(str::to_owned);
        let qr = url.as_ref().and_then(|u| QrCode::new(u.as_bytes()).ok());
        Donate {
            url,
            qr,
            marker,
            never: false,
        }
    }

    /// The card for this build and user.
    pub fn for_this_build() -> Self {
        Self::new(rebootly_host::DONATE_URL, marker_path())
    }

    pub fn visible(&self) -> bool {
        self.url.is_some()
    }

    fn close(&mut self) {
        if self.never
            && let Some(m) = &self.marker
        {
            if let Some(dir) = m.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(m, b"The donation card was turned off.\n");
        }
        self.url = None;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(url) = self.url.clone() else { return };
        ui.add_space(12.0);
        let mut close = false;
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(qr) = &self.qr {
                    draw_qr(ui, qr, 4.0);
                    ui.add_space(12.0);
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Rebootly is free").strong().size(17.0));
                    ui.label("If it saved you time, you can buy me a coffee. Scan the code with your phone, or open the page.");
                    ui.label(RichText::new("Completely optional: Rebootly works the same either way.").weak());
                    ui.horizontal(|ui| {
                        if ui.button("Open the page").clicked() {
                            crate::widgets::open_path(std::path::Path::new(&url));
                        }
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                        ui.checkbox(&mut self.never, "Don't show this again");
                    });
                });
            });
        });
        if close {
            self.close();
        }
    }
}

/// QR code as squares, black on white with the quiet zone, whatever the
/// theme (phones need the contrast).
fn draw_qr(ui: &mut egui::Ui, qr: &QrCode, module: f32) {
    let w = qr.width();
    let quiet = 4;
    let side = (w + 2 * quiet) as f32 * module;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, Color32::WHITE);
    let colors = qr.to_colors();
    for (i, c) in colors.iter().enumerate() {
        if *c == Color::Dark {
            let (x, y) = ((i % w + quiet) as f32, (i / w + quiet) as f32);
            let min = rect.min + egui::vec2(x * module, y * module);
            p.rect_filled(
                egui::Rect::from_min_size(min, egui::vec2(module, module)),
                0.0,
                Color32::BLACK,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_without_a_link_or_after_never() {
        let t = tempfile::tempdir().unwrap();
        let m = t.path().join("x/no-donation-card");
        assert!(!Donate::new(None, Some(m.clone())).visible());
        assert!(!Donate::new(Some(""), Some(m.clone())).visible());
        let mut d = Donate::new(Some("https://buymeacoffee.com/example"), Some(m.clone()));
        assert!(d.visible() && d.qr.is_some());
        d.never = true;
        d.close();
        assert!(!d.visible() && m.exists());
        assert!(!Donate::new(Some("https://buymeacoffee.com/example"), Some(m)).visible());
    }
}
