//! gpui key presses as key chords of the keymap.

use gpui::Keystroke;
use kalem_core::keys::{KeyChord, Modifiers};

/// The chord of a key press. `swap_primary`: Command and Control trade
/// places, so that a keymap written with Control uses Command on macOS.
pub fn chord(k: &Keystroke, swap_primary: bool) -> Option<KeyChord> {
    let m = k.modifiers;
    let (mut ctrl, mut cmd) = (m.control, m.platform);
    if swap_primary {
        std::mem::swap(&mut ctrl, &mut cmd);
    }
    let mut mods = Modifiers {
        ctrl,
        alt: m.alt,
        shift: m.shift,
        cmd,
    };
    let key = k.key.as_str();
    let key = match key {
        "" => return None,
        "backspace" | "delete" | "enter" | "tab" | "escape" | "space" | "up" | "down" | "left"
        | "right" | "home" | "end" | "pageup" | "pagedown" | "insert" => key.to_string(),
        k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u8>().is_ok() => k.to_string(),
        k if k.chars().count() == 1 => {
            let c = k.chars().next().expect("one character");
            if c.is_alphabetic() {
                mods.shift |= c.is_uppercase();
                c.to_lowercase().collect()
            } else {
                c.to_string()
            }
        }
        _ => return None,
    };
    Some(KeyChord { mods, key })
}

/// The chord for a key press with the character it types, if gpui reports
/// one: punctuation with Shift becomes the shifted character.
pub fn chord_typed(k: &Keystroke, swap_primary: bool) -> Option<KeyChord> {
    let mut c = chord(k, swap_primary)?;
    if c.mods.shift
        && let Some(t) = &k.key_char
        && t.chars().count() == 1
        && !t.chars().all(char::is_alphabetic)
        && !c.key.chars().all(char::is_alphabetic)
    {
        c.mods.shift = false;
        c.key = t.clone();
    }
    Some(c)
}

/// The gpui keystroke text for a chord (`cmd-s`), for menu shortcuts.
pub fn gpui_keys(c: &KeyChord, swap_primary: bool) -> String {
    let (mut ctrl, mut cmd) = (c.mods.ctrl, c.mods.cmd);
    if swap_primary {
        std::mem::swap(&mut ctrl, &mut cmd);
    }
    let mut s = String::new();
    for (on, name) in [
        (ctrl, "ctrl-"),
        (c.mods.alt, "alt-"),
        (c.mods.shift, "shift-"),
        (cmd, "cmd-"),
    ] {
        if on {
            s.push_str(name);
        }
    }
    s.push_str(&c.key);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        let k = Keystroke::parse("cmd-b").unwrap();
        assert_eq!(chord(&k, true).unwrap().to_string(), "ctrl+b");
        assert_eq!(chord(&k, false).unwrap().to_string(), "cmd+b");
        let k = Keystroke::parse("ctrl-a").unwrap();
        assert_eq!(chord(&k, true).unwrap().to_string(), "cmd+a");
        let k = Keystroke::parse("shift-tab").unwrap();
        assert_eq!(chord(&k, true).unwrap().to_string(), "shift+tab");
        let k = Keystroke::parse("alt-up").unwrap();
        assert_eq!(chord(&k, true).unwrap().to_string(), "alt+up");
        assert_eq!(
            gpui_keys(&KeyChord::parse("ctrl+s").unwrap(), true),
            "cmd-s"
        );
    }
}
