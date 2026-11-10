//! Start screen (live Windows) and the saved-jobs list. In WinPE/Linux the
//! app starts straight in the Rescue flow instead.

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use rebootly_core::backup::profile::{self, JobProfile};

use crate::app::{Ctx, Nav, Screen};
use crate::backup::source::SourceScreen;
use crate::backup::target::DRIVE_FOLDER;
use crate::restore::pick::PickScreen;
use crate::ui::dim;
use crate::widgets::Menu;

#[derive(Clone, Copy)]
enum Item {
    Backup,
    Restore,
    Jobs,
    Quit,
}

/// Saved job profiles on the listed drives: (profile, target folder).
fn saved_jobs(cx: &Ctx) -> Vec<(JobProfile, PathBuf)> {
    let mut out = Vec::new();
    for d in cx.platform.drives() {
        for t in [d.path.join(DRIVE_FOLDER), d.path.clone()] {
            out.extend(profile::list(&t).into_iter().map(|(_, p)| (p, t.clone())));
        }
    }
    out
}

pub struct HomeScreen {
    items: Vec<Item>,
    menu: Menu,
    version: String,
}

impl HomeScreen {
    pub fn new(cx: &mut Ctx) -> Self {
        let jobs = saved_jobs(cx).len();
        let mut items = vec![Item::Backup, Item::Restore];
        if jobs > 0 {
            items.push(Item::Jobs);
        }
        items.push(Item::Quit);
        let menu = Menu::new(
            items
                .iter()
                .map(|i| match i {
                    Item::Backup => (
                        "Back up".into(),
                        "this PC or another Windows disk, to a USB drive or NAS".into(),
                    ),
                    Item::Restore => (
                        "Restore".into(),
                        "a backup onto this PC (finds backups on your drives)".into(),
                    ),
                    Item::Jobs => (
                        "Run a saved job".into(),
                        format!("{jobs} job profile(s) on your drives"),
                    ),
                    Item::Quit => ("Quit".into(), String::new()),
                })
                .collect(),
        );
        HomeScreen {
            items,
            menu,
            version: cx.platform.version(),
        }
    }
}

impl Screen for HomeScreen {
    fn title(&self) -> String {
        "Move to a new PC, or keep a copy".into()
    }

    fn hints(&self) -> String {
        "↑↓ move · Enter choose · q quit".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [body, foot] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(area);
        self.menu.draw(f, body);
        f.render_widget(
            Paragraph::new(Line::styled(format!("Rebootly {}", self.version), dim())),
            foot,
        );
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
            return Nav::Quit;
        }
        match self.menu.key(&k).map(|i| self.items[i]) {
            Some(Item::Backup) => Nav::Push(Box::new(SourceScreen::new(cx, true, None))),
            Some(Item::Restore) => Nav::Push(Box::new(PickScreen::new(cx))),
            Some(Item::Jobs) => Nav::Push(Box::new(JobsScreen::new(cx))),
            Some(Item::Quit) => Nav::Quit,
            None => Nav::Ignored,
        }
    }
}

pub struct JobsScreen {
    jobs: Vec<(JobProfile, PathBuf)>,
    menu: Menu,
}

impl JobsScreen {
    pub fn new(cx: &mut Ctx) -> Self {
        let jobs = saved_jobs(cx);
        let menu = Menu::new(
            jobs.iter()
                .map(|(p, t)| {
                    (
                        p.name.clone(),
                        format!("{} → {}", p.selection.mode.label(), t.display()),
                    )
                })
                .collect(),
        );
        JobsScreen { jobs, menu }
    }
}

impl Screen for JobsScreen {
    fn title(&self) -> String {
        "Run a saved job".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        self.menu.draw(f, area);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        match self.menu.key(&k) {
            Some(i) => Nav::Push(Box::new(SourceScreen::new(
                cx,
                true,
                Some(self.jobs[i].clone()),
            ))),
            None => Nav::Ignored,
        }
    }
}
