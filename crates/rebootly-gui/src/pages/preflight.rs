//! Plain-language pre-flight summary, live-mode reminders, saving a job
//! profile, the passphrase, and the start of the copy.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use rebootly_core::backup::Job;
use rebootly_core::backup::preflight::gib;
use rebootly_core::backup::profile::{self, JobProfile, PROFILE_SCHEMA, Selection};
use rebootly_core::backup::sensitive::Passphrase;
use rebootly_host::session::{Prepared, Shared, prepare};

use super::backup_run::BackupRunPage;
use crate::app::{Cx, GOOD, Nav, Page};
use crate::task::Task;
use crate::widgets::{busy, error, primary, warning};

pub struct PreflightPage {
    ctx: Shared,
    sel: Selection,
    target: PathBuf,
    ready: Task<Result<Prepared, String>>,
    force: bool,
    profile_name: String,
    pass: String,
    pass2: String,
    message: Option<(String, bool)>,
}

impl PreflightPage {
    pub fn new(cx: &mut Cx, ctx: Shared, sel: Selection, target: PathBuf) -> Self {
        let (c, s, t, p) = (
            ctx.clone(),
            sel.clone(),
            target.clone(),
            cx.platform.clone(),
        );
        let ready = Task::spawn(&cx.ctx, move || {
            prepare(&c, &s, &t, p.as_ref()).map_err(|e| format!("{e:#}"))
        });
        PreflightPage {
            ctx,
            sel,
            target,
            ready,
            force: false,
            profile_name: String::new(),
            pass: String::new(),
            pass2: String::new(),
            message: None,
        }
    }

    fn start(&mut self, cx: &Cx, resume: Option<PathBuf>) -> Nav {
        let Some(Ok(p)) = self.ready.get() else {
            return Nav::Stay;
        };
        let needs_pass = self.sel.sensitive && !p.plan.sensitive.is_empty();
        let pass = if needs_pass {
            match (self.pass == self.pass2).then(|| Passphrase::new(self.pass.clone())) {
                None => {
                    self.message = Some(("The two passphrases differ.".into(), true));
                    return Nav::Stay;
                }
                Some(Err(e)) => {
                    let n = self.pass.chars().count();
                    self.message = Some((format!("{e} (got {n})."), true));
                    return Nav::Stay;
                }
                Some(Ok(p)) => Some(p),
            }
        } else {
            None
        };
        let job = match &resume {
            Some(dir) => Job::resume(dir),
            None => Job::create(&self.target, p.plan.source.machine.as_deref()),
        };
        let job = match job {
            Ok(j) => j,
            Err(e) => {
                self.message = Some((format!("Can't create the backup folder: {e}"), true));
                return Nav::Stay;
            }
        };
        let Some(Ok(p)) = self.ready.take() else {
            return Nav::Stay;
        };
        Nav::Replace(Box::new(BackupRunPage::new(
            cx,
            self.ctx.clone(),
            p.plan,
            job,
            pass,
        )))
    }

    fn save_profile(&mut self) {
        let p = JobProfile {
            schema: PROFILE_SCHEMA,
            name: self.profile_name.trim().to_string(),
            selection: self.sel.clone(),
            jobs: None,
        };
        self.message = Some(match profile::save(&self.target, &p) {
            Ok(path) => (format!("Saved job profile: {}", path.display()), false),
            Err(e) => (format!("Couldn't save the profile: {e}"), true),
        });
    }
}

impl Page for PreflightPage {
    fn title(&self) -> String {
        format!("Back up {}: ready?", self.ctx.label)
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        let live = self.ctx.live;
        let p = match self.ready.get() {
            None => {
                busy(ui, "Counting files and testing the target's speed…");
                return Nav::Stay;
            }
            Some(Err(e)) => {
                error(ui, &format!("Can't plan this backup: {e}"));
                return Nav::Stay;
            }
            Some(Ok(p)) => p,
        };
        ui.label(RichText::new(&p.summary.headline).size(18.0).strong());
        ui.label(RichText::new(format!("Into {}", self.target.display())).weak());
        ui.add_space(4.0);
        egui::CollapsingHeader::new("What's in it")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("sizes")
                    .num_columns(3)
                    .spacing([24.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for f in &p.pre.folders {
                            ui.label(&f.folder);
                            ui.label(if f.files == 1 {
                                "1 file".to_string()
                            } else {
                                format!("{} files", f.files)
                            });
                            ui.label(gib(f.bytes));
                            ui.end_row();
                        }
                    });
            });
        let sens = p.plan.sensitive_count();
        let needs_pass = self.sel.sensitive && sens > 0;
        if sens > 0 && !self.sel.sensitive {
            ui.label(format!(
                "{} key/credential location(s) are left out (choose Full, or tick them in Custom).",
                p.plan.sensitive_jobs.len()
            ));
        }
        for w in &p.pre.warnings {
            warning(ui, &format!("! {w}"));
        }
        for b in &p.pre.blockers {
            error(ui, &format!("× {b}"));
        }
        let source_blocker = p.pre.blockers.iter().any(|b| b.contains("source disk"));
        let blocked = !p.pre.blockers.is_empty();
        let resumable = p.resumable.clone();
        if live {
            ui.add_space(4.0);
            ui.label(RichText::new("Before you start").strong());
            ui.label("• If you won't use this PC again, deactivate licenses now (Adobe, Office, games, …).");
            ui.label("• Save your BitLocker recovery key (aka.ms/myrecoverykey) if the disk is encrypted.");
        }
        if needs_pass {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("Passphrase for {sens} key/credential file(s)")).strong(),
            );
            ui.label(RichText::new("At least 8 characters. It is not stored anywhere: without it they can't be opened.").weak());
            ui.add(
                egui::TextEdit::singleline(&mut self.pass)
                    .password(true)
                    .hint_text("passphrase"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.pass2)
                    .password(true)
                    .hint_text("type it again"),
            );
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Save as a job profile").show(ui, |ui| {
            ui.label(
                RichText::new(
                    "Keeps these choices beside the backups, to run again on this or other PCs.",
                )
                .weak(),
            );
            ui.horizontal(|ui| {
                let l = ui.label("Profile name");
                ui.add(egui::TextEdit::singleline(&mut self.profile_name))
                    .labelled_by(l.id);
                if ui
                    .add_enabled(
                        !self.profile_name.trim().is_empty(),
                        egui::Button::new("Save profile"),
                    )
                    .clicked()
                {
                    self.save_profile();
                }
            });
        });
        if blocked && !source_blocker {
            ui.checkbox(&mut self.force, "Start anyway, despite the problems above");
        }
        if let Some((m, bad)) = &self.message {
            if *bad {
                error(ui, m)
            } else {
                ui.label(RichText::new(m).color(GOOD));
            }
        }
        let can_start = !blocked || (self.force && !source_blocker);
        let mut nav = Nav::Stay;
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if primary(ui, "Start backup", can_start) {
                nav = self.start(cx, None);
            }
            if let Some(dir) = resumable {
                ui.add_space(8.0);
                if ui
                    .button("Continue the unfinished backup")
                    .on_hover_text(dir.display().to_string())
                    .clicked()
                {
                    nav = self.start(cx, Some(dir));
                }
            }
        });
        nav
    }
}
