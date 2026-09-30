//! Incremental reparsing.
//!
//! After an edit, only the part of the tree the edit can affect is parsed
//! again; every other subtree is shared with the previous tree. The result
//! is always identical to parsing the new text from scratch (this is
//! checked by property tests). Three levels are tried in order:
//!
//! 1. **Element splice.** Inside a section, parsing restarts at the element
//!    before the edit and stops as soon as an element ends where an old
//!    element began, in the same parsing mode. The new elements replace the
//!    old ones in between.
//! 2. **Section.** When the edited lines contain something that can change
//!    elements before the edit (the end line of a block or drawer, a
//!    `\end{...}`, a footnote definition), the whole section is parsed again.
//! 3. **Document.** When the edit changes the headline structure, the
//!    in-buffer settings or radio targets, everything is parsed again.
//!
//! A document with CRLF line endings or a byte order mark is reparsed
//! the same way, in the coordinates of its normalized text
//! (`try_crlf`).

use rowan::{GreenNode, NodeOrToken, TextRange, TextSize};

use crate::SyntaxKind::{self, *};
use crate::elements::{Mode, Parser, next_mode};
use crate::{Parse, ParseContext, SyntaxNode, raw};

/// A change to the text: `range` (in the old text) was replaced by
/// `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// The replaced range, in the old text.
    pub range: TextRange,
    /// The new text.
    pub insert: String,
}

impl TextEdit {
    /// Applies the edit to `text`.
    pub fn apply(&self, text: &str) -> String {
        let (a, b) = (
            usize::from(self.range.start()),
            usize::from(self.range.end()),
        );
        let mut out = String::with_capacity(text.len() - (b - a) + self.insert.len());
        out.push_str(&text[..a]);
        out.push_str(&self.insert);
        out.push_str(&text[b..]);
        out
    }
}

/// How a reparse was done, for tests and statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReparseLevel {
    /// Some elements of a section were parsed again.
    Elements,
    /// One section was parsed again.
    Section,
    /// The whole document was parsed again.
    Document,
}

impl Parse {
    /// Returns the parse of `new_text`, which is the old text with `edit`
    /// applied, reusing as much of this tree as possible.
    pub fn reparse(&self, new_text: &str, edit: &TextEdit) -> Parse {
        self.reparse_with_level(new_text, edit).0
    }

    /// The incremental part of [`Parse::reparse`]: `None` when the edit
    /// needs a full parse (for example, it changed an in-buffer setting),
    /// which the caller may then run elsewhere, such as on a background
    /// thread.
    pub fn try_reparse(&self, new_text: &str, edit: &TextEdit) -> Option<(Parse, ReparseLevel)> {
        try_incremental(self, new_text, edit)
    }

    /// A full parse of `new_text` with the context source of this parse
    /// (the fallback of [`Parse::reparse`]).
    pub fn parse_again(&self, new_text: &str) -> Parse {
        match &self.source {
            crate::ContextSource::Explicit => crate::parse_with(new_text, self.context()),
            crate::ContextSource::Document { base, loader } => {
                crate::parse_document(new_text, base.clone(), loader.clone())
            }
        }
    }

    /// Like [`Parse::reparse`], and also reports how much was parsed again.
    pub fn reparse_with_level(&self, new_text: &str, edit: &TextEdit) -> (Parse, ReparseLevel) {
        match try_incremental(self, new_text, edit) {
            Some(r) => r,
            None => {
                let parse = match &self.source {
                    crate::ContextSource::Explicit => crate::parse_with(new_text, self.context()),
                    crate::ContextSource::Document { base, loader } => {
                        crate::parse_document(new_text, base.clone(), loader.clone())
                    }
                };
                (parse, ReparseLevel::Document)
            }
        }
    }
}

/// Headline levels of the lines, using the parse context's inlinetask rule.
fn headline_levels(lines: &str, ctx: &ParseContext) -> Vec<usize> {
    let max = match ctx.inlinetask_min_level {
        None => usize::MAX,
        Some(min) => {
            if ctx.odd_levels_only {
                2 * min - 1
            } else {
                min
            }
        }
    };
    lines
        .split('\n')
        .filter_map(|l| {
            let n = l.len() - l.trim_start_matches('*').len();
            (n > 0 && l.as_bytes().get(n) == Some(&b' ') && n < max).then_some(n)
        })
        .collect()
}

/// Lines that can change the document context (in-buffer settings or radio
/// targets).
fn touches_context(lines: &str) -> bool {
    lines.contains("<<<")
        || lines.contains(">>>")
        || lines.split('\n').any(|l| {
            let t = l.trim_start_matches([' ', '\t']);
            t.len() >= 2 && t.as_bytes()[..2].eq_ignore_ascii_case(b"#+") && {
                let key: String = t[2..]
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != ':')
                    .collect();
                let key = key.to_ascii_uppercase();
                matches!(
                    key.as_str(),
                    "TODO" | "SEQ_TODO" | "TYP_TODO" | "LINK" | "STARTUP" | "SETUPFILE"
                )
            }
        })
}

/// The parts of a subtree that feed the document context: radio target
/// values and the keywords read by the pre-pass.
fn context_facts(nodes: &[SyntaxNode]) -> (Vec<String>, Vec<String>) {
    use crate::ast::AstNode;
    let mut radios: Vec<String> = Vec::new();
    let mut keywords: Vec<String> = Vec::new();
    for n in nodes.iter().flat_map(|n| n.descendants()) {
        match n.kind() {
            RADIO_TARGET => {
                if let Some(r) = crate::ast::RadioTarget::cast(n) {
                    radios.push(r.value());
                }
            }
            KEYWORD => {
                if let Some(k) = crate::ast::Keyword::cast(n) {
                    let key = k.key();
                    if matches!(
                        key.as_str(),
                        "TODO" | "SEQ_TODO" | "TYP_TODO" | "LINK" | "STARTUP" | "SETUPFILE"
                    ) {
                        keywords.push(format!("{key}\0{}", k.value()));
                    }
                }
            }
            _ => {}
        }
    }
    radios.sort();
    radios.dedup();
    (radios, keywords)
}

/// Whether a `#+CALL:` line before `pos` has brackets that do not balance
/// on its own line.
fn unbalanced_call_before(root: &SyntaxNode, pos: usize) -> bool {
    root.descendants()
        .filter(|n| n.kind() == BABEL_CALL)
        .any(|n| {
            if usize::from(n.text_range().start()) >= pos {
                return false;
            }
            let text = n.text().to_string();
            let line = text
                .lines()
                .find(|l| l.trim_start().to_ascii_uppercase().starts_with("#+CALL"))
                .unwrap_or("");
            let count = |o: char, c: char| {
                line.chars().filter(|&x| x == o).count() != line.chars().filter(|&x| x == c).count()
            };
            count('(', ')') || count('[', ']')
        })
}

/// Lines whose presence can change elements that start before them.
fn is_risky(lines: &str) -> bool {
    lines.split('\n').any(|l| {
        let t = l
            .trim_start_matches([' ', '\t'])
            .trim_end_matches([' ', '\t']);
        let upper = t.to_ascii_uppercase();
        upper.starts_with("#+END")
            || upper == ":END:"
            || upper.contains("\\END{")
            || l.starts_with("[fn:")
            || (upper.trim_start_matches('*').trim() == "END" && t.starts_with('*'))
    })
}

fn try_incremental(old: &Parse, new_text: &str, edit: &TextEdit) -> Option<(Parse, ReparseLevel)> {
    match &old.norm {
        None => try_plain(old, new_text, edit, false).map(|(p, l, _)| (p, l)),
        Some(norm) => try_crlf(old, norm, new_text, edit),
    }
}

/// The indices, among node children, from the root down to `n`.
fn node_path(n: &SyntaxNode) -> Vec<usize> {
    let mut path = Vec::new();
    let mut cur = n.clone();
    while let Some(p) = cur.parent() {
        path.push(p.children().position(|c| c == cur).unwrap_or(0));
        cur = p;
    }
    path.reverse();
    path
}

fn follow(root: &SyntaxNode, path: &[usize]) -> Option<SyntaxNode> {
    let mut cur = root.clone();
    for &i in path {
        cur = cur.children().nth(i)?;
    }
    Some(cur)
}

/// Incremental reparsing of a document with CRLF line endings or a byte
/// order mark: the edit is reparsed in the normalized text's tree, and only
/// the replaced node is converted back to the original text, reusing the
/// old node's unchanged children.
fn try_crlf(
    old: &Parse,
    norm: &crate::crlf::Norm,
    new_text: &str,
    edit: &TextEdit,
) -> Option<(Parse, ReparseLevel)> {
    let (a, b) = (
        usize::from(edit.range.start()),
        usize::from(edit.range.end()),
    );
    let old_len = usize::from(old.syntax().text_range().end());
    let new_b = a + edit.insert.len();
    if b > old_len || a > b || old_len - (b - a) + edit.insert.len() != new_text.len() {
        return None;
    }
    let map = &norm.map;
    // The mark stays untouched, and no mark appears (a U+FEFF that becomes
    // the first character is a mark).
    if a < map.bom || (map.bom == 0 && new_text.starts_with('\u{feff}')) {
        return None;
    }
    // Widen the edit so that no `\r\n` pair of the old or the new text
    // crosses its boundaries: then it maps to one edit of the normalized
    // text. One character on each side is always enough.
    let nb = new_text.as_bytes();
    let (mut a2, mut b2) = (a, b);
    let mut insert = edit.insert.clone();
    if a > map.bom && nb[a - 1] == b'\r' && (map.cr_at(a - 1) || nb.get(a) == Some(&b'\n')) {
        a2 = a - 1;
        insert.insert(0, '\r');
    }
    if nb.get(new_b) == Some(&b'\n')
        && ((b > 0 && map.cr_at(b - 1)) || (new_b > 0 && nb[new_b - 1] == b'\r'))
    {
        b2 = b + 1;
        insert.push('\n');
    }
    // Apply the edit to the normalized text and the pair positions rather
    // than normalizing the whole new text again.
    let (insert, pairs) = crate::crlf::strip_pairs(&insert);
    let (na, nb2) = (map.norm(a2), map.norm(b2));
    if na > nb2 || nb2 > norm.text.len() {
        return None;
    }
    let new_map = map.edited(na, nb2, &pairs, insert.len());
    if new_map.is_empty() {
        return None;
    }
    let mut new_norm_text = String::with_capacity(norm.text.len() - (nb2 - na) + insert.len());
    new_norm_text.push_str(&norm.text[..na]);
    new_norm_text.push_str(&insert);
    new_norm_text.push_str(&norm.text[nb2..]);
    debug_assert_eq!(
        crate::crlf::normalize(new_text).map(|(t, _)| t).as_deref(),
        Some(new_norm_text.as_str())
    );
    let nedit = TextEdit {
        range: TextRange::new(TextSize::from(na as u32), TextSize::from(nb2 as u32)),
        insert,
    };
    let old_norm = Parse {
        green: norm.green.clone(),
        context: old.context.clone(),
        source: old.source.clone(),
        norm: None,
    };
    let (new_norm, level, path) = try_plain(&old_norm, &new_norm_text, &nedit, true)?;
    let old_node = follow(&old.syntax(), &path)?;
    let old_norm_node = follow(&old_norm.syntax(), &path)?;
    let new_norm_node = follow(&new_norm.syntax(), &path)?;
    let converted = crate::deep(|| {
        to_original(
            &new_norm_node,
            &old_norm_node,
            &old_node,
            new_text,
            &new_map,
        )
    });
    let green = old_node.replace_with(converted);
    let parse = Parse {
        green,
        context: old.context.clone(),
        source: old.source.clone(),
        norm: Some(std::sync::Arc::new(crate::crlf::Norm {
            green: new_norm.green,
            map: new_map,
            text: new_norm_text,
        })),
    };
    debug_assert_eq!(parse.syntax().to_string(), new_text);
    Some((parse, level))
}

/// Converts `new` (a node of the normalized tree) to the original text.
/// Children it shares with the old normalized node at the same place
/// (a common prefix, or a common suffix counted from the end) are taken from
/// the old original node; the rest is rebuilt token by token.
fn to_original(
    new: &SyntaxNode,
    old_norm: &SyntaxNode,
    old_orig: &SyntaxNode,
    text: &str,
    map: &crate::crlf::Map,
) -> GreenNode {
    let nk: Vec<_> = new.children_with_tokens().collect();
    let ok: Vec<_> = old_norm.children_with_tokens().collect();
    let oo: Vec<_> = old_orig.children_with_tokens().collect();
    let same = |x: &crate::SyntaxElement, y: &crate::SyntaxElement| match (x, y) {
        (NodeOrToken::Node(x), NodeOrToken::Node(y)) => std::ptr::eq(x.green(), y.green()),
        _ => false,
    };
    let aligned = ok.len() == oo.len();
    let mut prefix = 0;
    while aligned && prefix < nk.len() && prefix < ok.len() && same(&nk[prefix], &ok[prefix]) {
        prefix += 1;
    }
    let mut suffix = 0;
    while aligned
        && suffix < nk.len() - prefix
        && suffix < ok.len() - prefix
        && same(&nk[nk.len() - 1 - suffix], &ok[ok.len() - 1 - suffix])
    {
        suffix += 1;
    }
    let reuse = |e: &crate::SyntaxElement| match e {
        NodeOrToken::Node(n) => NodeOrToken::Node(n.green().to_owned()),
        NodeOrToken::Token(t) => NodeOrToken::Token(t.green().to_owned()),
    };
    let mut children = Vec::with_capacity(nk.len());
    for (i, e) in nk.iter().enumerate() {
        if i < prefix {
            children.push(reuse(&oo[i]));
        } else if i >= nk.len() - suffix {
            children.push(reuse(&oo[oo.len() - (nk.len() - i)]));
        } else {
            children.push(fresh(e, text, map));
        }
    }
    GreenNode::new(new.kind().into(), children)
}

/// Rebuilds a normalized element against the original text.
fn fresh(
    e: &crate::SyntaxElement,
    text: &str,
    map: &crate::crlf::Map,
) -> NodeOrToken<GreenNode, rowan::GreenToken> {
    match e {
        NodeOrToken::Token(t) => {
            let r = t.text_range();
            let (s, e) = (
                map.orig(usize::from(r.start())),
                map.orig(usize::from(r.end())),
            );
            NodeOrToken::Token(rowan::GreenToken::new(t.kind().into(), &text[s..e]))
        }
        NodeOrToken::Node(n) => {
            let kids: Vec<_> = n
                .children_with_tokens()
                .map(|c| crate::deep(|| fresh(&c, text, map)))
                .collect();
            NodeOrToken::Node(GreenNode::new(n.kind().into(), kids))
        }
    }
}

/// The incremental algorithm on text the parser reads as is. `normalized`
/// is true when the text is the normalized form of a CRLF document.
fn try_plain(
    old: &Parse,
    new_text: &str,
    edit: &TextEdit,
    normalized: bool,
) -> Option<(Parse, ReparseLevel, Vec<usize>)> {
    let root = old.syntax();
    let old_len = usize::from(root.text_range().end());
    let (a, b) = (
        usize::from(edit.range.start()),
        usize::from(edit.range.end()),
    );
    if b > old_len || a > b || old_len - (b - a) + edit.insert.len() != new_text.len() {
        return None;
    }
    // A `\r\n` pair or a byte order mark makes the document one that is
    // parsed normalized (see `try_crlf`); lone carriage returns are text.
    if !normalized
        && (memchr::memmem::find(new_text.as_bytes(), b"\r\n").is_some()
            || new_text.starts_with('\u{feff}'))
    {
        return None;
    }
    let new_b = a + edit.insert.len();
    if !new_text.is_char_boundary(a)
        || !new_text.is_char_boundary(new_b)
        || new_text[a..new_b] != edit.insert
    {
        return None;
    }
    // Read the removed text from the smallest node covering it. An
    // insertion belongs to the token after it: at the first character of a
    // section, the token before is the headline's line feed.
    let covering = if a == b && a < old_len {
        root.token_at_offset(edit.range.start())
            .right_biased()
            .map(NodeOrToken::Token)
            .unwrap_or_else(|| root.covering_element(edit.range))
    } else {
        root.covering_element(edit.range)
    };
    let cover_node = match covering {
        NodeOrToken::Node(n) => n,
        NodeOrToken::Token(t) => t.parent()?,
    };
    let base = cover_node.text_range().start();
    let removed = cover_node
        .text()
        .slice(TextRange::new(
            edit.range.start() - base,
            edit.range.end() - base,
        ))
        .to_string();
    // The old text's affected lines: the unchanged prefix of the first line,
    // the removed text, and the unchanged rest of the last line.
    let line_start = new_text[..a].rfind('\n').map_or(0, |i| i + 1);
    let line_end = new_text[new_b..]
        .find('\n')
        .map_or(new_text.len(), |i| new_b + i);
    let old_lines_owned = format!(
        "{}{}{}",
        &new_text[line_start..a],
        removed,
        &new_text[new_b..line_end]
    );
    let old_lines = old_lines_owned.as_str();
    let new_lines = &new_text[line_start..line_end];
    let ctx = old.context();
    if touches_context(old_lines) || touches_context(new_lines) {
        return None;
    }
    // `#+CALL:` lines search the rest of the buffer for a closing bracket
    // (Emacs behavior), so an edit that changes brackets can change a call
    // anywhere before it, headline lines included.
    if (removed.contains(['(', ')', '[', ']']) || edit.insert.contains(['(', ')', '[', ']']))
        && unbalanced_call_before(&root, a)
    {
        return None;
    }
    // Edits on headline lines change headlines, not sections.
    let old_levels = headline_levels(old_lines, ctx);
    let new_levels = headline_levels(new_lines, ctx);
    if !old_levels.is_empty() || !new_levels.is_empty() {
        // Within one headline line that keeps its level, only the line's own
        // tokens change: the subtree below is reused as is.
        if old_levels.len() == 1
            && old_levels == new_levels
            && !old_lines.contains('\n')
            && !new_lines.contains('\n')
        {
            return headline_line(old, &root, &cover_node, line_start, new_text, !normalized);
        }
        return None;
    }
    let delta = new_text.len() as isize - old_len as isize;
    // The section containing the edit, strictly after its start. Sections do
    // not nest, so the first one among the ancestors is the only candidate.
    let section = cover_node.ancestors().find(|n| n.kind() == SECTION)?;
    {
        let r = section.text_range();
        // The edit may start at the section's first character: the check
        // below keeps its first line non-blank, so the section still starts
        // there.
        if !(usize::from(r.start()) <= a && b <= usize::from(r.end())) {
            return None;
        }
    }
    // Sections nested in headlines or at the top of the document only.
    let parent_kind = section.parent().map(|p| p.kind())?;
    if !matches!(parent_kind, HEADLINE | DOCUMENT) {
        return None;
    }
    // A section starts at the first non-blank line after its headline: the
    // edit must leave that line non-blank.
    let sec_start = usize::from(section.text_range().start());
    let first_line_end = new_text[sec_start..]
        .find('\n')
        .map_or(new_text.len(), |i| sec_start + i);
    if !new_text[sec_start..first_line_end].contains(|c: char| !matches!(c, ' ' | '\t' | '\r')) {
        return None;
    }
    let parser = Parser::new(new_text, ctx);
    // Searches stay inside the section; index only its lines.
    let new_sec_end_pos = shift(usize::from(section.text_range().end()), delta)?;
    parser.index.set_region(sec_start, new_sec_end_pos);
    let first_mode = if parent_kind == DOCUMENT {
        Mode::TopComment
    } else {
        Mode::Planning
    };
    let sec_begin = usize::from(section.text_range().start());
    let risky = is_risky(old_lines) || is_risky(new_lines);
    let (new_section, level, old_facts, new_facts) = if risky {
        let g = reparse_section(&parser, sec_begin, first_mode, new_text, !normalized)?;
        let nf = context_facts(&[SyntaxNode::new_root(g.clone())]);
        (
            g,
            ReparseLevel::Section,
            context_facts(std::slice::from_ref(&section)),
            nf,
        )
    } else {
        let (g, old_nodes, new_nodes) = splice(
            &parser,
            &section,
            first_mode,
            a,
            b,
            new_b,
            delta,
            new_text,
            !normalized,
        )?;
        let nf = context_facts(
            &new_nodes
                .into_iter()
                .map(SyntaxNode::new_root)
                .collect::<Vec<_>>(),
        );
        (g, ReparseLevel::Elements, context_facts(&old_nodes), nf)
    };
    // The document context (in-buffer settings, radio targets) must not
    // change: an edit can, for example, end a block early and turn text
    // inside it into a radio target.
    if old_facts != new_facts {
        return None;
    }
    let path = node_path(&section);
    let green = section.replace_with(new_section);
    let parse = Parse {
        green,
        context: old.context.clone(),
        source: old.source.clone(),
        norm: None,
    };
    debug_assert_eq!(parse.syntax().to_string(), new_text);
    Some((parse, level, path))
}

/// Rebuilds the first line of the headline starting at `line_start`.
fn headline_line(
    old: &Parse,
    root: &SyntaxNode,
    cover: &SyntaxNode,
    line_start: usize,
    text: &str,
    crlf: bool,
) -> Option<(Parse, ReparseLevel, Vec<usize>)> {
    let headline = cover.ancestors().find(|n| {
        matches!(n.kind(), HEADLINE) && usize::from(n.text_range().start()) == line_start
    })?;
    let _ = root;
    let parser = Parser::new(text, old.context());
    let line_end = parser.buf.next_line(line_start);
    let true_level = text[line_start..]
        .bytes()
        .take_while(|&c| c == b'*')
        .count();
    let mut line = crate::raw::Raw::new(HEADLINE, line_start, line_end);
    parser.headline_title(&mut line, line_start, true_level, HEADLINE);
    let line_green = raw::build_green_mode(&line, text, crlf);
    // The old headline's children after its first line.
    let old_line_end = usize::from(headline.text_range().start())
        + headline
            .text()
            .to_string()
            .find('\n')
            .map_or(usize::from(headline.text_range().len()), |i| i + 1);
    let rest = headline
        .children_with_tokens()
        .filter(|e| usize::from(e.text_range().start()) >= old_line_end);
    let children: Vec<NodeOrToken<GreenNode, rowan::GreenToken>> = line_green
        .children()
        .map(|c| c.to_owned())
        .chain(rest.map(|e| match e {
            NodeOrToken::Node(n) => NodeOrToken::Node(n.green().to_owned()),
            NodeOrToken::Token(t) => NodeOrToken::Token(t.green().to_owned()),
        }))
        .collect();
    // The title may hold radio targets.
    let new_headline = GreenNode::new(SyntaxKind::HEADLINE.into(), children);
    if context_facts(std::slice::from_ref(&SyntaxNode::new_root(line_green)))
        != context_facts(&headline_line_nodes(&headline, old_line_end))
    {
        return None;
    }
    let path = node_path(&headline);
    let green = headline.replace_with(new_headline);
    let parse = Parse {
        green,
        context: old.context.clone(),
        source: old.source.clone(),
        norm: None,
    };
    debug_assert_eq!(parse.syntax().to_string(), text);
    Some((parse, ReparseLevel::Elements, path))
}

/// The child nodes of a headline's first line (its title).
fn headline_line_nodes(headline: &SyntaxNode, line_end: usize) -> Vec<SyntaxNode> {
    headline
        .children()
        .filter(|c| usize::from(c.text_range().end()) <= line_end)
        .collect()
}

/// Parses the section starting at `sec_begin` in the new text.
fn reparse_section(
    parser: &Parser<'_>,
    sec_begin: usize,
    first_mode: Mode,
    text: &str,
    crlf: bool,
) -> Option<GreenNode> {
    let mut sec = parser.section(sec_begin);
    let (cb, ce) = (sec.cb?, sec.ce?);
    sec.children = parser.parse_elements(cb, ce, first_mode, None);
    Some(raw::build_green_mode(&sec, text, crlf))
}

/// Re-parses the elements around the edit and splices them into the
/// section.
#[allow(clippy::too_many_arguments)]
fn splice(
    parser: &Parser<'_>,
    section: &SyntaxNode,
    first_mode: Mode,
    a: usize,
    b: usize,
    new_b: usize,
    delta: isize,
    text: &str,
    crlf: bool,
) -> Option<(GreenNode, Vec<SyntaxNode>, Vec<GreenNode>)> {
    let kids: Vec<SyntaxNode> = section.children().collect();
    if kids.is_empty() || section.children_with_tokens().count() != kids.len() {
        return None;
    }
    // Modes before each old element.
    let mut modes = Vec::with_capacity(kids.len() + 1);
    let mut m = first_mode;
    for k in &kids {
        modes.push(m);
        m = next_mode(m, k.kind(), false);
    }
    // The element containing the edit start, then one before it.
    let k = kids
        .iter()
        .position(|n| a < usize::from(n.text_range().end()))
        .unwrap_or(kids.len() - 1);
    let mut s = k.saturating_sub(1);
    // Orphaned affiliated keywords (such as `#+NAME:` before a blank line)
    // are separate keyword elements. An edit after them can give them an
    // element to attach to, so restart before the whole run.
    while s > 0 && is_affiliated_keyword_element(&kids[s - 1]) {
        s -= 1;
    }
    if is_affiliated_keyword_element(&kids[s]) {
        while s > 0 && is_affiliated_keyword_element(&kids[s - 1]) {
            s -= 1;
        }
    }
    let old_sec_end = usize::from(section.text_range().end());
    let new_sec_end = shift(old_sec_end, delta)?;
    let mut pos = usize::from(kids[s].text_range().start());
    let mut mode = modes[s];
    let mut new_nodes: Vec<GreenNode> = Vec::new();
    let mut resume = kids.len();
    // Old element starts after the edit, by new position.
    let old_starts: Vec<(usize, usize)> = kids
        .iter()
        .enumerate()
        .skip(s + 1)
        .filter(|(_, n)| usize::from(n.text_range().start()) >= b)
        .filter_map(|(j, n)| Some((shift(usize::from(n.text_range().start()), delta)?, j)))
        .collect();
    while pos < new_sec_end {
        let el = parser.parse_one(pos, new_sec_end, mode, None);
        pos = el.end;
        mode = next_mode(mode, el.kind, false);
        new_nodes.push(raw::build_green_mode(&el, text, crlf));
        // In the modes at a section's start, whether an element is a
        // planning line or a property drawer depends on the line before it,
        // which the edit may have changed: parse on instead of resuming.
        if pos >= new_b
            && !matches!(
                mode,
                Mode::Planning | Mode::PropertyDrawer | Mode::TopComment
            )
            && let Ok(i) = old_starts.binary_search_by_key(&pos, |x| x.0)
            && modes[old_starts[i].1] == mode
        {
            resume = old_starts[i].1;
            break;
        }
    }
    let _ = a;
    let children: Vec<NodeOrToken<GreenNode, rowan::GreenToken>> = kids[..s]
        .iter()
        .map(|n| NodeOrToken::Node(n.green().to_owned()))
        .chain(new_nodes.iter().cloned().map(NodeOrToken::Node))
        .chain(
            kids[resume..]
                .iter()
                .map(|n| NodeOrToken::Node(n.green().to_owned())),
        )
        .collect();
    let replaced = kids[s..resume].to_vec();
    Some((
        GreenNode::new(SyntaxKind::SECTION.into(), children),
        replaced,
        new_nodes,
    ))
}

fn is_affiliated_keyword_element(n: &SyntaxNode) -> bool {
    use crate::ast::AstNode;
    crate::ast::Keyword::cast(n.clone()).is_some_and(|k| {
        let key = k.key();
        matches!(
            key.as_str(),
            "CAPTION"
                | "DATA"
                | "HEADER"
                | "HEADERS"
                | "LABEL"
                | "NAME"
                | "PLOT"
                | "RESNAME"
                | "RESULT"
                | "RESULTS"
                | "SOURCE"
                | "SRCNAME"
                | "TBLNAME"
        ) || key.starts_with("ATTR_")
    })
}

fn shift(p: usize, delta: isize) -> Option<usize> {
    usize::try_from(p as isize + delta).ok()
}

#[allow(dead_code)]
fn _size(_: TextSize) {}
