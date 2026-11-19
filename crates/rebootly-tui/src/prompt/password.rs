//! A secret typed as dots: the BitLocker password of `rebootly-cli open`
//! and the sensitive-bundle passphrase. A dot per character (at most 40
//! shown) and the count, so a missed key is noticed.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{LineEdit, Step, common};

/// BitLocker's upper limit.
const MAX: usize = 256;
/// age has no limit; this only stops a runaway paste.
const PASSPHRASE_MAX: usize = 4096;

pub struct PasswordLine {
    text: String,
    label: &'static str,
    max: usize,
}

impl Default for PasswordLine {
    /// The BitLocker password.
    fn default() -> Self {
        Self {
            text: String::new(),
            label: "Password",
            max: MAX,
        }
    }
}

impl LineEdit for PasswordLine {
    fn key(&mut self, k: &KeyEvent) -> Step {
        if let Some(step) = common(k) {
            return step;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char(c) if !ctrl && !c.is_control() => self.push(&c.to_string()),
            KeyCode::Backspace => {
                self.text.pop();
            }
            KeyCode::Enter if !self.text.is_empty() => return Step::Done(self.text.clone()),
            _ => {}
        }
        Step::Continue
    }

    /// The first line of the pasted text (a copied line often ends with a
    /// line break, which isn't part of the password).
    fn paste(&mut self, text: Option<&str>) {
        if let Some(first) = text.and_then(|t| t.lines().next()) {
            self.push(first);
        }
    }

    fn render(&self) -> String {
        let n = self.text.chars().count();
        let dots = "•".repeat(n.min(40));
        let more = if n > 40 { "…" } else { "" };
        let unit = if n == 1 { "character" } else { "characters" };
        format!("{}: {dots}{more}  {n} {unit}", self.label)
    }

    fn entered(&self) -> String {
        format!("{}: entered", self.label)
    }

    fn cancelled(&self) -> String {
        format!("{}: cancelled", self.label)
    }
}

impl PasswordLine {
    /// A passphrase line titled `label` (at most 16 characters, so the
    /// line stays within 79 columns).
    pub fn passphrase(label: &'static str) -> Self {
        Self {
            text: String::new(),
            label,
            max: PASSPHRASE_MAX,
        }
    }

    fn push(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control()) {
            if self.text.chars().count() < self.max {
                self.text.push(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn dots_count_paste_and_enter() {
        let mut l = PasswordLine::default();
        assert_eq!(l.key(&press(KeyCode::Enter)), Step::Continue, "empty");
        for c in "Sommer 2026".chars() {
            l.key(&press(KeyCode::Char(c)));
        }
        assert_eq!(
            l.render(),
            format!("Password: {}  11 characters", "•".repeat(11))
        );
        assert!(!l.render().contains("Sommer"));
        l.paste(Some("!\r\nsecond line"));
        l.key(&press(KeyCode::Backspace));
        l.paste(Some("!"));
        assert_eq!(
            l.key(&press(KeyCode::Enter)),
            Step::Done("Sommer 2026!".into())
        );
        assert_eq!(l.entered(), "Password: entered");
    }

    #[test]
    fn long_passwords_stay_on_one_line() {
        let mut l = PasswordLine::default();
        l.paste(Some(&"x".repeat(300)));
        assert!(l.render().ends_with("…  256 characters"), "{}", l.render());
        assert!(l.render().chars().count() <= 79);
        let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(l.key(&ctrl_v), Step::Paste);
        assert_eq!(l.key(&press(KeyCode::Esc)), Step::Cancel);
    }

    #[test]
    fn passphrase_line_has_its_label_and_room() {
        let mut l = PasswordLine::passphrase("Passphrase again");
        l.paste(Some(&"y".repeat(300)));
        let shown = l.render();
        assert!(shown.starts_with("Passphrase again: "), "{shown}");
        assert!(shown.ends_with("…  300 characters"), "{shown}");
        assert!(shown.chars().count() <= 79, "{shown}");
        assert_eq!(l.entered(), "Passphrase again: entered");
        assert_eq!(l.cancelled(), "Passphrase again: cancelled");
        assert_eq!(l.key(&press(KeyCode::Enter)), Step::Done("y".repeat(300)));
    }
}
