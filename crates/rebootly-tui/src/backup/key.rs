//! The recovery-key field of the Rescue flow's unlock step.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use rebootly_core::volume::key_status;

use crate::ui::dim;
use crate::widgets::TextField;

/// Typing a recovery key for a locked volume.
pub struct KeyEntry {
    pub device: String,
    pub field: TextField,
}

/// The key field: the digits in groups of 6, as on Windows' own recovery
/// screen, and how many arrived so far or which group has a typo.
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
    f.render_widget(
        Paragraph::new(Line::styled(key_status(&k.field.value), dim())),
        count,
    );
}
