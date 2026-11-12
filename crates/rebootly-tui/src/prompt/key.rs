//! The recovery-key line of `rebootly-cli open`: the digits are shown as
//! typed, in groups of 6 like Windows' own recovery screen, with a count or the group that has a typo. After Enter the line
//! is rewritten as "entered", so the key isn't left in the console's
//! history (or in output pasted elsewhere). Ctrl+V / Shift+Insert paste
//! from the clipboard; a page copied with the key ID and date around the
//! key works too (the key is found in it).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rebootly_core::volume::{find_key, grouped_key, key_progress, pasted_key_digits};

use super::{LineEdit, Step, common};

/// The line being typed (digits only; dashes are only shown).
#[derive(Default)]
pub struct KeyLine {
    digits: String,
    /// Shown instead of the count until the next key.
    note: Option<&'static str>,
}

impl LineEdit for KeyLine {
    fn key(&mut self, k: &KeyEvent) -> Step {
        if let Some(step) = common(k) {
            return step;
        }
        self.note = None;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char(c) if c.is_ascii_digit() && !ctrl && self.digits.len() < 48 => {
                self.digits.push(c);
            }
            KeyCode::Backspace => {
                self.digits.pop();
            }
            KeyCode::Enter if key_progress(&self.digits) == (48, None) => {
                return Step::Done(grouped_key(&self.digits));
            }
            _ => {}
        }
        Step::Continue
    }

    /// Pasted text: a whole key in it replaces the line; part of a key
    /// (digits, dashes, spaces) is added; anything else is refused.
    fn paste(&mut self, text: Option<&str>) {
        if let Some(k) = text.and_then(find_key) {
            self.digits = k.replace('-', "");
        } else if let Some(d) = text.and_then(pasted_key_digits) {
            self.digits.push_str(&d);
            self.digits.truncate(48);
        } else {
            self.note = Some("clipboard: no key");
        }
    }

    /// At most 79 columns.
    fn render(&self) -> String {
        let (n, bad) = key_progress(&self.digits);
        let status = match (self.note, bad) {
            (Some(note), _) => note.to_string(),
            (None, Some(g)) => format!("group {g} is wrong"),
            (None, None) => format!("{n}/48"),
        };
        format!("Key: {}  {status}", grouped_key(&self.digits))
    }

    fn entered(&self) -> String {
        "Key: entered (48 digits)".into()
    }

    fn cancelled(&self) -> &'static str {
        "Key: cancelled"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::{LineEdit, Step};
    use ratatui::crossterm::event::KeyEventKind;

    fn press(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn typed(line: &mut KeyLine, s: &str) -> Step {
        let mut last = Step::Continue;
        for c in s.chars() {
            last = line.key(&press(KeyCode::Char(c)));
        }
        last
    }

    const KEY: &str = "000000-000011-123453-720885-111111-222222-333333-444444";

    #[test]
    fn shows_groups_and_count_and_finishes_on_enter() {
        let mut l = KeyLine::default();
        typed(&mut l, "0000000");
        assert_eq!(l.render(), "Key: 000000-0  7/48");
        l.key(&press(KeyCode::Backspace));
        assert_eq!(l.render(), "Key: 000000  6/48");
        // Dashes and spaces are ignored (pasted keys have them).
        typed(&mut l, &KEY[6..]);
        assert_eq!(l.render(), format!("Key: {KEY}  48/48"));
        assert!(l.render().len() <= 79, "{}", l.render().len());
        assert_eq!(l.key(&press(KeyCode::Enter)), Step::Done(KEY.into()));
    }

    #[test]
    fn typo_is_named_and_enter_waits_for_a_fix() {
        let mut l = KeyLine::default();
        typed(&mut l, &KEY.replace("123453", "123454"));
        assert_eq!(l.render().rsplit("  ").next(), Some("group 3 is wrong"));
        assert!(l.render().len() <= 79, "{}", l.render().len());
        assert_eq!(l.key(&press(KeyCode::Enter)), Step::Continue);
        // A 49th digit is ignored; short keys don't finish either.
        typed(&mut l, "5");
        assert!(l.render().contains("group 3"));
        let mut short = KeyLine::default();
        typed(&mut short, "123");
        assert_eq!(short.key(&press(KeyCode::Enter)), Step::Continue);
    }

    #[test]
    fn paste_takes_the_key_out_of_copied_text() {
        let mut l = KeyLine::default();
        let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        let shift_ins = KeyEvent::new(KeyCode::Insert, KeyModifiers::SHIFT);
        assert_eq!(l.key(&ctrl_v), Step::Paste);
        assert_eq!(l.key(&shift_ins), Step::Paste);
        typed(&mut l, "999");
        l.paste(Some(&format!(
            "Key ID: 1A2B3C4D  Recovery key: {KEY}  11/11/2026"
        )));
        assert_eq!(l.render(), format!("Key: {KEY}  48/48"));
        assert_eq!(l.key(&press(KeyCode::Enter)), Step::Done(KEY.into()));

        // Part of a key is added; anything else is refused with a note.
        let mut l = KeyLine::default();
        l.paste(Some("000000-0000"));
        assert_eq!(l.render(), "Key: 000000-0000  10/48");
        l.paste(Some("hello 2026"));
        assert_eq!(l.render(), "Key: 000000-0000  clipboard: no key");
        l.paste(None);
        assert!(l.render().ends_with("clipboard: no key"));
        typed(&mut l, "1");
        assert_eq!(l.render(), "Key: 000000-00001  11/48");
    }

    #[test]
    fn releases_are_ignored_and_esc_or_ctrl_c_cancel() {
        let mut l = KeyLine::default();
        let mut release = press(KeyCode::Char('1'));
        release.kind = KeyEventKind::Release;
        l.key(&press(KeyCode::Char('1')));
        l.key(&release);
        assert_eq!(l.render(), "Key: 1  1/48");
        assert_eq!(l.key(&press(KeyCode::Esc)), Step::Cancel);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(l.key(&ctrl_c), Step::Cancel);
    }
}
