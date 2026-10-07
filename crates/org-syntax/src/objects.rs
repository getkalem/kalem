//! Object-level parsing: `org-element--object-lex`,
//! `org-element--parse-objects` and every object parser.
//!
//! Objects are parsed with the buffer narrowed to their container, as in
//! Emacs, so every parser receives a narrowed [`Buf`].

use regex_automata::meta::Regex;

use crate::SyntaxKind::{self, *};
use crate::buf::Buf;
use crate::context::{ItemTerminator, ParseContext};
use crate::elements::Parser;
use crate::raw::Raw;
use crate::re::{self, Lazy};
use crate::tables;

// ------------------------------------------------------------------------
// Restrictions (`org-element-object-restrictions`).

/// A set of object kinds, as a bitset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Restriction(u32);

fn bit(k: SyntaxKind) -> u32 {
    debug_assert!(k.is_object());
    1 << (k as u16 - BOLD as u16)
}

impl Restriction {
    fn of(kinds: &[SyntaxKind]) -> Self {
        Restriction(kinds.iter().fold(0, |acc, k| acc | bit(*k)))
    }
    pub(crate) fn has(self, k: SyntaxKind) -> bool {
        self.0 & bit(k) != 0
    }
    fn with(self, other: Restriction) -> Self {
        Restriction(self.0 | other.0)
    }
    fn without(self, kinds: &[SyntaxKind]) -> Self {
        Restriction(self.0 & !Restriction::of(kinds).0)
    }
}

fn minimal_set() -> Restriction {
    Restriction::of(&[
        BOLD,
        CODE,
        ENTITY,
        ITALIC,
        LATEX_FRAGMENT,
        STRIKE_THROUGH,
        SUBSCRIPT,
        SUPERSCRIPT,
        UNDERLINE,
        VERBATIM,
    ])
}

fn all_objects() -> Restriction {
    Restriction::of(&[
        BOLD,
        CITATION,
        CITATION_REFERENCE,
        CODE,
        ENTITY,
        EXPORT_SNIPPET,
        FOOTNOTE_REFERENCE,
        INLINE_BABEL_CALL,
        INLINE_SRC_BLOCK,
        ITALIC,
        LINE_BREAK,
        LATEX_FRAGMENT,
        LINK,
        MACRO,
        RADIO_TARGET,
        STATISTICS_COOKIE,
        STRIKE_THROUGH,
        SUBSCRIPT,
        SUPERSCRIPT,
        TABLE_CELL,
        TARGET,
        TIMESTAMP,
        UNDERLINE,
        VERBATIM,
    ])
}

fn standard_set() -> Restriction {
    all_objects().without(&[CITATION_REFERENCE, TABLE_CELL])
}

/// `(org-element-restriction TYPE)`: the objects allowed inside `kind`, or
/// `None` when it cannot contain objects.
pub(crate) fn restriction_for(kind: SyntaxKind) -> Option<Restriction> {
    let std = standard_set();
    let no_lb = std.without(&[LINE_BREAK]);
    Some(match kind {
        BOLD | FOOTNOTE_REFERENCE | ITALIC | PARAGRAPH | STRIKE_THROUGH | SUBSCRIPT
        | SUPERSCRIPT | UNDERLINE | VERSE_BLOCK => std,
        CITATION => Restriction::of(&[CITATION_REFERENCE]),
        CITATION_REFERENCE => {
            no_lb.without(&[CITATION, CITATION_REFERENCE, FOOTNOTE_REFERENCE, LINK])
        }
        HEADLINE | INLINETASK | ITEM => no_lb,
        KEYWORD => std.without(&[FOOTNOTE_REFERENCE]),
        LINK => Restriction::of(&[
            EXPORT_SNIPPET,
            INLINE_BABEL_CALL,
            INLINE_SRC_BLOCK,
            MACRO,
            STATISTICS_COOKIE,
        ])
        .with(minimal_set()),
        RADIO_TARGET => minimal_set(),
        TABLE_CELL => Restriction::of(&[
            CITATION,
            EXPORT_SNIPPET,
            FOOTNOTE_REFERENCE,
            LINK,
            MACRO,
            RADIO_TARGET,
            TARGET,
            TIMESTAMP,
        ])
        .with(minimal_set()),
        TABLE_ROW => Restriction::of(&[TABLE_CELL]),
        _ => return None,
    })
}

pub(crate) fn restriction_keyword() -> Restriction {
    restriction_for(KEYWORD).expect("keyword restriction")
}

// ------------------------------------------------------------------------
// Context-dependent regexps.

/// Regexps that depend on the parse context: link types, radio targets
/// and list settings.
#[derive(Debug, Clone)]
pub(crate) struct ContextRegexes {
    pub(crate) paragraph_separate: Regex,
    plain_link: Regex,
    angle_link: Regex,
    pub(crate) types_prefix: Regex,
    radio: Option<crate::radio::RadioMatcher>,
    /// Lowercase link type names, longest first.
    types: Vec<String>,
    /// The bytes a link type can start with, in either case (all when a
    /// type is empty).
    type_first: [bool; 256],
}

fn escape(s: &str) -> String {
    regex_syntax::escape(s)
}

impl ContextRegexes {
    pub(crate) fn new(ctx: &ParseContext) -> Self {
        let mut types: Vec<String> = ctx
            .link_types
            .iter()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        types.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        types.dedup();
        let alt = types
            .iter()
            .map(|t| escape(t))
            .collect::<Vec<_>>()
            .join("|");
        let term = match ctx.item_terminator {
            ItemTerminator::Both => "[.)]",
            ItemTerminator::Dot => r"\.",
            ItemTerminator::Paren => r"\)",
        };
        let alpha = if ctx.list_allow_alphabetical {
            "|[A-Za-z]"
        } else {
            ""
        };
        let clock = r"^[ \t]*CLOCK:(?:[ \t]+\[[0-9]{4}-[0-9]{2}-[0-9]{2}(?: .*?)?\](?:--\[[0-9]{4}-[0-9]{2}-[0-9]{2}(?: .*?)?\][ \t]+=>[ \t]+[0-9]+:[0-9][0-9])?|[ \t]+=>[ \t]+[0-9]+:[0-9][0-9])[ \t]*$";
        let sep = format!(
            r"(?mi)^(?:\*+ |\[fn:[-_{{W}}]+\]|%%\(|[ \t]*(?:$|\||\+(?:-+\+)+[ \t]*$|#(?: |$|\+(?:BEGIN_[^{{S}}]+|[^{{S}}]+(?:\[.*\])?:[ \t]*))|:(?: |$|[-_{{W}}]+:[ \t]*$)|-{{5,}}[ \t]*$|\\begin\{{([A-Za-z0-9*]+)\}}|{clock}|(?:[-+*]|(?:[0-9]+{alpha}){term})(?:[ \t]|$)))"
        );
        let non_space_bracket = r"[^\]\[ \t\n()<>]";
        let paren = format!(
            r"[<(\[](?:{nsb}|[<(\[]{nsb}*[\])>])*[\])>]",
            nsb = non_space_bracket
        );
        let plain = format!(
            r"(?i)({alt}):((?:{nsb}|{paren})+(?:[^{{P}} \t\n]|/|{paren}))",
            nsb = non_space_bracket
        );
        let angle = format!(r"(?i)<({alt}):([^>\n]*(?:\n[ \t]*[^> \t\n][^>\n]*)*)>");
        let types_prefix = format!(r"(?i)\A({alt}):");
        let radio = (!ctx.radio_targets.is_empty())
            .then(|| crate::radio::RadioMatcher::new(&ctx.radio_targets));
        ContextRegexes {
            paragraph_separate: re::compile(&sep),
            plain_link: re::compile(&plain),
            angle_link: re::compile(&angle),
            types_prefix: re::compile(&types_prefix),
            radio,
            type_first: {
                let mut first = [false; 256];
                for t in &types {
                    match t.as_bytes().first() {
                        Some(&c) => {
                            first[usize::from(c.to_ascii_lowercase())] = true;
                            first[usize::from(c.to_ascii_uppercase())] = true;
                        }
                        None => first = [true; 256],
                    }
                }
                first
            },
            types,
        }
    }

    /// Does a link type (case-insensitively) followed by `suffix` start at
    /// `p`? Returns the end of the type.
    fn type_at(&self, b: &Buf<'_>, p: usize, colon: bool) -> Option<usize> {
        if !self.type_first[usize::from(b.b.get(p).copied().unwrap_or(0))] && !self.type_first[0] {
            return None;
        }
        for t in &self.types {
            if b.looking_at_ci(p, t) && (!colon || b.byte(p + t.len()) == Some(b':')) {
                return Some(p + t.len());
            }
        }
        None
    }
}

// ------------------------------------------------------------------------
// Static regexps.

static ENTITY_RE: Lazy =
    Lazy::new(r"(?m)\\(?:(_ +)|(there4|sup[123]|frac[13][24]|[a-zA-Z]+)($|\{\}|[^{AL}]))");
static LATEX_MACRO: Lazy = Lazy::new(r"\\[a-zA-Z]+\*?(?:(\[[^\]\[\n{}]*\])|(\{[^{}\n]*\}))*");
static EXPORT_SNIPPET_RE: Lazy = Lazy::new(r"(?i)@@([-A-Za-z0-9]+):");
static FOOTNOTE_REF: Lazy = Lazy::new(r"(?i)\[fn:(?:([-_{W}]+)?(:)|([-_{W}]+)\])");
static INLINE_CALL: Lazy = Lazy::new(r"call_([^ \t\n\[(]+)[(\[]");
static INLINE_SRC: Lazy = Lazy::new(r"src_([^ \t\n\[{]+)[{\[]");
static MACRO_RE: Lazy = Lazy::new(r"\{\{\{([a-zA-Z][-a-zA-Z0-9_]*)(\(((?s:.)*?)\))?\}\}\}");
static RADIO_TARGET_RE: Lazy =
    Lazy::new(r"<<<([^<>\n\r \t]|[^<>\n\r \t][^<>\n\r]*[^<>\n\r \t])>>>");
static TARGET_RE: Lazy = Lazy::new(r"<<([^<>\n\r \t]|[^<>\n\r \t][^<>\n\r]*[^<>\n\r \t])>>");
static COOKIE: Lazy = Lazy::new(r"\[[0-9]*(%|/[0-9]*)\]");
static TIMESTAMP_ANY: Lazy = Lazy::new(
    r"(?i)(?:[\[<]([0-9]{4}-[0-9]{2}-[0-9]{2}(?: .*?)?)[\]>]|<[0-9]+-[0-9]+-[0-9]+[^>\n]+?\+[0-9]+[dwmy]>|<%%(?:\([^>\n]+\))([^\n>]*)>)",
);
static TIMESTAMP_RAW: Lazy =
    Lazy::new(r"(?i)([<\[](%%)?.*?)[\]>](?:--([\[<]([0-9]{4}-[0-9]{2}-[0-9]{2}(?: .*?)?)[\]>]))?");
static LINK_BRACKET: Lazy =
    Lazy::new(r"\[\[((?:[^\[\]\\]|\\(?:\\\\)*[\[\]]|\\+[^\[\]])+)\](?:\[((?s:.)+?)\])?\]");
static CITATION_PREFIX_RE: Lazy = Lazy::new(r"(?i)\[cite(?:/([/_\-{AN}]+))?:[\t\n ]*");
static CITATION_KEY: Lazy = Lazy::new(r"@([{W}\-.:?!`'/*@+|(){}<>&_^$#%~]+)");
static TABLE_CELL_RE: Lazy = Lazy::new(r"(?m)[ \t]*(.*?)[ \t]*(?:\||$)");

/// `org-match-substring-regexp`, with `org-match-sexp-depth` 3.
fn match_substring() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        let multibrace = |l: &str, r: &str| {
            let nothing = format!("[^{l}{r}]*?");
            let mut re = nothing.clone();
            let mut next = format!("(?:{nothing}{l}{nothing}{r})+{nothing}");
            for _ in 0..2 {
                re = format!("{re}|{next}");
                next = format!("(?:{nothing}{l}{next}{r})+{nothing}");
            }
            format!("{l}({re}){r}")
        };
        let pattern = format!(
            r"([^{{S}}])([_^])((?:{})|(?:{})|(?:\*|[+-]?[{{AN}}.,\\]*[{{AN}}]))",
            multibrace(r"\{", r"\}"),
            multibrace(r"\(", r"\)")
        );
        re::compile(&pattern)
    })
}

// ------------------------------------------------------------------------
// The object loop.

/// `org-element--parse-objects` between `beg` and `end`.
pub(crate) fn parse_objects(p: &Parser<'_>, beg: usize, end: usize, r: Restriction) -> Vec<Raw> {
    let depth = p.depth.get();
    if depth >= crate::elements::MAX_DEPTH {
        return Vec::new();
    }
    p.depth.set(depth + 1);
    let out = crate::deep(|| parse_objects_inner(p, beg, end, r));
    p.depth.set(depth);
    out
}

fn parse_objects_inner(p: &Parser<'_>, beg: usize, end: usize, r: Restriction) -> Vec<Raw> {
    let b = p.buf.narrowed(beg, end);
    let mut out = Vec::new();
    let mut pos = beg;
    let mut radio = None;
    while pos < end {
        let Some(mut obj) = object_lex(p, &b, pos, r, &mut radio) else {
            break;
        };
        if obj.begin < pos || obj.end <= pos || obj.end > end {
            // Defensive: never loop and never leave the container.
            break;
        }
        if let (Some(cb), Some(ce)) = (obj.cb, obj.ce)
            && let Some(inner) = restriction_for(obj.kind)
            && cb < ce
        {
            let kids = parse_objects(p, cb, ce, inner);
            obj.children.extend(kids);
        }
        pos = obj.end;
        out.push(obj);
    }
    out
}

/// Emacs's `\<`: a word-constituent character that does not continue a
/// word (see `tables::word_boundary`).
fn is_word_start(b: &Buf<'_>, p: usize) -> bool {
    match (b.char_before(p), b.char_at(p)) {
        (_, None) => false,
        (_, Some(c)) if !tables::is_word(c) => false,
        (None, Some(_)) => true,
        (Some(prev), Some(c)) => !tables::is_word(prev) || tables::word_boundary(prev, c),
    }
}

/// Finds the next position where `org-element--object-regexp` matches,
/// from `p`, with match end at or before `bound`.
fn next_candidate(ctx: &ContextRegexes, b: &Buf<'_>, mut p: usize, bound: usize) -> Option<usize> {
    let bytes = b.b;
    while p < bound {
        let c = bytes[p];
        let hit = match c {
            b'_' | b'^' | b'*' | b'~' | b'=' | b'+' | b'/' => {
                let nc = b.char_at(p + 1);
                let n = nc.map_or(0, |c| c.len_utf8());
                p + 1 + n <= bound
                    && nc.is_some_and(|nc| {
                        let subsup = matches!(c, b'_' | b'^')
                            && (matches!(nc, '-' | '{' | '(' | '*' | '+' | '.' | ',')
                                || tables::is_alnum(nc));
                        let emph = c != b'^' && !tables::is_space(nc);
                        subsup || emph
                    })
            }
            b'[' => {
                let rest = &bytes[p + 1..bound];
                let ci = |s: &str| {
                    rest.len() >= s.len() && rest[..s.len()].eq_ignore_ascii_case(s.as_bytes())
                };
                ci("cite:")
                    || ci("cite/")
                    || ci("fn:")
                    || rest
                        .first()
                        .is_some_and(|c| c.is_ascii_digit() || *c == b'[')
                    || ci("%]")
                    || (rest.first() == Some(&b'/') && {
                        let mut i = 1;
                        while i < rest.len() && rest[i].is_ascii_digit() {
                            i += 1;
                        }
                        rest.get(i) == Some(&b']')
                    })
            }
            b'@' => p + 2 <= bound && bytes.get(p + 1) == Some(&b'@'),
            b'{' => {
                p + 3 <= bound && bytes.get(p + 1) == Some(&b'{') && bytes.get(p + 2) == Some(&b'{')
            }
            b'<' => {
                let n = bytes.get(p + 1).copied();
                p + 2 <= bound
                    && (matches!(n, Some(b'<'))
                        || n.is_some_and(|c| c.is_ascii_digit())
                        || (n == Some(b'%') && bytes.get(p + 2) == Some(&b'%') && p + 3 <= bound)
                        || ctx.type_at(b, p + 1, false).is_some_and(|e| e <= bound))
            }
            b'$' => true,
            b'\\' => match bytes.get(p + 1).copied() {
                Some(c)
                    if (c.is_ascii_alphabetic() || c == b'[' || c == b'(') && p + 2 <= bound =>
                {
                    true
                }
                Some(b'\\') => {
                    let q = b.skip_blank(p + 2);
                    b.is_eol(q) && q <= bound
                }
                Some(b'_') => bytes.get(p + 2) == Some(&b' ') && p + 3 <= bound,
                _ => false,
            },
            c if c.is_ascii_alphabetic() || c >= 0x80 => {
                let low = c.to_ascii_lowercase();
                let plain =
                    is_word_start(b, p) && ctx.type_at(b, p, true).is_some_and(|e| e < bound);
                plain
                    || (low == b'c' && b.looking_at_ci(p, "call_") && p + 5 <= bound)
                    || (low == b's' && b.looking_at_ci(p, "src_") && p + 4 <= bound)
            }
            _ => false,
        };
        if hit {
            return Some(p);
        }
        p = b.next_char(p);
    }
    None
}

/// The last radio link search of an object loop: where it started and what
/// it found (see `radio_search`).
type RadioMemo = Option<(usize, Option<(usize, usize, usize)>)>;

/// `org-element--object-lex`. `memo` keeps the radio link search between
/// the calls of one object loop.
fn object_lex(
    p: &Parser<'_>,
    b: &Buf<'_>,
    start: usize,
    r: Restriction,
    memo: &mut RadioMemo,
) -> Option<Raw> {
    if r.has(TABLE_CELL) {
        return table_cell(b, start);
    }
    if r.has(CITATION_REFERENCE) {
        return citation_reference(p, b, start);
    }
    let rx = p.ctx.regexes();
    // Radio links: a hard limit one character after the next radio link.
    let limit = match &rx.radio {
        Some(radio) if r.has(LINK) => {
            let q = if b.is_bol(start) {
                start
            } else {
                b.prev_char(start)
            };
            // A search from a later position finds the same target while
            // it starts before that target, and nothing when the earlier
            // search found nothing: every position the earlier one passed
            // fails again (a line start, required where a search starts,
            // is also a boundary). Without this, each call would search to
            // the end of the container.
            let first = match *memo {
                Some((q0, found)) if q0 <= q && found.is_none_or(|(s1, _, _)| q < s1) => found,
                _ => {
                    let found = radio_search(radio, b, q);
                    *memo = Some((q, found));
                    found
                }
            };
            match first {
                None => None,
                Some((s1, e1, pt)) => {
                    // `start == e1` first: `bol` reads back to the line
                    // start, the whole paragraph on a long line.
                    if start == e1 && start == b.next_char(b.bol(pt)) {
                        radio_search(radio, b, pt).map(|(s2, _, _)| b.next_char(s2))
                    } else {
                        Some(b.next_char(s1))
                    }
                }
            }
        }
        _ => None,
    };
    let bound = limit.unwrap_or(b.zv);
    let mut pos = start;
    let found = loop {
        let Some(c) = next_candidate(rx, b, pos, bound) else {
            break None;
        };
        if let Some(obj) = try_object(p, b, c, r) {
            break Some(obj);
        }
        if b.is_eob(c) {
            break None;
        }
        pos = b.next_char(c);
    };
    match found {
        Some(o) => Some(o),
        None => limit.and_then(|l| link(p, b, b.prev_char(l))),
    }
}

/// The part of `org-target-link-regexp` before the target:
/// `\(?:^\|[^[:alnum:]]\|\c|\)`, true when a target may start at `p`.
fn radio_boundary_before(b: &Buf<'_>, p: usize) -> bool {
    match b.char_before(p) {
        None | Some('\n') => true,
        Some(c) => !tables::is_alnum(c) || tables::bits(c) & tables::LINE_BREAKABLE != 0,
    }
}

/// `(re-search-forward org-target-link-regexp nil t)` from `q`: the
/// target's start and end, and the end of the whole match.
fn radio_search(
    radio: &crate::radio::RadioMatcher,
    b: &Buf<'_>,
    q: usize,
) -> Option<(usize, usize, usize)> {
    let mut p = q;
    while p <= b.zv {
        // The boundary (a character before the target, or a line start)
        // must lie at or after Q, as in the full regexp.
        let ok = if p == q {
            b.is_bol(p)
        } else {
            radio_boundary_before(b, p)
        };
        if ok && let Some((e1, end)) = radio.match_at(b.s, p, b.zv) {
            return Some((p, e1, end));
        }
        if p >= b.zv {
            return None;
        }
        p = b.next_char(p);
    }
    None
}

/// Dispatches on the text at a candidate position.
fn try_object(p: &Parser<'_>, b: &Buf<'_>, c: usize, r: Restriction) -> Option<Raw> {
    if b.looking_at_ci(c, "call_") {
        return if r.has(INLINE_BABEL_CALL) {
            inline_babel_call(p, b, c)
        } else {
            None
        };
    }
    if b.looking_at_ci(c, "src_") {
        return if r.has(INLINE_SRC_BLOCK) {
            inline_src_block(p, b, c)
        } else {
            None
        };
    }
    let second = b.byte(c + 1);
    match b.byte(c)? {
        b'^' => {
            if r.has(SUPERSCRIPT) {
                sub_superscript(b, c, SUPERSCRIPT)
            } else {
                None
            }
        }
        b'_' => (if r.has(UNDERLINE) {
            emphasis(p, b, c, b'_', UNDERLINE)
        } else {
            None
        })
        .or_else(|| {
            if r.has(SUBSCRIPT) {
                sub_superscript(b, c, SUBSCRIPT)
            } else {
                None
            }
        }),
        b'*' => {
            if r.has(BOLD) {
                emphasis(p, b, c, b'*', BOLD)
            } else {
                None
            }
        }
        b'/' => {
            if r.has(ITALIC) {
                emphasis(p, b, c, b'/', ITALIC)
            } else {
                None
            }
        }
        b'~' => {
            if r.has(CODE) {
                emphasis(p, b, c, b'~', CODE)
            } else {
                None
            }
        }
        b'=' => {
            if r.has(VERBATIM) {
                emphasis(p, b, c, b'=', VERBATIM)
            } else {
                None
            }
        }
        b'+' => {
            if r.has(STRIKE_THROUGH) {
                emphasis(p, b, c, b'+', STRIKE_THROUGH)
            } else {
                None
            }
        }
        b'@' => {
            if r.has(EXPORT_SNIPPET) {
                export_snippet(b, c)
            } else {
                None
            }
        }
        b'{' => {
            if r.has(MACRO) {
                macro_(b, c)
            } else {
                None
            }
        }
        b'$' => {
            if r.has(LATEX_FRAGMENT) {
                latex_fragment(b, c)
            } else {
                None
            }
        }
        b'<' => {
            if second == Some(b'<') {
                (if r.has(RADIO_TARGET) {
                    radio_target(p, b, c)
                } else {
                    None
                })
                .or_else(|| if r.has(TARGET) { target(b, c) } else { None })
            } else {
                (if r.has(TIMESTAMP) {
                    timestamp(b, c)
                } else {
                    None
                })
                .or_else(|| if r.has(LINK) { link(p, b, c) } else { None })
            }
        }
        b'\\' => {
            if second == Some(b'\\') {
                if r.has(LINE_BREAK) {
                    line_break(b, c)
                } else {
                    None
                }
            } else {
                (if r.has(ENTITY) { entity(b, c) } else { None }).or_else(|| {
                    if r.has(LATEX_FRAGMENT) {
                        latex_fragment(b, c)
                    } else {
                        None
                    }
                })
            }
        }
        b'[' => match second {
            Some(b'[') if r.has(LINK) => link(p, b, c),
            Some(b'f') if r.has(FOOTNOTE_REFERENCE) => footnote_reference(p, b, c),
            Some(b'c') if r.has(CITATION) => citation(p, b, c),
            Some(b'%' | b'/') if r.has(STATISTICS_COOKIE) => statistics_cookie(b, c),
            _ => (if r.has(TIMESTAMP) {
                timestamp(b, c)
            } else {
                None
            })
            .or_else(|| {
                if r.has(STATISTICS_COOKIE) {
                    statistics_cookie(b, c)
                } else {
                    None
                }
            }),
        },
        _ => {
            if r.has(LINK) {
                link(p, b, c)
            } else {
                None
            }
        }
    }
}

fn looking_at(re: &Lazy, b: &Buf<'_>, p: usize) -> Option<re::Captures> {
    re::looking_at(re.get(), b.s, b.begv, b.zv, p)
}

/// Emits trailing spaces and tabs as post-blank and returns the new end.
fn post_blank(b: &Buf<'_>, p: usize) -> usize {
    b.skip_blank(p)
}

// ------------------------------------------------------------------------
// Object parsers.

/// `org-element--parse-generic-emphasis`.
fn emphasis(p: &Parser<'_>, b: &Buf<'_>, origin: usize, mark: u8, kind: SyntaxKind) -> Option<Raw> {
    let non_space_at = |q: usize| b.char_at(q).is_some_and(|c| !tables::is_space(c));
    let opening = if b.is_bol(origin) {
        b.byte(origin) == Some(mark) && non_space_at(origin + 1)
    } else {
        let q = b.prev_char(origin);
        let pre = b.char_at(q).expect("char before origin");
        (b.is_bol(q) && b.byte(q) == Some(mark) && non_space_at(q + 1))
            || ((tables::is_space(pre) || matches!(pre, '-' | '(' | '\'' | '"' | '{'))
                && b.byte(origin) == Some(mark)
                && non_space_at(origin + 1))
    };
    if !opening {
        return None;
    }
    // Closing: `(not space) (group MARK) (or (any space - . , ; : ! ? ' " ) } \\ [) line-end)`,
    // searched through the index of closing candidates.
    let closing = p.index.closer(b, origin + 1, mark)?;
    let end = post_blank(b, closing);
    let mut o = Raw::new(kind, origin, end).pb(end - closing);
    o.tok(MARKER, origin, origin + 1);
    o.tok(MARKER, closing - 1, closing);
    if matches!(kind, CODE | VERBATIM) {
        o.tok(CODE_TEXT, origin + 1, closing - 1);
    } else {
        o.cb = Some(origin + 1);
        o.ce = Some(closing - 1);
    }
    Some(o)
}

/// Sub- and superscript parsers.
fn sub_superscript(b: &Buf<'_>, c: usize, kind: SyntaxKind) -> Option<Raw> {
    let q = if b.is_bol(c) {
        if kind == SUBSCRIPT {
            return None;
        }
        c
    } else {
        b.prev_char(c)
    };
    let m = re::looking_at(match_substring(), b.s, b.begv, b.zv, q)?;
    let begin = m.start(2)?;
    let (cb, ce) = m.get(4).or(m.get(3))?;
    let end0 = m.whole().1;
    let end = post_blank(b, end0);
    let mut o = Raw::new(kind, begin, end)
        .contents(Some(cb), Some(ce))
        .pb(end - end0);
    o.tok(MARKER, begin, begin + 1);
    if m.get(4).is_some() {
        o.tok(MARKER, cb - 1, cb);
        o.tok(MARKER, ce, ce + 1);
    }
    Some(o)
}

/// `org-element-entity-parser`.
fn entity(b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&ENTITY_RE, b, c)?;
    let (ns, ne) = m.get(1).or(m.get(2))?;
    tables::entity(b.slice(ns, ne))?;
    let brackets = m.get(3).is_some_and(|(s, e)| &b.s[s..e] == "{}");
    let mut e = ne;
    if brackets {
        e += 2;
    }
    let end = post_blank(b, e);
    let mut o = Raw::new(ENTITY, c, end).pb(end - e);
    o.tok(MARKER, c, c + 1);
    o.tok(KEY, ns, ne);
    if brackets {
        o.tok(MARKER, ne, ne + 2);
    }
    Some(o)
}

/// `org-element-export-snippet-parser`.
fn export_snippet(b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&EXPORT_SNIPPET_RE, b, c)?;
    let (bs, be) = m.get(1)?;
    let vb = m.whole().1;
    let close = b.s[vb..b.zv].find("@@").map(|i| vb + i)?;
    let end = post_blank(b, close + 2);
    let mut o = Raw::new(EXPORT_SNIPPET, c, end).pb(end - close - 2);
    o.tok(MARKER, c, bs);
    o.tok(KEY, bs, be);
    o.tok(MARKER, be, vb);
    o.tok(CODE_TEXT, vb, close);
    o.tok(MARKER, close, close + 2);
    Some(o)
}

/// `org-element-footnote-reference-parser`.
fn footnote_reference(p: &Parser<'_>, b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&FOOTNOTE_REF, b, c)?;
    let closing = p.index.match_bracket(b, c, b'[', b']')?;
    let inline = m.get(2).is_some();
    let end = post_blank(b, closing);
    let mut o = Raw::new(FOOTNOTE_REFERENCE, c, end).pb(end - closing);
    let inner_begin = m.whole().1;
    o.tok(MARKER, c, c + 4);
    if let Some((ls, le)) = m.get(1).or(m.get(3)) {
        o.tok(KEY, ls, le);
    }
    if inline {
        o.tok(MARKER, inner_begin - 1, inner_begin);
        o.cb = Some(inner_begin);
        o.ce = Some(closing - 1);
    }
    o.tok(MARKER, closing - 1, closing);
    Some(o)
}

/// `org-element--parse-paired-brackets`.
fn paired(parser: &Parser<'_>, b: &Buf<'_>, p: usize, open: u8) -> Option<(usize, usize)> {
    let close = match open {
        b'[' => b']',
        b'(' => b')',
        b'{' => b'}',
        _ => return None,
    };
    let e = parser.index.match_bracket(b, p, open, close)?;
    Some((p + 1, e - 1))
}

fn nw(b: &Buf<'_>, s: usize, e: usize) -> bool {
    b.s[s..e]
        .chars()
        .any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
}

/// `org-element-inline-babel-call-parser`.
fn inline_babel_call(parser: &Parser<'_>, b: &Buf<'_>, c: usize) -> Option<Raw> {
    if !b.looking_at_str(c, "call_") || !is_word_start(b, c) {
        return None;
    }
    let m = looking_at(&INLINE_CALL, b, c)?;
    let (ns, ne) = m.get(1)?;
    let mut p = ne;
    if let Some((_, e)) = paired(parser, b, p, b'[') {
        p = e + 1;
    }
    let (as_, ae) = paired(parser, b, p, b'(')?;
    p = ae + 1;
    let _ = nw(b, as_, ae);
    if let Some((_, e)) = paired(parser, b, p, b'[') {
        p = e + 1;
    }
    let end = post_blank(b, p);
    let mut o = Raw::new(INLINE_BABEL_CALL, c, end).pb(end - p);
    o.tok(MARKER, c, c + 5);
    o.tok(KEY, ns, ne);
    Some(o)
}

/// `org-element-inline-src-block-parser`.
fn inline_src_block(parser: &Parser<'_>, b: &Buf<'_>, c: usize) -> Option<Raw> {
    if !b.looking_at_str(c, "src_") || !is_word_start(b, c) {
        return None;
    }
    let m = looking_at(&INLINE_SRC, b, c)?;
    let (ns, ne) = m.get(1)?;
    let mut p = ne;
    if let Some((_, e)) = paired(parser, b, p, b'[') {
        p = e + 1;
    }
    let (vs, ve) = paired(parser, b, p, b'{')?;
    p = ve + 1;
    let end = post_blank(b, p);
    let mut o = Raw::new(INLINE_SRC_BLOCK, c, end).pb(end - p);
    o.tok(MARKER, c, c + 4);
    o.tok(KEY, ns, ne);
    o.tok(MARKER, vs - 1, vs);
    o.tok(CODE_TEXT, vs, ve);
    o.tok(MARKER, ve, ve + 1);
    Some(o)
}

/// `org-element-latex-fragment-parser`.
fn latex_fragment(b: &Buf<'_>, c: usize) -> Option<Raw> {
    let s = b.s;
    let zv = b.zv;
    let after: Option<usize> = if b.byte(c) != Some(b'$') {
        match b.byte(c + 1) {
            Some(b'(') => s[c..zv].find("\\)").map(|i| c + i + 2),
            Some(b'[') => s[c..zv].find("\\]").map(|i| c + i + 2),
            _ => looking_at(&LATEX_MACRO, b, c).map(|m| m.whole().1),
        }
    } else if b.byte(c + 1) == Some(b'$') {
        // `(search-forward "$$" nil t 2)`: the second occurrence.
        s[c + 2..zv].find("$$").map(|i| c + 2 + i + 2)
    } else {
        let ok_before = b.char_before(c) != Some('$');
        let ok_after = !matches!(
            b.byte(c + 1),
            Some(b' ' | b'\t' | b'\n' | b',' | b'.' | b';')
        );
        if !(ok_before && ok_after) {
            None
        } else {
            let close = s[c + 1..zv].find('$').map(|i| c + 1 + i)?;
            let before_close = b.char_before(close);
            if matches!(before_close, Some(' ' | '\t' | '\n' | ',' | '.')) {
                None
            } else {
                let q = close + 1;
                let ok = match b.char_at(q) {
                    None => true,
                    Some('\n') => true,
                    Some(ch) => {
                        let syn = tables::syntax(ch);
                        syn == tables::SYNTAX_PUNCT
                            || syn == tables::SYNTAX_WHITESPACE
                            || syn == tables::SYNTAX_OPEN
                            || syn == tables::SYNTAX_CLOSE
                            || syn == tables::SYNTAX_STRING
                            || ch == '\''
                    }
                };
                ok.then_some(q)
            }
        }
    };
    let after = after?;
    let end = post_blank(b, after);
    let mut o = Raw::new(LATEX_FRAGMENT, c, end).pb(end - after);
    o.tok(CODE_TEXT, c, after);
    Some(o)
}

/// `org-element-line-break-parser`.
fn line_break(b: &Buf<'_>, c: usize) -> Option<Raw> {
    if b.byte(c) != Some(b'\\')
        || b.byte(c + 1) != Some(b'\\')
        || !b.rest_is_blank(c + 2)
        || b.char_before(c) == Some('\\')
    {
        return None;
    }
    let end = b.lbp2(c);
    let mut o = Raw::new(LINE_BREAK, c, end);
    o.tok(MARKER, c, c + 2);
    Some(o)
}

/// `org-element-link-parser`.
pub(crate) fn link(p: &Parser<'_>, b: &Buf<'_>, c: usize) -> Option<Raw> {
    let rx = p.ctx.regexes();
    // Type 1: radio link.
    if let Some(radio) = &rx.radio
        && radio_boundary_before(b, c)
        && let Some((e1, _)) = radio.match_at(b.s, c, b.zv)
    {
        let s1 = c;
        let end = post_blank(b, e1);
        return Some(
            Raw::new(LINK, c, end)
                .contents(Some(s1), Some(e1))
                .pb(end - e1),
        );
    }
    // Type 2: bracket link.
    if let Some(m) = looking_at(&LINK_BRACKET, b, c) {
        let (ps, pe) = m.get(1)?;
        let link_end = m.whole().1;
        let end = post_blank(b, link_end);
        let mut o = Raw::new(LINK, c, end).pb(end - link_end);
        o.tok(MARKER, c, c + 2);
        o.tok(CODE_TEXT, ps, pe);
        if let Some((ds, de)) = m.get(2) {
            o.tok(MARKER, pe, pe + 2);
            o.cb = Some(ds);
            o.ce = Some(de);
            o.tok(MARKER, de, de + 2);
        } else {
            o.tok(MARKER, pe, pe + 2);
        }
        return Some(o);
    }
    // Type 3: plain link.
    if is_word_start(b, c)
        && let Some(m) = re::looking_at(&rx.plain_link, b.s, b.begv, b.zv, c)
    {
        let link_end = m.whole().1;
        let end = post_blank(b, link_end);
        let mut o = Raw::new(LINK, c, end).pb(end - link_end);
        o.tok(CODE_TEXT, c, link_end);
        return Some(o);
    }
    // Type 4: angle link.
    if let Some(m) = re::looking_at(&rx.angle_link, b.s, b.begv, b.zv, c) {
        let link_end = m.whole().1;
        let end = post_blank(b, link_end);
        let mut o = Raw::new(LINK, c, end).pb(end - link_end);
        o.tok(MARKER, c, c + 1);
        o.tok(CODE_TEXT, c + 1, link_end - 1);
        o.tok(MARKER, link_end - 1, link_end);
        return Some(o);
    }
    None
}

/// `org-element-macro-parser`.
fn macro_(b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&MACRO_RE, b, c)?;
    let (ks, ke) = m.get(1)?;
    let e = m.whole().1;
    let end = post_blank(b, e);
    let mut o = Raw::new(MACRO, c, end).pb(end - e);
    o.tok(MARKER, c, c + 3);
    o.tok(KEY, ks, ke);
    o.tok(MARKER, e - 3, e);
    Some(o)
}

/// `org-element-radio-target-parser`.
fn radio_target(_p: &Parser<'_>, b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&RADIO_TARGET_RE, b, c)?;
    let (s1, e1) = m.get(1)?;
    let e = m.whole().1;
    let end = post_blank(b, e);
    let mut o = Raw::new(RADIO_TARGET, c, end)
        .contents(Some(s1), Some(e1))
        .pb(end - e);
    o.tok(MARKER, c, s1);
    o.tok(MARKER, e1, e);
    Some(o)
}

/// `org-element-target-parser`.
fn target(b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&TARGET_RE, b, c)?;
    let (s1, e1) = m.get(1)?;
    let e = m.whole().1;
    let end = post_blank(b, e);
    let mut o = Raw::new(TARGET, c, end).pb(end - e);
    o.tok(MARKER, c, s1);
    o.tok(MARKER, e1, e);
    Some(o)
}

/// `org-element-statistics-cookie-parser`.
fn statistics_cookie(b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&COOKIE, b, c)?;
    let e = m.whole().1;
    let end = post_blank(b, e);
    Some(Raw::new(STATISTICS_COOKIE, c, end).pb(end - e))
}

/// `org-element-timestamp-parser`, usable at element level (planning,
/// clock) with the unnarrowed buffer.
pub(crate) fn timestamp_at(_p: &Parser<'_>, b: &Buf<'_>, c: usize) -> Option<Raw> {
    timestamp(b, c)
}

fn timestamp(b: &Buf<'_>, c: usize) -> Option<Raw> {
    if !re::looking_at_p(TIMESTAMP_ANY.get(), b.s, b.begv, b.zv, c) {
        return None;
    }
    let m = looking_at(&TIMESTAMP_RAW, b, c)?;
    let e = m.whole().1;
    let end = post_blank(b, e);
    Some(Raw::new(TIMESTAMP, c, end).pb(end - e))
}

/// `org-element-citation-parser`.
fn citation(p: &Parser<'_>, b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&CITATION_PREFIX_RE, b, c)?;
    let start = m.whole().1;
    let closing = p.index.match_bracket(b, c, b'[', b']')?;
    let km = re::search_forward(CITATION_KEY.get(), b.s, b.begv, b.zv, start, closing)?;
    let first_key_end = km.whole().1;
    let end = post_blank(b, closing);
    let mut o = Raw::new(CITATION, c, end).pb(end - closing);
    o.tok(MARKER, c, start);
    o.tok(MARKER, closing - 1, closing);
    let restr = restriction_for(CITATION_REFERENCE).expect("citation-reference restriction");
    // Common prefix.
    let semi = b.s[start..first_key_end].rfind(';').map(|i| start + i);
    match semi {
        None => o.cb = Some(start),
        Some(sp) => {
            if start < sp {
                let mut n = Raw::new(CITATION_PREFIX, start, sp).contents(Some(start), Some(sp));
                n.children = p.parse_objects(start, sp, restr);
                o.child(n);
            }
            o.tok(MARKER, sp, sp + 1);
            o.cb = Some(sp + 1);
        }
    }
    // Common suffix.
    let e = b.skip_bwd(closing - 1, b" \r\t\n", 0);
    let last_semi = b.s[first_key_end..e.max(first_key_end)]
        .rfind(';')
        .map(|i| first_key_end + i);
    let key_after =
        last_semi.and_then(|sp| re::search_forward(CITATION_KEY.get(), b.s, b.begv, b.zv, sp, e));
    match (last_semi, key_after) {
        (None, _) | (_, Some(_)) => o.ce = Some(e),
        (Some(sp), None) => {
            let q = sp + 1;
            if q < e {
                let mut n = Raw::new(CITATION_SUFFIX, q, e).contents(Some(q), Some(e));
                n.children = p.parse_objects(q, e, restr);
                o.child(n);
            }
            o.ce = Some(q);
        }
    }
    Some(o)
}

/// `org-element-citation-reference-parser`.
fn citation_reference(p: &Parser<'_>, b: &Buf<'_>, begin: usize) -> Option<Raw> {
    let m = re::search_forward(CITATION_KEY.get(), b.s, b.begv, b.zv, begin, b.zv)?;
    let (key_start, key_end) = m.whole();
    let sep = b.s[key_end..b.zv].find(';').map(|i| key_end + i + 1);
    let end = sep.unwrap_or(b.zv);
    let suffix_end = if sep.is_some() { end - 1 } else { end };
    let restr = restriction_for(CITATION_REFERENCE).expect("citation-reference restriction");
    let mut o = Raw::new(CITATION_REFERENCE, begin, end);
    if begin < key_start {
        let mut n =
            Raw::new(CITATION_PREFIX, begin, key_start).contents(Some(begin), Some(key_start));
        n.children = p.parse_objects(begin, key_start, restr);
        o.child(n);
    }
    o.tok(MARKER, key_start, key_start + 1);
    o.tok(KEY, key_start + 1, key_end);
    if key_end < suffix_end {
        let mut n = Raw::new(CITATION_SUFFIX, key_end, suffix_end)
            .contents(Some(key_end), Some(suffix_end));
        n.children = p.parse_objects(key_end, suffix_end, restr);
        o.child(n);
    }
    if sep.is_some() {
        o.tok(MARKER, end - 1, end);
    }
    Some(o)
}

/// `org-element-table-cell-parser`.
fn table_cell(b: &Buf<'_>, c: usize) -> Option<Raw> {
    let m = looking_at(&TABLE_CELL_RE, b, c)?;
    let (s, e) = m.whole();
    let (cs, ce) = m.get(1)?;
    let mut o = Raw::new(TABLE_CELL, s, e).contents(Some(cs), Some(ce));
    if e > ce && b.byte(e - 1) == Some(b'|') {
        o.tok(MARKER, e - 1, e);
    }
    Some(o)
}
