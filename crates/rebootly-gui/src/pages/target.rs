//! Where the backup goes: a listed drive (into its `Rebootly` folder) or
//! any folder (NAS share, another directory).

use std::path::PathBuf;

use eframe::egui;
use rebootly_core::backup::profile::Selection;
use rebootly_host::Drive;
use rebootly_host::session::Shared;

use super::home::DRIVE_FOLDER;
use super::preflight::PreflightPage;
use crate::app::{Cx, Nav, Page};
use crate::widgets::{choice, folder_field, intro, primary};

pub struct TargetPage {
    ctx: Shared,
    sel: Selection,
    drives: Vec<Drive>,
    /// Index into `drives`; `drives.len()` = the folder field.
    pick: usize,
    other: String,
}

impl TargetPage {
    pub fn new(cx: &mut Cx, ctx: Shared, sel: Selection) -> Self {
        let drives = cx.platform.drives();
        TargetPage {
            ctx,
            sel,
            drives,
            pick: 0,
            other: String::new(),
        }
    }
}

impl Page for TargetPage {
    fn title(&self) -> String {
        format!("Back up {}: where to", self.ctx.label)
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Nav {
        intro(
            ui,
            &format!(
                "Where should the backup go? On a drive it goes into a {DRIVE_FOLDER} folder; each \
                 backup gets its own folder named after the PC and the time."
            ),
        );
        for (i, d) in self.drives.iter().enumerate() {
            if choice(ui, self.pick == i, &d.path.display().to_string(), &d.note) {
                self.pick = i;
            }
        }
        let n = self.drives.len();
        if choice(
            ui,
            self.pick == n,
            "Another folder",
            "A NAS share (\\\\server\\share) or any folder.",
        ) {
            self.pick = n;
        }
        if self.pick == n {
            folder_field(ui, &mut self.other, "e.g. \\\\nas\\backups");
        }
        let target = if self.pick < n {
            Some(self.drives[self.pick].path.join(DRIVE_FOLDER))
        } else {
            Some(PathBuf::from(self.other.trim())).filter(|p| !p.as_os_str().is_empty())
        };
        if primary(ui, "Next", target.is_some())
            && let Some(t) = target
        {
            return Nav::Push(Box::new(PreflightPage::new(
                cx,
                self.ctx.clone(),
                self.sel.clone(),
                t,
            )));
        }
        Nav::Stay
    }
}
