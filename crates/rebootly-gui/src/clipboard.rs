//! Paste in every text field on Windows without eframe's clipboard crates
//! (they bring the `image` crate and more): eframe's own shortcut handling
//! is off there (`clipboard_shortcuts: false`), and the keys are turned
//! into events here, the paste filled from the Windows clipboard.
//! Copy and Cut still work inside the window; they don't reach other apps.

use eframe::egui::{Event, Key};

/// Ctrl+V / Shift+Insert → `Paste` with `read()`'s text (dropped if the
/// clipboard has none); Ctrl+C / Ctrl+Insert → `Copy`; Ctrl+X /
/// Shift+Delete → `Cut`. Other events pass unchanged.
pub fn translate(events: &mut Vec<Event>, read: impl Fn() -> Option<String>) {
    let old = std::mem::take(events);
    for e in old {
        let Event::Key {
            key,
            pressed: true,
            modifiers: m,
            ..
        } = &e
        else {
            events.push(e);
            continue;
        };
        let cmd = m.command_only();
        match key {
            Key::V if cmd => {
                if let Some(t) = read().filter(|t| !t.is_empty()) {
                    events.push(Event::Paste(t.replace("\r\n", "\n")));
                }
            }
            Key::Insert if m.shift => {
                if let Some(t) = read().filter(|t| !t.is_empty()) {
                    events.push(Event::Paste(t.replace("\r\n", "\n")));
                }
            }
            Key::C if cmd => events.push(Event::Copy),
            Key::Insert if m.ctrl => events.push(Event::Copy),
            Key::X if cmd => events.push(Event::Cut),
            Key::Delete if m.shift => events.push(Event::Cut),
            _ => events.push(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Modifiers;

    fn key(key: Key, modifiers: Modifiers, pressed: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn shortcuts_become_clipboard_events() {
        let shift = Modifiers::SHIFT;
        let mut ev = vec![
            key(Key::V, Modifiers::COMMAND, true),
            key(Key::V, Modifiers::COMMAND, false),
            key(Key::Insert, shift, true),
            key(Key::C, Modifiers::COMMAND, true),
            key(Key::X, Modifiers::COMMAND, true),
            key(Key::V, Modifiers::NONE, true),
            Event::Text("v".into()),
        ];
        translate(&mut ev, || Some("a\r\nb".into()));
        assert_eq!(ev[0], Event::Paste("a\nb".into()));
        assert!(matches!(ev[1], Event::Key { pressed: false, .. }));
        assert_eq!(ev[2], Event::Paste("a\nb".into()));
        assert_eq!(ev[3], Event::Copy);
        assert_eq!(ev[4], Event::Cut);
        assert!(matches!(ev[5], Event::Key { key: Key::V, .. }));
        assert_eq!(ev[6], Event::Text("v".into()));

        // An empty clipboard pastes nothing.
        let mut ev = vec![key(Key::V, Modifiers::COMMAND, true)];
        translate(&mut ev, || None);
        assert!(ev.is_empty());
    }
}
