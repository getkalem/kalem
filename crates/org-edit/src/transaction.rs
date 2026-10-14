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

/// An error building a transaction: an edit overlapping another, or a
/// range that ends before it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlapError {
    /// The rejected range.
    pub range: Range<usize>,
}

impl std::fmt::Display for OverlapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Range { start, end } = self.range;
        if start > end {
            write!(f, "edit {start}..{end} ends before it starts")
        } else {
            write!(f, "edit {start}..{end} overlaps another edit")
        }
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
    /// order they were added), and a range must not end before it starts.
    /// A transaction an [`edit`](Transaction::edit) broke takes no more:
    /// it changes nothing rather than half of what it meant to.
    pub fn replace(
        &mut self,
        range: Range<usize>,
        insert: impl Into<String>,
    ) -> Result<&mut Self, OverlapError> {
        if self.broken {
            return Ok(self);
        }
        if range.start > range.end {
            return Err(OverlapError { range });
        }
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
    /// overlap after all, or end before it starts, a bug: debug builds and
    /// tests stop on it, and a release drops every edit of the
    /// transaction, so the command changes nothing instead of ending the
    /// program or applying half of itself.
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

    /// Whether the transaction can be applied to `text`: its edits in
    /// order and apart, each within the text and on character
    /// boundaries. One made for another text (a server's answer to an
    /// older version, a plugin's mistake, an undo step the text no longer
    /// matches) is refused by its callers rather than applied in the
    /// wrong place, or ending the program on a slice of the text.
    pub fn fits(&self, text: &str) -> bool {
        let mut at = 0;
        self.edits.iter().all(|e| {
            let r = &e.range;
            let ok = at <= r.start
                && r.start <= r.end
                && r.end <= text.len()
                && text.is_char_boundary(r.start)
                && text.is_char_boundary(r.end);
            at = r.end;
            ok
        })
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

    /// [`Transaction::map`] for many positions: each in a time that grows
    /// with the logarithm of the edits rather than with their number, so
    /// that the cursors, marks and line marks of a transaction with an
    /// edit for each of them (typing with many cursors) are not mapped in
    /// a time that grows with its square.
    pub fn mapper(&self) -> Mapper<'_> {
        let mut shifts = Vec::with_capacity(self.edits.len() + 1);
        let mut shift = 0isize;
        shifts.push(shift);
        for e in &self.edits {
            shift += e.insert.len() as isize - e.range.len() as isize;
            shifts.push(shift);
        }
        Mapper {
            edits: &self.edits,
            shifts,
        }
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

/// Maps positions through a transaction's edits ([`Transaction::mapper`]).
#[derive(Debug)]
pub struct Mapper<'a> {
    edits: &'a [Edit],
    /// How far the edits before each one, and all of them, move the text.
    shifts: Vec<isize>,
}

impl Mapper<'_> {
    /// Where `pos` goes, as [`Transaction::map`] has it.
    pub fn map(&self, pos: usize, assoc: Assoc) -> usize {
        let at = |i: usize, p: usize| (p as isize + self.shifts[i]) as usize;
        // The edits that start before `pos`; the last of them may hold it.
        let k = self.edits.partition_point(|e| e.range.start < pos);
        if let Some(e) = k.checked_sub(1).map(|i| &self.edits[i])
            && pos < e.range.end
        {
            let start = at(k - 1, e.range.start);
            return match assoc {
                Assoc::Before => start,
                Assoc::After => start + e.insert.len(),
            };
        }
        // Those that start at it: insertions there go before it or after
        // it, and a replacement there holds it.
        let mut i = k;
        while let Some(e) = self.edits.get(i).filter(|e| e.range.start == pos) {
            if !e.range.is_empty() {
                return match assoc {
                    Assoc::Before => at(i, pos),
                    Assoc::After => at(i, pos) + e.insert.len(),
                };
            }
            if assoc == Assoc::Before {
                break;
            }
            i += 1;
        }
        at(i, pos)
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

    #[test]
    fn reversed_ranges_and_broken_transactions() {
        // A range that ends before it starts is refused, not a crash.
        let mut t = Transaction::new("t");
        let e = t.replace(Range { start: 5, end: 3 }, "x").unwrap_err();
        assert_eq!(e.to_string(), "edit 5..3 ends before it starts");
        assert!(t.edits.is_empty());
        // A transaction an edit broke takes no more, whichever way they
        // come: it changes nothing rather than half of what it meant to.
        t.broken = true;
        t.replace(0..1, "a").unwrap();
        t.insert(2, "b").unwrap();
        t.delete(3..4).unwrap();
        t.edit(0..1, "c");
        assert!(t.edits.is_empty());
    }

    #[test]
    fn fits_only_a_text_it_was_made_for() {
        let mut t = Transaction::new("t");
        t.replace(1..3, "x").unwrap();
        assert!(t.fits("abc"));
        // Past the end, or inside a character.
        assert!(!t.fits("ab"));
        assert!(!t.fits("éa"));
        // Out of order, as a list pushed by hand may be.
        t.edits.push(Edit {
            range: 0..1,
            insert: String::new(),
        });
        assert!(!t.fits("abcdef"));
    }

    #[test]
    fn the_mapper_maps_as_map() {
        let mut t = Transaction::new("t");
        t.insert(0, "<").unwrap();
        t.insert(3, "ab").unwrap();
        t.insert(3, "c").unwrap();
        t.replace(3..6, "XY").unwrap();
        t.delete(8..10).unwrap();
        t.replace(12..13, "long text").unwrap();
        t.insert(20, ">").unwrap();
        let m = t.mapper();
        for pos in 0..=24 {
            for assoc in [Assoc::Before, Assoc::After] {
                assert_eq!(m.map(pos, assoc), t.map(pos, assoc), "{pos} {assoc:?}");
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn the_mapper_maps_as_map_whatever_the_edits(
            ops in proptest::collection::vec((0usize..40, 0usize..4, 0usize..3), 0..12),
        ) {
            let mut t = Transaction::new("t");
            for (at, len, n) in ops {
                let _ = t.replace(at..at + len, "x".repeat(n));
            }
            let m = t.mapper();
            for pos in 0..50 {
                for assoc in [Assoc::Before, Assoc::After] {
                    proptest::prop_assert_eq!(m.map(pos, assoc), t.map(pos, assoc));
                }
            }
        }
    }
}
