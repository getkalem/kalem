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
    /// A wiki link `[[Page]]` or `[[Page|title]]`, with its target.
    WikiLink {
        /// The page named.
        target: String,
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
    /// For each line, where its nodes start in `line_nodes` (one more
    /// entry than lines).
    line_off: Vec<u32>,
    /// The nodes touching each line, line after line.
    line_nodes: Vec<u32>,
}

fn options() -> comrak::Options<'static> {
    options_with(true)
}

/// The options, with front matter read or not (a region of a document
/// that does not start it has none).
fn options_with(front_matter: bool) -> comrak::Options<'static> {
    let mut o = comrak::Options::default();
    o.extension.table = true;
    o.extension.strikethrough = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.math_dollars = true;
    if front_matter {
        o.extension.front_matter_delimiter = Some("---".to_string());
    }
    // Obsidian's and Logseq's `[[Page]]` and `[[Page|title]]`.
    o.extension.wikilinks_title_after_pipe = true;
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
        Md::parse_with(text, true)
    }

    fn parse_with(text: &str, front_matter: bool) -> Md {
        use comrak::nodes::{ListType, NodeValue as V};
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, text, &options_with(front_matter));
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
            let mut b = (at(p.end)? + 1).min(text.len()).max(a);
            // A position on a line ending ends before it.
            while b > a && matches!(text.as_bytes()[b - 1], b'\n' | b'\r') {
                b -= 1;
            }
            // Nor on a line of blanks after the first (comrak puts the end
            // of a block there at the end of its input).
            while let Some(nl) = text[a..b].rfind('\n').map(|i| a + i)
                && text[nl + 1..b].trim().is_empty()
            {
                b = nl;
                while b > a && text.as_bytes()[b - 1] == b'\r' {
                    b -= 1;
                }
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
                V::WikiLink(w) => MdKind::WikiLink {
                    target: w.url.clone(),
                },
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
        md.index();
        md
    }

    /// Each line's nodes, from the nodes and the line starts.
    fn index(&mut self) {
        let lines = self.starts.len();
        let spans: Vec<(usize, usize)> = self
            .nodes
            .iter()
            .map(|n| {
                let a = self.line_of(n.range.start);
                let b = self.line_of(n.range.end.max(n.range.start)).min(lines - 1);
                (a, b)
            })
            .collect();
        let mut off = vec![0u32; lines + 1];
        for &(a, b) in &spans {
            for l in a..=b {
                off[l + 1] += 1;
            }
        }
        for l in 0..lines {
            off[l + 1] += off[l];
        }
        let mut fill = off.clone();
        let mut nodes = vec![0u32; off[lines] as usize];
        for (i, &(a, b)) in spans.iter().enumerate() {
            for l in a..=b {
                nodes[fill[l] as usize] = i as u32;
                fill[l] += 1;
            }
        }
        self.line_off = off;
        self.line_nodes = nodes;
    }

    /// The parse of `text` after an edit of `old_text`, whose parse this
    /// is (T2.7c.6): only the top-level blocks the edit touches are parsed
    /// again, with a block more on each side up to a blank line (a setext
    /// underline, a lazy continuation line or a table's delimiter row can
    /// change the block before), and the rest is shifted. A text with
    /// link reference definitions or footnotes, which reach across the
    /// document, and an edit whose blocks would run on past the region
    /// (an unclosed fence, two lists meeting) are parsed whole. Always
    /// the same nodes as [`Md::parse`] of `text`.
    pub fn reparse(&self, old_text: &str, text: &str) -> Md {
        if has_globals(old_text) || has_globals(text) {
            return Md::parse(text);
        }
        let tops: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].parent.is_none())
            .collect();
        if tops.is_empty() {
            return Md::parse(text);
        }
        // The edit, as the bytes that differ.
        let (ob, nb) = (old_text.as_bytes(), text.as_bytes());
        let mut pre = ob.iter().zip(nb).take_while(|(a, b)| a == b).count();
        let max_suf = ob.len().min(nb.len()) - pre;
        let mut suf = ob
            .iter()
            .rev()
            .zip(nb.iter().rev())
            .take(max_suf)
            .take_while(|(a, b)| a == b)
            .count();
        while pre > 0 && !(old_text.is_char_boundary(pre) && text.is_char_boundary(pre)) {
            pre -= 1;
        }
        while suf > 0
            && !(old_text.is_char_boundary(ob.len() - suf) && text.is_char_boundary(nb.len() - suf))
        {
            suf -= 1;
        }
        let (old_end, delta) = (ob.len() - suf, nb.len() as isize - ob.len() as isize);
        let node = |k: usize| &self.nodes[tops[k]];
        // The top-level blocks the edit touches, one more each side.
        let mut k0 = tops
            .iter()
            .position(|&i| self.nodes[i].range.end >= pre)
            .unwrap_or(tops.len() - 1)
            .saturating_sub(1);
        let mut k1 = tops
            .iter()
            .rposition(|&i| self.nodes[i].range.start <= old_end)
            .unwrap_or(0)
            .max(k0);
        k1 = (k1 + 1).min(tops.len() - 1);
        let line_start = |pos: usize| old_text[..pos].rfind('\n').map_or(0, |i| i + 1);
        let blank_between = |a: usize, b: usize| {
            // A whole line between the two with nothing on it.
            let pieces: Vec<&str> = old_text[a.min(b)..b].split('\n').collect();
            pieces.len() >= 3
                && pieces[1..pieces.len() - 1]
                    .iter()
                    .any(|l| l.trim().is_empty())
        };
        // Out to a blank line on each side.
        while k0 > 0 && !blank_between(node(k0 - 1).range.end, node(k0).range.start) {
            k0 -= 1;
        }
        while k1 + 1 < tops.len() && !blank_between(node(k1).range.end, node(k1 + 1).range.start) {
            k1 += 1;
        }
        let start = if k0 == 0 {
            0
        } else {
            line_start(node(k0).range.start)
        };
        let end = if k1 + 1 < tops.len() {
            line_start(node(k1 + 1).range.start)
        } else {
            ob.len()
        };
        if start > pre || end < old_end {
            return Md::parse(text);
        }
        let new_end = (end as isize + delta) as usize;
        // The region with the next block as a guard, so that the region's
        // last block is not at the end of the input (comrak ends some
        // blocks differently there), and the guard read as before shows
        // that the edit does not reach past the region.
        let guarded = k1 + 1 < tops.len();
        let guard_end = if !guarded {
            new_end
        } else if k1 + 2 < tops.len() {
            (line_start(node(k1 + 2).range.start) as isize + delta) as usize
        } else {
            text.len()
        };
        let mut region = Md::parse_with(&text[start..guard_end], start == 0);
        if guarded {
            let cut_at = new_end - start;
            let Some(cut) = region
                .nodes
                .iter()
                .position(|n| n.parent.is_none() && n.range.start >= cut_at)
            else {
                return Md::parse(text);
            };
            let g = &region.nodes[cut];
            let o = node(k1 + 1);
            let moved = |r: &Range<usize>| {
                (r.start as isize + delta - start as isize) as usize
                    ..(r.end as isize + delta - start as isize) as usize
            };
            if g.kind != o.kind || g.range != moved(&o.range) {
                return Md::parse(text);
            }
            region.nodes.truncate(cut);
        }
        // Blocks that would run on into what follows, or meet what comes
        // before: a full parse.
        let region_tops: Vec<&MdNode> =
            region.nodes.iter().filter(|n| n.parent.is_none()).collect();
        let open_end = region_tops.last().is_some_and(|n| {
            matches!(
                n.kind,
                MdKind::CodeBlock { fenced: true, .. } | MdKind::HtmlBlock
            ) && n.range.end >= text[start..new_end].trim_end().len()
        });
        let list = |n: Option<&MdNode>| n.is_some_and(|n| matches!(n.kind, MdKind::List { .. }));
        let meets_after = guarded && list(region_tops.last().copied()) && list(Some(node(k1 + 1)));
        let meets_before = k0 > 0 && list(region_tops.first().copied()) && list(Some(node(k0 - 1)));
        // A list takes in indented blocks after blank lines.
        let indented = |pos: usize, t: &str| {
            let ls = t[..pos].rfind('\n').map_or(0, |i| i + 1);
            t[ls..].starts_with([' ', '\t'])
        };
        let absorbs_after = guarded
            && list(region_tops.last().copied())
            && indented(node(k1 + 1).range.start, old_text);
        let absorbs_before = k0 > 0
            && list(Some(node(k0 - 1)))
            && region_tops
                .first()
                .is_some_and(|n| indented(start + n.range.start, text));
        if (open_end && guarded) || meets_after || meets_before || absorbs_after || absorbs_before {
            return Md::parse(text);
        }
        // Old nodes before the region, the region's, the old ones after.
        let a = tops[k0];
        let b = if k1 + 1 < tops.len() {
            tops[k1 + 1]
        } else {
            self.nodes.len()
        };
        let mut nodes = Vec::with_capacity(self.nodes.len() + region.nodes.len());
        nodes.extend_from_slice(&self.nodes[..a]);
        let base = a as u32;
        for n in region.nodes.drain(..) {
            nodes.push(shifted(n, start as isize, |p| p + base));
        }
        let moved = nodes.len() as isize - b as isize;
        for n in &self.nodes[b..] {
            nodes.push(shifted(n.clone(), delta, |p| (p as isize + moved) as u32));
        }
        let mut md = Md {
            nodes,
            starts: line_starts(text),
            line_off: Vec::new(),
            line_nodes: Vec::new(),
        };
        md.index();
        md
    }

    /// The line holding byte `pos`.
    fn line_of(&self, pos: usize) -> usize {
        self.starts.partition_point(|&s| s <= pos).saturating_sub(1)
    }

    /// The nodes touching line `line`, outermost first.
    pub fn on_line(&self, line: usize) -> impl Iterator<Item = &MdNode> {
        let (a, b) = match (self.line_off.get(line), self.line_off.get(line + 1)) {
            (Some(&a), Some(&b)) => (a as usize, b as usize),
            _ => (0, 0),
        };
        self.line_nodes[a..b]
            .iter()
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

/// The last parse, with the text version and length it is for, and the
/// text, for the next edit's reparse.
type Memo = ((u64, usize), Rc<Md>, Rc<str>);

/// Whether `text` has what reaches across a Markdown document: a link
/// reference definition (`[label]: …` starting a line) or a footnote.
fn has_globals(text: &str) -> bool {
    text.contains("[^")
        || (text.contains("]:")
            && text
                .lines()
                .any(|l| l.trim_start().starts_with('[') && l.contains("]:")))
}

/// `n` moved by `by` bytes, its parent renumbered by `parent`.
fn shifted(mut n: MdNode, by: isize, parent: impl Fn(u32) -> u32) -> MdNode {
    let m = |x: usize| (x as isize + by) as usize;
    n.range = m(n.range.start)..m(n.range.end);
    n.content = m(n.content.start)..m(n.content.end);
    if let MdKind::TaskItem { boxed, .. } = &mut n.kind {
        *boxed = m(boxed.start)..m(boxed.end);
    }
    n.parent = n.parent.map(parent);
    n
}

thread_local! {
    static PARSED: std::cell::RefCell<Option<Memo>> =
        const { std::cell::RefCell::new(None) };
}

/// The parse of `doc`, for its text version.
pub fn parsed(doc: &crate::DocumentState) -> Rc<Md> {
    let key = (doc.version(), doc.text().len());
    PARSED.with(|p| {
        if let Some((k, md, _)) = &*p.borrow()
            && *k == key
        {
            return md.clone();
        }
        let text = doc.text().as_str();
        let md = Rc::new(match &*p.borrow() {
            Some((_, old, old_text)) => old.reparse(old_text, text),
            None => Md::parse(text),
        });
        *p.borrow_mut() = Some((key, md.clone(), Rc::from(text)));
        md
    })
}

/// The largest document drawn as it reads. A keystroke reparses only
/// the blocks around it ([`Md::reparse`]), but opening a document parses
/// all of it on the editor's thread, at about 8 MB a second; larger
/// documents show their source until that parse runs in the background.
pub const LIVE_LIMIT: usize = 2 * 1024 * 1024;

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
            MdKind::Link { .. } | MdKind::WikiLink { .. } => {
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
/// The selection wrapped in `open` and `close` (`**` for bold), or the
/// markers taken away when they are already around it; without a
/// selection the markers with the cursor between them.
pub fn wrap(
    text: &str,
    sel: org_edit::Selection,
    open: &str,
    close: &str,
) -> org_edit::Transaction {
    let (a, z) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let mut tx = org_edit::Transaction::new("Emphasis");
    let around =
        a >= open.len() && text[..a].ends_with(open) && text[z..].starts_with(close) && a < z;
    if around {
        let _ = tx.delete(z..z + close.len());
        let _ = tx.delete(a - open.len()..a);
        let (s, e) = (a - open.len(), z - open.len());
        return tx.select(org_edit::Selection { anchor: s, head: e });
    }
    if a == z {
        let _ = tx.insert(a, format!("{open}{close}"));
    } else {
        let _ = tx.insert(z, close.to_string());
        let _ = tx.insert(a, open.to_string());
    }
    let s = a + open.len();
    tx.select(org_edit::Selection {
        anchor: s,
        head: s + (z - a),
    })
}

/// A link around the selection: `[text](|)`, the cursor where the address
/// goes, or `[|]()` without a selection; with `bare`, `<|>` around it.
pub fn insert_link(text: &str, sel: org_edit::Selection, bare: bool) -> org_edit::Transaction {
    let (a, z) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let mut tx = org_edit::Transaction::new("Insert Link");
    if bare && a == z {
        let _ = tx.insert(a, "<>".to_string());
        return tx.select(org_edit::Selection::caret(a + 1));
    }
    if bare {
        let _ = tx.insert(z, ">".to_string());
        let _ = tx.insert(a, "<".to_string());
        return tx.select(org_edit::Selection::caret(z + 1));
    }
    let _ = text;
    if a == z {
        let _ = tx.insert(a, "[]()".to_string());
        return tx.select(org_edit::Selection::caret(a + 1));
    }
    let _ = tx.insert(z, "]()".to_string());
    let _ = tx.insert(a, "[".to_string());
    let caret = z + 3;
    tx.select(org_edit::Selection::caret(caret))
}

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

/// Where a link at `at` leads: a web or mail address, a file relative to
/// the document (a `#heading` after it as the search), or a wiki page
/// found in the project ([`resolve_wiki`]).
pub fn link_at(
    md: &Md,
    at: usize,
    doc: Option<&std::path::Path>,
) -> Option<crate::input::LinkAction> {
    use crate::input::LinkAction;
    let n = md.nodes.iter().rev().find(|n| {
        matches!(
            n.kind,
            MdKind::Link { .. } | MdKind::WikiLink { .. } | MdKind::Image { .. }
        ) && n.range.start <= at
            && at <= n.range.end
    })?;
    let (url, wiki) = match &n.kind {
        MdKind::Link { url } | MdKind::Image { url } => (url.clone(), false),
        MdKind::WikiLink { target } => (target.clone(), true),
        _ => return None,
    };
    if !wiki && (url.contains("://") || url.starts_with("mailto:")) {
        return Some(LinkAction::Url(url));
    }
    let (path, search) = match url.split_once('#') {
        Some((p, h)) => (p.to_string(), Some(h.to_string())),
        None => (url, None),
    };
    if path.is_empty() {
        return Some(LinkAction::Missing(search.unwrap_or_default()));
    }
    let path = if wiki {
        let found = resolve_wiki(doc, &path);
        match doc.and_then(std::path::Path::parent) {
            Some(dir) => found
                .strip_prefix(dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| found.to_string_lossy().into_owned()),
            None => found.to_string_lossy().into_owned(),
        }
    } else {
        path
    };
    Some(LinkAction::File { path, search })
}

/// The Markdown files of the project of the document at `doc` (its
/// folder without a project), at most ten thousand, by path.
pub fn project_pages(doc: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    let Some(dir) = doc.and_then(std::path::Path::parent) else {
        return Vec::new();
    };
    let root = kalem_project::list::detect_root(dir).unwrap_or_else(|| dir.to_path_buf());
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut out = Vec::new();
    kalem_project::files::walk(&root, &[], &cancel, |p| {
        let md = p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"));
        if md && out.len() < 10_000 {
            out.push(if p.is_relative() { root.join(p) } else { p });
        }
    });
    out.sort();
    out
}

/// The file a wiki link names: `Page.md` (or the name as given when it
/// has an extension) in the document's folder, else the first of that
/// name, ignoring case, anywhere in the project, else `Page.md` beside
/// the document, to be created.
pub fn resolve_wiki(doc: Option<&std::path::Path>, target: &str) -> std::path::PathBuf {
    let name = if std::path::Path::new(target).extension().is_some() {
        target.to_string()
    } else {
        format!("{target}.md")
    };
    let dir = doc
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let here = dir.join(&name);
    if here.exists() {
        return here;
    }
    let want = name.to_lowercase();
    project_pages(doc)
        .into_iter()
        .find(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| f.to_lowercase() == want)
                || p.to_string_lossy()
                    .to_lowercase()
                    .ends_with(&format!("/{want}"))
        })
        .unwrap_or(here)
}

/// Markdown as HTML, comrak's rendering with the extensions Kalem reads
/// (raw HTML left out), for Copy as HTML and Copy as Rich Text.
pub fn to_html(text: &str) -> String {
    let arena = comrak::Arena::new();
    let o = options();
    let root = comrak::parse_document(&arena, text, &o);
    let mut out = String::new();
    let _ = comrak::format_html(root, &o, &mut out);
    out.trim().to_string()
}

/// The headings of a Markdown document for the outline sidebar.
/// The blocks of a Markdown document for the views' folding (T2.7c.3):
/// the front matter as a drawer, folded to its first line while the
/// cursor is away from it as Org folds a property drawer, and the rest
/// one paragraph. None without front matter.
pub fn blocks(doc: &crate::DocumentState) -> Vec<crate::view::Block> {
    use crate::view::{Block, BlockKind};
    let md = parsed(doc);
    if !md
        .nodes
        .iter()
        .any(|n| matches!(n.kind, MdKind::FrontMatter))
    {
        return Vec::new();
    }
    let text = doc.text().as_str();
    let Some(end) = front_matter_end(text) else {
        return Vec::new();
    };
    let mut out = vec![Block {
        kind: BlockKind::Drawer,
        range: 0..end,
        content_end: end,
        depth: 0,
        headline: None,
    }];
    if end < text.len() {
        out.push(Block {
            kind: BlockKind::Paragraph,
            range: end..text.len(),
            content_end: text.len(),
            depth: 0,
            headline: None,
        });
    }
    out
}

/// Where the front matter `---` … `---` (or `...`) at the start of `text`
/// ends: after its closing line.
fn front_matter_end(text: &str) -> Option<usize> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let skip = text.len() - body.len();
    let mut lines = body.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let mut at = skip + first.len();
    for l in lines {
        at += l.len();
        if matches!(l.trim_end(), "---" | "...") {
            return Some(at);
        }
    }
    None
}

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
            file: None,
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
            MdKind::Link { .. } | MdKind::WikiLink { .. } => Kind::Link { target: None },
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

    #[test]
    fn emphasis_and_links() {
        let apply = |t: &str, tx: &org_edit::Transaction| {
            let mut s = t.to_string();
            for e in tx.edits.iter().rev() {
                s.replace_range(e.range.clone(), &e.insert);
            }
            (s, tx.selection_after.unwrap())
        };
        let sel = |a, h| org_edit::Selection { anchor: a, head: h };
        let t = "say hello now";
        let (s, after) = apply(t, &wrap(t, sel(4, 9), "**", "**"));
        assert_eq!(s, "say **hello** now");
        assert_eq!(&s[after.anchor..after.head], "hello");
        // Again: taken away.
        let (s, _) = apply(&s, &wrap(&s, after, "**", "**"));
        assert_eq!(s, t);
        // Without a selection: the cursor between the markers.
        let (s, after) = apply(t, &wrap(t, sel(4, 4), "`", "`"));
        assert_eq!((s.as_str(), after.head), ("say ``hello now", 5));
        let (s, after) = apply(t, &insert_link(t, sel(4, 9), false));
        assert_eq!((s.as_str(), after.head), ("say [hello]() now", 12));
        let (s, after) = apply(t, &insert_link(t, sel(4, 4), false));
        assert_eq!((s.as_str(), after.head), ("say []()hello now", 5));
        let (s, _) = apply(t, &insert_link(t, sel(4, 9), true));
        assert_eq!(s, "say <hello> now");
    }

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
    fn links_lead_somewhere() {
        use crate::input::LinkAction;
        let dir = std::env::temp_dir().join(format!("kalem-md-wiki-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("notes/deep")).unwrap();
        std::fs::write(dir.join(".kalem"), "").unwrap();
        std::fs::write(dir.join("notes/deep/Other Page.md"), "x").unwrap();
        let doc = dir.join("notes/index.md");
        let t =
            "See [[Other Page|the other]], [[New]], [web](https://x.org) and [file](a.md#Part).\n";
        let md = Md::parse(t);
        let at = |w: &str| t.find(w).unwrap() + 1;
        assert_eq!(
            link_at(&md, at("the other"), Some(&doc)),
            Some(LinkAction::File {
                path: "deep/Other Page.md".into(),
                search: None
            })
        );
        assert_eq!(
            link_at(&md, at("New"), Some(&doc)),
            Some(LinkAction::File {
                path: "New.md".into(),
                search: None
            })
        );
        assert_eq!(
            link_at(&md, at("web"), Some(&doc)),
            Some(LinkAction::Url("https://x.org".into()))
        );
        assert_eq!(
            link_at(&md, at("file"), Some(&doc)),
            Some(LinkAction::File {
                path: "a.md".into(),
                search: Some("Part".into())
            })
        );
        assert!(link_at(&md, 0, Some(&doc)).is_none());
        // A wiki link shows its title, its target hidden away from it.
        let v = view_line(&md, t, 0..t.len() - 1, None);
        assert!(
            v.display().starts_with("See the other, New, web"),
            "{}",
            v.display()
        );
        assert_eq!(project_pages(Some(&doc)).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(to_html("*a* [[B]]").contains("<em>a</em>"));
    }

    /// A smaller text where replacing `len` bytes at `at` by `ins` still
    /// reparses differently from a full parse.
    fn minimize(text: &str, at: usize, len: usize, ins: &str) -> (String, usize, String, usize) {
        let fails = |t: &str, at: usize, len: usize| {
            if at + len > t.len() || !t.is_char_boundary(at) || !t.is_char_boundary(at + len) {
                return false;
            }
            let mut after = t.to_string();
            after.replace_range(at..at + len, ins);
            Md::parse(t).reparse(t, &after).nodes != Md::parse(&after).nodes
        };
        let (mut t, mut at, len) = (text.to_string(), at, len);
        let mut changed = true;
        while changed {
            changed = false;
            let mut i = 0;
            while i < t.len() {
                if i >= at && i < at + len {
                    i += 1;
                    continue;
                }
                let mut u = t.clone();
                u.remove(i);
                let a = if i < at { at - 1 } else { at };
                if fails(&u, a, len) {
                    t = u;
                    at = a;
                    changed = true;
                } else {
                    i += 1;
                }
            }
        }
        (t, at, ins.to_string(), len)
    }

    #[test]
    fn reparse_equals_parse() {
        // Edits of every kind at every place of documents of every block,
        // the incremental parse compared with a full one (T2.7c.6).
        let docs = [
            "# Title\n\nA paragraph\nwith two lines.\n\n- a\n- b\n\n  more of b\n\n1. one\n2. two\n\n> quote\n> more\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nText *em* **strong** `code`.\n\n---\n\nEnd.\n",
            "Para\n\nSetext\n\nmore\n\n- x\n\n- y\n\n* z\n",
            "a\n\n<div>\nhtml\n</div>\n\nb\n\n    code\n\nc\n",
        ];
        let edits = [
            "", "x", "\n", "\n\n", "# ", "- ", "```", "---", "|", "> ", "*", "1. ", "===", "<div>",
            "    ",
        ];
        let mut seed: u64 = std::env::var("KALEM_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(7);
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for doc in docs {
            let mut text = doc.to_string();
            let mut md = Md::parse(&text);
            let rounds: usize = std::env::var("KALEM_ROUNDS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(400);
            for _ in 0..rounds {
                let at = next(text.len() + 1);
                let len = next(4).min(text.len() - at);
                let ins = edits[next(edits.len())];
                let mut after = text.clone();
                after.replace_range(at..at + len, ins);
                let inc = md.reparse(&text, &after);
                let full = Md::parse(&after);
                if inc.nodes != full.nodes {
                    let (t, a, i, l) = minimize(&text, at, len, ins);
                    panic!("{t:?} with {l} bytes at {a} replaced by {i:?}");
                }
                assert_eq!(inc.line_nodes, full.line_nodes);
                text = after;
                md = inc;
                if text.len() > 2000 {
                    text = doc.to_string();
                    md = Md::parse(&text);
                }
            }
        }
    }

    #[test]
    fn a_keystroke_reparses_a_little() {
        // A megabyte of sections; a letter typed in the middle reparses
        // the block around it, not the document.
        let section = "## Part\n\nSome *text* with `code` and a [link](x.md).\n\n- one\n- two\n\n```rust\nlet x = 1;\n```\n\n";
        let text = section.repeat(1_000_000 / section.len());
        let md = Md::parse(&text);
        let at = text.len() / 2;
        let at = at + text[at..].find("text").unwrap();
        let mut after = text.clone();
        after.insert(at, 'x');
        let t = std::time::Instant::now();
        let full = Md::parse(&after);
        let full_time = t.elapsed();
        let t = std::time::Instant::now();
        let inc = md.reparse(&text, &after);
        let inc_time = t.elapsed();
        assert_eq!(inc.nodes, full.nodes);
        assert!(
            inc_time * 3 < full_time,
            "{inc_time:?} against {full_time:?}"
        );
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

/// Conformance with the CommonMark and GFM specifications (T2.7c.11):
/// GitHub's `spec.txt` of `cmark-gfm` (CC BY-SA 4.0, not in the
/// repository), from `KALEM_GFM_SPEC` or the spike's download
/// (`spikes/md-parser/README.md` says how). Without it the test says so
/// and passes.
#[cfg(test)]
mod spec {
    use super::*;

    /// An example of the specification.
    pub(super) struct Example {
        section: String,
        extension: Option<String>,
        markdown: String,
        html: String,
    }

    fn examples(spec: &str) -> Vec<Example> {
        let fence = "````````````````````````````````";
        let mut out = Vec::new();
        let mut section = String::new();
        let mut lines = spec.lines();
        while let Some(l) = lines.next() {
            if let Some(h) = l.strip_prefix("## ").or_else(|| l.strip_prefix("# ")) {
                section = h.trim().to_string();
            }
            let Some(rest) = l.strip_prefix(fence) else {
                continue;
            };
            let rest = rest.trim();
            let Some(ext) = rest.strip_prefix("example") else {
                continue;
            };
            let ext = ext.trim();
            let mut md = String::new();
            let mut html = String::new();
            let mut in_html = false;
            for l in lines.by_ref() {
                if l.starts_with(fence) {
                    break;
                }
                if l == "." && !in_html {
                    in_html = true;
                    continue;
                }
                let l = l.replace('→', "\t");
                if in_html {
                    html.push_str(&l);
                    html.push('\n');
                } else {
                    md.push_str(&l);
                    md.push('\n');
                }
            }
            out.push(Example {
                section: section.clone(),
                extension: (!ext.is_empty()).then(|| ext.to_string()),
                markdown: md,
                html,
            });
        }
        out
    }

    /// HTML compared as cmark's test runner does, roughly: whitespace between
    /// tags and at the ends dropped, self-closing tags and attribute order
    /// made alike.
    fn normalize(html: &str) -> String {
        // `"` and `&quot;` are the same character in text (cmark's runner
        // compares them so).
        let mut s = html.replace("\r\n", "\n").replace("&quot;", "\"");
        // Void elements with or without the slash.
        s = s.replace(" />", ">").replace("/>", ">");
        // Table alignment as an attribute or a style; an empty body.
        for a in ["left", "center", "right"] {
            s = s.replace(
                &format!("style=\"text-align: {a}\""),
                &format!("align=\"{a}\""),
            );
        }
        s = s
            .replace("<tbody></tbody>", "")
            .replace("<tbody>\n</tbody>", "");
        // Attributes of input elements in a fixed order.
        s = s.replace(
            "<input disabled=\"\" type=\"checkbox\"",
            "<input type=\"checkbox\" disabled=\"\"",
        );
        s = s.replace(
            "<input checked=\"\" disabled=\"\" type=\"checkbox\"",
            "<input type=\"checkbox\" checked=\"\" disabled=\"\"",
        );
        s = s.replace(
            "<input type=\"checkbox\" disabled=\"\" checked=\"\"",
            "<input type=\"checkbox\" checked=\"\" disabled=\"\"",
        );
        let mut out = String::new();
        let mut pending = String::new();
        for c in s.chars() {
            if c.is_whitespace() {
                pending.push(c);
                continue;
            }
            if !pending.is_empty() {
                let after_tag = out.ends_with('>');
                if after_tag && c != '<' {
                    // A line break or a space after a tag reads the same.
                    out.push(' ');
                } else if !after_tag {
                    out.push_str(&pending);
                }
                pending.clear();
            }
            out.push(c);
        }
        out.trim().to_string()
    }

    /// Whether an example is about GitHub's extensions: labelled with one,
    /// or in a section marked "(extension)" (the task lists are not
    /// labelled).
    fn uses_extensions(e: &Example) -> bool {
        e.extension.as_deref().is_some_and(|x| x != "disabled") || e.section.contains("(extension)")
    }

    /// The HTML as GitHub renders it, raw HTML kept, with Kalem's
    /// extensions.
    fn html(md: &str) -> String {
        let arena = comrak::Arena::new();
        let mut o = options();
        o.render.r#unsafe = true;
        let root = comrak::parse_document(&arena, md, &o);
        let mut out = String::new();
        let _ = comrak::format_html(root, &o, &mut out);
        out
    }

    #[test]
    #[allow(clippy::print_stderr)]
    fn spec_examples() {
        let path = std::env::var("KALEM_GFM_SPEC").unwrap_or_else(|_| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../spikes/md-parser/data/gfm-spec.txt"
            )
            .into()
        });
        let Ok(spec) = std::fs::read_to_string(&path) else {
            eprintln!("skipped: no {path} (see spikes/md-parser/README.md)");
            return;
        };
        let ex = examples(&spec);
        let (mut core, mut core_ok, mut ext, mut ext_ok) = (0, 0, 0, 0);
        let mut differ = Vec::new();
        for e in &ex {
            let ok = normalize(&html(&e.markdown)) == normalize(&e.html);
            if uses_extensions(e) {
                ext += 1;
                ext_ok += usize::from(ok);
            } else {
                core += 1;
                core_ok += usize::from(ok);
            }
            if !ok {
                differ.push(e.section.clone());
                if std::env::var("KALEM_SHOW").is_ok() && !e.section.starts_with("Emphasis") {
                    eprintln!(
                        "--- {}\n{:?}\n{:?}\n{:?}",
                        e.section,
                        e.markdown,
                        e.html,
                        html(&e.markdown)
                    );
                }
            }
            // Every node inside the text and its parent; every line drawn.
            let md = Md::parse(&e.markdown);
            for n in &md.nodes {
                assert!(n.range.end <= e.markdown.len(), "{:?}", e.markdown);
                if let Some(p) = n.parent {
                    let pr = &md.nodes[p as usize].range;
                    assert!(
                        pr.start <= n.range.start && n.range.end <= pr.end,
                        "{:?} {n:?}",
                        e.markdown
                    );
                }
            }
            let mut s = 0;
            for l in e.markdown.split_inclusive('\n') {
                let line = s..s + l.trim_end_matches(['\n', '\r']).len();
                let _ = view_line(&md, &e.markdown, line.clone(), Some(line.start));
                let _ = view_line(&md, &e.markdown, line, None);
                s += l.len();
            }
            // An edit in the middle reparsed as a whole parse reads it.
            let mid = (e.markdown.len() / 2..e.markdown.len())
                .find(|&i| e.markdown.is_char_boundary(i))
                .unwrap_or(0);
            let mut after = e.markdown.clone();
            after.insert(mid, 'x');
            assert_eq!(
                md.reparse(&e.markdown, &after).nodes,
                Md::parse(&after).nodes,
                "{:?}",
                e.markdown
            );
        }
        eprintln!("{core_ok}/{core} CommonMark, {ext_ok}/{ext} extensions; differing: {differ:?}");
        // The counts only go up (`docs`: the known differences).
        assert!(
            core_ok >= KNOWN_CORE && ext_ok >= KNOWN_EXT,
            "{core_ok}/{core}, {ext_ok}/{ext}"
        );
    }

    /// The examples that agree today.
    const KNOWN_CORE: usize = 632;
    const KNOWN_EXT: usize = 23;
}
