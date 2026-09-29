//! Multiple cursors and column selection (T2.7a.2): the selections beside
//! the primary one ([`DocumentState::extra`]), how they are made (a cursor
//! above or below, the next occurrence of the selection, all of them, a
//! column), and typing, deleting and pasting at all of them as one undo
//! step.

use std::time::Instant;

use org_edit::{Assoc, ChangeKind, Selection, Transaction};

use crate::document::DocumentState;

fn ordered(s: Selection) -> (usize, usize) {
    (s.anchor.min(s.head), s.anchor.max(s.head))
}

impl DocumentState {
    /// Every selection, in text order, and the index of the primary one.
    pub fn cursors(&self) -> (Vec<Selection>, usize) {
        let mut all = self.extra.clone();
        all.push(self.selection);
        all.sort_by_key(|s| ordered(*s));
        let i = all.iter().position(|s| *s == self.selection).unwrap_or(0);
        (all, i)
    }

    /// Sets the extra selections: sorted, without the primary one, and
    /// merged where they touch.
    pub fn set_extra(&mut self, extra: Vec<Selection>) {
        let len = self.text().len();
        let primary = self.selection;
        let mut all: Vec<Selection> = extra
            .into_iter()
            .map(|s| Selection {
                anchor: s.anchor.min(len),
                head: s.head.min(len),
            })
            .collect();
        all.push(primary);
        all.sort_by_key(|s| ordered(*s));
        let mut merged: Vec<Selection> = Vec::new();
        for s in all {
            match merged.last_mut() {
                Some(last) if ordered(s).0 < ordered(*last).1 || ordered(s) == ordered(*last) => {
                    // Overlapping: one selection covering both, the
                    // primary's direction kept when it is one of them.
                    let (a, b) = (ordered(*last).0, ordered(s).1.max(ordered(*last).1));
                    let keep = if s == primary { s } else { *last };
                    *last = if keep.head >= keep.anchor {
                        Selection { anchor: a, head: b }
                    } else {
                        Selection { anchor: b, head: a }
                    };
                    if keep == primary {
                        self.selection = *last;
                    }
                }
                _ => merged.push(s),
            }
        }
        let primary = self.selection;
        self.extra = merged
            .into_iter()
            .filter(|s| *s != primary && ordered(*s) != ordered(primary))
            .collect();
    }

    /// Leaves only the primary cursor.
    pub fn clear_extra(&mut self) {
        self.extra.clear();
    }

    /// Sets all selections at once; `primary` indexes `all`.
    pub fn set_cursors(&mut self, all: Vec<Selection>, primary: usize) {
        let Some(&p) = all.get(primary) else { return };
        self.selection = p;
        let extra = all
            .into_iter()
            .enumerate()
            .filter(|(i, _)| *i != primary)
            .map(|(_, s)| s)
            .collect();
        self.set_extra(extra);
    }

    /// Replaces each selection with what `text` gives for it (its index in
    /// text order), as one undo step; the cursors end after the new text.
    pub fn insert_at_cursors(&mut self, text: impl Fn(usize) -> String, now: Instant) {
        let (all, primary) = self.cursors();
        let mut tx = Transaction::new("Typing");
        for (i, s) in all.iter().enumerate() {
            let (a, b) = ordered(*s);
            if tx.replace(a..b, text(i)).is_err() {
                return;
            }
        }
        let after: Vec<Selection> = all
            .iter()
            .map(|s| Selection::caret(tx.map(ordered(*s).1, Assoc::After)))
            .collect();
        self.apply_at_cursors(tx, after, primary, now);
    }

    /// Deletes each selection, or the grapheme before (`forward`: after)
    /// each cursor, as one undo step.
    pub fn delete_at_cursors(&mut self, forward: bool, now: Instant) {
        let (all, primary) = self.cursors();
        let len = self.text().len();
        let mut tx = Transaction::new("Delete");
        for s in &all {
            let (a, b) = ordered(*s);
            let r = if a != b {
                a..b
            } else if forward && b < len {
                b..self.grapheme_after(b)
            } else if !forward && a > 0 {
                self.grapheme_before(a)..a
            } else {
                continue;
            };
            // Two cursors reaching into the same character: once.
            let _ = tx.replace(r, "");
        }
        let after: Vec<Selection> = all
            .iter()
            .map(|s| Selection::caret(tx.map(ordered(*s).0, Assoc::Before)))
            .collect();
        self.apply_at_cursors(tx, after, primary, now);
    }

    /// Pastes `text` at every cursor: one line at each when it has as many
    /// lines as there are cursors (what copying them gave), else all of it
    /// at each.
    pub fn paste_at_cursors(&mut self, text: &str, now: Instant) {
        let n = self.extra.len() + 1;
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines: Vec<&str> = body.split('\n').collect();
        if lines.len() == n && n > 1 {
            let lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            self.insert_at_cursors(|i| lines[i].clone(), now);
        } else {
            let t = text.to_string();
            self.insert_at_cursors(|_| t.clone(), now);
        }
    }

    /// The text of every selection, one a line (Copy with several
    /// cursors); `None` when all are empty.
    pub fn cursors_text(&self) -> Option<String> {
        let (all, _) = self.cursors();
        let text = self.text().as_str();
        let parts: Vec<&str> = all
            .iter()
            .map(|s| {
                let (a, b) = ordered(*s);
                &text[a..b]
            })
            .collect();
        parts
            .iter()
            .any(|p| !p.is_empty())
            .then(|| parts.join("\n"))
    }

    /// What Copy copies: the selection, or with several cursors the text
    /// of each, one a line.
    pub fn copy_text(&self) -> Option<String> {
        if self.extra.is_empty() {
            self.selected_text().map(str::to_string)
        } else {
            self.cursors_text()
        }
    }

    /// What Cut removes after copying: the selection, or every selection.
    pub fn cut_selections(&mut self, now: Instant) {
        if self.extra.is_empty() {
            let _ = self.delete_backward(now);
        } else {
            self.insert_at_cursors(|_| String::new(), now);
        }
    }

    /// A cursor at `pos` besides the ones there are (Alt+click); a click
    /// on an extra cursor removes it.
    pub fn toggle_cursor_at(&mut self, pos: usize) {
        if let Some(i) = self
            .extra
            .iter()
            .position(|s| s.head == pos && s.anchor == pos)
        {
            self.extra.remove(i);
            return;
        }
        let mut extra = self.extra.clone();
        extra.push(self.selection);
        self.selection = Selection::caret(pos);
        self.set_extra(extra);
    }

    fn apply_at_cursors(
        &mut self,
        tx: Transaction,
        after: Vec<Selection>,
        primary: usize,
        now: Instant,
    ) {
        if tx.is_empty() {
            return;
        }
        let tx = tx.select(after[primary]);
        self.extra.clear();
        self.apply(&tx, ChangeKind::Typing, now);
        self.set_cursors(after, primary);
    }

    /// A cursor on the line below the last one (`up`: above the first),
    /// at the same column, or as near as the line allows.
    pub fn add_cursor_vertical(&mut self, up: bool) -> bool {
        let (all, _) = self.cursors();
        let from = if up { all[0] } else { all[all.len() - 1] };
        let text = self.text();
        let line = text.line_of(from.head);
        let col = text.as_str()[text.line_start(line)..from.head]
            .chars()
            .count();
        let target = if up {
            match line.checked_sub(1) {
                Some(l) => l,
                None => return false,
            }
        } else if line + 1 < text.line_count() {
            line + 1
        } else {
            return false;
        };
        let pos = column_pos(text, target, col);
        let mut extra = self.extra.clone();
        extra.push(self.selection);
        self.selection = Selection::caret(pos);
        self.set_extra(extra);
        true
    }

    /// Column selection one line further down (`up`: up): the columns of
    /// the primary selection on the next line too, the new one primary.
    pub fn extend_column(&mut self, up: bool) -> bool {
        let (all, _) = self.cursors();
        let from = if up { all[0] } else { all[all.len() - 1] };
        let text = self.text();
        let line = text.line_of(from.head);
        let start = text.line_start(line);
        let col_of = |p: usize| {
            let l = text.line_of(p);
            text.as_str()[text.line_start(l)..p].chars().count()
        };
        let (ca, ch) = (col_of(from.anchor.max(start)), col_of(from.head));
        let target = if up {
            match line.checked_sub(1) {
                Some(l) => l,
                None => return false,
            }
        } else if line + 1 < text.line_count() {
            line + 1
        } else {
            return false;
        };
        let sel = Selection {
            anchor: column_pos(text, target, ca),
            head: column_pos(text, target, ch),
        };
        let mut extra = self.extra.clone();
        extra.push(self.selection);
        self.selection = sel;
        self.set_extra(extra);
        true
    }

    /// Selects the word at the cursor when nothing is selected; else adds
    /// a selection of the next occurrence of the primary selection's text
    /// after the last selection (from the start again at the end).
    pub fn add_next_occurrence(&mut self) -> bool {
        let s = self.selection;
        let text = self.text().as_str();
        if s.anchor == s.head {
            let Some(w) = word_at(text, s.head) else {
                return false;
            };
            self.selection = Selection {
                anchor: w.start,
                head: w.end,
            };
            return true;
        }
        let (a, b) = ordered(s);
        let needle = text[a..b].to_string();
        let (all, _) = self.cursors();
        let last = ordered(all[all.len() - 1]).1;
        let taken = |p: usize| all.iter().any(|s| ordered(*s).0 == p);
        let found = text[last..]
            .match_indices(needle.as_str())
            .map(|(i, _)| last + i)
            .chain(text[..last].match_indices(needle.as_str()).map(|(i, _)| i))
            .find(|&p| !taken(p));
        let Some(p) = found else { return false };
        let mut extra = self.extra.clone();
        extra.push(self.selection);
        self.selection = Selection {
            anchor: p,
            head: p + needle.len(),
        };
        self.set_extra(extra);
        true
    }

    /// A selection at every occurrence of the primary selection's text (of
    /// the word at the cursor when nothing is selected).
    pub fn select_all_occurrences(&mut self) -> usize {
        if self.selection.anchor == self.selection.head && !self.add_next_occurrence() {
            return 0;
        }
        let (a, b) = ordered(self.selection);
        let text = self.text().as_str();
        let needle = text[a..b].to_string();
        let all: Vec<Selection> = text
            .match_indices(needle.as_str())
            .map(|(i, _)| Selection {
                anchor: i,
                head: i + needle.len(),
            })
            .collect();
        let n = all.len();
        let primary = all.iter().position(|s| s.anchor == a).unwrap_or(0);
        self.set_cursors(all, primary);
        n
    }
}

/// The position of character column `col` on line `line`, or the line's
/// end when it is shorter.
fn column_pos(text: &crate::Text, line: usize, col: usize) -> usize {
    let r = text.line_range(line);
    let s = &text.as_str()[r.clone()];
    s.char_indices()
        .nth(col)
        .map_or(r.end, |(i, _)| r.start + i)
}

/// The word around `pos`.
fn word_at(text: &str, pos: usize) -> Option<std::ops::Range<usize>> {
    use unicode_segmentation::UnicodeSegmentation;
    let line_start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    text[line_start..line_end]
        .split_word_bound_indices()
        .map(|(i, w)| (line_start + i)..(line_start + i + w.len()))
        .find(|r| {
            r.start <= pos && pos <= r.end && text[r.clone()].chars().any(char::is_alphanumeric)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LineEnding, Metadata};
    use crate::mode::DocumentMode;

    fn doc(text: &str) -> DocumentState {
        DocumentState::new(
            text,
            Metadata {
                path: None,
                mode: DocumentMode::Text { language: None },
                line_ending: LineEnding::Lf,
                bom: false,
                encoding: encoding_rs::UTF_8,
            },
            std::sync::Arc::new(org_model::Settings::default()),
        )
    }

    #[test]
    fn typing_at_several_cursors() {
        let now = Instant::now();
        let mut d = doc("one\ntwo\nthree\n");
        d.move_cursor(0, false);
        assert!(d.add_cursor_vertical(false));
        assert!(d.add_cursor_vertical(false));
        assert_eq!(d.extra.len(), 2);
        d.insert_text("- ", now);
        assert_eq!(d.text().as_str(), "- one\n- two\n- three\n");
        d.break_undo_group();
        d.delete_backward(now);
        assert_eq!(d.text().as_str(), "-one\n-two\n-three\n");
        // One undo step.
        d.undo();
        assert_eq!(d.text().as_str(), "- one\n- two\n- three\n");
        assert!(d.extra.is_empty());
    }

    #[test]
    fn occurrences_and_columns() {
        let now = Instant::now();
        let mut d = doc("cat dog cat bird cat\n");
        d.move_cursor(1, false);
        assert!(d.add_next_occurrence());
        assert_eq!(d.selected_text(), Some("cat"));
        assert!(d.add_next_occurrence());
        assert_eq!(d.extra.len(), 1);
        d.insert_text("cow", now);
        assert_eq!(d.text().as_str(), "cow dog cow bird cat\n");
        d.clear_extra();
        d.move_cursor(9, false);
        assert_eq!(d.select_all_occurrences(), 2);
        assert_eq!(d.cursors_text().as_deref(), Some("cow\ncow"));
        // Pasting as many lines as cursors: one at each.
        d.paste_at_cursors("a\nb", now);
        assert_eq!(d.text().as_str(), "a dog b bird cat\n");
        // A column: the same columns on the next lines.
        let mut d = doc("abcdef\nghijkl\nmn\n");
        d.selection = Selection { anchor: 1, head: 3 };
        assert!(d.extend_column(false));
        assert!(d.extend_column(false));
        assert_eq!(d.cursors_text().as_deref(), Some("bc\nhi\nn"));
        d.delete_at_cursors(false, now);
        assert_eq!(d.text().as_str(), "adef\ngjkl\nm\n");
    }
}
