//! Plain-language pre-flight summary (SPEC §11), the live-mode reminders,
//! saving a job profile, the passphrase, and the start of the copy.

use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use rebootly_core::backup::preflight::{Preflight, gib};
use rebootly_core::backup::profile::{self, JobProfile, PROFILE_SCHEMA, Selection};
use rebootly_core::backup::sensitive::Passphrase;
use rebootly_core::backup::summary::{Summary, speed_probe, summarize};
use rebootly_core::backup::{self, Job, Plan};
use rebootly_core::restore::find::find_backups;

use super::Shared;
use super::done::DoneScreen;
use super::progress::run_backup;
use crate::app::{Ctx, Nav, Screen};
use crate::ui::{BAD, GOOD, WARN, bold, dim};
use crate::widgets::TextField;

enum Input {
    None,
    ProfileName(TextField),
    Pass(TextField, Option<TextField>),
}

pub struct PreflightScreen {
    ctx: Shared,
    sel: Selection,
    target: PathBuf,
    ready: Result<(Plan, Preflight, Summary), String>,
    resumable: Option<PathBuf>,
    input: Input,
    message: Option<(String, bool)>,
}

fn nearest_existing(p: &Path) -> PathBuf {
    p.ancestors()
        .find(|a| a.is_dir())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

impl PreflightScreen {
    pub fn new(cx: &mut Ctx, ctx: Shared, sel: Selection, target: PathBuf) -> Self {
        cx.busy(
            "Pre-flight",
            "Counting files and testing the target's speed…",
        );
        let probe = nearest_existing(&target);
        let ready = ctx.plan(&sel).map_err(|e| format!("{e:#}")).map(|plan| {
            let pre = backup::preflight(&plan, &probe, None, cx.platform.target_fs(&probe));
            let speed = speed_probe(&probe);
            let sum = summarize(&plan, &pre, speed);
            (plan, pre, sum)
        });
        let machine = ready
            .as_ref()
            .ok()
            .and_then(|(p, _, _)| p.source.machine.clone());
        let resumable = find_backups(std::slice::from_ref(&target))
            .into_iter()
            .find(|b| !b.complete && Some(&b.machine) == machine.as_ref())
            .map(|b| b.dir);
        PreflightScreen {
            ctx,
            sel,
            target,
            ready,
            resumable,
            input: Input::None,
            message: None,
        }
    }

    fn needs_passphrase(&self) -> bool {
        matches!(&self.ready, Ok((p, _, _)) if self.sel.sensitive && !p.sensitive.is_empty())
    }

    fn start(&mut self, cx: &mut Ctx, pass: Option<Passphrase>, resume: bool) -> Nav {
        let Ok((plan, _, _)) = &self.ready else {
            return Nav::Stay;
        };
        let job = match (resume, &self.resumable) {
            (true, Some(dir)) => Job::resume(dir),
            _ => Job::create(&self.target, plan.source.machine.as_deref()),
        };
        let job = match job {
            Ok(j) => j,
            Err(e) => {
                self.message = Some((format!("Can't create the backup folder: {e}"), true));
                return Nav::Stay;
            }
        };
        let out = run_backup(cx, &self.ctx, plan, &job, pass);
        Nav::Replace(Box::new(DoneScreen::new(cx, out, job.dir)))
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let (plan, pre, sum) = match &self.ready {
            Ok(r) => r,
            Err(e) => {
                return vec![Line::styled(
                    format!("Can't plan this backup: {e}"),
                    Style::default().fg(BAD),
                )];
            }
        };
        let mut l = vec![
            Line::styled(sum.headline.clone(), bold()),
            Line::styled(format!("Target: {}", self.target.display()), dim()),
            Line::default(),
        ];
        for f in pre.folders.iter().take(12) {
            l.push(Line::raw(format!(
                "  {:<36} {:>7} files {:>11}",
                f.folder,
                f.files,
                gib(f.bytes)
            )));
        }
        if pre.folders.len() > 12 {
            l.push(Line::styled(
                format!("  … and {} more", pre.folders.len() - 12),
                dim(),
            ));
        }
        if !plan.sensitive.is_empty() {
            let msg = if self.sel.sensitive {
                format!(
                    "{} key/credential file(s) go into the encrypted sensitive.age",
                    plan.sensitive.len()
                )
            } else {
                format!(
                    "{} key/credential location(s) left out (choose Full or tick them in Custom)",
                    plan.sensitive_jobs.len()
                )
            };
            l.push(Line::raw(format!("  {msg}")));
        }
        l.push(Line::default());
        for w in &pre.warnings {
            l.push(Line::styled(format!("! {w}"), Style::default().fg(WARN)));
        }
        for b in &pre.blockers {
            l.push(Line::styled(format!("✗ {b}"), Style::default().fg(BAD)));
        }
        if self.ctx.live {
            l.push(Line::styled("Before you start:", bold()));
            l.push(Line::raw("  • If you won't use this PC again, deactivate licenses now (Adobe, Office, games, …)."));
            l.push(Line::raw("  • Save your BitLocker recovery key (aka.ms/myrecoverykey) if the disk is encrypted."));
        }
        if let Some(dir) = &self.resumable {
            l.push(Line::styled(
                format!(
                    "An unfinished backup of this PC is there ({}): press r to continue it.",
                    dir.display()
                ),
                Style::default().fg(WARN),
            ));
        }
        l
    }

    fn input_key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Option<Nav> {
        if matches!(self.input, Input::None) {
            return None;
        }
        if k.code == KeyCode::Esc {
            self.input = Input::None;
            return Some(Nav::Stay);
        }
        if k.code != KeyCode::Enter {
            if let Input::ProfileName(t) | Input::Pass(_, Some(t)) | Input::Pass(t, None) =
                &mut self.input
            {
                t.key(&k);
            }
            return Some(Nav::Stay);
        }
        match std::mem::replace(&mut self.input, Input::None) {
            Input::None => {}
            Input::ProfileName(t) => {
                let p = JobProfile {
                    schema: PROFILE_SCHEMA,
                    name: t.value.trim().to_string(),
                    selection: self.sel.clone(),
                    jobs: None,
                };
                self.message = Some(match profile::save(&self.target, &p) {
                    Ok(path) => (format!("Saved job profile: {}", path.display()), false),
                    Err(e) => (format!("Couldn't save the profile: {e}"), true),
                });
            }
            Input::Pass(first, None) => match Passphrase::new(first.value.clone()) {
                Ok(_) => {
                    self.message = None;
                    self.input = Input::Pass(first, Some(TextField::new("", true)));
                }
                Err(e) => {
                    let n = first.value.chars().count();
                    self.message = Some((format!("{e} (got {n})"), true));
                    self.input = Input::Pass(first, None);
                }
            },
            Input::Pass(first, Some(again)) => {
                if first.value != again.value {
                    self.message =
                        Some(("The two passphrases differ; type it again.".into(), true));
                    self.input = Input::Pass(TextField::new("", true), None);
                } else {
                    self.message = None;
                    let pass = Passphrase::new(first.value).ok();
                    return Some(self.start(cx, pass, false));
                }
            }
        }
        Some(Nav::Stay)
    }
}

impl Screen for PreflightScreen {
    fn title(&self) -> String {
        format!("Back up {}: ready?", self.ctx.label)
    }

    fn hints(&self) -> String {
        match self.input {
            Input::None => {
                "Enter start · s save as job profile · f start despite problems · Esc back".into()
            }
            _ => "Enter OK · Esc cancel".into(),
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let mut l = self.lines();
        if let Some((m, bad)) = &self.message {
            l.push(Line::styled(
                m.clone(),
                Style::default().fg(if *bad { BAD } else { GOOD }),
            ));
        }
        match &self.input {
            Input::None => {}
            Input::ProfileName(t) => {
                l.push(Line::styled(
                    "Name for this job profile (saved beside the backups):",
                    bold(),
                ));
                l.push(t.line(true));
            }
            Input::Pass(first, again) => {
                l.push(Line::from(vec![
                    Span::styled("Passphrase for keys and credentials ", bold()),
                    Span::styled("(at least 8 characters; it is not stored anywhere)", dim()),
                ]));
                l.push(first.line(again.is_none()));
                if let Some(a) = again {
                    l.push(Line::styled("Type it again:", bold()));
                    l.push(a.line(true));
                }
            }
        }
        f.render_widget(Paragraph::new(l).wrap(Wrap { trim: false }), area);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if let Some(nav) = self.input_key(k, cx) {
            return nav;
        }
        let Ok((_, pre, _)) = &self.ready else {
            return Nav::Ignored;
        };
        let source_blocker = pre.blockers.iter().any(|b| b.contains("source disk"));
        let blocked = !pre.blockers.is_empty();
        match k.code {
            KeyCode::Char('s') => {
                self.input = Input::ProfileName(TextField::new("", false));
                Nav::Stay
            }
            KeyCode::Char('r') if self.resumable.is_some() => {
                if self.needs_passphrase() {
                    self.message = Some((
                        "Resume this backup from the CLI with --resume and --sensitive.".into(),
                        true,
                    ));
                    return Nav::Stay;
                }
                self.start(cx, None, true)
            }
            KeyCode::Enter | KeyCode::Char('f') => {
                if blocked && (k.code == KeyCode::Enter || source_blocker) {
                    self.message = Some((
                        if source_blocker {
                            "The target is on the disk being backed up; choose another target."
                                .into()
                        } else {
                            "Fix the problems above, or press f to start anyway.".into()
                        },
                        true,
                    ));
                    return Nav::Stay;
                }
                if self.needs_passphrase() {
                    self.input = Input::Pass(TextField::new("", true), None);
                    return Nav::Stay;
                }
                self.start(cx, None, false)
            }
            _ => Nav::Ignored,
        }
    }
}
