//! Small stateful widgets: a menu, a checkbox tree with sizes, a text field.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::ui::{ACCENT, bold, dim};

/// Up/Down/Home/End over `len` rows; true if the key moved the cursor.
fn nav(cursor: &mut usize, len: usize, k: &KeyEvent) -> bool {
    if len == 0 {
        return false;
    }
    match k.code {
        KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.checked_sub(1).unwrap_or(len - 1),
        KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1) % len,
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = len - 1,
        KeyCode::PageUp => *cursor = cursor.saturating_sub(10),
        KeyCode::PageDown => *cursor = (*cursor + 10).min(len - 1),
        _ => return false,
    }
    true
}

/// A list of choices, each with a label and a dim note.
pub struct Menu {
    pub items: Vec<(String, String)>,
    pub cursor: usize,
}

impl Menu {
    pub fn new(items: Vec<(String, String)>) -> Self {
        Menu { items, cursor: 0 }
    }

    /// Moves the cursor; returns the chosen row on Enter.
    pub fn key(&mut self, k: &KeyEvent) -> Option<usize> {
        if nav(&mut self.cursor, self.items.len(), k) {
            return None;
        }
        (k.code == KeyCode::Enter && !self.items.is_empty()).then_some(self.cursor)
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|(label, note)| {
                let mut spans = vec![Span::styled(label.clone(), bold())];
                if !note.is_empty() {
                    spans.push(Span::styled(format!("  {note}"), dim()));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let mut st = ListState::default().with_selected(Some(self.cursor));
        let list = List::new(items)
            .highlight_symbol("› ")
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
        f.render_stateful_widget(list, area, &mut st);
    }
}

/// One row of a checkbox tree. Headings can't be ticked.
#[derive(Debug, Clone)]
pub struct Check {
    pub label: String,
    pub note: String,
    pub on: bool,
    pub heading: bool,
    /// Caller's key for the row (e.g. `folder:Documents`).
    pub id: String,
}

impl Check {
    pub fn item(id: &str, label: &str, note: String, on: bool) -> Self {
        Check {
            label: label.into(),
            note,
            on,
            heading: false,
            id: id.into(),
        }
    }

    pub fn heading(label: &str) -> Self {
        Check {
            label: label.into(),
            note: String::new(),
            on: false,
            heading: true,
            id: String::new(),
        }
    }
}

pub struct Checks {
    pub rows: Vec<Check>,
    pub cursor: usize,
}

impl Checks {
    pub fn new(rows: Vec<Check>) -> Self {
        let cursor = rows.iter().position(|r| !r.heading).unwrap_or(0);
        Checks { rows, cursor }
    }

    /// Space toggles, `a` ticks all, `n` clears all. True if handled.
    pub fn key(&mut self, k: &KeyEvent) -> bool {
        let len = self.rows.len();
        if nav(&mut self.cursor, len, k) {
            // Skip headings in the direction of travel.
            let up = matches!(k.code, KeyCode::Up | KeyCode::Char('k') | KeyCode::PageUp);
            for _ in 0..len {
                if !self.rows[self.cursor].heading {
                    break;
                }
                nav(
                    &mut self.cursor,
                    len,
                    &KeyEvent::from(if up { KeyCode::Up } else { KeyCode::Down }),
                );
            }
            return true;
        }
        match k.code {
            KeyCode::Char(' ') => {
                if let Some(r) = self.rows.get_mut(self.cursor).filter(|r| !r.heading) {
                    r.on = !r.on;
                }
            }
            KeyCode::Char('a') | KeyCode::Char('n') => {
                let on = k.code == KeyCode::Char('a');
                self.rows
                    .iter_mut()
                    .filter(|r| !r.heading)
                    .for_each(|r| r.on = on);
            }
            _ => return false,
        }
        true
    }

    pub fn on(&self, id: &str) -> bool {
        self.rows.iter().any(|r| r.id == id && r.on)
    }

    /// Ids of ticked rows starting with `prefix`, prefix removed.
    pub fn ticked(&self, prefix: &str) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.on && !r.heading)
            .filter_map(|r| r.id.strip_prefix(prefix).map(str::to_owned))
            .collect()
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|r| {
                if r.heading {
                    return ListItem::new(Line::from(Span::styled(
                        r.label.clone(),
                        bold().fg(ACCENT),
                    )));
                }
                let mark = if r.on { "[x] " } else { "[ ] " };
                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {mark}")),
                    Span::raw(format!("{:<34}", r.label)),
                    Span::styled(r.note.clone(), dim()),
                ]))
            })
            .collect();
        let mut st = ListState::default().with_selected(Some(self.cursor));
        let list = List::new(items)
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
        f.render_stateful_widget(list, area, &mut st);
    }
}

/// One-line text input; `secret` shows dots.
pub struct TextField {
    pub value: String,
    pub secret: bool,
}

impl TextField {
    pub fn new(value: &str, secret: bool) -> Self {
        TextField {
            value: value.into(),
            secret,
        }
    }

    /// Typing and Backspace; true if handled.
    pub fn key(&mut self, k: &KeyEvent) -> bool {
        match k.code {
            KeyCode::Char(c)
                if !k
                    .modifiers
                    .contains(ratatui::crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.value.push(c);
            }
            KeyCode::Backspace => {
                self.value.pop();
            }
            _ => return false,
        }
        true
    }

    pub fn line(&self, focused: bool) -> Line<'static> {
        let shown = if self.secret {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };
        let cursor = if focused { "▏" } else { "" };
        Line::from(vec![
            Span::styled("> ", dim()),
            Span::styled(shown, bold()),
            Span::styled(cursor, Style::default().fg(ACCENT)),
        ])
    }

    pub fn draw(&self, f: &mut Frame, area: Rect, focused: bool) {
        f.render_widget(Paragraph::new(self.line(focused)), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{ch, k};

    #[test]
    fn checks_skip_headings_and_toggle() {
        let mut c = Checks::new(vec![
            Check::heading("Folders"),
            Check::item("folder:Desktop", "Desktop", String::new(), true),
            Check::heading("Apps"),
            Check::item("pack:vscode", "VS Code", String::new(), false),
        ]);
        assert_eq!(c.cursor, 1);
        c.key(&k(KeyCode::Down));
        assert_eq!(c.cursor, 3);
        c.key(&ch(' '));
        assert!(c.on("pack:vscode"));
        c.key(&k(KeyCode::Up));
        assert_eq!(c.cursor, 1);
        c.key(&ch('n'));
        assert!(c.ticked("folder:").is_empty());
        c.key(&ch('a'));
        assert_eq!(c.ticked("pack:"), ["vscode"]);
    }

    #[test]
    fn text_field() {
        let mut t = TextField::new("E:", false);
        t.key(&ch('\\'));
        t.key(&k(KeyCode::Backspace));
        t.key(&ch('/'));
        assert_eq!(t.value, "E:/");
    }
}
