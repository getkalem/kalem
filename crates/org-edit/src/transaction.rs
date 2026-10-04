//! Transactions: a set of non-overlapping replacements applied at once,
//! with their inverse for undo.

use std::ops::Range;

/// One replacement, in the coordinates of the text before the transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The replaced range (byte offsets).
    pub range: Range<usize>,
    /// The new text.
    pub insert: String,
}

/// A selection: the cursor is `head`; `anchor == head` for a caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: usize,
    /// Where the cursor is.
    pub head: usize,
}

impl Selection {
    /// A caret at `pos`.
    pub fn caret(pos: usize) -> Selection {
        Selection {
            anchor: pos,
            head: pos,
        }
    }
}

/// Which way a position at the edge of an insertion moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    /// Stay before text inserted at the position.
    Before,
    /// Move after text inserted at the position.
    After,
}

/// Replacements applied together, as one undo step.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transaction {
    /// Sorted, non-overlapping replacements.
    pub edits: Vec<Edit>,
    /// The selection after the transaction, if the command sets one.
    pub selection_after: Option<Selection>,
    /// A label for the undo menu.
    pub label: String,
    /// An [`edit`](Transaction::edit) overlapped another: the transaction
    /// changes nothing rather than half of what it meant to.
    pub broken: bool,
}

/// An error building a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlapError {
    /// The rejected range.
    pub range: Range<usize>,
}

impl std::fmt::Display for OverlapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "edit {}..{} overlaps another edit",
            self.range.start, self.range.end
        )
    }
}

impl std::error::Error for OverlapError {}

impl Transaction {
    /// An empty transaction.
    pub fn new(label: impl Into<String>) -> Transaction {
        Transaction {
            label: label.into(),
            ..Default::default()
        }
    }

    /// Adds a replacement. Replacements may come in any order but must not
    /// overlap (two insertions at the same position are applied in the
    /// order they were added).
    pub fn replace(
        &mut self,
        range: Range<usize>,
        insert: impl Into<String>,
    ) -> Result<&mut Self, OverlapError> {
        assert!(range.start <= range.end, "reversed range");
        // Sorted by start; at one position, insertions come before a
        // replacement, and equal keys keep their order.
        let key = |r: &Range<usize>| (r.start, !r.is_empty());
        let k = key(&range);
        let i = self.edits.partition_point(|e| key(&e.range) <= k);
        if let Some(prev) = i.checked_sub(1).map(|j| &self.edits[j])
            && prev.range.end > range.start
        {
            return Err(OverlapError { range });
        }
        if let Some(next) = self.edits.get(i)
            && next.range.start < range.end
        {
            return Err(OverlapError { range });
        }
        self.edits.insert(
            i,
            Edit {
                range,
                insert: insert.into(),
            },
        );
        Ok(self)
    }

    /// Adds a replacement that the caller knows overlaps no other (the
    /// only one, or ranges taken apart from one another). Should it
    /// overlap after all, a bug: debug builds and tests stop on it, and a
    /// release drops every edit of the transaction, so the command
    /// changes nothing instead of ending the program or applying half of
    /// itself.
    pub fn edit(&mut self, range: Range<usize>, insert: impl Into<String>) -> &mut Self {
        if self.broken {
            return self;
        }
        if let Err(e) = self.replace(range, insert) {
            debug_assert!(false, "{e}");
            self.edits.clear();
            self.selection_after = None;
            self.broken = true;
        }
        self
    }

    /// Adds an insertion.
    pub fn insert(
        &mut self,
        at: usize,
        text: impl Into<String>,
    ) -> Result<&mut Self, OverlapError> {
        self.replace(at..at, text)
    }

    /// Adds a deletion.
    pub fn delete(&mut self, range: Range<usize>) -> Result<&mut Self, OverlapError> {
        self.replace(range, "")
    }

    /// Sets the selection after the transaction.
    pub fn select(mut self, selection: Selection) -> Transaction {
        if !self.broken {
            self.selection_after = Some(selection);
        }
        self
    }

    /// Whether the transaction changes nothing.
    pub fn is_empty(&self) -> bool {
        self.edits
            .iter()
            .all(|e| e.range.is_empty() && e.insert.is_empty())
    }

    /// The text after the transaction.
    pub fn apply(&self, text: &str) -> String {
        let delta: isize = self
            .edits
            .iter()
            .map(|e| e.insert.len() as isize - e.range.len() as isize)
            .sum();
        let mut out = String::with_capacity((text.len() as isize + delta).max(0) as usize);
        let mut pos = 0;
        for e in &self.edits {
            out.push_str(&text[pos..e.range.start]);
            out.push_str(&e.insert);
            pos = e.range.end;
        }
        out.push_str(&text[pos..]);
        out
    }

    /// The transaction that undoes this one, given the text before it.
    pub fn invert(&self, before: &str) -> Transaction {
        let mut shift: isize = 0;
        let edits = self
            .edits
            .iter()
            .map(|e| {
                let start = (e.range.start as isize + shift) as usize;
                shift += e.insert.len() as isize - e.range.len() as isize;
                Edit {
                    range: start..start + e.insert.len(),
                    insert: before[e.range.clone()].to_string(),
                }
            })
            .collect();
        Transaction {
            edits,
            selection_after: None,
            label: self.label.clone(),
            broken: false,
        }
    }

    /// Where `pos` (before the transaction) is afterwards. A position
    /// inside a replaced range goes to the start or the end of its
    /// replacement.
    pub fn map(&self, pos: usize, assoc: Assoc) -> usize {
        let mut shift: isize = 0;
        for e in &self.edits {
            let (s, t) = (e.range.start, e.range.end);
            if pos < s {
                break;
            }
            if s == t {
                if pos == s && assoc == Assoc::Before {
                    break;
                }
                shift += e.insert.len() as isize;
                continue;
            }
            if pos < t {
                let ns = (s as isize + shift) as usize;
                return match assoc {
                    Assoc::Before => ns,
                    Assoc::After => ns + e.insert.len(),
                };
            }
            shift += e.insert.len() as isize - (t - s) as isize;
        }
        (pos as isize + shift) as usize
    }

    /// The smallest single replacement equivalent to this transaction, for
    /// an incremental reparse: the range it covers in the text before, and
    /// the new text of that range.
    pub fn covering_edit(&self, before: &str) -> Option<org_syntax::TextEdit> {
        let first = self.edits.first()?;
        let last = self.edits.last()?;
        let (start, end) = (first.range.start, last.range.end);
        let mut insert = String::new();
        let mut pos = start;
        for e in &self.edits {
            insert.push_str(&before[pos..e.range.start]);
            insert.push_str(&e.insert);
            pos = e.range.end;
        }
        Some(org_syntax::TextEdit {
            range: org_syntax::TextRange::new((start as u32).into(), (end as u32).into()),
            insert,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_invert_map() {
        let text = "hello world";
        let mut t = Transaction::new("t");
        t.replace(6..11, "there").unwrap();
        t.insert(0, ">> ").unwrap();
        assert!(t.replace(8..9, "x").is_err());
        let after = t.apply(text);
        assert_eq!(after, ">> hello there");
        assert_eq!(t.invert(text).apply(&after), text);
        assert_eq!(t.map(0, Assoc::Before), 0);
        assert_eq!(t.map(0, Assoc::After), 3);
        assert_eq!(t.map(5, Assoc::After), 8);
        assert_eq!(t.map(8, Assoc::Before), 9);
        assert_eq!(t.map(8, Assoc::After), 14);
        let c = t.covering_edit(text).unwrap();
        assert_eq!(c.apply(text), after);
    }
}
