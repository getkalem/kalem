//! The view model both frontends draw (design §6.3, §7.2): the document as
//! blocks, each source line as styled runs mapped back to source offsets,
//! markup hidden away from the cursor, and folding.
//!
//! **Reveal rule.** An object's markers (`*` of bold, `[[…][` of a link,
//! `^{` of a superscript) show only while the cursor is inside the object,
//! from its first character to its last, both ends included; trailing
//! blanks are not part of it. Headline stars, list bullets and checkboxes
//! show as source while the cursor is on their line. Away from the cursor,
//! entities show as their characters, formulas and image links as widgets.
//!
//! **Motion.** Cursor motion goes through the display text
//! ([`LineView::next_position`]), so hidden text is skipped; landing inside
//! an object reveals it on the next build of the line.

use std::collections::BTreeMap;
use std::ops::Range;

use org_edit::{Assoc, Transaction};
use org_syntax::SyntaxKind::{self, *};
use org_syntax::{ParseContext, SyntaxNode, SyntaxToken, TextSize, ast};
use unicode_segmentation::UnicodeSegmentation;

/// Inline styles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    /// Bold, and headline titles.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strike: bool,
    /// Monospace with a background: code, verbatim, inline source.
    pub code: bool,
    /// A link.
    pub link: bool,
    /// Dimmed: planning, drawers, keywords, comments, block delimiters.
    pub dim: bool,
    /// A TODO keyword: `Some(true)` for done states.
    pub todo: Option<bool>,
    /// Headline tags.
    pub tag: bool,
    /// A timestamp.
    pub timestamp: bool,
    /// A priority cookie.
    pub priority: bool,
    /// The document title.
    pub title: bool,
    /// The byline: `#+AUTHOR`, `#+DATE`, `#+SUBTITLE`, `#+EMAIL`.
    pub byline: bool,
    /// Superscript.
    pub superscript: bool,
    /// Subscript.
    pub subscript: bool,
    /// A footnote reference.
    pub footnote: bool,
    /// A target or radio target.
    pub target: bool,
    /// A statistics cookie.
    pub cookie: bool,
    /// Kalem's own formatting: font, size, colors (`crate::rich`).
    pub rich: crate::rich::CharFormat,
}

/// A checkbox's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// `[ ]`
    Unchecked,
    /// `[X]`
    Checked,
    /// `[-]`
    Partial,
}

/// Something drawn in place of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Widget {
    /// A list checkbox.
    Checkbox(CheckState),
    /// A LaTeX fragment.
    Math {
        /// The source, delimiters included.
        source: String,
        /// `$$…$$` or `\[…\]`.
        display: bool,
    },
    /// An image link without a description.
    Image {
        /// The file, as written in the link; an `attachment:` link's file
        /// in its heading's attachment folder.
        path: String,
        /// The width `#+ATTR_ORG: :width` asks for.
        width: Option<ImageWidth>,
    },
}

/// The width of an image, from `#+ATTR_ORG: :width` (`300`, `300px`,
/// `50%` or `0.5`), as `org-display-inline-images` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageWidth {
    /// Pixels.
    Pixels(u32),
    /// A share of the text width, in percent.
    Percent(u32),
}

impl ImageWidth {
    /// Reads a `:width` value.
    pub fn parse(v: &str) -> Option<ImageWidth> {
        let v = v.trim();
        if let Some(p) = v.strip_suffix('%') {
            let p: f64 = p.trim().parse().ok()?;
            return (p > 0.0).then(|| ImageWidth::Percent(p.round().min(1000.0) as u32));
        }
        let n = v.strip_suffix("px").unwrap_or(v).trim();
        if let Ok(px) = n.parse::<u32>() {
            return (px > 0).then_some(ImageWidth::Pixels(px));
        }
        let f: f64 = n.parse().ok()?;
        (f > 0.0 && f <= 10.0).then(|| ImageWidth::Percent((f * 100.0).round() as u32))
    }

    /// The width in pixels for a text `available` pixels wide.
    pub fn resolve(self, available: f32) -> f32 {
        match self {
            ImageWidth::Pixels(p) => p as f32,
            ImageWidth::Percent(p) => available * p as f32 / 100.0,
        }
    }
}

/// The `:width` of the `#+ATTR_ORG:` lines of the element holding `n`.
fn attr_org_width(n: &SyntaxNode) -> Option<ImageWidth> {
    let element = n.ancestors().find(|a| a.kind().is_element())?;
    ast::affiliated_keywords(&element)
        .filter(|k| k.key().eq_ignore_ascii_case("ATTR_ORG"))
        .find_map(|k| {
            let v = k.value();
            let mut words = v.split_whitespace();
            while let Some(w) = words.next() {
                if w.eq_ignore_ascii_case(":width") {
                    return words.next().and_then(ImageWidth::parse);
                }
            }
            None
        })
}

/// Where `org-attach` keeps the attachments of the heading holding `n`:
/// its `DIR` property, else `data/` and its `ID` split after two
/// characters (`org-attach-id-uuid-folder-format`), relative to the
/// document's folder.
pub fn attachment_dir(n: &SyntaxNode) -> Option<String> {
    for h in n
        .ancestors()
        .filter_map(<ast::Headline as ast::AstNode>::cast)
    {
        let props = h.properties();
        let get = |k: &str| {
            props
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        if let Some(d) = get("DIR").or_else(|| get("ATTACH_DIR")) {
            return Some(d);
        }
        if let Some(id) = get("ID") {
            let split = id.char_indices().nth(2).map_or(id.len(), |(i, _)| i);
            return Some(format!("data/{}/{}", &id[..split], &id[split..]));
        }
    }
    None
}

/// The display text of a widget (the object replacement character).
pub const PLACEHOLDER: &str = "\u{FFFC}";

/// A piece of a line: source text shown as is, replaced (an entity, a
/// bullet) or drawn as a widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The source it stands for.
    pub src: Range<usize>,
    /// The text shown.
    pub text: String,
    /// Whether `text` is the source text, so offsets map one to one.
    pub verbatim: bool,
    /// The style.
    pub style: Style,
    /// A widget drawn instead of the text.
    pub widget: Option<Widget>,
}

/// What a line is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineRole {
    /// Content.
    #[default]
    Content,
    /// The first or last line of a block or drawer (`#+begin_src`,
    /// `:END:`); frontends may shrink or hide it away from the cursor.
    Delimiter,
}

/// A source line as displayed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineView {
    /// The source line, without its line feed.
    pub range: Range<usize>,
    /// The runs, in order.
    pub runs: Vec<Run>,
    /// Headline level (1 to 6, deeper levels as 6), 0 for other lines.
    pub heading: u8,
    /// What the line is for.
    pub role: LineRole,
    /// Monospace: code, tables, fixed-width lines.
    pub mono: bool,
    /// The paragraph's alignment (`#+ATTR_KALEM: :align`, center blocks).
    pub align: crate::rich::Align,
}

impl LineView {
    /// The displayed text.
    pub fn display(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// The display offset of source offset `src` (clamped into the line).
    /// A position inside a replaced run maps to its start.
    pub fn display_offset(&self, src: usize) -> usize {
        let mut d = 0;
        for r in &self.runs {
            if src < r.src.start {
                return d;
            }
            if src <= r.src.end {
                return if r.verbatim {
                    d + (src - r.src.start).min(r.text.len())
                } else if src == r.src.end {
                    d + r.text.len()
                } else {
                    d
                };
            }
            d += r.text.len();
        }
        d
    }

    /// The source offset of display offset `d`. At the boundary of two
    /// runs it is the end of the first.
    pub fn source_offset(&self, d: usize) -> usize {
        let mut at = 0;
        for r in &self.runs {
            if d < at + r.text.len() || (d == at && r.text.is_empty()) {
                return if r.verbatim {
                    r.src.start + (d - at)
                } else {
                    r.src.start
                };
            }
            at += r.text.len();
            if d == at {
                return r.src.end;
            }
        }
        self.runs.last().map_or(self.range.end, |r| r.src.end)
    }

    /// The source ranges of the line that are not shown.
    pub fn hidden(&self) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut at = self.range.start;
        for r in &self.runs {
            if r.src.start > at {
                out.push(at..r.src.start);
            }
            at = at.max(r.src.end);
        }
        if at < self.range.end {
            out.push(at..self.range.end);
        }
        out
    }

    /// The source offset one grapheme right of `src` in the display, or
    /// `None` at the end of the line.
    pub fn next_position(&self, src: usize) -> Option<usize> {
        let text = self.display();
        let d = self.display_offset(src);
        let g = text[d..].graphemes(true).next()?;
        Some(self.source_offset(d + g.len()))
    }

    /// `src`, or where the shown text after it starts when hidden text
    /// follows (a run boundary): the character a block cursor there is on.
    pub fn run_start_at(&self, src: usize) -> usize {
        let d = self.display_offset(src);
        let mut at = 0;
        for r in &self.runs {
            if at == d && !r.text.is_empty() {
                return r.src.start.max(src);
            }
            at += r.text.len();
        }
        src
    }

    /// The source offset one grapheme left of `src` in the display, or
    /// `None` at the start of the line.
    pub fn prev_position(&self, src: usize) -> Option<usize> {
        let text = self.display();
        let d = self.display_offset(src);
        let g = text[..d].graphemes(true).next_back()?;
        let p = d - g.len();
        // The start of a run, not the end of the one before it.
        let mut at = 0;
        for r in &self.runs {
            if at == p {
                return Some(r.src.start);
            }
            at += r.text.len();
        }
        Some(self.source_offset(p))
    }
}

fn end(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().end())
}

fn start(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().start())
}

/// Where the cursor reveals an object: its range without trailing blanks.
fn reveal_range(n: &SyntaxNode) -> Range<usize> {
    let blank = match n.last_child_or_token() {
        Some(org_syntax::NodeOrToken::Token(t)) if t.kind() == WHITESPACE => {
            usize::from(t.text_range().len())
        }
        _ => 0,
    };
    start(n)..end(n) - blank
}

fn revealed(n: &SyntaxNode, cursor: Option<usize>) -> bool {
    let r = reveal_range(n);
    cursor.is_some_and(|c| r.start <= c && c <= r.end)
}

/// Objects whose markers hide away from the cursor.
const HIDABLE: &[SyntaxKind] = &[
    BOLD,
    ITALIC,
    UNDERLINE,
    STRIKE_THROUGH,
    CODE,
    VERBATIM,
    LINK,
    SUBSCRIPT,
    SUPERSCRIPT,
    RADIO_TARGET,
    TARGET,
    FOOTNOTE_REFERENCE,
    INLINE_SRC_BLOCK,
];

/// The body of a LaTeX fragment: `$x$`, `$$x$$`, `\(x\)` or `\[x\]`.
pub fn math_body(src: &str) -> Option<&str> {
    for (a, b) in [("$$", "$$"), ("\\(", "\\)"), ("\\[", "\\]"), ("$", "$")] {
        if src.len() >= a.len() + b.len()
            && let Some(inner) = src.strip_prefix(a).and_then(|s| s.strip_suffix(b))
        {
            return Some(inner);
        }
    }
    None
}

/// `org-image-file-name-regexp`.
fn is_image(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    matches!(
        ext.as_deref(),
        Some(
            "png"
                | "jpeg"
                | "jpg"
                | "gif"
                | "tiff"
                | "tif"
                | "xbm"
                | "xpm"
                | "pbm"
                | "pgm"
                | "ppm"
                | "webp"
                | "svg"
        )
    )
}

/// The widget an object becomes away from the cursor.
fn widget_of(n: &SyntaxNode, ctx: &ParseContext) -> Option<Widget> {
    match n.kind() {
        LATEX_FRAGMENT => {
            let r = reveal_range(n);
            let src = n.text().to_string()[..r.end - r.start].to_string();
            let body = math_body(&src)?;
            (!body.trim().is_empty() && !src.contains('\n')).then(|| Widget::Math {
                display: src.starts_with("$$") || src.starts_with("\\["),
                source: src,
            })
        }
        LINK => {
            let link: ast::Link = ast::AstNode::cast(n.clone())?;
            if link.description().is_some() {
                return None;
            }
            let info = link.info(ctx);
            if !(matches!(info.link_type.as_str(), "file" | "attachment") && is_image(&info.path)) {
                return None;
            }
            let path = match info.link_type.as_str() {
                "attachment" => match attachment_dir(n) {
                    Some(d) => format!("{}/{}", d.trim_end_matches('/'), info.path),
                    None => info.path,
                },
                _ => info.path,
            };
            Some(Widget::Image {
                path,
                width: attr_org_width(n),
            })
        }
        _ => None,
    }
}

struct LineBuilder<'a> {
    ctx: &'a ParseContext,
    cursor: Option<usize>,
    on_line: bool,
    reveal_all: bool,
    view: LineView,
    hide_blank: bool,
    /// Kalem's formatted spans of the line's element.
    spans: Vec<(Range<usize>, crate::rich::CharFormat)>,
}

/// One step right (or left) of `pos` over the text the rich view shows,
/// hidden markers skipped as the arrow keys skip them; `None` without a
/// current parse (not an Org document) or at the line's end (start).
pub fn visible_step(doc: &crate::DocumentState, pos: usize, right: bool) -> Option<usize> {
    let (parse, fresh) = doc.parse()?;
    if !fresh {
        return None;
    }
    let text = doc.text();
    let line = text.line_of(pos);
    let mut range = text.line_range(line);
    if text.as_str()[range.clone()].ends_with('\r') {
        range.end -= 1;
    }
    let v = line_view(&parse.syntax(), parse.context(), range, Some(pos));
    let next = if right {
        v.next_position(pos).map(|p| v.run_start_at(p))
    } else {
        v.prev_position(pos)
    }?;
    (next != pos).then_some(next)
}

/// Builds the view of the source line `line` (without its line feed) with
/// the cursor at `cursor`.
pub fn line_view(
    root: &SyntaxNode,
    ctx: &ParseContext,
    line: Range<usize>,
    cursor: Option<usize>,
) -> LineView {
    line_view_with(root, ctx, line, cursor, false)
}

/// A heading in an outline panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    /// Its level.
    pub level: usize,
    /// Its TODO keyword, and whether it is a done state.
    pub todo: Option<(String, bool)>,
    /// Its title.
    pub title: String,
    /// Where it starts.
    pub start: usize,
}

/// The headings of `doc` for an outline panel (inlinetasks left out).
pub fn outline_items(doc: &org_model::Document) -> Vec<OutlineItem> {
    let ctx = doc.parse().context();
    doc.outline()
        .entries
        .iter()
        .filter(|e| !e.inlinetask)
        .map(|e| OutlineItem {
            level: e.level,
            todo: e.todo.clone().map(|t| {
                let done = ctx.is_done_keyword(&t);
                (t, done)
            }),
            title: e.raw_title.clone(),
            start: usize::from(e.range.start()),
        })
        .collect()
}

/// The source view's line (T1.5.13): every byte of `line` shown as it is,
/// with the styles of the rich view as Org highlighting. Nothing changes
/// size: no widgets, scripts or titles, and every line is monospace. Bytes
/// the rich view leaves out are dimmed.
pub fn source_line_view(
    root: &SyntaxNode,
    ctx: &ParseContext,
    text: &str,
    line: Range<usize>,
) -> LineView {
    let v = line_view_with(root, ctx, line.clone(), None, true);
    let gap = |a: usize, b: usize| Run {
        src: a..b,
        text: text[a..b].to_string(),
        verbatim: true,
        style: Style {
            dim: true,
            ..Style::default()
        },
        widget: None,
    };
    let mut runs = Vec::new();
    let mut at = line.start;
    for r in v.runs {
        let src = r.src.start.max(at)..r.src.end.min(line.end);
        if src.start >= src.end {
            continue;
        }
        if src.start > at {
            runs.push(gap(at, src.start));
        }
        runs.push(Run {
            text: text[src.clone()].to_string(),
            verbatim: true,
            style: Style {
                superscript: false,
                subscript: false,
                title: false,
                ..r.style
            },
            widget: None,
            src: src.clone(),
        });
        at = src.end;
    }
    if at < line.end {
        runs.push(gap(at, line.end));
    }
    LineView {
        range: line,
        runs,
        heading: v.heading,
        role: v.role,
        mono: true,
        align: crate::rich::Align::Left,
    }
}

/// [`line_view`], with all markup shown if `reveal_all` (styles stay):
/// for a table being edited, whose columns follow the source.
pub fn line_view_with(
    root: &SyntaxNode,
    ctx: &ParseContext,
    line: Range<usize>,
    cursor: Option<usize>,
    reveal_all: bool,
) -> LineView {
    let mut b = LineBuilder {
        ctx,
        cursor,
        reveal_all,
        on_line: reveal_all || cursor.is_some_and(|c| line.start <= c && c <= line.end),
        view: LineView {
            range: line.clone(),
            ..LineView::default()
        },
        hide_blank: false,
        spans: Vec::new(),
    };
    let len = end(root);
    if line.start >= line.end || line.start >= len {
        return b.view;
    }
    let Some(mut tok) = root
        .token_at_offset(TextSize::from(line.start as u32))
        .right_biased()
    else {
        return b.view;
    };
    // Kalem's formatting: the spans and alignment of the line's element.
    if let Some(el) = tok.parent_ancestors().find(|a| {
        matches!(
            a.kind(),
            PARAGRAPH | HEADLINE | INLINETASK | VERSE_BLOCK | TABLE_ROW
        )
    }) {
        if el.kind() == PARAGRAPH {
            b.view.align = crate::rich::align(&el);
        }
        if el.text().contains_char('@') {
            b.spans = crate::rich::spans(&el);
        }
    }
    loop {
        let r = tok.text_range();
        let (ts, te) = (usize::from(r.start()), usize::from(r.end()));
        if ts >= line.end {
            break;
        }
        let (s, e) = (ts.max(line.start), te.min(line.end));
        if s < e {
            b.token(&tok, s, e);
        }
        match tok.next_token() {
            Some(t) => tok = t,
            None => break,
        }
    }
    b.view
}

impl LineBuilder<'_> {
    /// Whether an object's markup shows.
    fn shows(&self, n: &SyntaxNode) -> bool {
        self.reveal_all || revealed(n, self.cursor)
    }

    fn push(
        &mut self,
        src: Range<usize>,
        text: String,
        verbatim: bool,
        style: Style,
        widget: Option<Widget>,
    ) {
        self.view.runs.push(Run {
            src,
            text,
            verbatim,
            style,
            widget,
        });
    }

    fn token(&mut self, tok: &SyntaxToken, s: usize, e: usize) {
        let kind = tok.kind();
        let tstart = usize::from(tok.text_range().start());
        let text = &tok.text()[s - tstart..e - tstart];
        let parent = tok.parent();
        // Trailing blanks of objects belong to their nodes but are not
        // styled with them.
        let mut blank_of: Vec<SyntaxNode> = Vec::new();
        if kind == WHITESPACE {
            let mut node = tok.parent();
            while let Some(n) = node {
                if n.kind().is_object() && n.text_range().end() == tok.text_range().end() {
                    blank_of.push(n.clone());
                    node = n.parent();
                } else {
                    break;
                }
            }
        }
        // Objects drawn as widgets.
        for a in tok.parent_ancestors() {
            if blank_of.contains(&a) || !matches!(a.kind(), LATEX_FRAGMENT | LINK) || self.shows(&a)
            {
                continue;
            }
            if let Some(w) = widget_of(&a, self.ctx) {
                let r = reveal_range(&a);
                if s == r.start {
                    self.push(r, PLACEHOLDER.into(), false, Style::default(), Some(w));
                }
                self.hide_blank = false;
                return;
            }
        }
        // Kalem's formatting snippets never show in the rich view; their
        // trailing blanks do.
        if tok
            .parent_ancestors()
            .any(|a| !blank_of.contains(&a) && crate::rich::is_marker(&a))
            && !self.reveal_all
        {
            return;
        }
        let mut style = Style::default();
        if let Some((_, f)) = self.spans.iter().find(|(r, _)| r.start <= s && s < r.end) {
            style.rich = *f;
        }
        let mut shown: Option<String> = None;
        let mut hidden = false;
        for a in tok.parent_ancestors() {
            if blank_of.contains(&a) {
                continue;
            }
            match a.kind() {
                BOLD => style.bold = true,
                ITALIC => style.italic = true,
                UNDERLINE => style.underline = true,
                STRIKE_THROUGH => style.strike = true,
                CODE | VERBATIM | INLINE_SRC_BLOCK => style.code = true,
                LINK => style.link = true,
                TIMESTAMP => style.timestamp = true,
                SUPERSCRIPT => style.superscript = true,
                SUBSCRIPT => style.subscript = true,
                FOOTNOTE_REFERENCE => style.footnote = true,
                TARGET | RADIO_TARGET => style.target = true,
                STATISTICS_COOKIE => style.cookie = true,
                SRC_BLOCK | EXAMPLE_BLOCK | EXPORT_BLOCK | FIXED_WIDTH | TABLE => {
                    self.view.mono = true;
                }
                // A comment block is not exported: dimmed.
                COMMENT_BLOCK => {
                    self.view.mono = true;
                    style.dim = true;
                }
                BLOCK_BEGIN | BLOCK_END => {
                    style.dim = true;
                    self.view.role = LineRole::Delimiter;
                }
                PROPERTY_DRAWER | DRAWER | PLANNING | CLOCK | AFFILIATED_KEYWORD | COMMENT
                | DIARY_SEXP => {
                    style.dim = true;
                }
                KEYWORD => {
                    let key = keyword_key(&a);
                    let title = key.eq_ignore_ascii_case("TITLE");
                    let byline = BYLINE.iter().any(|k| key.eq_ignore_ascii_case(k));
                    if title || byline {
                        style.title = title;
                        style.byline = byline;
                        // `#+TITLE:` and the blank after it.
                        let value = a
                            .children_with_tokens()
                            .find(|t| !matches!(t.kind(), MARKER | KEY | WHITESPACE))
                            .map_or(end(&a), |t| usize::from(t.text_range().start()));
                        if !self.on_line && s < value {
                            hidden = true;
                        }
                    } else {
                        style.dim = true;
                    }
                }
                HEADLINE | INLINETASK if kind != NEWLINE && on_first_line(&a, s) => {
                    style.bold = true;
                    if self.view.heading == 0 {
                        let stars = a
                            .children_with_tokens()
                            .find(|t| t.kind() == STARS)
                            .map_or(1, |t| usize::from(t.text_range().len()));
                        let level = if self.ctx.odd_levels_only {
                            stars.div_ceil(2)
                        } else {
                            stars
                        };
                        self.view.heading = level.min(6) as u8;
                    }
                }
                _ => {}
            }
        }
        match kind {
            NEWLINE | BLANK_LINE => hidden = true,
            STARS
                if !self.on_line
                    && parent
                        .as_ref()
                        .is_some_and(|p| matches!(p.kind(), HEADLINE | INLINETASK)) =>
            {
                hidden = true;
                self.hide_blank = true;
            }
            WHITESPACE if self.hide_blank => {
                hidden = true;
            }
            TODO_KEYWORD => style.todo = Some(self.ctx.is_done_keyword(text)),
            PRIORITY => style.priority = true,
            COMMENT_KEYWORD => style.dim = true,
            TAGS => {
                style.tag = true;
                style.bold = false;
            }
            BULLET if !self.on_line && matches!(text.trim(), "-" | "+" | "*") => {
                shown = Some(text.replace(['-', '+', '*'], "•"));
            }
            CHECKBOX if !self.on_line => {
                let state = match text {
                    "[X]" | "[x]" => CheckState::Checked,
                    "[-]" => CheckState::Partial,
                    _ => CheckState::Unchecked,
                };
                self.push(
                    s..e,
                    PLACEHOLDER.into(),
                    false,
                    style,
                    Some(Widget::Checkbox(state)),
                );
                return;
            }
            MARKER | TEXT
                if parent
                    .as_ref()
                    .is_some_and(|p| p.kind() == LINE_BREAK && !self.shows(p)) =>
            {
                // `\\` at the end of a line: a return sign.
                shown = Some("↵".into());
                style.dim = true;
            }
            MARKER => {
                if let Some(p) = &parent
                    && (HIDABLE.contains(&p.kind()) || p.kind() == ENTITY)
                    && !self.shows(p)
                {
                    hidden = true;
                }
            }
            CODE_TEXT => {
                // A link's target is hidden when the link has a description.
                if let Some(p) = &parent
                    && p.kind() == LINK
                    && p.children_with_tokens()
                        .filter(|t| t.kind() == MARKER)
                        .count()
                        == 3
                    && !self.shows(p)
                {
                    hidden = true;
                }
            }
            KEY => {
                if let Some(p) = &parent
                    && p.kind() == ENTITY
                    && !self.shows(p)
                    && let Some(u) =
                        ast::AstNode::cast(p.clone()).and_then(|e: ast::Entity| e.utf8())
                {
                    shown = Some(u.to_string());
                }
                if let Some(p) = &parent
                    && p.kind() == FOOTNOTE_REFERENCE
                    && !self.shows(p)
                {
                    // The label, without `fn:` and brackets.
                    style.footnote = true;
                }
            }
            _ => {}
        }
        if !matches!(kind, WHITESPACE | STARS) {
            self.hide_blank = false;
        }
        if kind == WHITESPACE && hidden {
            self.hide_blank = false;
        }
        if hidden {
            return;
        }
        let verbatim = shown.is_none();
        self.push(
            s..e,
            shown.unwrap_or_else(|| text.to_string()),
            verbatim,
            style,
            None,
        );
    }
}

fn on_first_line(headline: &SyntaxNode, pos: usize) -> bool {
    let s = start(headline);
    let first_end = headline
        .children_with_tokens()
        .find(|t| t.kind() == NEWLINE)
        .map_or(end(headline), |t| usize::from(t.text_range().start()));
    s <= pos && pos < first_end.max(s + 1)
}

/// A table cell: its content without the padding, and its runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    /// The content, trimmed.
    pub range: Range<usize>,
    /// The runs shown for it.
    pub runs: Vec<Run>,
}

/// A table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableRow {
    /// Fields.
    Data {
        /// The source line, without its line feed.
        line: Range<usize>,
        /// The cells.
        cells: Vec<TableCell>,
    },
    /// A rule (`|---+---|`).
    Rule {
        /// The source line.
        line: Range<usize>,
    },
}

impl TableRow {
    /// The row's source line.
    pub fn line(&self) -> &Range<usize> {
        match self {
            TableRow::Data { line, .. } | TableRow::Rule { line } => line,
        }
    }
}

/// An Org table as a grid, for drawing it aligned whatever its source
/// spacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableView {
    /// The table's source.
    pub range: Range<usize>,
    /// The rows.
    pub rows: Vec<TableRow>,
    /// Column alignment as `org-table-align` decides it: `'l'`, `'r'`
    /// (numbers) or `'c'`.
    pub align: Vec<char>,
}

/// The runs of `view` within `range`, cut at its ends.
fn runs_within(view: &LineView, range: &Range<usize>) -> Vec<Run> {
    view.runs
        .iter()
        .filter(|r| r.src.start < range.end && r.src.end > range.start)
        .map(|r| {
            if !r.verbatim || (r.src.start >= range.start && r.src.end <= range.end) {
                return r.clone();
            }
            let (a, b) = (r.src.start.max(range.start), r.src.end.min(range.end));
            Run {
                src: a..b,
                text: r.text[a - r.src.start..b - r.src.start].to_string(),
                ..r.clone()
            }
        })
        .collect()
}

/// The Org table at `pos` (not table.el tables), with the cursor at
/// `cursor` for revealing markup.
pub fn table_view(
    root: &SyntaxNode,
    ctx: &ParseContext,
    pos: usize,
    cursor: Option<usize>,
) -> Option<TableView> {
    if pos >= end(root) {
        return None;
    }
    let tok = root
        .token_at_offset(TextSize::from(pos as u32))
        .right_biased()?;
    let table = tok.parent_ancestors().find(|a| a.kind() == TABLE)?;
    if table.text().to_string().trim_start().starts_with('+') {
        return None;
    }
    let mut rows = Vec::new();
    for row in table.children().filter(|r| r.kind() == TABLE_ROW) {
        let s = start(&row);
        let e = row
            .children_with_tokens()
            .find(|t| t.kind() == NEWLINE)
            .map_or(end(&row), |t| usize::from(t.text_range().start()));
        let cells: Vec<SyntaxNode> = row.children().filter(|c| c.kind() == TABLE_CELL).collect();
        if cells.is_empty() {
            rows.push(TableRow::Rule { line: s..e });
            continue;
        }
        let view = line_view(root, ctx, s..e, cursor);
        let text = row.text().to_string();
        let cells = cells
            .iter()
            .map(|c| {
                let (cs, mut ce) = (start(c), end(c));
                if c.last_token()
                    .is_some_and(|t| t.kind() == MARKER && t.text() == "|")
                {
                    ce -= 1;
                }
                let raw = &text[cs - s..ce - s];
                let lead = raw.len() - raw.trim_start_matches([' ', '\t']).len();
                let trail = raw.len() - raw.trim_end_matches([' ', '\t']).len();
                let range = (cs + lead)..(ce - trail).max(cs + lead);
                let runs = runs_within(&view, &range);
                TableCell { range, runs }
            })
            .collect();
        rows.push(TableRow::Data { line: s..e, cells });
    }
    let source = table.text().to_string();
    let base = start(&table);
    let n = rows
        .iter()
        .map(|r| match r {
            TableRow::Data { cells, .. } => cells.len(),
            TableRow::Rule { .. } => 0,
        })
        .max()
        .unwrap_or(0);
    let align = (0..n)
        .map(|i| {
            org_edit::table::column_alignment(rows.iter().filter_map(|r| {
                match r {
                    TableRow::Data { cells, .. } => Some(
                        cells
                            .get(i)
                            .map_or("", |c| &source[c.range.start - base..c.range.end - base]),
                    ),
                    TableRow::Rule { .. } => None,
                }
            }))
        })
        .collect();
    Some(TableView {
        range: base..end(&table),
        rows,
        align,
    })
}

/// Keywords shown as the document's byline.
const BYLINE: &[&str] = &["AUTHOR", "DATE", "SUBTITLE", "EMAIL"];

/// The key of a keyword node.
fn keyword_key(n: &SyntaxNode) -> String {
    n.children_with_tokens()
        .find(|t| t.kind() == KEY)
        .map(|t| t.to_string())
        .unwrap_or_default()
}

/// The kinds of blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// A headline's first line (and the blank lines after it).
    Heading {
        /// The level, 1 and up.
        level: usize,
    },
    /// An inline task, whole.
    Inlinetask,
    /// A paragraph.
    Paragraph,
    /// A top-level item of a plain list, with its sub-items.
    ListItem,
    /// A table.
    Table,
    /// A source block.
    Code {
        /// Its language.
        language: Option<String>,
    },
    /// An example, export or comment block, or fixed-width lines.
    Verbatim,
    /// A quote block.
    Quote,
    /// A center block.
    Center,
    /// A verse block.
    Verse,
    /// An export block (`#+begin_export html`), with its back-end.
    Export {
        /// `html`, `latex`…
        backend: Option<String>,
    },
    /// A comment block, left out of exports.
    CommentBlock,
    /// A special block (`#+begin_note`).
    Special,
    /// A dynamic block.
    Dynamic,
    /// A drawer.
    Drawer,
    /// A property drawer.
    Properties,
    /// A planning line.
    Planning,
    /// A clock line.
    Clock,
    /// `#+TITLE:`.
    Title,
    /// `#+AUTHOR:`, `#+DATE:`, `#+SUBTITLE:` or `#+EMAIL:`.
    Byline,
    /// Another keyword or a babel call.
    Keyword,
    /// A horizontal rule.
    Rule,
    /// Comment lines.
    Comment,
    /// A footnote definition.
    Footnote,
    /// A LaTeX environment.
    Math,
    /// Blank lines not after any element (the start of the document).
    Blank,
    /// Anything else.
    Other,
}

impl BlockKind {
    /// The language a block's lines are highlighted in: a source block's,
    /// or the back-end of an export block (`html`, `latex`, `md`).
    pub fn highlight_language(&self) -> Option<&str> {
        match self {
            BlockKind::Code { language } => language.as_deref(),
            BlockKind::Export { backend } => match backend.as_deref()? {
                "md" | "gfm" => Some("markdown"),
                "ascii" | "utf-8" => None,
                b => Some(b),
            },
            _ => None,
        }
    }

    /// What the first line of a block shows away from the cursor: a
    /// source block's language, `export html`, or the block's type
    /// (`example`, `quote`) taken from `first_line`.
    pub fn label(&self, first_line: &str) -> String {
        match self {
            BlockKind::Code { language } => language.clone().unwrap_or_else(|| "code".into()),
            BlockKind::Export { backend } => match backend {
                Some(b) => format!("export {b}"),
                None => "export".into(),
            },
            _ => first_line
                .trim()
                .get(7..)
                .unwrap_or("")
                .trim_start_matches(['_', ':'])
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase(),
        }
    }

    /// Code-like: monospace on a background, with a copy button.
    pub fn is_code(&self) -> bool {
        matches!(
            self,
            BlockKind::Code { .. } | BlockKind::Export { .. } | BlockKind::Verbatim
        )
    }
}

/// A block: whole source lines drawn together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// What it is.
    pub kind: BlockKind,
    /// Its source, with the blank lines after it.
    pub range: Range<usize>,
    /// The end of its content, before those blank lines.
    pub content_end: usize,
    /// The level of the headline it is under (its own for headings), 0
    /// before the first headline.
    pub depth: usize,
    /// The start of that headline.
    pub headline: Option<usize>,
}

fn kind_of(n: &SyntaxNode) -> BlockKind {
    match n.kind() {
        PARAGRAPH => BlockKind::Paragraph,
        TABLE => BlockKind::Table,
        SRC_BLOCK => BlockKind::Code {
            language: ast::AstNode::cast(n.clone()).and_then(|b: ast::SrcBlock| b.language()),
        },
        EXPORT_BLOCK => BlockKind::Export {
            backend: n
                .children()
                .find(|c| c.kind() == BLOCK_BEGIN)
                .and_then(|b| {
                    b.text()
                        .to_string()
                        .split_whitespace()
                        .nth(1)
                        .map(str::to_ascii_lowercase)
                }),
        },
        COMMENT_BLOCK => BlockKind::CommentBlock,
        EXAMPLE_BLOCK | FIXED_WIDTH => BlockKind::Verbatim,
        QUOTE_BLOCK => BlockKind::Quote,
        CENTER_BLOCK => BlockKind::Center,
        VERSE_BLOCK => BlockKind::Verse,
        SPECIAL_BLOCK => BlockKind::Special,
        DYNAMIC_BLOCK => BlockKind::Dynamic,
        DRAWER => BlockKind::Drawer,
        PROPERTY_DRAWER => BlockKind::Properties,
        PLANNING => BlockKind::Planning,
        CLOCK => BlockKind::Clock,
        KEYWORD => {
            let key = keyword_key(n);
            if key.eq_ignore_ascii_case("TITLE") {
                BlockKind::Title
            } else if BYLINE.iter().any(|k| key.eq_ignore_ascii_case(k)) {
                BlockKind::Byline
            } else {
                BlockKind::Keyword
            }
        }
        BABEL_CALL => BlockKind::Keyword,
        HORIZONTAL_RULE => BlockKind::Rule,
        COMMENT => BlockKind::Comment,
        FOOTNOTE_DEFINITION => BlockKind::Footnote,
        LATEX_ENVIRONMENT => BlockKind::Math,
        INLINETASK => BlockKind::Inlinetask,
        _ => BlockKind::Other,
    }
}

fn content_end(text: &str, r: &Range<usize>) -> usize {
    let t = &text[r.clone()];
    let trimmed = t.trim_end_matches(['\n', ' ', '\t', '\r']);
    // Keep the line feed of the last content line.
    let e = r.start + trimmed.len();
    if e < r.end {
        (e + t[trimmed.len()..].find('\n').map_or(0, |i| i + 1)).min(r.end)
    } else {
        e
    }
}

struct Blocks<'a> {
    text: String,
    ctx: &'a ParseContext,
    out: Vec<Block>,
}

impl Blocks<'_> {
    fn add(&mut self, kind: BlockKind, range: Range<usize>, depth: usize, headline: Option<usize>) {
        let content_end = content_end(&self.text, &range);
        self.out.push(Block {
            kind,
            range,
            content_end,
            depth,
            headline,
        });
    }

    fn walk(&mut self, node: &SyntaxNode, depth: usize, headline: Option<usize>) {
        for c in node.children() {
            match c.kind() {
                k if !k.is_element() => {}
                SECTION => self.walk(&c, depth, headline),
                HEADLINE => {
                    let stars = c
                        .children_with_tokens()
                        .find(|t| t.kind() == STARS)
                        .map_or(1, |t| usize::from(t.text_range().len()));
                    let level = if self.ctx.odd_levels_only {
                        stars.div_ceil(2)
                    } else {
                        stars
                    };
                    let first_child = c
                        .children()
                        .find(|n| matches!(n.kind(), SECTION | HEADLINE))
                        .map_or(end(&c), |n| start(&n));
                    let h = start(&c);
                    self.add(BlockKind::Heading { level }, h..first_child, level, Some(h));
                    self.walk(&c, level, Some(h));
                }
                PLAIN_LIST => {
                    for item in c.children().filter(|i| i.kind() == ITEM) {
                        self.add(
                            BlockKind::ListItem,
                            start(&item)..end(&item),
                            depth,
                            headline,
                        );
                    }
                }
                _ => self.add(kind_of(&c), start(&c)..end(&c), depth, headline),
            }
        }
    }
}

/// The document's blocks, in order. Together they cover the whole text.
pub fn blocks(root: &SyntaxNode, ctx: &ParseContext) -> Vec<Block> {
    let mut b = Blocks {
        text: root.text().to_string(),
        ctx,
        out: Vec::new(),
    };
    b.walk(root, 0, None);
    // Close gaps (blank lines owned by containers) so the blocks cover the
    // text.
    let len = b.text.len();
    let found = std::mem::take(&mut b.out);
    for block in found {
        match b.out.last_mut() {
            Some(last) if last.range.end < block.range.start => last.range.end = block.range.start,
            None if block.range.start > 0 => b.add(BlockKind::Blank, 0..block.range.start, 0, None),
            _ => {}
        }
        b.out.push(block);
    }
    match b.out.last_mut() {
        Some(last) => last.range.end = last.range.end.max(len),
        None if len > 0 => b.add(BlockKind::Blank, 0..len, 0, None),
        None => {}
    }
    b.out
}

/// What of a document shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visible {
    /// Byte ranges that show, merged and in order.
    pub ranges: Vec<Range<usize>>,
    /// Starts of blocks folded to their first line away from the cursor:
    /// drawers and runs of setting keywords.
    pub folded: std::collections::HashSet<usize>,
}

/// The part of the document a view shows, if not all of it: the narrowed
/// part, or in focus mode the section holding the cursor (its innermost
/// headline's subtree, or the text before the first headline).
pub fn limit(doc: &crate::DocumentState, focus: bool) -> Option<Range<usize>> {
    if let Some(r) = &doc.narrowing {
        return Some(r.clone());
    }
    if !focus {
        return None;
    }
    let text = doc.text().as_str();
    let ctx = doc
        .parse()
        .map(|(p, _)| p.context().clone())
        .unwrap_or_default();
    let head = doc.selection.head.min(text.len());
    Some(
        org_edit::headline::subtree_range(text, head, &ctx).unwrap_or_else(|_| {
            // Before the first headline.
            let mut at = 0;
            for line in text.split_inclusive('\n') {
                let stars = line.len() - line.trim_start_matches('*').len();
                if stars > 0 && line[stars..].starts_with(' ') {
                    return 0..at;
                }
                at += line.len();
            }
            0..text.len()
        }),
    )
}

/// `ranges` cut to `to`.
pub fn clip(ranges: &[Range<usize>], to: &Range<usize>) -> Vec<Range<usize>> {
    ranges
        .iter()
        .filter_map(|r| {
            let (s, e) = (r.start.max(to.start), r.end.min(to.end));
            (s < e || (s == e && s == to.start)).then_some(s..e)
        })
        .collect()
}

/// What shows of `text` with these blocks, folds and the cursor at
/// `cursor`: folded headlines hide their content, drawers away from the
/// cursor show their first line, and so do runs of two or more setting
/// keywords.
pub fn visible(text: &str, blocks: &[Block], folds: &Folds, cursor: usize) -> Visible {
    let mut out = Visible::default();
    let first_line_end = |b: &Block| {
        text[b.range.start..]
            .find('\n')
            .map_or(text.len(), |i| b.range.start + i + 1)
            .min(b.range.end)
    };
    let push = |ranges: &mut Vec<Range<usize>>, r: Range<usize>| {
        if r.is_empty() {
            return;
        }
        match ranges.last_mut() {
            Some(last) if last.end == r.start => last.end = r.end,
            _ => ranges.push(r),
        }
    };
    let shown = folds.visible(blocks);
    let inside = |a: &Block, z: &Block| a.range.start <= cursor && cursor <= z.content_end;
    let mut i = 0;
    while i < shown.len() {
        let b = shown[i];
        match b.kind {
            BlockKind::Properties | BlockKind::Drawer
                if !inside(b, b) && first_line_end(b) < b.content_end =>
            {
                push(&mut out.ranges, b.range.start..first_line_end(b));
                push(&mut out.ranges, b.content_end..b.range.end);
                out.folded.insert(b.range.start);
            }
            BlockKind::Keyword => {
                let mut j = i + 1;
                while j < shown.len()
                    && shown[j].kind == BlockKind::Keyword
                    && shown[j].range.start == shown[j - 1].range.end
                {
                    j += 1;
                }
                let last = shown[j - 1];
                if j - i >= 2 && !inside(b, last) {
                    push(&mut out.ranges, b.range.start..first_line_end(b));
                    push(&mut out.ranges, last.content_end..last.range.end);
                    out.folded.insert(b.range.start);
                } else {
                    for k in &shown[i..j] {
                        push(&mut out.ranges, k.range.clone());
                    }
                }
                i = j;
                continue;
            }
            // `#+ATTR_KALEM:` lines above a paragraph do not show: they
            // are its alignment.
            BlockKind::Paragraph if crate::rich::is_attr_line(text, b.range.start) => {
                let mut at = b.range.start;
                while at < b.content_end && crate::rich::is_attr_line(text, at) {
                    at = text[at..].find('\n').map_or(text.len(), |n| at + n + 1);
                }
                push(&mut out.ranges, at.min(b.range.end)..b.range.end);
            }
            _ => push(&mut out.ranges, b.range.clone()),
        }
        i += 1;
    }
    out
}

/// How a headline is folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    /// Everything under the heading line is hidden.
    Subtree,
    /// The headline's own content is hidden; sub-headlines show.
    Body,
}

/// The visibility of a headline, as `org-cycle` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Only the heading line.
    Folded,
    /// The heading and its direct children's headings.
    Children,
    /// Everything.
    Subtree,
}

/// Folded headlines, by the start of their heading line. View state: kept
/// by the editor per document, moved through edits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds {
    folds: BTreeMap<usize, Fold>,
}

fn heading_level(b: &Block) -> Option<usize> {
    match b.kind {
        BlockKind::Heading { level } => Some(level),
        _ => None,
    }
}

/// The heading blocks of the subtree of heading block `i`, after it.
fn subtree(blocks: &[Block], i: usize) -> Range<usize> {
    let level = heading_level(&blocks[i]).unwrap_or(0);
    let end = blocks[i + 1..]
        .iter()
        .position(|b| heading_level(b).is_some_and(|l| l <= level))
        .map_or(blocks.len(), |n| i + 1 + n);
    i + 1..end
}

impl Folds {
    /// How the headline at `heading` is folded.
    pub fn get(&self, heading: usize) -> Option<Fold> {
        self.folds.get(&heading).copied()
    }

    /// Folds or unfolds a headline.
    pub fn set(&mut self, heading: usize, fold: Option<Fold>) {
        match fold {
            Some(f) => self.folds.insert(heading, f),
            None => self.folds.remove(&heading),
        };
    }

    /// Moves the folds through an edit. Folds whose headline is gone are
    /// dropped by the next [`Folds::retain`].
    pub fn map(&mut self, tx: &Transaction) {
        self.folds = self
            .folds
            .iter()
            .map(|(h, f)| (tx.map(*h, Assoc::After), *f))
            .collect();
    }

    /// Drops folds that are not at a heading.
    pub fn retain(&mut self, blocks: &[Block]) {
        self.folds.retain(|h, _| {
            blocks
                .iter()
                .any(|b| b.range.start == *h && heading_level(b).is_some())
        });
    }

    /// The blocks that show, in order.
    pub fn visible<'a>(&self, blocks: &'a [Block]) -> Vec<&'a Block> {
        let mut out = Vec::new();
        // Folded ancestors: (level, fold).
        let mut stack: Vec<(usize, Fold)> = Vec::new();
        for b in blocks {
            if let Some(level) = heading_level(b) {
                while stack.last().is_some_and(|(l, _)| *l >= level) {
                    stack.pop();
                }
                if stack.iter().any(|(_, f)| *f == Fold::Subtree) {
                    continue;
                }
                out.push(b);
                if let Some(f) = self.get(b.range.start) {
                    stack.push((level, f));
                }
            } else if stack.is_empty() || (b.headline.is_none()) {
                out.push(b);
            } else {
                // Content is hidden by any folded ancestor: a Subtree fold
                // above, or a Body fold of its own headline.
                let own = stack.last().is_some_and(|(l, _)| *l == b.depth);
                let hidden = stack.iter().any(|(_, f)| *f == Fold::Subtree)
                    || (own && stack.last().is_some_and(|(_, f)| *f == Fold::Body));
                if !hidden {
                    out.push(b);
                }
            }
        }
        out
    }

    /// The visibility of heading block `i`.
    pub fn visibility(&self, blocks: &[Block], i: usize) -> Visibility {
        let h = blocks[i].range.start;
        match self.get(h) {
            Some(Fold::Subtree) => Visibility::Folded,
            Some(Fold::Body) => Visibility::Children,
            None => Visibility::Subtree,
        }
    }

    /// `org-cycle` on heading block `i`: folded, children, subtree, and
    /// folded again. A headline without sub-headlines goes from folded to
    /// subtree.
    pub fn cycle(&mut self, blocks: &[Block], i: usize) -> Visibility {
        let Some(level) = heading_level(&blocks[i]) else {
            return Visibility::Subtree;
        };
        let sub = subtree(blocks, i);
        let children: Vec<usize> = sub
            .clone()
            .filter(|&j| heading_level(&blocks[j]).is_some_and(|l| l > level))
            .filter(|&j| {
                // Direct children: no heading between them and `i` at a
                // level between.
                let l = heading_level(&blocks[j]).expect("heading");
                !(i + 1..j).any(|k| heading_level(&blocks[k]).is_some_and(|m| m > level && m < l))
            })
            .collect();
        let clear = |f: &mut Folds| {
            for j in sub.clone() {
                f.folds.remove(&blocks[j].range.start);
            }
        };
        let h = blocks[i].range.start;
        match self.visibility(blocks, i) {
            Visibility::Folded if children.is_empty() => {
                self.set(h, None);
                clear(self);
                Visibility::Subtree
            }
            Visibility::Folded => {
                clear(self);
                self.set(h, Some(Fold::Body));
                for j in children {
                    self.set(blocks[j].range.start, Some(Fold::Subtree));
                }
                Visibility::Children
            }
            Visibility::Children => {
                self.set(h, None);
                clear(self);
                Visibility::Subtree
            }
            Visibility::Subtree => {
                clear(self);
                self.set(h, Some(Fold::Subtree));
                Visibility::Folded
            }
        }
    }

    /// `#+STARTUP` visibility and global cycling: `overview` shows the
    /// top-level headings, `content` all headings, `showall` everything.
    pub fn startup(blocks: &[Block], option: &str) -> Folds {
        let mut f = Folds::default();
        let min = blocks.iter().filter_map(heading_level).min().unwrap_or(1);
        for b in blocks {
            let Some(level) = heading_level(b) else {
                continue;
            };
            match option {
                "overview" | "fold" if level == min => f.set(b.range.start, Some(Fold::Subtree)),
                "content" => f.set(b.range.start, Some(Fold::Body)),
                _ => {}
            }
        }
        f
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn export_and_comment_blocks() {
        let t = "#+begin_export html\n<b>x</b>\n#+end_export\n#+begin_comment\nhidden\n#+end_comment\n#+begin_export md\n*x*\n#+end_export\n";
        let p = org_syntax::parse(t);
        let kinds: Vec<BlockKind> = blocks(&p.syntax(), p.context())
            .into_iter()
            .map(|b| b.kind)
            .collect();
        assert_eq!(
            kinds[0],
            BlockKind::Export {
                backend: Some("html".into())
            }
        );
        assert_eq!(kinds[1], BlockKind::CommentBlock);
        assert_eq!(kinds[0].highlight_language(), Some("html"));
        assert_eq!(kinds[2].highlight_language(), Some("markdown"));
        assert_eq!(kinds[0].label("#+begin_export html"), "export html");
        assert_eq!(kinds[1].label("#+begin_comment"), "comment");
        assert!(kinds[0].is_code() && !kinds[1].is_code());
        // Comment lines are dimmed.
        let v = line_view(&p.syntax(), p.context(), lines(t)[4].clone(), None);
        assert!(v.runs.iter().all(|r| r.style.dim), "{v:?}");
    }

    #[test]
    fn image_widths_and_attachments() {
        assert_eq!(ImageWidth::parse("300"), Some(ImageWidth::Pixels(300)));
        assert_eq!(ImageWidth::parse("300px"), Some(ImageWidth::Pixels(300)));
        assert_eq!(ImageWidth::parse("50%"), Some(ImageWidth::Percent(50)));
        assert_eq!(ImageWidth::parse("0.25"), Some(ImageWidth::Percent(25)));
        assert_eq!(ImageWidth::parse("wide"), None);
        assert_eq!(ImageWidth::Percent(50).resolve(800.), 400.);
        let t = "* A\n:PROPERTIES:\n:ID: abcdef-12\n:END:\n#+ATTR_ORG: :width 50%\n[[attachment:pic.png]]\n* B\n:PROPERTIES:\n:DIR: ~/pics/\n:END:\n#+attr_org: :align center :width 120px\n[[attachment:b.jpg]] and [[file:c.png]]\n";
        let p = org_syntax::parse(t);
        let widgets = |line: usize| -> Vec<Widget> {
            let v = line_view(&p.syntax(), p.context(), lines(t)[line].clone(), None);
            v.runs.iter().filter_map(|r| r.widget.clone()).collect()
        };
        assert_eq!(
            widgets(5),
            [Widget::Image {
                path: "data/ab/cdef-12/pic.png".into(),
                width: Some(ImageWidth::Percent(50)),
            }]
        );
        assert_eq!(
            widgets(11),
            [
                Widget::Image {
                    path: "~/pics/b.jpg".into(),
                    width: Some(ImageWidth::Pixels(120)),
                },
                Widget::Image {
                    path: "c.png".into(),
                    width: Some(ImageWidth::Pixels(120)),
                }
            ]
        );
    }
    use super::*;

    fn lines(text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut s = 0;
        for (i, _) in text.match_indices('\n') {
            out.push(s..i);
            s = i + 1;
        }
        if s < text.len() {
            out.push(s..text.len());
        }
        out
    }

    fn show(text: &str, line: usize, cursor: Option<usize>) -> String {
        let p = org_syntax::parse(text);
        line_view(&p.syntax(), p.context(), lines(text)[line].clone(), cursor).display()
    }

    #[test]
    fn hides_markup_away_from_the_cursor() {
        let t = "* TODO Title :tag:\nSome *bold* and [[https://x.org][a link]] and \\alpha.\n- [X] done\n";
        assert_eq!(show(t, 0, None), "TODO Title :tag:");
        assert_eq!(show(t, 0, Some(3)), "* TODO Title :tag:");
        assert_eq!(show(t, 1, None), "Some bold and a link and α.");
        assert_eq!(show(t, 1, Some(19 + 7)), "Some *bold* and a link and α.");
        assert_eq!(
            show(t, 1, Some(19 + 20)),
            "Some bold and [[https://x.org][a link]] and α."
        );
        assert_eq!(show(t, 2, None), "• \u{FFFC} done");
        assert_eq!(show(t, 2, Some(73)), "- [X] done");
        // The blank after an object does not reveal it.
        assert_eq!(show(t, 1, Some(19 + 12)), "Some bold and a link and α.");
        assert_eq!(show(t, 1, Some(19 + 11)), "Some *bold* and a link and α.");
    }

    #[test]
    fn titles_scripts_footnotes_images() {
        let t = "#+TITLE: My doc\nx^{2} and H_2 [fn:1] [[file:a.png]] $E=mc^2$\n";
        assert_eq!(show(t, 0, None), "My doc");
        assert_eq!(show("#+author: Ada\n", 0, None), "Ada");
        assert_eq!(show("one \\\\\ntwo\n", 0, None), "one ↵");
        assert_eq!(show(t, 0, Some(3)), "#+TITLE: My doc");
        let p = org_syntax::parse(t);
        let v = line_view(&p.syntax(), p.context(), lines(t)[1].clone(), None);
        assert_eq!(v.display(), "x2 and H2 1 \u{FFFC} \u{FFFC}");
        let sup = v.runs.iter().find(|r| r.text == "2").unwrap();
        assert!(sup.style.superscript);
        let widgets: Vec<&Widget> = v.runs.iter().filter_map(|r| r.widget.as_ref()).collect();
        assert_eq!(
            widgets,
            [
                &Widget::Image {
                    path: "a.png".into(),
                    width: None,
                },
                &Widget::Math {
                    source: "$E=mc^2$".into(),
                    display: false
                }
            ]
        );
        assert!(
            v.runs
                .iter()
                .find(|r| r.text == "1")
                .unwrap()
                .style
                .footnote
        );
    }

    #[test]
    fn offsets_and_motion() {
        let t = "Some *bold* text\n";
        let p = org_syntax::parse(t);
        let v = line_view(&p.syntax(), p.context(), 0..16, None);
        assert_eq!(v.display(), "Some bold text");
        assert_eq!(v.display_offset(6), 5);
        assert_eq!(v.source_offset(5), 5);
        assert_eq!(v.source_offset(6), 7);
        assert_eq!(v.display_offset(16), 14);
        assert_eq!(v.hidden(), [5..6, 10..11]);
        // Right from before the star skips it.
        assert_eq!(v.next_position(5), Some(7));
        assert_eq!(v.prev_position(7), Some(6));
        assert_eq!(v.next_position(16), None);
        assert_eq!(v.prev_position(0), None);
        // Graphemes.
        let t = "e\u{301}x\n";
        let p = org_syntax::parse(t);
        let v = line_view(&p.syntax(), p.context(), 0..4, None);
        assert_eq!(v.next_position(0), Some(3));
    }

    #[test]
    fn tables() {
        let t = "| Name | Qty |\n|---+---|\n| *a*  |   3 |\n|b| 10|\n";
        let p = org_syntax::parse(t);
        let tv = table_view(&p.syntax(), p.context(), 0, None).unwrap();
        assert_eq!(tv.align, ['l', 'r']);
        assert_eq!(tv.rows.len(), 4);
        assert!(matches!(tv.rows[1], TableRow::Rule { .. }));
        let TableRow::Data { cells, .. } = &tv.rows[2] else {
            panic!("a data row")
        };
        let shown: String = cells[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(shown, "a");
        assert_eq!(&t[cells[1].range.clone()], "3");
        let TableRow::Data { cells, .. } = &tv.rows[3] else {
            panic!("a data row")
        };
        assert_eq!(&t[cells[1].range.clone()], "10");
        // Everything revealed.
        let v = line_view_with(&p.syntax(), p.context(), 25..39, None, true);
        assert_eq!(v.display(), "| *a*  |   3 |");
    }

    #[test]
    fn delimiters_and_mono() {
        let t = "#+begin_src sh\nls\n#+end_src\n";
        let p = org_syntax::parse(t);
        let v = line_view(&p.syntax(), p.context(), 0..14, None);
        assert_eq!(v.role, LineRole::Delimiter);
        let v = line_view(&p.syntax(), p.context(), 15..17, None);
        assert!(v.mono && v.role == LineRole::Content);
    }

    fn corpus() -> Vec<String> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/corpus");
        let mut out = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "org")
                    && let Ok(t) = std::fs::read_to_string(&p)
                {
                    out.push(t);
                }
            }
        }
        out
    }

    #[test]
    fn blocks_cover_the_text() {
        let files = corpus();
        assert!(files.len() > 10);
        for t in &files {
            let p = org_syntax::parse(t);
            let bs = blocks(&p.syntax(), p.context());
            let mut at = 0;
            for b in &bs {
                assert_eq!(b.range.start, at, "gap before {b:?}");
                assert!(
                    b.range.start <= b.content_end && b.content_end <= b.range.end,
                    "{b:?}"
                );
                at = b.range.end;
            }
            assert_eq!(at, t.len());
            // Every line builds, and shown runs are in order within it.
            for l in lines(t) {
                let v = line_view(&p.syntax(), p.context(), l.clone(), Some(l.start));
                let mut prev = l.start;
                for r in &v.runs {
                    assert!(r.src.start >= prev && r.src.end <= l.end, "{r:?} in {l:?}");
                    prev = r.src.end;
                }
            }
        }
    }

    #[test]
    // A single range is a one-range list.
    #[allow(clippy::single_range_in_vec_init)]
    fn limits() {
        let text = "intro\n* A\na\n** A1\nx\n* B\nb\n";
        let meta = crate::Metadata {
            path: None,
            mode: crate::DocumentMode::Org,
            line_ending: crate::LineEnding::Lf,
            bom: false,
        };
        let mut d = crate::DocumentState::new(text, meta, std::sync::Arc::default());
        assert_eq!(limit(&d, false), None);
        d.move_cursor(2, false);
        assert_eq!(limit(&d, true), Some(0..6));
        d.move_cursor(text.find("x").unwrap(), false);
        assert_eq!(limit(&d, true).map(|r| &text[r]), Some("** A1\nx\n"));
        d.move_cursor(text.find("b\n").unwrap(), false);
        assert_eq!(limit(&d, true).map(|r| &text[r]), Some("* B\nb\n"));
        // Narrowing wins.
        d.narrowing = Some(6..10);
        assert_eq!(limit(&d, true), Some(6..10));
        assert_eq!(clip(&[0..4, 8..20], &(6..10)), [8..10]);
        assert!(clip(&[0..4], &(6..10)).is_empty());
    }

    #[test]
    fn source_view_shows_the_source() {
        for t in &corpus() {
            let p = org_syntax::parse(t);
            let root = p.syntax();
            for l in lines(t) {
                let v = source_line_view(&root, p.context(), t, l.clone());
                assert_eq!(v.display(), &t[l.clone()]);
                let mut at = l.start;
                for r in &v.runs {
                    assert_eq!(r.src.start, at);
                    assert!(r.widget.is_none() && r.verbatim);
                    at = r.src.end;
                }
            }
        }
        let t = "* TODO Head :tag:\nSome *bold* \\alpha x^2\n";
        let p = org_syntax::parse(t);
        let v = source_line_view(&p.syntax(), p.context(), t, 0..17);
        assert_eq!(v.heading, 1);
        assert!(
            v.runs
                .iter()
                .any(|r| r.style.todo == Some(false) && r.text == "TODO")
        );
        let v = source_line_view(&p.syntax(), p.context(), t, 18..40);
        assert!(
            v.runs
                .iter()
                .any(|r| r.style.bold && r.text.contains("bold"))
        );
    }

    #[test]
    fn block_kinds() {
        let t = "\n#+TITLE: T\n* A\n\nText.\n- a\n  - b\n- c\n\n** B\n#+begin_src rust\nx\n#+end_src\n* C\n";
        let p = org_syntax::parse(t);
        let bs = blocks(&p.syntax(), p.context());
        let kinds: Vec<(&BlockKind, &str, usize)> = bs
            .iter()
            .map(|b| (&b.kind, &t[b.range.start..b.content_end], b.depth))
            .collect();
        assert_eq!(
            kinds,
            [
                (&BlockKind::Blank, "\n", 0),
                (&BlockKind::Title, "#+TITLE: T\n", 0),
                (&BlockKind::Heading { level: 1 }, "* A\n", 1),
                (&BlockKind::Paragraph, "Text.\n", 1),
                (&BlockKind::ListItem, "- a\n  - b\n", 1),
                (&BlockKind::ListItem, "- c\n", 1),
                (&BlockKind::Heading { level: 2 }, "** B\n", 2),
                (
                    &BlockKind::Code {
                        language: Some("rust".into())
                    },
                    "#+begin_src rust\nx\n#+end_src\n",
                    2
                ),
                (&BlockKind::Heading { level: 1 }, "* C\n", 1),
            ]
        );
    }

    #[test]
    fn folding() {
        let t = "* A\na\n** B\nb\n*** C\nc\n** D\n* E\ne\n";
        let p = org_syntax::parse(t);
        let bs = blocks(&p.syntax(), p.context());
        let shown = |f: &Folds| -> String {
            f.visible(&bs)
                .iter()
                .map(|b| t[b.range.clone()].trim_end().replace('\n', "|"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut f = Folds::default();
        assert_eq!(f.cycle(&bs, 0), Visibility::Folded);
        assert_eq!(shown(&f), "* A * E e");
        assert_eq!(f.cycle(&bs, 0), Visibility::Children);
        assert_eq!(shown(&f), "* A ** B ** D * E e");
        assert_eq!(f.cycle(&bs, 0), Visibility::Subtree);
        assert_eq!(shown(&f), "* A a ** B b *** C c ** D * E e");
        // No sub-headlines: folded, then everything.
        let e = bs
            .iter()
            .position(|b| t[b.range.clone()].starts_with("* E"))
            .unwrap();
        assert_eq!(f.cycle(&bs, e), Visibility::Folded);
        assert_eq!(f.cycle(&bs, e), Visibility::Subtree);
        let f = Folds::startup(&bs, "overview");
        assert_eq!(shown(&f), "* A * E");
        let f = Folds::startup(&bs, "content");
        assert_eq!(shown(&f), "* A ** B *** C ** D * E");
        // Folds follow edits.
        let mut f = Folds::default();
        f.set(8, Some(Fold::Subtree));
        let mut tx = Transaction::new("x");
        tx.insert(0, "new\n").unwrap();
        f.map(&tx);
        assert_eq!(f.get(12), Some(Fold::Subtree));
    }
}
