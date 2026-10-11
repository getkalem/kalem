//! vim-surround's keys, which Doom Emacs has as evil-surround: `ds` and
//! a mark deletes the pair around the cursor, `cs` and two marks changes
//! it, `ys` with a motion or text object and a mark puts a pair around
//! the text, `yss` around the line. A mark is a bracket (`(`, `[`, `{`,
//! `<`, or `b`, `r`, `B`, `a`; an opening one puts spaces inside the
//! pair, and deleting with one takes them away), a quote, `t` or `<` for
//! a tag typed to `>`, or any other sign for itself on both sides, which
//! makes Org's and Markdown's emphasis (`ysiw*`).

use std::ops::Range;
use std::time::Instant;

use org_edit::{ChangeKind, Selection, Transaction};

use super::{
    DocumentState, Key, Target, Vim, char_at, first_non_blank, line_end, line_of, line_start,
    object_n, objects,
};

/// What the surround keys wait for.
#[derive(Debug, Clone)]
pub(super) enum Surround {
    /// `ds`: the mark of the pair to delete.
    Delete,
    /// `cs`: the mark of the pair to change, then (`Some`) the new one.
    Change(Option<char>),
    /// `ys` and a motion: the mark of the pair to put around the text.
    Add(Target),
    /// A tag being typed (`cs"<em>`, `ysiw<p>`): what it goes into, and
    /// the tag so far.
    Tag(Box<Surround>, String),
}

/// The two halves a mark puts around text, or `None` for a letter that
/// names nothing.
fn pair(c: char) -> Option<(String, String)> {
    let (open, close) = match c {
        '(' => ("( ", " )"),
        ')' | 'b' => ("(", ")"),
        '[' => ("[ ", " ]"),
        ']' | 'r' => ("[", "]"),
        '{' => ("{ ", " }"),
        '}' | 'B' => ("{", "}"),
        '>' | 'a' => ("<", ">"),
        c if c.is_alphanumeric() || c.is_whitespace() || c.is_control() => return None,
        c => return Some((c.to_string(), c.to_string())),
    };
    Some((open.into(), close.into()))
}

/// The pair around `pos` that mark `c` names, `n` pairs out: the ranges
/// of its opening and closing halves, with the blanks inside for an
/// opening bracket.
fn around(
    doc: &DocumentState,
    pos: usize,
    c: char,
    n: usize,
) -> Option<(Range<usize>, Range<usize>)> {
    let text = doc.text().as_str();
    let (open, close) = match c {
        '(' | ')' | 'b' | '[' | ']' | 'r' | '{' | '}' | 'B' | '<' | '>' | 'a' => {
            let key = match c {
                'b' => ')',
                'r' => ']',
                'B' => '}',
                'a' => '>',
                c => c,
            };
            let Target::Chars(r) = object_n(doc, pos, key, false, n)? else {
                return None;
            };
            let close_len = text[..r.end].chars().next_back()?.len_utf8();
            (r.start..r.start + 1, r.end - close_len..r.end)
        }
        '"' | '\'' | '`' => {
            let r = objects::current_quote(doc, pos, c, false, 1)?;
            (r.start.checked_sub(1)?..r.start, r.end..r.end + 1)
        }
        't' => super::tag_around(text, pos)?,
        c if pair(c).is_some() => {
            // A sign: the one before the cursor and the next after it on
            // the line, or the cursor's own and the next.
            let line = line_of(doc, pos);
            let (s, e) = (line_start(doc, line), line_end(doc, line));
            let len = c.len_utf8();
            let after = |from: usize| text[from..e].find(c).map(|i| from + i);
            let (a, b) = if char_at(doc, pos) == Some(c) {
                match after(pos + len) {
                    Some(b) => (pos, b),
                    None => (text[s..pos].rfind(c).map(|i| s + i)?, pos),
                }
            } else {
                (text[s..pos].rfind(c).map(|i| s + i)?, after(pos)?)
            };
            (a..a + len, b..b + len)
        }
        _ => return None,
    };
    if matches!(c, '(' | '[' | '{' | '<') {
        let blank = |ch: char| ch == ' ' || ch == '\t';
        let inner = &text[open.end..close.start];
        let lead = inner.len() - inner.trim_start_matches(blank).len();
        let trail = (inner.len() - inner.trim_end_matches(blank).len()).min(inner.len() - lead);
        return Some((open.start..open.end + lead, close.start - trail..close.end));
    }
    Some((open, close))
}

impl Vim {
    /// A key for the surround keys waiting in `s`.
    pub(super) fn surround_key(&mut self, doc: &mut DocumentState, s: Surround, key: Key) {
        if let Surround::Tag(into, mut tag) = s {
            match key {
                Key::Char('>') | Key::Enter => {
                    let name: String = tag.split_whitespace().next().unwrap_or("").into();
                    if name.is_empty() {
                        self.failed = true;
                        return self.reset();
                    }
                    let halves = (format!("<{tag}>"), format!("</{name}>"));
                    return self.surround_with(doc, *into, halves);
                }
                Key::Char(c) => tag.push(c),
                Key::Backspace => {
                    tag.pop();
                }
                _ => return self.reset(),
            }
            self.surround = Some(Surround::Tag(into, tag));
            return;
        }
        let Key::Char(c) = key else {
            // Escape, or a key that is no mark: nothing.
            self.failed = key != Key::Esc;
            return self.reset();
        };
        match s {
            Surround::Delete => {
                let n = self.count.take().unwrap_or(1);
                let Some((open, close)) = around(doc, self.cursor, c, n) else {
                    self.failed = true;
                    return self.reset();
                };
                self.replace_halves(doc, open, close, ("", ""));
                self.changed();
                self.reset();
            }
            Surround::Change(None) => self.surround = Some(Surround::Change(Some(c))),
            into @ (Surround::Change(Some(_)) | Surround::Add(_)) => {
                if matches!(c, 't' | '<') {
                    self.surround = Some(Surround::Tag(Box::new(into), String::new()));
                    return;
                }
                match pair(c) {
                    Some(halves) => self.surround_with(doc, into, halves),
                    None => {
                        self.failed = true;
                        self.reset();
                    }
                }
            }
            Surround::Tag(..) => {}
        }
    }

    /// Puts `halves` in place of the pair `cs` named, or around the text
    /// of `ys`.
    fn surround_with(&mut self, doc: &mut DocumentState, into: Surround, halves: (String, String)) {
        match into {
            Surround::Change(Some(old)) => {
                let n = self.count.take().unwrap_or(1);
                let Some((open, close)) = around(doc, self.cursor, old, n) else {
                    self.failed = true;
                    return self.reset();
                };
                self.replace_halves(doc, open, close, (&halves.0, &halves.1));
            }
            Surround::Add(target) => {
                let text = doc.text().as_str();
                let (open, close, lines) = match target {
                    Target::Chars(r) => {
                        // Blanks at the ends of the text stay outside.
                        let r = r.start.min(text.len())..r.end.min(text.len());
                        let t = &text[r.clone()];
                        let lead = t.len() - t.trim_start().len();
                        let end = r.start + t.trim_end().len().max(lead);
                        (r.start + lead, end, None)
                    }
                    // Lines: the halves on lines of their own around them,
                    // indented as the first.
                    Target::Lines(l1, l2) => {
                        let indent =
                            text[line_start(doc, l1)..first_non_blank(doc, l1)].to_string();
                        (line_start(doc, l1), line_end(doc, l2), Some(indent))
                    }
                    Target::Block { .. } => {
                        self.failed = true;
                        return self.reset();
                    }
                };
                let (o, c) = match &lines {
                    Some(indent) => (
                        format!("{indent}{}\n", halves.0.trim_end()),
                        format!("\n{indent}{}", halves.1.trim_start()),
                    ),
                    None => halves,
                };
                doc.selection = Selection::caret(open);
                // The opening half first: an empty text gets `()`.
                let mut tx = Transaction::new("Surround");
                let _ = tx.replace(open..open, o);
                let _ = tx.replace(close..close, c);
                let at = if lines.is_some() {
                    open + lines.as_ref().map_or(0, String::len)
                } else {
                    open
                };
                doc.apply(
                    &tx.select(Selection::caret(at)),
                    ChangeKind::Command,
                    Instant::now(),
                );
                self.cursor = at;
            }
            Surround::Delete | Surround::Change(None) | Surround::Tag(..) => {}
        }
        self.changed();
        self.reset();
    }

    /// The halves of a pair, `open` and `close`, replaced by `new`; the
    /// cursor where the pair began.
    fn replace_halves(
        &mut self,
        doc: &mut DocumentState,
        open: Range<usize>,
        close: Range<usize>,
        new: (&str, &str),
    ) {
        // Undo puts the cursor back where the pair began, as in Vim.
        doc.selection = Selection::caret(open.start);
        let mut tx = Transaction::new("Surround");
        let _ = tx.replace(close, new.1);
        let _ = tx.replace(open.clone(), new.0);
        doc.apply(
            &tx.select(Selection::caret(open.start)),
            ChangeKind::Command,
            Instant::now(),
        );
        self.cursor = open.start;
    }
}
