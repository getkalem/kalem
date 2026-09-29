//! Everyday line commands (T2.7a.6): duplicate, move, join and sort the
//! lines of the selection, trim trailing whitespace, select the word at
//! the cursor, and grow or shrink the selection by syntax.

use org_edit::{Selection, Transaction};

/// The byte range of the lines a selection covers, line feed of the last
/// excluded: a selection ending at the start of a line leaves it out.
pub fn covered(text: &str, sel: Selection) -> std::ops::Range<usize> {
    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let start = text[..a].rfind('\n').map_or(0, |i| i + 1);
    let b = if b > a && b > start && text[..b].ends_with('\n') {
        b - 1
    } else {
        b
    };
    let end = text[b..].find('\n').map_or(text.len(), |i| b + i);
    start..end
}

/// Duplicates the lines of the selection below them; the selection moves
/// to the copy.
pub fn duplicate(text: &str, sel: Selection) -> Transaction {
    let r = covered(text, sel);
    let block = &text[r.clone()];
    let mut tx = Transaction::new("Duplicate Lines");
    tx.replace(r.end..r.end, format!("\n{block}"))
        .expect("one edit");
    let shift = block.len() + 1;
    tx.select(Selection {
        anchor: sel.anchor + shift,
        head: sel.head + shift,
    })
}

/// Moves the lines of the selection one line up (or down), the selection
/// with them; `None` at the start (or end) of the text.
pub fn move_lines(text: &str, sel: Selection, up: bool) -> Option<Transaction> {
    let r = covered(text, sel);
    let block = &text[r.clone()];
    let mut tx = Transaction::new(if up {
        "Move Lines Up"
    } else {
        "Move Lines Down"
    });
    if up {
        if r.start == 0 {
            return None;
        }
        let prev = text[..r.start - 1].rfind('\n').map_or(0, |i| i + 1);
        let above = &text[prev..r.start - 1];
        tx.replace(prev..r.end, format!("{block}\n{above}"))
            .expect("one edit");
        let shift = r.start - prev;
        Some(tx.select(Selection {
            anchor: sel.anchor - shift,
            head: sel.head - shift,
        }))
    } else {
        if r.end >= text.len() {
            return None;
        }
        let next_end = text[r.end + 1..]
            .find('\n')
            .map_or(text.len(), |i| r.end + 1 + i);
        let below = &text[r.end + 1..next_end];
        // The last line of a text without a final line feed.
        if below.is_empty() && next_end == text.len() {
            return None;
        }
        tx.replace(r.start..next_end, format!("{below}\n{block}"))
            .expect("one edit");
        let shift = below.len() + 1;
        Some(tx.select(Selection {
            anchor: sel.anchor + shift,
            head: sel.head + shift,
        }))
    }
}

/// Joins the lines of the selection into one (the line at the cursor with
/// the next when the selection is on one line), each line's indentation
/// and the blanks at the joins becoming one space.
pub fn join(text: &str, sel: Selection) -> Option<Transaction> {
    let mut r = covered(text, sel);
    if !text[r.clone()].contains('\n') {
        if r.end >= text.len() {
            return None;
        }
        r.end = text[r.end + 1..]
            .find('\n')
            .map_or(text.len(), |i| r.end + 1 + i);
    }
    let mut tx = Transaction::new("Join Lines");
    let mut cursor = sel.head;
    let mut at = r.start;
    while let Some(i) = text[at..r.end].find('\n') {
        let nl = at + i;
        let before = text[..nl].trim_end_matches([' ', '\t']).len().max(r.start);
        let after = nl
            + 1
            + text[nl + 1..r.end]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();
        let sep = if after >= r.end || before == r.start {
            ""
        } else {
            " "
        };
        tx.replace(before..after, sep).expect("separate joins");
        cursor = before;
        at = after.max(nl + 1);
    }
    let cursor = tx.map(cursor, org_edit::Assoc::After);
    Some(tx.select(Selection::caret(cursor)))
}

/// Sorts the lines of the selection (by their text; `reverse` for
/// descending), keeping them selected.
pub fn sort(text: &str, sel: Selection, reverse: bool) -> Option<Transaction> {
    let r = covered(text, sel);
    let mut lines: Vec<&str> = text[r.clone()].split('\n').collect();
    if lines.len() < 2 {
        return None;
    }
    lines.sort();
    if reverse {
        lines.reverse();
    }
    let sorted = lines.join("\n");
    let mut tx = Transaction::new("Sort Lines");
    tx.replace(r.clone(), sorted).expect("one edit");
    Some(tx.select(Selection {
        anchor: r.start,
        head: r.end,
    }))
}

/// Removes the blanks at the ends of lines; `None` when there are none.
pub fn trim_trailing(text: &str) -> Option<Transaction> {
    let mut tx = Transaction::new("Trim Trailing Whitespace");
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let kept = body.trim_end_matches([' ', '\t']).len();
        if kept < body.len() {
            tx.replace(at + kept..at + body.len(), "")
                .expect("separate lines");
        }
        at += line.len();
    }
    (!tx.is_empty()).then_some(tx)
}

/// The word at `pos` (letters, digits and `_`).
pub fn word_at(text: &str, pos: usize) -> Option<std::ops::Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..pos]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(pos, |(i, _)| i);
    let end = text[pos..]
        .char_indices()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(pos, |(i, c)| pos + i + c.len_utf8());
    (start < end).then_some(start..end)
}

/// The ranges around `a..b` that a bracket pair makes: the inside, then
/// with the brackets, from the innermost out.
fn bracket_ranges(text: &str, a: usize, b: usize) -> Vec<std::ops::Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut lo, mut hi) = (a, b);
    for _ in 0..16 {
        // The nearest unmatched opening bracket before `lo`.
        let mut depth = 0i32;
        let mut open = None;
        let floor = lo.saturating_sub(64 * 1024);
        for i in (floor..lo).rev() {
            match bytes[i] {
                b')' | b']' | b'}' => depth += 1,
                b'(' | b'[' | b'{' if depth == 0 => {
                    open = Some(i);
                    break;
                }
                b'(' | b'[' | b'{' => depth -= 1,
                _ => {}
            }
        }
        let Some(o) = open else { break };
        let Some((_, c)) = crate::code::matching(text, o + 1).filter(|(x, _)| *x == o) else {
            break;
        };
        if c < hi {
            break;
        }
        out.push(o + 1..c);
        out.push(o..c + 1);
        lo = o;
        hi = c + 1;
    }
    out
}

/// The next larger selection around `sel`: the word, the inside of the
/// brackets around it and the brackets, the element or object of an Org
/// document (`root`) or the line and the paragraph of plain text, and the
/// whole text.
pub fn expand(
    text: &str,
    root: Option<&org_syntax::SyntaxNode>,
    sel: Selection,
) -> Option<Selection> {
    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let mut candidates: Vec<std::ops::Range<usize>> = Vec::new();
    if let Some(w) = word_at(text, a) {
        candidates.push(w);
    }
    candidates.extend(bracket_ranges(text, a, b));
    let trim = |r: std::ops::Range<usize>| {
        let end = r.start
            + text[r.clone()]
                .trim_end_matches([' ', '\t', '\n', '\r'])
                .len();
        r.start..end.max(r.start)
    };
    match root {
        Some(root) if !text.is_empty() => {
            let at = org_syntax::TextSize::try_from(a.min(text.len() - 1)).ok()?;
            if let Some(tok) = root.token_at_offset(at).right_biased() {
                for n in tok.parent_ancestors() {
                    let r = n.text_range();
                    candidates.push(trim(usize::from(r.start())..usize::from(r.end())));
                }
            }
        }
        _ => {
            // The line, then the paragraph of lines without blank ones.
            let ls = text[..a].rfind('\n').map_or(0, |i| i + 1);
            let le = text[b..].find('\n').map_or(text.len(), |i| b + i);
            let first = text[ls..le].len() - text[ls..le].trim_start().len();
            candidates.push(ls + first..le);
            candidates.push(ls..le);
            let mut ps = ls;
            while ps > 0 {
                let prev = text[..ps - 1].rfind('\n').map_or(0, |i| i + 1);
                if text[prev..ps - 1].trim().is_empty() {
                    break;
                }
                ps = prev;
            }
            let mut pe = le;
            while pe < text.len() {
                let next = text[pe + 1..].find('\n').map_or(text.len(), |i| pe + 1 + i);
                if text[pe + 1..next].trim().is_empty() {
                    break;
                }
                pe = next;
            }
            candidates.push(ps..pe);
        }
    }
    candidates.push(0..text.len());
    candidates
        .into_iter()
        .filter(|r| r.start <= a && b <= r.end && r.len() > b - a)
        .min_by_key(|r| r.len())
        .map(|r| Selection {
            anchor: r.start,
            head: r.end,
        })
}

impl crate::DocumentState {
    /// Expand Selection: the next larger selection by syntax.
    pub fn expand_selection(&mut self) -> bool {
        let root = match (&self.meta.mode, self.parse()) {
            (crate::DocumentMode::Org, Some((p, _))) => Some(p.syntax()),
            _ => None,
        };
        let before = self.selection;
        let Some(after) = expand(self.text().as_str(), root.as_ref(), before) else {
            return false;
        };
        if self.expansions.last().is_some_and(|(_, a)| *a != before) {
            self.expansions.clear();
        }
        self.expansions.push((before, after));
        self.selection = after;
        true
    }

    /// Shrink Selection: back to the selection before the last Expand
    /// Selection.
    pub fn shrink_selection(&mut self) -> bool {
        match self.expansions.pop() {
            Some((before, after)) if after == self.selection => {
                self.selection = before;
                true
            }
            _ => {
                self.expansions.clear();
                false
            }
        }
    }

    /// What saving does first, as the settings say: the blanks at the ends
    /// of lines removed (`editor.trim_trailing_whitespace`).
    pub fn before_save(&mut self, config: &crate::settings::Config, now: std::time::Instant) {
        if config.bool("editor.trim_trailing_whitespace")
            && self.dired.is_none()
            && let Some(tx) = trim_trailing(self.text().as_str())
        {
            self.apply(&tx, org_edit::ChangeKind::Command, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, tx: Transaction) -> (String, Selection) {
        let s = tx.selection_after.unwrap_or(Selection::caret(0));
        (tx.apply(text), s)
    }

    #[test]
    fn line_commands() {
        let t = "a\nb\nc\n";
        let (d, s) = apply(t, duplicate(t, Selection::caret(2)));
        assert_eq!((d.as_str(), s.head), ("a\nb\nb\nc\n", 4));
        let (m, s) = apply(t, move_lines(t, Selection::caret(2), true).unwrap());
        assert_eq!((m.as_str(), s.head), ("b\na\nc\n", 0));
        let (m, s) = apply(t, move_lines(t, Selection::caret(2), false).unwrap());
        assert_eq!((m.as_str(), s.head), ("a\nc\nb\n", 4));
        assert!(move_lines(t, Selection::caret(0), true).is_none());
        assert!(move_lines("a\nb", Selection::caret(2), false).is_none());
        // Two selected lines move together.
        let (m, _) = apply(
            t,
            move_lines(t, Selection { anchor: 0, head: 3 }, false).unwrap(),
        );
        assert_eq!(m, "c\na\nb\n");
        let t = "one  \n   two\nthree\n";
        let (j, s) = apply(t, join(t, Selection::caret(1)).unwrap());
        assert_eq!((j.as_str(), s.head), ("one two\nthree\n", 4));
        let (j, _) = apply(
            t,
            join(
                t,
                Selection {
                    anchor: 0,
                    head: 17,
                },
            )
            .unwrap(),
        );
        assert_eq!(j, "one two three\n");
        let t = "pear\napple\nfig\n";
        let (o, _) = apply(
            t,
            sort(
                t,
                Selection {
                    anchor: 0,
                    head: 14,
                },
                false,
            )
            .unwrap(),
        );
        assert_eq!(o, "apple\nfig\npear\n");
        let (o, _) = apply(
            t,
            sort(
                t,
                Selection {
                    anchor: 0,
                    head: 14,
                },
                true,
            )
            .unwrap(),
        );
        assert_eq!(o, "pear\nfig\napple\n");
        let (w, _) = apply("a  \nb\t\r\nc", trim_trailing("a  \nb\t\r\nc").unwrap());
        assert_eq!(w, "a\nb\r\nc");
        assert!(trim_trailing("a\nb\n").is_none());
        assert_eq!(word_at("foo_bar baz", 3), Some(0..7));
        assert_eq!(word_at("a  b", 2), None);
    }

    #[test]
    fn expanding() {
        let t = "fn a() {\n    call(x, y);\n}\n\nnext\n";
        let x = t.find("x,").unwrap();
        let step = |s: Selection| expand(t, None, s).unwrap();
        let s = step(Selection::caret(x));
        assert_eq!(&t[s.anchor..s.head], "x");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "x, y");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "(x, y)");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "call(x, y);");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "    call(x, y);");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "\n    call(x, y);\n");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "{\n    call(x, y);\n}");
        // Org: by the syntax tree.
        let o = "* Head\nSome *bold word* here.\n";
        let root = org_syntax::parse(o).syntax();
        let w = o.find("word").unwrap();
        let s = expand(o, Some(&root), Selection::caret(w)).unwrap();
        assert_eq!(&o[s.anchor..s.head], "word");
        let s = expand(o, Some(&root), s).unwrap();
        assert_eq!(&o[s.anchor..s.head], "*bold word*");
        let s = expand(o, Some(&root), s).unwrap();
        assert_eq!(&o[s.anchor..s.head], "Some *bold word* here.");
    }
}
