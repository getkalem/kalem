//! The intermediate tree produced by the parser, and its conversion into
//! a lossless rowan tree.
//!
//! The parser mirrors `org-element.el`, which describes elements by their
//! boundaries. [`Raw`] keeps those boundaries (begin, end, contents,
//! post-affiliated, post-blank) together with the explicit syntax tokens
//! of each node. The builder then fills every uncovered byte with generic
//! tokens, so the resulting tree always reproduces the input exactly.

use std::rc::Rc;

use rowan::{GreenNode, GreenNodeBuilder};

use crate::SyntaxKind;
use crate::lists::ListStruct;

#[derive(Debug, Clone)]
pub(crate) struct Raw {
    pub(crate) kind: SyntaxKind,
    pub(crate) begin: usize,
    pub(crate) end: usize,
    pub(crate) cb: Option<usize>,
    pub(crate) ce: Option<usize>,
    pub(crate) pa: usize,
    pub(crate) pb: usize,
    pub(crate) children: Vec<Raw>,
    pub(crate) tokens: Vec<Tok>,
    /// List structure shared by a plain list and its items.
    pub(crate) structure: Option<Rc<ListStruct>>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Tok {
    pub(crate) kind: SyntaxKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl Raw {
    pub(crate) fn new(kind: SyntaxKind, begin: usize, end: usize) -> Self {
        Raw {
            kind,
            begin,
            end,
            cb: None,
            ce: None,
            pa: begin,
            pb: 0,
            children: Vec::new(),
            tokens: Vec::new(),
            structure: None,
        }
    }

    pub(crate) fn contents(mut self, cb: Option<usize>, ce: Option<usize>) -> Self {
        self.cb = cb;
        self.ce = ce;
        self
    }

    pub(crate) fn pa(mut self, pa: usize) -> Self {
        self.pa = pa;
        self
    }

    pub(crate) fn pb(mut self, pb: usize) -> Self {
        self.pb = pb;
        self
    }

    pub(crate) fn tok(&mut self, kind: SyntaxKind, start: usize, end: usize) {
        if start < end {
            self.tokens.push(Tok { kind, start, end });
        }
    }

    pub(crate) fn child(&mut self, child: Raw) {
        self.children.push(child);
    }
}

/// Builds the rowan tree for `raw` over `text`.
pub(crate) fn build(raw: &Raw, text: &str) -> GreenNode {
    build_green(raw, text)
}

/// Builds the green node for one subtree.
pub(crate) fn build_green(raw: &Raw, text: &str) -> GreenNode {
    build_green_mode(raw, text, true)
}

/// Builds the green node for one subtree. With `crlf` false, `\r\n` in the
/// gaps is a carriage return character followed by a line feed: that is how
/// the normalized text of a CRLF document reads (every real pair was
/// already turned into `\n`).
pub(crate) fn build_green_mode(raw: &Raw, text: &str, crlf: bool) -> GreenNode {
    let mut b = GreenNodeBuilder::new();
    build_node(raw, text, crlf, &mut b);
    b.finish()
}

enum Item<'r> {
    Node(&'r Raw),
    Tok(Tok),
}

impl Item<'_> {
    fn start(&self) -> usize {
        match self {
            Item::Node(n) => n.begin,
            Item::Tok(t) => t.start,
        }
    }
    fn end(&self) -> usize {
        match self {
            Item::Node(n) => n.end,
            Item::Tok(t) => t.end,
        }
    }
}

fn build_node(raw: &Raw, text: &str, crlf: bool, b: &mut GreenNodeBuilder<'_>) {
    crate::deep(|| build_node_inner(raw, text, crlf, b))
}

fn build_node_inner(raw: &Raw, text: &str, crlf: bool, b: &mut GreenNodeBuilder<'_>) {
    b.start_node(raw.kind.into());
    let mut items: Vec<Item<'_>> = raw
        .children
        .iter()
        .filter(|c| c.begin < c.end)
        .map(Item::Node)
        .chain(raw.tokens.iter().copied().map(Item::Tok))
        .collect();
    items.sort_by_key(|i| (i.start(), i.end()));
    let mut pos = raw.begin;
    for item in items {
        let (s, e) = (item.start(), item.end());
        // Items must be disjoint and inside the node. An overlap would be
        // a parser bug; skipping the item keeps the tree lossless.
        if s < pos || e > raw.end {
            debug_assert!(
                false,
                "overlapping item {s}..{e} in {:?} at {pos}",
                raw.kind
            );
            continue;
        }
        if s > pos {
            gap_mode(text, pos, s, crlf, b);
        }
        match item {
            Item::Node(n) => build_node(n, text, crlf, b),
            Item::Tok(t) => b.token(t.kind.into(), &text[t.start..t.end]),
        }
        pos = e;
    }
    if pos < raw.end {
        gap_mode(text, pos, raw.end, crlf, b);
    }
    b.finish_node();
}

/// Emits generic tokens for `[a, b)`: whole blank lines become
/// [`SyntaxKind::BLANK_LINE`], line feeds [`SyntaxKind::NEWLINE`], runs
/// of spaces and tabs [`SyntaxKind::WHITESPACE`], everything else
/// [`SyntaxKind::TEXT`].
fn gap_mode(text: &str, a: usize, b: usize, crlf: bool, out: &mut GreenNodeBuilder<'_>) {
    let bytes = text.as_bytes();
    // A line ending is `\n` or `\r\n`; returns its length at `i`.
    let eol_len = |i: usize| -> usize {
        match bytes.get(i) {
            Some(b'\n') => 1,
            Some(b'\r') if crlf && i + 1 < b && bytes.get(i + 1) == Some(&b'\n') => 2,
            _ => 0,
        }
    };
    let mut p = a;
    while p < b {
        let at_bol = p == 0 || bytes[p - 1] == b'\n';
        if at_bol {
            let mut q = p;
            while q < b && (bytes[q] == b' ' || bytes[q] == b'\t') {
                q += 1;
            }
            let n = eol_len(q);
            if q < b && n > 0 {
                out.token(SyntaxKind::BLANK_LINE.into(), &text[p..q + n]);
                p = q + n;
                continue;
            }
            if q == b && q == text.len() && q > p {
                out.token(SyntaxKind::BLANK_LINE.into(), &text[p..q]);
                p = q;
                continue;
            }
        }
        let n = eol_len(p);
        if n > 0 {
            out.token(SyntaxKind::NEWLINE.into(), &text[p..p + n]);
            p += n;
            continue;
        }
        match bytes[p] {
            b' ' | b'\t' => {
                let mut q = p;
                while q < b && (bytes[q] == b' ' || bytes[q] == b'\t') {
                    q += 1;
                }
                out.token(SyntaxKind::WHITESPACE.into(), &text[p..q]);
                p = q;
            }
            _ => {
                let mut q = p;
                while q < b && bytes[q] != b'\n' && eol_len(q) == 0 {
                    q += 1;
                }
                // Keep trailing spaces of a line separate from the text.
                let mut t = q;
                while t > p && (bytes[t - 1] == b' ' || bytes[t - 1] == b'\t') {
                    t -= 1;
                }
                out.token(SyntaxKind::TEXT.into(), &text[p..t]);
                p = t;
            }
        }
    }
}
