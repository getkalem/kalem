//! Positions: Kalem's byte offsets against the protocol's lines and
//! characters, counted in the encoding the server and the client agreed
//! on (UTF-16 unless the server offers UTF-8 or UTF-32). The bugs language
//! integrations are known for, off by one at a non-ASCII character, are
//! here and nowhere else.

use std::ops::Range;

use serde_json::{Value, json};

/// How the `character` of a position is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// Bytes of UTF-8.
    Utf8,
    /// UTF-16 code units, the protocol's default.
    #[default]
    Utf16,
    /// Unicode scalar values.
    Utf32,
}

impl Encoding {
    /// The protocol's name: `utf-8`, `utf-16`, `utf-32`.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16 => "utf-16",
            Encoding::Utf32 => "utf-32",
        }
    }

    /// From the protocol's name; UTF-16 for anything else.
    pub fn from_name(name: &str) -> Encoding {
        match name {
            "utf-8" => Encoding::Utf8,
            "utf-32" => Encoding::Utf32,
            _ => Encoding::Utf16,
        }
    }

    fn width(self, c: char) -> u32 {
        match self {
            Encoding::Utf8 => c.len_utf8() as u32,
            Encoding::Utf16 => c.len_utf16() as u32,
            Encoding::Utf32 => 1,
        }
    }
}

/// A protocol position: a zero-based line and a character in the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    /// The line, from 0.
    pub line: u32,
    /// The character in the line, in the agreed encoding.
    pub character: u32,
}

impl Position {
    /// As JSON.
    pub fn to_json(self) -> Value {
        json!({ "line": self.line, "character": self.character })
    }

    /// From JSON; `None` when it is not a position.
    pub fn from_json(v: &Value) -> Option<Position> {
        Some(Position {
            line: u32::try_from(v.get("line")?.as_u64()?).ok()?,
            character: u32::try_from(v.get("character")?.as_u64()?).ok()?,
        })
    }
}

/// The position of byte `at` of `text` (clamped to the text, and moved
/// back to a character boundary).
pub fn position(text: &str, at: usize, enc: Encoding) -> Position {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = text[..line_start].bytes().filter(|&b| b == b'\n').count() as u32;
    let character = text[line_start..at].chars().map(|c| enc.width(c)).sum();
    Position { line, character }
}

/// The byte offset of `pos` in `text`. A line past the end gives the
/// end; a character past the line's end gives the line's end (before its
/// line feed); a character inside a UTF-16 pair gives the character's
/// start.
pub fn offset(text: &str, pos: Position, enc: Encoding) -> usize {
    let mut start = 0;
    for _ in 0..pos.line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = text[start..line_end].trim_end_matches('\r');
    let mut count = 0;
    for (i, c) in line.char_indices() {
        let w = enc.width(c);
        if count + w > pos.character {
            return start + i;
        }
        count += w;
    }
    start + line.len()
}

/// A protocol range for bytes `range` of `text`.
pub fn range_json(text: &str, range: Range<usize>, enc: Encoding) -> Value {
    json!({
        "start": position(text, range.start, enc).to_json(),
        "end": position(text, range.end, enc).to_json(),
    })
}

/// The bytes of `text` a protocol range covers; `None` when it is not a
/// range.
pub fn byte_range(text: &str, range: &Value, enc: Encoding) -> Option<Range<usize>> {
    let start = offset(text, Position::from_json(range.get("start")?)?, enc);
    let end = offset(text, Position::from_json(range.get("end")?)?, enc);
    Some(start.min(end)..end.max(start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn utf16_counts_pairs() {
        let text = "a😀b\nçx";
        let enc = Encoding::Utf16;
        assert_eq!(
            position(text, 0, enc),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            position(text, 1, enc),
            Position {
                line: 0,
                character: 1
            }
        );
        assert_eq!(
            position(text, 5, enc),
            Position {
                line: 0,
                character: 3
            }
        );
        assert_eq!(
            position(text, 7, enc),
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            position(text, 9, enc),
            Position {
                line: 1,
                character: 1
            }
        );
        // Inside the pair: the emoji's start.
        assert_eq!(
            offset(
                text,
                Position {
                    line: 0,
                    character: 2
                },
                enc
            ),
            1
        );
        assert_eq!(
            offset(
                text,
                Position {
                    line: 0,
                    character: 99
                },
                enc
            ),
            6
        );
        assert_eq!(
            offset(
                text,
                Position {
                    line: 9,
                    character: 0
                },
                enc
            ),
            text.len()
        );
    }

    #[test]
    fn utf8_and_utf32() {
        let text = "é😀z";
        assert_eq!(position(text, 6, Encoding::Utf8).character, 6);
        assert_eq!(position(text, 6, Encoding::Utf32).character, 2);
        assert_eq!(
            offset(
                text,
                Position {
                    line: 0,
                    character: 2
                },
                Encoding::Utf32
            ),
            6
        );
    }

    #[test]
    fn crlf_line_end() {
        let text = "ab\r\ncd";
        assert_eq!(
            offset(
                text,
                Position {
                    line: 0,
                    character: 9
                },
                Encoding::Utf16
            ),
            2
        );
        assert_eq!(
            offset(
                text,
                Position {
                    line: 1,
                    character: 1
                },
                Encoding::Utf16
            ),
            5
        );
    }

    proptest! {
        #[test]
        fn round_trip(text in "(a|é|😀|e\u{301}|\n|\r\n| ){0,40}", at in 0usize..200) {
            for enc in [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32] {
                let mut b = at.min(text.len());
                while !text.is_char_boundary(b) { b -= 1; }
                // A byte between `\r` and `\n` maps to the line's end.
                let p = position(&text, b, enc);
                let back = offset(&text, p, enc);
                if text[..b].ends_with('\r') && text[b..].starts_with('\n') {
                    prop_assert_eq!(back, b - 1);
                } else {
                    prop_assert_eq!(back, b);
                }
            }
        }
    }
}
