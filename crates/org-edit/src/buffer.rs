//! A small model of an Emacs buffer, so that commands can be written as
//! the sequence of edits the Emacs command makes, with point moving the
//! way Emacs moves it.

use unicode_width::UnicodeWidthChar;

use crate::transaction::{Selection, Transaction};

/// An error a command signals, with the message Emacs shows. The text is
/// unchanged; `point` is where Emacs leaves the cursor, if it moved it
/// before failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditError {
    /// The message.
    pub message: String,
    /// The cursor position after the failed command, if it changed.
    pub point: Option<usize>,
}

impl EditError {
    pub(crate) fn new(message: &str) -> EditError {
        EditError {
            message: message.to_string(),
            point: None,
        }
    }

    pub(crate) fn at(message: &str, point: usize) -> EditError {
        EditError {
            message: message.to_string(),
            point: Some(point),
        }
    }
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EditError {}

/// `user-error`.
pub(crate) fn user_error<T>(msg: &str) -> Result<T, EditError> {
    Err(EditError::new(msg))
}

/// Emacs's `char-width` (tabs are handled by [`column_at`]).
pub(crate) fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// `string-width`.
pub(crate) fn string_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// The parts of a line that font-lock hides with `org-link-descriptive`:
/// the brackets of bracket links and, when there is a description, the
/// target. Byte ranges relative to the line.
pub(crate) fn hidden_link_ranges(line: &str) -> Vec<(usize, usize)> {
    use org_syntax::{NodeOrToken, SyntaxKind};
    if !line.contains("[[") {
        return Vec::new();
    }
    let parse = org_syntax::parse(line);
    let mut out = Vec::new();
    for link in parse
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::LINK)
    {
        let markers: Vec<(String, usize, usize)> = link
            .children_with_tokens()
            .filter_map(|c| match c {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::MARKER => {
                    let r = t.text_range();
                    Some((
                        t.text().to_string(),
                        usize::from(r.start()),
                        usize::from(r.end()),
                    ))
                }
                _ => None,
            })
            .collect();
        let Some(open) = markers.first().filter(|m| m.0 == "[[") else {
            continue;
        };
        let Some(close) = markers.iter().find(|m| m.0 == "]]") else {
            continue;
        };
        match markers.iter().find(|m| m.0 == "][") {
            Some(mid) => {
                out.push((open.1, mid.2));
                out.push((close.1, close.2));
            }
            None => {
                out.push((open.1, open.2));
                out.push((close.1, close.2));
            }
        }
    }
    out.sort();
    out
}

/// `current-column` at `pos`: display columns from the line start, with
/// tabs to the next multiple of 8 and hidden link markup counting for
/// nothing.
pub(crate) fn column_at(text: &str, pos: usize) -> usize {
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let eol = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    let hidden = hidden_link_ranges(&text[bol..eol]);
    let mut col = 0;
    for (i, c) in text[bol..pos].char_indices() {
        if hidden.iter().any(|&(s, e)| i >= s && i < e) {
            continue;
        }
        if c == '\t' {
            col = (col / 8 + 1) * 8;
        } else {
            col += char_width(c);
        }
    }
    col
}

/// `move-to-column` on the line at `bol`: the first position at or past
/// display column `col`, after any hidden text there, or the line end.
pub(crate) fn move_to_column(text: &str, bol: usize, col: usize) -> usize {
    let eol = text[bol..].find('\n').map_or(text.len(), |i| bol + i);
    let hidden = hidden_link_ranges(&text[bol..eol]);
    let mut c = 0;
    let mut i = 0;
    let line = &text[bol..eol];
    while i < line.len() {
        if let Some(&(_, e)) = hidden.iter().find(|&&(s, e)| i >= s && i < e) {
            i = e;
            continue;
        }
        if c >= col {
            return bol + i;
        }
        let ch = line[i..].chars().next().expect("char");
        c = if ch == '\t' {
            (c / 8 + 1) * 8
        } else {
            c + char_width(ch)
        };
        i += ch.len_utf8();
    }
    eol
}

/// Text being edited, with point and markers.
#[derive(Debug, Clone)]
pub(crate) struct Buf {
    pub(crate) text: String,
    pub(crate) point: usize,
    original: String,
    /// Emacs markers (insertion type nil): positions that follow edits.
    markers: Vec<usize>,
}

/// Where a marker at `p` goes when `start..end` becomes `len` bytes: a
/// position inside the old text moves to its start, one at its end or after
/// moves with it, one at an insertion point stays before the insertion.
fn adjust(p: usize, start: usize, end: usize, len: usize) -> usize {
    if p >= end && end > start {
        p + len - (end - start)
    } else if p > start {
        if end == start { p + len } else { start }
    } else {
        p
    }
}

impl Buf {
    pub(crate) fn new(text: &str, point: usize) -> Buf {
        Buf {
            text: text.to_string(),
            point,
            original: text.to_string(),
            markers: Vec::new(),
        }
    }

    /// A new marker at `pos`; see [`Buf::marker`].
    pub(crate) fn add_marker(&mut self, pos: usize) -> usize {
        self.markers.push(pos);
        self.markers.len() - 1
    }

    /// The position of a marker.
    pub(crate) fn marker(&self, id: usize) -> usize {
        self.markers[id]
    }

    fn edit(&mut self, start: usize, end: usize, new: &str) {
        for m in &mut self.markers {
            *m = adjust(*m, start, end, new.len());
        }
        self.text.replace_range(start..end, new);
    }

    /// `replace-match` / `replace_range`: a position inside the replaced
    /// text moves to its start, one at its end or after moves with it.
    pub(crate) fn replace(&mut self, start: usize, end: usize, new: &str) {
        let p = self.point;
        self.point = if p >= end {
            p + new.len() - (end - start)
        } else if p > start {
            start
        } else {
            p
        };
        self.edit(start, end, new);
    }

    /// `replace-match` with an empty string followed by
    /// `insert-before-markers`: a position inside or at either end of the
    /// replaced text, point or marker, ends up after the new text.
    pub(crate) fn replace_before_markers(&mut self, start: usize, end: usize, new: &str) {
        let f = |p: usize| {
            if p > end {
                p + new.len() - (end - start)
            } else if p >= start {
                start + new.len()
            } else {
                p
            }
        };
        self.point = f(self.point);
        for m in &mut self.markers {
            *m = f(*m);
        }
        self.text.replace_range(start..end, new);
    }

    /// `delete-region`: positions inside or at the end collapse to the
    /// start.
    pub(crate) fn delete(&mut self, start: usize, end: usize) {
        let p = self.point;
        self.point = if p >= end {
            p - (end - start)
        } else if p > start {
            start
        } else {
            p
        };
        self.edit(start, end, "");
    }

    /// `insert` at `pos` when point is elsewhere or kept by a marker: point
    /// at `pos` stays before the text (`save-excursion`).
    pub(crate) fn insert_before_point(&mut self, pos: usize, s: &str) {
        if self.point > pos {
            self.point += s.len();
        }
        self.edit(pos, pos, s);
    }

    /// `insert` at point: point moves after the text.
    pub(crate) fn insert_at_point(&mut self, s: &str) {
        let p = self.point;
        self.edit(p, p, s);
        self.point = p + s.len();
    }

    /// The start of the line containing `pos`.
    pub(crate) fn bol(&self, pos: usize) -> usize {
        self.text[..pos].rfind('\n').map_or(0, |i| i + 1)
    }

    /// The end of the line containing `pos` (before its line feed).
    pub(crate) fn eol(&self, pos: usize) -> usize {
        self.text[pos..]
            .find('\n')
            .map_or(self.text.len(), |i| pos + i)
    }

    /// The transaction from the original text to the current one: one
    /// replacement covering the changed part.
    pub(crate) fn transaction(&self, label: &str) -> Transaction {
        let (a, b) = (self.original.as_bytes(), self.text.as_bytes());
        let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        while !self.original.is_char_boundary(pre) || !self.text.is_char_boundary(pre) {
            pre -= 1;
        }
        let max_suf = a.len().min(b.len()) - pre;
        let mut suf = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(max_suf)
            .take_while(|(x, y)| x == y)
            .count();
        while !self.original.is_char_boundary(a.len() - suf)
            || !self.text.is_char_boundary(b.len() - suf)
        {
            suf -= 1;
        }
        let mut t = Transaction::new(label);
        if pre + suf < a.len() || pre + suf < b.len() {
            t.replace(pre..a.len() - suf, &self.text[pre..b.len() - suf])
                .expect("one edit");
        }
        t.select(Selection::caret(self.point))
    }
}
