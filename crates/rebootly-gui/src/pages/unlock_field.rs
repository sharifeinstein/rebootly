//! The unlock field for a locked drive: its recovery key (shown as typed,
//! grouped `123456-234567-…`, a key in pasted text picked out) or its
//! BitLocker password (dots). A TPM PIN can't be used off the original
//! PC's own boot, so it isn't offered.

use eframe::egui::{self, RichText};
use rebootly_core::volume::{Password, RecoveryKey, Unlock, key_status, tidy_key};

#[derive(Default)]
pub struct UnlockField {
    pub password: bool,
    pub text: String,
}

impl UnlockField {
    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// What was typed, checked; the error is a plain sentence.
    pub fn parse(&self) -> Result<Unlock, String> {
        if self.password {
            Password::new(&self.text)
                .map(Unlock::from)
                .map_err(|e| e.to_string())
        } else {
            RecoveryKey::parse(&self.text)
                .map(Unlock::from)
                .map_err(|e| e.to_string())
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let was = self.password;
            ui.radio_value(&mut self.password, false, "Recovery key");
            ui.radio_value(&mut self.password, true, "Password");
            if was != self.password {
                self.text.clear();
            }
        });
        let help = if self.password {
            "The password set for this drive when BitLocker was turned on. (A PIN used with \
             the PC's TPM can't be used here: only the recovery key opens such a drive.)"
        } else {
            "Recovery key (48 digits): for a Microsoft account at aka.ms/myrecoverykey, or on \
             the printout/USB saved when BitLocker was turned on."
        };
        ui.label(RichText::new(help).weak());
        if self.password {
            ui.horizontal(|ui| {
                let l = ui.label("Password");
                ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .password(true)
                        .desired_width(460.0),
                )
                .labelled_by(l.id);
            });
            let n = self.text.chars().count();
            ui.label(RichText::new(format!("{n} characters")).weak());
            return;
        }
        ui.horizontal(|ui| {
            let l = ui.label("Recovery key");
            // Shown as typed, in groups of 6 (dashes are added), so it can
            // be checked against the printout.
            let out = egui::TextEdit::singleline(&mut self.text)
                .font(egui::TextStyle::Monospace)
                .desired_width(460.0)
                .show(ui);
            let resp = out.response.response.clone().labelled_by(l.id);
            if resp.changed() {
                let g = tidy_key(&self.text);
                if g != self.text {
                    self.text = g;
                    // Keep the cursor at the end after dashes are added.
                    let mut st = out.state;
                    let end = egui::text::CCursor::new(self.text.chars().count());
                    st.cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(end)));
                    st.store(ui.ctx(), resp.id);
                }
            }
        });
        ui.label(RichText::new(key_status(&self.text)).weak());
    }
}
