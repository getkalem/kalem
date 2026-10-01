//! Markdown mode (§2.6.1, T2.7c.2): CommonMark with GitHub's extensions
//! parsed by comrak (D19, Kalem's fork, T2.7c.1a), its line and column
//! positions turned into byte ranges, and each source line drawn as the
//! Org view draws Org: markers hidden away from the cursor (the reveal
//! rule of [`crate::view`]), headings, emphasis, code, links, images,
//! footnote references and formulas, checkboxes as widgets. The text is
//! never rewritten: the view maps every shown character to its source.

use std::ops::Range;
use std::rc::Rc;

use crate::modes::{Detect, Kind, ModeSpec, TextEdit, Tree};
use crate::view::{LineRole, LineView, Run, Style, Widget};

/// What a Markdown node is, as the view needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdKind {
    /// `#` or underlined heading.
    Heading {
        /// 1 to 6.
        level: u8,
        /// Underlined (`===`, `---`).
        setext: bool,
    },
    /// A paragraph.
    Paragraph,
    /// A list.
    List {
        /// Numbered.
        ordered: bool,
    },
    /// A list item.
    Item,
    /// A task list item, with where its box is written.
    TaskItem {
        /// Checked.
        checked: bool,
        /// The `[ ]` or `[x]`.
        boxed: Range<usize>,
    },
    /// `>`.
    Quote,
    /// A code block.
    CodeBlock {
        /// Fenced (as against indented).
        fenced: bool,
        /// The info string's first word.
        language: Option<String>,
    },
    /// Raw HTML as a block.
    HtmlBlock,
    /// `---`.
    Rule,
    /// A table.
    Table,
    /// A table row.
    TableRow,
    /// A table cell.
    TableCell,
    /// Front matter (`---` … `---` at the top).
    FrontMatter,
    /// A footnote definition.
    FootnoteDefinition,
    /// Text.
    Text,
    /// Inline code.
    Code,
    /// `*x*`.
    Emphasis,
    /// `**x**`.
    Strong,
    /// `~~x~~`.
    Strikethrough,
    /// A link, with its destination.
    Link {
        /// The destination.
        url: String,
    },
    /// An image, with its source.
    Image {
        /// The source.
        url: String,
    },
    /// `[^1]`.
    FootnoteRef,
    /// `$x$` or `$$x$$`.
    Math {
        /// Display math.
        display: bool,
    },
    /// Raw HTML inline.
    HtmlInline,
    /// Anything else.
    Other(&'static str),
}

/// A node: its kind, bytes, the bytes of its content (its children, or a
/// code span's text), and its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdNode {
    /// What it is.
    pub kind: MdKind,
    /// Its bytes.
    pub range: Range<usize>,
    /// Its content's bytes: what is left when the markers are hidden.
    pub content: Range<usize>,
    /// Its parent, an index before it.
    pub parent: Option<u32>,
}

/// A parsed Markdown document.
#[derive(Debug, Clone, Default)]
pub struct Md {
    /// The nodes, in document order.
    pub nodes: Vec<MdNode>,
    /// Line starts of the text.
    starts: Vec<usize>,
    /// For each line, the nodes that touch it.
    by_line: Vec<Vec<u32>>,
}

fn options() -> comrak::Options<'static> {
    let mut o = comrak::Options::default();
    o.extension.table = true;
    o.extension.strikethrough = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.math_dollars = true;
    o.extension.front_matter_delimiter = Some("---".to_string());
    o
}

/// Byte offsets of the line starts.
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

impl Md {
    /// Parses `text`.
    pub fn parse(text: &str) -> Md {
        use comrak::nodes::{ListType, NodeValue as V};
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, text, &options());
        let starts = line_starts(text);
        // comrak's columns count bytes from 1; its ends are inclusive.
        let at = |lc: comrak::nodes::LineColumn| -> Option<usize> {
            if lc.line == 0 {
                return None;
            }
            let s = *starts.get(lc.line - 1)?;
            Some((s + lc.column.saturating_sub(1)).min(text.len()))
        };
        let span = |p: comrak::nodes::Sourcepos| -> Option<Range<usize>> {
            let a = at(p.start)?;
            let mut b = (at(p.end)? + 1).min(text.len());
            // A position on a line ending ends before it.
            while b > a && matches!(text.as_bytes()[b - 1], b'\n' | b'\r') {
                b -= 1;
            }
            // Ends inside a character go to its end.
            while b < text.len() && !text.is_char_boundary(b) {
                b += 1;
            }
            Some(a..b.max(a))
        };
        let mut md = Md {
            starts: starts.clone(),
            ..Md::default()
        };
        // Pre-order, each node with its parent's index.
        let mut stack: Vec<(&comrak::nodes::AstNode<'_>, Option<u32>)> = root
            .children()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|c| (c, None))
            .collect();
        while let Some((node, parent)) = stack.pop() {
            let data = node.data();
            let Some(mut range) = span(data.sourcepos) else {
                // No position: its children take the parent.
                let kids: Vec<_> = node.children().collect();
                drop(data);
                for c in kids.into_iter().rev() {
                    stack.push((c, parent));
                }
                continue;
            };
            if let Some(p) = parent {
                let pr = &md.nodes[p as usize].range;
                range =
                    range.start.max(pr.start)..range.end.min(pr.end).max(range.start.max(pr.start));
            }
            let kind = match &data.value {
                V::Heading(h) => MdKind::Heading {
                    level: h.level.clamp(1, 6),
                    setext: h.setext,
                },
                V::Paragraph => MdKind::Paragraph,
                V::List(l) => MdKind::List {
                    ordered: l.list_type == ListType::Ordered,
                },
                V::Item(_) => MdKind::Item,
                V::TaskItem(t) => {
                    let boxed = span(t.symbol_sourcepos)
                        .map(|r| r.start.saturating_sub(1)..(r.end + 1).min(text.len()))
                        .filter(|r| {
                            text.get(r.clone())
                                .is_some_and(|s| s.starts_with('[') && s.ends_with(']'))
                        })
                        .or_else(|| {
                            // The box after the bullet.
                            let s = &text[range.clone()];
                            let i = s.find('[')?;
                            Some(range.start + i..range.start + i + 3)
                        })
                        .unwrap_or(range.start..range.start);
                    MdKind::TaskItem {
                        checked: t.symbol.is_some(),
                        boxed,
                    }
                }
                V::BlockQuote | V::MultilineBlockQuote(_) | V::Alert(_) => MdKind::Quote,
                V::CodeBlock(c) => MdKind::CodeBlock {
                    fenced: c.fenced,
                    language: c.info.split_whitespace().next().map(str::to_string),
                },
                V::HtmlBlock(_) => MdKind::HtmlBlock,
                V::ThematicBreak => MdKind::Rule,
                V::Table(_) => MdKind::Table,
                V::TableRow(_) => MdKind::TableRow,
                V::TableCell => MdKind::TableCell,
                V::FrontMatter(_) => MdKind::FrontMatter,
                V::FootnoteDefinition(_) => MdKind::FootnoteDefinition,
                V::Text(_) => MdKind::Text,
                V::Code(_) => MdKind::Code,
                V::Emph => MdKind::Emphasis,
                V::Strong => MdKind::Strong,
                V::Strikethrough => MdKind::Strikethrough,
                V::Link(l) => MdKind::Link { url: l.url.clone() },
                V::Image(l) => MdKind::Image { url: l.url.clone() },
                V::FootnoteReference(_) => MdKind::FootnoteRef,
                V::Math(m) => MdKind::Math {
                    display: m.display_math,
                },
                V::HtmlInline(_) => MdKind::HtmlInline,
                V::SoftBreak => MdKind::Other("softbreak"),
                V::LineBreak => MdKind::Other("linebreak"),
                _ => MdKind::Other("other"),
            };
            let kids: Vec<_> = node.children().collect();
            drop(data);
            let content = match &kind {
                // A code span's text: inside its backticks.
                MdKind::Code => {
                    let s = &text[range.clone()];
                    let n = s.bytes().take_while(|&b| b == b'`').count();
                    let m = s.bytes().rev().take_while(|&b| b == b'`').count();
                    (range.start + n).min(range.end)
                        ..range.end.saturating_sub(m).max(range.start + n)
                }
                MdKind::Math { .. } => {
                    let s = &text[range.clone()];
                    let n = s.bytes().take_while(|&b| b == b'$').count();
                    let m = s.bytes().rev().take_while(|&b| b == b'$').count();
                    (range.start + n).min(range.end)
                        ..range.end.saturating_sub(m).max(range.start + n)
                }
                _ => range.clone(),
            };
            let i = md.nodes.len() as u32;
            md.nodes.push(MdNode {
                kind,
                range,
                content,
                parent,
            });
            for c in kids.into_iter().rev() {
                stack.push((c, Some(i)));
            }
        }
        // Containers' content: from their first child to their last.
        for i in (0..md.nodes.len()).rev() {
            if let Some(p) = md.nodes[i].parent {
                let r = md.nodes[i].range.clone();
                let parent = &mut md.nodes[p as usize];
                if matches!(parent.kind, MdKind::Code | MdKind::Math { .. }) {
                    continue;
                }
                if parent.content == parent.range {
                    parent.content = r;
                } else {
                    parent.content =
                        parent.content.start.min(r.start)..parent.content.end.max(r.end);
                }
            }
        }
        // Each line's nodes.
        md.by_line = vec![Vec::new(); starts.len()];
        for (i, n) in md.nodes.iter().enumerate() {
            let a = md.line_of(n.range.start);
            let b = md.line_of(n.range.end.max(n.range.start));
            for l in a..=b.min(starts.len() - 1) {
                md.by_line[l].push(i as u32);
            }
        }
        md
    }

    /// The line holding byte `pos`.
    fn line_of(&self, pos: usize) -> usize {
        self.starts.partition_point(|&s| s <= pos).saturating_sub(1)
    }

    /// The nodes touching line `line`, outermost first.
    pub fn on_line(&self, line: usize) -> impl Iterator<Item = &MdNode> {
        self.by_line
            .get(line)
            .into_iter()
            .flatten()
            .map(|&i| &self.nodes[i as usize])
    }

    /// The headings: level, title range and start.
    pub fn headings(&self) -> impl Iterator<Item = (u8, &MdNode)> {
        self.nodes.iter().filter_map(|n| match n.kind {
            MdKind::Heading { level, .. } => Some((level, n)),
            _ => None,
        })
    }
}

/// The last parse, with the text version and length it is for.
type Memo = ((u64, usize), Rc<Md>);

thread_local! {
    static PARSED: std::cell::RefCell<Option<Memo>> =
        const { std::cell::RefCell::new(None) };
}

/// The parse of `doc`, for its text version.
pub fn parsed(doc: &crate::DocumentState) -> Rc<Md> {
    let key = (doc.version(), doc.text().len());
    PARSED.with(|p| {
        if let Some((k, md)) = &*p.borrow()
            && *k == key
        {
            return md.clone();
        }
        let md = Rc::new(Md::parse(doc.text().as_str()));
        *p.borrow_mut() = Some((key, md.clone()));
        md
    })
}

/// The largest document drawn as it reads: a keystroke reparses the
/// whole text until the reparse is incremental (T2.7c.6), and comrak
/// parses about 8 MB a second; larger documents show their source.
pub const LIVE_LIMIT: usize = 256 * 1024;

/// Line `range` of the Markdown document `doc` as displayed, the cursor at
/// `cursor`.
pub fn line_view(
    doc: &crate::DocumentState,
    range: Range<usize>,
    cursor: Option<usize>,
) -> LineView {
    if doc.text().len() > LIVE_LIMIT {
        return crate::view::plain_line_view(doc.text().as_str(), range, cursor);
    }
    let md = parsed(doc);
    view_line(&md, doc.text().as_str(), range, cursor)
}

/// A piece of the line: shown with a style, hidden, or replaced.
#[derive(Clone)]
enum Piece {
    Style(Range<usize>, fn(&mut Style)),
    Hide(Range<usize>),
    Replace(Range<usize>, String, Option<Widget>, Style),
}

/// Line `line` of `text` as displayed (see [`line_view`]).
pub fn view_line(md: &Md, text: &str, line: Range<usize>, cursor: Option<usize>) -> LineView {
    let markers = crate::view::source_markers();
    let revealed = |r: &Range<usize>| match markers {
        crate::view::Markers::Always => true,
        crate::view::Markers::Never => false,
        crate::view::Markers::Cursor => cursor.is_some_and(|c| r.start <= c && c <= r.end),
    };
    let on_line = cursor.is_some_and(|c| line.start <= c && c <= line.end);
    let idx = md.line_of(line.start);
    let mut view = LineView {
        range: line.clone(),
        ..LineView::default()
    };
    let mut pieces: Vec<Piece> = Vec::new();
    let clip = |r: &Range<usize>| r.start.max(line.start)..r.end.min(line.end);
    // The markers around a node's content.
    let around = |n: &MdNode, pieces: &mut Vec<Piece>| {
        pieces.push(Piece::Hide(clip(&(n.range.start..n.content.start))));
        pieces.push(Piece::Hide(clip(&(n.content.end..n.range.end))));
    };
    for n in md.on_line(idx) {
        let r = clip(&n.range);
        match &n.kind {
            MdKind::Heading { level, setext } => {
                let title = md.line_of(n.content.start) == idx || n.content == n.range;
                if title {
                    view.heading = *level;
                    pieces.push(Piece::Style(clip(&n.content), |s| s.bold = true));
                    if !*setext && !on_line && markers != crate::view::Markers::Always {
                        // `## ` before the title, closing `#`s after it.
                        around(n, &mut pieces);
                    }
                } else {
                    // The underline of `===` or `---`.
                    view.role = LineRole::Delimiter;
                    pieces.push(Piece::Style(r, |s| s.dim = true));
                }
            }
            MdKind::CodeBlock { fenced, .. } => {
                view.mono = true;
                let first = md.line_of(n.range.start) == idx;
                let last = md.line_of(n.range.end.saturating_sub(1).max(n.range.start)) == idx;
                if *fenced && (first || last) {
                    view.role = LineRole::Delimiter;
                    pieces.push(Piece::Style(r, |s| s.dim = true));
                } else {
                    pieces.push(Piece::Style(r, |s| s.code = true));
                }
            }
            MdKind::HtmlBlock | MdKind::Table | MdKind::TableRow => view.mono = true,
            MdKind::FrontMatter => {
                view.mono = true;
                pieces.push(Piece::Style(r, |s| s.dim = true));
            }
            MdKind::Rule => pieces.push(Piece::Style(r, |s| s.dim = true)),
            MdKind::Quote => {
                // The `>` of this line, dimmed.
                let s = &text[line.clone()];
                let k = s
                    .bytes()
                    .take_while(|b| matches!(b, b' ' | b'>' | b'\t'))
                    .count();
                if k > 0 {
                    pieces.push(Piece::Style(line.start..line.start + k, |s| s.dim = true));
                }
            }
            MdKind::TaskItem { checked, boxed } if !on_line && !boxed.is_empty() => {
                if md.line_of(boxed.start) == idx {
                    pieces.push(Piece::Replace(
                        clip(boxed),
                        text[clip(boxed)].to_string(),
                        Some(Widget::Checkbox(if *checked {
                            crate::view::CheckState::Checked
                        } else {
                            crate::view::CheckState::Unchecked
                        })),
                        Style::default(),
                    ));
                }
            }
            MdKind::Emphasis => {
                pieces.push(Piece::Style(clip(&n.content), |s| s.italic = true));
                if !revealed(&n.range) {
                    around(n, &mut pieces);
                }
            }
            MdKind::Strong => {
                pieces.push(Piece::Style(clip(&n.content), |s| s.bold = true));
                if !revealed(&n.range) {
                    around(n, &mut pieces);
                }
            }
            MdKind::Strikethrough => {
                pieces.push(Piece::Style(clip(&n.content), |s| s.strike = true));
                if !revealed(&n.range) {
                    around(n, &mut pieces);
                }
            }
            MdKind::Code => {
                pieces.push(Piece::Style(r.clone(), |s| s.code = true));
                if !revealed(&n.range) {
                    around(n, &mut pieces);
                }
            }
            MdKind::Link { .. } => {
                pieces.push(Piece::Style(clip(&n.content), |s| s.link = true));
                if !revealed(&n.range) {
                    around(n, &mut pieces);
                }
            }
            MdKind::FootnoteRef => pieces.push(Piece::Style(r, |s| s.footnote = true)),
            MdKind::HtmlInline => pieces.push(Piece::Style(r, |s| s.dim = true)),
            MdKind::Image { url }
                if !revealed(&n.range)
                    && n.range.start >= line.start
                    && n.range.end <= line.end =>
            {
                let alt = text[n.content.clone()].to_string();
                pieces.push(Piece::Replace(
                    n.range.clone(),
                    alt,
                    Some(Widget::Image {
                        path: url.clone(),
                        width: None,
                    }),
                    Style::default(),
                ));
            }
            MdKind::Math { display }
                if !revealed(&n.range)
                    && n.range.start >= line.start
                    && n.range.end <= line.end =>
            {
                pieces.push(Piece::Replace(
                    n.range.clone(),
                    text[n.range.clone()].to_string(),
                    Some(Widget::Math {
                        source: text[n.range.clone()].to_string(),
                        display: *display,
                    }),
                    Style::default(),
                ));
            }
            _ => {}
        }
    }
    view.runs = runs(text, line, &pieces);
    view
}

/// The runs of `line` from its pieces: every boundary cuts, hidden bytes
/// get no run, a replaced range one run.
fn runs(text: &str, line: Range<usize>, pieces: &[Piece]) -> Vec<Run> {
    let mut cuts: Vec<usize> = vec![line.start, line.end];
    for p in pieces {
        let r = match p {
            Piece::Style(r, _) | Piece::Hide(r) | Piece::Replace(r, ..) => r,
        };
        if r.start < r.end {
            cuts.push(r.start.clamp(line.start, line.end));
            cuts.push(r.end.clamp(line.start, line.end));
        }
    }
    cuts.retain(|&c| text.is_char_boundary(c));
    cuts.sort_unstable();
    cuts.dedup();
    let mut out: Vec<Run> = Vec::new();
    let mut skip_to = line.start;
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a < skip_to || a == b {
            continue;
        }
        let inside = |r: &Range<usize>| r.start <= a && b <= r.end && r.start < r.end;
        if let Some(Piece::Replace(r, shown, widget, style)) = pieces
            .iter()
            .find(|p| matches!(p, Piece::Replace(r, ..) if inside(r)))
        {
            out.push(Run {
                src: r.clone(),
                text: shown.clone(),
                verbatim: false,
                style: *style,
                widget: widget.clone(),
            });
            skip_to = r.end;
            continue;
        }
        if pieces
            .iter()
            .any(|p| matches!(p, Piece::Hide(r) if inside(r)))
        {
            continue;
        }
        let mut style = Style::default();
        for p in pieces {
            if let Piece::Style(r, f) = p
                && inside(r)
            {
                f(&mut style);
            }
        }
        match out.last_mut() {
            Some(last)
                if last.verbatim
                    && last.style == style
                    && last.src.end == a
                    && last.widget.is_none() =>
            {
                last.src.end = b;
                last.text.push_str(&text[a..b]);
            }
            _ => out.push(Run {
                src: a..b,
                text: text[a..b].to_string(),
                verbatim: true,
                style,
                widget: None,
            }),
        }
    }
    out
}

/// The code on line `line` of a Markdown document with its language: the
/// line's text inside a fenced code block that names one, for the
/// editors to color as they color code.
pub fn code_on_line(doc: &crate::DocumentState, line: Range<usize>) -> Vec<(Range<usize>, String)> {
    if doc.meta.mode != crate::DocumentMode::Markdown || doc.text().len() > LIVE_LIMIT {
        return Vec::new();
    }
    let md = parsed(doc);
    code_lines(&md, line)
}

fn code_lines(md: &Md, line: Range<usize>) -> Vec<(Range<usize>, String)> {
    let idx = md.line_of(line.start);
    md.on_line(idx)
        .find_map(|n| match &n.kind {
            MdKind::CodeBlock {
                fenced: true,
                language: Some(l),
            } => {
                let first = md.line_of(n.range.start);
                let last = md.line_of(n.range.end.saturating_sub(1).max(n.range.start));
                (idx > first && idx < last).then(|| vec![(line.clone(), l.clone())])
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// A list item's or quote's prefix on a line: its indentation, `>`
/// markers, bullet or number and checkbox, and the length it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Prefix {
    /// What the next line starts with.
    next: String,
    /// The bytes of the line the prefix covers.
    len: usize,
}

fn prefix(line: &str) -> Option<Prefix> {
    let b = line.as_bytes();
    let mut i = 0;
    let mut next = String::new();
    let mut any = false;
    // Indentation and `>` markers.
    loop {
        let start = i;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        if i < b.len() && b[i] == b'>' {
            i += 1;
            if i < b.len() && b[i] == b' ' {
                i += 1;
            }
            next.push_str(&line[start..i]);
            any = true;
            continue;
        }
        next.push_str(&line[start..i]);
        break;
    }
    // A bullet or a number.
    let rest = &line[i..];
    let bullet = rest.as_bytes().first().copied();
    if matches!(bullet, Some(b'-' | b'*' | b'+')) && rest.as_bytes().get(1) == Some(&b' ') {
        next.push_str(&rest[..2]);
        i += 2;
        any = true;
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let delim = rest.as_bytes().get(digits).copied();
        if (1..=9).contains(&digits)
            && matches!(delim, Some(b'.' | b')'))
            && rest.as_bytes().get(digits + 1) == Some(&b' ')
        {
            let n: u64 = rest[..digits].parse().ok()?;
            next.push_str(&format!("{}{} ", n + 1, delim? as char));
            i += digits + 2;
            any = true;
        }
    }
    // A checkbox: the next item gets an empty one.
    let rest = &line[i..];
    if rest.len() >= 4
        && rest.starts_with('[')
        && matches!(rest.as_bytes()[1], b' ' | b'x' | b'X')
        && rest[2..].starts_with("] ")
    {
        next.push_str("[ ] ");
        i += 4;
    }
    any.then_some(Prefix { next, len: i })
}

/// Enter in a list item or a quote (T2.7c.5): a new item or quoted line
/// with the same markers, the number one higher, an empty checkbox; on an
/// item or quoted line holding nothing but its markers, the markers go,
/// which ends the list. `None` elsewhere (a code block, a paragraph):
/// Enter is then a plain new line.
pub fn newline(md: &Md, text: &str, at: usize) -> Option<org_edit::Transaction> {
    let idx = md.line_of(at);
    if let Some(tx) = close_fence(md, text, at) {
        return Some(tx);
    }
    let in_code = md.on_line(idx).any(|n| {
        matches!(
            n.kind,
            MdKind::CodeBlock { .. } | MdKind::HtmlBlock | MdKind::Table
        )
    });
    let listed = md.on_line(idx).any(|n| {
        matches!(
            n.kind,
            MdKind::Item | MdKind::TaskItem { .. } | MdKind::Quote
        )
    });
    if in_code || !listed {
        return None;
    }
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let line = &text[start..end];
    let p = prefix(line)?;
    if at < start + p.len {
        return None;
    }
    let mut tx = org_edit::Transaction::new("New Line");
    if line[p.len..].trim().is_empty() {
        // Only markers: they go, and the list ends here.
        tx.replace(start..end, "").ok()?;
        return Some(tx.select(org_edit::Selection::caret(start)));
    }
    let insert = format!("\n{}", p.next);
    tx.replace(at..at, &insert).ok()?;
    renumber_after(md, text, end, &p.next, &mut tx);
    Some(tx.select(org_edit::Selection::caret(at + insert.len())))
}

/// The items after a new numbered item, numbered on from it: the number
/// of each following item of the same list one higher than the one before.
fn renumber_after(
    md: &Md,
    text: &str,
    line_end: usize,
    next: &str,
    tx: &mut org_edit::Transaction,
) {
    let digits = next.trim_start_matches([' ', '\t', '>']);
    let Some(mut n) = digits
        .split(['.', ')'])
        .next()
        .and_then(|d| d.parse::<u64>().ok())
    else {
        return;
    };
    // The ordered list holding the line, and its items after it.
    let Some(list) = md
        .nodes
        .iter()
        .enumerate()
        .rev()
        .find(|(_, l)| {
            matches!(l.kind, MdKind::List { ordered: true })
                && l.range.start <= line_end
                && line_end <= l.range.end
        })
        .map(|(i, _)| i as u32)
    else {
        return;
    };
    for item in md.nodes.iter().filter(|i| {
        i.parent == Some(list)
            && matches!(i.kind, MdKind::Item | MdKind::TaskItem { .. })
            && i.range.start > line_end
    }) {
        n += 1;
        let s = &text[item.range.clone()];
        let lead = s.len() - s.trim_start().len();
        let start = item.range.start + lead;
        let len = text[start..].bytes().take_while(u8::is_ascii_digit).count();
        if len == 0 {
            break;
        }
        let _ = tx.replace(start..start + len, n.to_string());
    }
}

/// Enter at the end of an opening fence (```` ``` ```` or `~~~` with a
/// language) whose block runs to the end of the text, unclosed: a blank
/// line for the code and the closing fence after it.
fn close_fence(md: &Md, text: &str, at: usize) -> Option<org_edit::Transaction> {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    if at != end {
        return None;
    }
    let line = &text[start..end];
    let indent = &line[..line.len() - line.trim_start().len()];
    let body = line.trim_start();
    let fence: String = body
        .chars()
        .take_while(|c| *c == '`' || *c == '~')
        .collect();
    if fence.len() < 3 || !(fence.bytes().all(|b| b == b'`') || fence.bytes().all(|b| b == b'~')) {
        return None;
    }
    let block = md.nodes.iter().find(|n| {
        matches!(n.kind, MdKind::CodeBlock { fenced: true, .. })
            && n.range.start >= start
            && n.range.start <= end
    })?;
    // Unclosed: no closing fence after the opening line.
    let rest = &text[end.min(block.range.end)..block.range.end];
    if rest.lines().skip(1).any(|l| {
        l.trim_start().starts_with(fence.as_str())
            && l.trim()
                .chars()
                .all(|c| c == fence.chars().next().unwrap_or('`'))
    }) {
        return None;
    }
    if block.range.end < text.trim_end().len() {
        return None;
    }
    let insert = format!("\n{indent}\n{indent}{fence}");
    let mut tx = org_edit::Transaction::new("New Line");
    tx.replace(at..at, &insert).ok()?;
    Some(tx.select(org_edit::Selection::caret(at + 1 + indent.len())))
}

/// Toggles the box of the task list item whose line holds `at`: `[ ]`
/// becomes `[x]`, `[x]` or `[X]` becomes `[ ]`; one character changes.
pub fn toggle_checkbox(md: &Md, text: &str, at: usize) -> Option<org_edit::Transaction> {
    let line = md.line_of(at);
    let boxed = md.nodes.iter().rev().find_map(|n| match &n.kind {
        MdKind::TaskItem { boxed, .. } if boxed.len() == 3 && md.line_of(boxed.start) == line => {
            Some(boxed.clone())
        }
        _ => None,
    })?;
    let mark = boxed.start + 1..boxed.start + 2;
    let new = if text[mark.clone()].trim().is_empty() {
        "x"
    } else {
        " "
    };
    let mut tx = org_edit::Transaction::new("Toggle Checkbox");
    tx.replace(mark, new).ok()?;
    Some(tx)
}

/// The headings of a Markdown document for the outline sidebar.
pub fn outline_items(doc: &crate::DocumentState) -> Vec<crate::view::OutlineItem> {
    let md = parsed(doc);
    outline(&md, doc.text().as_str())
}

fn outline(md: &Md, text: &str) -> Vec<crate::view::OutlineItem> {
    md.headings()
        .map(|(level, n)| crate::view::OutlineItem {
            level: usize::from(level),
            todo: None,
            title: text[n.content.clone()].trim().to_string(),
            start: n.range.start,
        })
        .collect()
}

/// Markdown on the mode contract (§11.11).
#[derive(Debug, Default)]
pub struct MarkdownMode;

impl ModeSpec for MarkdownMode {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn detect(&self) -> Detect {
        Detect {
            extensions: &["md", "markdown", "mdown", "mkd", "mkdn"],
            sniff: None,
        }
    }

    fn parse(&self, text: &str, _edit: Option<&TextEdit>, _previous: Option<&Tree>) -> Tree {
        to_tree(&Md::parse(text))
    }

    fn outline(&self, text: &str, _tree: &Tree) -> Vec<crate::view::OutlineItem> {
        outline(&Md::parse(text), text)
    }
}

/// The parse as the contract's tree.
pub fn to_tree(md: &Md) -> Tree {
    let mut tree = Tree::default();
    for n in &md.nodes {
        let kind = match &n.kind {
            MdKind::Heading { level, .. } => Kind::Heading(*level),
            MdKind::Paragraph => Kind::Paragraph,
            MdKind::List { ordered } => Kind::List { ordered: *ordered },
            MdKind::Item => Kind::ListItem { checkbox: None },
            MdKind::TaskItem { checked, .. } => Kind::ListItem {
                checkbox: Some(*checked),
            },
            MdKind::Quote => Kind::Quote,
            MdKind::CodeBlock { language, .. } => Kind::Code {
                language: language.clone(),
            },
            MdKind::Table => Kind::Table,
            MdKind::TableRow => Kind::TableRow,
            MdKind::TableCell => Kind::TableCell,
            MdKind::Rule => Kind::Rule,
            MdKind::Emphasis => Kind::Emphasis,
            MdKind::Strong => Kind::Strong,
            MdKind::Code => Kind::InlineCode,
            MdKind::Link { .. } => Kind::Link { target: None },
            MdKind::Image { .. } => Kind::Image { target: None },
            MdKind::Math { display: true } => Kind::MathBlock,
            MdKind::Math { display: false } => Kind::Math,
            MdKind::FootnoteRef => Kind::FootnoteRef,
            MdKind::HtmlBlock => Kind::Other("html".into()),
            MdKind::FrontMatter => Kind::Other("front-matter".into()),
            MdKind::FootnoteDefinition => Kind::Other("footnote".into()),
            MdKind::Strikethrough => Kind::Other("strikethrough".into()),
            MdKind::HtmlInline => Kind::Other("html-inline".into()),
            MdKind::Text => Kind::Other("text".into()),
            MdKind::Other(s) => Kind::Other((*s).into()),
        };
        tree.push(kind, n.range.clone(), n.parent);
    }
    tree
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(t: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut s = 0;
        for l in t.split_inclusive('\n') {
            let e = s + l.trim_end_matches(['\n', '\r']).len();
            out.push(s..e);
            s += l.len();
        }
        out
    }

    fn shown(t: &str, line: usize, cursor: Option<usize>) -> String {
        let md = Md::parse(t);
        view_line(&md, t, lines(t)[line].clone(), cursor).display()
    }

    #[test]
    fn markers_hide_away_from_the_cursor() {
        let t = "# Title\n\nSome *em* and **strong**, `code` and [a link](http://x.org).\n";
        assert_eq!(shown(t, 0, None), "Title");
        assert_eq!(shown(t, 0, Some(3)), "# Title");
        assert_eq!(shown(t, 2, None), "Some em and strong, code and a link.");
        // Inside an object, its markers show; the others stay hidden.
        let at = t.find("em*").unwrap();
        assert_eq!(
            shown(t, 2, Some(at)),
            "Some *em* and strong, code and a link."
        );
        let at = t.find("a link").unwrap();
        assert_eq!(
            shown(t, 2, Some(at)),
            "Some em and strong, code and [a link](http://x.org)."
        );
        let md = Md::parse(t);
        let v = view_line(&md, t, lines(t)[0].clone(), None);
        assert_eq!(v.heading, 1);
        let v = view_line(&md, t, lines(t)[2].clone(), None);
        let style = |w: &str| {
            v.runs
                .iter()
                .find(|r| r.text == w)
                .map(|r| r.style)
                .unwrap_or_else(|| panic!("{w}: {:?}", v.runs))
        };
        assert!(style("em").italic);
        assert!(style("strong").bold);
        assert!(style("code").code);
        assert!(style("a link").link);
        // Offsets map through the hidden markers.
        let at = t.find("strong").unwrap() + 2;
        assert_eq!(v.source_offset(v.display_offset(at)), at);
    }

    #[test]
    fn blocks() {
        let t = "Title\n=====\n\n> quoted *x*\n\n```rust\nlet x = 1;\n```\n\n- [ ] todo\n- [x] done\n\n---\n";
        let md = Md::parse(t);
        let l = lines(t);
        let v = |i: usize, c: Option<usize>| view_line(&md, t, l[i].clone(), c);
        assert_eq!(v(0, None).heading, 1);
        assert_eq!(v(1, None).role, LineRole::Delimiter);
        assert!(v(3, None).runs[0].style.dim);
        assert_eq!(v(3, None).display(), "> quoted x");
        assert_eq!(v(5, None).role, LineRole::Delimiter);
        assert!(v(6, None).mono && v(6, None).runs[0].style.code);
        let todo = v(9, None);
        assert!(matches!(
            todo.runs.iter().find_map(|r| r.widget.clone()),
            Some(Widget::Checkbox(crate::view::CheckState::Unchecked))
        ));
        assert!(matches!(
            v(10, None).runs.iter().find_map(|r| r.widget.clone()),
            Some(Widget::Checkbox(crate::view::CheckState::Checked))
        ));
        // On its line the box shows as written.
        assert_eq!(v(10, Some(l[10].start)).display(), "- [x] done");
        assert!(v(12, None).runs[0].style.dim);
    }

    #[test]
    fn code_blocks_name_their_language() {
        let t = "```rust\nlet x = 1;\n```\n\n    indented\n";
        let md = Md::parse(t);
        let l = lines(t);
        assert_eq!(
            code_lines(&md, l[1].clone()),
            [(l[1].clone(), "rust".to_string())]
        );
        assert!(code_lines(&md, l[0].clone()).is_empty());
        assert!(code_lines(&md, l[4].clone()).is_empty());
    }

    #[test]
    fn enter_continues_lists_and_quotes() {
        let run = |t: &str, at: usize| {
            let md = Md::parse(t);
            let tx = newline(&md, t, at)?;
            let mut s = t.to_string();
            for e in tx.edits.iter().rev() {
                s.replace_range(e.range.clone(), &e.insert);
            }
            Some((s, tx.selection_after.unwrap().head))
        };
        assert_eq!(run("- one\n", 5).unwrap(), ("- one\n- \n".to_string(), 8));
        assert_eq!(run("9. nine\n", 7).unwrap().0, "9. nine\n10. \n");
        assert_eq!(run("1) a\n", 4).unwrap().0, "1) a\n2) \n");
        assert_eq!(run("- [x] done\n", 10).unwrap().0, "- [x] done\n- [ ] \n");
        assert_eq!(run("  * nested\n", 10).unwrap().0, "  * nested\n  * \n");
        assert_eq!(run("> quote\n", 7).unwrap().0, "> quote\n> \n");
        assert_eq!(run("> - in quote\n", 12).unwrap().0, "> - in quote\n> - \n");
        // Splitting an item at the cursor.
        assert_eq!(run("- onetwo\n", 5).unwrap().0, "- one\n- two\n");
        // An empty item ends the list.
        assert_eq!(run("- a\n- \n", 6).unwrap(), ("- a\n\n".to_string(), 4));
        // The numbers after a new item go one up.
        assert_eq!(
            run("1. a\n2. b\n3. c\n", 4).unwrap().0,
            "1. a\n2. \n3. b\n4. c\n"
        );
        assert_eq!(run("- a\n- b\n", 3).unwrap().0, "- a\n- \n- b\n");
        // An opening fence gets its closing one.
        assert_eq!(
            run("```rust", 7).unwrap(),
            ("```rust\n\n```".to_string(), 8)
        );
        assert_eq!(
            run("Text\n\n  ~~~\n", 11).unwrap().0,
            "Text\n\n  ~~~\n  \n  ~~~\n"
        );
        assert!(run("```\nx\n```\n", 3).is_none());
        // Not in a paragraph or a code block, nor before the bullet.
        assert!(run("text\n", 4).is_none());
        assert!(run("```\n- x\n```\n", 7).is_none());
        assert!(run("- one\n", 0).is_none());
    }

    #[test]
    fn checkboxes_toggle() {
        let t = "- [ ] todo\n- [X] done\n- plain\n";
        let md = Md::parse(t);
        let run = |at: usize| {
            let tx = toggle_checkbox(&md, t, at)?;
            let mut s = t.to_string();
            for e in tx.edits.iter().rev() {
                s.replace_range(e.range.clone(), &e.insert);
            }
            Some(s)
        };
        assert_eq!(run(8).unwrap(), "- [x] todo\n- [X] done\n- plain\n");
        assert_eq!(run(12).unwrap(), "- [ ] todo\n- [ ] done\n- plain\n");
        assert!(run(25).is_none());
    }

    #[test]
    fn images_and_formulas_are_widgets() {
        let t = "See ![a cat](cat.png) and $x^2$.\n";
        let md = Md::parse(t);
        let v = view_line(&md, t, 0..t.len() - 1, None);
        let widgets: Vec<_> = v.runs.iter().filter_map(|r| r.widget.clone()).collect();
        assert!(matches!(&widgets[0], Widget::Image { path, .. } if path == "cat.png"));
        assert!(
            matches!(&widgets[1], Widget::Math { source, display: false } if source == "$x^2$")
        );
        // The cursor in the image shows its source.
        let at = t.find("cat.png").unwrap();
        assert_eq!(
            view_line(&md, t, 0..t.len() - 1, Some(at)).display(),
            "See ![a cat](cat.png) and $x^2$."
        );
    }

    #[test]
    fn non_ascii_and_crlf() {
        let t = "# İstanbul *çok* güzel\r\n\r\nŞehir `kod`\r\n";
        assert_eq!(shown(t, 0, None), "İstanbul çok güzel");
        assert_eq!(shown(t, 2, None), "Şehir kod");
    }

    #[test]
    fn outline_and_contract() {
        let t = "---\ntitle: x\n---\n# One\n\ntext\n\n## Two *b*\n\nSetext\n------\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n1. one\n2. two\n\n[^1]: note\n\nRef[^1].\n";
        let md = Md::parse(t);
        let o = outline(&md, t);
        assert_eq!(
            o.iter()
                .map(|i| (i.level, i.title.as_str()))
                .collect::<Vec<_>>(),
            [(1, "One"), (2, "Two *b*"), (2, "Setext")]
        );
        let modes = crate::modes::Modes::with_builtins();
        let mode = modes.get("markdown").expect("registered");
        for text in [t, "", "# x", "*a* **b** `c`\n> q\n"] {
            let edits = [
                (0..0, "x"),
                (text.len() / 2..text.len() / 2, "*"),
                (0..text.len().min(3), ""),
            ];
            crate::modes::check(mode, text, &edits).unwrap();
        }
    }

    #[test]
    fn every_spec_example_maps_within_the_text() {
        // Byte ranges inside the text and inside their parents for
        // pathological input.
        for t in [
            "- a\n- b\n\n<!-- -->\n\n- c\n",
            "| a | b |\n| --- | --- |\n| c |\nd\n\ne\n",
            "*a **b** c*\n",
            "[a](<b c> \"t\")\n",
            "  > > nested\n  > > quote\n",
            "\t\tcode\n",
            "<div>\n*x*\n</div>\n",
            "a  \nb\\\nc\n",
        ] {
            let md = Md::parse(t);
            for n in &md.nodes {
                assert!(
                    n.range.end <= t.len() && n.range.start <= n.range.end,
                    "{t:?} {n:?}"
                );
                if let Some(p) = n.parent {
                    let pr = &md.nodes[p as usize].range;
                    assert!(
                        pr.start <= n.range.start && n.range.end <= pr.end,
                        "{t:?} {n:?} in {pr:?}"
                    );
                }
            }
            for (i, l) in lines(t).into_iter().enumerate() {
                let v = view_line(&md, t, l.clone(), None);
                let _ = v.display();
                let _ = i;
            }
        }
    }
}
