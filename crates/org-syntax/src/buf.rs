//! A read-only view of the text with the Emacs buffer operations that
//! `org-element.el` relies on.
//!
//! Positions are byte offsets. `begv` and `zv` delimit the accessible
//! portion, like Emacs narrowing: objects are parsed with the buffer
//! narrowed to their container, which changes what counts as the
//! beginning or end of a line.

use crate::tables;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Buf<'a> {
    pub(crate) s: &'a str,
    pub(crate) b: &'a [u8],
    pub(crate) begv: usize,
    pub(crate) zv: usize,
}

impl<'a> Buf<'a> {
    pub(crate) fn new(s: &'a str) -> Self {
        Buf {
            s,
            b: s.as_bytes(),
            begv: 0,
            zv: s.len(),
        }
    }

    /// Returns a copy narrowed to `[begv, zv)`.
    pub(crate) fn narrowed(&self, begv: usize, zv: usize) -> Self {
        Buf {
            s: self.s,
            b: self.b,
            begv,
            zv,
        }
    }

    #[inline]
    pub(crate) fn byte(&self, p: usize) -> Option<u8> {
        if p < self.zv { Some(self.b[p]) } else { None }
    }

    /// `char-after`.
    #[inline]
    pub(crate) fn char_at(&self, p: usize) -> Option<char> {
        if p < self.zv {
            self.s[p..self.zv].chars().next()
        } else {
            None
        }
    }

    /// `char-before`.
    #[inline]
    pub(crate) fn char_before(&self, p: usize) -> Option<char> {
        if p > self.begv {
            self.s[self.begv..p].chars().next_back()
        } else {
            None
        }
    }

    /// Position after the character at `p`.
    #[inline]
    pub(crate) fn next_char(&self, p: usize) -> usize {
        match self.char_at(p) {
            Some(c) => p + c.len_utf8(),
            None => p,
        }
    }

    /// Position of the character before `p`.
    #[inline]
    pub(crate) fn prev_char(&self, p: usize) -> usize {
        match self.char_before(p) {
            Some(c) => p - c.len_utf8(),
            None => p,
        }
    }

    /// `bolp`.
    #[inline]
    pub(crate) fn is_bol(&self, p: usize) -> bool {
        p <= self.begv || self.b[p - 1] == b'\n'
    }

    /// `eolp`.
    #[inline]
    pub(crate) fn is_eol(&self, p: usize) -> bool {
        p >= self.zv || self.b[p] == b'\n'
    }

    /// `eobp`.
    #[inline]
    pub(crate) fn is_eob(&self, p: usize) -> bool {
        p >= self.zv
    }

    /// `line-beginning-position`.
    pub(crate) fn bol(&self, p: usize) -> usize {
        match memchr::memrchr(b'\n', &self.b[self.begv..p]) {
            Some(i) => self.begv + i + 1,
            None => self.begv,
        }
    }

    /// `line-end-position`.
    pub(crate) fn eol(&self, p: usize) -> usize {
        match memchr::memchr(b'\n', &self.b[p..self.zv]) {
            Some(i) => p + i,
            None => self.zv,
        }
    }

    /// `(forward-line)` from `p`: the start of the next line, or the end
    /// of the accessible portion.
    pub(crate) fn next_line(&self, p: usize) -> usize {
        let e = self.eol(p);
        if e < self.zv { e + 1 } else { self.zv }
    }

    /// `(line-beginning-position 2)`.
    pub(crate) fn lbp2(&self, p: usize) -> usize {
        self.next_line(p)
    }

    /// `(line-beginning-position 0)`: the start of the previous line, or
    /// the start of the current line when there is none.
    pub(crate) fn lbp0(&self, p: usize) -> usize {
        let b = self.bol(p);
        if b > self.begv { self.bol(b - 1) } else { b }
    }

    /// `(forward-line -1)` from `p`.
    pub(crate) fn prev_line(&self, p: usize) -> usize {
        self.lbp0(p)
    }

    /// `(line-end-position 0)`: the end of the previous line.
    pub(crate) fn lep0(&self, p: usize) -> usize {
        let b = self.bol(p);
        if b > self.begv { b - 1 } else { b }
    }

    /// `skip-chars-forward` for an ASCII set, bounded by `limit`.
    pub(crate) fn skip_fwd(&self, mut p: usize, set: &[u8], limit: usize) -> usize {
        let limit = limit.min(self.zv);
        while p < limit && set.contains(&self.b[p]) {
            p += 1;
        }
        p
    }

    /// `skip-chars-backward` for an ASCII set, bounded by `limit`.
    pub(crate) fn skip_bwd(&self, mut p: usize, set: &[u8], limit: usize) -> usize {
        let limit = limit.max(self.begv);
        while p > limit && set.contains(&self.b[p - 1]) {
            p -= 1;
        }
        p
    }

    /// `skip-chars-forward " \t"` to the end of the accessible portion.
    #[inline]
    pub(crate) fn skip_blank(&self, p: usize) -> usize {
        self.skip_fwd(p, b" \t", self.zv)
    }

    /// `skip-chars-forward " \r\t\n"` bounded by `limit`.
    #[inline]
    pub(crate) fn skip_ws(&self, p: usize, limit: usize) -> usize {
        self.skip_fwd(p, b" \r\t\n", limit)
    }

    /// `count-lines`.
    pub(crate) fn count_lines(&self, a: usize, b: usize) -> usize {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            return 0;
        }
        let n = memchr::memchr_iter(b'\n', &self.b[a..b]).count();
        if self.b[b - 1] == b'\n' { n } else { n + 1 }
    }

    /// The common "end of element" computation:
    /// `(skip-chars-forward " \r\t\n" limit)` followed by
    /// `(if (eobp) (point) (line-beginning-position))`.
    pub(crate) fn element_end(&self, p: usize, limit: usize) -> usize {
        let q = self.skip_ws(p, limit);
        if self.is_eob(q) { q } else { self.bol(q) }
    }

    /// Text of `[a, b)`.
    #[inline]
    pub(crate) fn slice(&self, a: usize, b: usize) -> &'a str {
        &self.s[a..b]
    }

    /// Returns `true` if `s[p..]` starts with `pat`, ignoring ASCII case,
    /// within the accessible portion.
    pub(crate) fn looking_at_ci(&self, p: usize, pat: &str) -> bool {
        let n = pat.len();
        p + n <= self.zv && self.b[p..p + n].eq_ignore_ascii_case(pat.as_bytes())
    }

    /// Returns `true` if `s[p..]` starts with `pat`, within the accessible
    /// portion.
    pub(crate) fn looking_at_str(&self, p: usize, pat: &str) -> bool {
        let n = pat.len();
        p + n <= self.zv && &self.b[p..p + n] == pat.as_bytes()
    }
    /// Skips characters without whitespace syntax forward (`\S-`).
    pub(crate) fn skip_nonspace_syntax(&self, mut p: usize, limit: usize) -> usize {
        while p < limit {
            match self.char_at(p) {
                Some(c) if !tables::is_space(c) => p += c.len_utf8(),
                _ => break,
            }
        }
        p
    }

    /// Returns `true` when the line containing `p`, from `p` to its end,
    /// consists only of spaces and tabs (`looking-at "[ \t]*$"`).
    pub(crate) fn rest_is_blank(&self, p: usize) -> bool {
        self.is_eol(self.skip_blank(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let b = Buf::new("ab\ncd\n\nef");
        assert_eq!(b.bol(4), 3);
        assert_eq!(b.eol(3), 5);
        assert_eq!(b.next_line(0), 3);
        assert_eq!(b.next_line(7), 9);
        assert_eq!(b.lbp0(4), 0);
        assert_eq!(b.lbp0(1), 0);
        assert_eq!(b.count_lines(0, 3), 1);
        assert_eq!(b.count_lines(0, 4), 2);
        assert_eq!(b.count_lines(0, 9), 4);
        assert_eq!(b.count_lines(5, 5), 0);
        assert_eq!(b.element_end(5, 9), 7);
    }

    #[test]
    fn narrowing() {
        let b = Buf::new("xx *a* yy").narrowed(3, 6);
        assert!(b.is_bol(3));
        assert!(b.is_eol(6));
        assert_eq!(b.eol(3), 6);
        assert_eq!(b.char_before(3), None);
    }
}
