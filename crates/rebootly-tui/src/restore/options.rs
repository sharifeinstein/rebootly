//! What to restore (the CLI's --apps, --no-packs, --tasks, … as ticks) and
//! the passphrase for the sensitive bundle.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use rebootly_core::backup::sensitive::Passphrase;
use rebootly_core::restore::{AppsMode, Bundle, RestoreOptions};

use super::Backup;
use super::plan::PlanScreen;
use crate::app::{Ctx, Nav, Screen};
use crate::ui::{BAD, bold};
use crate::widgets::{Check, Checks, TextField};

pub struct OptionsScreen {
    backup: Backup,
    user: String,
    checks: Checks,
    pass: Option<TextField>,
    error: Option<String>,
}

impl OptionsScreen {
    pub fn new(backup: Backup, user: String) -> Self {
        let mut rows = vec![
            Check::heading("Restore"),
            Check::item(
                "apps",
                "Reinstall apps (winget)",
                "first, so their settings land on top".into(),
                true,
            ),
            Check::item(
                "packs",
                "App settings",
                "browsers, VS Code, terminal, …".into(),
                true,
            ),
            Check::item(
                "registry",
                "Windows settings",
                "Explorer, taskbar, mouse, keyboard, …".into(),
                true,
            ),
            Check::item(
                "system",
                "System items",
                "hosts, Wi-Fi, printers list, power plan".into(),
                true,
            ),
            Check::item(
                "tasks",
                "Scheduled tasks",
                "only if you need them on this PC".into(),
                false,
            ),
            Check::item(
                "explorer",
                "Restart Explorer at the end",
                "so the settings show at once".into(),
                true,
            ),
        ];
        if backup.bundle_path().is_some() {
            rows.push(Check::item(
                "sensitive",
                "Keys and credentials",
                "from sensitive.age; needs the backup's passphrase".into(),
                true,
            ));
        }
        OptionsScreen {
            backup,
            user,
            checks: Checks::new(rows),
            pass: None,
            error: None,
        }
    }

    fn options(&self) -> RestoreOptions {
        let on = |id| self.checks.on(id);
        RestoreOptions {
            user: Some(self.user.clone()),
            apps: if on("apps") {
                AppsMode::Install
            } else {
                AppsMode::Skip
            },
            packs: on("packs"),
            registry: on("registry"),
            system: on("system"),
            tasks: on("tasks"),
            restart_explorer: on("explorer"),
            ..RestoreOptions::default()
        }
    }

    fn next(&mut self, cx: &mut Ctx, bundle: Option<Bundle>) -> Nav {
        match PlanScreen::new(cx, self.backup.clone(), self.options(), bundle) {
            Ok(s) => Nav::Push(Box::new(s)),
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                Nav::Stay
            }
        }
    }
}

impl Screen for OptionsScreen {
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

    fn hints(&self) -> String {
        if self.pass.is_some() {
            "type the passphrase · Enter OK · Esc cancel".into()
        } else {
            "↑↓ move · Space tick · Enter preview · Esc back".into()
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let [body, foot] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);
        self.checks.draw(f, body);
        let mut l = Vec::new();
        if let Some(t) = &self.pass {
            l.push(Line::styled(
                "Passphrase of the backup's keys and credentials:",
                bold(),
            ));
            l.push(t.line(true));
        }
        if let Some(e) = &self.error {
            l.push(Line::styled(e.clone(), Style::default().fg(BAD)));
        }
        f.render_widget(Paragraph::new(l), foot);
    }

    fn key(&mut self, k: KeyEvent, cx: &mut Ctx) -> Nav {
        if let Some(t) = &mut self.pass {
            match k.code {
                KeyCode::Esc => self.pass = None,
                KeyCode::Enter => {
                    let pass = Passphrase::new(t.value.clone());
                    let path = self.backup.bundle_path();
                    let bundle = match (pass, path) {
                        (Ok(p), Some(path)) => {
                            cx.busy("Restore", "Opening sensitive.age…");
                            Bundle::open(&path, &p).map_err(|e| e.to_string())
                        }
                        (Err(e), _) => Err(e),
                        (_, None) => Err("this backup has no sensitive.age".into()),
                    };
                    match bundle {
                        Ok(b) => {
                            self.pass = None;
                            return self.next(cx, Some(b));
                        }
                        Err(e) => {
                            t.value.clear();
                            self.error = Some(e);
                        }
                    }
                }
                _ => {
                    t.key(&k);
                }
            }
            return Nav::Stay;
        }
        if self.checks.key(&k) {
            return Nav::Stay;
        }
        if k.code != KeyCode::Enter {
            return Nav::Ignored;
        }
        self.error = None;
        if self.checks.on("sensitive") {
            self.pass = Some(TextField::new("", true));
            return Nav::Stay;
        }
        self.next(cx, None)
    }
}
