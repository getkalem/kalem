//! Regular expressions translated from Org's Emacs regexps.
//!
//! Emacs regexps depend on the buffer's syntax table and character
//! classes. Patterns here are written in Rust regex syntax with
//! placeholders that expand to the exact Emacs classes:
//!
//! | Placeholder | Emacs |
//! |---|---|
//! | `{S}`   | `\s-`, `[[:space:]]` (whitespace syntax) |
//! | `{W}`   | `\w`, `[[:word:]]` (word syntax) |
//! | `{AN}`  | `[[:alnum:]]` |
//! | `{AL}`  | `[[:alpha:]]` |
//! | `{P}`   | `[[:punct:]]` |
//! | `{LB}`  | `\c|` (line breakable) |
//!
//! Placeholders expand to class bodies, so they are used inside brackets:
//! `[{S}]`, `[^{S}]`, `[{AN}_@#%]`.

use std::sync::OnceLock;

use regex_automata::meta::Regex;
use regex_automata::{Anchored, Input};

use crate::tables;

struct Classes {
    space: String,
    word: String,
    alnum: String,
    alpha: String,
    punct: String,
    line_breakable: String,
}

fn classes() -> &'static Classes {
    static C: OnceLock<Classes> = OnceLock::new();
    C.get_or_init(|| Classes {
        space: tables::class_body(|b| b & 0xF == tables::SYNTAX_WHITESPACE),
        word: tables::class_body(|b| b & 0xF == tables::SYNTAX_WORD),
        alnum: tables::class_body(|b| b & tables::ALNUM != 0),
        alpha: tables::class_body(|b| b & tables::ALPHA != 0),
        punct: tables::class_body(|b| b & tables::PUNCT != 0),
        line_breakable: tables::class_body(|b| b & tables::LINE_BREAKABLE != 0),
    })
}

/// Expands class placeholders in `pattern`.
pub(crate) fn expand(pattern: &str) -> String {
    let c = classes();
    pattern
        .replace("{S}", &c.space)
        .replace("{W}", &c.word)
        .replace("{AN}", &c.alnum)
        .replace("{AL}", &c.alpha)
        .replace("{P}", &c.punct)
        .replace("{LB}", &c.line_breakable)
}

/// Compiles a pattern after expanding placeholders. Patterns are
/// constants, so failure is a programming error.
pub(crate) fn compile(pattern: &str) -> Regex {
    let expanded = expand(pattern);
    Regex::new(&expanded).unwrap_or_else(|e| panic!("bad regex {pattern:?}: {e}"))
}

/// A lazily compiled regex.
pub(crate) struct Lazy {
    pattern: &'static str,
    /// The bytes an anchored match can start with (after blanks when
    /// `blanks`); empty when not known.
    first: &'static [u8],
    blanks: bool,
    cell: OnceLock<Regex>,
}

impl Lazy {
    pub(crate) const fn new(pattern: &'static str) -> Self {
        Lazy {
            pattern,
            first: b"",
            blanks: false,
            cell: OnceLock::new(),
        }
    }

    /// A regex whose matches start with one of the bytes `first`, after
    /// spaces and tabs if `blanks`: anchored matches elsewhere are ruled
    /// out without running the regex ([`Lazy::may_start`]).
    pub(crate) const fn starting(
        pattern: &'static str,
        first: &'static [u8],
        blanks: bool,
    ) -> Self {
        Lazy {
            pattern,
            first,
            blanks,
            cell: OnceLock::new(),
        }
    }

    /// Whether an anchored match can start at `p` of `s`, from the first
    /// byte there; always true when the regex gives no first bytes.
    #[inline]
    pub(crate) fn may_start(&self, s: &[u8], p: usize) -> bool {
        if self.first.is_empty() {
            return true;
        }
        let mut q = p;
        if self.blanks {
            while matches!(s.get(q), Some(b' ' | b'\t')) {
                q += 1;
            }
        }
        s.get(q).is_some_and(|c| self.first.contains(c))
    }

    pub(crate) fn get(&self) -> &Regex {
        self.cell.get_or_init(|| compile(self.pattern))
    }
}

/// The result of a match: group spans as absolute positions.
#[derive(Debug, Clone)]
pub(crate) struct Captures {
    spans: Vec<Option<(usize, usize)>>,
}

impl Captures {
    pub(crate) fn get(&self, i: usize) -> Option<(usize, usize)> {
        self.spans.get(i).copied().flatten()
    }
    pub(crate) fn start(&self, i: usize) -> Option<usize> {
        self.get(i).map(|s| s.0)
    }
    pub(crate) fn whole(&self) -> (usize, usize) {
        self.get(0).expect("group 0 always matches")
    }
}

fn to_captures(re: &Regex, caps: &regex_automata::util::captures::Captures) -> Captures {
    let n = re.group_info().group_len(regex_automata::PatternID::ZERO);
    Captures {
        spans: (0..n)
            .map(|i| caps.get_group(i).map(|s| (s.start, s.end)))
            .collect(),
    }
}

/// `looking-at`: an anchored match at `p`, with `[begv, zv)` as the
/// haystack, so `^` and `$` behave as in a narrowed Emacs buffer.
pub(crate) fn looking_at(
    re: &Regex,
    s: &str,
    begv: usize,
    zv: usize,
    p: usize,
) -> Option<Captures> {
    if p > zv {
        return None;
    }
    let hay = &s[begv..zv];
    let input = Input::new(hay)
        .span(p - begv..hay.len())
        .anchored(Anchored::Yes);
    let mut caps = re.create_captures();
    re.search_captures(&input, &mut caps);
    if !caps.is_match() {
        return None;
    }
    let mut c = to_captures(re, &caps);
    for s in c.spans.iter_mut().flatten() {
        s.0 += begv;
        s.1 += begv;
    }
    Some(c)
}

/// `looking-at` for a regex that cannot match past the end of the line:
/// the haystack ends at the line end. This keeps the search short, so the
/// fast engines apply, and gives the same result because `$` matches at
/// the end of the haystack.
pub(crate) fn looking_at_line(
    re: &Regex,
    s: &str,
    begv: usize,
    zv: usize,
    p: usize,
) -> Option<Captures> {
    let eol = line_end(s, p, zv);
    looking_at(re, s, begv, eol, p)
}

/// `looking-at-p` for a regex that cannot match past the end of the line.
pub(crate) fn looking_at_line_p(re: &Regex, s: &str, begv: usize, zv: usize, p: usize) -> bool {
    let eol = line_end(s, p, zv);
    looking_at_p(re, s, begv, eol, p)
}

fn line_end(s: &str, p: usize, zv: usize) -> usize {
    if p >= zv {
        return zv;
    }
    memchr::memchr(b'\n', &s.as_bytes()[p..zv]).map_or(zv, |i| p + i)
}

/// `looking-at-p`.
pub(crate) fn looking_at_p(re: &Regex, s: &str, begv: usize, zv: usize, p: usize) -> bool {
    if p > zv {
        return false;
    }
    let hay = &s[begv..zv];
    let input = Input::new(hay)
        .span(p - begv..hay.len())
        .anchored(Anchored::Yes);
    re.is_match(input)
}

/// `re-search-forward` from `p` with bound `limit`: the first match that
/// starts at or after `p` and ends at or before `limit`.
pub(crate) fn search_forward(
    re: &Regex,
    s: &str,
    begv: usize,
    zv: usize,
    p: usize,
    limit: usize,
) -> Option<Captures> {
    let limit = limit.min(zv);
    if p > limit {
        return None;
    }
    // Emacs does not let a match consume text past the bound, but `$`
    // and look-ahead still see the real buffer. Bounds are line starts in
    // practice, so searching the slice up to the bound is equivalent.
    let hay = &s[begv..limit];
    let input = Input::new(hay).span(p - begv..hay.len());
    let mut caps = re.create_captures();
    re.search_captures(&input, &mut caps);
    if !caps.is_match() {
        return None;
    }
    let mut c = to_captures(re, &caps);
    for s in c.spans.iter_mut().flatten() {
        s.0 += begv;
        s.1 += begv;
    }
    Some(c)
}

/// `re-search-forward` when only the match bounds are needed. Skipping
/// capture groups lets the fast DFA engines answer on their own.
pub(crate) fn find_forward(
    re: &Regex,
    s: &str,
    begv: usize,
    zv: usize,
    p: usize,
    limit: usize,
) -> Option<(usize, usize)> {
    let limit = limit.min(zv);
    if p > limit {
        return None;
    }
    let hay = &s[begv..limit];
    let input = Input::new(hay).span(p - begv..hay.len());
    re.search(&input)
        .map(|m| (m.start() + begv, m.end() + begv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        let re = compile(r"\A[{W}]+\z");
        assert!(re.is_match("çalışma"));
        assert!(!re.is_match("a-b"));
    }

    #[test]
    fn anchored() {
        static RE: Lazy = Lazy::new(r"(?m)^[ \t]*#\+(?i:END_SRC)[ \t]*$");
        let s = "x\n  #+end_src  \ny";
        assert!(looking_at_p(RE.get(), s, 0, s.len(), 2));
        assert!(!looking_at_p(RE.get(), s, 0, s.len(), 0));
        let c = search_forward(RE.get(), s, 0, s.len(), 0, s.len()).unwrap();
        assert_eq!(c.whole(), (2, 15));
    }
}
