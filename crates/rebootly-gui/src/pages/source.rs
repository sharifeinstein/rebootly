//! What to back up: this PC, or a Windows installation on another disk.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use rebootly_core::backup::profile::JobProfile;
use rebootly_core::install::WindowsInstall;
use rebootly_host::session::BackupCtx;

use super::accounts::AccountsPage;
use super::preflight::PreflightPage;
use crate::app::{Cx, Nav, Page};
use crate::task::Task;
use crate::widgets::{busy, choice, error, folder_field, intro, primary};

#[derive(Clone, PartialEq)]
enum Pick {
    Live,
    Install(PathBuf),
    Other,
}

pub struct SourcePage {
    live: bool,
    job: Option<(JobProfile, PathBuf)>,
    installs: Task<Vec<PathBuf>>,
    pick: Pick,
    other: String,
    opening: Option<Task<anyhow::Result<BackupCtx>>>,
    error: Option<String>,
}

impl SourcePage {
    pub fn new(cx: &mut Cx, live: bool, job: Option<(JobProfile, PathBuf)>) -> Self {
        let roots = cx.platform.install_roots();
        let installs = Task::spawn(&cx.ctx, move || {
            WindowsInstall::scan(&roots)
                .into_iter()
                .map(|i| i.volume_root)
                .collect()
        });
        SourcePage {
            live,
            job,
            installs,
            pick: if live { Pick::Live } else { Pick::Other },
            other: String::new(),
            opening: None,
            error: None,
        }
    }

    fn open(&mut self, cx: &Cx) {
        let p = cx.platform.clone();
        let pick = self.pick.clone();
        let other = PathBuf::from(self.other.trim());
        self.error = None;
        self.opening = Some(Task::spawn(&cx.ctx, move || {
            let (src, label) = match pick {
                Pick::Live => (p.open_live()?, "This PC".to_string()),
                Pick::Install(r) => (p.open_offline(&r)?, r.display().to_string()),
                Pick::Other => (p.open_offline(&other)?, other.display().to_string()),
            };
            BackupCtx::new(src, label, p.packs())
        }));
    }
}

impl Page for SourcePage {
    fn title(&self) -> String {
        match &self.job {
            Some((j, _)) => format!("Saved job “{}”: what to back up", j.name),
            None => "Back up: what to back up".into(),
        }
    }

    fn can_go_back(&self) -> bool {
        self.opening.is_none()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        if self.live {
            intro(ui, "Pick the Windows to back up. Usually that's this PC.");
            if choice(
                ui,
                self.pick == Pick::Live,
                "This PC",
                "The Windows that is running now.",
            ) {
                self.pick = Pick::Live;
            }
        } else {
            intro(
                ui,
                "Pick the Windows installation to copy files from (it isn't started).",
            );
        }
        if self.installs.pending() {
            busy(ui, "Looking for Windows on other disks…");
        } else if let Some(list) = self.installs.get() {
            for r in list.clone() {
                let title = format!("Windows on {}", r.display());
                if choice(
                    ui,
                    self.pick == Pick::Install(r.clone()),
                    &title,
                    "Another disk, read without starting it.",
                ) {
                    self.pick = Pick::Install(r);
                }
            }
        }
        if choice(
            ui,
            self.pick == Pick::Other,
            "Another folder",
            "A mounted Windows volume (the folder that contains Windows).",
        ) {
            self.pick = Pick::Other;
        }
        if self.pick == Pick::Other {
            folder_field(ui, &mut self.other, "e.g. E:\\");
        }

        if let Some(t) = &mut self.opening {
            match t.take() {
                None => busy(ui, "Reading the accounts and folders…"),
                Some(Ok(ctx)) => {
                    self.opening = None;
                    let ctx = Arc::new(ctx);
                    return match self.job.clone() {
                        Some((job, target)) => {
                            Nav::Push(Box::new(PreflightPage::new(cx, ctx, job.selection, target)))
                        }
                        None => Nav::Push(Box::new(AccountsPage::new(ctx))),
                    };
                }
                Some(Err(e)) => {
                    self.opening = None;
                    self.error = Some(format!("{e:#}"));
                }
            }
        }
        if let Some(e) = &self.error {
            error(ui, e);
        }
        let ready =
            self.opening.is_none() && (self.pick != Pick::Other || !self.other.trim().is_empty());
        if primary(ui, "Next", ready) {
            self.open(cx);
        }
        Nav::Stay
    }
}
