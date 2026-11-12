//! The unlock field of the Rescue flow: the recovery key (shown as typed,
//! grouped) or, after Tab, the drive's BitLocker password (dots).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use rebootly_core::volume::{key_status, tidy_key};

use crate::ui::dim;
use crate::widgets::TextField;

/// Typing a recovery key or password for a locked volume.
pub struct KeyEntry {
    pub device: String,
    pub field: TextField,
    /// The password instead of the recovery key.
    pub password: bool,
}

impl KeyEntry {
    pub fn new(device: &str) -> Self {
        KeyEntry {
            device: device.into(),
            field: TextField::new("", false),
            password: false,
        }
    }

    /// Pasted text: a key is picked out of it; a password takes its first
    /// line (a copied line often ends with a line break).
    pub fn paste(&mut self, text: &str) {
        let v = &mut self.field.value;
        if self.password {
            v.push_str(text.lines().next().unwrap_or(""));
        } else {
            *v = tidy_key(&format!("{v}{text}"));
        }
    }
}

/// The key field: the digits in groups of 6, as on Windows' own recovery
/// screen, and how many arrived so far or which group has a typo.
pub fn draw_key(f: &mut Frame, area: Rect, k: &KeyEntry) {
    let [label, field, count] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .areas(area);
    let text = if k.password {
        format!(
            "Password for {} (the one set when BitLocker was turned on; a TPM PIN can't be \
             used here). Tab: use the recovery key instead.",
            k.device
        )
    } else {
        format!(
            "Recovery key for {} (Microsoft account: aka.ms/myrecoverykey, or the printout/USB \
             saved when BitLocker was turned on). Tab: use the drive's password instead.",
            k.device
        )
    };
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), label);
    k.field.draw(f, field, true);
    let status = if k.password {
        let n = k.field.value.chars().count();
        format!("{n} characters")
    } else {
        key_status(&k.field.value)
    };
    f.render_widget(Paragraph::new(Line::styled(status, dim())), count);
}
