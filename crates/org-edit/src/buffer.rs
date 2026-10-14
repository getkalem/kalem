//! A small model of an Emacs buffer, so that commands can be written as
//! the sequence of edits the Emacs command makes, with point moving the
//! way Emacs moves it.

use unicode_width::UnicodeWidthChar;

use crate::transaction::{Edit, Selection, Transaction};

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
        let Some(ch) = line[i..].chars().next() else {
            break;
        };
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
    /// Markers of insertion type t: text inserted at them goes before.
    advancing: Vec<usize>,
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

/// Where a position at `p` goes when `start..end` is deleted
/// (`delete-region`): inside or at the end, to the start.
fn deleted(p: usize, start: usize, end: usize) -> usize {
    if p >= end {
        p - (end - start)
    } else if p > start {
        start
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
            advancing: Vec::new(),
        }
    }

    /// A new marker of insertion type t at `pos`; see
    /// [`Buf::advancing_marker`].
    pub(crate) fn add_advancing_marker(&mut self, pos: usize) -> usize {
        self.advancing.push(pos);
        self.advancing.len() - 1
    }

    /// The position of a marker of insertion type t.
    pub(crate) fn advancing_marker(&self, id: usize) -> usize {
        self.advancing[id]
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
        self.follow(start, end, new.len());
        self.text.replace_range(start..end, new);
    }

    /// The markers moved as `start..end` becoming `len` bytes moves them.
    fn follow(&mut self, start: usize, end: usize, len: usize) {
        for m in &mut self.markers {
            *m = adjust(*m, start, end, len);
        }
        for m in &mut self.advancing {
            *m = if start == end && *m == start {
                *m + len
            } else {
                adjust(*m, start, end, len)
            };
        }
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
        for m in self.markers.iter_mut().chain(&mut self.advancing) {
            *m = f(*m);
        }
        self.text.replace_range(start..end, new);
    }

    /// `delete-region`: positions inside or at the end collapse to the
    /// start.
    pub(crate) fn delete(&mut self, start: usize, end: usize) {
        self.point = deleted(self.point, start, end);
        self.edit(start, end, "");
    }

    /// Whole lines replaced, each as `org-table-align` replaces a line:
    /// [`Buf::insert_before_point`] of the new line and a line feed at the
    /// old line's start, then [`Buf::delete`] of the old line with its line
    /// feed. The text is made once rather than once a line, which made
    /// aligning a long table quadratic; point and markers move as the
    /// edits one by one move them. `lines` are the old lines' starts, in
    /// order, with their new text.
    pub(crate) fn replace_lines(&mut self, lines: &[(usize, String)]) {
        if lines.is_empty() {
            return;
        }
        let old = std::mem::take(&mut self.text);
        let mut text = String::with_capacity(old.len());
        let mut copied = 0;
        // How far the old text after the lines replaced so far has moved.
        let mut delta = 0isize;
        for (start, new) in lines {
            let end = old[*start..]
                .find('\n')
                .map_or(old.len(), |i| start + i + 1);
            text.push_str(&old[copied..*start]);
            text.push_str(new);
            text.push('\n');
            copied = end;
            let p = start.saturating_add_signed(delta);
            let inserted = new.len() + 1;
            if self.point > p {
                self.point += inserted;
            }
            self.follow(p, p, inserted);
            let s = p + inserted;
            let e = s + (end - start);
            self.point = deleted(self.point, s, e);
            self.follow(s, e, 0);
            delta += inserted as isize - (end - start) as isize;
        }
        text.push_str(&old[copied..]);
        self.text = text;
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

    /// The transaction from the original text to the current one: a
    /// replacement for each run of lines changed ([`line_edits`]), so that
    /// marks, cursors and the narrowing between two changes far apart
    /// (a table's rows aligned, a heading and its drawer) stay where they
    /// are, and the undo step holds the changes alone.
    pub(crate) fn transaction(&self, label: &str) -> Transaction {
        let mut t = Transaction::new(label);
        for e in line_edits(&self.original, &self.text) {
            t.edit(e.range, e.insert);
        }
        t.select(Selection::caret(self.point))
    }

    /// The part of the original text changed, and its new text; none when
    /// the text is as it was.
    pub(crate) fn changed(&self) -> Option<(std::ops::Range<usize>, &str)> {
        changed_part(&self.original, &self.text)
    }
}

/// The most differing lines [`line_edits`] looks for before it takes the
/// whole changed part for one replacement.
const MOST_DIFFERENCES: usize = 256;

/// The edits from `old` to `new`: their lines compared (Myers's
/// difference), each run of lines that differ one replacement without
/// the bytes it shares at its start and end. Texts that differ in more
/// than [`MOST_DIFFERENCES`] lines are one replacement of the part
/// between what they share at the start and at the end.
pub(crate) fn line_edits(old: &str, new: &str) -> Vec<Edit> {
    let Some((range, insert)) = changed_part(old, new) else {
        return Vec::new();
    };
    // Whole lines: from the start of the first changed line to the end of
    // the last, which the two texts share.
    let start = old[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let tail = old[range.end..]
        .find('\n')
        .map_or(old.len() - range.end, |i| i + 1);
    let new_end = range.start + insert.len();
    let (a, b) = (&old[start..range.end + tail], &new[start..new_end + tail]);
    let la: Vec<&str> = a.split_inclusive('\n').collect();
    let lb: Vec<&str> = b.split_inclusive('\n').collect();
    let Some(hunks) = differing_lines(&la, &lb, MOST_DIFFERENCES) else {
        return vec![Edit {
            range,
            insert: insert.to_string(),
        }];
    };
    let offsets = |lines: &[&str]| {
        let mut at = vec![0];
        for l in lines {
            at.push(at.last().unwrap_or(&0) + l.len());
        }
        at
    };
    let (oa, ob) = (offsets(&la), offsets(&lb));
    hunks
        .into_iter()
        .filter_map(|(x, y)| {
            let (r, s) = changed_part(&a[oa[x.start]..oa[x.end]], &b[ob[y.start]..ob[y.end]])?;
            let at = start + oa[x.start];
            Some(Edit {
                range: at + r.start..at + r.end,
                insert: s.to_string(),
            })
        })
        .collect()
}

/// The part of `old` that `new` changes, and its new text: what lies
/// between the bytes they share at the start and at the end, on
/// character boundaries; `None` when they are the same.
fn changed_part<'a>(old: &str, new: &'a str) -> Option<(std::ops::Range<usize>, &'a str)> {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(pre) || !new.is_char_boundary(pre) {
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
    while !old.is_char_boundary(a.len() - suf) || !new.is_char_boundary(b.len() - suf) {
        suf -= 1;
    }
    (pre + suf < a.len() || pre + suf < b.len())
        .then(|| (pre..a.len() - suf, &new[pre..b.len() - suf]))
}

/// The runs of lines that differ between `a` and `b`, as pairs of ranges
/// of their lines in order, by Myers's greedy algorithm; `None` when they
/// differ in more than `most` lines.
#[allow(clippy::type_complexity)]
fn differing_lines(
    a: &[&str],
    b: &[&str],
    most: usize,
) -> Option<Vec<(std::ops::Range<usize>, std::ops::Range<usize>)>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let most = most.min(a.len() + b.len()) as isize;
    let off = most + 1;
    let mut v = vec![0isize; 2 * most as usize + 3];
    // The furthest point of each diagonal before each step, to go back.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut end = None;
    'search: for d in 0..=most {
        trace.push(v.clone());
        for k in (-d..=d).step_by(2) {
            let i = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[i - 1] < v[i + 1]) {
                v[i + 1]
            } else {
                v[i - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[i] = x;
            if x >= n && y >= m {
                end = Some(d);
                break 'search;
            }
        }
    }
    let end = end?;
    // Back from the end: the lines the two share, last first.
    let mut same = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..=end).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let i = (k + off) as usize;
        let prev = if k == -d || (k != d && v[i - 1] < v[i + 1]) {
            k + 1
        } else {
            k - 1
        };
        let px = if d == 0 { 0 } else { v[(prev + off) as usize] };
        let py = if d == 0 { 0 } else { px - prev };
        while x > px && y > py {
            x -= 1;
            y -= 1;
            same.push((x as usize, y as usize));
        }
        (x, y) = (px, py);
    }
    same.reverse();
    // The runs between the lines they share.
    let mut hunks = Vec::new();
    let (mut pa, mut pb) = (0, 0);
    for (sa, sb) in same.into_iter().chain([(a.len(), b.len())]) {
        if sa > pa || sb > pb {
            hunks.push((pa..sa, pb..sb));
        }
        (pa, pb) = (sa + 1, sb + 1);
    }
    Some(hunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::Assoc;

    #[test]
    fn changes_far_apart_are_edits_apart() {
        let old = "| a |\n|b|\nkept 1\nkept 2\n| c |\n";
        let new = "| a  |\n|b|\nkept 1\nkept 2\n| cc |\n";
        let edits = line_edits(old, new);
        assert_eq!(edits.len(), 2, "{edits:?}");
        let mut t = Transaction::new("t");
        for e in &edits {
            t.edit(e.range.clone(), e.insert.clone());
        }
        assert_eq!(t.apply(old), new);
        // A position between them stays where it was.
        let kept = old.find("kept 2").unwrap();
        assert_eq!(&new[t.map(kept, Assoc::After)..][..6], "kept 2");
        // Lines added and taken away, apart.
        let new = "| a |\n|b|\nadded\nkept 1\nkept 2\n";
        let edits = line_edits(old, new);
        assert_eq!(edits.len(), 2, "{edits:?}");
        assert!(line_edits(old, old).is_empty());
    }

    proptest::proptest! {
        #[test]
        fn line_edits_make_the_new_text(
            old in proptest::collection::vec("[ab\n]{0,4}", 0..12),
            new in proptest::collection::vec("[ab\n]{0,4}", 0..12),
        ) {
            let (old, new) = (old.concat(), new.concat());
            let mut t = Transaction::new("t");
            for e in line_edits(&old, &new) {
                t.replace(e.range, e.insert).unwrap();
            }
            proptest::prop_assert_eq!(t.apply(&old), new);
        }

        #[test]
        fn too_many_differences_are_one_edit(n in 0usize..40) {
            let old: String = (0..n).map(|i| format!("{i}\n")).collect();
            let new: String = (0..n).map(|i| format!("x{i}\n")).collect();
            let edits = differing_lines(
                &old.split_inclusive('\n').collect::<Vec<_>>(),
                &new.split_inclusive('\n').collect::<Vec<_>>(),
                3,
            );
            proptest::prop_assert_eq!(edits.is_none(), n * 2 > 3);
        }
    }

    /// [`Buf::replace_lines`] the way `org-table-align` did it, a line at a
    /// time.
    fn one_by_one(buf: &mut Buf, lines: &[(usize, String)]) {
        let mut moved = 0isize;
        for (start, new) in lines {
            let p = start.saturating_add_signed(moved);
            let old_len = buf.text[p..]
                .find('\n')
                .map_or(buf.text.len() - p, |i| i + 1);
            buf.insert_before_point(p, &format!("{new}\n"));
            let s = p + new.len() + 1;
            buf.delete(s, s + old_len);
            moved += new.len() as isize + 1 - old_len as isize;
        }
    }

    #[test]
    fn lines_replaced_at_once_move_point_and_markers_as_one_by_one() {
        let text = "| a | b |\n|---+---|\n| 10 | x |\n| y | 2000 |\nafter";
        let starts = [0, 10, 20, 31];
        for point in 0..=text.len() {
            for lines in [
                vec![
                    (0, "|  a | b    |".to_string()),
                    (20, "| 10 | x    |".to_string()),
                ],
                vec![(10, "|----+------|".to_string()), (31, "| y |".to_string())],
                starts
                    .iter()
                    .map(|&s| (s, "|".repeat(s / 10 + 1)))
                    .collect(),
                vec![(
                    text.len() - 5,
                    "the last line, with no line feed".to_string(),
                )],
            ] {
                let mut a = Buf::new(text, point);
                let mut b = Buf::new(text, point);
                for m in [0, 5, 10, 25, 31, text.len()] {
                    a.add_marker(m);
                    b.add_marker(m);
                    a.add_advancing_marker(m);
                    b.add_advancing_marker(m);
                }
                a.replace_lines(&lines);
                one_by_one(&mut b, &lines);
                assert_eq!(
                    (&a.text, a.point, &a.markers, &a.advancing),
                    (&b.text, b.point, &b.markers, &b.advancing),
                    "point {point}, lines {lines:?}"
                );
            }
        }
    }
}
