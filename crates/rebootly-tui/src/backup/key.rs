//! The recovery-key field of the Rescue flow's unlock step.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use crate::ui::dim;
use crate::widgets::TextField;

/// Typing a recovery key for a locked volume.
pub struct KeyEntry {
    pub device: String,
    pub field: TextField,
}

/// The key field: dots, and how many digits arrived so far (typing a long
/// key blind is error-prone; the count shows a missed or doubled digit).
pub fn draw_key(f: &mut Frame, area: Rect, k: &KeyEntry) {
    let [label, field, count] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new(format!(
            "Recovery key for {} (Microsoft account: aka.ms/myrecoverykey, or the printout/USB \
             saved when BitLocker was turned on):",
            k.device
        ))
        .wrap(Wrap { trim: true }),
        label,
    );
    k.field.draw(f, field, true);
    let digits = k.field.value.chars().filter(char::is_ascii_digit).count();
    f.render_widget(
        Paragraph::new(Line::styled(
            format!("{digits} of 48 digits (dashes optional)"),
            dim(),
        )),
        count,
    );
}
