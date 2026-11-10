//! Restore: what to restore (the CLI's --apps, --no-packs, --tasks, … as
//! ticks), the passphrase, and planning against this PC.

use std::sync::Arc;

use eframe::egui::{self, RichText};
use rebootly_core::backup::sensitive::Passphrase;
use rebootly_core::restore::{self, AppsMode, Backup, Bundle, RestoreOptions};

use super::restore_plan::{PlanPage, Planned};
use crate::app::{Cx, Nav, Page};
use crate::task::Task;
use crate::widgets::{busy, error, intro, primary};

pub struct OptionsPage {
    backup: Backup,
    user: String,
    apps: bool,
    packs: bool,
    registry: bool,
    system: bool,
    tasks: bool,
    explorer: bool,
    sensitive: bool,
    pass: String,
    planning: Option<Task<Result<Planned, String>>>,
    error: Option<String>,
}

impl OptionsPage {
    pub fn new(backup: Backup, user: String) -> Self {
        let sensitive = backup.bundle_path().is_some();
        OptionsPage {
            backup,
            user,
            apps: true,
            packs: true,
            registry: true,
            system: true,
            tasks: false,
            explorer: true,
            sensitive,
            pass: String::new(),
            planning: None,
            error: None,
        }
    }

    fn options(&self) -> RestoreOptions {
        RestoreOptions {
            user: Some(self.user.clone()),
            apps: if self.apps {
                AppsMode::Install
            } else {
                AppsMode::Skip
            },
            packs: self.packs,
            registry: self.registry,
            system: self.system,
            tasks: self.tasks,
            restart_explorer: self.explorer,
            ..RestoreOptions::default()
        }
    }

    fn plan(&mut self, cx: &Cx) {
        let (b, opts, p) = (self.backup.clone(), self.options(), cx.platform.clone());
        let pass = (self.sensitive && b.bundle_path().is_some()).then(|| self.pass.clone());
        self.error = None;
        self.planning = Some(Task::spawn(&cx.ctx, move || {
            let bundle = match (pass, b.bundle_path()) {
                (Some(pw), Some(path)) => {
                    let pw = Passphrase::new(pw)?;
                    Some(Arc::new(
                        Bundle::open(&path, &pw).map_err(|e| e.to_string())?,
                    ))
                }
                _ => None,
            };
            let user = opts.user.clone().unwrap_or_default();
            let (target, _) = p
                .restore_target(true, &user)
                .map_err(|e| format!("{e:#}"))?;
            let plan = restore::plan(&b, &target, &p.packs(), &opts, bundle.as_deref())
                .map_err(|e| e.to_string())?;
            Ok(Planned {
                backup: b,
                opts,
                bundle,
                plan,
            })
        }));
    }
}

impl Page for OptionsPage {
    fn title(&self) -> String {
        format!(
            "Restore {} from {}",
            self.user,
            self.backup
                .manifest
                .source
                .machine
                .as_deref()
                .unwrap_or("?")
        )
    }

    fn can_go_back(&self) -> bool {
        self.planning.is_none()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        intro(ui, "What should be restored?");
        let row = |ui: &mut egui::Ui, v: &mut bool, t: &str, n: &str| {
            ui.horizontal(|ui| {
                ui.checkbox(v, t);
                ui.label(RichText::new(n).weak());
            });
        };
        row(
            ui,
            &mut self.apps,
            "Reinstall apps (winget)",
            "first, so their settings land on top",
        );
        row(
            ui,
            &mut self.packs,
            "App settings",
            "browsers, VS Code, terminal, …",
        );
        row(
            ui,
            &mut self.registry,
            "Windows settings",
            "Explorer, taskbar, mouse, keyboard, …",
        );
        row(
            ui,
            &mut self.system,
            "System items",
            "hosts, Wi-Fi, power plan",
        );
        row(
            ui,
            &mut self.tasks,
            "Scheduled tasks",
            "only if you need them on this PC",
        );
        row(
            ui,
            &mut self.explorer,
            "Restart Explorer at the end",
            "so the settings show at once",
        );
        if self.backup.bundle_path().is_some() {
            row(
                ui,
                &mut self.sensitive,
                "Keys and credentials",
                "from sensitive.age; needs the backup's passphrase",
            );
            if self.sensitive {
                ui.add(
                    egui::TextEdit::singleline(&mut self.pass)
                        .password(true)
                        .hint_text("passphrase of the backup"),
                );
            }
        }
        if let Some(t) = &mut self.planning {
            match t.take() {
                None => busy(ui, "Comparing the backup with this PC…"),
                Some(Ok(p)) => {
                    self.planning = None;
                    return Nav::Push(Box::new(PlanPage::new(p)));
                }
                Some(Err(e)) => {
                    self.planning = None;
                    self.error = Some(e);
                }
            }
        }
        if let Some(e) = &self.error {
            error(ui, e);
        }
        if primary(ui, "Preview", self.planning.is_none()) {
            self.plan(cx);
        }
        Nav::Stay
    }
}
