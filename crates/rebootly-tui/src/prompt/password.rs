//! The BitLocker password line of `rebootly-cli open`: a dot per character
//! (at most 40 shown) and the count, so a missed key is noticed.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{LineEdit, Step, common};

/// BitLocker's upper limit.
const MAX: usize = 256;

#[derive(Default)]
pub struct PasswordLine {
    text: String,
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
        format!("Password: {dots}{more}  {n} {unit}")
    }

    fn entered(&self) -> String {
        "Password: entered".into()
    }

    fn cancelled(&self) -> &'static str {
        "Password: cancelled"
    }
}

impl PasswordLine {
    fn push(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control()) {
            if self.text.chars().count() < MAX {
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
}
