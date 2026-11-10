//! The restore on a worker thread. Questions (a file that differs under an
//! "ask" policy, "apps done?") go to the window over a channel and the
//! worker waits for the answer.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, RichText};
use rebootly_core::restore::conflict::Category;
use rebootly_core::restore::map::FileAction;
use rebootly_core::restore::{self, Event, Interact, Policy, RestoreResult, report};

use super::restore_plan::Planned;
use crate::app::{Cx, Nav, Page};
use crate::task::Task;
use crate::widgets::{Checklist, busy, error, primary};

enum Question {
    Conflict { text: String, category: String },
    AppsDone,
}

enum Answer {
    Conflict(Policy, bool),
    Apps(bool),
}

#[derive(Default)]
struct Prog {
    phase: String,
    done: usize,
    total: usize,
    notes: Vec<String>,
}

struct ThreadInteract {
    prog: Arc<Mutex<Prog>>,
    ask: Sender<Question>,
    answers: Receiver<Answer>,
    ctx: egui::Context,
}

impl ThreadInteract {
    fn ask(&mut self, q: Question) -> Option<Answer> {
        self.ask.send(q).ok()?;
        self.ctx.request_repaint();
        self.answers.recv().ok()
    }
}

impl Interact for ThreadInteract {
    fn conflict(&mut self, f: &FileAction, _existing: &Path) -> (Policy, bool) {
        let q = Question::Conflict {
            text: format!("{} is already there and differs (from {}).", f.dst, f.from),
            category: Category::as_str(f.category).to_string(),
        };
        match self.ask(q) {
            Some(Answer::Conflict(p, all)) => (p, all),
            _ => (Policy::Skip, true),
        }
    }

    fn apps_done(&mut self) -> bool {
        !matches!(self.ask(Question::AppsDone), Some(Answer::Apps(false)))
    }

    fn event(&mut self, e: Event<'_>) {
        let mut g = self.prog.lock().unwrap_or_else(|e| e.into_inner());
        match e {
            Event::Phase(p) => (g.phase, g.done, g.total) = (format!("{}…", p.label()), 0, 0),
            Event::Progress { done, total } => (g.done, g.total) = (done, total),
            Event::Note(n) => {
                g.notes.push(n.to_owned());
                if g.notes.len() > 12 {
                    g.notes.remove(0);
                }
            }
        }
        self.ctx.request_repaint();
    }
}

/// The finished restore (result and its report text) or why it couldn't run.
type Outcome = Result<(RestoreResult, String), String>;

pub struct RestoreRunPage {
    dry_run: bool,
    prog: Arc<Mutex<Prog>>,
    questions: Receiver<Question>,
    answers: Sender<Answer>,
    open: Option<Question>,
    task: Task<Outcome>,
    done: Option<(Outcome, Checklist)>,
}

impl RestoreRunPage {
    pub fn new(cx: &mut Cx, p: Arc<Planned>, dry_run: bool) -> Self {
        let prog = Arc::new(Mutex::new(Prog::default()));
        let (qtx, qrx) = mpsc::channel();
        let (atx, arx) = mpsc::channel();
        let mut it = ThreadInteract {
            prog: prog.clone(),
            ask: qtx,
            answers: arx,
            ctx: cx.ctx.clone(),
        };
        let platform = cx.platform.clone();
        let task = Task::spawn(&cx.ctx, move || {
            let user = p.opts.user.clone().unwrap_or_default();
            let (target, host) = platform
                .restore_target(dry_run, &user)
                .map_err(|e| format!("{e:#}"))?;
            let plan = restore::plan(
                &p.backup,
                &target,
                &platform.packs(),
                &p.opts,
                p.bundle.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            if !plan.blockers.is_empty() {
                return Err(format!("Can't restore: {}", plan.blockers.join("; ")));
            }
            let r = restore::execute(&plan, host.as_ref(), &mut it, dry_run);
            let text = report::result_text(&plan, &r);
            Ok((r, text))
        });
        RestoreRunPage {
            dry_run,
            prog,
            questions: qrx,
            answers: atx,
            open: None,
            task,
            done: None,
        }
    }

    fn question_ui(&mut self, ctx: &egui::Context) {
        if self.open.is_none() {
            self.open = self.questions.try_recv().ok();
        }
        let Some(q) = &self.open else { return };
        let mut answer = None;
        egui::Modal::new(egui::Id::unique("restore-question")).show(ctx, |ui| match q {
            Question::Conflict { text, category } => {
                ui.heading("File already there");
                ui.label(text);
                let mut all = false;
                ui.checkbox(&mut all, format!("Do the same for all {category}"));
                ui.horizontal(|ui| {
                    for (label, p) in [("Skip", Policy::Skip), ("Overwrite", Policy::Overwrite), ("Newer wins", Policy::OverwriteIfNewer), ("Keep both", Policy::KeepBoth)] {
                        if ui.button(label).clicked() {
                            answer = Some(Answer::Conflict(p, all));
                        }
                    }
                });
            }
            Question::AppsDone => {
                ui.heading("Apps");
                ui.label("winget has finished. Install any other apps you need now (they're listed in the preview).");
                ui.horizontal(|ui| {
                    if ui.button("Continue with files and settings").clicked() {
                        answer = Some(Answer::Apps(true));
                    }
                    if ui.button("Stop here").clicked() {
                        answer = Some(Answer::Apps(false));
                    }
                });
            }
        });
        if let Some(a) = answer {
            let _ = self.answers.send(a);
            self.open = None;
        }
    }
}

impl Page for RestoreRunPage {
    fn title(&self) -> String {
        match (self.done.is_some(), self.dry_run) {
            (false, true) => "Dry run (nothing is changed)".into(),
            (false, false) => "Restoring".into(),
            (true, true) => "Dry run done".into(),
            (true, false) => "Restore done".into(),
        }
    }

    fn can_go_back(&self) -> bool {
        self.done.is_some() && self.dry_run
    }

    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx) -> Nav {
        if self.done.is_none()
            && let Some(r) = self.task.take()
        {
            let steps = r
                .as_ref()
                .map(|(r, _)| r.manual_steps.clone())
                .unwrap_or_default();
            self.done = Some((r, Checklist::new(steps)));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
        }
        let Some((result, checklist)) = &mut self.done else {
            let g = self.prog.lock().unwrap_or_else(|e| e.into_inner());
            busy(
                ui,
                if g.phase.is_empty() {
                    "Getting ready…"
                } else {
                    &g.phase
                },
            );
            let ratio = if g.total == 0 {
                0.0
            } else {
                g.done as f32 / g.total as f32
            };
            ui.add(egui::ProgressBar::new(ratio));
            for n in &g.notes {
                ui.label(RichText::new(n).weak());
            }
            drop(g);
            self.question_ui(ui.ctx());
            ui.ctx().request_repaint_after(Duration::from_millis(250));
            return Nav::Stay;
        };
        match result {
            Err(e) => error(ui, e),
            Ok((r, text)) => {
                if let Some(d) = &r.undo_dir {
                    ui.label(format!(
                        "To undo it later: Home, then Undo a restore (saved in {}).",
                        d.display()
                    ));
                }
                egui::CollapsingHeader::new("Details").show(ui, |ui| {
                    ui.label(RichText::new(text.as_str()).monospace());
                });
            }
        }
        ui.add_space(6.0);
        checklist.ui(ui);
        if primary(ui, "Done", true) {
            return Nav::Home;
        }
        Nav::Stay
    }
}
