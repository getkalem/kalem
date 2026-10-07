//! The text of a document: one contiguous `String`, so the parser reads it
//! without copies, with a line index kept up to date edit by edit.
//!
//! A rope would make edits O(log n) instead of a move of the text after the
//! edit, but the parser needs contiguous text, and moving a few megabytes
//! is well under a millisecond (`examples/text_timing.rs`), so the simple
//! layout wins (T1.3.1a).

use std::ops::Range;

/// A document's text and line index.
#[derive(Debug, Clone, Default)]
pub struct Text {
    text: String,
    /// Byte offsets of line starts; the first is 0.
    lines: Vec<usize>,
}

fn line_starts(s: &str, base: usize) -> impl Iterator<Item = usize> + '_ {
    s.match_indices('\n').map(move |(i, _)| base + i + 1)
}

impl Text {
    /// Text from a string.
    pub fn new(text: impl Into<String>) -> Text {
        let text = text.into();
        let mut lines = vec![0];
        lines.extend(line_starts(&text, 0));
        Text { text, lines }
    }

    /// The whole text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replaces `range` with `insert`, updating the line index for the
    /// affected lines only.
    pub fn replace(&mut self, range: Range<usize>, insert: &str) {
        let (a, b) = (range.start, range.end);
        let delta = insert.len() as isize - (b - a) as isize;
        // Line starts after `a` and up to `b` disappear; those after `b`
        // move; the insert brings its own.
        let first = self.lines.partition_point(|&l| l <= a);
        let last = self.lines.partition_point(|&l| l <= b);
        let new_lines: Vec<usize> = line_starts(insert, a).collect();
        let n_new = new_lines.len();
        self.lines.splice(first..last, new_lines);
        for l in &mut self.lines[first + n_new..] {
            *l = (*l as isize + delta) as usize;
        }
        self.text.replace_range(a..b, insert);
    }

    /// Applies `edits` (sorted, not overlapping, in offsets of the text as
    /// it is): one at a time when they are few, else the text and its line
    /// index made again in one pass. Each replacement moves the text after
    /// it, so one on every line of a large file (Replace All, the line
    /// endings changed, typing with many cursors) took seconds: 11 s for
    /// 100,000 lines.
    pub fn apply(&mut self, edits: &[org_edit::Edit]) {
        if edits.len() <= 8 {
            for e in edits.iter().rev() {
                self.replace(e.range.clone(), &e.insert);
            }
            return;
        }
        let added: usize = edits.iter().map(|e| e.insert.len()).sum();
        let removed: usize = edits.iter().map(|e| e.range.len()).sum();
        let mut out = String::with_capacity((self.text.len() + added).saturating_sub(removed));
        let mut at = 0;
        for e in edits {
            out.push_str(&self.text[at..e.range.start]);
            out.push_str(&e.insert);
            at = e.range.end;
        }
        out.push_str(&self.text[at..]);
        *self = Text::new(out);
    }

    /// The number of lines (a final line feed starts an empty last line).
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The start of line `line` (0-based).
    pub fn line_start(&self, line: usize) -> usize {
        self.lines[line.min(self.lines.len() - 1)]
    }

    /// The byte range of line `line`, without its line feed.
    pub fn line_range(&self, line: usize) -> Range<usize> {
        let s = self.line_start(line);
        let e = self.lines.get(line + 1).map_or(self.text.len(), |n| n - 1);
        s..e
    }

    /// The line containing `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        self.lines.partition_point(|&l| l <= offset) - 1
    }

    /// (line, column in characters) of `offset`.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset);
        let col = self.text[self.lines[line]..offset].chars().count();
        (line, col)
    }

    /// The offset of (line, column in characters), clamped to the line.
    pub fn offset(&self, line: usize, col: usize) -> usize {
        let r = self.line_range(line);
        self.text[r.clone()]
            .char_indices()
            .nth(col)
            .map_or(r.end, |(i, _)| r.start + i)
    }
}

/// How a text indents: with tabs, or with a number of spaces a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    /// Tabs.
    Tabs,
    /// Spaces, this many a level.
    Spaces(usize),
}

/// How `text` indents, from its lines: tabs if more lines start with a tab
/// than with spaces, else the most common step between the indentation of
/// a line and the next (2, 3, 4 or 8 spaces); `None` without indented
/// lines.
pub fn detect_indent(text: &str) -> Option<Indent> {
    let (mut tabs, mut spaces) = (0usize, 0usize);
    let mut steps = [0usize; 9];
    let mut prev = 0usize;
    // The first lines, and at most a megabyte (a very long line).
    let mut cut = text.len().min(1 << 20);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    for line in text[..cut].lines().take(10_000) {
        if line.trim().is_empty() {
            continue;
        }
        let n = line.len() - line.trim_start_matches(' ').len();
        if line.starts_with('\t') {
            tabs += 1;
        } else if n > 0 {
            spaces += 1;
        }
        if n > prev && n - prev <= 8 {
            steps[n - prev] += 1;
        }
        prev = n;
    }
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > spaces {
        return Some(Indent::Tabs);
    }
    let best = [2, 4, 3, 8]
        .into_iter()
        .max_by_key(|&w| steps[w])
        .filter(|&w| steps[w] > 0)
        .unwrap_or(4);
    Some(Indent::Spaces(best))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn indentation() {
        assert_eq!(
            detect_indent("a\n  b\n    c\n  d\n"),
            Some(Indent::Spaces(2))
        );
        assert_eq!(
            detect_indent("a\n    b\n        c\n"),
            Some(Indent::Spaces(4))
        );
        assert_eq!(detect_indent("a\n\tb\n\t\tc\n"), Some(Indent::Tabs));
        assert_eq!(detect_indent("a\nb\n"), None);
    }

    #[test]
    fn lines() {
        let mut t = Text::new("a\nbb\nccc");
        assert_eq!(t.line_count(), 3);
        assert_eq!(t.line_range(1), 2..4);
        assert_eq!(t.line_col(6), (2, 1));
        assert_eq!(t.offset(2, 5), 8);
        t.replace(1..5, "X\nY\nZ");
        assert_eq!(t.as_str(), "aX\nY\nZccc");
        assert_eq!(t.line_count(), 3);
        assert_eq!(t.line_start(2), 5);
    }

    proptest! {
        #[test]
        fn index_matches_rebuild(ops in proptest::collection::vec((0usize..60, 0usize..8, "[a\\n]{0,4}"), 1..30)) {
            let mut t = Text::new("abc\ndef\n\nghi");
            for (at, len, ins) in ops {
                let a = at.min(t.len());
                let b = (a + len).min(t.len());
                t.replace(a..b, &ins);
                let fresh = Text::new(t.as_str().to_string());
                prop_assert_eq!(&t.lines, &fresh.lines);
            }
        }

        #[test]
        fn edits_at_once_as_one_by_one(
            cuts in proptest::collection::vec((0usize..6, 0usize..4, "[aé\\n]{0,3}"), 0..24)
        ) {
            // Sorted, not overlapping edits; few are applied one by one,
            // many in one pass: the same text and index either way.
            let start = "abc\ndéf\n\nghi\njkl\nmno\npqr\nstu\nvwx\nyz\n".repeat(3);
            let mut edits = Vec::new();
            let mut at = 0;
            for (gap, len, insert) in cuts {
                let a = (at + gap).min(start.len());
                let b = (a + len).min(start.len());
                if !start.is_char_boundary(a) || !start.is_char_boundary(b) {
                    continue;
                }
                edits.push(org_edit::Edit { range: a..b, insert });
                at = b;
            }
            let mut one_by_one = Text::new(start.clone());
            for e in edits.iter().rev() {
                one_by_one.replace(e.range.clone(), &e.insert);
            }
            let mut t = Text::new(start);
            t.apply(&edits);
            prop_assert_eq!(t.as_str(), one_by_one.as_str());
            prop_assert_eq!(&t.lines, &one_by_one.lines);
        }
    }
}
