//! End of a backup: result, where the reports are, and the manual steps.

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::manifest::FileStatus;
use rebootly_core::backup::preflight::gib;
use rebootly_core::backup::summary::duration_words;

use super::progress::Outcome;
use crate::app::{Ctx, Nav, Screen};
use crate::checklist::Checklist;
use crate::ui::{BAD, GOOD, WARN, bold, dim};

pub struct DoneScreen {
    lines: Vec<Line<'static>>,
    checklist: Checklist,
}

impl DoneScreen {
    pub fn new(cx: &mut Ctx, out: Outcome, dir: PathBuf) -> Self {
        let mut lines = Vec::new();
        let steps = match &out.result {
            Err(e) => {
                cx.exit = cx.exit.max(1);
                lines.push(Line::styled(
                    format!("The backup stopped: {e}"),
                    Style::default().fg(BAD),
                ));
                lines.push(Line::raw(
                    "Choose the same target again to continue where it stopped.",
                ));
                Vec::new()
            }
            Ok(m) => {
                let (ok, _, failed) = m.counts();
                let bytes: u64 = m
                    .files
                    .iter()
                    .filter(|f| f.status == FileStatus::Ok)
                    .map(|f| f.size)
                    .sum();
                let (msg, color) = match (m.complete, failed) {
                    (false, _) => (
                        "Stopped. Choose the same target again to continue it.",
                        WARN,
                    ),
                    (true, 0) => ("Backup finished.", GOOD),
                    (true, _) => ("Backup finished, but some files couldn't be copied.", WARN),
                };
                if failed > 0 {
                    cx.exit = cx.exit.max(2);
                }
                lines.push(Line::styled(msg, bold().fg(color)));
                lines.push(Line::raw(format!(
                    "{ok} files copied ({}), {failed} failed, in {}.",
                    gib(bytes),
                    duration_words(out.elapsed).trim_start_matches("about ")
                )));
                if m.sensitive.is_some() {
                    lines.push(Line::raw(
                        "Keys and credentials are in sensitive.age; keep the passphrase safe.",
                    ));
                }
                m.manual_steps.clone()
            }
        };
        lines.push(Line::styled(
            format!("Backup folder: {}", dir.display()),
            dim(),
        ));
        lines.push(Line::styled(
            format!(
                "Printable report with this checklist: {}",
                dir.join("report.html").display()
            ),
            dim(),
        ));
        DoneScreen {
            lines,
            checklist: Checklist::new(steps),
        }
    }
}

impl Screen for DoneScreen {
    fn title(&self) -> String {
        "Backup done".into()
    }

    fn hints(&self) -> String {
        "↑↓ move · Space tick · Enter back to start · q quit".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let n = self.lines.len() as u16 + 1;
        let [top, rest] = Layout::vertical([Constraint::Length(n), Constraint::Min(3)]).areas(area);
        f.render_widget(
            Paragraph::new(self.lines.clone()).wrap(Wrap { trim: false }),
            top,
        );
        self.checklist.draw(f, rest);
    }

    fn key(&mut self, k: KeyEvent, _cx: &mut Ctx) -> Nav {
        if self.checklist.key(&k) {
            return Nav::Stay;
        }
        match k.code {
            KeyCode::Enter | KeyCode::Esc => Nav::Home,
            KeyCode::Char('q') => Nav::Quit,
            _ => Nav::Ignored,
        }
    }
}
