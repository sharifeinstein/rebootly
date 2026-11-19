//! One-line prompts for the CLI (no full screen): the recovery key (shown
//! as typed, grouped), a BitLocker password and the sensitive-bundle
//! passphrase (dots). Ctrl+V /
//! Shift+Insert paste from the clipboard. After Enter the line is rewritten
//! ("entered"), so nothing secret is left in the console's history or in
//! output pasted elsewhere. Without a terminal (piped input) one line is
//! read as is.

mod key;
mod password;

use std::io::{self, BufRead, IsTerminal, Write};

use ratatui::crossterm::cursor::MoveToColumn;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{self, Clear, ClearType};
use ratatui::crossterm::{execute, queue};

pub use key::KeyLine;
pub use password::PasswordLine;

/// What a key press did to the line.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    /// Accepted (a plausible key; a non-empty password).
    Done(String),
    Cancel,
    /// Ctrl+V / Shift+Insert: read the clipboard and call `paste`.
    Paste,
}

/// A line being edited in place.
pub trait LineEdit {
    fn key(&mut self, k: &KeyEvent) -> Step;
    fn paste(&mut self, text: Option<&str>);
    /// The line as shown; at most 79 columns (fits an 80-column console,
    /// so redrawing in place never wraps).
    fn render(&self) -> String;
    /// What stays on screen after Enter.
    fn entered(&self) -> String;
    fn cancelled(&self) -> String;
}

/// Keys every line treats the same: releases (Windows reports them too)
/// do nothing; Esc / Ctrl+C cancel; Ctrl+V / Shift+Insert paste.
pub(crate) fn common(k: &KeyEvent) -> Option<Step> {
    if k.kind != KeyEventKind::Press {
        return Some(Step::Continue);
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    match k.code {
        KeyCode::Esc => Some(Step::Cancel),
        KeyCode::Char('c') if ctrl => Some(Step::Cancel),
        KeyCode::Char('v') if ctrl => Some(Step::Paste),
        KeyCode::Insert if shift => Some(Step::Paste),
        _ => None,
    }
}

/// Undoes raw mode however the prompt ends.
struct Raw;

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Ask for a recovery key; `None` if cancelled.
pub fn recovery_key() -> io::Result<Option<String>> {
    run(
        KeyLine::default(),
        "Type the digits (dashes are added), or paste them. Enter when done, Esc to cancel.",
    )
}

/// Ask for a BitLocker password; `None` if cancelled.
pub fn password() -> io::Result<Option<String>> {
    run(
        PasswordLine::default(),
        "Type or paste the password (shown as dots). Enter when done, Esc to cancel.",
    )
}

/// Ask for the sensitive-bundle passphrase on a line titled `label`
/// (at most 16 characters), after `intro`; `None` if cancelled. Piped
/// input: one line, without its line break.
pub fn passphrase(label: &'static str, intro: &str) -> io::Result<Option<String>> {
    let got = run(PasswordLine::passphrase(label), intro)?;
    Ok(got.map(|s| s.trim_end_matches(['\r', '\n']).to_owned()))
}

fn run(mut line: impl LineEdit, intro: &str) -> io::Result<Option<String>> {
    let mut err = io::stderr();
    if !io::stdin().is_terminal() {
        let mut s = String::new();
        io::stdin().lock().read_line(&mut s)?;
        return Ok(Some(s));
    }
    writeln!(err, "{intro}")?;
    terminal::enable_raw_mode()?;
    let _raw = Raw;
    let show = |err: &mut io::Stderr, text: &str| -> io::Result<()> {
        queue!(
            err,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            Print(text)
        )?;
        err.flush()
    };
    show(&mut err, &line.render())?;
    loop {
        let step = match event::read()? {
            Event::Key(k) => line.key(&k),
            // A terminal's own paste (bracketed, on Unix terminals).
            Event::Paste(text) => {
                line.paste(Some(&text));
                Step::Continue
            }
            _ => continue,
        };
        match step {
            Step::Continue => show(&mut err, &line.render())?,
            Step::Paste => {
                line.paste(rebootly_host::clipboard_text().as_deref());
                show(&mut err, &line.render())?;
            }
            Step::Done(secret) => {
                show(&mut err, &line.entered())?;
                execute!(err, Print("\r\n"))?;
                return Ok(Some(secret));
            }
            Step::Cancel => {
                show(&mut err, &line.cancelled())?;
                execute!(err, Print("\r\n"))?;
                return Ok(None);
            }
        }
    }
}
