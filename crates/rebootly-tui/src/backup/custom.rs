//! Custom mode: a tree with checkboxes and sizes (SPEC §11). Sizes come
//! from a Full plan of the chosen accounts.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Paragraph;
use rebootly_core::backup::sizes::sizes;
use rebootly_core::backup::preflight::gib;
use rebootly_core::backup::profile::{Mode, Selection};

use super::Shared;
use super::target::TargetScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::widgets::{Check, Checks};

fn note(s: Option<&(u64, u64)>) -> String {
    match s {
        Some((n, b)) => format!("{:>10}  {n} files", gib(*b)),
        None => format!("{:>10}", "empty"),
    }
}

pub struct CustomScreen {
    ctx: Shared,
    users: Vec<String>,
    checks: Checks,
}

impl CustomScreen {
    pub fn new(cx: &mut Ctx, ctx: Shared, sel: Selection) -> anyhow::Result<Self> {
        cx.busy("Custom", "Scanning folders to show their sizes…");
        let mut full = Selection::new(Mode::Full);
        full.users = sel.users.clone();
        let plan = ctx.plan(&full)?;
        let sz = sizes(&plan);
        let mut rows = vec![Check::heading("Folders")];
        for kf in Selection::tier1() {
            let id = format!("folder:{kf}");
            let s = sz.get(&id);
            rows.push(Check::item(&id, kf, note(s), s.is_some()));
        }
        rows.push(Check::heading("App settings"));
        let mut seen = std::collections::BTreeSet::new();
        for p in &plan.packs {
            if seen.insert(p.id.clone()) {
                let id = format!("pack:{}", p.id);
                rows.push(Check::item(&id, &p.name, note(sz.get(&id)), true));
            }
        }
        rows.push(Check::heading("Extras"));
        rows.push(Check::item(
            "system",
            "System items (app list, hosts, Wi-Fi, printers…)",
            note(sz.get("system")),
            true,
        ));
        let has = |id: &str| sz.contains_key(id);
        if has("public") {
            rows.push(Check::item(
                "public",
                "Public folder (shared by all users)",
                note(sz.get("public")),
                false,
            ));
        }
        for r in &ctx.offered_roots {
            let id = format!("root:{r}");
            rows.push(Check::item(
                &id,
                &format!("Drive root: {r}"),
                note(sz.get(&id)),
                false,
            ));
        }
        if has("sensitive") {
            rows.push(Check::item(
                "sensitive",
                "Keys and credentials (encrypted, needs a passphrase)",
                note(sz.get("sensitive")),
                false,
            ));
        }
        Ok(CustomScreen {
            ctx,
            users: sel.users,
            checks: Checks::new(rows),
        })
    }

    pub fn selection(&self) -> Selection {
        let mut s = Selection::new(Mode::Custom);
        s.users = self.users.clone();
        let all = |prefix: &str| {
            self.checks
                .rows
                .iter()
                .filter(|r| r.id.starts_with(prefix))
                .count()
        };
        let folders = self.checks.ticked("folder:");
        // Nothing ticked must not mean "all": use a name that matches nothing.
        s.folders = match folders.len() {
            0 => vec!["-".into()],
            n if n == all("folder:") => Vec::new(),
            _ => folders,
        };
        let packs = self.checks.ticked("pack:");
        s.packs = (packs.len() != all("pack:")).then_some(packs);
        s.public = self.checks.on("public");
        s.root_folders = self.checks.ticked("root:");
        s.system = self.checks.on("system");
        s.sensitive = self.checks.on("sensitive");
        s
    }
}

impl Screen for CustomScreen {
    fn title(&self) -> String {
        format!("Back up {}: choose", self.ctx.label)
    }

    fn hints(&self) -> String {
        "↑↓ move · Space tick · a all · n none · Enter next · Esc back".into()
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [intro, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).areas(area);
        f.render_widget(
            Paragraph::new("Tick what to back up. Registry hives are always included (small)."),
            intro,
        );
        self.checks.draw(f, body);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if self.checks.key(&k) {
            return Nav::Stay;
        }
        if k.code != KeyCode::Enter {
            return Nav::Ignored;
        }
        Nav::Push(Box::new(TargetScreen::new(
            cx,
            self.ctx.clone(),
            self.selection(),
        )))
    }
}
