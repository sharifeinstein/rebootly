//! The copy itself: progress with ETA and current file, pause/resume and a
//! safe cancel (the job can be resumed), then a bell.

use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Gauge, Paragraph, Wrap};
use rebootly_core::backup::manifest::{FileStatus, Manifest};
use rebootly_core::backup::preflight::gib;
use rebootly_core::backup::sensitive::Passphrase;
use rebootly_core::backup::summary::duration_words;
use rebootly_core::backup::{self, Event, Flow, Job, Plan, RunOptions};

use super::BackupCtx;
use crate::app::Ctx;
use crate::ui::{ACCENT, BAD, WARN, bold, chrome, dim, popup};

/// What the done screen shows.
pub struct Outcome {
    pub result: Result<Manifest, String>,
    pub elapsed: Duration,
}

#[derive(Default)]
pub struct Progress {
    pub files: usize,
    pub bytes: u64,
    pub done: usize,
    pub failed: usize,
    pub already: usize,
    pub current: String,
    pub stage: String,
    active: Duration,
    since: Option<Instant>,
}

impl Progress {
    /// Time spent copying (pauses not counted).
    fn tick(&mut self, paused: bool) {
        let now = Instant::now();
        if let Some(t) = self.since.take() {
            if !paused {
                self.active += now - t;
            }
        }
        self.since = Some(now);
    }

    /// Remaining time from the average rate so far; `None` early on.
    pub fn eta(&self, copied: u64) -> Option<Duration> {
        let secs = self.active.as_secs_f64();
        if secs < 3.0 || copied == 0 {
            return None;
        }
        let rate = copied as f64 / secs;
        let left = self.bytes.saturating_sub(copied) as f64;
        Some(Duration::from_secs_f64(left / rate))
    }

    pub fn lines(&self, copied: u64, paused: bool) -> Vec<Line<'static>> {
        let copied = copied.min(self.bytes);
        let rate = copied as f64 / self.active.as_secs_f64().max(0.001);
        let mut l = vec![Line::styled(
            format!(
                "{} of {} · {} of {} files{}",
                gib(copied),
                gib(self.bytes),
                self.done,
                self.files,
                if self.already > 0 {
                    format!(" ({} copied before)", self.already)
                } else {
                    String::new()
                }
            ),
            bold(),
        )];
        let when = match (paused, self.eta(copied)) {
            (true, _) => "Paused".to_string(),
            (false, Some(d)) => format!("{} left · {}/s", duration_words(d), gib(rate as u64)),
            (false, None) => "Working out the time left…".to_string(),
        };
        l.push(Line::raw(when));
        if self.failed > 0 {
            l.push(Line::styled(
                format!(
                    "{} file(s) couldn't be copied (listed in the report)",
                    self.failed
                ),
                Style::default().fg(BAD),
            ));
        }
        if !self.stage.is_empty() {
            l.push(Line::styled(self.stage.clone(), Style::default().fg(WARN)));
        }
        l.push(Line::default());
        l.push(Line::styled(format!("Last: {}", self.current), dim()));
        l
    }
}

fn draw(f: &mut Frame, p: &Progress, flow: &Flow, dir: &Path, confirm: bool) {
    let area = chrome(
        f,
        "Backing up",
        "p pause/resume · c stop (you can continue later)",
    );
    let [head, gauge, body] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(3),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new(Line::styled(format!("Into {}", dir.display()), dim())),
        head,
    );
    let copied = flow.bytes().min(p.bytes);
    let ratio = if p.bytes == 0 {
        if p.files == 0 {
            1.0
        } else {
            p.done as f64 / p.files as f64
        }
    } else {
        copied as f64 / p.bytes as f64
    };
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(ACCENT))
            .ratio(ratio.clamp(0.0, 1.0)),
        gauge,
    );
    f.render_widget(
        Paragraph::new(p.lines(copied, flow.paused())).wrap(Wrap { trim: false }),
        body.inner(ratatui::layout::Margin::new(0, 1)),
    );
    if confirm {
        popup(
            f,
            "Stop the backup?",
            vec![
                Line::raw("Files copied so far are kept; choose this backup again to continue it."),
                Line::styled("y stop · n keep going", bold()),
            ],
        );
    }
}

pub fn run_backup(
    cx: &mut Ctx,
    ctx: &BackupCtx,
    plan: &Plan,
    job: &Job,
    passphrase: Option<Passphrase>,
) -> Outcome {
    let started = Instant::now();
    let _awake = cx.platform.keep_awake(ctx.live);
    let flow = Flow::default();
    let ro = RunOptions {
        passphrase,
        flow: flow.clone(),
        ..RunOptions::default()
    };
    let mut p = Progress {
        stage: cx.platform.power_warning().unwrap_or_default(),
        ..Progress::default()
    };
    let mut confirm = false;
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let ui = &mut *cx.ui;
    let result = backup::run(ctx.src.as_ref(), plan, job, &ro, &mut |e| {
        p.tick(flow.paused());
        match e {
            Event::Start {
                files,
                bytes,
                already_done,
            } => {
                p.files = files;
                p.bytes = bytes;
                p.already = already_done;
            }
            Event::File(f) => {
                p.done += 1;
                if f.status == FileStatus::Failed {
                    p.failed += 1;
                }
                p.current = f.source.clone();
            }
            Event::Sensitive { files } => {
                p.stage = format!("Encrypting {files} key/credential file(s) into sensitive.age…");
            }
            Event::Tick => {}
        }
        while let Ok(Some(k)) = ui.key(Duration::ZERO) {
            match (confirm, k.code) {
                (false, KeyCode::Char('p') | KeyCode::Char(' ')) => flow.set_paused(!flow.paused()),
                (false, KeyCode::Char('c') | KeyCode::Esc) => confirm = true,
                (true, KeyCode::Char('y')) => {
                    confirm = false;
                    p.stage = "Stopping…".into();
                    flow.stop();
                }
                (true, KeyCode::Char('n') | KeyCode::Esc) => confirm = false,
                _ => {}
            }
        }
        if last_draw.elapsed() >= Duration::from_millis(100) {
            let _ = ui.draw(&mut |f| draw(f, &p, &flow, &job.dir, confirm));
            last_draw = Instant::now();
        }
    });
    ui.bell();
    Outcome {
        result: result.map_err(|e| e.to_string()),
        elapsed: started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_from_rate() {
        let mut p = Progress {
            bytes: 100,
            ..Progress::default()
        };
        assert!(p.eta(10).is_none());
        p.active = Duration::from_secs(10);
        let e = p.eta(50).unwrap();
        assert_eq!(e.as_secs(), 10);
    }
}
