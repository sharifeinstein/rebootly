//! One-line recovery-key prompt for `rebootly-cli open` (no full screen):
//! the digits are shown as typed, in groups of 6 like Windows' own recovery
//! screen, with a count or the group that has a typo. After Enter the line
//! is rewritten as "entered", so the key isn't left in the console's
//! history (or in output pasted elsewhere).

use std::io::{self, BufRead, IsTerminal, Write};

use ratatui::crossterm::cursor::MoveToColumn;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{self, Clear, ClearType};
use ratatui::crossterm::{execute, queue};
use rebootly_core::volume::{grouped_key, key_progress};

/// What a key press did to the line.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    /// 48 digits, every group plausible.
    Done(String),
    Cancel,
}

/// The line being typed (digits only; dashes are only shown).
#[derive(Default)]
pub struct KeyLine {
    digits: String,
}

impl KeyLine {
    pub fn key(&mut self, k: &KeyEvent) -> Step {
        // Windows reports releases too; only presses type.
        if k.kind != KeyEventKind::Press {
            return Step::Continue;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('c') if ctrl => return Step::Cancel,
            KeyCode::Esc => return Step::Cancel,
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

    /// The line as shown; at most 79 columns (fits an 80-column console,
    /// so redrawing in place never wraps).
    pub fn render(&self) -> String {
        let (n, bad) = key_progress(&self.digits);
        let status = match bad {
            Some(g) => format!("group {g} is wrong"),
            None => format!("{n}/48"),
        };
        format!("Key: {}  {status}", grouped_key(&self.digits))
    }
}

/// Undoes raw mode however the prompt ends.
struct Raw;

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Ask for a recovery key on the terminal; `None` if cancelled. Without a
/// terminal (piped input) one line is read as is.
pub fn recovery_key() -> io::Result<Option<String>> {
    let mut err = io::stderr();
    if !io::stdin().is_terminal() {
        let mut s = String::new();
        io::stdin().lock().read_line(&mut s)?;
        return Ok(Some(s));
    }
    writeln!(
        err,
        "Type the digits (dashes are added). Enter when done, Esc to cancel."
    )?;
    let mut line = KeyLine::default();
    terminal::enable_raw_mode()?;
    let _raw = Raw;
    let show = |err: &mut io::Stderr, text: &str| -> io::Result<()> {
        queue!(err, MoveToColumn(0), Clear(ClearType::CurrentLine), Print(text))?;
        err.flush()
    };
    show(&mut err, &line.render())?;
    loop {
        let Event::Key(k) = event::read()? else {
            continue;
        };
        match line.key(&k) {
            Step::Continue => show(&mut err, &line.render())?,
            Step::Done(key) => {
                show(&mut err, "Key: entered (48 digits)")?;
                execute!(err, Print("\r\n"))?;
                return Ok(Some(key));
            }
            Step::Cancel => {
                show(&mut err, "Key: cancelled")?;
                execute!(err, Print("\r\n"))?;
                return Ok(None);
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
