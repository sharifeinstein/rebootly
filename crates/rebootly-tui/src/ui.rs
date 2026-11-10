//! Drawing and key input behind a small trait, so long-running steps can
//! redraw and read keys, and tests can drive screens with scripted keys on
//! a `TestBackend`.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::{Backend, TestBackend};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub trait Ui {
    fn draw(&mut self, f: &mut dyn FnMut(&mut Frame)) -> io::Result<()>;
    /// The next key press, waiting at most `timeout`.
    fn key(&mut self, timeout: Duration) -> io::Result<Option<KeyEvent>>;
    /// Completion sound (terminal bell).
    fn bell(&mut self) {}
    /// Scripted input ran out (tests): the app loop stops.
    fn finished(&self) -> bool {
        false
    }
}

/// The real terminal (crossterm).
pub struct TermUi<B: Backend> {
    pub term: Terminal<B>,
}

impl<B: Backend> Ui for TermUi<B> {
    fn draw(&mut self, f: &mut dyn FnMut(&mut Frame)) -> io::Result<()> {
        self.term.draw(|fr| f(fr)).map(|_| ())
    }

    fn key(&mut self, timeout: Duration) -> io::Result<Option<KeyEvent>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        match event::read()? {
            // Windows also reports releases; only presses count.
            Event::Key(k) if k.kind != KeyEventKind::Release => Ok(Some(k)),
            _ => Ok(None),
        }
    }

    fn bell(&mut self) {
        use std::io::Write as _;
        let mut out = io::stdout();
        let _ = out.write_all(b"\x07");
        let _ = out.flush();
    }
}

/// Test UI: keys from a script, drawing into a `TestBackend`.
pub struct ScriptUi {
    pub term: Terminal<TestBackend>,
    pub keys: VecDeque<KeyEvent>,
    pub bells: usize,
    /// Calls to `key` that found the script empty.
    idle: usize,
}

impl ScriptUi {
    pub fn new(keys: &[KeyEvent]) -> Self {
        ScriptUi {
            term: Terminal::new(TestBackend::new(100, 32)).expect("test backend"),
            keys: keys.iter().copied().collect(),
            bells: 0,
            idle: 0,
        }
    }

    /// The last drawn screen as text, one line per row.
    pub fn screen(&self) -> String {
        let buf = self.term.backend().buffer();
        let w = buf.area.width as usize;
        buf.content
            .chunks(w)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Ui for ScriptUi {
    fn draw(&mut self, f: &mut dyn FnMut(&mut Frame)) -> io::Result<()> {
        self.term.draw(|fr| f(fr)).map(|_| ())
    }

    fn key(&mut self, _timeout: Duration) -> io::Result<Option<KeyEvent>> {
        let k = self.keys.pop_front();
        if k.is_none() {
            self.idle += 1;
        }
        Ok(k)
    }

    fn bell(&mut self) {
        self.bells += 1;
    }

    fn finished(&self) -> bool {
        self.keys.is_empty() && self.idle > 0
    }
}

/// Shorthand for scripted keys.
pub fn k(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn ch(c: char) -> KeyEvent {
    k(KeyCode::Char(c))
}

pub fn text(s: &str) -> Vec<KeyEvent> {
    s.chars().map(ch).collect()
}

pub fn is_ctrl_c(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c')
}

pub const ACCENT: Color = Color::Cyan;
pub const WARN: Color = Color::Yellow;
pub const BAD: Color = Color::Red;
pub const GOOD: Color = Color::Green;

pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Title bar, body and key hints; returns the body area.
pub fn chrome(f: &mut Frame, title: &str, hints: &str) -> Rect {
    let [top, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());
    let t = Line::from(vec![
        Span::styled(" Rebootly ", bold().fg(Color::Black).bg(ACCENT)),
        Span::raw("  "),
        Span::styled(title.to_owned(), bold()),
    ]);
    f.render_widget(Paragraph::new(t), top);
    f.render_widget(Paragraph::new(Span::styled(hints.to_owned(), dim())), foot);
    body.inner(ratatui::layout::Margin::new(1, 1))
}

/// A centred box over the screen (busy messages, confirmations).
pub fn popup(f: &mut Frame, title: &str, lines: Vec<Line<'_>>) {
    let area = f.area();
    let w = area.width.saturating_sub(8).min(72);
    let h = (lines.len() as u16 + 4).min(area.height);
    let r = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(format!(" {title} "), bold()));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        r,
    );
}
