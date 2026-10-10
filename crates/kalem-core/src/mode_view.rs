//! How each mode shows a document (roadmap R3.2): its lines, its blocks
//! and its outline, behind one trait, so the editors draw any mode the
//! same way instead of each choosing the mode's functions itself. The
//! mode contract ([`crate::modes::ModeSpec`]) works on text, for plugins
//! as for built-in modes; these work on the open document, whose caches
//! (the parse, the LaTeX model, the CSV layout) they read.

use std::ops::Range;

use crate::DocumentMode;
use crate::document::DocumentState;
use crate::view::{self, Block, LineView, OutlineItem};

/// What a mode shows of a document.
pub trait ModeView: Send + Sync {
    /// The line at `range` as it reads, the cursor at `cursor`.
    /// `paragraph`: the source lines shown as one paragraph that start on
    /// this line, when the mode joins lines (LaTeX away from the cursor).
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        paragraph: Option<Range<usize>>,
    ) -> LineView;

    /// The blocks folding and the visible lines are made of.
    fn blocks(&self, _doc: &DocumentState) -> Vec<Block> {
        Vec::new()
    }

    /// The outline, `None` when the mode has none.
    fn outline(&self, _doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
        None
    }
}

/// The view of `doc`; with `source`, the text as it is (the source view).
pub fn view_of(doc: &DocumentState, source: bool) -> &'static dyn ModeView {
    match &doc.meta.mode {
        DocumentMode::Flow if doc.flow.is_some() => &Flow,
        DocumentMode::Org if source => &OrgSource,
        DocumentMode::Org => &Org,
        _ if source => &Plain,
        DocumentMode::Latex => &Latex,
        DocumentMode::Markdown => &Markdown,
        DocumentMode::Csv => &Csv,
        _ if crate::bibtex::is_bib(doc) => &Bib,
        _ => &Plain,
    }
}

/// The line at `range` of `doc` as the editors show it: a very long line
/// as plain text around the cursor in every mode, the rest by the mode.
pub fn line_view(
    doc: &DocumentState,
    source: bool,
    range: Range<usize>,
    cursor: Option<usize>,
    paragraph: Option<Range<usize>>,
) -> LineView {
    if range.len() > view::LONG_LINE {
        let mut v = view::plain_line_view(doc.text().as_str(), range, cursor);
        v.mono = doc.meta.mode != DocumentMode::Org;
        return v;
    }
    let mut v = view_of(doc, source).line_view(doc, range, cursor, paragraph);
    // A layer's overlays, on the document as it reads (`crate::layers`).
    if !source && let Some(o) = doc.overlays() {
        crate::layers::apply_line(&mut v, &o, cursor);
    }
    v
}

/// The blocks of `doc` (folding works on them whichever view shows it),
/// cut where a layer hides or folds lines.
pub fn blocks(doc: &DocumentState) -> Vec<Block> {
    let blocks = view_of(doc, false).blocks(doc);
    match doc.overlays() {
        Some(o) if !o.lines.is_empty() => {
            // A mode that gives no blocks shows every line; one block of
            // the whole text, to be cut.
            let blocks = if blocks.is_empty() {
                let len = doc.text().len();
                vec![Block {
                    kind: crate::view::BlockKind::Paragraph,
                    range: 0..len,
                    content_end: len,
                    depth: 0,
                    headline: None,
                }]
            } else {
                blocks
            };
            crate::layers::apply_blocks(blocks, &o)
        }
        _ => blocks,
    }
}

/// What the blocks and the views of a document depend on: its text's
/// version and the layers' generation.
pub type ViewKey = (u64, u64);

/// The [`ViewKey`] of `doc`. The editors keep its blocks by it.
pub fn view_key(doc: &DocumentState) -> ViewKey {
    (doc.version(), crate::layers::generation())
}

/// The outline of `doc`: its mode's, or else its language pack's or its
/// viewer's; `None` when there is none yet (a parse still running).
pub fn outline_items(doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
    view_of(doc, false)
        .outline(doc)
        .or_else(|| crate::packs::outline_items(doc))
        .or_else(|| crate::viewer::outline_items(doc))
}

/// The language the highlighter colors `doc` as: a code file's, and the
/// source of Markdown and LaTeX.
pub fn highlight_language(doc: &DocumentState) -> Option<&str> {
    match &doc.meta.mode {
        DocumentMode::Text { language: Some(l) } => Some(l.as_str()),
        DocumentMode::Markdown => Some("md"),
        DocumentMode::Latex => Some("latex"),
        _ => None,
    }
}

/// The command a Ctrl-click (Cmd-click) on a link runs in `doc`.
pub fn open_link_command(doc: &DocumentState) -> &'static str {
    match doc.meta.mode {
        DocumentMode::Latex => "latex.link.open",
        DocumentMode::Markdown => "markdown.openLink",
        _ => "org.link.open",
    }
}

/// The command a click on a checkbox runs in `doc`.
pub fn checkbox_command(doc: &DocumentState) -> &'static str {
    match doc.meta.mode {
        DocumentMode::Markdown => "markdown.toggleCheckbox",
        _ => "list.toggleCheckbox",
    }
}

/// A document of flowing text (`crate::flow`): its paragraphs as the
/// plugin gave them, in both the view and the source view (its text is
/// not a source of its own).
struct Flow;

impl ModeView for Flow {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        _: Option<Range<usize>>,
    ) -> LineView {
        match doc.flow.as_deref() {
            Some(f) => crate::flow::line_view(f, range),
            None => view::plain_line_view(doc.text().as_str(), range, cursor),
        }
    }

    fn outline(&self, doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
        doc.flow.as_deref().map(crate::flow::FlowState::outline)
    }

    /// The horizontal rules, drawn across the line away from the cursor
    /// as Org's are.
    fn blocks(&self, doc: &DocumentState) -> Vec<Block> {
        let Some(f) = doc.flow.as_deref() else {
            return Vec::new();
        };
        f.rule_lines()
            .into_iter()
            .map(|range| Block {
                kind: crate::view::BlockKind::Rule,
                content_end: range.end,
                range,
                depth: 0,
                headline: None,
            })
            .collect()
    }
}

/// Plain text, and every mode's source view but Org's: the text as it is,
/// monospace, LaTeX's diagnostics flagged.
struct Plain;

impl ModeView for Plain {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        _: Option<Range<usize>>,
    ) -> LineView {
        let mut v = view::plain_line_view(doc.text().as_str(), range, cursor);
        v.mono = doc.meta.mode != DocumentMode::Org;
        // LaTeX's checks; a language server's problems in code files.
        crate::latex_view::flag_diagnostics(doc, &mut v);
        crate::lsp::flag_diagnostics(doc, &mut v);
        v
    }

    fn blocks(&self, doc: &DocumentState) -> Vec<Block> {
        // LaTeX's source view still folds by its blocks.
        if doc.latex().is_some() {
            crate::latex_view::blocks(doc)
        } else {
            Vec::new()
        }
    }
}

/// Org as it reads.
struct Org;

impl ModeView for Org {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        p: Option<Range<usize>>,
    ) -> LineView {
        match doc.parse() {
            Some((parse, true)) => {
                let text = doc.text();
                let table = text.as_str()[range.clone()].trim_start().starts_with('|');
                view::line_view_with(&parse.syntax(), parse.context(), range, cursor, table)
            }
            // While a full parse runs: the text as it is.
            _ => Plain.line_view(doc, range, cursor, p),
        }
    }

    fn blocks(&self, doc: &DocumentState) -> Vec<Block> {
        match doc.parse() {
            Some((p, true)) => view::blocks(&p.syntax(), p.context()),
            _ => Vec::new(),
        }
    }

    fn outline(&self, doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
        doc.model().map(|m| view::outline_items(&m))
    }
}

/// Org's source view: the text as it is, with Org's highlighting.
struct OrgSource;

impl ModeView for OrgSource {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        p: Option<Range<usize>>,
    ) -> LineView {
        match doc.parse() {
            Some((parse, true)) => {
                view::source_line_view(&parse.syntax(), parse.context(), doc.text().as_str(), range)
            }
            _ => Plain.line_view(doc, range, cursor, p),
        }
    }

    fn blocks(&self, doc: &DocumentState) -> Vec<Block> {
        Org.blocks(doc)
    }

    fn outline(&self, doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
        Org.outline(doc)
    }
}

/// LaTeX as the document reads: a paragraph over several lines as one.
struct Latex;

impl ModeView for Latex {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        paragraph: Option<Range<usize>>,
    ) -> LineView {
        match paragraph {
            Some(p) => crate::latex_view::paragraph_view(doc, p, cursor),
            None => crate::latex_view::line_view(doc, range, cursor),
        }
    }

    fn blocks(&self, doc: &DocumentState) -> Vec<Block> {
        crate::latex_view::blocks(doc)
    }

    fn outline(&self, doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
        crate::latex_view::outline_items(doc)
    }
}

/// Markdown as it reads, its markers hidden away from the cursor.
struct Markdown;

impl ModeView for Markdown {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        _: Option<Range<usize>>,
    ) -> LineView {
        crate::markdown::line_view(doc, range, cursor)
    }

    fn blocks(&self, doc: &DocumentState) -> Vec<Block> {
        crate::markdown::blocks(doc)
    }

    fn outline(&self, doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
        Some(crate::markdown::outline_items(doc))
    }
}

/// A CSV row as a row of the grid.
struct Csv;

impl ModeView for Csv {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        _: Option<Range<usize>>,
    ) -> LineView {
        let layout = crate::csv::layout(doc);
        crate::csv::line_view(&layout, doc.text().as_str(), range, cursor)
    }
}

/// A BibTeX entry as a row of the grid.
struct Bib;

impl ModeView for Bib {
    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        _: Option<Range<usize>>,
    ) -> LineView {
        crate::bibtex::line_view(doc, range, cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(name: &str, text: &str) -> DocumentState {
        let meta = crate::Metadata {
            path: None,
            mode: DocumentMode::detect(Some(std::path::Path::new(name)), text.as_bytes()),
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        )
    }

    #[test]
    fn each_mode_by_its_view() {
        // Org hides its markup away from the cursor; its source view not.
        let mut d = doc("a.org", "* Head\nSome *bold* text.\n");
        let r = d.text().line_range(1);
        let read = line_view(&d, false, r.clone(), None, None);
        let src = line_view(&d, true, r.clone(), None, None);
        let shown = |v: &LineView| v.runs.iter().map(|r| r.text.as_str()).collect::<String>();
        assert_eq!(shown(&read), "Some bold text.");
        assert_eq!(shown(&src), "Some *bold* text.");
        assert!(!blocks(&d).is_empty());
        assert_eq!(outline_items(&mut d).map(|o| o.len()), Some(1));
        // Markdown likewise.
        let mut d = doc("a.md", "# Head\nSome **bold** text.\n");
        let r = d.text().line_range(1);
        assert_eq!(
            shown(&line_view(&d, false, r, None, None)),
            "Some bold text."
        );
        assert_eq!(outline_items(&mut d).map(|o| o.len()), Some(1));
        // Plain text as it is, monospace.
        let d = doc("a.txt", "plain *text*\n");
        let v = line_view(&d, false, d.text().line_range(0), None, None);
        assert!(v.mono && shown(&v) == "plain *text*");
    }

    #[test]
    fn a_long_line_is_plain_in_every_mode() {
        let long = format!("Some *{}* text\n", "x".repeat(view::LONG_LINE));
        let d = doc("a.md", &long);
        let v = line_view(&d, false, d.text().line_range(0), Some(0), None);
        assert!(v.mono);
    }
}
