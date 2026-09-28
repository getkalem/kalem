//! Builds the display form of one source line from the Org syntax tree:
//! which characters are shown, how they are styled, and how display
//! offsets map back to source offsets. Independent of gpui.

use org_syntax::{SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sty {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    pub code_bg: bool,
    pub link: bool,
    pub dim: bool,
    pub todo: Option<bool>,
    pub tag: bool,
    pub timestamp: bool,
    pub priority: bool,
    pub title: bool,
}

/// An inline widget drawn instead of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Widget {
    /// A list checkbox: `b' '`, `b'X'` or `b'-'`.
    Checkbox(u8),
    /// A LaTeX fragment such as `$x^2$`.
    Math(String),
}

/// Display text standing for a widget (the object replacement character).
pub const PLACEHOLDER: &str = "\u{FFFC}";

#[derive(Debug, Clone)]
pub struct Seg {
    pub src_start: usize,
    pub src_end: usize,
    pub text: String,
    /// Whether the text is the source text (so offsets map one to one).
    pub verbatim: bool,
    pub sty: Sty,
    pub widget: Option<Widget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockKind {
    #[default]
    Text,
    Code,
    Table,
    Meta,
}

#[derive(Debug, Clone, Default)]
pub struct LineView {
    pub segs: Vec<Seg>,
    /// Headline level, 0 for other lines.
    pub heading: u8,
    pub block: BlockKind,
}

impl LineView {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn display(&self) -> String {
        self.segs.iter().map(|s| s.text.as_str()).collect()
    }

    /// Display offset of source offset `src` (clamped into the line).
    pub fn display_offset(&self, src: usize) -> usize {
        let mut d = 0;
        for s in &self.segs {
            if src < s.src_start {
                return d;
            }
            if src <= s.src_end {
                return if s.verbatim { d + (src - s.src_start).min(s.text.len()) } else if src == s.src_end { d + s.text.len() } else { d };
            }
            d += s.text.len();
        }
        d
    }

    /// Source offset of display offset `d`.
    pub fn source_offset(&self, d: usize, line_end: usize) -> usize {
        let mut at = 0;
        for s in &self.segs {
            if d < at + s.text.len() || (d == at + s.text.len() && d == at) {
                return if s.verbatim { s.src_start + (d - at) } else { s.src_start };
            }
            at += s.text.len();
            if d == at {
                return s.src_end;
            }
        }
        self.segs.last().map_or(line_end, |s| s.src_end)
    }
}

fn contains(node: &SyntaxNode, cursor: Option<usize>) -> bool {
    let r = node.text_range();
    cursor.is_some_and(|c| usize::from(r.start()) <= c && c <= usize::from(r.end()))
}

const HIDABLE: &[SyntaxKind] = &[
    BOLD, ITALIC, UNDERLINE, STRIKE_THROUGH, CODE, VERBATIM, LINK, SUBSCRIPT, SUPERSCRIPT, RADIO_TARGET, TARGET,
    FOOTNOTE_REFERENCE, INLINE_SRC_BLOCK,
];

/// Builds the view of the source line `[ls, le)`.
pub fn line_view(root: &SyntaxNode, ls: usize, le: usize, cursor: Option<usize>) -> LineView {
    let mut view = LineView::default();
    let cursor_on_line = cursor.is_some_and(|c| ls <= c && c <= le);
    if ls >= le {
        return view;
    }
    let Some(mut tok) = token_at(root, ls) else { return view };
    let mut hide_ws_after_stars = false;
    loop {
        let r = tok.text_range();
        let (ts, te) = (usize::from(r.start()), usize::from(r.end()));
        if ts >= le {
            break;
        }
        let (s, e) = (ts.max(ls), te.min(le));
        if s < e {
            process(&tok, s, e, cursor, cursor_on_line, &mut view, &mut hide_ws_after_stars);
        }
        match tok.next_token() {
            Some(t) => tok = t,
            None => break,
        }
    }
    view
}

fn token_at(root: &SyntaxNode, offset: usize) -> Option<SyntaxToken> {
    let len = usize::from(root.text_range().end());
    if offset >= len {
        return root.last_token();
    }
    root.token_at_offset(TextSize::from(offset as u32)).right_biased()
}

fn process(tok: &SyntaxToken, s: usize, e: usize, cursor: Option<usize>, on_line: bool, view: &mut LineView, hide_ws: &mut bool) {
    let kind = tok.kind();
    // A LaTeX fragment away from the cursor becomes one formula widget.
    if let Some(f) = tok.parent_ancestors().find(|a| a.kind() == LATEX_FRAGMENT) {
        // The node includes the whitespace after the fragment.
        let fs = usize::from(f.text_range().start());
        let src = f.text().to_string().trim_end_matches([' ', '\t']).to_string();
        let fe = fs + src.len();
        let inside = cursor.is_some_and(|c| fs <= c && c <= fe);
        if s < fe && !inside && crate::math::body(&src).is_some_and(|b| !b.trim().is_empty()) && !src.contains('\n') {
            if s == fs {
                let sty = Sty::default();
                view.segs.push(Seg { src_start: fs, src_end: fe, text: PLACEHOLDER.into(), verbatim: false, sty, widget: Some(Widget::Math(src)) });
            }
            *hide_ws = false;
            return;
        }
    }
    let text = &tok.text()[s - usize::from(tok.text_range().start())..e - usize::from(tok.text_range().start())];
    let mut sty = Sty::default();
    let mut shown: Option<String> = None;
    let mut hidden = false;
    let parent = tok.parent();
    // Whitespace after an object belongs to the object's node (its
    // post-blank) but is not styled as part of it.
    let mut post_blank_of: Vec<SyntaxNode> = Vec::new();
    if kind == WHITESPACE {
        let mut node = tok.parent();
        while let Some(n) = node {
            if n.kind().is_object() && n.text_range().end() == tok.text_range().end() {
                post_blank_of.push(n.clone());
                node = n.parent();
            } else {
                break;
            }
        }
    }
    // Styles from ancestors.
    for a in tok.parent_ancestors() {
        if post_blank_of.contains(&a) {
            continue;
        }
        match a.kind() {
            BOLD => sty.bold = true,
            ITALIC => sty.italic = true,
            UNDERLINE => sty.underline = true,
            STRIKE_THROUGH => sty.strike = true,
            CODE | VERBATIM | INLINE_SRC_BLOCK => {
                sty.mono = true;
                sty.code_bg = true;
            }
            LINK => sty.link = true,
            TIMESTAMP => sty.timestamp = true,
            SRC_BLOCK | EXAMPLE_BLOCK | EXPORT_BLOCK | COMMENT_BLOCK | FIXED_WIDTH => {
                sty.mono = true;
                view.block = BlockKind::Code;
            }
            TABLE => {
                sty.mono = true;
                view.block = BlockKind::Table;
            }
            BLOCK_BEGIN | BLOCK_END | PROPERTY_DRAWER | DRAWER | PLANNING | CLOCK | AFFILIATED_KEYWORD | COMMENT => {
                sty.dim = true;
                if view.block == BlockKind::Text {
                    view.block = BlockKind::Meta;
                }
            }
            KEYWORD => {
                let is_title = a.children_with_tokens().any(|t| t.kind() == KEY && t.to_string().eq_ignore_ascii_case("TITLE"));
                if is_title {
                    sty.title = true;
                } else {
                    sty.dim = true;
                    view.block = BlockKind::Meta;
                }
            }
            HEADLINE | INLINETASK if kind != NEWLINE && on_headline_line(&a, s) => {
                sty.bold = true;
                if view.heading == 0 {
                    view.heading = a
                        .children_with_tokens()
                        .find(|t| t.kind() == STARS)
                        .map_or(1usize, |t| usize::from(t.text_range().len()))
                        .min(6) as u8;
                }
            }
            _ => {}
        }
    }
    match kind {
        NEWLINE | BLANK_LINE => hidden = true,
        STARS => {
            if !on_line {
                hidden = true;
                *hide_ws = true;
            }
        }
        WHITESPACE if *hide_ws => {
            hidden = true;
            *hide_ws = false;
        }
        TODO_KEYWORD => sty.todo = Some(matches!(text, "DONE" | "CANCELLED" | "CANCELED")),
        PRIORITY => sty.priority = true,
        TAGS => {
            sty.tag = true;
            sty.bold = false;
        }
        BULLET if !on_line && matches!(text, "-" | "+" | "*") => shown = Some("•".into()),
        CHECKBOX if !on_line => {
            let state = match text {
                "[X]" | "[x]" => b'X',
                "[-]" => b'-',
                _ => b' ',
            };
            view.segs.push(Seg { src_start: s, src_end: e, text: PLACEHOLDER.into(), verbatim: false, sty, widget: Some(Widget::Checkbox(state)) });
            return;
        }
        MARKER => {
            if let Some(p) = &parent
                && HIDABLE.contains(&p.kind())
                && !contains(p, cursor)
            {
                hidden = true;
            }
        }
        CODE_TEXT => {
            // A link path is hidden when the link has a description.
            if let Some(p) = &parent
                && p.kind() == LINK
                && p.children_with_tokens().filter(|t| t.kind() == MARKER).count() == 3
                && !contains(p, cursor)
            {
                hidden = true;
            }
        }
        KEY => {
            if let Some(p) = &parent
                && p.kind() == ENTITY
                && !contains(p, cursor)
                && let Some(u) = org_syntax::ast::AstNode::cast(p.clone()).and_then(|e: org_syntax::ast::Entity| e.utf8())
            {
                shown = Some(u.to_string());
            }
        }
        _ => {}
    }
    if kind != WHITESPACE && kind != STARS {
        *hide_ws = false;
    }
    if hidden {
        return;
    }
    // Entity backslash marker disappears with the replacement.
    if kind == MARKER
        && parent.as_ref().is_some_and(|p| p.kind() == ENTITY && !contains(p, cursor))
    {
        return;
    }
    let verbatim = shown.is_none();
    view.segs.push(Seg { src_start: s, src_end: e, text: shown.unwrap_or_else(|| text.to_string()), verbatim, sty, widget: None });
}

fn on_headline_line(headline: &SyntaxNode, pos: usize) -> bool {
    let start = usize::from(headline.text_range().start());
    let first_line_end = headline
        .children_with_tokens()
        .find(|t| t.kind() == NEWLINE)
        .map_or(usize::from(headline.text_range().end()), |t| usize::from(t.text_range().start()));
    start <= pos && pos < first_line_end.max(start + 1)
}

pub fn range(a: usize, b: usize) -> TextRange {
    TextRange::new(TextSize::from(a as u32), TextSize::from(b as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(text: &str, line: usize, cursor: Option<usize>) -> String {
        let p = org_syntax::parse(text);
        let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let ls = starts[line];
        let le = text[ls..].find('\n').map_or(text.len(), |i| ls + i);
        line_view(&p.syntax(), ls, le, cursor).display()
    }

    #[test]
    fn hides_markers_away_from_the_cursor() {
        let t = "* TODO Title :tag:\nSome *bold* and [[https://x.org][a link]] and \\alpha.\n- [X] done\n";
        assert_eq!(show(t, 0, None), "TODO Title :tag:");
        assert_eq!(show(t, 0, Some(3)), "* TODO Title :tag:");
        assert_eq!(show(t, 1, None), "Some bold and a link and α.");
        assert_eq!(show(t, 1, Some(19 + 7)), "Some *bold* and a link and α.");
        assert_eq!(show(t, 1, Some(19 + 20)), "Some bold and [[https://x.org][a link]] and α.");
        assert_eq!(show(t, 2, None), "• \u{FFFC} done");
    }

    #[test]
    fn post_blank_is_not_styled() {
        let t = "A [[x][link]] b\n";
        let p = org_syntax::parse(t);
        let v = line_view(&p.syntax(), 0, 15, None);
        let space = v.segs.iter().find(|s| s.src_start == 13).unwrap();
        assert_eq!(space.text, " ");
        assert!(!space.sty.link);
    }

    #[test]
    fn formulas_become_widgets() {
        let t = "Energy $E=mc^2$ here\n";
        assert_eq!(show(t, 0, None), "Energy \u{FFFC} here");
        assert_eq!(show(t, 0, Some(9)), "Energy $E=mc^2$ here");
        let p = org_syntax::parse(t);
        let v = line_view(&p.syntax(), 0, 20, None);
        assert_eq!(v.source_offset(7, 20), 7);
        assert_eq!(v.source_offset(10, 20), 15);
        assert_eq!(v.display_offset(15), 10);
    }

    #[test]
    fn offsets_round_trip() {
        let t = "Some *bold* text\n";
        let p = org_syntax::parse(t);
        let v = line_view(&p.syntax(), 0, 16, None);
        assert_eq!(v.display(), "Some bold text");
        assert_eq!(v.display_offset(6), 5);
        assert_eq!(v.source_offset(5, 16), 5);
        assert_eq!(v.source_offset(6, 16), 7);
        assert_eq!(v.display_offset(16), 14);
    }
}
