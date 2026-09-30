//! Editing a one-line input (the palette, a prompt): a cursor, moved by
//! the arrow keys, Home and End, by words with Ctrl (Option on macOS),
//! and deleting on either side of it. The cursor is kept as the number of
//! characters after it, so code that sets or appends to the text leaves
//! it at the end.

/// A key that edits a one-line input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKey {
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
    Backspace,
    Delete,
    WordBackspace,
    WordDelete,
    /// Deletes to the end (Ctrl+K).
    KillToEnd,
    /// Deletes to the start (Ctrl+U).
    KillToStart,
}

/// The edit for key `name` (`left`, `home`, `backspace`…): `word` when
/// the word modifier is held (Ctrl, Option on macOS), `line` for
/// Command on macOS, which goes to either end.
pub fn from_key(name: &str, word: bool, line: bool) -> Option<LineKey> {
    Some(match (name, word, line) {
        ("left", _, true) | ("home", _, _) => LineKey::Home,
        ("right", _, true) | ("end", _, _) => LineKey::End,
        ("left", true, _) => LineKey::WordLeft,
        ("right", true, _) => LineKey::WordRight,
        ("left", ..) => LineKey::Left,
        ("right", ..) => LineKey::Right,
        ("backspace", _, true) => LineKey::KillToStart,
        ("backspace", true, _) => LineKey::WordBackspace,
        ("backspace", ..) => LineKey::Backspace,
        ("delete", true, _) => LineKey::WordDelete,
        ("delete", ..) => LineKey::Delete,
        _ => return None,
    })
}

/// The byte offset of the cursor in `text`, `back` characters from the
/// end (clamped).
pub fn cursor(text: &str, back: usize) -> usize {
    let n = text.chars().count();
    let before = n - back.min(n);
    text.char_indices()
        .nth(before)
        .map_or(text.len(), |(i, _)| i)
}

/// The text before and after the cursor.
pub fn split(text: &str, back: usize) -> (&str, &str) {
    text.split_at(cursor(text, back))
}

/// Inserts `s` at the cursor; the cursor stays after it.
pub fn insert(text: &mut String, back: usize, s: &str) {
    let at = cursor(text, back);
    text.insert_str(at, s);
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte offset a word left of `at`: over non-word characters, then
/// over the word.
fn word_left(text: &str, at: usize) -> usize {
    let mut i = at;
    let mut seen_word = false;
    for (j, c) in text[..at].char_indices().rev() {
        if is_word(c) {
            seen_word = true;
        } else if seen_word {
            break;
        }
        i = j;
    }
    i
}

fn word_right(text: &str, at: usize) -> usize {
    let mut seen_word = false;
    for (j, c) in text[at..].char_indices() {
        if is_word(c) {
            seen_word = true;
        } else if seen_word {
            return at + j;
        }
    }
    text.len()
}

/// Applies `key`; `true` if the text changed.
pub fn apply(text: &mut String, back: &mut usize, key: LineKey) -> bool {
    let at = cursor(text, *back);
    let set_at = |text: &str, back: &mut usize, pos: usize| {
        *back = text[pos..].chars().count();
    };
    match key {
        LineKey::Left => {
            let pos = text[..at].char_indices().next_back().map_or(0, |(i, _)| i);
            set_at(text, back, pos);
            false
        }
        LineKey::Right => {
            let pos = text[at..].chars().next().map_or(at, |c| at + c.len_utf8());
            set_at(text, back, pos);
            false
        }
        LineKey::WordLeft => {
            set_at(text, back, word_left(text, at));
            false
        }
        LineKey::WordRight => {
            set_at(text, back, word_right(text, at));
            false
        }
        LineKey::Home => {
            set_at(text, back, 0);
            false
        }
        LineKey::End => {
            *back = 0;
            false
        }
        LineKey::Backspace => {
            let Some((i, _)) = text[..at].char_indices().next_back() else {
                return false;
            };
            text.replace_range(i..at, "");
            true
        }
        LineKey::Delete => {
            let Some(c) = text[at..].chars().next() else {
                return false;
            };
            text.replace_range(at..at + c.len_utf8(), "");
            *back = back.saturating_sub(1);
            true
        }
        LineKey::WordBackspace => {
            let from = word_left(text, at);
            text.replace_range(from..at, "");
            from != at
        }
        LineKey::WordDelete => {
            let to = word_right(text, at);
            let gone = text[at..to].chars().count();
            text.replace_range(at..to, "");
            *back -= gone;
            to != at
        }
        LineKey::KillToEnd => {
            text.truncate(at);
            let changed = *back > 0;
            *back = 0;
            changed
        }
        LineKey::KillToStart => {
            text.replace_range(..at, "");
            at > 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, keys: &[LineKey]) -> (String, String) {
        let mut t = text.to_string();
        let mut back = 0;
        for k in keys {
            apply(&mut t, &mut back, *k);
        }
        let (a, b) = split(&t, back);
        (a.to_string(), b.to_string())
    }

    #[test]
    fn moving_and_deleting() {
        use LineKey::*;
        assert_eq!(
            run("notes.org", &[Left, Left, Left, Left]),
            ("notes".into(), ".org".into())
        );
        assert_eq!(
            run("notes.org", &[Home, Right, Delete]),
            ("n".into(), "tes.org".into())
        );
        assert_eq!(
            run("notes.org", &[Left, Backspace]),
            ("notes.o".into(), "g".into())
        );
        assert_eq!(run("a/bc de", &[WordLeft]), ("a/bc ".into(), "de".into()));
        assert_eq!(
            run("a/bc de", &[WordLeft, WordLeft]),
            ("a/".into(), "bc de".into())
        );
        assert_eq!(
            run("a/bc de", &[Home, WordRight]),
            ("a".into(), "/bc de".into())
        );
        assert_eq!(
            run("a/bc de", &[WordLeft, WordBackspace]),
            ("a/".into(), "de".into())
        );
        assert_eq!(
            run("a/bc de", &[Home, WordDelete]),
            ("".into(), "/bc de".into())
        );
        assert_eq!(run("abc", &[Left, KillToEnd]), ("ab".into(), "".into()));
        assert_eq!(run("abc", &[Left, KillToStart]), ("".into(), "c".into()));
        // Past either end nothing happens; characters, not bytes.
        assert_eq!(
            run("çğ", &[Right, Left, Left, Left, Delete]),
            ("".into(), "ğ".into())
        );
        let mut t = "şu".to_string();
        let mut back = 1;
        insert(&mut t, back, "ı");
        assert_eq!(t, "şıu");
        apply(&mut t, &mut back, LineKey::End);
        assert_eq!(back, 0);
    }
}
