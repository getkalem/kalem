//! Terminal key events as key chords of the keymap.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kalem_core::keys::{KeyChord, Modifiers};

/// The chord of a key event. `enhanced` is whether the kitty keyboard
/// protocol is on; without it, Control with `4` to `7` is how terminals
/// send `ctrl+\`, `ctrl+]`, `ctrl+^` and `ctrl+_` (which `ctrl+/` sends).
pub fn chord(k: &KeyEvent, enhanced: bool) -> Option<KeyChord> {
    let m = k.modifiers;
    let mut mods = Modifiers {
        ctrl: m.contains(KeyModifiers::CONTROL),
        alt: m.contains(KeyModifiers::ALT) || m.contains(KeyModifiers::META),
        shift: m.contains(KeyModifiers::SHIFT),
        cmd: m.contains(KeyModifiers::SUPER),
    };
    let key = match k.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) if c.is_alphabetic() => {
            mods.shift |= c.is_uppercase();
            c.to_lowercase().collect()
        }
        KeyCode::Char(c) => {
            // Shift is part of the character (`?`, `%`).
            mods.shift = false;
            match c {
                '4' if mods.ctrl && !enhanced => "\\".into(),
                '5' if mods.ctrl && !enhanced => "]".into(),
                '6' if mods.ctrl && !enhanced => "^".into(),
                '7' if mods.ctrl && !enhanced => "_".into(),
                c => c.to_string(),
            }
        }
        KeyCode::Enter => "enter".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => {
            mods.shift = true;
            "tab".into()
        }
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Insert => "insert".into(),
        KeyCode::Esc => "escape".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return None,
    };
    Some(KeyChord { mods, key })
}

/// The text a key event types, if it types one: a character without
/// Control or Alt. On Windows, AltGr arrives as Control with Alt.
pub fn text(k: &KeyEvent) -> Option<char> {
    let KeyCode::Char(c) = k.code else {
        return None;
    };
    let m = k.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let altgr = ctrl && alt && !c.is_ascii_alphabetic();
    ((!ctrl && !alt) || altgr).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode, m: KeyModifiers) -> String {
        chord(&KeyEvent::new(code, m), false).unwrap().to_string()
    }

    #[test]
    fn chords() {
        assert_eq!(k(KeyCode::Char('b'), KeyModifiers::CONTROL), "ctrl+b");
        assert_eq!(
            k(KeyCode::Char('T'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            "alt+shift+t"
        );
        assert_eq!(k(KeyCode::Char('?'), KeyModifiers::SHIFT), "?");
        assert_eq!(k(KeyCode::Char('7'), KeyModifiers::CONTROL), "ctrl+_");
        assert_eq!(k(KeyCode::BackTab, KeyModifiers::SHIFT), "shift+tab");
        assert_eq!(k(KeyCode::Char(' '), KeyModifiers::CONTROL), "ctrl+space");
        assert_eq!(k(KeyCode::F(5), KeyModifiers::NONE), "f5");
        assert_eq!(
            text(&KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            Some('A')
        );
        assert_eq!(
            text(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            None
        );
    }
}
