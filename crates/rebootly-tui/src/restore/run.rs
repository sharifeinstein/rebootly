//! Running the restore: phase and progress on screen, conflict and "apps
//! done?" questions as boxes, then the result with the checklist.

use std::path::Path;
use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Gauge, Paragraph, Wrap};
use rebootly_core::restore::conflict::Category;
use rebootly_core::restore::map::FileAction;
use rebootly_core::restore::{
    self, Event, Interact, Policy, RestoreHost, RestorePlan, RestoreResult, report,
};

use crate::app::{Ctx, Nav, Screen};
use crate::checklist::Checklist;
use crate::textview::TextView;
use crate::ui::{ACCENT, Ui, bold, chrome, dim, popup};

struct TuiInteract<'a> {
    ui: &'a mut dyn Ui,
    title: String,
    phase: String,
    done: usize,
    total: usize,
    notes: Vec<String>,
}

impl TuiInteract<'_> {
    fn draw(&mut self, ask: Option<(&str, Vec<Line<'static>>)>) {
        let (title, phase, notes) = (&self.title, &self.phase, &self.notes);
        let ratio = if self.total == 0 {
            0.0
        } else {
            self.done as f64 / self.total as f64
        };
        let _ = self.ui.draw(&mut |f: &mut Frame| {
            let area = chrome(f, title, "");
            let [head, gauge, body] = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Min(2),
            ])
            .areas(area);
            f.render_widget(Paragraph::new(Line::styled(phase.clone(), bold())), head);
            f.render_widget(
                Gauge::default()
                    .gauge_style(ratatui::style::Style::default().fg(ACCENT))
                    .ratio(ratio.min(1.0)),
                gauge,
            );
            let l: Vec<Line> = notes
                .iter()
                .map(|n| Line::styled(n.clone(), dim()))
                .collect();
            f.render_widget(
                Paragraph::new(l).wrap(Wrap { trim: true }),
                body.inner(ratatui::layout::Margin::new(0, 1)),
            );
            if let Some((t, lines)) = &ask {
                popup(f, t, lines.clone());
            }
        });
    }

    /// Waits for one of `keys` (a scripted UI that runs dry gets `None`).
    fn ask(&mut self, title: &str, lines: Vec<Line<'static>>, keys: &[char]) -> Option<char> {
        loop {
            self.draw(Some((title, lines.clone())));
            match self.ui.key(Duration::from_millis(250)) {
                Ok(Some(KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                })) if keys.contains(&c) => return Some(c),
                Ok(Some(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                })) if keys.contains(&'\n') => return Some('\n'),
                Ok(_) if !self.ui.finished() => {}
                _ => return None,
            }
        }
    }
}

impl Interact for TuiInteract<'_> {
    fn conflict(&mut self, f: &FileAction, _existing: &Path) -> (Policy, bool) {
        let cat = Category::as_str(f.category);
        let lines = vec![
            Line::raw(format!(
                "{} is already there and differs (from {}).",
                f.dst, f.from
            )),
            Line::styled("s skip · o overwrite · n newer wins · k keep both", bold()),
            Line::styled(
                format!("Capital letter (S, O, N, K) = the same for all {cat}"),
                dim(),
            ),
        ];
        let keys = ['s', 'o', 'n', 'k', 'S', 'O', 'N', 'K'];
        let Some(c) = self.ask("File already there", lines, &keys) else {
            return (Policy::Skip, true);
        };
        let p = match c.to_ascii_lowercase() {
            'o' => Policy::Overwrite,
            'n' => Policy::OverwriteIfNewer,
            'k' => Policy::KeepBoth,
            _ => Policy::Skip,
        };
        (p, c.is_ascii_uppercase())
    }

    fn apps_done(&mut self) -> bool {
        let lines = vec![
            Line::raw(
                "winget has finished. Install any other apps you need now (they're listed in the plan).",
            ),
            Line::styled(
                "Enter continue with files and settings · q stop here",
                bold(),
            ),
        ];
        !matches!(self.ask("Apps", lines, &['\n', 'q']), Some('q'))
    }

    fn event(&mut self, e: Event<'_>) {
        match e {
            Event::Phase(p) => {
                self.phase = format!("{}…", p.label());
                self.done = 0;
                self.total = 0;
            }
            Event::Progress { done, total } => {
                self.done = done;
                self.total = total;
            }
            Event::Note(n) => {
                self.notes.push(n.to_owned());
                if self.notes.len() > 12 {
                    self.notes.remove(0);
                }
            }
        }
        self.draw(None);
    }
}

pub fn run_restore(
    cx: &mut Ctx,
    plan: &RestorePlan,
    host: &dyn RestoreHost,
    dry_run: bool,
) -> RestoreResult {
    let mut ui = TuiInteract {
        ui: &mut *cx.ui,
        title: if dry_run {
            "Dry run (nothing is changed)".into()
        } else {
            "Restoring".into()
        },
        phase: String::new(),
        done: 0,
        total: 0,
        notes: Vec::new(),
    };
    let r = restore::execute(plan, host, &mut ui, dry_run);
    cx.ui.bell();
    r
}

pub struct ResultScreen {
    view: TextView,
    checklist: Checklist,
    dry_run: bool,
}

impl ResultScreen {
    pub fn new(cx: &mut Ctx, plan: &RestorePlan, r: RestoreResult) -> Self {
        if r.failed() > 0 {
            cx.exit = cx.exit.max(2);
        }
        let mut text = report::result_text(plan, &r);
        if let Some(d) = &r.undo_dir {
            text.push_str(&format!(
                "\nTo undo this restore: rebootly undo   (saved in {})\n",
                d.display()
            ));
        }
        ResultScreen {
            view: TextView::plain(&text),
            checklist: Checklist::new(r.manual_steps),
            dry_run: r.dry_run,
        }
    }
}

impl Screen for ResultScreen {
    fn title(&self) -> String {
        if self.dry_run {
            "Dry run done".into()
        } else {
            "Restore done".into()
        }
    }

    fn hints(&self) -> String {
        "↑↓ move · Space tick · PgUp PgDn scroll · Enter back to start · q quit".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [top, rest] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Min(6)]).areas(area);
        self.view.draw(f, top);
        self.checklist.draw(f, rest);
    }

    fn key(&mut self, k: KeyEvent, _cx: &mut Ctx) -> Nav {
        if matches!(k.code, KeyCode::PageUp | KeyCode::PageDown) {
            self.view.key(&k);
            return Nav::Stay;
        }
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
