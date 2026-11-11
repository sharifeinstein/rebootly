//! What to back up: this PC, or a Windows installation on another disk.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, RichText};
use rebootly_core::backup::profile::JobProfile;
use rebootly_core::install::WindowsInstall;
use rebootly_core::volume::{RecoveryKey, Volume, key_status, tidy_key};
use rebootly_host::session::BackupCtx;

use super::accounts::AccountsPage;
use super::preflight::PreflightPage;
use crate::app::{Cx, Nav, Page};
use crate::task::Task;
use crate::widgets::{busy, choice, error, folder_field, intro, primary, warning};

#[derive(Clone, PartialEq)]
enum Pick {
    Live,
    Install(PathBuf),
    /// A BitLocker-locked drive (device), unlocked with its recovery key.
    Locked(String),
    Other,
}

/// Installations found, and locked drives that may hold more.
struct Scan {
    installs: Vec<PathBuf>,
    locked: Vec<Volume>,
}

type Unlocked = anyhow::Result<(PathBuf, Vec<String>)>;

fn scan(cx: &Cx) -> Task<Scan> {
    let p = cx.platform.clone();
    Task::spawn(&cx.ctx, move || Scan {
        installs: WindowsInstall::scan(&p.install_roots())
            .into_iter()
            .map(|i| i.volume_root)
            .collect(),
        locked: p.volumes().into_iter().filter(Volume::is_locked).collect(),
    })
}

pub struct SourcePage {
    live: bool,
    job: Option<(JobProfile, PathBuf)>,
    installs: Task<Scan>,
    pick: Pick,
    other: String,
    opening: Option<Task<anyhow::Result<BackupCtx>>>,
    error: Option<String>,
    /// Recovery key being typed for `Pick::Locked`.
    key: String,
    unlocking: Option<Task<Unlocked>>,
    /// Notes from unlocking (e.g. the Windows was hibernated).
    notes: Vec<String>,
}

impl SourcePage {
    pub fn new(cx: &mut Cx, live: bool, job: Option<(JobProfile, PathBuf)>) -> Self {
        SourcePage {
            live,
            job,
            installs: scan(cx),
            pick: if live { Pick::Live } else { Pick::Other },
            other: String::new(),
            opening: None,
            error: None,
            key: String::new(),
            unlocking: None,
            notes: Vec::new(),
        }
    }

    fn unlock(&mut self, cx: &Cx, device: String) {
        self.error = None;
        let key = match RecoveryKey::parse(&self.key) {
            Ok(k) => k,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        let p = cx.platform.clone();
        self.unlocking = Some(Task::spawn(&cx.ctx, move || {
            p.open_volume(&device, Some(&key))
        }));
    }

    /// The locked drive's key field and progress.
    fn locked_ui(&mut self, ui: &mut egui::Ui, cx: &Cx) {
        if let Some(t) = &mut self.unlocking {
            match t.take() {
                None => busy(ui, "Unlocking the disk (this can take a minute)…"),
                Some(Ok((root, notes))) => {
                    self.unlocking = None;
                    self.key.clear();
                    self.notes.extend(notes);
                    // A data drive: its letter now maps for the backup.
                    self.pick = if WindowsInstall::from_path(&root).is_some() {
                        Pick::Install(root)
                    } else if self.live {
                        Pick::Live
                    } else {
                        Pick::Other
                    };
                    self.installs = scan(cx);
                }
                Some(Err(e)) => {
                    self.unlocking = None;
                    self.error = Some(format!("{e:#}"));
                }
            }
            return;
        }
        ui.label(
            RichText::new(
                "Recovery key (48 digits): for a Microsoft account at aka.ms/myrecoverykey, or \
                 on the printout/USB saved when BitLocker was turned on.",
            )
            .weak(),
        );
        ui.horizontal(|ui| {
            let l = ui.label("Recovery key");
            // Shown as typed, in groups of 6 (dashes are added), so it can
            // be checked against the printout. A pasted page with the key
            // ID and date around the key: the key is picked out.
            let out = egui::TextEdit::singleline(&mut self.key)
                .font(egui::TextStyle::Monospace)
                .desired_width(460.0)
                .show(ui);
            let resp = out.response.response.clone().labelled_by(l.id);
            if resp.changed() {
                let g = tidy_key(&self.key);
                if g != self.key {
                    self.key = g;
                    let mut st = out.state;
                    let end = egui::text::CCursor::new(self.key.chars().count());
                    st.cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(end)));
                    st.store(ui.ctx(), resp.id);
                }
            }
        });
        ui.label(RichText::new(key_status(&self.key)).weak());
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
                Pick::Locked(d) => anyhow::bail!("{d} is still locked"),
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
        } else if let Some(found) = self.installs.get() {
            let (list, locked) = (found.installs.clone(), found.locked.clone());
            for r in list {
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
            for v in locked {
                let title = format!("Unlock {}", v.describe());
                let pick = Pick::Locked(v.device.clone());
                if choice(
                    ui,
                    self.pick == pick,
                    &title,
                    "Locked with BitLocker: unlock it with its recovery key.",
                ) {
                    self.pick = pick;
                    self.error = None;
                }
                if self.pick == Pick::Locked(v.device) {
                    self.locked_ui(ui, cx);
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
        for n in &self.notes {
            warning(ui, n);
        }
        if let Pick::Locked(dev) = self.pick.clone() {
            let ready = self.unlocking.is_none() && !self.key.trim().is_empty();
            if primary(ui, "Unlock", ready) {
                self.unlock(cx, dev);
            }
            return Nav::Stay;
        }
        let ready =
            self.opening.is_none() && (self.pick != Pick::Other || !self.other.trim().is_empty());
        if primary(ui, "Next", ready) {
            self.open(cx);
        }
        Nav::Stay
    }
}
