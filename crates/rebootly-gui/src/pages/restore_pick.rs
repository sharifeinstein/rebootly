//! Restore: pick a backup (found on the drives, newest first), the backup
//! user, and what to restore.

use eframe::egui;
use rebootly_core::backup::preflight::gib;
use rebootly_core::restore::Backup;
use rebootly_core::restore::find::{FoundBackup, find_backups};
use std::path::PathBuf;

use super::home::DRIVE_FOLDER;
use super::restore_options::OptionsPage;
use crate::app::{Cx, Nav, Page};
use crate::task::Task;
use crate::widgets::{busy, choice, error, folder_field, intro, primary};

fn when(s: &str) -> String {
    s.get(..16).unwrap_or(s).replace('T', " ")
}

pub struct RestorePickPage {
    found: Task<Vec<FoundBackup>>,
    pick: usize,
    other: String,
    error: Option<String>,
}

impl RestorePickPage {
    pub fn new(cx: &mut Cx) -> Self {
        let roots: Vec<PathBuf> = cx
            .platform
            .drives()
            .into_iter()
            .flat_map(|d| [d.path.join(DRIVE_FOLDER), d.path])
            .collect();
        RestorePickPage {
            found: Task::spawn(&cx.ctx, move || find_backups(&roots)),
            pick: 0,
            other: String::new(),
            error: None,
        }
    }
}

/// The next page for an opened backup.
fn after_open(b: Backup) -> Result<Nav, String> {
    match b.manifest.users.len() {
        0 => Err("That backup has no user accounts in it.".into()),
        1 => {
            let u = b.manifest.users[0].username.clone();
            Ok(Nav::Push(Box::new(OptionsPage::new(b, u))))
        }
        _ => Ok(Nav::Push(Box::new(UserPage::new(b)))),
    }
}

impl Page for RestorePickPage {
    fn title(&self) -> String {
        "Restore: choose a backup".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx) -> Nav {
        let Some(found) = self.found.get() else {
            busy(ui, "Looking for backups on your drives…");
            return Nav::Stay;
        };
        if found.is_empty() {
            intro(
                ui,
                "No backups found on your drives. Plug in the drive, or choose the folder.",
            );
        } else {
            intro(
                ui,
                "The newest backup is at the top. Restore puts it into the account you're signed in with now.",
            );
        }
        for (i, b) in found.iter().enumerate() {
            let state = match &b.finished {
                Some(f) if b.complete => format!("finished {}", when(f)),
                _ => "not finished".into(),
            };
            let title = format!("{} · {state}", b.machine);
            let note = format!(
                "{} · {} · {}",
                b.users.join(", "),
                gib(b.bytes),
                b.dir.display()
            );
            if choice(ui, self.pick == i, &title, &note) {
                self.pick = i;
            }
        }
        let n = found.len();
        if choice(
            ui,
            self.pick == n,
            "Another folder",
            "A backup folder (it contains manifest.json).",
        ) {
            self.pick = n;
        }
        if self.pick == n {
            folder_field(
                ui,
                &mut self.other,
                "e.g. E:\\Rebootly\\OLD-PC_20261110-131615",
            );
        }
        let dir = if self.pick < n {
            Some(found[self.pick].dir.clone())
        } else {
            Some(PathBuf::from(self.other.trim())).filter(|p| !p.as_os_str().is_empty())
        };
        if let Some(e) = &self.error {
            error(ui, e);
        }
        if primary(ui, "Next", dir.is_some())
            && let Some(d) = dir
        {
            match Backup::open(&d)
                .map_err(|e| e.to_string())
                .and_then(after_open)
            {
                Ok(nav) => return nav,
                Err(e) => self.error = Some(e),
            }
        }
        Nav::Stay
    }
}

pub struct UserPage {
    backup: Backup,
    order: Vec<usize>,
    pick: usize,
}

impl UserPage {
    pub fn new(backup: Backup) -> Self {
        let users = &backup.manifest.users;
        let mut order: Vec<usize> = (0..users.len()).collect();
        order.sort_by_key(|&i| users[i].username.to_lowercase());
        UserPage {
            backup,
            order,
            pick: 0,
        }
    }
}

impl Page for UserPage {
    fn title(&self) -> String {
        "Restore: whose files".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx) -> Nav {
        intro(
            ui,
            "Which old account goes into the account you're signed in with? For the others, sign in as each of them and run Restore again.",
        );
        for (k, &i) in self.order.iter().enumerate() {
            let u = &self.backup.manifest.users[i];
            if choice(
                ui,
                self.pick == k,
                &u.username,
                &format!("{} folder(s)", u.folders.len()),
            ) {
                self.pick = k;
            }
        }
        if primary(ui, "Next", true) {
            let u = self.backup.manifest.users[self.order[self.pick]]
                .username
                .clone();
            return Nav::Push(Box::new(OptionsPage::new(self.backup.clone(), u)));
        }
        Nav::Stay
    }
}
