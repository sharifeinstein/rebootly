//! Manual steps as tick-boxes (end of backup and restore, SPEC §11); the
//! selected step's details are shown below the list.

use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::manifest::ManualStep;

use crate::ui::{bold, dim};
use crate::widgets::{Check, Checks};

pub struct Checklist {
    steps: Vec<ManualStep>,
    checks: Checks,
}

impl Checklist {
    pub fn new(steps: Vec<ManualStep>) -> Self {
        let rows = steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Check::item(
                    &format!("step:{i}"),
                    &format!("{}. {}", i + 1, s.title),
                    String::new(),
                    false,
                )
            })
            .collect();
        Checklist {
            steps,
            checks: Checks::new(rows),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn done(&self) -> usize {
        self.checks.ticked("step:").len()
    }

    /// Space ticks; arrows move. `a`/`n` are left to the caller.
    pub fn key(&mut self, k: &KeyEvent) -> bool {
        use ratatui::crossterm::event::KeyCode;
        if matches!(k.code, KeyCode::Char('a') | KeyCode::Char('n')) {
            return false;
        }
        self.checks.key(k)
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        if self.steps.is_empty() {
            f.render_widget(
                Paragraph::new(Line::styled("Nothing to do by hand.", dim())),
                area,
            );
            return;
        }
        let [head, list, detail] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(4),
        ])
        .areas(area);
        f.render_widget(
            Paragraph::new(Line::styled(
                format!(
                    "Things to do by hand ({} of {} done)",
                    self.done(),
                    self.steps.len()
                ),
                bold(),
            )),
            head,
        );
        self.checks.draw(f, list);
        if let Some(s) = self.steps.get(self.checks.cursor) {
            f.render_widget(
                Paragraph::new(Line::styled(s.detail.clone(), dim())).wrap(Wrap { trim: true }),
                detail,
            );
        }
    }
}
