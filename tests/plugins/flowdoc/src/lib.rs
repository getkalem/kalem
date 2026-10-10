//! A fake document of flowing text of the Rust contract, exported through
//! the adapter with the feature `flow` (API 0.2.7). A `.flow` file is a
//! paragraph a line: `# ` a heading, `- ` a list item, `|a|b|` a table's
//! row of one-paragraph cells, `---` a horizontal rule, `*word*` bold.
//! Its last paragraph is a footnote that the first one's `^` marks.
//! Comments and tracking live in the document, not the file.

use kalem_viewer::{
    Anchor, Annotation, AnnotationKind, Aside, AsideKind, Detection, FileHandle, FlowAlign,
    FlowCell, FlowItem, FlowLayout, FlowParagraph, FlowPlace, FlowRole, FlowRow, FlowRun,
    FlowStyle, FlowStyleKind, FlowTable, ListKind, MarkChange, Marks, ParagraphChange, Piece,
    RenderRequest, Rendered, Result, Structure, Unit, UnitKind, Viewer, ViewerDocument,
    ViewerError,
};

pub struct Flows;

#[derive(Debug, Clone, Default)]
struct State {
    lines: Vec<String>,
    comments: Vec<Annotation>,
    /// Formatting, coarse: every run of a paragraph a range touches.
    looks: Vec<(u32, MarkChange)>,
    /// Paragraph styles given, by paragraph.
    styles: Vec<(u32, String)>,
    /// Paragraphs' look and lists changed, by paragraph.
    paras: Vec<(u32, ParagraphChange)>,
}

/// A paragraph change made on a paragraph.
fn apply_paragraph(p: &mut FlowParagraph, c: &ParagraphChange) {
    match c {
        ParagraphChange::Align(a) => p.align = *a,
        ParagraphChange::IndentStart(v) => p.indent.0 = *v,
        ParagraphChange::IndentEnd(v) => p.indent.1 = *v,
        ParagraphChange::FirstLine(v) => p.indent.2 = *v,
        ParagraphChange::SpaceBefore(v) => p.spacing.0 = *v,
        ParagraphChange::SpaceAfter(v) => p.spacing.1 = *v,
        ParagraphChange::LineSpacing(_) => {}
        ParagraphChange::List(Some(kind)) => {
            p.role = FlowRole::ListItem;
            p.level = p.level.max(1);
            let label = match kind {
                ListKind::Bullet => "•",
                ListKind::Numbered(_) => "1.",
            };
            p.label = Some((label.to_string(), Marks::default()));
        }
        ParagraphChange::List(None) => {
            p.role = FlowRole::Body;
            p.level = 0;
            p.label = None;
        }
        ParagraphChange::ListLevel(l) => {
            if p.role == FlowRole::ListItem {
                p.level = l + 1;
            }
        }
        ParagraphChange::Clear => {
            p.align = FlowAlign::Start;
            p.indent = (0.0, 0.0, 0.0);
            p.spacing = (0.0, 0.0);
        }
    }
}

/// A mark change made on a run's marks.
fn apply_mark(m: &mut Marks, c: &MarkChange) {
    match c {
        MarkChange::Bold(b) => m.bold = *b,
        MarkChange::Italic(b) => m.italic = *b,
        MarkChange::Underline(u) => m.underline = u.clone(),
        MarkChange::Strike(b) => m.strike = *b,
        MarkChange::Script(s) => m.script = *s,
        MarkChange::Color(c) => m.color = *c,
        MarkChange::Highlight(h) => m.highlight = *h,
        MarkChange::Size(s) => m.size = *s,
        MarkChange::Face(f) => m.face = f.clone(),
        MarkChange::Clear => *m = Marks::default(),
    }
}

#[derive(Default)]
pub struct Doc {
    state: State,
    saved: Vec<String>,
    /// The formatting and styles given when it was saved.
    saved_looks: (usize, usize),
    undo: Vec<State>,
    redo: Vec<State>,
    batch: Option<usize>,
    version: u64,
    tracking: bool,
    author: String,
}

impl Viewer for Flows {
    fn id(&self) -> &str {
        "flowdoc"
    }

    fn name(&self) -> &str {
        "Flow document"
    }

    fn extensions(&self) -> &[&str] {
        &["flow"]
    }

    fn detect(&self, name: &str, _head: &[u8]) -> Detection {
        if name.ends_with(".flow") {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        let text = String::from_utf8(file.read_all()?).map_err(|e| ViewerError(e.to_string()))?;
        // A line of a NUL alone makes a test's file binary, so that Kalem
        // asks its viewers for it: not a paragraph.
        let lines: Vec<String> = text
            .lines()
            .filter(|l| *l != "\0")
            .map(str::to_string)
            .collect();
        Ok(Box::new(Doc {
            saved: lines.clone(),
            state: State {
                lines,
                ..State::default()
            },
            author: "Someone".into(),
            ..Doc::default()
        }))
    }
}

/// A line's runs: `*bold*` bold, `^` a note's mark, the rest text; the
/// line's edit text, `^` as the object character.
fn runs(line: &str, base: usize, comments: &[Annotation], index: u32) -> (String, Vec<FlowRun>) {
    let mut text = String::new();
    let mut out = Vec::new();
    let mut bold = false;
    let body = &line[base..];
    let mut cur = String::new();
    let flush = |cur: &mut String, text: &mut String, out: &mut Vec<FlowRun>, bold: bool| {
        if cur.is_empty() {
            return;
        }
        let start = text.len() as u32;
        text.push_str(cur);
        let end = text.len() as u32;
        let annotations = comments
            .iter()
            .filter(|c| {
                c.anchors.iter().any(|a| {
                    matches!(a, Anchor::Flow { from, to, .. }
                        if from.paragraph <= index && to.paragraph >= index)
                })
            })
            .map(|c| c.id.clone())
            .collect();
        out.push(FlowRun {
            text: std::mem::take(cur),
            piece: Piece::Text,
            marks: Marks {
                bold,
                ..Marks::default()
            },
            source: start..end,
            annotations,
            ..FlowRun::default()
        });
    };
    for c in body.chars() {
        match c {
            '*' => {
                flush(&mut cur, &mut text, &mut out, bold);
                bold = !bold;
            }
            '^' => {
                flush(&mut cur, &mut text, &mut out, bold);
                let start = text.len() as u32;
                text.push(kalem_viewer::OBJECT);
                out.push(FlowRun {
                    text: "1".into(),
                    piece: Piece::NoteMark("n1".into()),
                    marks: Marks {
                        script: kalem_viewer::Script::Superscript,
                        ..Marks::default()
                    },
                    source: start..text.len() as u32,
                    locked: Some("a note's mark".into()),
                    ..FlowRun::default()
                });
            }
            c => cur.push(c),
        }
    }
    flush(&mut cur, &mut text, &mut out, bold);
    (text, out)
}

impl Doc {
    /// The lines that are paragraphs (not rules, not tables): their line
    /// numbers, in order; a paragraph's index is its place here.
    fn paragraphs(&self) -> Vec<usize> {
        (0..self.state.lines.len())
            .filter(|&i| {
                let l = &self.state.lines[i];
                l != "---" && !l.starts_with('|')
            })
            .collect()
    }

    fn items(&self) -> Vec<FlowItem> {
        let paras = self.paragraphs();
        let last = paras.last().copied();
        let mut out = Vec::new();
        for (i, line) in self.state.lines.iter().enumerate() {
            if line == "---" {
                out.push(FlowItem::Rule("line".into()));
                continue;
            }
            if let Some(row) = line.strip_prefix('|') {
                let cells: Vec<&str> = row.trim_end_matches('|').split('|').collect();
                out.push(FlowItem::TableStart(FlowTable {
                    columns: vec![72.0; cells.len()],
                    style: "Grid".into(),
                }));
                out.push(FlowItem::RowStart(FlowRow { header: false }));
                for c in cells {
                    out.push(FlowItem::CellStart(FlowCell {
                        columns: 1,
                        ..FlowCell::default()
                    }));
                    out.push(FlowItem::Paragraph(FlowParagraph {
                        text: c.into(),
                        runs: vec![FlowRun {
                            text: c.into(),
                            source: 0..c.len() as u32,
                            ..FlowRun::default()
                        }],
                        ..FlowParagraph::default()
                    }));
                    out.push(FlowItem::CellEnd);
                }
                out.push(FlowItem::RowEnd);
                out.push(FlowItem::TableEnd);
                continue;
            }
            let index = paras.iter().position(|&p| p == i).map(|p| p as u32);
            let (role, level, base, style, label) = if line.starts_with("# ") {
                (FlowRole::Heading, 1, 2, "Heading 1", None)
            } else if line.starts_with("- ") {
                (
                    FlowRole::ListItem,
                    1,
                    2,
                    "List",
                    Some(("•".to_string(), Marks::default())),
                )
            } else {
                (FlowRole::Body, 0, 0, "Normal", None)
            };
            let (text, mut runs) = runs(line, base, &self.state.comments, index.unwrap_or(0));
            let mut style = style.to_string();
            let (mut role, mut level) = (role, level);
            if let Some(ix) = index {
                for (_, c) in self.state.looks.iter().filter(|(p, _)| *p == ix) {
                    for r in runs.iter_mut().filter(|r| r.piece == Piece::Text) {
                        apply_mark(&mut r.marks, c);
                    }
                }
                if let Some((_, s)) = self.state.styles.iter().rev().find(|(p, _)| *p == ix) {
                    style = s.clone();
                    (role, level) = if s == "Heading 1" {
                        (FlowRole::Heading, 1)
                    } else {
                        (FlowRole::Body, 0)
                    };
                }
            }
            let mut p = FlowParagraph {
                index,
                role,
                level,
                style,
                label,
                text,
                runs,
                ..FlowParagraph::default()
            };
            if let Some(ix) = index {
                for (_, c) in self.state.paras.iter().filter(|(q, _)| *q == ix) {
                    apply_paragraph(&mut p, c);
                }
            }
            if Some(i) == last && i > 0 {
                out.push(FlowItem::AsideStart(Aside {
                    kind: AsideKind::Footnote,
                    id: "n1".into(),
                    label: "1".into(),
                }));
                out.push(FlowItem::Paragraph(p));
                out.push(FlowItem::AsideEnd);
            } else {
                out.push(FlowItem::Paragraph(p));
            }
        }
        out
    }

    /// The line of paragraph `index`, and where its edit text starts in
    /// it.
    fn line(&self, index: u32) -> Result<(usize, usize)> {
        let i = *self
            .paragraphs()
            .get(index as usize)
            .ok_or_else(|| ViewerError(format!("no paragraph {index}")))?;
        let l = &self.state.lines[i];
        let base = if l.starts_with("# ") || l.starts_with("- ") {
            2
        } else {
            0
        };
        Ok((i, base))
    }

    /// A paragraph's line with its edit text replaced (bold marks are
    /// dropped by an edit: the fake keeps text only).
    fn set_text(&mut self, index: u32, text: &str) -> Result<()> {
        let (i, base) = self.line(index)?;
        let head = self.state.lines[i][..base].to_string();
        self.state.lines[i] = format!("{head}{}", text.replace(kalem_viewer::OBJECT, "^"));
        Ok(())
    }

    fn text_of(&self, index: u32) -> Result<String> {
        let (i, base) = self.line(index)?;
        Ok(runs(&self.state.lines[i], base, &[], index).0)
    }

    fn step(&mut self) {
        if self.batch.is_none() {
            self.undo.push(self.state.clone());
            self.redo.clear();
        }
        self.version += 1;
    }
}

fn byte(text: &str, at: u32) -> Result<usize> {
    let at = at as usize;
    if at > text.len() || !text.is_char_boundary(at) {
        return Err(ViewerError(format!("{at} is not a place in the paragraph")));
    }
    Ok(at)
}

impl ViewerDocument for Doc {
    fn structure(&self) -> Structure {
        Structure {
            units: vec![Unit {
                kind: UnitKind::Page,
                label: "Body".into(),
                duration_ms: None,
            }],
            outline: Vec::new(),
        }
    }

    fn render(&mut self, _unit: usize, _request: RenderRequest) -> Result<Rendered> {
        Err(ViewerError("a flow is laid out by the host".into()))
    }

    fn text(&self, _unit: usize) -> String {
        self.state.lines.join("\n")
    }

    fn modified(&self) -> bool {
        self.state.lines != self.saved
            || (
                self.state.looks.len(),
                self.state.styles.len() + self.state.paras.len(),
            ) != self.saved_looks
    }

    fn save(&mut self) -> Result<kalem_viewer::SaveOutput> {
        self.saved = self.state.lines.clone();
        self.saved_looks = (
            self.state.looks.len(),
            self.state.styles.len() + self.state.paras.len(),
        );
        Ok(kalem_viewer::SaveOutput {
            bytes: (self.state.lines.join("\n") + "\n").into_bytes(),
            losses: Vec::new(),
        })
    }

    fn flow(&mut self, unit: usize) -> Option<FlowLayout> {
        (unit == 0).then(|| FlowLayout {
            items: self.items().len() as u32,
            version: self.version,
            editable: true,
        })
    }

    fn flow_items(&mut self, _unit: usize, from: u32, count: u32) -> Vec<FlowItem> {
        self.items()
            .into_iter()
            .skip(from as usize)
            .take(count as usize)
            .collect()
    }

    fn flow_replace(
        &mut self,
        _unit: usize,
        paragraph: u32,
        range: std::ops::Range<u32>,
        text: &str,
    ) -> Result<()> {
        let t = self.text_of(paragraph)?;
        let (a, b) = (byte(&t, range.start)?, byte(&t, range.end)?);
        if t[a..b].contains(kalem_viewer::OBJECT) {
            return Err(ViewerError("a note's mark is not deleted".into()));
        }
        if self.tracking {
            return Err(ViewerError(
                "tracked edits are not written by the fake".into(),
            ));
        }
        self.step();
        self.set_text(paragraph, &format!("{}{text}{}", &t[..a], &t[b..]))
    }

    fn flow_split(&mut self, _unit: usize, at: FlowPlace) -> Result<()> {
        let t = self.text_of(at.paragraph)?;
        let a = byte(&t, at.offset)?;
        self.step();
        let (i, _) = self.line(at.paragraph)?;
        self.set_text(at.paragraph, &t[..a])?;
        self.state.lines.insert(i + 1, t[a..].to_string());
        Ok(())
    }

    fn flow_join(&mut self, _unit: usize, paragraph: u32) -> Result<()> {
        let (i, _) = self.line(paragraph)?;
        let (j, base) = self.line(paragraph + 1)?;
        if j != i + 1 {
            return Err(ViewerError("not beside each other".into()));
        }
        self.step();
        let next = self.state.lines.remove(j)[base..].to_string();
        self.state.lines[i].push_str(&next);
        Ok(())
    }

    fn flow_delete(&mut self, unit: usize, from: FlowPlace, to: FlowPlace) -> Result<()> {
        if from.paragraph == to.paragraph {
            return self.flow_replace(unit, from.paragraph, from.offset..to.offset, "");
        }
        self.begin_batch();
        self.step();
        let tail = self.text_of(to.paragraph)?;
        let b = byte(&tail, to.offset)?;
        let head = self.text_of(from.paragraph)?;
        let a = byte(&head, from.offset)?;
        let (i, _) = self.line(from.paragraph)?;
        let (j, _) = self.line(to.paragraph)?;
        self.set_text(from.paragraph, &format!("{}{}", &head[..a], &tail[b..]))?;
        self.state.lines.drain(i + 1..=j);
        self.end_batch();
        Ok(())
    }

    fn flow_styles(&mut self) -> Vec<FlowStyle> {
        let style = |name: &str, kind, shown| FlowStyle {
            id: name.into(),
            name: name.into(),
            kind,
            shown,
        };
        vec![
            style("Quote", FlowStyleKind::Paragraph, true),
            style("Heading 1", FlowStyleKind::Paragraph, true),
            style("Normal", FlowStyleKind::Paragraph, true),
            style("Strong", FlowStyleKind::Character, true),
            style("Hidden", FlowStyleKind::Paragraph, false),
        ]
    }

    fn flow_set_marks(
        &mut self,
        _unit: usize,
        from: FlowPlace,
        to: FlowPlace,
        changes: &[MarkChange],
    ) -> Result<()> {
        self.step();
        for p in from.paragraph..=to.paragraph {
            for c in changes {
                self.state.looks.push((p, c.clone()));
            }
        }
        Ok(())
    }

    fn flow_set_paragraphs(
        &mut self,
        _unit: usize,
        from: u32,
        to: u32,
        changes: &[ParagraphChange],
    ) -> Result<()> {
        let unknown = |c: &ParagraphChange| matches!(c, ParagraphChange::List(Some(ListKind::Numbered(f))) if f == "hebrew");
        if changes.iter().any(unknown) {
            return Err(ViewerError("no hebrew numbering".into()));
        }
        self.step();
        for p in from..=to {
            for c in changes {
                self.state.paras.push((p, c.clone()));
            }
        }
        Ok(())
    }

    fn flow_set_style(&mut self, _unit: usize, from: u32, to: u32, style: &str) -> Result<()> {
        if !self.flow_styles().iter().any(|s| s.id == style) {
            return Err(ViewerError(format!("no style {style}")));
        }
        self.step();
        for p in from..=to {
            self.state.styles.push((p, style.to_string()));
        }
        Ok(())
    }

    fn has_history(&self) -> bool {
        true
    }

    fn undo(&mut self) -> Result<bool> {
        let Some(s) = self.undo.pop() else {
            return Ok(false);
        };
        self.redo.push(std::mem::replace(&mut self.state, s));
        self.version += 1;
        Ok(true)
    }

    fn redo(&mut self) -> Result<bool> {
        let Some(s) = self.redo.pop() else {
            return Ok(false);
        };
        self.undo.push(std::mem::replace(&mut self.state, s));
        self.version += 1;
        Ok(true)
    }

    fn begin_batch(&mut self) {
        if self.batch.is_none() {
            self.undo.push(self.state.clone());
            self.redo.clear();
            self.batch = Some(self.undo.len());
        }
    }

    fn end_batch(&mut self) {
        self.batch = None;
    }

    fn annotations(&mut self, _unit: Option<usize>) -> Vec<Annotation> {
        self.state.comments.clone()
    }

    fn set_author(&mut self, name: &str) {
        self.author = name.into();
    }

    fn comment(&mut self, on: Anchor, text: &str) -> Result<String> {
        self.step();
        let id = format!("c{}", self.state.comments.len() + 1);
        self.state.comments.push(Annotation {
            id: id.clone(),
            kind: AnnotationKind::Comment,
            author: self.author.clone(),
            date: Some("2026-10-09T12:00:00Z".into()),
            text: text.into(),
            anchors: vec![on],
            ..Annotation::default()
        });
        Ok(id)
    }

    fn reply(&mut self, parent: &str, text: &str) -> Result<String> {
        if !self.state.comments.iter().any(|c| c.id == parent) {
            return Err(ViewerError(format!("no comment {parent}")));
        }
        self.step();
        let id = format!("c{}", self.state.comments.len() + 1);
        self.state.comments.push(Annotation {
            id: id.clone(),
            author: self.author.clone(),
            text: text.into(),
            parent: Some(parent.into()),
            ..Annotation::default()
        });
        Ok(id)
    }

    fn set_comment_text(&mut self, id: &str, text: &str) -> Result<()> {
        self.step();
        let c = self
            .state
            .comments
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| ViewerError(format!("no comment {id}")))?;
        c.text = text.into();
        Ok(())
    }

    fn resolve(&mut self, id: &str, done: bool) -> Result<()> {
        self.step();
        let c = self
            .state
            .comments
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| ViewerError(format!("no comment {id}")))?;
        c.resolved = done;
        Ok(())
    }

    fn remove_comment(&mut self, id: &str) -> Result<()> {
        self.step();
        self.state
            .comments
            .retain(|c| c.id != id && c.parent.as_deref() != Some(id));
        Ok(())
    }

    fn tracking(&mut self) -> Option<bool> {
        Some(self.tracking)
    }

    fn set_tracking(&mut self, on: bool) -> Result<()> {
        self.tracking = on;
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
kalem_plugin::export_viewer_of!(Flows);
