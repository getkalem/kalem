//! Key chords and sequences, written as in keymaps: `ctrl+shift+t`,
//! `alt+up`, or Emacs style `C-c C-t` (a sequence of two chords).

use std::fmt;

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Modifiers {
    /// Control.
    pub ctrl: bool,
    /// Alt (Option on macOS, Meta in Emacs).
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// Command on macOS, Super elsewhere.
    pub cmd: bool,
}

/// One key press with modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyChord {
    /// The modifiers.
    pub mods: Modifiers,
    /// The key: a character in lower case, or a name such as `enter`,
    /// `tab`, `up`, `f5`.
    pub key: String,
}

/// Keys pressed in order, such as `C-c C-t`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeySequence(pub Vec<KeyChord>);

const NAMED: &[&str] = &[
    "enter",
    "tab",
    "escape",
    "space",
    "backspace",
    "delete",
    "insert",
    "home",
    "end",
    "pageup",
    "pagedown",
    "up",
    "down",
    "left",
    "right",
];

fn key_name(k: &str) -> Option<String> {
    let l = k.to_lowercase();
    let l = match l.as_str() {
        "ret" | "return" => "enter".to_string(),
        "esc" => "escape".to_string(),
        "spc" => "space".to_string(),
        "del" => "delete".to_string(),
        "bs" => "backspace".to_string(),
        "pgup" => "pageup".to_string(),
        "pgdn" => "pagedown".to_string(),
        _ => l,
    };
    let function = l
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
    (NAMED.contains(&l.as_str()) || function || l.chars().count() == 1).then_some(l)
}

impl KeyChord {
    /// Parses `ctrl+shift+t`, `Alt+Up` or Emacs style `C-M-x`, `S-<up>`.
    pub fn parse(s: &str) -> Option<KeyChord> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let mut mods = Modifiers::default();
        // Emacs style: prefixes like `C-`, `M-`, `S-`, `s-`, and `<name>`.
        let emacs = s.len() > 2 && s.as_bytes()[1] == b'-' && "CMSs".contains(s.chars().next()?);
        if emacs || s.starts_with('<') {
            let mut rest = s;
            while rest.len() > 2 && rest.as_bytes()[1] == b'-' {
                match rest.as_bytes()[0] {
                    b'C' => mods.ctrl = true,
                    b'M' => mods.alt = true,
                    b'S' => mods.shift = true,
                    b's' => mods.cmd = true,
                    _ => return None,
                }
                rest = &rest[2..];
            }
            let key = rest
                .strip_prefix('<')
                .and_then(|r| r.strip_suffix('>'))
                .unwrap_or(rest);
            return Some(KeyChord {
                mods,
                key: key_name(key)?,
            });
        }
        // `+` separated; a final `+` is the plus key.
        let (mods_part, key) = match s.rsplit_once('+') {
            Some((m, "")) => (m.trim_end_matches('+'), "+"),
            Some((m, k)) => (m, k),
            None => ("", s),
        };
        for m in mods_part.split('+').filter(|m| !m.is_empty()) {
            match m.to_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" | "meta" => mods.alt = true,
                "shift" => mods.shift = true,
                "cmd" | "super" | "win" => mods.cmd = true,
                _ => return None,
            }
        }
        Some(KeyChord {
            mods,
            key: key_name(key)?,
        })
    }
}

impl KeyChord {
    /// Whether a terminal without an enhanced keyboard protocol (such as
    /// kitty's) can send this chord distinctly. Control with Shift, with a
    /// digit, with Enter or Tab, `ctrl+i`, `ctrl+m`, `ctrl+h` and `ctrl+[`
    /// arrive as other keys; Command never reaches the terminal.
    pub fn terminal_safe(&self) -> bool {
        let m = self.mods;
        if m.cmd {
            return false;
        }
        let k = self.key.as_str();
        let letter = k.len() == 1 && k.as_bytes()[0].is_ascii_lowercase();
        let navigation = matches!(
            k,
            "up" | "down"
                | "left"
                | "right"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
                | "delete"
                | "insert"
        ) || (k.len() > 1 && k.starts_with('f'));
        if m.ctrl {
            if navigation {
                return true;
            }
            if letter {
                return !m.shift && !matches!(k, "i" | "m" | "h");
            }
            // NUL, 0x1C, 0x1D and 0x1F (which `ctrl+/` sends too).
            return !m.shift && matches!(k, "space" | "\\" | "]" | "_");
        }
        match k {
            "enter" | "space" | "backspace" => !m.shift,
            "tab" => !m.alt,
            "escape" => !m.alt && !m.shift,
            _ => true,
        }
    }

    /// The chord a legacy terminal gets instead of this one: Alt with the
    /// same letter or digit for Control chords that cannot be sent
    /// (`ctrl+shift+t` becomes `alt+t`, `ctrl+1` `alt+1`), and `ctrl+_` for
    /// `ctrl+/`, which terminals send as `ctrl+_`.
    pub fn terminal_variant(&self) -> Option<KeyChord> {
        if self.terminal_safe() {
            return Some(self.clone());
        }
        if self.mods.ctrl && !self.mods.shift && !self.mods.cmd && self.key == "/" {
            return Some(KeyChord {
                key: "_".into(),
                ..self.clone()
            });
        }
        let k = self.key.as_bytes();
        let alnum = k.len() == 1 && k[0].is_ascii_alphanumeric();
        (self.mods.ctrl && !self.mods.cmd && alnum).then(|| KeyChord {
            mods: Modifiers {
                alt: true,
                ..Modifiers::default()
            },
            key: self.key.clone(),
        })
    }
}

impl KeySequence {
    /// Parses chords separated by spaces.
    pub fn parse(s: &str) -> Option<KeySequence> {
        let chords: Option<Vec<KeyChord>> = s.split_whitespace().map(KeyChord::parse).collect();
        chords.filter(|c| !c.is_empty()).map(KeySequence)
    }

    /// Whether every chord is [`KeyChord::terminal_safe`].
    pub fn terminal_safe(&self) -> bool {
        self.0.iter().all(KeyChord::terminal_safe)
    }

    /// The sequence with each chord replaced by its
    /// [`KeyChord::terminal_variant`], if they all have one.
    pub fn terminal_variant(&self) -> Option<KeySequence> {
        self.0
            .iter()
            .map(KeyChord::terminal_variant)
            .collect::<Option<Vec<_>>>()
            .map(KeySequence)
    }

    /// Whether this sequence begins with `prefix` (and is longer).
    pub fn starts_with(&self, prefix: &KeySequence) -> bool {
        self.0.len() > prefix.0.len() && self.0[..prefix.0.len()] == prefix.0[..]
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.mods.ctrl, "ctrl"),
            (self.mods.alt, "alt"),
            (self.mods.shift, "shift"),
            (self.mods.cmd, "cmd"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&self.key)
    }
}

impl fmt::Display for KeySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(ToString::to_string).collect();
        f.write_str(&parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        let c = KeyChord::parse("Ctrl+Shift+T").unwrap();
        assert!(c.mods.ctrl && c.mods.shift && c.key == "t");
        assert_eq!(KeyChord::parse("ctrl++").unwrap().key, "+");
        assert_eq!(
            KeySequence::parse("C-c C-t").unwrap().to_string(),
            "ctrl+c ctrl+t"
        );
        assert_eq!(KeySequence::parse("M-<up>").unwrap().to_string(), "alt+up");
        assert_eq!(
            KeySequence::parse("S-M-RET").unwrap().to_string(),
            "alt+shift+enter"
        );
        assert!(KeyChord::parse("hyper+x").is_none());
        let safe = |k: &str| KeySequence::parse(k).unwrap().terminal_safe();
        for k in [
            "ctrl+b",
            "C-c C-t",
            "alt+shift+up",
            "M-RET",
            "shift+tab",
            "ctrl+space",
            "ctrl+left",
            "f5",
        ] {
            assert!(safe(k), "{k}");
        }
        for k in [
            "ctrl+shift+t",
            "ctrl+1",
            "ctrl+i",
            "ctrl+enter",
            "shift+enter",
            "cmd+s",
            "C-/",
        ] {
            assert!(!safe(k), "{k}");
        }
        let variant = |k: &str| {
            KeySequence::parse(k)
                .unwrap()
                .terminal_variant()
                .map(|v| v.to_string())
        };
        assert_eq!(variant("ctrl+shift+t").as_deref(), Some("alt+t"));
        assert_eq!(variant("ctrl+1").as_deref(), Some("alt+1"));
        assert_eq!(variant("ctrl+b").as_deref(), Some("ctrl+b"));
        assert_eq!(variant("ctrl+enter"), None);
        assert_eq!(variant("C-/").as_deref(), Some("ctrl+_"));
        assert!(
            KeySequence::parse("C-c C-t")
                .unwrap()
                .starts_with(&KeySequence::parse("C-c").unwrap())
        );
    }
}
