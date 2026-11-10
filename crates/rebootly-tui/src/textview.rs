//! A scrollable block of text (restore plans and results).

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

pub struct TextView {
    pub lines: Vec<Line<'static>>,
    pub scroll: u16,
}

impl TextView {
    pub fn new(lines: Vec<Line<'static>>) -> Self {
        TextView { lines, scroll: 0 }
    }

    pub fn plain(text: &str) -> Self {
        Self::new(text.lines().map(|l| Line::raw(l.to_owned())).collect())
    }

    /// Up/Down/PageUp/PageDown/Home; true if handled.
    pub fn key(&mut self, k: &KeyEvent) -> bool {
        let max = self.lines.len().saturating_sub(1) as u16;
        self.scroll = match k.code {
            KeyCode::Up => self.scroll.saturating_sub(1),
            KeyCode::Down => (self.scroll + 1).min(max),
            KeyCode::PageUp => self.scroll.saturating_sub(15),
            KeyCode::PageDown => (self.scroll + 15).min(max),
            KeyCode::Home => 0,
            _ => return false,
        };
        true
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        f.render_widget(
            Paragraph::new(self.lines.clone())
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            area,
        );
    }
}
