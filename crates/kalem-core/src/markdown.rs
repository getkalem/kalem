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
        /// Fenced and closed by a fence (an unclosed one runs to the end
        /// of its container, its last line code).
        closed: bool,
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
    /// The top-level nodes, in order: each one's nodes run from it to the
    /// next one, so the nodes touching a line are found from these
    /// ([`Md::on_line`]).
    tops: Vec<u32>,
    /// For each top-level node, how far its nodes reach (a child's range
    /// can run past its parent's) and whether they are in the order of
    /// their starts.
    top_info: Vec<(usize, bool)>,
    /// For each top-level node, the furthest any of them up to it reaches.
    reach: Vec<usize>,
    /// The text has link reference definitions or footnotes, which reach
    /// across the document ([`has_globals`]): an edit parses it whole.
    globals: bool,
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

/// The links and pictures of `text` (a Markdown file at `path`) that
/// name a file of this computer that is not there: each link's bytes and
/// destination. Web addresses, `mailto:`, links within the document
/// (`#part`) and wiki links (a page not written yet is not a mistake)
/// are left alone.
pub fn missing_files(text: &str, path: &std::path::Path) -> Vec<(Range<usize>, String)> {
    let dir = path.parent().unwrap_or(std::path::Path::new(""));
    Md::parse(text)
        .nodes
        .iter()
        .filter_map(|n| {
            let url = match &n.kind {
                MdKind::Link { url } | MdKind::Image { url } => url,
                _ => return None,
            };
            let target = url.split(['#', '?']).next().unwrap_or("").trim();
            if target.is_empty() || target.contains("://") || target.starts_with("mailto:") {
                return None;
            }
            // Percent-encoded as editors write it (`My%20Note.md`, `%C3%A7`),
            // when the name as written is no file.
            let written = std::path::Path::new(target);
            let target = match crate::dired::percent_decode(target) {
                Some(d) if !written.exists() && !dir.join(written).exists() => d,
                _ => target.to_string(),
            };
            let file = if std::path::Path::new(&target).is_absolute() {
                std::path::PathBuf::from(&target)
            } else {
                dir.join(&target)
            };
            (!file.exists()).then(|| (n.range.clone(), url.clone()))
        })
        .collect()
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
        let mut md = Md::parse_with(text, true);
        md.globals = has_globals(text);
        md
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
        // The text of each text node, to place it where comrak misplaces it.
        let mut literals: Vec<Option<String>> = Vec::new();
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
                    closed: c.fenced && c.closed,
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
            let literal = match &data.value {
                V::Text(t) => Some(t.to_string()),
                _ => None,
            };
            // An inline link whose title is on the next line ends, by
            // comrak's position, at the end of the first: to its `)`.
            if matches!(kind, MdKind::Link { .. })
                && text[range.clone()].starts_with('[')
                && text[range.clone()].contains("](")
                && !text[range.clone()].ends_with(')')
                && let Some(p) = parent
                && let Some(k) = text[range.end..md.nodes[p as usize].range.end].find(')')
            {
                range.end += k + 1;
            }
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
            literals.push(literal);
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
        // A paragraph's or heading's text after link reference definitions:
        // comrak places it from the paragraph's start (`[a]: /u\nbar`, `bar`
        // at 0..3); moved to where it is.
        for p in 0..md.nodes.len() {
            if !matches!(md.nodes[p].kind, MdKind::Paragraph | MdKind::Heading { .. }) {
                continue;
            }
            let pr = md.nodes[p].range.clone();
            if !text[pr.clone()].trim_start().starts_with('[') {
                continue;
            }
            let inside = |mut i: usize| {
                while let Some(q) = md.nodes[i].parent {
                    if q as usize == p {
                        return true;
                    }
                    i = q as usize;
                }
                false
            };
            let desc: Vec<usize> = (p + 1..md.nodes.len()).take_while(|&i| inside(i)).collect();
            let Some(&t) = desc.iter().find(|&&i| literals[i].is_some()) else {
                continue;
            };
            let lit = literals[t].as_deref().unwrap_or("");
            let at = md.nodes[t].range.start;
            if lit.is_empty() || text[at..].starts_with(lit) {
                continue;
            }
            let Some(k) = text[at..pr.end].find(lit) else {
                continue;
            };
            // comrak counts the lines from the paragraph's first, the
            // definitions' lines left out: every node of it is that many
            // lines early, in the right column.
            let line_of = |o: usize| starts.partition_point(|&s| s <= o).saturating_sub(1);
            let d = line_of(at + k) - line_of(at);
            if d == 0 {
                continue;
            }
            let mv = |o: usize| {
                let l = line_of(o);
                let col = o - starts[l];
                starts.get(l + d).map_or(pr.end, |&s| (s + col).min(pr.end))
            };
            for &i in &desc {
                let n = &mut md.nodes[i];
                n.range = mv(n.range.start)..mv(n.range.end).max(mv(n.range.start));
                n.content = mv(n.content.start)..mv(n.content.end).max(mv(n.content.start));
                // A text node's length from its text (comrak's end can be
                // short there too).
                if let Some(l) = literals[i].as_deref()
                    && text[n.range.start..].starts_with(l)
                {
                    n.range.end = (n.range.start + l.len()).min(pr.end);
                    n.content = n.range.clone();
                }
            }
        }
        // A table cell's inline nodes: comrak places them as if each `\|`
        // had lost its backslash already (each one an earlier byte).
        for p in 0..md.nodes.len() {
            if md.nodes[p].kind != MdKind::TableCell {
                continue;
            }
            let pr = md.nodes[p].range.clone();
            let cell = &text[pr.clone()];
            // Where each escape would be with the backslashes before it gone.
            let escapes: Vec<usize> = cell
                .match_indices("\\|")
                .enumerate()
                .map(|(i, (k, _))| pr.start + k - i)
                .collect();
            if escapes.is_empty() {
                continue;
            }
            let real = |x: usize| x + escapes.iter().filter(|&&e| e < x).count();
            for i in p + 1..md.nodes.len() {
                let mut q = md.nodes[i].parent;
                let mut inside = false;
                while let Some(a) = q {
                    if a as usize == p {
                        inside = true;
                        break;
                    }
                    q = md.nodes[a as usize].parent;
                }
                if !inside {
                    break;
                }
                let n = &mut md.nodes[i];
                n.range = real(n.range.start).min(pr.end)..real(n.range.end).min(pr.end);
                n.content = real(n.content.start).min(pr.end)..real(n.content.end).min(pr.end);
                // A code span's text: inside its backticks, read again.
                if n.kind == MdKind::Code {
                    let s = &text[n.range.clone()];
                    let a = s.bytes().take_while(|&b| b == b'`').count();
                    let b = s.bytes().rev().take_while(|&b| b == b'`').count();
                    n.content = (n.range.start + a).min(n.range.end)
                        ..n.range.end.saturating_sub(b).max(n.range.start + a);
                }
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
    /// The top-level nodes, from the parents.
    fn index(&mut self) {
        self.tops = (0..self.nodes.len() as u32)
            .filter(|&i| self.nodes[i as usize].parent.is_none())
            .collect();
        self.top_info = (0..self.tops.len()).map(|j| self.info_of(j)).collect();
        self.fill_reach();
    }

    /// [`Md::top_info`] of the `j`th top-level node.
    fn info_of(&self, j: usize) -> (usize, bool) {
        let a = self.tops[j] as usize;
        let b = self
            .tops
            .get(j + 1)
            .map_or(self.nodes.len(), |&u| u as usize);
        let nodes = &self.nodes[a..b];
        let far = nodes
            .iter()
            .map(|n| n.range.end.max(n.range.start))
            .max()
            .unwrap_or(0);
        let sorted = nodes
            .windows(2)
            .all(|w| w[0].range.start <= w[1].range.start);
        (far, sorted)
    }

    /// [`Md::reach`] from [`Md::top_info`].
    fn fill_reach(&mut self) {
        let mut far = 0;
        self.reach = self
            .top_info
            .iter()
            .map(|&(end, _)| {
                far = far.max(end);
                far
            })
            .collect();
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
        self.clone().reparse_owned(old_text, text)
    }

    /// [`Md::reparse`], reusing this parse's memory: the nodes after the
    /// edit are moved where they lie rather than copied, so a keystroke
    /// costs the region parsed again and a pass over the nodes after it.
    pub fn reparse_owned(mut self, old_text: &str, text: &str) -> Md {
        if old_text == text {
            return self;
        }
        if self.globals {
            return Md::parse(text);
        }
        let tops: Vec<usize> = std::mem::take(&mut self.tops)
            .into_iter()
            .map(|i| i as usize)
            .collect();
        let info = std::mem::take(&mut self.top_info);
        if tops.is_empty() {
            return Md::parse(text);
        }
        // The edit, as the bytes that differ.
        let (ob, nb) = (old_text.as_bytes(), text.as_bytes());
        let mut pre = common_prefix(ob, nb);
        let max_suf = ob.len().min(nb.len()) - pre;
        let mut suf = common_suffix(ob, nb, max_suf);
        while pre > 0 && !(old_text.is_char_boundary(pre) && text.is_char_boundary(pre)) {
            pre -= 1;
        }
        while suf > 0
            && !(old_text.is_char_boundary(ob.len() - suf) && text.is_char_boundary(nb.len() - suf))
        {
            suf -= 1;
        }
        let (old_end, delta) = (ob.len() - suf, nb.len() as isize - ob.len() as isize);
        // A reference definition or footnote the edit makes: on the lines
        // it changes.
        {
            let a = text[..pre].rfind('\n').map_or(0, |i| i + 1);
            let b = text[nb.len() - suf..]
                .find('\n')
                .map_or(nb.len(), |i| nb.len() - suf + i);
            if has_globals(&text[a..b]) {
                return Md::parse(text);
            }
        }
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
        // The region's nodes in place of the old ones, the ones after it
        // moved.
        let a = tops[k0];
        let b = if k1 + 1 < tops.len() {
            tops[k1 + 1]
        } else {
            self.nodes.len()
        };
        let base = a as u32;
        let count = region.nodes.len();
        // The guard block cut off the region is not among them.
        let region_tops: Vec<u32> = region
            .tops
            .iter()
            .filter(|&&i| (i as usize) < count)
            .map(|&i| i + base)
            .collect();
        self.nodes.splice(
            a..b,
            region
                .nodes
                .drain(..)
                .map(|n| shifted(n, start as isize, |p| p + base)),
        );
        let moved = count as isize - (b - a) as isize;
        for n in &mut self.nodes[a + count..] {
            shift(n, delta, moved);
        }
        let mut new_tops: Vec<u32> = tops[..k0].iter().map(|&i| i as u32).collect();
        let region_count = region_tops.len();
        new_tops.extend(region_tops);
        new_tops.extend(tops[k1 + 1..].iter().map(|&i| (i as isize + moved) as u32));
        self.tops = new_tops;
        let mut new_info: Vec<(usize, bool)> = info[..k0].to_vec();
        for j in k0..k0 + region_count {
            new_info.push(self.info_of(j));
        }
        new_info.extend(
            info[k1 + 1..]
                .iter()
                .map(|&(end, sorted)| ((end as isize + delta) as usize, sorted)),
        );
        self.top_info = new_info;
        self.fill_reach();
        // The line starts: the region's found again, those after moved.
        let first = self.starts.partition_point(|&s| s < start);
        if end >= ob.len() {
            self.starts.truncate(first);
            self.starts
                .extend(line_starts(&text[start..]).into_iter().map(|s| s + start));
        } else {
            let after = self.starts.partition_point(|&s| s < end);
            let region_starts: Vec<usize> = line_starts(&text[start..new_end])
                .into_iter()
                .map(|s| s + start)
                .filter(|&s| s < new_end)
                .collect();
            let tail = first + region_starts.len();
            self.starts.splice(first..after, region_starts);
            for s in &mut self.starts[tail..] {
                *s = (*s as isize + delta) as usize;
            }
        }
        debug_assert_eq!(self.starts, line_starts(text));
        self
    }

    /// The line holding byte `pos`.
    fn line_of(&self, pos: usize) -> usize {
        self.starts.partition_point(|&s| s <= pos).saturating_sub(1)
    }

    /// The nodes touching line `line`, outermost first.
    pub fn on_line(&self, line: usize) -> impl Iterator<Item = &MdNode> {
        // A node touches the line when it starts before the next line and
        // its end is on the line or after.
        let ids: Vec<u32> = match self.starts.get(line) {
            None => Vec::new(),
            Some(&from) => {
                let to = self.starts.get(line + 1).copied().unwrap_or(usize::MAX);
                let end = |n: &MdNode| n.range.end.max(n.range.start);
                let k = self.reach.partition_point(|&r| r < from);
                let mut ids = Vec::new();
                for (j, &t) in self.tops.iter().enumerate().skip(k) {
                    if self.nodes[t as usize].range.start >= to {
                        break;
                    }
                    let last = self
                        .tops
                        .get(j + 1)
                        .map_or(self.nodes.len(), |&u| u as usize);
                    let nodes = &self.nodes[t as usize..last];
                    // In the order of their starts, those starting after
                    // the line are at the end.
                    let upto = if self.top_info[j].1 {
                        nodes.partition_point(|n| n.range.start < to)
                    } else {
                        nodes.len()
                    };
                    ids.extend(
                        (0..upto)
                            .filter(|&i| end(&nodes[i]) >= from)
                            .map(|i| t + i as u32),
                    );
                }
                ids
            }
        };
        ids.into_iter().map(|i| &self.nodes[i as usize])
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
/// The last parse: the document (serial), its version and length; the
/// parse; the text it parsed.
type Memo = ((u64, u64, usize), Rc<Md>, Rc<str>);

/// Whether `text` has what reaches across a Markdown document: a link
/// reference definition (`[label]: …` starting a line) or a footnote.
fn has_globals(text: &str) -> bool {
    text.contains("[^")
        || (text.contains("]:")
            && text
                .lines()
                .any(|l| l.trim_start().starts_with('[') && l.contains("]:")))
}

/// How many bytes `a` and `b` start with in common, compared a chunk at
/// a time.
fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    const CHUNK: usize = 64;
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + CHUNK <= n && a[i..i + CHUNK] == b[i..i + CHUNK] {
        i += CHUNK;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// How many bytes `a` and `b` end with in common, at most `max`.
fn common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
    const CHUNK: usize = 64;
    let (la, lb) = (a.len(), b.len());
    let mut i = 0;
    while i + CHUNK <= max && a[la - i - CHUNK..la - i] == b[lb - i - CHUNK..lb - i] {
        i += CHUNK;
    }
    while i < max && a[la - i - 1] == b[lb - i - 1] {
        i += 1;
    }
    i
}

/// `n` moved by `by` bytes where it lies, its parent renumbered by
/// `moved`.
fn shift(n: &mut MdNode, by: isize, moved: isize) {
    let m = |x: usize| (x as isize + by) as usize;
    n.range = m(n.range.start)..m(n.range.end);
    n.content = m(n.content.start)..m(n.content.end);
    if let MdKind::TaskItem { boxed, .. } = &mut n.kind {
        *boxed = m(boxed.start)..m(boxed.end);
    }
    if let Some(p) = &mut n.parent {
        *p = (*p as isize + moved) as u32;
    }
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
    let key = (doc.serial(), doc.version(), doc.text().len());
    PARSED.with(|p| {
        if let Some((k, md, _)) = &*p.borrow()
            && *k == key
        {
            return md.clone();
        }
        let text = doc.text().as_str();
        let last = p.borrow_mut().take();
        let md = Rc::new(match last {
            // The last parse reused when nothing else holds it.
            Some((_, old, old_text)) => {
                let old = Rc::try_unwrap(old).unwrap_or_else(|rc| (*rc).clone());
                old.reparse_owned(&old_text, text)
            }
            None => Md::parse(text),
        });
        *p.borrow_mut() = Some((key, md.clone(), Rc::from(text)));
        md
    })
}

/// The largest document parsed on the editor's thread when it opens (at
/// about 7 MB a second). A larger one is parsed in the background and
/// shows its source until then ([`ready`]); after that a keystroke
/// reparses only the blocks around it ([`Md::reparse_owned`]).
pub const LIVE_LIMIT: usize = 2 * 1024 * 1024;

/// An edit small enough to reparse within a frame: at most this many
/// bytes changed.
const SMALL_EDIT: usize = 64 * 1024;

/// Whether `new` is `old` with a small edit.
fn small_edit(old: &str, new: &str) -> bool {
    if old.len().abs_diff(new.len()) > SMALL_EDIT {
        return false;
    }
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let pre = common_prefix(a, b);
    let suf = common_suffix(a, b, a.len().min(b.len()) - pre);
    a.len() - pre - suf <= SMALL_EDIT && b.len() - pre - suf <= SMALL_EDIT
}

/// A large document's parse running in the background: its text, and the
/// parse when it is done.
type Background = (std::sync::Arc<str>, Option<Md>);

static BACKGROUND: std::sync::Mutex<Option<Background>> = std::sync::Mutex::new(None);

/// A background parse finished and has not been reported yet.
static FINISHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a background parse finished since the last call: the views
/// are to be drawn again ([`crate::DocumentState::poll`]).
pub fn background_done() -> bool {
    FINISHED.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// The parse of `doc` when it can be had within a frame: at once for a
/// document up to [`LIVE_LIMIT`], or one a small edit away from the last
/// parse; a larger document is parsed in the background first, `None`
/// until then (its source shows).
pub fn ready(doc: &crate::DocumentState) -> Option<Rc<Md>> {
    let text = doc.text().as_str();
    if text.len() <= LIVE_LIMIT {
        return Some(parsed(doc));
    }
    let key = (doc.serial(), doc.version(), text.len());
    let near = PARSED.with(|p| {
        p.borrow().as_ref().map(|(k, md, old)| {
            (*k == key)
                .then(|| md.clone())
                .ok_or_else(|| small_edit(old, text))
        })
    });
    match near {
        Some(Ok(md)) => return Some(md),
        Some(Err(true)) => return Some(parsed(doc)),
        _ => {}
    }
    let Ok(mut bg) = BACKGROUND.lock() else {
        return None;
    };
    match bg.take() {
        // Done: reparsed to the text as it is now.
        Some((then, Some(md))) if small_edit(&then, text) => {
            PARSED.with(|p| {
                *p.borrow_mut() = Some(((u64::MAX, u64::MAX, 0), Rc::new(md), Rc::from(&*then)));
            });
            drop(bg);
            Some(parsed(doc))
        }
        // Running for this text, or a text a small edit away.
        Some((then, None)) if small_edit(&then, text) => {
            *bg = Some((then, None));
            None
        }
        // Not started, or for another document: started for this one.
        _ => {
            let then: std::sync::Arc<str> = std::sync::Arc::from(text);
            *bg = Some((then.clone(), None));
            std::thread::spawn(move || {
                let md = Md::parse(&then);
                if let Ok(mut bg) = BACKGROUND.lock()
                    && let Some((t, slot)) = bg.as_mut()
                    && std::sync::Arc::ptr_eq(t, &then)
                {
                    *slot = Some(md);
                    FINISHED.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
            None
        }
    }
}

/// Line `range` of the Markdown document `doc` as displayed, the cursor at
/// `cursor`.
pub fn line_view(
    doc: &crate::DocumentState,
    range: Range<usize>,
    cursor: Option<usize>,
) -> LineView {
    let Some(md) = ready(doc) else {
        return crate::view::plain_line_view(doc.text().as_str(), range, cursor);
    };
    let text = doc.text().as_str();
    // A table away from the cursor: a grid, its columns lined up.
    if crate::view::source_markers() != crate::view::Markers::Always
        && let Some(table) = md
            .on_line(md.line_of(range.start))
            .find(|n| n.kind == MdKind::Table)
            .map(|n| n.range.clone())
        && !cursor.is_some_and(|c| table.start <= c && c <= table.end)
    {
        return table_row(&md, text, doc.version(), table, range);
    }
    view_line(&md, text, range, cursor)
}

/// The unescaped `|` of a table row, as offsets into the text.
fn row_bars(text: &str, row: Range<usize>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut prev = 0u8;
    for (k, c) in text[row.clone()].bytes().enumerate() {
        if c == b'|' && prev != b'\\' {
            out.push(row.start + k);
        }
        prev = c;
    }
    out
}

/// Whether `row` is a table's delimiter row (`| --- | :-: |`).
fn is_delimiter_row(text: &str, row: Range<usize>) -> bool {
    let t = text[row].trim();
    t.contains('-')
        && t.bytes()
            .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
}

/// The cells of a row's view: the runs between its bars, the bars
/// themselves dropped, a run cut where a bar falls inside it.
fn row_cells(v: &LineView, bars: &[usize]) -> Vec<Vec<crate::view::Run>> {
    let mut cells: Vec<Vec<crate::view::Run>> = vec![Vec::new()];
    for r in &v.runs {
        if !r.verbatim || r.src.is_empty() {
            if let Some(c) = cells.last_mut() {
                c.push(r.clone());
            }
            continue;
        }
        let mut start = r.src.start;
        for &b in bars.iter().filter(|&&b| r.src.start <= b && b < r.src.end) {
            if b > start {
                let mut piece = r.clone();
                piece.src = start..b;
                piece.text = r.text[start - r.src.start..b - r.src.start].to_string();
                if let Some(c) = cells.last_mut() {
                    c.push(piece);
                }
            }
            cells.push(Vec::new());
            start = b + 1;
        }
        if start < r.src.end {
            let mut piece = r.clone();
            piece.src = start..r.src.end;
            piece.text = r.text[start - r.src.start..].to_string();
            if let Some(c) = cells.last_mut() {
                c.push(piece);
            }
        }
    }
    cells
}

/// `cell` without the blanks around its content (source runs only).
fn trim_cell(mut cell: Vec<crate::view::Run>) -> Vec<crate::view::Run> {
    while let Some(r) = cell.first_mut() {
        if !r.verbatim || r.widget.is_some() {
            break;
        }
        let n = r.text.len() - r.text.trim_start_matches([' ', '\t']).len();
        r.text.drain(..n);
        r.src.start += n;
        if !r.text.is_empty() {
            break;
        }
        cell.remove(0);
    }
    while let Some(r) = cell.last_mut() {
        if !r.verbatim || r.widget.is_some() {
            break;
        }
        let keep = r.text.trim_end_matches([' ', '\t']).len();
        let cut = r.text.len() - keep;
        r.text.truncate(keep);
        r.src.end -= cut;
        if !r.text.is_empty() {
            break;
        }
        cell.pop();
    }
    cell
}

/// How a column is aligned, from its delimiter cell: `:--` left, `--:`
/// right, `:-:` center.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnAlign {
    Left,
    Right,
    Center,
}

/// The alignment of each cell of the table at `table`, from its
/// delimiter row, indexed as the cells of a row.
fn column_aligns(text: &str, table: &Range<usize>) -> Vec<ColumnAlign> {
    let rows = text[table.clone()].split('\n');
    let Some(delim) = rows.clone().find(|r| {
        let t = r.trim();
        t.contains('-')
            && t.bytes()
                .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
    }) else {
        return Vec::new();
    };
    delim
        .split('|')
        .map(|c| {
            let c = c.trim();
            match (c.starts_with(':'), c.ends_with(':') && c.len() > 1) {
                (true, true) => ColumnAlign::Center,
                (false, true) => ColumnAlign::Right,
                _ => ColumnAlign::Left,
            }
        })
        .collect()
}

fn cell_width(cell: &[crate::view::Run]) -> usize {
    cell.iter()
        .map(|r| unicode_width::UnicodeWidthStr::width(r.text.as_str()))
        .sum()
}

/// A table by the text's version, the text's address and the table's
/// bytes.
type TableKey = (u64, usize, Range<usize>);

thread_local! {
    /// The column widths of the tables drawn last.
    static WIDTHS: std::cell::RefCell<Vec<(TableKey, std::rc::Rc<Vec<usize>>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The width of each column of the table at `table`: its widest cell.
fn column_widths(
    md: &Md,
    text: &str,
    version: u64,
    table: &Range<usize>,
) -> std::rc::Rc<Vec<usize>> {
    let key = (version, text.as_ptr() as usize, table.clone());
    if let Some(w) = WIDTHS.with(|c| {
        c.borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, w)| w.clone())
    }) {
        return w;
    }
    let mut widths: Vec<usize> = Vec::new();
    let mut at = table.start;
    while at < table.end {
        let end = text[at..table.end].find('\n').map_or(table.end, |i| at + i);
        let row = at..end;
        if !is_delimiter_row(text, row.clone()) {
            let v = view_line(md, text, row.clone(), None);
            let bars = row_bars(text, row.clone());
            for (i, c) in row_cells(&v, &bars).into_iter().enumerate() {
                if widths.len() <= i {
                    widths.push(0);
                }
                widths[i] = widths[i].max(cell_width(&trim_cell(c)));
            }
        }
        at = end + 1;
    }
    let w = std::rc::Rc::new(widths);
    WIDTHS.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|(k, _)| k.0 == version && k.1 == key.1);
        c.truncate(16);
        c.push((key, w.clone()));
    });
    w
}

/// Row `row` of the table at `table`, away from the cursor: its cells
/// padded to their column's width, its bars drawn as lines, its delimiter
/// row as a rule. The text is not changed.
fn table_row(
    md: &Md,
    text: &str,
    version: u64,
    table: Range<usize>,
    row: Range<usize>,
) -> LineView {
    use crate::view::{Run, Style};
    let widths = column_widths(md, text, version, &table);
    let bars = row_bars(text, row.clone());
    let dim = Style {
        dim: true,
        ..Style::default()
    };
    let mut out = LineView {
        range: row.clone(),
        mono: true,
        ..LineView::default()
    };
    if is_delimiter_row(text, row.clone()) {
        // `| --- | :-: |` as `├─────┼─────┤`, the first and last bar
        // where the row has them.
        let lead = text[row.clone()].trim_start().starts_with('|');
        let trail = text[row.clone()].trim_end().ends_with('|');
        let mut s = String::new();
        if lead {
            s.push('├');
        }
        // The widths are by cell, the space before the first bar and
        // after the last counted as cells.
        let from = usize::from(lead).min(widths.len());
        let to = widths.len().saturating_sub(usize::from(trail)).max(from);
        let columns = &widths[from..to];
        for (i, w) in columns.iter().enumerate() {
            s.push_str(&"─".repeat(w + 2));
            if i + 1 < columns.len() {
                s.push('┼');
            }
        }
        if trail {
            s.push('┤');
        }
        out.runs.push(Run {
            src: row,
            text: s,
            verbatim: false,
            style: dim,
            widget: None,
        });
        return out;
    }
    let v = view_line(md, text, row.clone(), None);
    let lead = text[row.clone()].trim_start().starts_with('|');
    let aligns = column_aligns(text, &table);
    let cells = row_cells(&v, &bars);
    let last = cells.len().saturating_sub(1);
    let blank = |at: usize, n: usize| Run {
        src: at..at,
        text: " ".repeat(n),
        verbatim: false,
        style: Style::default(),
        widget: None,
    };
    for (i, cell) in cells.into_iter().enumerate() {
        // The space before the first bar and after the last is not a
        // column's: shown as it is.
        let edge = (lead && i == 0) || (i == last && i > 0);
        if edge {
            out.runs.extend(cell);
        } else {
            // Right after the bar before the cell (or the row's start).
            let start = i
                .checked_sub(1)
                .and_then(|j| bars.get(j))
                .map_or(row.start, |b| b + 1);
            let end = bars.get(i).copied().unwrap_or(row.end);
            let cell = trim_cell(cell);
            let pad = widths
                .get(i)
                .map_or(0, |w| w.saturating_sub(cell_width(&cell)));
            let (before, after) = match aligns.get(i).copied().unwrap_or(ColumnAlign::Left) {
                ColumnAlign::Left => (0, pad),
                ColumnAlign::Right => (pad, 0),
                ColumnAlign::Center => (pad / 2, pad - pad / 2),
            };
            let from = cell.first().map_or(start, |r| r.src.start);
            let to = cell.last().map_or(end, |r| r.src.end);
            out.runs.push(blank(from, 1 + before));
            out.runs.extend(cell);
            out.runs.push(blank(to, after + 1));
        }
        if let Some(&b) = bars.get(i) {
            out.runs.push(Run {
                src: b..b + 1,
                text: "│".into(),
                verbatim: false,
                style: dim,
                widget: None,
            });
        }
    }
    out
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
                // A setext heading's title lines, all before its underline.
                let title = !*setext || n.content == n.range || line.start < n.content.end;
                if title {
                    view.heading = *level;
                    pieces.push(Piece::Style(clip(&n.content), |s| s.bold = true));
                    if !*setext && n.content == n.range {
                        // Empty (`##`): dimmed.
                        pieces.push(Piece::Style(r.clone(), |s| s.dim = true));
                    } else if !*setext && !on_line && markers != crate::view::Markers::Always {
                        // `## ` before the title, closing `#`s after it.
                        around(n, &mut pieces);
                    }
                } else {
                    // The underline of `===` or `---`.
                    view.role = LineRole::Delimiter;
                    pieces.push(Piece::Style(r, |s| s.dim = true));
                }
            }
            MdKind::CodeBlock {
                fenced,
                closed,
                language,
            } => {
                view.mono = true;
                let first = md.line_of(n.range.start) == idx;
                let last =
                    *closed && md.line_of(n.range.end.saturating_sub(1).max(n.range.start)) == idx;
                if *fenced && (first || last) {
                    view.role = LineRole::Delimiter;
                    let away = !revealed(&n.range) && !on_line;
                    if away && first && !r.is_empty() && language.is_some() {
                        // ```` ```bash ```` away from the cursor: the
                        // language only, as a label (nothing without one).
                        let label = language.clone().unwrap_or_default();
                        pieces.push(Piece::Replace(
                            r,
                            label,
                            None,
                            Style {
                                dim: true,
                                ..Style::default()
                            },
                        ));
                    } else if away {
                        // The closing fence: an empty line of the block.
                        pieces.push(Piece::Hide(r));
                    } else {
                        pieces.push(Piece::Style(r, |s| s.dim = true));
                    }
                } else {
                    pieces.push(Piece::Style(r, |s| s.code = true));
                }
            }
            MdKind::HtmlBlock => {
                view.mono = true;
                // The tags and comments, which a browser does not show.
                for t in html_tags(&text[n.range.clone()]) {
                    let t = clip(&(n.range.start + t.start..n.range.start + t.end));
                    if t.start < t.end {
                        pieces.push(Piece::Style(t, |s| s.dim = true));
                    }
                }
            }
            MdKind::Table => {
                view.mono = true;
                // The row under the header (`| --- | :-: |`): markup.
                if !md.on_line(idx).any(|m| m.kind == MdKind::TableRow) {
                    pieces.push(Piece::Style(r, |s| s.dim = true));
                }
            }
            MdKind::TableRow => {
                view.mono = true;
                // The bars between the cells, dimmed: markup as Org's are
                // (GFM's escaped `\|`, in code too, is a `|` of the cell).
                // The header's cells: a row's cells past them are dropped.
                let columns = n
                    .parent
                    .and_then(|t| {
                        md.nodes
                            .iter()
                            .find(|m| m.kind == MdKind::TableRow && m.parent == Some(t))
                    })
                    .map(|h| {
                        let h = &text[h.range.clone()];
                        h.matches('|').count() - h.matches("\\|").count()
                    })
                    .unwrap_or(usize::MAX);
                let row = &text[r.clone()];
                let header_bars = if row.starts_with('|') {
                    columns
                } else {
                    columns.saturating_add(1)
                };
                let mut prev = 0u8;
                let mut bars = 0;
                for (k, c) in row.bytes().enumerate() {
                    if c == b'|' && prev == b'\\' {
                        pieces.push(Piece::Hide(r.start + k - 1..r.start + k));
                    } else if c == b'|' {
                        pieces.push(Piece::Style(r.start + k..r.start + k + 1, |s| s.dim = true));
                        bars += 1;
                        // Past the header's last bar: cells it does not have.
                        if bars >= header_bars && k + 1 < row.len() {
                            pieces.push(Piece::Style(r.start + k + 1..r.end, |s| s.dim = true));
                            break;
                        }
                    }
                    prev = c;
                }
            }
            MdKind::FrontMatter => {
                view.mono = true;
                pieces.push(Piece::Style(r, |s| s.dim = true));
            }
            MdKind::Rule => pieces.push(Piece::Style(r, |s| s.dim = true)),
            MdKind::Quote => {
                // The `>`s of this line among its containers' markers
                // (`> 1. > x`), dimmed.
                for r in quote_markers(&text[line.clone()]) {
                    pieces.push(Piece::Style(
                        line.start + r.start..line.start + r.end,
                        |s| s.dim = true,
                    ));
                }
            }
            MdKind::Text
                if !revealed(&n.range)
                    && !n.parent.is_some_and(|p| {
                        let p = &md.nodes[p as usize];
                        // An autolink's text is read as written.
                        matches!(p.kind, MdKind::Link { .. })
                            && !text[p.range.clone()].starts_with('[')
                    }) =>
            {
                let r = clip(&n.range);
                // The parser starts the text after an escape's backslash.
                let before = text[line.start..r.start]
                    .bytes()
                    .rev()
                    .take_while(|&b| b == b'\\')
                    .count();
                let escaped = before % 2 == 1
                    && text[r.start..]
                        .bytes()
                        .next()
                        .is_some_and(|b| b.is_ascii_punctuation());
                if escaped {
                    pieces.push(Piece::Hide(r.start - 1..r.start));
                }
                let from = if escaped { r.start + 1 } else { r.start };
                escapes_and_entities(text, from.min(r.end)..r.end, &mut pieces);
            }
            MdKind::Other("linebreak") if !revealed(&n.range) => {
                // The `\` of a hard line break.
                let r = clip(&n.range);
                if text[r.clone()].starts_with('\\') {
                    pieces.push(Piece::Hide(r.start..r.start + 1));
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
            MdKind::Link { .. } | MdKind::WikiLink { .. }
                if n.content == n.range && text[n.range.clone()].starts_with('[') =>
            {
                // No text (`[](url)`): shown, dimmed, as it would not be.
                pieces.push(Piece::Style(r, |s| s.dim = true));
            }
            MdKind::Link { .. } | MdKind::WikiLink { .. } => {
                pieces.push(Piece::Style(clip(&n.content), |s| s.link = true));
                if !revealed(&n.range) {
                    around(n, &mut pieces);
                }
            }
            MdKind::FootnoteRef => pieces.push(Piece::Style(r, |s| s.footnote = true)),
            MdKind::HtmlInline => {
                // Its tags, as a browser reads them (`<![CDATA[>` ends at
                // the first `>`: what follows is text).
                for t in html_tags(&text[n.range.clone()]) {
                    let t = clip(&(n.range.start + t.start..n.range.start + t.end));
                    if t.start < t.end {
                        pieces.push(Piece::Style(t, |s| s.dim = true));
                    }
                }
            }
            MdKind::Image { url }
                if !revealed(&n.range)
                    && n.range.start >= line.start
                    && n.range.end <= line.end =>
            {
                let alt = plain_text(md, text, n);
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
    let before_text = md.on_line(idx).any(|n| {
        matches!(
            n.kind,
            MdKind::Paragraph | MdKind::Heading { setext: true, .. }
        ) && n.content != n.range
            && n.content.start >= line.end
    });
    if before_text
        || !text[line.clone()].trim().is_empty()
            && !md.on_line(idx).any(|n| {
                !matches!(
                    n.kind,
                    MdKind::List { .. }
                        | MdKind::Item
                        | MdKind::TaskItem { .. }
                        | MdKind::Quote
                        | MdKind::FootnoteDefinition
                )
            })
    {
        // A line no leaf block covers, or before a paragraph's text: a
        // link reference definition, which prints nothing.
        pieces.push(Piece::Style(line.clone(), |s| s.dim = true));
    }
    view.runs = runs(text, line, &pieces);
    view
}

/// The `>` markers among the container markers at the start of `line`.
fn quote_markers(line: &str) -> Vec<Range<usize>> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        while i < b.len() && matches!(b[i], b' ' | b'\t') {
            i += 1;
        }
        if i < b.len() && b[i] == b'>' {
            out.push(i..i + 1);
            i += 1;
            continue;
        }
        // A list marker followed by a space.
        let d = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        let m = if d > 0 && d <= 9 && matches!(b.get(i + d), Some(b'.' | b')')) {
            d + 1
        } else if d == 0 && matches!(b.get(i), Some(b'-' | b'+' | b'*')) {
            1
        } else {
            break;
        };
        if matches!(b.get(i + m), Some(b' ' | b'\t')) {
            i += m;
        } else {
            break;
        }
    }
    out
}

/// The tags, comments and declarations of a block of raw HTML, as a
/// browser reads them: a tag to its `>` outside quoted attribute values,
/// a comment to `-->`, `<?…` and `<!…` (CDATA too) to the first `>`.
fn html_tags(html: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(j) = html[i..].find('<') {
        let a = i + j;
        let rest = &html[a..];
        let end = if rest.starts_with("<!-->") {
            // HTML's empty comments, `<!-->` and `<!--->`.
            Some(a + 5)
        } else if rest.starts_with("<!--->") {
            Some(a + 6)
        } else if let Some(body) = rest.strip_prefix("<!--") {
            Some(body.find("-->").map_or(html.len(), |k| a + 4 + k + 3))
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            Some(rest.find('>').map_or(html.len(), |k| a + k + 1))
        } else if rest[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
            Some(tag_end(rest).map_or(html.len(), |k| a + k))
        } else {
            None
        };
        let Some(end) = end else {
            i = a + 1;
            continue;
        };
        // The tags GFM's tag filter escapes are shown as text on GitHub.
        let name = rest[1..].trim_start_matches('/');
        let filtered = [
            "title",
            "textarea",
            "style",
            "xmp",
            "iframe",
            "noembed",
            "noframes",
            "script",
            "plaintext",
        ]
        .iter()
        .any(|t| {
            name.len() > t.len()
                && name.is_char_boundary(t.len())
                && name[..t.len()].eq_ignore_ascii_case(t)
                && !name.as_bytes()[t.len()].is_ascii_alphanumeric()
        });
        if filtered {
            i = a + 1;
            continue;
        }
        out.push(a..end);
        i = end;
    }
    out
}

/// Where the tag `tag` starts with ends: after its `>`, a `>` inside a
/// quoted attribute value not counting.
fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (k, c) in tag.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '>') => return Some(k + 1),
            _ => {}
        }
    }
    None
}

/// The text of node `n`'s content as a browser shows it: its text and
/// code without markup (an image's description).
fn plain_text(md: &Md, text: &str, n: &MdNode) -> String {
    let mut out = String::new();
    for m in &md.nodes {
        if m.range.start >= n.content.start && m.range.end <= n.content.end {
            match m.kind {
                MdKind::Text => out.push_str(&unescaped(&text[m.range.clone()])),
                MdKind::Code => out.push_str(&text[m.content.clone()]),
                _ => {}
            }
        }
    }
    if out.is_empty() && n.content != n.range {
        out = text[n.content.clone()].to_string();
    }
    out
}

/// `s` with its backslash escapes and character references read.
fn unescaped(s: &str) -> String {
    let mut pieces = Vec::new();
    escapes_and_entities(s, 0..s.len(), &mut pieces);
    let mut out = String::new();
    let mut at = 0;
    for p in &pieces {
        match p {
            Piece::Hide(r) => {
                out.push_str(&s[at..r.start]);
                at = r.end;
            }
            Piece::Replace(r, shown, ..) => {
                out.push_str(&s[at..r.start]);
                out.push_str(shown);
                at = r.end;
            }
            Piece::Style(..) => {}
        }
    }
    out.push_str(&s[at..]);
    out
}

/// In text `r` of `text`: the backslash of each escape hidden, each
/// character reference (`&copy;`, `&#35;`) shown as its character.
fn escapes_and_entities(text: &str, r: Range<usize>, pieces: &mut Vec<Piece>) {
    let s = &text[r.clone()];
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            pieces.push(Piece::Hide(r.start + i..r.start + i + 1));
            i += 2;
            continue;
        }
        if b[i] == b'&'
            && let Some((c, len)) = character_reference(&s[i..])
        {
            pieces.push(Piece::Replace(
                r.start + i..r.start + i + len,
                c,
                None,
                Style::default(),
            ));
            i += len;
            continue;
        }
        i += 1;
    }
}

/// The character of the reference `s` starts with, and its length.
fn character_reference(s: &str) -> Option<(String, usize)> {
    let end = s.find(';')?;
    let name = &s[1..end];
    if let Some(num) = name.strip_prefix('#') {
        let (digits, radix) = match num.strip_prefix(['x', 'X']) {
            Some(h) if (1..=6).contains(&h.len()) && h.bytes().all(|c| c.is_ascii_hexdigit()) => {
                (h, 16)
            }
            None if (1..=7).contains(&num.len()) && num.bytes().all(|c| c.is_ascii_digit()) => {
                (num, 10)
            }
            _ => return None,
        };
        let n = u32::from_str_radix(digits, radix).ok()?;
        let c = if n == 0 {
            '\u{FFFD}'
        } else {
            char::from_u32(n).unwrap_or('\u{FFFD}')
        };
        return Some((c.to_string(), end + 1));
    }
    if name.is_empty() || name.len() > 32 || !name.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    // The named ones as the parser reads them.
    thread_local! {
        static NAMED: std::cell::RefCell<std::collections::HashMap<String, Option<String>>> = Default::default();
    }
    let shown = NAMED.with(|m| {
        m.borrow_mut()
            .entry(name.to_string())
            .or_insert_with(|| {
                let arena = comrak::Arena::new();
                let o = comrak::Options::default();
                let src = format!("&{name};");
                let root = comrak::parse_document(&arena, &src, &o);
                let mut out = String::new();
                for d in root.descendants() {
                    if let comrak::nodes::NodeValue::Text(t) = &d.data.borrow().value {
                        out.push_str(t);
                    }
                }
                (out != src).then_some(out)
            })
            .clone()
    })?;
    Some((shown, end + 1))
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
            // The styles around it too (a character reference in bold).
            let mut style = *style;
            for p in pieces {
                if let Piece::Style(s, f) = p
                    && s.start <= r.start
                    && r.end <= s.end
                {
                    f(&mut style);
                }
            }
            out.push(Run {
                src: r.clone(),
                text: shown.clone(),
                verbatim: false,
                style,
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
    if doc.meta.mode != crate::DocumentMode::Markdown {
        return Vec::new();
    }
    let Some(md) = ready(doc) else {
        return Vec::new();
    };
    code_lines(&md, line)
}

/// Whether the line at `line` of a Markdown document is in a code block,
/// its fences included: the editors paint the block's background behind
/// the whole line.
pub fn in_code_block(doc: &crate::DocumentState, line: Range<usize>) -> bool {
    if doc.meta.mode != crate::DocumentMode::Markdown {
        return false;
    }
    let Some(md) = ready(doc) else {
        return false;
    };
    md.on_line(md.line_of(line.start))
        .any(|n| matches!(n.kind, MdKind::CodeBlock { .. }))
}

/// The fenced code block of a known language holding line `line`: the
/// bytes of its code (the lines between the fences), the line's place
/// among them, and the language, so that the line is coloured with the
/// state of the lines before it (T2.7c.3).
pub fn code_block_on_line(
    doc: &crate::DocumentState,
    line: Range<usize>,
) -> Option<(Range<usize>, usize, String)> {
    if doc.meta.mode != crate::DocumentMode::Markdown {
        return None;
    }
    let md = ready(doc)?;
    let idx = md.line_of(line.start);
    md.on_line(idx).find_map(|n| match &n.kind {
        MdKind::CodeBlock {
            fenced: true,
            language: Some(l),
            closed,
        } => {
            let first = md.line_of(n.range.start);
            let last = md.line_of(n.range.end.saturating_sub(1).max(n.range.start));
            if !(idx > first && (idx < last || (!closed && idx == last))) {
                return None;
            }
            let text = doc.text();
            let start = text.line_start(first + 1);
            let end = if *closed {
                text.line_start(last)
            } else {
                n.range.end
            };
            Some((start..end, idx - first - 1, l.clone()))
        }
        _ => None,
    })
}

fn code_lines(md: &Md, line: Range<usize>) -> Vec<(Range<usize>, String)> {
    let idx = md.line_of(line.start);
    md.on_line(idx)
        .find_map(|n| match &n.kind {
            MdKind::CodeBlock {
                fenced: true,
                language: Some(l),
                closed,
            } => {
                let first = md.line_of(n.range.start);
                let last = md.line_of(n.range.end.saturating_sub(1).max(n.range.start));
                (idx > first && (idx < last || (!closed && idx == last)))
                    .then(|| vec![(line.clone(), l.clone())])
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

/// The innermost list item holding `at`, by index.
fn item_at(md: &Md, at: usize) -> Option<usize> {
    md.nodes
        .iter()
        .enumerate()
        .rev()
        .find(|(_, n)| {
            matches!(n.kind, MdKind::Item | MdKind::TaskItem { .. })
                && n.range.start <= at
                && at <= n.range.end
        })
        .map(|(i, _)| i)
}

/// Whether `at` is in a list item.
pub fn in_item(md: &Md, at: usize) -> bool {
    item_at(md, at).is_some()
}

/// The column where a list item's text starts on its first line: after
/// its indentation, its marker (`-`, `*`, `+`, `1.`, `1)`) and the spaces
/// after it (one to four).
fn item_text_column(line: &str) -> usize {
    let b = line.as_bytes();
    let mut i = b.iter().take_while(|c| **c == b' ').count();
    if i < b.len() && b"-*+".contains(&b[i]) {
        i += 1;
    } else {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i < b.len() && b".)".contains(&b[i]) {
            i += 1;
        }
    }
    let spaces = b[i..].iter().take_while(|c| **c == b' ').count();
    i + spaces.clamp(1, 4)
}

/// Tab on a list item (`deeper`): the item, with all it holds, nested
/// under the item before it, its lines indented to that item's text;
/// Shift+Tab: out of the item it is nested in, to that item's indentation.
/// `None` for a list's first item (nothing to nest under), or a top-level
/// one going out.
pub fn indent_item(md: &Md, text: &str, at: usize, deeper: bool) -> Option<org_edit::Transaction> {
    let i = item_at(md, at)?;
    let item = &md.nodes[i];
    let is_item = |n: &MdNode| matches!(n.kind, MdKind::Item | MdKind::TaskItem { .. });
    let lines = whole_lines(text, &item.range);
    let first = &text[lines.start..];
    let current = first.bytes().take_while(|c| *c == b' ').count();
    let line_of = |r: &Range<usize>| {
        let s = text[..r.start].rfind('\n').map_or(0, |k| k + 1);
        let e = text[s..].find('\n').map_or(text.len(), |k| s + k);
        &text[s..e]
    };
    let target = if deeper {
        let prev =
            md.nodes.iter().rev().find(|n| {
                n.parent == item.parent && is_item(n) && n.range.end <= item.range.start
            })?;
        item_text_column(line_of(&prev.range))
    } else {
        let list = md.nodes.get(item.parent? as usize)?;
        let parent = md.nodes.get(list.parent? as usize).filter(|n| is_item(n))?;
        let line = line_of(&parent.range);
        line.bytes().take_while(|c| *c == b' ').count()
    };
    if target == current {
        return None;
    }
    let mut tx = org_edit::Transaction::new(if deeper { "Nest Item" } else { "Unnest Item" });
    let mut start = lines.start;
    for line in text[lines.clone()].split_inclusive('\n') {
        let blank = line.trim().is_empty();
        if !blank {
            if target > current {
                tx.edit(start..start, " ".repeat(target - current));
            } else {
                let lead = line.bytes().take_while(|c| *c == b' ').count();
                tx.edit(start..start + lead.min(current - target), "");
            }
        }
        start += line.len();
    }
    let caret = tx.map(at, org_edit::Assoc::After);
    Some(tx.select(org_edit::Selection::caret(caret)))
}

/// The whole lines of `r` in `text`: from the start of its first line to
/// after the line feed of its last.
fn whole_lines(text: &str, r: &Range<usize>) -> Range<usize> {
    let start = text[..r.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[r.end..]
        .find('\n')
        .map_or(text.len(), |i| r.end + i + 1);
    start..end
}

/// `text` with the ordered list holding `at` numbered from `from` (else
/// its first item's number) on, one more each item; `None` outside an
/// ordered list.
pub fn renumbered(text: &str, at: usize, from: Option<u64>) -> Option<String> {
    let md = Md::parse(text);
    let (list, l) = md.nodes.iter().enumerate().rev().find(|(_, l)| {
        matches!(l.kind, MdKind::List { ordered: true }) && l.range.start <= at && at <= l.range.end
    })?;
    let _ = l;
    let mut out = text.to_string();
    let mut n: Option<u64> = None;
    let mut edits = Vec::new();
    for item in md.nodes.iter().filter(|i| {
        i.parent == Some(list as u32) && matches!(i.kind, MdKind::Item | MdKind::TaskItem { .. })
    }) {
        let s = &text[item.range.clone()];
        let lead = s.len() - s.trim_start().len();
        let start = item.range.start + lead;
        let len = text[start..].bytes().take_while(u8::is_ascii_digit).count();
        let Ok(this) = text[start..start + len].parse::<u64>() else {
            continue;
        };
        let want = n.map_or(from.unwrap_or(this), |k| k + 1);
        n = Some(want);
        if want != this {
            edits.push((start..start + len, want.to_string()));
        }
    }
    for (r, s) in edits.into_iter().rev() {
        out.replace_range(r, &s);
    }
    Some(out)
}

/// Renumbers the ordered list at `at` (as Emacs's markdown-mode cleans up
/// list numbers); `None` when it is numbered already.
pub fn renumber_list(text: &str, at: usize) -> Option<org_edit::Transaction> {
    let new = renumbered(text, at, None)?;
    crate::lines::replace_differing(text, &new, "Renumber List")
}

/// The list item at `at` (with what it holds) moved above the item
/// before it (`up`) or below the one after, the list renumbered when it is
/// ordered, the cursor moving with the item.
pub fn move_item(md: &Md, text: &str, at: usize, up: bool) -> Option<org_edit::Transaction> {
    let i = item_at(md, at)?;
    let item = &md.nodes[i];
    let sibling = md
        .nodes
        .iter()
        .enumerate()
        .filter(|(j, n)| {
            *j != i
                && n.parent == item.parent
                && matches!(n.kind, MdKind::Item | MdKind::TaskItem { .. })
        })
        .filter(|(_, n)| {
            if up {
                n.range.end <= item.range.start
            } else {
                n.range.start >= item.range.end
            }
        })
        .map(|(_, n)| n)
        .reduce(|a, b| {
            if up {
                b
            } else {
                if a.range.start < b.range.start { a } else { b }
            }
        })?;
    let mine = whole_lines(text, &item.range);
    let theirs = whole_lines(text, &sibling.range);
    let (first, second) = if up {
        (theirs, mine.clone())
    } else {
        (mine.clone(), theirs)
    };
    if first.end > second.start {
        return None;
    }
    let mut a = text[first.clone()].to_string();
    let mut b = text[second.clone()].to_string();
    // The last item of a text without a final line feed.
    if !b.ends_with('\n') {
        b.push('\n');
        a = a.strip_suffix('\n').unwrap_or(&a).to_string();
    }
    let between = &text[first.end..second.start];
    let mut new = String::with_capacity(text.len() + 1);
    new.push_str(&text[..first.start]);
    new.push_str(&b);
    new.push_str(between);
    new.push_str(&a);
    new.push_str(&text[second.end..]);
    let offset = at - mine.start;
    let caret = if up {
        first.start + offset
    } else {
        first.start + b.len() + between.len() + offset
    };
    // The list starts at the number it started at.
    let first = [&text[first.clone()], &text[second.clone()]]
        .iter()
        .find_map(|s| {
            let s = s.trim_start();
            s[..s.bytes().take_while(u8::is_ascii_digit).count()]
                .parse::<u64>()
                .ok()
        });
    let new = renumbered(&new, caret, first).unwrap_or(new);
    let tx = crate::lines::replace_differing(text, &new, "Move Item")?;
    Some(tx.select(org_edit::Selection::caret(caret.min(new.len()))))
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

/// Where a link at `at` leads: a web or mail address, a file relative to
/// the document (a `#heading` after it as the search), or a wiki page
/// found in the project ([`resolve_wiki`]).
pub fn link_at(
    md: &Md,
    text: &str,
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
    // `#heading`: the heading of this document whose anchor it is.
    if path.is_empty() {
        let anchor = search.unwrap_or_default();
        return Some(match heading_for_anchor(md, text, &anchor) {
            Some(at) => LinkAction::Jump(at),
            None => LinkAction::Missing(anchor),
        });
    }
    // Percent-encoded as editors write it, when only that names a file.
    let path = if wiki {
        path
    } else {
        let dir = doc.and_then(std::path::Path::parent);
        let exists = |p: &str| dir.map_or(std::path::Path::new(p).exists(), |d| d.join(p).exists());
        match crate::dired::percent_decode(&path) {
            Some(d) if !exists(&path) && exists(&d) => d,
            _ => path,
        }
    };
    // `OTHER.md#heading`: the heading's line in that file, which the
    // editors take as the place to open it at.
    let search = match search {
        Some(anchor) if !wiki && is_markdown_path(&path) => {
            let dir = doc.and_then(std::path::Path::parent);
            let file = dir.map_or_else(|| std::path::PathBuf::from(&path), |d| d.join(&path));
            std::fs::read_to_string(&file)
                .ok()
                .and_then(|t| {
                    let other = Md::parse(&t);
                    let at = heading_for_anchor(&other, &t, &anchor)?;
                    Some((t[..at].matches('\n').count() + 1).to_string())
                })
                .or(Some(anchor))
        }
        s => s,
    };
    let path = if wiki {
        let found = resolve_wiki(doc, &path);
        // Relative to the document's folder, written with `/`; the folder
        // as written or as the project's root resolved it (`/private/var`
        // on macOS).
        let rel = doc.and_then(std::path::Path::parent).and_then(|dir| {
            found
                .strip_prefix(dir)
                .ok()
                .map(std::path::Path::to_path_buf)
                .or_else(|| {
                    let dir = dunce::canonicalize(dir).ok()?;
                    dunce::canonicalize(&found)
                        .ok()?
                        .strip_prefix(dir)
                        .ok()
                        .map(std::path::Path::to_path_buf)
                })
        });
        match rel {
            Some(p) => p
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
            None => found.to_string_lossy().into_owned(),
        }
    } else {
        path
    };
    Some(LinkAction::File { path, search })
}

/// GitHub's anchor of a heading's text (`#install-from-source`): lower
/// case, spaces as `-`, all but letters, digits, `-` and `_` left out (so
/// `` `kalem fmt` `` is `kalem-fmt`).
pub fn anchor_of(title: &str) -> String {
    title
        .trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// The start of the heading of `md` that `#anchor` names, repeated titles
/// numbered as GitHub numbers them (`intro`, `intro-1`, …).
pub fn heading_for_anchor(md: &Md, text: &str, anchor: &str) -> Option<usize> {
    let anchor = crate::dired::percent_decode(anchor).unwrap_or_else(|| anchor.to_string());
    let want = anchor.to_lowercase();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, n) in md.headings() {
        let base = anchor_of(&text[n.content.clone()]);
        let k = seen.entry(base.clone()).or_insert(0);
        let a = if *k == 0 {
            base.clone()
        } else {
            format!("{base}-{k}")
        };
        *k += 1;
        if a == want {
            return Some(n.range.start);
        }
    }
    None
}

fn is_markdown_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mkdn"
            )
        })
}

/// The Markdown files of the project of the document at `doc` (its
/// folder without a project), at most ten thousand, by path.
pub fn project_pages(doc: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    let Some(dir) = doc.and_then(std::path::Path::parent) else {
        return Vec::new();
    };
    let root = kalem_project::list::detect_root(dir).unwrap_or_else(|| dir.to_path_buf());
    // Asked again at each keystroke after `[[`: a walk kept a few seconds.
    type Pages = Option<(
        std::path::PathBuf,
        std::time::Instant,
        Vec<std::path::PathBuf>,
    )>;
    static CACHE: std::sync::Mutex<Pages> = std::sync::Mutex::new(None);
    if let Ok(c) = CACHE.lock()
        && let Some((r, at, pages)) = c.as_ref()
        && *r == root
        && at.elapsed() < std::time::Duration::from_secs(5)
    {
        return pages.clone();
    }
    // By name only (no file is read), hidden folders and build trees
    // left out, and a bound on what is looked at: without a project the
    // root is the document's folder, which may be a whole home folder.
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    let mut seen = 0usize;
    'walk: while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > 50_000 || out.len() >= 10_000 {
                break 'walk;
            }
            let name = e.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = e.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') && !matches!(name.as_ref(), "node_modules" | "target") {
                    stack.push(e.path());
                }
            } else if is_markdown_path(&name) {
                out.push(e.path());
            }
        }
    }
    out.sort();
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((root, std::time::Instant::now(), out.clone()));
    }
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

/// The blocks of a Markdown document for the views' folding (T2.7c.3):
/// the front matter as a drawer, folded to its first line while the
/// cursor is away from it as Org folds a property drawer, and the rest
/// one paragraph. None without front matter.
pub fn blocks(doc: &crate::DocumentState) -> Vec<crate::view::Block> {
    use crate::view::{Block, BlockKind};
    let Some(md) = ready(doc) else {
        return Vec::new();
    };
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

/// The headings of a Markdown document for the outline sidebar.
pub fn outline_items(doc: &crate::DocumentState) -> Vec<crate::view::OutlineItem> {
    let Some(md) = ready(doc) else {
        return Vec::new();
    };
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

    /// Tab nests an item (and what it holds) under the one before it, at
    /// that item's text; Shift+Tab takes it out; a first item stays.
    #[test]
    fn nesting_list_items() {
        let run = |text: &str, at: usize, deeper: bool| {
            let md = Md::parse(text);
            indent_item(&md, text, at, deeper).map(|tx| tx.apply(text))
        };
        let t = "- one\n- two\n  - child\n- three\n";
        let at = t.find("two").unwrap();
        assert_eq!(
            run(t, at, true).as_deref(),
            Some("- one\n  - two\n    - child\n- three\n")
        );
        let nested = "- one\n  - two\n- three\n";
        let at = nested.find("two").unwrap();
        assert_eq!(
            run(nested, at, false).as_deref(),
            Some("- one\n- two\n- three\n")
        );
        assert_eq!(run(t, 2, true), None);
        let ordered = "1. one\n2. two\n";
        let at = ordered.find("two").unwrap();
        assert_eq!(
            run(ordered, at, true).as_deref(),
            Some("1. one\n   2. two\n")
        );
    }

    /// Links to headings: `#anchor` in the document, `OTHER.md#anchor` as
    /// the heading's line in that file; percent-encoded names decoded when
    /// only they name a file.
    #[test]
    fn links_to_headings_and_encoded_names() {
        use crate::input::LinkAction;
        assert_eq!(anchor_of("Install from Source"), "install-from-source");
        assert_eq!(anchor_of("`kalem fmt`"), "kalem-fmt");
        assert_eq!(anchor_of("Kurulum ve Ayarlar"), "kurulum-ve-ayarlar");
        let text = "# Intro\n\n## Install\n\nSee [x](#install-1).\n\n## Install\n\n[z](#nothing)\n";
        let md = Md::parse(text);
        let at = text.find("[x]").unwrap() + 1;
        let second = text.rfind("## Install").unwrap();
        assert_eq!(link_at(&md, text, at, None), Some(LinkAction::Jump(second)));
        let at = text.find("[z]").unwrap() + 1;
        assert_eq!(
            link_at(&md, text, at, None),
            Some(LinkAction::Missing("nothing".into()))
        );
        let dir = std::env::temp_dir().join(format!("kalem-md-anchors-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("GUIDE.md"), "# Guide\n\n## Configuration\n").unwrap();
        std::fs::write(dir.join("My Note.md"), "# Note\n").unwrap();
        let doc = dir.join("README.md");
        let text = "[c](GUIDE.md#configuration) [n](My%20Note.md)\n";
        let md = Md::parse(text);
        assert_eq!(
            link_at(&md, text, 1, Some(&doc)),
            Some(LinkAction::File {
                path: "GUIDE.md".into(),
                search: Some("3".into())
            })
        );
        let at = text.find("[n]").unwrap() + 1;
        assert_eq!(
            link_at(&md, text, at, Some(&doc)),
            Some(LinkAction::File {
                path: "My Note.md".into(),
                search: None
            })
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn items_moved_and_lists_renumbered() {
        let t = "Steps:\n\n1. one\n2. two\n   more of two\n3. three\n\nEnd.\n";
        let md = Md::parse(t);
        let apply = |t: &str, tx: &org_edit::Transaction| {
            let mut s = t.to_string();
            for e in tx.edits.iter().rev() {
                s.replace_range(e.range.clone(), &e.insert);
            }
            (s, tx.selection_after.map_or(0, |a| a.head))
        };
        // Two up: its own lines with it, the numbers in order again.
        let at = t.find("two").unwrap();
        let (s, c) = apply(t, &move_item(&md, t, at, true).unwrap());
        assert_eq!(
            s,
            "Steps:\n\n1. two\n   more of two\n2. one\n3. three\n\nEnd.\n"
        );
        assert_eq!(&s[c..c + 3], "two");
        // Three down: nothing after it.
        assert!(move_item(&md, t, t.find("three").unwrap(), false).is_none());
        let (s, c) = apply(
            t,
            &move_item(&md, t, t.find("one").unwrap(), false).unwrap(),
        );
        assert_eq!(
            s,
            "Steps:\n\n1. two\n   more of two\n2. one\n3. three\n\nEnd.\n"
        );
        assert_eq!(&s[c..c + 3], "one");
        // A deleted item: Renumber List fixes the numbers from the first.
        let gap = "4. a\n6. b\n9. c\n";
        let (s, _) = apply(gap, &renumber_list(gap, 0).unwrap());
        assert_eq!(s, "4. a\n5. b\n6. c\n");
        assert!(renumber_list(&s, 0).is_none());
        // Bullets move too.
        let b = "- x\n- y\n";
        let (s, _) = apply(b, &move_item(&Md::parse(b), b, 5, true).unwrap());
        assert_eq!(s, "- y\n- x\n");
    }

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
        // A fence away from the cursor: the language as a label, the
        // closing fence empty; with the cursor in the block, as written.
        assert_eq!(v(5, None).display(), "rust");
        assert_eq!(v(7, None).display(), "");
        let inside = t.find("let x").unwrap();
        assert_eq!(v(5, Some(inside)).display(), "```rust");
        assert_eq!(v(7, Some(inside)).display(), "```");
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
            link_at(&md, t, at("the other"), Some(&doc)),
            Some(LinkAction::File {
                path: "deep/Other Page.md".into(),
                search: None
            })
        );
        assert_eq!(
            link_at(&md, t, at("New"), Some(&doc)),
            Some(LinkAction::File {
                path: "New.md".into(),
                search: None
            })
        );
        assert_eq!(
            link_at(&md, t, at("web"), Some(&doc)),
            Some(LinkAction::Url("https://x.org".into()))
        );
        assert_eq!(
            link_at(&md, t, at("file"), Some(&doc)),
            Some(LinkAction::File {
                path: "a.md".into(),
                search: Some("Part".into())
            })
        );
        assert!(link_at(&md, t, 0, Some(&doc)).is_none());
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
                assert_eq!(inc.tops, full.tops);
                assert_eq!(inc.starts, full.starts);
                assert_eq!(eager_on_line(&inc), eager_on_line(&full));
                let lazy: Vec<Vec<*const MdNode>> = (0..inc.starts.len())
                    .map(|l| inc.on_line(l).map(|n| n as *const MdNode).collect())
                    .collect();
                let eager: Vec<Vec<*const MdNode>> = eager_on_line(&inc)
                    .into_iter()
                    .map(|ids| {
                        ids.into_iter()
                            .map(|i| &inc.nodes[i as usize] as *const MdNode)
                            .collect()
                    })
                    .collect();
                if lazy != eager {
                    let l = (0..lazy.len()).find(|&l| lazy[l] != eager[l]).unwrap();
                    let ids = eager_on_line(&inc);
                    let missing: Vec<_> = ids[l]
                        .iter()
                        .filter(|&&i| !lazy[l].contains(&(&inc.nodes[i as usize] as *const MdNode)))
                        .map(|&i| (i, inc.nodes[i as usize].clone()))
                        .collect();
                    let tops: Vec<_> = inc
                        .tops
                        .iter()
                        .map(|&t| (t, inc.nodes[t as usize].range.clone()))
                        .collect();
                    panic!(
                        "line {l} ({:?}): missing {missing:?}; tops {tops:?}",
                        inc.starts.get(l..l + 2)
                    );
                }
                text = after;
                md = inc;
                if text.len() > 2000 {
                    text = doc.to_string();
                    md = Md::parse(&text);
                }
            }
        }
    }

    /// The nodes touching each line, as the index built them before it
    /// was found from the top-level nodes: the reference for `on_line`.
    fn eager_on_line(md: &Md) -> Vec<Vec<u32>> {
        let lines = md.starts.len();
        let mut out = vec![Vec::new(); lines];
        for (i, n) in md.nodes.iter().enumerate() {
            let a = md.line_of(n.range.start);
            let b = md.line_of(n.range.end.max(n.range.start)).min(lines - 1);
            for l in out.iter_mut().take(b + 1).skip(a) {
                l.push(i as u32);
            }
        }
        out
    }

    #[test]
    fn large_documents_are_parsed_in_the_background() {
        // Past the live limit: the source first, the parse once the
        // background thread has it, then keystrokes reparsed at once.
        let section = "## Part\n\nSome *text* here.\n\n- one\n- two\n\n";
        let text = section.repeat(LIVE_LIMIT / section.len() + 10);
        let meta = crate::Metadata {
            path: None,
            mode: crate::DocumentMode::Markdown,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: crate::encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = crate::DocumentState::new(
            text.clone(),
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        );
        assert!(ready(&d).is_none());
        let t = std::time::Instant::now();
        while !background_done() {
            assert!(t.elapsed() < std::time::Duration::from_secs(60));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let md = ready(&d).expect("parsed");
        assert_eq!(md.nodes, Md::parse(&text).nodes);
        let range = d.text().line_range(0);
        assert_eq!(line_view(&d, range.clone(), None).display(), "Part");
        d.selection = org_edit::Selection::caret(text.len() / 2);
        d.type_text("x", false, std::time::Instant::now());
        assert!(ready(&d).is_some());
    }

    /// A table away from the cursor is a grid: its cells padded to their
    /// column's width, its bars lines, its delimiter row a rule; the row
    /// does not wrap. With the cursor in it, its source.
    #[test]
    fn a_table_away_from_the_cursor_is_a_grid() {
        let text = "Intro.\n\n| Variable | Used in | Purpose |\n| --- | --- | --- |\n| `DATABASE_URL` | prod | the system database |\n| PORT | prod | listen port |\n\nAfter.\n";
        let meta = crate::Metadata {
            path: None,
            mode: crate::DocumentMode::Markdown,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let d = crate::DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        );
        let shown =
            |line: usize, cursor: Option<usize>| line_view(&d, d.text().line_range(line), cursor);
        let rows: Vec<String> = (2..6).map(|l| shown(l, Some(0)).display()).collect();
        assert_eq!(
            rows,
            [
                "│ Variable     │ Used in │ Purpose             │",
                "├──────────────┼─────────┼─────────────────────┤",
                "│ DATABASE_URL │ prod    │ the system database │",
                "│ PORT         │ prod    │ listen port         │",
            ]
        );
        assert!(shown(2, Some(0)).mono);
        // The text is the same; a click on a padded cell maps into it.
        let v = shown(5, Some(0));
        let at = v.source_offset(v.display().find("prod").unwrap());
        assert_eq!(&text[at..at + 4], "prod");
        // The cursor in the table: its source, as written.
        let inside = text.find("PORT").unwrap();
        assert_eq!(
            shown(5, Some(inside)).display(),
            "| PORT | prod | listen port |"
        );
        // `:--`, `--:` and `:-:` line a column up to the left, the right
        // and the middle.
        let text = "| Name | Qty | Mid |\n|:--|--:|:-:|\n| apple | 3 | x |\n";
        let mut d = d;
        d.meta.mode = crate::DocumentMode::Markdown;
        let all = d.text().len();
        d.apply(
            &{
                let mut tx = org_edit::Transaction::new("t");
                tx.edit(0..all, text);
                tx
            },
            org_edit::ChangeKind::Command,
            std::time::Instant::now(),
        );
        let row = line_view(&d, d.text().line_range(2), None).display();
        assert_eq!(row, "│ apple │   3 │  x  │");
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
        // The fastest of three runs each: a loaded machine (CI's macOS
        // runners) slows one run, not all.
        let fastest = |f: &dyn Fn() -> Md| {
            (0..3)
                .map(|_| {
                    let t = std::time::Instant::now();
                    let md = f();
                    (t.elapsed(), md)
                })
                .min_by_key(|(d, _)| *d)
                .unwrap()
        };
        let (full_time, full) = fastest(&|| Md::parse(&after));
        let (inc_time, inc) = fastest(&|| md.reparse(&text, &after));
        assert_eq!(inc.nodes, full.nodes);
        // Reading the document again would take as long as a full parse.
        assert!(
            inc_time * 2 < full_time,
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

    /// The HTML as GitHub renders it, raw HTML kept (its tag filter
    /// applied), with Kalem's extensions.
    fn html(md: &str) -> String {
        tag_filter(&html_with(md, options()))
    }

    /// The HTML of CommonMark's own examples: the options of [`html`]
    /// without the extensions that change what the core specification
    /// says (front matter, wiki links, bare addresses as links; GFM's
    /// test runner runs these examples so).
    fn core_html(md: &str) -> String {
        let mut o = options_with(false);
        o.extension.autolink = false;
        o.extension.wikilinks_title_after_pipe = false;
        html_with(md, o)
    }

    /// GFM's tag filter, which GitHub applies to the HTML it renders
    /// (Kalem renders no raw HTML): the tags that change how what follows
    /// is read lose their `<`.
    fn tag_filter(html: &str) -> String {
        const TAGS: [&str; 9] = [
            "title",
            "textarea",
            "style",
            "xmp",
            "iframe",
            "noembed",
            "noframes",
            "script",
            "plaintext",
        ];
        let mut out = String::new();
        let mut rest = html;
        while let Some(i) = rest.find('<') {
            out.push_str(&rest[..i]);
            let after = rest[i + 1..].strip_prefix('/').unwrap_or(&rest[i + 1..]);
            let filtered = TAGS.iter().any(|t| {
                after.len() > t.len()
                    && after.is_char_boundary(t.len())
                    && after[..t.len()].eq_ignore_ascii_case(t)
                    && matches!(
                        after.as_bytes()[t.len()],
                        b'>' | b'/' | b' ' | b'\t' | b'\n'
                    )
            });
            out.push_str(if filtered { "&lt;" } else { "<" });
            rest = &rest[i + 1..];
        }
        out.push_str(rest);
        out
    }

    fn html_with(md: &str, mut o: comrak::Options<'static>) -> String {
        let arena = comrak::Arena::new();
        o.render.r#unsafe = true;
        let root = comrak::parse_document(&arena, md, &o);
        let mut out = String::new();
        let _ = comrak::format_html(root, &o, &mut out);
        out
    }

    /// A specification's text: from the variable `var`, or the spike's
    /// download `file`.
    fn spec_text(var: &str, file: &str) -> Option<String> {
        let path = std::env::var(var).unwrap_or_else(|_| {
            format!(
                "{}/../../spikes/md-parser/data/{file}",
                env!("CARGO_MANIFEST_DIR")
            )
        });
        std::fs::read_to_string(&path).ok()
    }

    #[test]
    #[allow(clippy::print_stderr)]
    fn spec_examples() {
        // CommonMark's examples from its current specification (0.31.2,
        // which comrak follows: GFM's copy is of 0.29), GitHub's
        // extensions from GFM's.
        let (Some(cm), Some(gfm)) = (
            spec_text("KALEM_COMMONMARK_SPEC", "commonmark-spec.txt"),
            spec_text("KALEM_GFM_SPEC", "gfm-spec.txt"),
        ) else {
            eprintln!("skipped: no specifications (see spikes/md-parser/README.md)");
            return;
        };
        let core_ex = examples(&cm);
        let ext_ex: Vec<Example> = examples(&gfm).into_iter().filter(uses_extensions).collect();
        let (mut core_ok, mut ext_ok, mut kalem_ok) = (0, 0, 0);
        let mut differ = Vec::new();
        let show = std::env::var("KALEM_SHOW").is_ok();
        for (e, core) in core_ex
            .iter()
            .map(|e| (e, true))
            .chain(ext_ex.iter().map(|e| (e, false)))
        {
            let got = if core {
                core_html(&e.markdown)
            } else {
                html(&e.markdown)
            };
            let ok = normalize(&got) == normalize(&e.html);
            if core {
                core_ok += usize::from(ok);
                // With every extension Kalem reads, as the editor does.
                let full = normalize(&html(&e.markdown)) == normalize(&e.html);
                kalem_ok += usize::from(full);
                if !full && show {
                    eprintln!(
                        "--- with Kalem's extensions: {}\n{:?}\n{:?}",
                        e.section, e.markdown, e.html
                    );
                }
            } else {
                ext_ok += usize::from(ok);
            }
            if !ok {
                differ.push(e.section.clone());
                if show {
                    eprintln!(
                        "--- {}\n{:?}\n{:?}\n{:?}",
                        e.section, e.markdown, e.html, got
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
        let (core, ext) = (core_ex.len(), ext_ex.len());
        eprintln!(
            "{core_ok}/{core} CommonMark, {ext_ok}/{ext} GFM extensions, {kalem_ok}/{core} CommonMark with Kalem's extensions; differing: {differ:?}"
        );
        assert_eq!((core_ok, ext_ok), (core, ext), "differing: {differ:?}");
        // Kalem's extensions (front matter, wiki links, GFM's bare
        // addresses) change only the examples they are about.
        assert!(kalem_ok >= KNOWN_WITH_EXTENSIONS, "{kalem_ok}/{core}");
    }

    /// The text a browser shows of `html`, without white space: what the
    /// tags, comments and declarations (read as [`html_tags`] reads them)
    /// leave, an image's description in its place.
    fn shown(html: &str) -> String {
        let mut out = String::new();
        let mut at = 0;
        for r in html_tags(html) {
            out.push_str(&html[at..r.start]);
            let tag = &html[r.clone()];
            if tag.starts_with("<img")
                && let Some(a) = tag.split(" alt=\"").nth(1)
            {
                out.push_str(a.split('"').next().unwrap_or(""));
            }
            at = r.end;
        }
        out.push_str(&html[at..]);
        let out = out
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        out.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    #[allow(clippy::print_stderr)]
    fn the_view_shows_what_the_specification_shows() {
        let Some(cm) = spec_text("KALEM_COMMONMARK_SPEC", "commonmark-spec.txt") else {
            return;
        };
        // GitHub's extensions' examples too, when GFM's specification is
        // there.
        let gfm: Vec<Example> = spec_text("KALEM_GFM_SPEC", "gfm-spec.txt")
            .map(|g| examples(&g).into_iter().filter(uses_extensions).collect())
            .unwrap_or_default();
        let (mut n, mut ok) = (0, 0);
        for e in examples(&cm).into_iter().chain(gfm) {
            let md = Md::parse(&e.markdown);
            // List markers stand for the bullets and numbers HTML draws.
            let markers: Vec<Range<usize>> = md
                .nodes
                .iter()
                .filter(|n| matches!(n.kind, MdKind::Item | MdKind::TaskItem { .. }))
                .map(|n| {
                    let t = &e.markdown[n.range.clone()];
                    let k = t.bytes().take_while(u8::is_ascii_digit).count();
                    let k = if t[k..].starts_with(['-', '+', '*', '.', ')']) {
                        k + 1
                    } else {
                        k
                    };
                    n.range.start..n.range.start + k
                })
                .collect();
            let mut got = String::new();
            let mut s = 0;
            for l in e.markdown.split_inclusive('\n') {
                let line = s..s + l.trim_end_matches(['\n', '\r']).len();
                let v = view_line(&md, &e.markdown, line, None);
                if v.role == crate::view::LineRole::Content {
                    for r in &v.runs {
                        if matches!(r.widget, Some(Widget::Image { .. })) {
                            got.push_str(&r.text);
                        } else if !r.style.dim && r.widget.is_none() {
                            for (i, c) in r.text.char_indices() {
                                let at = r.src.start + i;
                                if !(r.verbatim && markers.iter().any(|m| m.contains(&at))) {
                                    got.push(c);
                                }
                            }
                        }
                    }
                }
                s += l.len();
            }
            let got: String = got.chars().filter(|c| !c.is_whitespace()).collect();
            let want = shown(&html(&e.markdown));
            n += 1;
            if got == want {
                ok += 1;
            } else if std::env::var("KALEM_SHOW").is_ok() {
                eprintln!(
                    "--- {}\n{:?}\n want {want:?}\n got  {got:?}",
                    e.section, e.markdown
                );
            }
        }
        eprintln!("view: {ok}/{n} examples show the specification's text");
        // The counts only go up (the rest: raw HTML's text a browser
        // hides, definitions over several lines, a title on its own line).
        assert!(ok >= KNOWN_VIEW, "{ok}/{n}");
    }

    /// The CommonMark examples whose view shows the specification's text.
    const KNOWN_VIEW: usize = 676;

    /// The CommonMark examples that agree with every extension of Kalem
    /// read: the others write front matter, a wiki link or a bare address.
    const KNOWN_WITH_EXTENSIONS: usize = 639;
}
