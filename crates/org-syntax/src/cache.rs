//! Lazily built indexes over the whole buffer.
//!
//! Org's parsing functions search forward a lot: for the line that ends a
//! block, for the bracket that closes an opening one, for the marker that
//! closes an emphasis. Done naively, nested structures make those searches
//! quadratic (Emacs has the same problem). These indexes answer each search
//! with a binary search and give exactly the same result (section 3.6 of
//! the design document).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::buf::Buf;
use crate::tables;

#[derive(Debug, Default)]
pub(crate) struct Caches {
    /// Line starts of lines matching `^[ \t]*MARKER[ \t]*$`, per upper-cased
    /// marker.
    marker_lines: HashMap<String, Vec<usize>>,
    /// For each opening bracket, the position after its match. Keyed by
    /// bracket and whether the table covers the whole buffer.
    brackets: HashMap<(u8, bool), Vec<(usize, usize)>>,
    /// Emphasis closing candidates per marker (see [`Index::closer`]).
    closers: HashMap<u8, Vec<usize>>,
    /// Lines whose first non-blank byte is the key: the line start and the
    /// position of that byte.
    led_by: HashMap<u8, Vec<(usize, usize)>>,
}

const UNMATCHED: usize = usize::MAX;

/// The indexes of one buffer.
#[derive(Debug, Default)]
pub(crate) struct Index {
    inner: RefCell<Caches>,
    /// The part of the buffer marker lines are indexed in; searches never
    /// leave it. Defaults to the whole buffer.
    region: std::cell::Cell<Option<(usize, usize)>>,
}

impl Index {
    /// Restricts marker-line indexing to `[lo, hi)`. Only valid when every
    /// search starts and ends inside that range.
    pub(crate) fn set_region(&self, lo: usize, hi: usize) {
        self.region.set(Some((lo, hi)));
    }

    /// Lines (start, first non-blank) led by `first` after blanks, in the
    /// indexed region: one pass for all markers starting with that byte.
    fn led_by(&self, b: &Buf<'_>, first: u8) {
        if self.inner.borrow().led_by.contains_key(&first) {
            return;
        }
        let bytes = b.b;
        let end = bytes.len();
        let (mut ls, region_end) = self.region.get().unwrap_or((0, end));
        let mut v = Vec::new();
        while ls < end {
            let mut q = ls;
            while q < end && (bytes[q] == b' ' || bytes[q] == b'\t') {
                q += 1;
            }
            if bytes.get(q) == Some(&first) {
                v.push((ls, q));
            }
            match memchr::memchr(b'\n', &bytes[q.min(end)..]) {
                Some(i) => ls = q + i + 1,
                None => break,
            }
            if ls >= region_end {
                break;
            }
        }
        self.inner.borrow_mut().led_by.insert(first, v);
    }

    /// Line starts of all lines that are `MARKER` surrounded by blanks,
    /// compared case-insensitively.
    fn marker_lines(&self, b: &Buf<'_>, marker: &str) -> std::cell::Ref<'_, Vec<usize>> {
        let key = marker.to_ascii_uppercase();
        if !self.inner.borrow().marker_lines.contains_key(&key) {
            let bytes = b.b;
            let end = bytes.len();
            let first = marker.as_bytes().first().copied().unwrap_or(b'\n');
            self.led_by(b, first);
            let v: Vec<usize> = self.inner.borrow().led_by[&first]
                .iter()
                .filter(|&&(_, q)| {
                    let e = q + marker.len();
                    if e > end || !bytes[q..e].eq_ignore_ascii_case(marker.as_bytes()) {
                        return false;
                    }
                    let mut r = e;
                    while r < end && (bytes[r] == b' ' || bytes[r] == b'\t') {
                        r += 1;
                    }
                    r == end || bytes[r] == b'\n'
                })
                .map(|&(ls, _)| ls)
                .collect();
            self.inner.borrow_mut().marker_lines.insert(key.clone(), v);
        }
        std::cell::Ref::map(self.inner.borrow(), |c| &c.marker_lines[&key])
    }

    /// `(re-search-forward "^[ \t]*MARKER[ \t]*$" limit t)` from `p` in an
    /// unnarrowed buffer: the start of the first matching line.
    pub(crate) fn find_marker_line(
        &self,
        b: &Buf<'_>,
        p: usize,
        limit: usize,
        marker: &str,
    ) -> Option<usize> {
        let start = if b.is_bol(p) { p } else { b.next_line(p) };
        let lines = self.marker_lines(b, marker);
        let i = lines.partition_point(|&ls| ls < start);
        let ls = *lines.get(i)?;
        (ls < limit && b.eol(ls) <= limit).then_some(ls)
    }

    fn bracket_table(&self, b: &Buf<'_>, open: u8, close: u8, whole: bool) {
        if self.inner.borrow().brackets.contains_key(&(open, whole)) {
            return;
        }
        let (lo, hi) = if whole {
            (0, b.b.len())
        } else {
            self.region.get().unwrap_or((0, b.b.len()))
        };
        let mut table: Vec<(usize, usize)> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        for (i, &c) in b.b[lo..hi].iter().enumerate() {
            if c == open {
                stack.push(table.len());
                table.push((lo + i, UNMATCHED));
            } else if c == close
                && let Some(j) = stack.pop()
            {
                table[j].1 = lo + i + 1;
            }
        }
        self.inner
            .borrow_mut()
            .brackets
            .insert((open, whole), table);
    }

    /// `scan-lists` over one balanced group from `p`, within the
    /// accessible portion of `b`: the position after the closing bracket.
    /// Searches stay inside the indexed region.
    pub(crate) fn match_bracket(
        &self,
        b: &Buf<'_>,
        p: usize,
        open: u8,
        close: u8,
    ) -> Option<usize> {
        self.match_bracket_in(b, p, open, close, false)
    }

    /// Like [`Index::match_bracket`], but the match may lie anywhere after
    /// `p` in the whole buffer (`#+CALL:` lines search that far).
    pub(crate) fn match_bracket_global(
        &self,
        b: &Buf<'_>,
        p: usize,
        open: u8,
        close: u8,
    ) -> Option<usize> {
        self.match_bracket_in(b, p, open, close, true)
    }

    fn match_bracket_in(
        &self,
        b: &Buf<'_>,
        p: usize,
        open: u8,
        close: u8,
        whole: bool,
    ) -> Option<usize> {
        if b.byte(p) != Some(open) {
            return None;
        }
        self.bracket_table(b, open, close, whole);
        let inner = self.inner.borrow();
        let table = &inner.brackets[&(open, whole)];
        let i = table.binary_search_by_key(&p, |e| e.0).ok()?;
        let end = table[i].1;
        // A narrowed scan finds the same bracket if it lies inside the
        // accessible portion, and nothing otherwise.
        (end != UNMATCHED && end <= b.zv).then_some(end)
    }

    /// Positions `r` where, in the whole buffer, `(not space) MARK (post)`
    /// matches with the non-space character at `r`.
    fn closers(&self, b: &Buf<'_>, mark: u8) -> std::cell::Ref<'_, Vec<usize>> {
        if !self.inner.borrow().closers.contains_key(&mark) {
            let s = b.s;
            let bytes = s.as_bytes();
            let (lo, hi) = self.region.get().unwrap_or((0, s.len()));
            let mut v = Vec::new();
            // Each marker `m` after a character `r` in `[lo, hi)`; the
            // marker is ASCII, so `m` is a character boundary.
            let upto = (hi + 4).min(bytes.len());
            if lo < upto {
                for m in memchr::memchr_iter(mark, &bytes[lo + 1..upto]).map(|i| i + lo + 1) {
                    let Some(c) = s[..m].chars().next_back() else {
                        continue;
                    };
                    let r = m - c.len_utf8();
                    if r < lo || r >= hi || tables::is_space(c) {
                        continue;
                    }
                    if post_ok(s[m + 1..].chars().next()) {
                        v.push(r);
                    }
                }
            }
            self.inner.borrow_mut().closers.insert(mark, v);
        }
        std::cell::Ref::map(self.inner.borrow(), |c| &c.closers[&mark])
    }

    /// The first `r >= from` where the emphasis closing regexp matches in
    /// the (possibly narrowed) buffer `b`. Returns the position after the
    /// closing marker.
    pub(crate) fn closer(&self, b: &Buf<'_>, from: usize, mark: u8) -> Option<usize> {
        let listed = {
            let v = self.closers(b, mark);
            let i = v.partition_point(|&r| r < from);
            v.get(i).copied()
        };
        // In a narrowed buffer, a marker just before the end is followed by
        // the end of the accessible portion, which counts as a line end.
        let at_end = {
            let zv = b.zv;
            (zv >= 2 && b.b.get(zv - 1) == Some(&mark))
                .then(|| b.prev_char(zv - 1))
                .filter(|&r| {
                    r >= from && r >= b.begv && b.char_at(r).is_some_and(|c| !tables::is_space(c))
                })
        };
        let listed = listed.filter(|&r| {
            let m = r + b.char_at(r).map_or(1, |c| c.len_utf8());
            m < b.zv
        });
        let r = match (listed, at_end) {
            (Some(a), Some(z)) => a.min(z),
            (a, z) => a.or(z)?,
        };
        let m = r + b.char_at(r).map_or(1, |c| c.len_utf8());
        Some(m + 1)
    }
}

/// The emphasis post-character rule, with `None` or a newline meaning the
/// end of a line.
fn post_ok(c: Option<char>) -> bool {
    match c {
        None | Some('\n') => true,
        Some(c) => {
            tables::is_space(c)
                || matches!(
                    c,
                    '-' | '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | ')' | '}' | '\\' | '['
                )
        }
    }
}
