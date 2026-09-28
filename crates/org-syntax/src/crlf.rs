//! Byte order marks and CRLF line endings.
//!
//! Emacs decodes a file with DOS line endings into a buffer without
//! carriage returns, and drops a UTF-8 byte order mark, so Org never sees
//! either. Kalem does the same: it parses a copy of the text without the
//! mark and with every `\r\n` turned into `\n`, then maps positions back to
//! the original text. A line feed token then covers both bytes of the
//! original `\r\n`, the mark becomes a [`crate::SyntaxKind::BOM`] token, and
//! the tree still reproduces the input exactly.

use crate::raw::Raw;

/// Positions (in the normalized text) of the line feeds that were preceded
/// by a carriage return.
#[derive(Debug, Clone)]
pub(crate) struct Map {
    /// Length of the removed byte order mark (0 or 3).
    pub(crate) bom: usize,
    crlf: Vec<usize>,
}

/// Returns the normalized text and the position map, or `None` when the
/// text has no `\r\n`.
pub(crate) fn normalize(text: &str) -> Option<(String, Map)> {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let body = &text[bom..];
    if bom == 0 && memchr::memmem::find(body.as_bytes(), b"\r\n").is_none() {
        return None;
    }
    let (out, crlf) = strip_pairs(body);
    Some((out, Map { bom, crlf }))
}

/// Turns every `\r\n` of `body` into `\n`; returns the result and the
/// positions of those line feeds in it.
pub(crate) fn strip_pairs(body: &str) -> (String, Vec<usize>) {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut crlf = Vec::new();
    let mut last = 0;
    for i in memchr::memmem::find_iter(bytes, b"\r\n") {
        out.push_str(&body[last..i]);
        crlf.push(out.len());
        last = i + 1;
    }
    out.push_str(&body[last..]);
    (out, crlf)
}

/// The tree of the normalized text, kept for documents with CRLF line
/// endings or a byte order mark so that edits can be reparsed
/// incrementally in normalized coordinates.
#[derive(Debug, Clone)]
pub(crate) struct Norm {
    pub(crate) green: rowan::GreenNode,
    pub(crate) map: Map,
    /// The normalized text.
    pub(crate) text: String,
}

impl Map {
    /// Maps a position in the normalized text to the original text.
    pub(crate) fn orig(&self, p: usize) -> usize {
        self.bom + p + self.crlf.partition_point(|&q| q < p)
    }

    /// Whether the text has any pair or a mark.
    pub(crate) fn is_empty(&self) -> bool {
        self.bom == 0 && self.crlf.is_empty()
    }

    /// The map after replacing the normalized range `[na, nb)` with text of
    /// length `len` whose own pairs were at `pairs` (relative to `na`).
    pub(crate) fn edited(&self, na: usize, nb: usize, pairs: &[usize], len: usize) -> Map {
        let before = self.crlf.partition_point(|&q| q < na);
        let after = self.crlf.partition_point(|&q| q < nb);
        let mut crlf = Vec::with_capacity(self.crlf.len() + pairs.len());
        crlf.extend_from_slice(&self.crlf[..before]);
        crlf.extend(pairs.iter().map(|&q| na + q));
        crlf.extend(self.crlf[after..].iter().map(|&q| q - nb + na + len));
        Map {
            bom: self.bom,
            crlf,
        }
    }

    /// The original position of the carriage return of pair `i`.
    fn cr(&self, i: usize) -> usize {
        self.bom + self.crlf[i] + i
    }

    /// Maps a position in the original text to the normalized text. Inside
    /// a `\r\n` pair both bytes map to the line feed.
    pub(crate) fn norm(&self, p: usize) -> usize {
        p.saturating_sub(self.bom) - self.pairs_before(p)
    }

    /// The number of pairs whose carriage return lies before `p`.
    fn pairs_before(&self, p: usize) -> usize {
        // `cr(i)` grows with `i`: binary search on the index.
        let (mut lo, mut hi) = (0, self.crlf.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.cr(mid) < p {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Whether the original text has the carriage return of a `\r\n` pair
    /// at `p`.
    pub(crate) fn cr_at(&self, p: usize) -> bool {
        let i = self.pairs_before(p);
        i < self.crlf.len() && self.cr(i) == p
    }

    pub(crate) fn apply(&self, raw: &mut Raw) {
        crate::deep(|| self.apply_inner(raw));
        if self.bom > 0 {
            // The document starts at the mark, which gets its own token.
            raw.begin = 0;
            raw.tok(crate::SyntaxKind::BOM, 0, self.bom);
        }
    }

    fn apply_inner(&self, raw: &mut Raw) {
        raw.begin = self.orig(raw.begin);
        raw.end = self.orig(raw.end);
        raw.pa = self.orig(raw.pa);
        raw.cb = raw.cb.map(|p| self.orig(p));
        raw.ce = raw.ce.map(|p| self.orig(p));
        for t in &mut raw.tokens {
            t.start = self.orig(t.start);
            t.end = self.orig(t.end);
        }
        for c in &mut raw.children {
            crate::deep(|| self.apply_inner(c));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{SyntaxKind, SyntaxNode};

    fn shape(n: &SyntaxNode) -> Vec<(SyntaxKind, usize)> {
        let mut out = Vec::new();
        for e in n.descendants() {
            out.push((e.kind(), e.children().count()));
        }
        out
    }

    #[test]
    fn crlf_parses_like_lf() {
        let lf = "#+TITLE: t\n\n* TODO Head :tag:\nSCHEDULED: <2026-01-01 Thu>\n:PROPERTIES:\n:A: b\n:END:\n\n- item *b*\n- two\n\n#+begin_src sh\necho\n#+end_src\n| a | b |\n|---+---|\n";
        let crlf = lf.replace('\n', "\r\n");
        let a = crate::parse(lf);
        let b = crate::parse(&crlf);
        assert_eq!(b.syntax().to_string(), crlf);
        assert_eq!(shape(&a.syntax()), shape(&b.syntax()));
    }

    #[test]
    fn byte_order_mark_is_skipped_like_emacs() {
        let text = "\u{feff}* Headline\r\ntext\r\n";
        let p = crate::parse(text);
        assert_eq!(p.syntax().to_string(), text);
        assert!(
            p.syntax()
                .descendants()
                .any(|n| n.kind() == crate::SyntaxKind::HEADLINE)
        );
        assert_eq!(
            p.syntax().first_token().unwrap().kind(),
            crate::SyntaxKind::BOM
        );
    }

    #[test]
    fn mixed_line_endings_roundtrip() {
        let text = "* a\r\n* b\n\rtext\r\r\n";
        assert_eq!(crate::parse(text).syntax().to_string(), text);
    }
}
