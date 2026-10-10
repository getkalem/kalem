//! Documents of flowing text (plugin API 0.2.7, the `flow` and
//! `annotations` interfaces): a Word document's, an e-book's, an e-mail's
//! paragraphs, which the plugin gives and Kalem lays out itself.
//!
//! A flow unit is shown as a document of the editor ([`DocumentMode::Flow`]):
//! its text is the paragraphs' edit texts, a paragraph a line, so that the
//! cursor, selection, search, Vim's motions and both frontends' drawing
//! work on it unchanged; [`line_view`] gives each line the look the plugin
//! gave its runs (marks, sizes, colors, list labels, tracked changes and
//! comments). A row of a table whose cells hold one paragraph each is one
//! line, its cells apart by tabs. An edit of that text is not applied to
//! it: [`FlowState::apply`] turns it into the plugin's edits (replace,
//! split, join, delete), which write the format, and the text is read
//! again from the plugin. Undo is the plugin's history.
//!
//! [`DocumentMode::Flow`]: crate::DocumentMode::Flow

use std::collections::HashMap;
use std::ops::Range;

use kalem_viewer::{
    Annotation, AnnotationKind, AsideKind, FlowAlign, FlowItem, FlowParagraph, FlowPlace, FlowRole,
    FlowRun, FlowStyle, FlowStyleKind, MarkChange, Marks, ParagraphChange, Piece, Script,
    ViewerDocument,
};
use org_edit::Transaction;

use crate::view::{LineRole, LineView, OutlineItem, Run, Style};
use crate::viewer::ViewerState;

/// A list item's kind as its label shows it: `Some(true)` numbered,
/// `Some(false)` a bullet; `None` for a paragraph that is no list item.
pub fn list_kind(p: &FlowParagraph) -> Option<bool> {
    if p.role != FlowRole::ListItem {
        return None;
    }
    let numbered = p
        .label
        .as_ref()
        .and_then(|(l, _)| l.chars().next())
        .is_some_and(char::is_alphanumeric);
    Some(numbered)
}

/// The setting `user.name`, as last applied.
static AUTHOR: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Applies the setting `user.name`.
pub fn set_author(name: &str) {
    if let Ok(mut a) = AUTHOR.lock() {
        *a = name.trim().to_string();
    }
}

/// The user's name, which comments and tracked changes made in Kalem
/// carry: the setting `user.name`, else the system account's name.
pub fn author_name() -> Option<String> {
    let set = AUTHOR.lock().map(|a| a.clone()).unwrap_or_default();
    if !set.is_empty() {
        return Some(set);
    }
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|n| !n.trim().is_empty())
}

/// How many items are asked for at a time.
const CHUNK: u32 = 2048;

/// What a line of a flow's text is.
#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    /// Paragraphs: one, or a table row's cells, one each.
    Text,
    /// A break, a placeholder: shown, not text.
    Mark(String),
}

/// A paragraph on a line, from byte `start` of the line.
#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    /// Where its edit text starts in the line.
    pub start: usize,
    /// The paragraph.
    pub para: FlowParagraph,
}

impl Seg {
    fn end(&self) -> usize {
        self.start + self.para.text.len()
    }
}

/// A line of a flow's text.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowLine {
    /// What it is.
    pub kind: LineKind,
    /// What is shown before it (a note's mark, a row's edge), dimmed.
    pub prefix: String,
    /// Its paragraphs.
    pub segs: Vec<Seg>,
    /// A table's row.
    pub row: bool,
}

/// A document's flow unit as the editor shows and edits it.
#[derive(Debug)]
pub struct FlowState {
    /// The viewer's document (the plugin's).
    pub viewer: Box<ViewerState>,
    /// The unit shown.
    pub unit: usize,
    /// The flow's version the text was read at.
    pub version: u64,
    /// Whether the plugin edits it.
    pub editable: bool,
    /// The lines.
    pub lines: Vec<FlowLine>,
    /// Where each line starts in the text.
    starts: Vec<usize>,
    text: String,
    /// Each line's first item, and whether the items are read from there
    /// as from the start (outside tables and asides): where reading may
    /// begin again.
    places: Vec<(u32, bool)>,
    /// How many items the flow has.
    items: u32,
    /// The lines changed since [`FlowState::take_changed`], which a
    /// frontend measures again; all of them when `None`.
    changed: Option<Range<usize>>,
    /// The size of body text, in points, the others are shown in ratio to.
    body_size: f32,
    /// How much body text there is of each size (in half points).
    sizes: HashMap<u32, isize>,
    /// The unit's annotations, by ID.
    pub annotations: HashMap<String, Annotation>,
    /// Whether edits are written as tracked changes; `None` for a format
    /// without them.
    pub tracking: Option<bool>,
}

/// The text a paragraph's edit text is shown as on a line: a line feed,
/// which would end the line, as the line break it stands for.
fn line_text(p: &FlowParagraph) -> String {
    p.text.replace('\n', &kalem_viewer::LINE_BREAK.to_string())
}

/// How much body text of each size (in half points) `lines` have,
/// counted into `counts`, or out of them with `sign` -1.
fn count_sizes(lines: &[FlowLine], counts: &mut HashMap<u32, isize>, sign: isize) {
    for p in lines.iter().flat_map(|l| &l.segs).map(|s| &s.para) {
        if p.role != FlowRole::Body {
            continue;
        }
        for r in &p.runs {
            if let Some(s) = r.marks.size {
                *counts.entry((s * 2.0).round() as u32).or_default() +=
                    sign * r.text.len() as isize;
            }
        }
    }
}

/// The most common size of body text, in points.
fn body_size(counts: &HashMap<u32, isize>) -> f32 {
    counts
        .iter()
        .filter(|(_, n)| **n > 0)
        .max_by_key(|(s, n)| (**n, **s))
        .map_or(0.0, |(s, _)| *s as f32 / 2.0)
}

/// The lines of a flow's items.
pub fn lines_of(items: Vec<FlowItem>) -> Vec<FlowLine> {
    let mut r = LineReader::default();
    r.read(items, 0);
    r.lines
}

/// Items read into lines: the asides open, the tables, the row being
/// read.
#[derive(Default)]
struct LineReader {
    lines: Vec<FlowLine>,
    /// Each line's first item, and whether reading was clean before it.
    places: Vec<(u32, bool)>,
    /// Where the next line's items begin, once one is read after a line.
    next: Option<(u32, bool)>,
    /// Asides open: their label shown before their first paragraph.
    asides: Vec<(AsideKind, String, bool)>,
    depth: usize,
    /// The row being read (at the outermost table): its cells'
    /// paragraphs.
    row: Option<Vec<Vec<FlowParagraph>>>,
}

impl LineReader {
    /// Reading as from the start: outside tables and asides.
    fn clean(&self) -> bool {
        self.asides.is_empty() && self.depth == 0 && self.row.is_none()
    }

    /// What is shown before the next line of the aside open.
    fn prefix(&mut self) -> String {
        match self.asides.last_mut() {
            Some((kind, label, first)) => {
                if std::mem::take(first) {
                    match kind {
                        AsideKind::Header => "Header ▏".to_string(),
                        AsideKind::Footer => "Footer ▏".to_string(),
                        AsideKind::Footnote | AsideKind::Endnote => format!("[{label}] "),
                        AsideKind::Frame | AsideKind::Sidebar => {
                            if label.is_empty() {
                                "▕ ".into()
                            } else {
                                format!("▕ {label}: ")
                            }
                        }
                    }
                } else {
                    "  ".to_string()
                }
            }
            None => String::new(),
        }
    }

    fn push(&mut self, line: FlowLine, at: u32) {
        self.places.push(self.next.take().unwrap_or((at, false)));
        self.lines.push(line);
    }

    /// Reads `items`, the first of them item `first` of the flow.
    fn read(&mut self, items: Vec<FlowItem>, first: u32) {
        for (at, item) in (first..).zip(items) {
            if self.next.is_none() {
                self.next = Some((at, self.clean()));
            }
            self.item(item, at);
        }
    }

    fn item(&mut self, item: FlowItem, at: u32) {
        match item {
            FlowItem::Paragraph(p) => {
                if let Some(cells) = self.row.as_mut() {
                    match cells.last_mut() {
                        Some(c) => c.push(p),
                        None => cells.push(vec![p]),
                    }
                } else {
                    let prefix = self.prefix();
                    self.push(
                        FlowLine {
                            kind: LineKind::Text,
                            prefix,
                            segs: vec![Seg { start: 0, para: p }],
                            row: false,
                        },
                        at,
                    );
                }
            }
            FlowItem::TableStart(_) => self.depth += 1,
            FlowItem::TableEnd => self.depth = self.depth.saturating_sub(1),
            FlowItem::RowStart(_) if self.depth == 1 => self.row = Some(Vec::new()),
            FlowItem::CellStart(_) if self.depth == 1 => {
                if let Some(cells) = self.row.as_mut() {
                    cells.push(Vec::new());
                }
            }
            FlowItem::RowEnd if self.depth == 1 => {
                let cells = self.row.take().unwrap_or_default();
                let edge = self.prefix();
                if cells.iter().all(|c| c.len() == 1) && !cells.is_empty() {
                    // One line: the cells apart by tabs.
                    let mut segs = Vec::new();
                    let mut start = 0;
                    for mut c in cells {
                        let p = c.remove(0);
                        let len = line_text(&p).len();
                        segs.push(Seg { start, para: p });
                        start += len + 1;
                    }
                    self.push(
                        FlowLine {
                            kind: LineKind::Text,
                            prefix: format!("{edge}▏"),
                            segs,
                            row: true,
                        },
                        at,
                    );
                } else {
                    // A paragraph a line, each cell's first marked.
                    for (i, c) in cells.into_iter().enumerate() {
                        for (j, p) in c.into_iter().enumerate() {
                            let mark = match (i, j) {
                                (0, 0) => "▏",
                                (_, 0) => "▏┆ ",
                                _ => "▏  ",
                            };
                            self.push(
                                FlowLine {
                                    kind: LineKind::Text,
                                    prefix: format!("{edge}{mark}"),
                                    segs: vec![Seg { start: 0, para: p }],
                                    row: true,
                                },
                                at,
                            );
                        }
                    }
                }
            }
            FlowItem::RowStart(_)
            | FlowItem::CellStart(_)
            | FlowItem::RowEnd
            | FlowItem::CellEnd => {}
            FlowItem::AsideStart(a) => self.asides.push((a.kind, a.label, true)),
            FlowItem::AsideEnd => {
                self.asides.pop();
            }
            FlowItem::Rule(kind) => {
                let shown = match kind.as_str() {
                    "page" => "page break".to_string(),
                    "column" => "column break".to_string(),
                    "line" => String::new(),
                    k => format!("section break ({k})"),
                };
                self.push(
                    FlowLine {
                        kind: LineKind::Mark(shown),
                        prefix: String::new(),
                        segs: Vec::new(),
                        row: false,
                    },
                    at,
                );
            }
            FlowItem::Placeholder(name) => self.push(
                FlowLine {
                    kind: LineKind::Mark(name),
                    prefix: String::new(),
                    segs: Vec::new(),
                    row: false,
                },
                at,
            ),
        }
    }
}

/// The text of a line.
fn line_string(l: &FlowLine, out: &mut String) {
    for (j, s) in l.segs.iter().enumerate() {
        if j > 0 {
            out.push('\t');
        }
        out.push_str(&line_text(&s.para));
    }
}

/// The text of lines: each paragraph's edit text, a row's cells apart by
/// tabs, a line feed after each line but the last.
fn text_of(lines: &[FlowLine]) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut starts = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        starts.push(text.len());
        line_string(l, &mut text);
    }
    (text, starts)
}

/// Items `from..from + count` of a unit's flow, fetched a chunk at a time.
fn fetch(doc: &mut dyn ViewerDocument, unit: usize, from: u32, count: u32) -> Vec<FlowItem> {
    let mut items = Vec::with_capacity(count as usize);
    let mut at = from;
    let end = from.saturating_add(count);
    while at < end {
        let got = doc.flow_items(unit, at, CHUNK.min(end - at));
        if got.is_empty() {
            break;
        }
        at += got.len() as u32;
        items.extend(got);
    }
    items
}

fn err(e: kalem_viewer::ViewerError) -> String {
    e.0
}

impl FlowState {
    /// The flow of the unit `viewer` shows, when it is one; the viewer
    /// back when it is not.
    pub fn open(viewer: ViewerState) -> Result<FlowState, Box<ViewerState>> {
        let unit = viewer.unit;
        let layout = viewer.doc().flow(unit);
        let Some(layout) = layout else {
            return Err(Box::new(viewer));
        };
        let mut f = FlowState {
            viewer: Box::new(viewer),
            unit,
            version: layout.version,
            editable: layout.editable,
            lines: Vec::new(),
            starts: Vec::new(),
            text: String::new(),
            places: Vec::new(),
            items: 0,
            changed: None,
            body_size: 0.0,
            sizes: HashMap::new(),
            annotations: HashMap::new(),
            tracking: None,
        };
        f.refresh();
        Ok(f)
    }

    /// Reads the flow again from the plugin; its text. What changed since
    /// the version held is read when the plugin tells (API 0.2.9,
    /// `flow-3`), the whole flow otherwise.
    pub fn refresh(&mut self) -> String {
        if !self.refresh_changed() {
            self.refresh_whole();
        }
        let (annotations, tracking) = {
            let mut doc = self.viewer.doc();
            (doc.annotations(Some(self.unit)), doc.tracking())
        };
        self.annotations = annotations.into_iter().map(|a| (a.id.clone(), a)).collect();
        self.tracking = tracking;
        self.text.clone()
    }

    /// The whole flow read again.
    fn refresh_whole(&mut self) {
        let (items, layout) = {
            let mut doc = self.viewer.doc();
            let layout = doc.flow(self.unit).unwrap_or_default();
            (fetch(&mut **doc, self.unit, 0, layout.items), layout)
        };
        self.version = layout.version;
        self.editable = layout.editable;
        self.items = layout.items;
        let mut r = LineReader::default();
        r.read(items, 0);
        self.lines = r.lines;
        self.places = r.places;
        let (text, starts) = text_of(&self.lines);
        self.text = text;
        self.starts = starts;
        self.sizes.clear();
        count_sizes(&self.lines, &mut self.sizes, 1);
        self.body_size = body_size(&self.sizes);
        self.changed = None;
    }

    /// Whether what was read edit by edit is what reading the whole flow
    /// gives (for tests); the flow is read whole after.
    #[doc(hidden)]
    pub fn reads_as_whole(&mut self) -> Result<(), String> {
        let kept = (
            self.lines.clone(),
            self.text.clone(),
            self.starts.clone(),
            self.places.clone(),
            self.items,
            self.body_size,
        );
        let changed = self.changed.clone();
        self.refresh_whole();
        self.changed = changed;
        let whole = (
            self.lines.clone(),
            self.text.clone(),
            self.starts.clone(),
            self.places.clone(),
            self.items,
            self.body_size,
        );
        if kept == whole {
            return Ok(());
        }
        let line = (0..kept.0.len().max(whole.0.len())).find(|i| kept.0.get(*i) != whole.0.get(*i));
        Err(format!(
            "read edit by edit: {} lines, {} items, text {:?}, starts {:?}, places {:?}, \
             first differing line {line:?}: {:?}; read whole: {} lines, {} items, text {:?}, \
             starts {:?}, places {:?}, {:?}",
            kept.0.len(),
            kept.4,
            kept.1,
            kept.2,
            kept.3,
            line.and_then(|i| kept.0.get(i)),
            whole.0.len(),
            whole.4,
            whole.1,
            whole.2,
            whole.3,
            line.and_then(|i| whole.0.get(i)),
        ))
    }

    /// The lines changed since last asked (their look may have changed
    /// with their text, or without it); all of them when `None`.
    pub fn take_changed(&mut self) -> Option<Range<usize>> {
        self.changed.replace(0..0)
    }

    /// Lines `first..last` replaced by `n` lines: the lines changed since
    /// last asked, as they are numbered now.
    fn lines_replaced(&mut self, first: usize, last: usize, n: usize) {
        self.changed = self.changed.take().map(|r| {
            if r.is_empty() {
                return first..first + n;
            }
            let end = if r.end <= first {
                r.end
            } else if r.end >= last {
                r.end + n - (last - first)
            } else {
                first + n
            };
            r.start.min(first)..end.max(first + n)
        });
    }

    /// Reads again the items changed since the version held, as the
    /// plugin tells, and the lines they make; false when it does not
    /// tell, or what it tells does not fit, and the flow is to be read
    /// whole.
    fn refresh_changed(&mut self) -> bool {
        if self.lines.is_empty() {
            return false;
        }
        let mut doc = self.viewer.doc();
        let Some(layout) = doc.flow(self.unit) else {
            return false;
        };
        if layout.version == self.version {
            self.editable = layout.editable;
            return true;
        }
        let Some(c) = doc.flow_changes(self.unit, self.version) else {
            return false;
        };
        if c.is_none() {
            self.version = layout.version;
            self.editable = layout.editable;
            return true;
        }
        let end = c.from.checked_add(c.removed);
        if end.is_none_or(|e| e > self.items)
            || i64::from(layout.items)
                != i64::from(self.items) - i64::from(c.removed) + i64::from(c.added)
        {
            return false;
        }
        // From the last line at or before the change read as from the
        // start, to the first such line after it.
        let mut first = self
            .places
            .partition_point(|(at, _)| *at <= c.from)
            .saturating_sub(1);
        while first > 0 && !self.places[first].1 {
            first -= 1;
        }
        let changed_end = c.from + c.removed;
        let mut last = self.places.partition_point(|(at, _)| *at < changed_end);
        while last < self.places.len() && !self.places[last].1 {
            last += 1;
        }
        let from = self.places[first].0;
        let old_end = self.places.get(last).map_or(self.items, |p| p.0);
        let grown = i64::from(c.added) - i64::from(c.removed);
        let new_end = (i64::from(old_end) + grown) as u32;
        let items = fetch(&mut **doc, self.unit, from, new_end.saturating_sub(from));
        drop(doc);
        if items.len() as u32 != new_end.saturating_sub(from) {
            return false;
        }
        let mut r = LineReader::default();
        r.read(items, from);
        if !r.clean() || r.lines.is_empty() {
            return false;
        }
        // The text: each line followed by a line feed, for the while.
        self.text.push('\n');
        let a = self.starts[first];
        let b = self.starts.get(last).copied().unwrap_or(self.text.len());
        let mut new = String::new();
        let mut starts = Vec::with_capacity(r.lines.len());
        for l in &r.lines {
            starts.push(a + new.len());
            line_string(l, &mut new);
            new.push('\n');
        }
        self.text.replace_range(a..b, &new);
        self.text.pop();
        let moved = new.len() as isize - (b - a) as isize;
        for s in &mut self.starts[last..] {
            *s = s.saturating_add_signed(moved);
        }
        let n = r.lines.len();
        self.starts.splice(first..last, starts);
        count_sizes(&self.lines[first..last], &mut self.sizes, -1);
        count_sizes(&r.lines, &mut self.sizes, 1);
        self.lines.splice(first..last, r.lines);
        for p in &mut self.places[last..] {
            p.0 = (i64::from(p.0) + grown) as u32;
        }
        self.places.splice(first..last, r.places);
        // The paragraphs after, numbered as the plugin moved them.
        if c.shift != 0 {
            for s in self.lines[first + n..].iter_mut().flat_map(|l| &mut l.segs) {
                if let Some(i) = &mut s.para.index {
                    *i = i.saturating_add_signed(c.shift);
                }
            }
        }
        self.version = layout.version;
        self.editable = layout.editable;
        self.items = layout.items;
        let size = body_size(&self.sizes);
        self.lines_replaced(first, last, n);
        if size != self.body_size {
            // Another size of body text: every line looks otherwise.
            self.changed = None;
        }
        self.body_size = size;
        true
    }

    /// The text shown.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The line holding byte `pos` of the text, and the byte of that line.
    pub fn locate(&self, pos: usize) -> (usize, usize) {
        let i = self.starts.partition_point(|s| *s <= pos).saturating_sub(1);
        (i, pos - self.starts.get(i).copied().unwrap_or(0))
    }

    /// The line starting at byte `start` of the text.
    pub fn line_at(&self, start: usize) -> Option<&FlowLine> {
        let (i, col) = self.locate(start);
        (col == 0).then(|| self.lines.get(i)).flatten()
    }

    /// The paragraph and byte of its edit text at byte `pos` of the text.
    pub fn seg_at(&self, pos: usize) -> Option<(&Seg, usize)> {
        let (i, col) = self.locate(pos);
        let line = self.lines.get(i)?;
        let s = line
            .segs
            .iter()
            .find(|s| s.start <= col && col <= s.end())?;
        Some((s, col - s.start))
    }

    /// The place at byte `pos` of the text: an edited paragraph and a byte
    /// of its edit text; why not, where it is none.
    pub fn place(&self, pos: usize) -> Result<FlowPlace, String> {
        let (i, _) = self.locate(pos);
        if let Some(LineKind::Mark(m)) = self.lines.get(i).map(|l| &l.kind) {
            let what = if m.is_empty() { "a line" } else { m.as_str() };
            return Err(format!("This is {what}, not text"));
        }
        let (s, offset) = self
            .seg_at(pos)
            .ok_or_else(|| "This is not text of the document".to_string())?;
        let index = s
            .para
            .index
            .ok_or_else(|| "This paragraph is not edited".to_string())?;
        Ok(FlowPlace {
            paragraph: index,
            offset: offset as u32,
        })
    }

    /// Applies `tx`, an edit of the text, as the plugin's edits, one step
    /// of its history; refused with the plugin's reason. The text is not
    /// changed here: [`FlowState::refresh`] reads it again.
    pub fn apply(&mut self, tx: &Transaction) -> Result<(), String> {
        if !self.editable {
            return Err("This document is not edited".into());
        }
        let unit = self.unit;
        let mut doc = self.viewer.doc();
        let before = doc.flow(unit).map(|l| l.version);
        doc.begin_batch();
        // From the last edit back, so that the places of the earlier ones
        // stay where they were.
        let mut result = Ok(());
        for e in tx.edits.iter().rev() {
            result = self.edit_one(&mut **doc, e.range.clone(), &e.insert);
            if result.is_err() {
                break;
            }
        }
        doc.end_batch();
        if result.is_err() && doc.flow(unit).map(|l| l.version) != before {
            // What the refused edit half did is undone.
            let _ = doc.undo();
        }
        result
    }

    fn edit_one(
        &self,
        doc: &mut dyn ViewerDocument,
        range: Range<usize>,
        insert: &str,
    ) -> Result<(), String> {
        let unit = self.unit;
        let pieces: Vec<&str> = insert.split('\n').collect();
        let a = self.place(range.start)?;
        let mut at = a;
        if range.start < range.end {
            let b = self.place(range.end)?;
            if a.paragraph == b.paragraph {
                if pieces.len() == 1 {
                    return doc
                        .flow_replace(unit, a.paragraph, a.offset..b.offset, pieces[0])
                        .map_err(err);
                }
                doc.flow_replace(unit, a.paragraph, a.offset..b.offset, "")
                    .map_err(err)?;
            } else {
                let (la, _) = self.locate(range.start);
                let (lb, _) = self.locate(range.end);
                if la == lb {
                    return Err("An edit across a table's cells is not made".into());
                }
                let a_len = self
                    .seg_at(range.start)
                    .map_or(0, |(s, _)| s.para.text.len()) as u32;
                if b.paragraph == a.paragraph + 1 && b.offset == 0 && a.offset == a_len {
                    // The line feed between two paragraphs: joined.
                    doc.flow_join(unit, a.paragraph).map_err(err)?;
                } else {
                    doc.flow_delete(unit, a, b).map_err(err)?;
                }
            }
        }
        for (i, piece) in pieces.iter().enumerate() {
            if i > 0 {
                doc.flow_split(unit, at).map_err(err)?;
                at = FlowPlace {
                    paragraph: at.paragraph + 1,
                    offset: 0,
                };
            }
            if !piece.is_empty() {
                doc.flow_replace(unit, at.paragraph, at.offset..at.offset, piece)
                    .map_err(err)?;
                at.offset += piece.len() as u32;
            }
        }
        Ok(())
    }

    /// Undoes the plugin's last step; whether there was one.
    pub fn undo(&mut self) -> Result<bool, String> {
        self.viewer.undo()
    }

    /// Redoes the plugin's last step undone.
    pub fn redo(&mut self) -> Result<bool, String> {
        self.viewer.redo()
    }

    /// The anchor of bytes `range` of the text: from the place at its
    /// start to the place at its end (a caret's, the word it is in).
    pub fn anchor(&self, range: Range<usize>) -> Result<kalem_viewer::Anchor, String> {
        Ok(kalem_viewer::Anchor::Flow {
            unit: self.unit,
            from: self.place(range.start)?,
            to: self.place(range.end)?,
        })
    }

    /// Runs `f` on the plugin's document; the flow read again after it.
    fn annotate<R>(
        &mut self,
        f: impl FnOnce(&mut dyn ViewerDocument) -> kalem_viewer::Result<R>,
    ) -> Result<R, String> {
        let r = {
            let mut doc = self.viewer.doc();
            f(&mut **doc).map_err(err)
        };
        self.refresh();
        r
    }

    /// Adds a comment on bytes `range` of the text; its ID.
    pub fn comment(&mut self, range: Range<usize>, text: &str) -> Result<String, String> {
        let on = self.anchor(range)?;
        self.annotate(|d| d.comment(on, text))
    }

    /// Answers comment `parent`.
    pub fn reply(&mut self, parent: &str, text: &str) -> Result<String, String> {
        self.annotate(|d| d.reply(parent, text))
    }

    /// The paragraphs bytes `range` of the text covers, each with the
    /// bytes of its edit text in the range; for a caret, the paragraph it
    /// is in, with an empty range.
    pub fn spans(&self, range: Range<usize>) -> Vec<(&FlowParagraph, Range<usize>)> {
        let mut out = Vec::new();
        if self.lines.is_empty() {
            return out;
        }
        let (first, _) = self.locate(range.start);
        let (last, _) = self.locate(range.end);
        for i in first..=last.min(self.lines.len() - 1) {
            let base = self.starts[i];
            for s in &self.lines[i].segs {
                let (a, b) = (base + s.start, base + s.end());
                let (from, to) = (range.start.max(a), range.end.min(b));
                let caret = range.is_empty() && a <= range.start && range.start <= b;
                if from < to || caret {
                    out.push((&s.para, from.min(to) - a..to.max(from) - a));
                }
            }
        }
        out
    }

    /// The runs of text bytes `range` of the text covers.
    pub fn runs_in(&self, range: Range<usize>) -> Vec<&FlowRun> {
        self.spans(range)
            .into_iter()
            .flat_map(|(p, r)| {
                p.runs.iter().filter(move |run| {
                    let (s, e) = (run.source.start as usize, run.source.end as usize);
                    run.piece == Piece::Text && s < r.end && e > r.start
                })
            })
            .collect()
    }

    /// The run of byte `pos`: the one before it, whose look typing there
    /// takes; at a paragraph's start, its first.
    pub fn run_at(&self, pos: usize) -> Option<&FlowRun> {
        let (s, off) = self.seg_at(pos)?;
        let runs = &s.para.runs;
        runs.iter()
            .filter(|r| r.piece == Piece::Text)
            .find(|r| (r.source.start as usize) < off && off <= r.source.end as usize)
            .or_else(|| runs.iter().find(|r| r.piece == Piece::Text))
    }

    /// The lines that are horizontal rules (a rule of the flow with no
    /// name: a line drawn across, not a page or section break), each with
    /// its line feed.
    pub fn rule_lines(&self) -> Vec<Range<usize>> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(&l.kind, LineKind::Mark(m) if m.is_empty()))
            .map(|(i, _)| {
                let start = self.starts[i];
                let end = self.starts.get(i + 1).copied().unwrap_or(self.text.len());
                start..end
            })
            .collect()
    }

    /// The paragraph of byte `pos`.
    pub fn paragraph_at(&self, pos: usize) -> Option<&FlowParagraph> {
        self.seg_at(pos).map(|(s, _)| &s.para)
    }

    /// The edited paragraphs bytes `range` covers: their indices and the
    /// bytes of each in it.
    fn edited(&self, range: Range<usize>) -> Result<Vec<(u32, Range<usize>)>, String> {
        let out: Vec<(u32, Range<usize>)> = self
            .spans(range)
            .into_iter()
            .filter_map(|(p, r)| p.index.map(|i| (i, r)))
            .collect();
        if out.is_empty() {
            return Err("This is not text the document edits".into());
        }
        Ok(out)
    }

    /// Changes the look of bytes `range` of the text: `changes` made on
    /// every run in it, by the plugin, one step.
    pub fn set_marks(&mut self, range: Range<usize>, changes: &[MarkChange]) -> Result<(), String> {
        let spans = self.edited(range)?;
        let (first, last) = (&spans[0], &spans[spans.len() - 1]);
        let from = FlowPlace {
            paragraph: first.0,
            offset: first.1.start as u32,
        };
        let to = FlowPlace {
            paragraph: last.0,
            offset: last.1.end as u32,
        };
        let unit = self.unit;
        self.annotate(|d| d.flow_set_marks(unit, from, to, changes))
    }

    /// Gives the paragraphs bytes `range` covers (the cursor's, for a
    /// caret) paragraph style `style` (its [`FlowStyle::id`]).
    pub fn set_style(&mut self, range: Range<usize>, style: &str) -> Result<(), String> {
        let spans = self.edited(range)?;
        let from = spans.iter().map(|s| s.0).min().unwrap_or(0);
        let to = spans.iter().map(|s| s.0).max().unwrap_or(0);
        let unit = self.unit;
        self.annotate(|d| d.flow_set_style(unit, from, to, style))
    }

    /// Changes the look of the paragraphs bytes `range` covers (the
    /// cursor's, for a caret): `changes` made on each, by the plugin, one
    /// step (API 0.2.8, `flow-2`).
    pub fn set_paragraphs(
        &mut self,
        range: Range<usize>,
        changes: &[ParagraphChange],
    ) -> Result<(), String> {
        let spans = self.edited(range)?;
        let from = spans.iter().map(|s| s.0).min().unwrap_or(0);
        let to = spans.iter().map(|s| s.0).max().unwrap_or(0);
        let unit = self.unit;
        self.annotate(|d| d.flow_set_paragraphs(unit, from, to, changes))
    }

    /// Changes worked out for each paragraph bytes `range` covers from its
    /// own look (a list's level one deeper, an indent a step further),
    /// all in one step of the plugin's history.
    pub fn set_each_paragraph(
        &mut self,
        range: Range<usize>,
        f: impl Fn(&FlowParagraph) -> Vec<ParagraphChange>,
    ) -> Result<(), String> {
        let mut each: Vec<(u32, Vec<ParagraphChange>)> = Vec::new();
        for (p, _) in self.spans(range) {
            if let Some(i) = p.index
                && !each.iter().any(|(j, _)| *j == i)
            {
                each.push((i, f(p)));
            }
        }
        if each.is_empty() {
            return Err("This is not text the document edits".into());
        }
        let unit = self.unit;
        self.annotate(|d| {
            d.begin_batch();
            let mut r = Ok(());
            let mut made = false;
            for (i, changes) in &each {
                if changes.is_empty() {
                    continue;
                }
                r = d.flow_set_paragraphs(unit, *i, *i, changes);
                if r.is_err() {
                    break;
                }
                made = true;
            }
            d.end_batch();
            // A refusal halfway: the paragraphs changed before it back.
            if r.is_err() && made {
                let _ = d.undo();
            }
            r
        })
    }

    /// The paragraph styles a user picks among, as the plugin offers them:
    /// the default style first, then the title's, the headings' in their
    /// order, then the others by name.
    pub fn paragraph_styles(&mut self) -> Vec<FlowStyle> {
        let mut list: Vec<FlowStyle> = self
            .viewer
            .doc()
            .flow_styles()
            .into_iter()
            .filter(|s| s.kind == FlowStyleKind::Paragraph && s.shown)
            .collect();
        let rank = |s: &FlowStyle| {
            let n = s.name.to_lowercase();
            if n == "normal" || n == "default paragraph style" {
                (0, 0, n)
            } else if n == "title" || n == "subtitle" {
                (1, usize::from(n == "subtitle"), n)
            } else if let Some(level) = n
                .strip_prefix("heading ")
                .and_then(|l| l.parse::<usize>().ok())
            {
                (2, level, n)
            } else {
                (3, 0, n)
            }
        };
        list.sort_by_key(rank);
        list
    }

    /// The look at byte `pos` as the toolbar shows it: the paragraph's
    /// style, the run's typeface, size and marks.
    pub fn look_at(&self, pos: usize) -> Option<(String, Marks)> {
        let p = self.paragraph_at(pos)?;
        let marks = self
            .run_at(pos)
            .map(|r| r.marks.clone())
            .unwrap_or_default();
        Some((p.style.clone(), marks))
    }

    /// Changes comment `id`'s text: a paragraph a line.
    pub fn set_comment_text(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.annotate(|d| d.set_comment_text(id, text))
    }

    /// The comment at byte `pos` of the text: the first of those whose
    /// text it is in, as the Review commands take it.
    pub fn comment_at(&self, pos: usize) -> Option<&Annotation> {
        self.annotations_at(pos)
            .into_iter()
            .find(|a| a.kind == AnnotationKind::Comment)
    }

    /// Marks comment `id` done, or not.
    pub fn resolve(&mut self, id: &str, done: bool) -> Result<(), String> {
        self.annotate(|d| d.resolve(id, done))
    }

    /// Takes comment `id` away, with its answers.
    pub fn remove_comment(&mut self, id: &str) -> Result<(), String> {
        self.annotate(|d| d.remove_comment(id))
    }

    /// Accepts (`true`) or rejects tracked change `id`.
    pub fn decide(&mut self, id: &str, accept: bool) -> Result<(), String> {
        self.annotate(|d| if accept { d.accept(id) } else { d.reject(id) })
    }

    /// Accepts or rejects every tracked change of the unit.
    pub fn decide_all(&mut self, accept: bool) -> Result<(), String> {
        let unit = self.unit;
        self.annotate(|d| {
            if accept {
                d.accept_all(Some(unit))
            } else {
                d.reject_all(Some(unit))
            }
        })
    }

    /// Turns tracking changes on or off.
    pub fn set_tracking(&mut self, on: bool) -> Result<(), String> {
        self.annotate(|d| d.set_tracking(on))
    }

    /// The comments of the unit, the answers after the comment they
    /// answer, in the order of the text.
    pub fn comments(&self) -> Vec<&Annotation> {
        let mut top: Vec<&Annotation> = self
            .annotations
            .values()
            .filter(|a| a.kind == AnnotationKind::Comment && a.parent.is_none())
            .collect();
        let place = |a: &Annotation| match a.anchors.first() {
            Some(kalem_viewer::Anchor::Flow { from, .. }) => (from.paragraph, from.offset),
            _ => (u32::MAX, 0),
        };
        top.sort_by_key(|a| (place(a), a.id.clone()));
        let mut out = Vec::new();
        for c in top {
            out.push(c);
            let mut answers: Vec<&Annotation> = self
                .annotations
                .values()
                .filter(|a| a.parent.as_deref() == Some(c.id.as_str()))
                .collect();
            answers.sort_by_key(|a| (a.date.clone(), a.id.clone()));
            out.extend(answers);
        }
        out
    }

    /// The tracked changes of the unit, in the order of the text.
    pub fn changes(&self) -> Vec<&Annotation> {
        let mut v: Vec<&Annotation> = self
            .annotations
            .values()
            .filter(|a| a.kind.is_change())
            .collect();
        v.sort_by_key(|a| {
            (
                match a.anchors.first() {
                    Some(kalem_viewer::Anchor::Flow { from, .. }) => (from.paragraph, from.offset),
                    _ => (u32::MAX, 0),
                },
                a.id.clone(),
            )
        });
        v
    }

    /// Where an annotation starts in the text, when it is in the unit's
    /// flow: the first run naming it.
    pub fn start_of(&self, id: &str) -> Option<usize> {
        for (i, l) in self.lines.iter().enumerate() {
            for s in &l.segs {
                if let Some(r) = s
                    .para
                    .runs
                    .iter()
                    .find(|r| r.annotations.iter().any(|a| a == id))
                {
                    return Some(self.starts[i] + s.start + r.source.start as usize);
                }
                if s.para.annotations.iter().any(|a| a == id) {
                    return Some(self.starts[i] + s.end());
                }
            }
        }
        None
    }

    /// The annotations of the text at byte `pos`: those of the run there.
    pub fn annotations_at(&self, pos: usize) -> Vec<&Annotation> {
        let Some((s, offset)) = self.seg_at(pos) else {
            return Vec::new();
        };
        let offset = offset as u32;
        let mut ids: Vec<&String> = s
            .para
            .runs
            .iter()
            .filter(|r| r.source.start <= offset && offset <= r.source.end)
            .flat_map(|r| r.annotations.iter())
            .collect();
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| self.annotations.get(id))
            .collect()
    }

    /// The headings, as an outline.
    pub fn outline(&self) -> Vec<OutlineItem> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let s = l.segs.first()?;
                let level = match s.para.role {
                    FlowRole::Heading => usize::from(s.para.level.max(1)),
                    FlowRole::Title => 1,
                    _ => return None,
                };
                let title: String = s
                    .para
                    .runs
                    .iter()
                    .filter(|r| !r.marks.hidden && r.piece == Piece::Text)
                    .map(|r| r.text.as_str())
                    .collect();
                let title = title.trim().to_string();
                (!title.is_empty()).then_some(OutlineItem {
                    level,
                    todo: None,
                    title,
                    start: self.starts[i],
                    file: None,
                })
            })
            .collect()
    }

    /// A color of the plugin's (`[r, g, b]`) as the theme's.
    fn color(c: [u8; 3]) -> crate::theme::Color {
        let [r, g, b] = c;
        crate::theme::Color(
            (u32::from(r) << 24) | (u32::from(g) << 16) | (u32::from(b) << 8) | 0xff,
        )
    }

    /// The style of a run's marks.
    fn style(&self, m: &Marks, annotations: &[String], link: bool) -> Style {
        let mut s = Style {
            bold: m.bold,
            italic: m.italic,
            underline: m.underline.is_some(),
            strike: m.strike || m.double_strike,
            superscript: m.script == Script::Superscript,
            subscript: m.script == Script::Subscript,
            link,
            ..Style::default()
        };
        if let Some(face) = &m.face {
            s.rich.font = Some(crate::rich::FontName::new(face));
        }
        if let Some(size) = m.size
            && self.body_size > 0.0
            && (size - self.body_size).abs() >= 0.5
        {
            s.rich.size = Some((size / self.body_size * 160.0).round().clamp(40.0, 1200.0) as u16);
        }
        s.rich.color = m.color.map(Self::color);
        s.rich.highlight = m.highlight.map(Self::color);
        // The document's colors, chosen for a white page.
        s.rich.paper = true;
        // Tracked changes and comments, the same for every plugin.
        for id in annotations {
            let Some(a) = self.annotations.get(id) else {
                continue;
            };
            match a.kind {
                AnnotationKind::Insertion | AnnotationKind::MoveTo => {
                    s.underline = true;
                    s.rich.color = Some(Self::color(INSERTED));
                }
                AnnotationKind::Deletion | AnnotationKind::MoveFrom => {
                    s.strike = true;
                    s.rich.color = Some(Self::color(DELETED));
                }
                AnnotationKind::Formatting => {}
                AnnotationKind::Comment => {
                    if s.rich.highlight.is_none() && !a.resolved {
                        s.rich.highlight = Some(Self::color(COMMENTED));
                    }
                }
            }
        }
        s
    }
}

/// What the status bar says at byte `pos` of a flow's text: the comment
/// there (its author, text and answers), else the tracked change.
pub fn status(f: &FlowState, pos: usize) -> Option<String> {
    let here = f.annotations_at(pos);
    if let Some(c) = here.iter().find(|a| a.kind == AnnotationKind::Comment) {
        let key = if c.resolved {
            "flow-comment-done"
        } else {
            "flow-comment"
        };
        let mut s = crate::tr!(key, author = c.author.clone(), text = c.text.clone());
        let answers = f
            .annotations
            .values()
            .filter(|a| a.parent.as_deref() == Some(c.id.as_str()))
            .count();
        if answers > 0 {
            s.push_str(&format!(
                " · {}",
                crate::tr!("flow-answers", count = answers)
            ));
        }
        return Some(s);
    }
    let change = here.into_iter().find(|a| a.kind.is_change())?;
    let key = match change.kind {
        AnnotationKind::Insertion => "flow-inserted",
        AnnotationKind::Deletion => "flow-deleted",
        AnnotationKind::Formatting => "flow-formatted",
        _ => "flow-moved",
    };
    let mut s = crate::tr!(key, author = change.author.clone());
    if let Some(d) = &change.date {
        s.push_str(&format!(", {}", d.replace('T', " ").trim_end_matches('Z')));
    }
    Some(s)
}

/// The color of inserted text (a tracked change).
const INSERTED: [u8; 3] = [0x1a, 0x7f, 0x37];
/// The color of deleted text.
const DELETED: [u8; 3] = [0xc0, 0x2b, 0x2b];
/// The highlight of text a comment is on.
const COMMENTED: [u8; 3] = [0xff, 0xe8, 0x99];

fn dim(text: impl Into<String>, at: usize) -> Run {
    Run {
        src: at..at,
        text: text.into(),
        verbatim: false,
        style: Style {
            dim: true,
            ..Style::default()
        },
        widget: None,
    }
}

/// The line of a flow document's text at `range` as the editors show it.
pub fn line_view(f: &FlowState, range: Range<usize>) -> LineView {
    let mut v = LineView {
        range: range.clone(),
        ..LineView::default()
    };
    let base = range.start;
    let Some(line) = f.line_at(base) else {
        return v;
    };
    if !line.prefix.is_empty() {
        v.runs.push(dim(line.prefix.clone(), base));
    }
    if let LineKind::Mark(m) = &line.kind {
        v.role = LineRole::Delimiter;
        let shown = if m.is_empty() {
            "────────".to_string()
        } else {
            format!("── {m} ──")
        };
        v.runs.push(dim(shown, base));
        return v;
    }
    for (k, seg) in line.segs.iter().enumerate() {
        let start = base + seg.start;
        if k > 0 {
            // The tab between cells.
            v.runs.push(Run {
                src: start - 1..start,
                text: " │ ".into(),
                verbatim: false,
                style: Style {
                    dim: true,
                    ..Style::default()
                },
                widget: None,
            });
        }
        let p = &seg.para;
        if k == 0 {
            v.heading = match p.role {
                FlowRole::Heading => p.level.clamp(1, 6),
                _ => 0,
            };
            v.mono = p.role == FlowRole::Code;
            v.align = match p.align {
                FlowAlign::Start => crate::rich::Align::Left,
                FlowAlign::Center => crate::rich::Align::Center,
                FlowAlign::End => crate::rich::Align::Right,
                FlowAlign::Justify => crate::rich::Align::Justify,
            };
        }
        if let Some((label, marks)) = &p.label {
            let mut style = f.style(marks, &[], false);
            style.dim = false;
            v.runs.push(Run {
                src: start..start,
                text: format!("{label} "),
                verbatim: false,
                style,
                widget: None,
            });
        }
        if p.role == FlowRole::Quote && p.label.is_none() {
            v.runs.push(dim("▎", start));
        }
        let text = line_text(p);
        let mut at = 0usize;
        let mut runs: Vec<&kalem_viewer::FlowRun> = p.runs.iter().collect();
        runs.sort_by_key(|r| r.source.start);
        let plain = |from: usize, to: usize, out: &mut Vec<Run>| {
            if from < to {
                out.push(Run {
                    src: start + from..start + to,
                    text: text[from..to].to_string(),
                    verbatim: true,
                    style: Style::default(),
                    widget: None,
                });
            }
        };
        for r in runs {
            let (a, b) = (r.source.start as usize, r.source.end as usize);
            if a < at || b > text.len() || !text.is_char_boundary(a) || !text.is_char_boundary(b) {
                continue;
            }
            plain(at, a, &mut v.runs);
            let mut style = f.style(&r.marks, &r.annotations, r.link.is_some());
            let source = &text[a..b];
            let shown = match &r.piece {
                Piece::Text => {
                    if r.marks.hidden {
                        String::new()
                    } else if r.marks.caps || r.marks.small_caps {
                        r.text.to_uppercase()
                    } else {
                        r.text.clone()
                    }
                }
                Piece::Tab => "\t".into(),
                Piece::LineBreak => {
                    style.dim = true;
                    "↵".into()
                }
                Piece::PageBreak => {
                    style.dim = true;
                    "── page break ──".into()
                }
                Piece::ColumnBreak => {
                    style.dim = true;
                    "── column break ──".into()
                }
                Piece::NoteMark(_) => {
                    style.superscript = true;
                    r.text.clone()
                }
                Piece::Picture(pic) => {
                    style.dim = true;
                    if pic.alt.is_empty() {
                        "[Picture]".into()
                    } else {
                        format!("[Picture: {}]", pic.alt)
                    }
                }
                Piece::Placeholder => {
                    style.dim = true;
                    r.text.clone()
                }
            };
            let verbatim = shown == source;
            v.runs.push(Run {
                src: start + a..start + b,
                text: shown,
                verbatim,
                style,
                widget: None,
            });
            at = b;
        }
        plain(at, text.len(), &mut v.runs);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use kalem_viewer::{Aside, FlowCell, FlowRow, FlowRun, FlowTable};

    fn para(index: u32, text: &str) -> FlowParagraph {
        FlowParagraph {
            index: Some(index),
            text: text.into(),
            runs: vec![FlowRun {
                text: text.into(),
                source: 0..text.len() as u32,
                ..FlowRun::default()
            }],
            ..FlowParagraph::default()
        }
    }

    #[test]
    fn items_become_lines() {
        let items = vec![
            FlowItem::Paragraph(para(0, "Title")),
            FlowItem::TableStart(FlowTable::default()),
            FlowItem::RowStart(FlowRow::default()),
            FlowItem::CellStart(FlowCell::default()),
            FlowItem::Paragraph(para(1, "a")),
            FlowItem::CellEnd,
            FlowItem::CellStart(FlowCell::default()),
            FlowItem::Paragraph(para(2, "bc")),
            FlowItem::CellEnd,
            FlowItem::RowEnd,
            FlowItem::RowStart(FlowRow::default()),
            FlowItem::CellStart(FlowCell::default()),
            FlowItem::Paragraph(para(3, "x")),
            FlowItem::Paragraph(para(4, "y")),
            FlowItem::CellEnd,
            FlowItem::RowEnd,
            FlowItem::TableEnd,
            FlowItem::Rule("page".into()),
            FlowItem::AsideStart(Aside {
                kind: AsideKind::Footnote,
                id: "1".into(),
                label: "1".into(),
            }),
            FlowItem::Paragraph(para(5, "Note\nwith a break")),
            FlowItem::AsideEnd,
        ];
        let lines = lines_of(items);
        let (text, starts) = text_of(&lines);
        assert_eq!(text, "Title\na\tbc\nx\ny\n\nNote\u{B}with a break");
        assert_eq!(starts, [0, 6, 11, 13, 15, 16]);
        assert!(lines[1].row && lines[1].segs.len() == 2 && lines[1].segs[1].start == 2);
        assert_eq!(lines[2].prefix, "▏");
        assert_eq!(lines[3].prefix, "▏  ");
        assert_eq!(lines[4].kind, LineKind::Mark("page break".into()));
        assert_eq!(lines[5].prefix, "[1] ");
    }
}
