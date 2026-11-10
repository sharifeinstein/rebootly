//! Restore plan preview (phase 1) and the start of the restore or dry run.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::restore::{self, Bundle, RestoreOptions, RestorePlan, report};

use super::Backup;
use super::run::{ResultScreen, run_restore};
use crate::app::{Ctx, Nav, Screen};
use crate::textview::TextView;
use crate::ui::{BAD, bold};

pub struct PlanScreen {
    backup: Backup,
    opts: RestoreOptions,
    bundle: Option<Bundle>,
    plan: RestorePlan,
    view: TextView,
    confirm: bool,
    error: Option<String>,
}

fn make_plan(
    cx: &mut Ctx,
    backup: &Backup,
    opts: &RestoreOptions,
    bundle: Option<&Bundle>,
    dry_run: bool,
) -> anyhow::Result<(RestorePlan, Box<dyn restore::RestoreHost>)> {
    let user = opts.user.clone().unwrap_or_default();
    let (target, host) = cx.platform.restore_target(dry_run, &user)?;
    let plan = restore::plan(backup, &target, &cx.platform.packs(), opts, bundle)?;
    Ok((plan, host))
}

impl PlanScreen {
    pub fn new(
        cx: &mut Ctx,
        backup: Backup,
        opts: RestoreOptions,
        bundle: Option<Bundle>,
    ) -> anyhow::Result<Self> {
        cx.busy("Restore", "Comparing the backup with this PC…");
        let (plan, _) = make_plan(cx, &backup, &opts, bundle.as_ref(), true)?;
        let mut lines: Vec<Line<'static>> = plan
            .blockers
            .iter()
            .map(|b| Line::styled(format!("✗ {b}"), Style::default().fg(BAD)))
            .collect();
        lines.extend(TextView::plain(&report::plan_text(&plan)).lines);
        Ok(PlanScreen {
            backup,
            opts,
            bundle,
            plan,
            view: TextView::new(lines),
            confirm: false,
            error: None,
        })
    }

    fn run(&mut self, cx: &mut Ctx, dry_run: bool) -> Nav {
        cx.busy("Restore", "Getting ready…");
        let (plan, host) =
            match make_plan(cx, &self.backup, &self.opts, self.bundle.as_ref(), dry_run) {
                Ok(r) => r,
                Err(e) => {
                    self.error = Some(format!("{e:#}"));
                    return Nav::Stay;
                }
            };
        if !plan.blockers.is_empty() {
            self.error = Some(format!("Can't restore: {}", plan.blockers.join("; ")));
            return Nav::Stay;
        }
        let result = run_restore(cx, &plan, host.as_ref(), dry_run);
        Nav::Replace(Box::new(ResultScreen::new(cx, &plan, result)))
    }
}

impl Screen for PlanScreen {
    fn title(&self) -> String {
        format!(
            "Restore into {} ({}): preview",
            self.plan.target.username, self.plan.target.profile
        )
    }

    fn hints(&self) -> String {
        if self.confirm {
            "y restore · n cancel".into()
        } else {
            "↑↓ PgUp PgDn scroll · Enter restore · d dry run (changes nothing) · Esc back".into()
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [body, foot] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(2)]).areas(area);
        self.view.draw(f, body);
        let l = match (&self.error, self.confirm) {
            (Some(e), _) => Line::styled(e.clone(), Style::default().fg(BAD)),
            (None, true) => Line::styled(
                format!(
                    "Restore into {} now? Everything it replaces is kept for Undo.",
                    self.plan.target.username
                ),
                bold(),
            ),
            (None, false) => Line::default(),
        };
        f.render_widget(Paragraph::new(l).wrap(Wrap { trim: true }), foot);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if self.confirm {
            self.confirm = false;
            return match k.code {
                KeyCode::Char('y') => self.run(cx, false),
                _ => Nav::Stay,
            };
        }
        if self.view.key(&k) {
            return Nav::Stay;
        }
        self.error = None;
        match k.code {
            KeyCode::Enter if !self.plan.blockers.is_empty() => {
                self.error = Some("Fix the problems at the top first.".into());
                Nav::Stay
            }
            KeyCode::Enter => {
                self.confirm = true;
                Nav::Stay
            }
            KeyCode::Char('d') => self.run(cx, true),
            _ => Nav::Ignored,
        }
    }
}
