//! Element-level parsing, following `org-element.el` function by function.
//!
//! Function names mirror their Emacs counterparts: `headline` is
//! `org-element-headline-parser`, `current_element` is
//! `org-element--current-element`, and so on. Comments quote the Emacs
//! behavior where it is not obvious.

use std::rc::Rc;

use crate::SyntaxKind::{self, *};
use crate::buf::Buf;
use crate::context::ParseContext;
use crate::lists::{self, ListStruct};
use crate::objects::{self, Restriction};
use crate::raw::Raw;
use crate::re::{self, Lazy};
use crate::tables;

/// `org-element--next-mode` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    None,
    FirstSection,
    Section,
    TopComment,
    Planning,
    PropertyDrawer,
    NodeProperty,
    Item,
    TableRow,
}

pub(crate) struct Parser<'a> {
    pub(crate) buf: Buf<'a>,
    pub(crate) ctx: &'a ParseContext,
    /// Parse objects (granularity `object`). The pre-pass turns this off.
    pub(crate) objects: bool,
    /// Search indexes over the buffer.
    pub(crate) index: crate::cache::Index,
    /// Current nesting depth of elements and objects.
    pub(crate) depth: std::cell::Cell<usize>,
}

/// The deepest nesting of elements and objects that is parsed. Emacs's own
/// parser gives up after a few hundred levels (`max-lisp-eval-depth`);
/// deeper content stays in the tree as plain text. The cap keeps every
/// recursive consumer of the tree safe (design document, section 3.6).
pub const MAX_DEPTH: usize = 4096;

/// Affiliated keywords collected before an element: the position of the
/// first one and their nodes.
pub(crate) struct Affiliated {
    pub(crate) begin: usize,
    pub(crate) nodes: Vec<Raw>,
}

impl Affiliated {
    fn at(pos: usize) -> Self {
        Affiliated {
            begin: pos,
            nodes: Vec::new(),
        }
    }
}

static CLOCK_LINE: Lazy = Lazy::starting(
    r"(?mi)^[ \t]*CLOCK:(?:[ \t]+\[[0-9]{4}-[0-9]{2}-[0-9]{2}(?: .*?)?\](?:--\[[0-9]{4}-[0-9]{2}-[0-9]{2}(?: .*?)?\][ \t]+=>[ \t]+[0-9]+:[0-9][0-9])?|[ \t]+=>[ \t]+[0-9]+:[0-9][0-9])[ \t]*$",
    b"Cc",
    true,
);
static AFFILIATED: Lazy = Lazy::starting(
    r"(?i)#\+(?:(CAPTION|RESULTS)(?:\[(.*)\])?|(DATA|HEADERS|HEADER|LABEL|NAME|PLOT|RESNAME|RESULT|SOURCE|SRCNAME|TBLNAME)|(ATTR_[-_A-Za-z0-9]+)):[ \t]*",
    b"#",
    false,
);
static LATEX_BEGIN: Lazy = Lazy::starting(r"(?mi)\\begin\{([A-Za-z0-9*]+)\}", b"\\", false);
static DRAWER_LINE: Lazy = Lazy::starting(r"(?mi):([-_{W}]+):[ \t]*$", b":", false);
static DYNAMIC_OPEN: Lazy =
    Lazy::starting(r"(?mi)#\+BEGIN:[ \t]*([{W}]+)(?:[ \t]+(.+))?", b"#", false);
static HASH_PLUS: Lazy = Lazy::starting(
    r"(?i)#\+(?:BEGIN_([^{S}]+)|(CALL:)|([^{S}]+):)",
    b"#",
    false,
);
static FOOTNOTE_DEF: Lazy = Lazy::starting(r"(?mi)^\[fn:([-_{W}]+)\]", b"[", false);
static TABLE_RULE: Lazy = Lazy::starting(r"(?m)[ \t]*\+(?:-+\+)+[ \t]*$", b"+", true);
static NON_TABLE_EL_LINE: Lazy = Lazy::new(r"(?m)^[ \t]*(?:$|[^+| \t])");
static KEYWORD_LINE: Lazy = Lazy::starting(r"(?i)#\+([^{S}]*):", b"#", false);
static SRC_BEGIN: Lazy = Lazy::new(
    r#"(?mi)^[ \t]*#\+BEGIN_SRC(?: +([^{S}]+))?((?: +(?:-(?:l ".+"|[ikr])|[-+]n(?: *[0-9]+)?))+)?(.*)[ \t]*$"#,
);
static EXAMPLE_BEGIN: Lazy = Lazy::new(r"(?mi)^[ \t]*#\+BEGIN_EXAMPLE(?: +(.*))?");
static EXPORT_BEGIN: Lazy = Lazy::new(r"(?mi)[ \t]*#\+BEGIN_EXPORT(?:[ \t]+([^{S}]+))?[ \t]*$");
static SPECIAL_BEGIN: Lazy =
    Lazy::starting(r"(?mi)#\+BEGIN_([^{S}]+)[ \t]*(.*)[ \t]*$", b"#", false);
static TBLFM_LINE: Lazy = Lazy::starting(r"(?mi)#\+TBLFM: +(.*)[ \t]*$", b"#", false);
static PROPERTY_LINE: Lazy =
    Lazy::starting(r"(?m)():([^{S}]+):(?:$|[ \t]+(.*?))([ \t]*)$", b":", false);
static DIARY_LINE: Lazy = Lazy::new(r"(?m)(%%\(.*)[ \t]*$");

/// Whether [`AFFILIATED`] can match at `q`: the word after `#+` must be
/// one of its keywords. Checked only for ASCII words, where Unicode case
/// folding is ASCII's; others go to the regex.
fn affiliated_candidate(b: &[u8], q: usize) -> bool {
    const NAMES: [&[u8]; 13] = [
        b"CAPTION", b"RESULTS", b"DATA", b"HEADERS", b"HEADER", b"LABEL", b"NAME", b"PLOT",
        b"RESNAME", b"RESULT", b"SOURCE", b"SRCNAME", b"TBLNAME",
    ];
    if b.get(q..q + 2) != Some(b"#+") {
        return false;
    }
    let rest = &b[q + 2..];
    let len = rest
        .iter()
        .position(|&c| matches!(c, b':' | b'[' | b'\n'))
        .unwrap_or(rest.len());
    let word = &rest[..len];
    if !word.is_ascii() {
        return true;
    }
    NAMES.iter().any(|n| word.eq_ignore_ascii_case(n))
        || (word.len() > 5
            && word[..5].eq_ignore_ascii_case(b"ATTR_")
            && word[5..]
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')))
}

impl<'a> Parser<'a> {
    pub(crate) fn new(text: &'a str, ctx: &'a ParseContext) -> Self {
        Parser {
            buf: Buf::new(text),
            ctx,
            objects: true,
            index: Default::default(),
            depth: Default::default(),
        }
    }

    /// A parser that stops at elements (the pre-pass uses it).
    pub(crate) fn elements_only(text: &'a str, ctx: &'a ParseContext) -> Self {
        Parser {
            buf: Buf::new(text),
            ctx,
            objects: false,
            index: Default::default(),
            depth: Default::default(),
        }
    }

    // ----------------------------------------------------------------
    // Small matchers.

    /// `looking-at`. Every element-level regex is line-local.
    fn looking_at(&self, re: &Lazy, p: usize) -> Option<re::Captures> {
        if !re.may_start(self.buf.b, p) {
            return None;
        }
        re::looking_at_line(re.get(), self.buf.s, self.buf.begv, self.buf.zv, p)
    }

    fn looking_at_p(&self, re: &Lazy, p: usize) -> bool {
        re.may_start(self.buf.b, p)
            && re::looking_at_line_p(re.get(), self.buf.s, self.buf.begv, self.buf.zv, p)
    }

    /// `looking-at` for a regex written without its leading `^[ \t]*`:
    /// skips the indentation by hand. Regex engines that report groups are
    /// slow on long runs of blanks, and indentation can be arbitrarily deep.
    fn at_indented(&self, re: &Lazy, p: usize) -> Option<re::Captures> {
        self.looking_at(re, self.buf.skip_blank(p))
    }

    fn at_indented_p(&self, re: &Lazy, p: usize) -> bool {
        self.looking_at_p(re, self.buf.skip_blank(p))
    }
    /// `^\*+ ` at `p`: returns the number of stars.
    fn headline_stars(&self, p: usize) -> Option<usize> {
        if !self.buf.is_bol(p) {
            return None;
        }
        let n = self.buf.skip_fwd(p, b"*", self.buf.zv) - p;
        (n > 0 && self.buf.byte(p + n) == Some(b' ')).then_some(n)
    }

    /// `org-comment-regexp`: `^[ \t]*#\(?: \|$\)`. Returns the match end.
    fn comment_line(&self, p: usize) -> Option<usize> {
        if !self.buf.is_bol(p) {
            return None;
        }
        let q = self.buf.skip_blank(p);
        if self.buf.byte(q) != Some(b'#') {
            return None;
        }
        match self.buf.byte(q + 1) {
            Some(b' ') => Some(q + 2),
            None | Some(b'\n') => Some(q + 1),
            _ => None,
        }
    }

    /// `org-element-planning-line-re` (case-insensitive when `ci`).
    fn planning_line(&self, p: usize, ci: bool) -> bool {
        if !self.buf.is_bol(p) {
            return false;
        }
        let q = self.buf.skip_blank(p);
        ["CLOSED:", "DEADLINE:", "SCHEDULED:"].iter().any(|k| {
            if ci {
                self.buf.looking_at_ci(q, k)
            } else {
                self.buf.looking_at_str(q, k)
            }
        })
    }

    /// Matches `^[ \t]*<marker>[ \t]*$` case-insensitively on the line at
    /// `ls`, where `marker` is for example `#+END_SRC` or `:END:`.
    fn line_is(&self, ls: usize, marker: &str) -> bool {
        let q = self.buf.skip_blank(ls);
        self.buf.looking_at_ci(q, marker) && self.buf.rest_is_blank(q + marker.len())
    }

    /// `(re-search-forward "^[ \t]*MARKER[ \t]*$" limit t)` from `p`:
    /// returns the start of the matching line.
    pub(crate) fn find_line(&self, p: usize, limit: usize, marker: &str) -> Option<usize> {
        self.index.find_marker_line(&self.buf, p, limit, marker)
    }

    /// `org-property-drawer-re` at `p`, case-insensitively. Returns the end
    /// of the `:END:` line (before its newline).
    fn property_drawer_at(&self, p: usize) -> Option<usize> {
        if !self.buf.is_bol(p) || !self.line_is(p, ":PROPERTIES:") {
            return None;
        }
        let first_end = self.buf.eol(p);
        if first_end >= self.buf.zv {
            return None;
        }
        let mut ls = first_end + 1;
        loop {
            let le = self.buf.eol(ls);
            if self.line_is(ls, ":END:") {
                return Some(le);
            }
            // `[ \t]*:\S-+:\(?:[ \t].*\)?[ \t]*\n`
            if le >= self.buf.zv || !self.property_line_shape(ls, le) {
                return None;
            }
            ls = le + 1;
        }
    }

    /// The line shape required inside a property drawer.
    fn property_line_shape(&self, ls: usize, le: usize) -> bool {
        let q = self.buf.skip_blank(ls);
        if self.buf.byte(q) != Some(b':') {
            return false;
        }
        let run_end = self.buf.skip_nonspace_syntax(q + 1, le);
        if run_end < q + 3 || self.buf.b[run_end - 1] != b':' {
            return false;
        }
        run_end == le || matches!(self.buf.byte(run_end), Some(b' ' | b'\t'))
    }

    /// `org-item-re` at `p` (a line start).
    pub(crate) fn item_line(&self, p: usize) -> bool {
        lists::item_re_match(&self.buf, self.ctx, p).is_some()
    }

    // ----------------------------------------------------------------
    // The parser.

    /// `org-element-parse-buffer`.
    pub(crate) fn parse_document(&self) -> Raw {
        let len = self.buf.zv;
        let start = self.buf.bol(self.buf.skip_fwd(0, b" \t\n\r", len));
        let mut doc = Raw::new(DOCUMENT, 0, len).contents(Some(start), Some(len));
        doc.children = self.parse_elements(start, len, Mode::FirstSection, None);
        doc
    }

    /// `org-element--parse-elements`.
    pub(crate) fn parse_elements(
        &self,
        beg: usize,
        end: usize,
        mode: Mode,
        structure: Option<Rc<ListStruct>>,
    ) -> Vec<Raw> {
        // Nesting depth is unbounded (Emacs stops at `max-lisp-eval-depth`);
        // grow the stack instead of overflowing it.
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return Vec::new();
        }
        self.depth.set(depth + 1);
        let out = crate::deep(|| self.parse_elements_inner(beg, end, mode, structure));
        self.depth.set(depth);
        out
    }

    fn parse_elements_inner(
        &self,
        beg: usize,
        end: usize,
        mut mode: Mode,
        structure: Option<Rc<ListStruct>>,
    ) -> Vec<Raw> {
        let mut out = Vec::new();
        let mut pos = beg;
        while pos < end {
            let el = self.parse_one(pos, end, mode, structure.clone());
            pos = el.end;
            mode = next_mode(mode, el.kind, false);
            out.push(el);
        }
        out
    }

    /// One step of `org-element--parse-elements`: the element at `pos`,
    /// with its contents parsed.
    pub(crate) fn parse_one(
        &self,
        pos: usize,
        end: usize,
        mode: Mode,
        structure: Option<Rc<ListStruct>>,
    ) -> Raw {
        let mut el = self.current_element(pos, end, mode, structure);
        if el.end <= pos {
            // Defensive: org-element always makes progress. Consume the
            // line as a paragraph rather than looping forever.
            debug_assert!(false, "no progress at {pos} for {:?}", el.kind);
            let e = self.buf.next_line(pos).max(pos + 1).min(end.max(pos + 1));
            el = Raw::new(PARAGRAPH, pos, e).contents(Some(pos), Some(e));
        }
        if el.end > end {
            // Emacs can let a malformed element run past its container;
            // keep the tree well formed instead.
            clamp(&mut el, end);
        }
        let kind = el.kind;
        if let (Some(cb), Some(ce)) = (el.cb, el.ce) {
            if kind.is_greater_element() {
                let s = if matches!(kind, ITEM | PLAIN_LIST) {
                    el.structure.clone()
                } else {
                    None
                };
                let kids = self.parse_elements(cb, ce, next_mode(mode, kind, true), s);
                el.children.extend(kids);
            } else if self.objects
                && let Some(r) = objects::restriction_for(kind)
            {
                let kids = self.parse_objects(cb, ce, r);
                el.children.extend(kids);
            }
        }
        el.structure = None;
        el
    }

    /// `org-element--parse-objects` over `[beg, end)`.
    pub(crate) fn parse_objects(&self, beg: usize, end: usize, r: Restriction) -> Vec<Raw> {
        if !self.objects || beg >= end {
            return Vec::new();
        }
        objects::parse_objects(self, beg, end, r)
    }

    /// `org-element--current-element`.
    fn current_element(
        &self,
        pos: usize,
        limit: usize,
        mode: Mode,
        structure: Option<Rc<ListStruct>>,
    ) -> Raw {
        let b = &self.buf;
        match mode {
            Mode::Item => {
                return self.item(pos, limit, structure.expect("item mode needs a structure"));
            }
            Mode::TableRow => return self.table_row(pos),
            Mode::NodeProperty => return self.node_property(pos),
            _ => {}
        }
        let stars = self.headline_stars(pos);
        let at_task = stars.is_some();
        if let Some(n) = stars {
            let is_headline = match self.ctx.inlinetask_min_level {
                None => true,
                Some(min) => {
                    n < if self.ctx.odd_levels_only {
                        2 * min - 1
                    } else {
                        min
                    }
                }
            };
            if is_headline {
                return self.headline(pos);
            }
        }
        if matches!(mode, Mode::Section | Mode::FirstSection) {
            return self.section(pos);
        }
        if self.comment_line(pos).is_some() {
            return self.comment(pos, limit);
        }
        let prev_line_star = || b.byte(b.lbp0(pos)) == Some(b'*');
        if mode == Mode::Planning && prev_line_star() && self.planning_line(pos, true) {
            return self.planning(pos, limit);
        }
        let drawer_ok = match mode {
            Mode::Planning => prev_line_star(),
            Mode::PropertyDrawer | Mode::TopComment => {
                // `(forward-line -1)` then `(skip-chars-forward "[:blank:]")`.
                let prev = b.prev_line(pos);
                let mut q = prev;
                while let Some(c) = b.char_at(q) {
                    if tables::bits(c) & tables::BLANK != 0 {
                        q += c.len_utf8()
                    } else {
                        break;
                    }
                }
                !b.is_eol(q) || b.skip_bwd(q, b" \t\n\r", 0) == 0
            }
            _ => false,
        };
        if drawer_ok && self.property_drawer_at(pos).is_some() {
            return self.property_drawer(pos, limit);
        }
        if !b.is_bol(pos) {
            return self.paragraph(pos, limit, Affiliated::at(pos));
        }
        if self.looking_at_p(&CLOCK_LINE, pos) {
            return self.clock(pos, limit);
        }
        if at_task {
            return self.inlinetask(pos, limit);
        }
        let (aff, p) = self.collect_affiliated(pos, limit);
        if !aff.nodes.is_empty() && p >= limit {
            return self.keyword(aff.begin, limit, None, None);
        }
        let aff_opt = aff;
        // `org-element--current-element-re`, alternatives in order.
        if self.at_indented_p(&LATEX_BEGIN, p) {
            return self.latex_environment(p, limit, aff_opt);
        }
        if self.at_indented_p(&DRAWER_LINE, p) {
            return self.drawer(p, limit, aff_opt);
        }
        {
            let q = b.skip_blank(p);
            if b.byte(q) == Some(b':') && matches!(b.byte(q + 1), None | Some(b' ' | b'\n')) {
                return self.fixed_width(p, limit, aff_opt);
            }
        }
        if self.at_indented_p(&DYNAMIC_OPEN, p) {
            return self.dynamic_block(p, limit, aff_opt);
        }
        if let Some(m) = self.at_indented(&HASH_PLUS, p) {
            if let Some((s, e)) = m.get(1) {
                let name = b.slice(s, e).to_ascii_uppercase();
                return match name.as_str() {
                    "CENTER" => self.greater_block(p, limit, aff_opt, CENTER_BLOCK, "#+END_CENTER"),
                    "QUOTE" => self.greater_block(p, limit, aff_opt, QUOTE_BLOCK, "#+END_QUOTE"),
                    "COMMENT" => self.raw_block(p, limit, aff_opt, COMMENT_BLOCK, "#+END_COMMENT"),
                    "EXAMPLE" => self.raw_block(p, limit, aff_opt, EXAMPLE_BLOCK, "#+END_EXAMPLE"),
                    "EXPORT" => self.raw_block(p, limit, aff_opt, EXPORT_BLOCK, "#+END_EXPORT"),
                    "SRC" => self.raw_block(p, limit, aff_opt, SRC_BLOCK, "#+END_SRC"),
                    "VERSE" => self.verse_block(p, limit, aff_opt),
                    _ => self.special_block(p, limit, aff_opt),
                };
            }
            if m.get(2).is_some() {
                return self.babel_call(p, limit, aff_opt);
            }
            if let Some(key) = m.get(3) {
                // `#\+([^{S}]+):` found the key `#\+([^{S}]*):` would.
                return self.keyword(p, limit, Some(aff_opt), Some(key));
            }
            return self.paragraph(p, limit, aff_opt);
        }
        if self.looking_at_p(&FOOTNOTE_DEF, p) {
            return self.footnote_definition(p, limit, aff_opt);
        }
        {
            let q = b.skip_blank(p);
            let dashes = b.skip_fwd(q, b"-", b.zv) - q;
            if dashes >= 5 && b.rest_is_blank(q + dashes) {
                return self.horizontal_rule(p, limit, aff_opt);
            }
        }
        if b.looking_at_str(p, "%%(") {
            return self.diary_sexp(p, limit, aff_opt);
        }
        if self.is_table_start(p, limit) {
            return self.table(p, limit, aff_opt);
        }
        if self.item_line(p) {
            let s = structure.unwrap_or_else(|| Rc::new(lists::list_struct(self, p, limit)));
            return self.plain_list(p, limit, aff_opt, s);
        }
        self.paragraph(p, limit, aff_opt)
    }

    /// The table test in `org-element--current-element`.
    fn is_table_start(&self, p: usize, limit: usize) -> bool {
        let b = &self.buf;
        if b.byte(b.skip_blank(p)) == Some(b'|') {
            return true;
        }
        if !self.looking_at_p(&TABLE_RULE, p) {
            return false;
        }
        let next = b.lbp2(p);
        if next >= limit {
            return false;
        }
        let from = b.eol(p);
        match re::find_forward(NON_TABLE_EL_LINE.get(), b.s, b.begv, b.zv, from, limit) {
            None => {
                // `'move` puts point at LIMIT.
                let q = limit;
                let ls = if b.is_bol(q) {
                    b.prev_line(q)
                } else {
                    b.bol(q)
                };
                self.looking_at_p(&TABLE_RULE, ls)
            }
            Some((_, match_end)) => {
                let ls = b.bol(match_end);
                if next == ls {
                    false
                } else {
                    self.looking_at_p(&TABLE_RULE, b.prev_line(ls))
                }
            }
        }
    }

    /// `org-element--collect-affiliated-keywords`. Returns the keywords and
    /// the position after them. When the keywords are orphaned, returns
    /// none and `pos`.
    fn collect_affiliated(&self, pos: usize, limit: usize) -> (Affiliated, usize) {
        let b = &self.buf;
        if !b.is_bol(pos) {
            return (Affiliated::at(pos), pos);
        }
        let mut nodes = Vec::new();
        let mut p = pos;
        while p < limit {
            if !affiliated_candidate(b.b, b.skip_blank(p)) {
                break;
            }
            let Some(m) = self.at_indented(&AFFILIATED, p) else {
                break;
            };
            let line_end = b.next_line(p);
            let mut node = Raw::new(AFFILIATED_KEYWORD, p, line_end);
            let hash = b.skip_blank(p);
            node.tok(MARKER, hash, hash + 2);
            let (ks, ke) = m.get(1).or(m.get(3)).or(m.get(4)).expect("keyword group");
            node.tok(KEY, ks, ke);
            let name = b.slice(ks, ke).to_ascii_uppercase();
            let parsed = name == "CAPTION";
            let value_begin = m.whole().1;
            let value_end = b.skip_bwd(b.eol(p), b" \t", value_begin);
            if let Some((ds, de)) = m.get(2) {
                node.tok(MARKER, ds - 1, ds);
                if parsed && self.objects {
                    let mut v = Raw::new(KEYWORD_VALUE, ds, de);
                    v.children = self.parse_objects(ds, de, objects::restriction_keyword());
                    node.child(v);
                }
                node.tok(MARKER, de, de + 1);
            }
            let colon = match m.get(2) {
                Some((_, de)) => de + 1,
                None => ke,
            };
            node.tok(MARKER, colon, colon + 1);
            if parsed && self.objects && value_begin < value_end {
                let mut v = Raw::new(KEYWORD_VALUE, value_begin, value_end);
                v.children =
                    self.parse_objects(value_begin, value_end, objects::restriction_keyword());
                node.child(v);
            }
            nodes.push(node);
            p = line_end;
        }
        // Orphaned keywords: blank line, comment, clock line or headline.
        let orphan = b.rest_is_blank(p)
            || self.comment_line(p).is_some()
            || {
                let q = b.skip_blank(p);
                b.is_bol(p) && b.looking_at_ci(q, "CLOCK:")
            }
            || self.headline_stars(p).is_some();
        if orphan {
            return (Affiliated::at(pos), pos);
        }
        (Affiliated { begin: pos, nodes }, p)
    }

    /// Finishes an element that may carry affiliated keywords.
    fn with_affiliated(&self, mut el: Raw, aff: Affiliated) -> Raw {
        el.begin = aff.begin;
        el.children.extend(aff.nodes);
        el
    }

    // ----------------------------------------------------------------
    // Greater elements.

    /// `org-element-headline-parser`.
    pub(crate) fn headline(&self, pos: usize) -> Raw {
        let b = &self.buf;
        let true_level = b.skip_fwd(pos, b"*", b.zv) - pos;
        // `(re-search-forward (org-headline-re true-level) nil t)`: the
        // next headline of the same or a higher level, ignoring LIMIT.
        let mut end = b.zv;
        let mut ls = b.next_line(pos);
        while ls < b.zv {
            let n = b.skip_fwd(ls, b"*", b.zv) - ls;
            if n >= 1 && n <= true_level && b.byte(ls + n) == Some(b' ') {
                end = ls;
                break;
            }
            ls = b.next_line(ls);
        }
        let cb = {
            let q = b.skip_ws(b.next_line(pos), end);
            (q != end).then(|| b.bol(q))
        };
        let ce = cb.map(|_| end);
        let pb = if ce.is_some() {
            0
        } else {
            b.count_lines(pos, end) - 1
        };
        let mut el = Raw::new(HEADLINE, pos, end).contents(cb, ce).pb(pb);
        self.headline_title(&mut el, pos, true_level, HEADLINE);
        el
    }

    /// `org-element--headline-parse-title`: tokens and the parsed title.
    pub(crate) fn headline_title(
        &self,
        el: &mut Raw,
        begin: usize,
        true_level: usize,
        kind: SyntaxKind,
    ) {
        let b = &self.buf;
        el.tok(STARS, begin, begin + true_level);
        let line_end = b.eol(begin);
        let mut p = b.skip_blank(begin + true_level);
        // TODO keyword, case-sensitive, followed by a space or the end of line.
        for k in self
            .ctx
            .todo_keywords
            .iter()
            .chain(self.ctx.done_keywords.iter())
        {
            if !k.is_empty() && b.looking_at_str(p, k) {
                let after = p + k.len();
                if after == line_end || b.byte(after) == Some(b' ') {
                    el.tok(TODO_KEYWORD, p, after);
                    p = if after == line_end { after } else { after + 1 };
                    p = b.skip_blank(p);
                    break;
                }
            }
        }
        // Priority: `\[#.\][ \t]*`.
        if b.byte(p) == Some(b'[')
            && b.byte(p + 1) == Some(b'#')
            && let Some(c) = b.char_at(p + 2)
            && c != '\n'
            && b.byte(p + 2 + c.len_utf8()) == Some(b']')
        {
            let e = p + 3 + c.len_utf8();
            el.tok(PRIORITY, p, e);
            p = b.skip_blank(e);
        }
        // COMMENT, case-sensitive.
        if b.looking_at_str(p, "COMMENT") {
            let after = p + 7;
            if after == line_end || b.byte(after) == Some(b' ') {
                el.tok(COMMENT_KEYWORD, p, after);
                p = b.skip_blank(if after == line_end { after } else { after + 1 });
            }
        }
        let title_start = p;
        // Tags: `\(:[[:alnum:]_@#%:]+:\)[ \t]*$`, leftmost match.
        let mut title_end = line_end;
        let mut q = title_start;
        while q < line_end {
            if b.b[q] == b':' {
                let mut r = q + 1;
                while let Some(c) = b.char_at(r) {
                    if r < line_end
                        && (tables::is_alnum(c) || matches!(c, '_' | '@' | '#' | '%' | ':'))
                    {
                        r += c.len_utf8();
                    } else {
                        break;
                    }
                }
                if r >= q + 3 && b.b[r - 1] == b':' && b.rest_is_blank(r) {
                    el.tok(TAGS, q, r);
                    title_end = q;
                    break;
                }
            }
            q = b.next_char(q);
        }
        let ts = b.skip_blank(title_start);
        let te = b.skip_bwd(title_end, b" \t", 0);
        if ts < te {
            let mut t = Raw::new(HEADLINE_TITLE, ts, te).contents(Some(ts), Some(te));
            let r = if kind == INLINETASK {
                objects::restriction_for(INLINETASK)
            } else {
                objects::restriction_for(HEADLINE)
            };
            t.children = self.parse_objects(ts, te, r.expect("headline restriction"));
            el.child(t);
        }
    }

    /// `org-element-section-parser`.
    pub(crate) fn section(&self, pos: usize) -> Raw {
        let b = &self.buf;
        let max = match self.ctx.inlinetask_min_level {
            None => usize::MAX,
            Some(min) => {
                let limit_level = min - 1;
                if self.ctx.odd_levels_only {
                    2 * limit_level - 1
                } else {
                    limit_level
                }
            }
        };
        let mut end = b.zv;
        let mut ls = pos;
        while ls < b.zv {
            let n = b.skip_fwd(ls, b"*", b.zv) - ls;
            if n >= 1 && n <= max && b.byte(ls + n) == Some(b' ') {
                end = ls;
                break;
            }
            ls = b.next_line(ls);
        }
        Raw::new(SECTION, pos, end).contents(Some(pos), Some(end))
    }

    /// `org-element-inlinetask-parser`.
    fn inlinetask(&self, pos: usize, limit: usize) -> Raw {
        let b = &self.buf;
        let true_level = b.skip_fwd(pos, b"*", b.zv) - pos;
        let task_end = {
            let mut found = None;
            let mut ls = b.next_line(pos);
            while ls < limit {
                let n = b.skip_fwd(ls, b"*", b.zv) - ls;
                if n >= 1 && b.byte(ls + n) == Some(b' ') {
                    if b.eol(ls) > limit {
                        break;
                    }
                    let q = b.skip_blank(ls + n + 1);
                    if b.looking_at_ci(q, "END") && b.rest_is_blank(q + 3) {
                        found = Some(ls);
                    }
                    break;
                }
                ls = b.next_line(ls);
            }
            found
        };
        let cb = task_end.and_then(|te| {
            (pos < te).then(|| {
                let q = b.skip_fwd(b.next_line(pos), b" \t\n", b.zv);
                b.bol(q)
            })
        });
        let ce = cb.and(task_end);
        let end = b.element_end(b.next_line(task_end.unwrap_or(pos)), limit);
        let pb = b
            .count_lines(task_end.unwrap_or(pos), end)
            .saturating_sub(1);
        let mut el = Raw::new(INLINETASK, pos, end).contents(cb, ce).pb(pb);
        self.headline_title(&mut el, pos, true_level, INLINETASK);
        if let Some(te) = task_end {
            let mut endline = Raw::new(BLOCK_END, te, b.next_line(te));
            let n = b.skip_fwd(te, b"*", b.zv) - te;
            endline.tok(STARS, te, te + n);
            let q = b.skip_blank(te + n);
            endline.tok(KEY, q, q + 3);
            el.child(endline);
        }
        el
    }

    /// `org-element-plain-list-parser`.
    fn plain_list(&self, pos: usize, limit: usize, aff: Affiliated, s: Rc<ListStruct>) -> Raw {
        let b = &self.buf;
        let first = b.skip_blank(pos);
        let ordered = b.byte(first).is_some_and(|c| c.is_ascii_alphanumeric());
        let item = s.get(pos).expect("list structure starts at the list");
        let descriptive = !ordered && item.tag.is_some();
        let ind = item.ind;
        let mut ce = item.end;
        while let Some(it) = s.get(ce) {
            if it.ind != ind {
                break;
            }
            ce = it.end;
        }
        let ce = {
            let q = b.skip_bwd(ce, b" \r\t\n", 0);
            if b.is_bol(q) { q } else { b.lbp2(q) }
        };
        let end = {
            let q = b.skip_ws(ce, limit);
            if q == limit { limit } else { b.bol(q) }
        };
        let pb = b.count_lines(ce, end);
        let _ = descriptive;
        let mut el = Raw::new(PLAIN_LIST, pos, end)
            .contents(Some(pos), Some(ce))
            .pa(pos)
            .pb(pb);
        el.structure = Some(s);
        self.with_affiliated(el, aff)
    }

    /// `org-element-item-parser`.
    fn item(&self, pos: usize, limit: usize, s: Rc<ListStruct>) -> Raw {
        let b = &self.buf;
        let begin = b.bol(pos);
        let m = lists::full_item_match(self, begin);
        let Some(m) = m else {
            // Not an item line (should not happen with a valid structure).
            let e = b.next_line(begin).max(begin + 1).min(limit.max(begin + 1));
            return Raw::new(PARAGRAPH, begin, e).contents(Some(begin), Some(e));
        };
        let (bs, be) = m.get(1).expect("bullet");
        let bullet = b.slice(bs, be);
        let item_end = s.get(begin).map_or(limit, |it| it.end);
        let end = limit.min(if b.is_bol(item_end) {
            item_end
        } else {
            b.lbp2(item_end)
        });
        let ordered_bullet = bullet.contains(['.', ')']);
        let start = match m.get(4) {
            Some((ts, _)) if ordered_bullet => ts,
            _ => m.whole().1,
        };
        let q = b.skip_ws(start, end);
        let cb = if q == end {
            None
        } else if b.bol(q) == begin {
            Some(q)
        } else {
            Some(b.bol(q))
        };
        let ce = cb.map(|_| b.lbp2(b.skip_bwd(end, b" \r\t\n", 0)));
        let pb = b.count_lines(ce.unwrap_or(begin), end);
        let mut el = Raw::new(ITEM, begin, end).contents(cb, ce).pb(pb);
        // Tokens: bullet, counter, checkbox, tag.
        let bullet_chars = b.skip_bwd(be, b" \t", bs);
        el.tok(BULLET, bs, bullet_chars);
        if let Some((cs, ce2)) = m.get(2) {
            let open = b.s[be..cs].rfind("[@").map_or(cs, |i| be + i);
            el.tok(COUNTER, open, ce2 + 1);
        }
        if let Some((xs, xe)) = m.get(3) {
            el.tok(CHECKBOX, xs, xe);
        }
        let tag_raw = s.get(begin).and_then(|it| it.tag.as_ref());
        if let (Some((ts, te)), Some(_)) = (m.get(4), tag_raw) {
            let mut t = Raw::new(ITEM_TAG, ts, te).contents(Some(ts), Some(te));
            t.children = self.parse_objects(
                ts,
                te,
                objects::restriction_for(ITEM).expect("item restriction"),
            );
            el.child(t);
            // The `::` separator.
            let sep = b.skip_blank(te);
            if b.looking_at_str(sep, "::") {
                el.tok(MARKER, sep, sep + 2);
            }
        }
        el.structure = Some(s);
        el
    }

    /// `org-element-property-drawer-parser`.
    fn property_drawer(&self, pos: usize, limit: usize) -> Raw {
        let b = &self.buf;
        let cb0 = b.lbp2(pos);
        let end_line = self.find_line(pos, limit, ":END:").unwrap_or(pos);
        let ce = (end_line > cb0).then_some(end_line);
        let before_blank = b.next_line(end_line);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(PROPERTY_DRAWER, pos, end)
            .contents(ce.map(|_| cb0), ce)
            .pb(b.count_lines(before_blank, end));
        el.child(self.block_line(BLOCK_BEGIN, pos, ":", "PROPERTIES", ":"));
        el.child(self.block_line(BLOCK_END, end_line, ":", "END", ":"));
        el
    }

    /// `org-element-node-property-parser`.
    fn node_property(&self, pos: usize) -> Raw {
        let b = &self.buf;
        let Some(m) = self.at_indented(&PROPERTY_LINE, pos) else {
            let e = b.next_line(pos).max(pos + 1);
            return Raw::new(NODE_PROPERTY, pos, e);
        };
        let end = b.zv.min(m.whole().1 + 1);
        let mut el = Raw::new(NODE_PROPERTY, pos, end);
        let (ks, ke) = m.get(2).expect("key");
        el.tok(MARKER, ks - 1, ks);
        el.tok(KEY, ks, ke);
        el.tok(MARKER, ke, ke + 1);
        el
    }

    /// A block's opening or closing line as a node, with `open`, `name`
    /// and `close` markers located after the indentation.
    fn block_line(&self, kind: SyntaxKind, ls: usize, open: &str, name: &str, close: &str) -> Raw {
        let b = &self.buf;
        let mut n = Raw::new(kind, ls, b.next_line(ls));
        let q = b.skip_blank(ls);
        n.tok(MARKER, q, q + open.len());
        n.tok(KEY, q + open.len(), q + open.len() + name.len());
        if !close.is_empty() {
            let c = q + open.len() + name.len();
            n.tok(MARKER, c, c + close.len());
        }
        n
    }

    /// `#+BEGIN_NAME` / `#+END_NAME` line nodes.
    fn begin_line(&self, ls: usize) -> Raw {
        let b = &self.buf;
        let q = b.skip_blank(ls);
        let name_end = b.skip_nonspace_syntax(q + 2, b.eol(ls));
        let mut n = Raw::new(BLOCK_BEGIN, ls, b.next_line(ls));
        n.tok(MARKER, q, q + 2);
        n.tok(KEY, q + 2, name_end);
        n
    }

    fn end_line(&self, ls: usize) -> Raw {
        let b = &self.buf;
        let q = b.skip_blank(ls);
        let name_end = b.skip_nonspace_syntax(q + 2, b.eol(ls));
        let mut n = Raw::new(BLOCK_END, ls, b.next_line(ls));
        n.tok(MARKER, q, q + 2);
        n.tok(KEY, q + 2, name_end);
        n
    }

    /// Center and quote blocks.
    fn greater_block(
        &self,
        pos: usize,
        limit: usize,
        aff: Affiliated,
        kind: SyntaxKind,
        end_marker: &str,
    ) -> Raw {
        let b = &self.buf;
        let Some(end_line) = self.find_line(pos, limit, end_marker) else {
            return self.paragraph(pos, limit, aff);
        };
        let cb0 = b.next_line(pos);
        let cb = (cb0 < end_line).then_some(cb0);
        let ce = cb.map(|_| end_line);
        let before_blank = b.next_line(end_line);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(kind, pos, end)
            .contents(cb, ce)
            .pa(pos)
            .pb(b.count_lines(before_blank, end));
        el.child(self.begin_line(pos));
        el.child(self.end_line(end_line));
        self.with_affiliated(el, aff)
    }

    /// `org-element-special-block-parser`.
    fn special_block(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let Some(m) = self.at_indented(&SPECIAL_BEGIN, pos) else {
            return self.paragraph(pos, limit, aff);
        };
        let (ts, te) = m.get(1).expect("type");
        let marker = format!("#+END_{}", b.slice(ts, te));
        self.greater_block(pos, limit, aff, SPECIAL_BLOCK, &marker)
    }

    /// `org-element-dynamic-block-parser`.
    fn dynamic_block(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        // `^[ \t]*#\+END:?[ \t]*$`
        let mut end_line = None;
        let mut ls = b.next_line(pos);
        while ls < limit && b.eol(ls) <= limit {
            let q = b.skip_blank(ls);
            if b.looking_at_ci(q, "#+END") {
                let mut r = q + 5;
                if b.byte(r) == Some(b':') {
                    r += 1;
                }
                if b.rest_is_blank(r) {
                    end_line = Some(ls);
                    break;
                }
            }
            if b.eol(ls) >= b.zv {
                break;
            }
            ls = b.next_line(ls);
        }
        let Some(end_line) = end_line else {
            return self.paragraph(pos, limit, aff);
        };
        let cb0 = b.next_line(pos);
        let cb = (cb0 < end_line).then_some(cb0);
        let ce = cb.map(|_| end_line);
        let before_blank = b.next_line(end_line);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(DYNAMIC_BLOCK, pos, end)
            .contents(cb, ce)
            .pa(pos)
            .pb(b.count_lines(before_blank, end));
        let mut begin = Raw::new(BLOCK_BEGIN, pos, b.next_line(pos));
        let q = b.skip_blank(pos);
        begin.tok(MARKER, q, q + 2);
        begin.tok(KEY, q + 2, q + 7);
        begin.tok(MARKER, q + 7, q + 8);
        el.child(begin);
        el.child(self.end_line(end_line));
        self.with_affiliated(el, aff)
    }

    /// `org-element-drawer-parser`.
    fn drawer(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let from = limit.min(b.eol(pos));
        let Some(end_line) = self.find_line(from, limit, ":END:") else {
            return self.paragraph(pos, limit, aff);
        };
        let m = self.at_indented(&DRAWER_LINE, pos).expect("drawer line");
        let (ns, ne) = m.get(1).expect("drawer name");
        let cb0 = b.next_line(pos);
        let cb = (cb0 < end_line).then_some(cb0);
        let ce = cb.map(|_| end_line);
        let before_blank = b.next_line(end_line);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(DRAWER, pos, end)
            .contents(cb, ce)
            .pa(pos)
            .pb(b.count_lines(before_blank, end));
        let mut begin = Raw::new(BLOCK_BEGIN, pos, b.next_line(pos));
        begin.tok(MARKER, ns - 1, ns);
        begin.tok(KEY, ns, ne);
        begin.tok(MARKER, ne, ne + 1);
        el.child(begin);
        el.child(self.block_line(BLOCK_END, end_line, ":", "END", ":"));
        self.with_affiliated(el, aff)
    }

    /// `org-element-footnote-definition-parser`.
    fn footnote_definition(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let m = self
            .looking_at(&FOOTNOTE_DEF, pos)
            .expect("footnote definition");
        let (ls_, le_) = m.get(1).expect("label");
        let begin = aff.begin;
        let pa = pos;
        let end = {
            let mut found = limit;
            let mut ls = b.next_line(pos);
            'scan: while ls < limit {
                // Headline.
                if self.headline_stars(ls).is_some() {
                    found = ls;
                    break;
                }
                // New footnote definition.
                if b.eol(ls) <= limit && self.looking_at_p(&FOOTNOTE_DEF, ls) {
                    let mut q = b.prev_line(ls);
                    while q > pa && self.at_indented_p(&AFFILIATED, q) {
                        q = b.prev_line(q);
                    }
                    found = b.lbp2(q);
                    break;
                }
                // Two or more blank lines.
                if b.rest_is_blank(ls) && b.eol(ls) < b.zv {
                    let second = b.eol(ls) + 1;
                    if b.rest_is_blank(second) && b.eol(second) < b.zv && b.eol(second) < limit {
                        let mut q = b.eol(second) + 1;
                        while q < limit && b.rest_is_blank(q) && b.eol(q) < b.zv && b.eol(q) < limit
                        {
                            q = b.eol(q) + 1;
                        }
                        let r = b.skip_ws(q, limit);
                        found = if r == limit { limit } else { b.bol(r) };
                        break 'scan;
                    }
                }
                ls = b.next_line(ls);
            }
            found
        };
        let label_close = le_;
        let q = b.skip_ws(label_close + 1, end);
        let cb = if q == end {
            None
        } else if b.bol(q) == pa {
            Some(q)
        } else {
            Some(b.bol(q))
        };
        let ce0 = b.lbp2(b.skip_bwd(end, b" \r\t\n", 0));
        let ce = cb.map(|_| ce0);
        let pb = b.count_lines(ce0, end);
        let mut el = Raw::new(FOOTNOTE_DEFINITION, pos, end)
            .contents(cb, ce)
            .pa(pa)
            .pb(pb);
        el.tok(MARKER, pos, ls_);
        el.tok(KEY, ls_, le_);
        el.tok(MARKER, le_, le_ + 1);
        let _ = begin;
        self.with_affiliated(el, aff)
    }

    // ----------------------------------------------------------------
    // Elements.

    /// `org-element-babel-call-parser`.
    fn babel_call(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let before_blank = b.lbp2(pos);
        let colon =
            memchr::memchr(b':', &b.b[pos..before_blank]).map_or(before_blank, |i| pos + i + 1);
        let mut p = b.skip_blank(colon);
        // `(skip-chars-forward "^[]()" before-blank)`
        while p < before_blank && !matches!(b.b[p], b'[' | b']' | b'(' | b')') {
            p += 1;
        }
        if let Some(e) = self.index.match_bracket_global(b, p, b'[', b']') {
            p = e;
        }
        if let Some(e) = self.index.match_bracket_global(b, p, b'(', b')') {
            p = e;
        }
        // Unbalanced brackets can make Emacs run past the container. Kalem
        // keeps elements inside their container (docs/known-differences.org).
        let end = b
            .element_end(b.next_line(p), limit)
            .min(limit)
            .max(before_blank.min(limit));
        let mut el = Raw::new(BABEL_CALL, pos, end)
            .pa(pos)
            .pb(b.count_lines(before_blank.min(end), end));
        let q = b.skip_blank(pos);
        el.tok(MARKER, q, q + 2);
        el.tok(KEY, q + 2, colon - 1);
        el.tok(MARKER, colon - 1, colon);
        self.with_affiliated(el, aff)
    }

    /// `org-element-clock-parser`.
    fn clock(&self, pos: usize, limit: usize) -> Raw {
        let b = &self.buf;
        let line_end = b.eol(pos);
        let key = {
            let mut q = pos;
            let mut found = None;
            while q + 6 <= line_end {
                if b.looking_at_ci(q, "CLOCK:") {
                    found = Some(q);
                    break;
                }
                q += 1;
            }
            found.unwrap_or(pos)
        };
        let mut p = b.skip_blank(key + 6);
        let ts = objects::timestamp_at(self, &self.buf, p);
        // `(search-forward "=> " (line-end-position) t)`
        if let Some(i) = b.s[p..line_end].find("=> ") {
            p = p + i + 3;
        }
        let before_blank = b.next_line(p);
        let mut q = b.skip_ws(before_blank, limit);
        q = b.skip_bwd(q, b" \t", 0);
        if !b.is_bol(q) {
            q = b.skip_blank(q);
        }
        let mut el = Raw::new(CLOCK, pos, q).pb(b.count_lines(before_blank, q));
        el.tok(KEY, key, key + 5);
        el.tok(MARKER, key + 5, key + 6);
        if let Some(t) = ts
            && t.end <= q
        {
            el.child(t);
        }
        el
    }

    /// `org-element-comment-parser`.
    fn comment(&self, pos: usize, limit: usize) -> Raw {
        let b = &self.buf;
        let mut el = Raw::new(COMMENT, pos, pos);
        let mut p = pos;
        while let Some(m) = self.comment_line(p) {
            let q = b.skip_blank(p);
            el.tok(MARKER, q, m);
            p = b.next_line(p);
            if p >= limit {
                break;
            }
        }
        let com_end = p;
        let end = b.element_end(com_end, limit);
        el.end = end;
        el.pb = b.count_lines(com_end, end);
        el
    }

    /// Blocks whose contents are not parsed: comment, example, export, src.
    fn raw_block(
        &self,
        pos: usize,
        limit: usize,
        aff: Affiliated,
        kind: SyntaxKind,
        end_marker: &str,
    ) -> Raw {
        let b = &self.buf;
        let Some(end_line) = self.find_line(pos, limit, end_marker) else {
            return self.paragraph(pos, limit, aff);
        };
        let before_blank = b.next_line(end_line);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(kind, pos, end)
            .pa(pos)
            .pb(b.count_lines(before_blank, end));
        el.child(self.begin_line(pos));
        let cb = b.next_line(pos);
        if cb < end_line {
            el.tok(CODE_TEXT, cb, end_line);
        }
        el.child(self.end_line(end_line));
        let _ = (&SRC_BEGIN, &EXAMPLE_BEGIN, &EXPORT_BEGIN);
        self.with_affiliated(el, aff)
    }

    /// `org-element-verse-block-parser`.
    fn verse_block(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let Some(end_line) = self.find_line(pos, limit, "#+END_VERSE") else {
            return self.paragraph(pos, limit, aff);
        };
        let cb = b.next_line(pos);
        let before_blank = b.next_line(end_line);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(VERSE_BLOCK, pos, end)
            .contents(Some(cb), Some(end_line))
            .pa(pos)
            .pb(b.count_lines(before_blank, end));
        el.child(self.begin_line(pos));
        el.child(self.end_line(end_line));
        self.with_affiliated(el, aff)
    }

    /// `org-element-diary-sexp-parser`.
    fn diary_sexp(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let _ = &DIARY_LINE;
        let before_blank = b.next_line(pos);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(DIARY_SEXP, pos, end)
            .pa(pos)
            .pb(b.count_lines(before_blank, end));
        el.tok(MARKER, pos, pos + 2);
        self.with_affiliated(el, aff)
    }

    /// `org-element-fixed-width-parser`.
    fn fixed_width(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let mut el = Raw::new(FIXED_WIDTH, pos, pos).pa(pos);
        let mut p = pos;
        while p < limit {
            let q = b.skip_blank(p);
            if b.byte(q) == Some(b':') && matches!(b.byte(q + 1), None | Some(b' ' | b'\n')) {
                el.tok(MARKER, q, q + 1);
                p = b.next_line(p);
            } else {
                break;
            }
        }
        let end_area = if b.is_bol(p) { b.lep0(p) } else { p };
        let end = b.element_end(end_area, limit);
        el.end = end;
        el.pb = b.count_lines(end_area, end);
        self.with_affiliated(el, aff)
    }

    /// `org-element-horizontal-rule-parser`.
    fn horizontal_rule(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let post_hr = b.next_line(pos);
        let end = b.element_end(post_hr, limit);
        let mut el = Raw::new(HORIZONTAL_RULE, pos, end)
            .pa(pos)
            .pb(b.count_lines(post_hr, end));
        let q = b.skip_blank(pos);
        el.tok(MARKER, q, b.skip_fwd(q, b"-", b.zv));
        self.with_affiliated(el, aff)
    }

    /// `org-element-keyword-parser`. `aff` is `None` for orphaned
    /// affiliated keywords parsed as regular keywords; `key` is the key's
    /// span when the caller matched it already.
    fn keyword(
        &self,
        pos: usize,
        limit: usize,
        aff: Option<Affiliated>,
        key: Option<(usize, usize)>,
    ) -> Raw {
        let b = &self.buf;
        let before_blank = b.next_line(pos);
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(KEYWORD, pos, end)
            .pa(pos)
            .pb(b.count_lines(before_blank, end));
        let key = key.or_else(|| {
            self.at_indented(&KEYWORD_LINE, pos)
                .map(|m| m.get(1).expect("key"))
        });
        if let Some((ks, ke)) = key {
            el.tok(MARKER, ks - 2, ks);
            el.tok(KEY, ks, ke);
            el.tok(MARKER, ke, ke + 1);
        }
        match aff {
            Some(a) => self.with_affiliated(el, a),
            None => el,
        }
    }

    /// `org-element-latex-environment-parser`.
    fn latex_environment(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let m = self.at_indented(&LATEX_BEGIN, pos).expect("latex begin");
        let (ns, ne) = m.get(1).expect("env name");
        let name = b.slice(ns, ne);
        // `\\end{NAME}[ \t]*$`, case-insensitive, from POS.
        let needle = format!("\\end{{{name}}}");
        let mut found = None;
        let mut p = pos;
        while p < limit {
            let hay = &b.s[p..limit];
            let Some(i) = find_ci(hay, &needle) else {
                break;
            };
            let e = p + i + needle.len();
            if b.rest_is_blank(e) && b.eol(e) <= limit {
                found = Some(b.eol(e));
                break;
            }
            p = p + i + 1;
        }
        let Some(match_end) = found else {
            return self.paragraph(pos, limit, aff);
        };
        let code_end = b.next_line(match_end);
        let end = b.element_end(code_end, limit);
        let el = Raw::new(LATEX_ENVIRONMENT, pos, end)
            .pa(pos)
            .pb(b.count_lines(code_end, end));
        self.with_affiliated(el, aff)
    }

    /// `org-element-paragraph-parser`.
    pub(crate) fn paragraph(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let cb = pos;
        let sep = self.ctx.regexes();
        let mut p = b.eol(pos);
        let before_blank = loop {
            let Some((_, match_end)) =
                re::find_forward(&sep.paragraph_separate, b.s, b.begv, b.zv, p, limit)
            else {
                p = limit;
                break limit;
            };
            let ls = b.bol(match_end);
            let stop = if self.at_indented_p(&DRAWER_LINE, ls) {
                self.find_line(b.next_line(ls), limit, ":END:").is_some()
            } else if let Some(bm) = self.block_begin_name(ls) {
                self.find_line(ls, limit, &format!("#+END_{bm}")).is_some()
            } else if let Some(lm) = self.at_indented(&LATEX_BEGIN, ls) {
                let (ns, ne) = lm.get(1).expect("env");
                let needle = format!("\\end{{{}}}", b.slice(ns, ne));
                let mut ok = false;
                let mut q = ls;
                while q < limit {
                    let Some(i) = find_ci(&b.s[q..limit], &needle) else {
                        break;
                    };
                    let e = q + i + needle.len();
                    if b.rest_is_blank(e) && b.eol(e) <= limit {
                        ok = true;
                        break;
                    }
                    q = q + i + 1;
                }
                ok
            } else {
                self.dual_keyword_line(ls).unwrap_or(true)
            };
            if stop {
                p = ls;
                break ls;
            }
            p = b.eol(ls);
        };
        let before_blank = if p == limit { limit } else { before_blank };
        let ce = b.lbp2(b.skip_bwd(before_blank, b" \r\t\n", cb));
        let end = b.element_end(before_blank, limit);
        let mut el = Raw::new(PARAGRAPH, pos, end)
            .contents(Some(cb), Some(ce))
            .pa(cb)
            .pb(b.count_lines(before_blank, end));
        // A paragraph can start on a blank line (for example the first
        // line of a block). That line is contents, not a trailing blank
        // line: give it explicit tokens.
        if b.rest_is_blank(cb) && b.is_bol(cb) {
            let e = b.eol(cb);
            el.tok(WHITESPACE, cb, e);
            el.tok(NEWLINE, e, (e + 1).min(ce.max(e)));
        }
        self.with_affiliated(el, aff)
    }

    /// `[ \t]*#\+BEGIN_\(\S-+\)` at `ls`: returns the block name.
    fn block_begin_name(&self, ls: usize) -> Option<&'a str> {
        let b = &self.buf;
        let q = b.skip_blank(ls);
        if !b.looking_at_ci(q, "#+BEGIN_") {
            return None;
        }
        let e = b.skip_nonspace_syntax(q + 8, b.zv);
        (e > q + 8).then(|| b.slice(q + 8, e))
    }

    /// `[ \t]*#\+\(\S-+\)\[.*\]:` at `ls`: returns whether the keyword is
    /// a dual keyword, or `None` when the line does not have that shape.
    fn dual_keyword_line(&self, ls: usize) -> Option<bool> {
        static DUAL: Lazy = Lazy::starting(r"(?i)#\+([^{S}]+)\[.*\]:", b"#", false);
        let m = self.at_indented(&DUAL, ls)?;
        let (s, e) = m.get(1)?;
        let name = self.buf.slice(s, e);
        Some(name.eq_ignore_ascii_case("CAPTION") || name.eq_ignore_ascii_case("RESULTS"))
    }

    /// `org-element-planning-parser`.
    fn planning(&self, pos: usize, limit: usize) -> Raw {
        let b = &self.buf;
        let before_blank = b.next_line(pos);
        let mut q = b.skip_ws(before_blank, limit);
        q = b.skip_bwd(q, b" \t", 0);
        if !b.is_bol(q) {
            q = b.skip_blank(q);
        }
        let end = q;
        let mut el = Raw::new(PLANNING, pos, end).pb(b.count_lines(before_blank, end));
        // Emacs keeps, for each keyword, the timestamp after its last
        // occurrence (`(setq scheduled time)`).
        let mut keys: Vec<(usize, usize)> = Vec::new();
        let mut last: [Option<Raw>; 3] = [None, None, None];
        let mut p = pos;
        while p < end {
            let hay = &b.s[p..end];
            let hit = ["CLOSED:", "DEADLINE:", "SCHEDULED:"]
                .iter()
                .enumerate()
                .filter_map(|(i, k)| hay.find(k).map(|at| (at, i, k.len())))
                .min_by_key(|x| x.0);
            let Some((at, which, n)) = hit else { break };
            let ks = p + at;
            keys.push((ks, ks + n));
            p = b.skip_fwd(ks + n, b" \t", end);
            last[which] = objects::timestamp_at(self, &self.buf, p).filter(|t| t.end <= end);
        }
        // Keep a disjoint set of timestamps, preferring later ones.
        let mut stamps: Vec<Raw> = last.into_iter().flatten().collect();
        stamps.sort_by_key(|t| std::cmp::Reverse(t.begin));
        let mut kept: Vec<Raw> = Vec::new();
        for t in stamps {
            if kept.iter().all(|k| t.end <= k.begin || t.begin >= k.end) {
                kept.push(t);
            }
        }
        for (ks, ke) in keys {
            if kept.iter().all(|k| ke <= k.begin || ks >= k.end) {
                el.tok(KEY, ks, ke);
            }
        }
        for t in kept {
            el.child(t);
        }
        el
    }

    /// `org-element-table-parser`.
    fn table(&self, pos: usize, limit: usize, aff: Affiliated) -> Raw {
        let b = &self.buf;
        let org = b.byte(b.skip_blank(pos)) == Some(b'|');
        // `^[ \t]*\($\|[^| \t]\)` (plus `+` for table.el).
        let mut table_end = limit;
        let mut ls = pos;
        while ls < limit {
            let q = b.skip_blank(ls);
            let hit = match b.byte(q) {
                None | Some(b'\n') => q <= limit,
                Some(b'|') => false,
                Some(b'+') => org && q < limit,
                Some(_) => q < limit,
            };
            if hit {
                table_end = ls;
                break;
            }
            if b.eol(ls) >= b.zv {
                break;
            }
            ls = b.eol(ls) + 1;
        }
        let mut p = table_end;
        let mut el = Raw::new(TABLE, pos, pos).pa(pos);
        while p < limit
            && let Some(m) = self.at_indented(&TBLFM_LINE, p)
        {
            let _ = m;
            let q = b.skip_blank(p);
            el.tok(MARKER, q, q + 2);
            el.tok(KEY, q + 2, q + 7);
            el.tok(MARKER, q + 7, q + 8);
            let n = b.next_line(p);
            if n == p {
                break;
            }
            p = n;
        }
        let before_blank = p;
        let end = if before_blank > limit {
            before_blank
        } else {
            b.element_end(before_blank, limit)
        };
        el.end = end;
        el.pb = b.count_lines(before_blank, end);
        if org {
            el.cb = Some(pos);
            el.ce = Some(table_end);
        } else if pos < table_end {
            el.tok(CODE_TEXT, pos, table_end);
        }
        self.with_affiliated(el, aff)
    }

    /// `org-element-table-row-parser`.
    fn table_row(&self, pos: usize) -> Raw {
        let b = &self.buf;
        let q = b.skip_blank(pos);
        let rule = b.byte(q) == Some(b'|') && b.byte(q + 1) == Some(b'-');
        let end = b.lbp2(pos);
        if rule {
            return Raw::new(TABLE_ROW, pos, end);
        }
        let cb = memchr::memchr(b'|', &b.b[pos..b.zv]).map_or(b.zv, |i| pos + i + 1);
        let ce = b.skip_bwd(b.eol(cb), b" \t", 0);
        let end = b.lbp2(ce);
        let mut el = Raw::new(TABLE_ROW, pos, end).contents(Some(cb), Some(ce));
        el.tok(MARKER, cb - 1, cb);
        el
    }
}

/// Truncates `el` and its descendants to end at or before `end`.
fn clamp(el: &mut Raw, end: usize) {
    crate::deep(|| clamp_inner(el, end))
}

fn clamp_inner(el: &mut Raw, end: usize) {
    el.end = el.end.min(end);
    el.cb = el.cb.filter(|&c| c <= end);
    el.ce = if el.cb.is_some() {
        el.ce.map(|c| c.min(end))
    } else {
        None
    };
    el.tokens.retain(|t| t.start < end);
    for t in &mut el.tokens {
        t.end = t.end.min(end);
    }
    el.children.retain(|c| c.begin < end);
    for c in &mut el.children {
        clamp(c, end);
    }
}

/// `org-element--next-mode`.
pub(crate) fn next_mode(mode: Mode, kind: SyntaxKind, parent: bool) -> Mode {
    if parent {
        match kind {
            HEADLINE => Mode::Section,
            SECTION if mode == Mode::FirstSection => Mode::TopComment,
            DOCUMENT => Mode::FirstSection,
            INLINETASK => Mode::Planning,
            PLAIN_LIST => Mode::Item,
            PROPERTY_DRAWER => Mode::NodeProperty,
            SECTION => Mode::Planning,
            TABLE => Mode::TableRow,
            _ => Mode::None,
        }
    } else {
        match mode {
            Mode::Item => Mode::Item,
            Mode::NodeProperty => Mode::NodeProperty,
            Mode::Planning if kind == PLANNING => Mode::PropertyDrawer,
            Mode::TableRow => Mode::TableRow,
            Mode::TopComment if kind == COMMENT => Mode::PropertyDrawer,
            _ => Mode::None,
        }
    }
}

/// ASCII case-insensitive substring search.
pub(crate) fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    if h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}
