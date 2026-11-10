//! The copy on a worker thread: progress with time left and last file,
//! pause/resume, a safe stop; then the result and the checklist.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use rebootly_core::backup::manifest::{FileStatus, Manifest};
use rebootly_core::backup::preflight::gib;
use rebootly_core::backup::sensitive::Passphrase;
use rebootly_core::backup::summary::duration_words;
use rebootly_core::backup::{self, Event, Flow, Job, Plan, RunOptions};
use rebootly_host::session::Shared;

use crate::app::{BAD, Cx, GOOD, Nav, Page, WARN};
use crate::task::Task;
use crate::widgets::{Checklist, open_path, primary};

#[derive(Default)]
struct Progress {
    files: usize,
    bytes: u64,
    done: usize,
    failed: usize,
    already: usize,
    current: String,
    stage: String,
}

pub struct BackupRunPage {
    dir: PathBuf,
    flow: Flow,
    prog: Arc<Mutex<Progress>>,
    task: Task<Result<Manifest, String>>,
    started: Instant,
    /// Copy time not counting pauses.
    active: Duration,
    last_tick: Instant,
    confirm_stop: bool,
    finished: Option<(Result<Manifest, String>, Duration, Checklist)>,
    told: bool,
    donate: crate::donate::Donate,
}

impl BackupRunPage {
    pub fn new(cx: &Cx, ctx: Shared, plan: Plan, job: Job, pass: Option<Passphrase>) -> Self {
        let flow = Flow::default();
        let prog = Arc::new(Mutex::new(Progress::default()));
        let dir = job.dir.clone();
        let (f, p) = (flow.clone(), prog.clone());
        let task = Task::spawn(&cx.ctx, move || {
            let _awake = rebootly_host::source::Awake::new(ctx.live);
            let ro = RunOptions {
                passphrase: pass,
                flow: f,
                ..RunOptions::default()
            };
            backup::run(ctx.src.as_ref(), &plan, &job, &ro, &mut |e| {
                let mut g = p.lock().unwrap_or_else(|e| e.into_inner());
                match e {
                    Event::Start {
                        files,
                        bytes,
                        already_done,
                    } => {
                        (g.files, g.bytes, g.already) = (files, bytes, already_done);
                    }
                    Event::File(f) => {
                        g.done += 1;
                        g.failed += usize::from(f.status == FileStatus::Failed);
                        g.current = f.source.clone();
                    }
                    Event::Sensitive { files } => {
                        g.stage = format!(
                            "Encrypting {files} key/credential file(s) into sensitive.age…"
                        );
                    }
                    Event::Tick => {}
                }
            })
            .map_err(|e| e.to_string())
        });
        BackupRunPage {
            dir,
            flow,
            prog,
            task,
            started: Instant::now(),
            active: Duration::ZERO,
            last_tick: Instant::now(),
            confirm_stop: false,
            finished: None,
            told: false,
            donate: crate::donate::Donate::for_this_build(),
        }
    }

    fn running_ui(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        if !self.flow.paused() {
            self.active += now - self.last_tick;
        }
        self.last_tick = now;
        let g = self.prog.lock().unwrap_or_else(|e| e.into_inner());
        let copied = self.flow.bytes().min(g.bytes);
        let ratio = if g.bytes > 0 {
            copied as f32 / g.bytes as f32
        } else {
            0.0
        };
        ui.label(RichText::new(format!("Into {}", self.dir.display())).weak());
        ui.add(
            egui::ProgressBar::new(ratio)
                .show_percentage()
                .animate(!self.flow.paused()),
        );
        ui.label(
            RichText::new(format!(
                "{} of {} · {} of {} files{}",
                gib(copied),
                gib(g.bytes),
                g.done,
                g.files,
                if g.already > 0 {
                    format!(" ({} copied before)", g.already)
                } else {
                    String::new()
                }
            ))
            .strong(),
        );
        let secs = self.active.as_secs_f64();
        let when = if self.flow.paused() {
            "Paused".to_string()
        } else if secs > 3.0 && copied > 0 {
            let rate = copied as f64 / secs;
            let left = Duration::from_secs_f64(g.bytes.saturating_sub(copied) as f64 / rate);
            format!("{} left · {}/s", duration_words(left), gib(rate as u64))
        } else {
            "Working out the time left…".into()
        };
        ui.label(when);
        if g.failed > 0 {
            ui.label(
                RichText::new(format!(
                    "{} file(s) couldn't be copied (listed in the report)",
                    g.failed
                ))
                .color(BAD),
            );
        }
        if !g.stage.is_empty() {
            ui.label(RichText::new(&g.stage).color(WARN));
        }
        ui.label(RichText::new(format!("Last: {}", g.current)).weak());
        drop(g);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = if self.flow.paused() {
                "Resume"
            } else {
                "Pause"
            };
            if ui.button(label).clicked() {
                self.flow.set_paused(!self.flow.paused());
            }
            if !self.flow.cancelled() && ui.button("Stop…").clicked() {
                self.confirm_stop = true;
            }
        });
        if self.confirm_stop {
            egui::Modal::new(egui::Id::unique("stop")).show(ui.ctx(), |ui| {
                ui.heading("Stop the backup?");
                ui.label("Files copied so far are kept. Choose this backup's target again to continue it.");
                ui.horizontal(|ui| {
                    if ui.button("Stop the backup").clicked() {
                        self.flow.stop();
                        self.confirm_stop = false;
                    }
                    if ui.button("Keep going").clicked() {
                        self.confirm_stop = false;
                    }
                });
            });
        }
        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }

    fn done_ui(&mut self, ui: &mut egui::Ui) -> Nav {
        let Some((result, elapsed, checklist)) = &mut self.finished else {
            return Nav::Stay;
        };
        if !self.told {
            // Completion notification: flashes the taskbar button.
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            self.told = true;
        }
        match result {
            Err(e) => {
                ui.label(
                    RichText::new(format!("The backup stopped: {e}"))
                        .color(BAD)
                        .strong(),
                );
                ui.label("Choose the same target again to continue where it stopped.");
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
                ui.label(RichText::new(msg).size(18.0).strong().color(color));
                ui.label(format!(
                    "{ok} files copied ({}), {failed} failed, in {}.",
                    gib(bytes),
                    duration_words(*elapsed).trim_start_matches("about ")
                ));
                if m.sensitive.is_some() {
                    ui.label(
                        "Keys and credentials are in sensitive.age; keep the passphrase safe.",
                    );
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Open the report").clicked() {
                open_path(&self.dir.join("report.html"));
            }
            if ui.button("Open the backup folder").clicked() {
                open_path(&self.dir);
            }
        });
        ui.add_space(6.0);
        checklist.ui(ui);
        let ok = result.is_ok();
        if primary(ui, "Done", true) {
            return Nav::Home;
        }
        if ok {
            self.donate.ui(ui);
        }
        Nav::Stay
    }
}

impl Page for BackupRunPage {
    fn title(&self) -> String {
        if self.finished.is_some() {
            "Backup done".into()
        } else {
            "Backing up".into()
        }
    }

    fn can_go_back(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx) -> Nav {
        if self.finished.is_none() {
            match self.task.take() {
                Some(r) => {
                    let steps = r
                        .as_ref()
                        .map(|m| m.manual_steps.clone())
                        .unwrap_or_default();
                    self.finished = Some((r, self.started.elapsed(), Checklist::new(steps)));
                }
                None if !self.task.pending() => {
                    self.finished = Some((
                        Err("the copy stopped unexpectedly".into()),
                        self.started.elapsed(),
                        Checklist::new(Vec::new()),
                    ));
                }
                None => {
                    self.running_ui(ui);
                    return Nav::Stay;
                }
            }
        }
        self.done_ui(ui)
    }
}
