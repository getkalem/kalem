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

    /// How the editors show and edit its documents beyond the lines
    /// ([`ViewTraits`]); `source` when this view is the source view.
    fn traits(&self, _doc: &DocumentState, _source: bool) -> ViewTraits {
        ViewTraits::DOCUMENT
    }

    /// The lines a filter or a sort shows, in the order shown (a CSV
    /// view's rows, a BibTeX grid's entries); `None` when every line shows
    /// in the file's order, as the blocks let it.
    fn shown_lines(&self, _doc: &DocumentState) -> Option<std::rc::Rc<Vec<usize>>> {
        None
    }

    /// The column the view is sorted by, and whether descending: its
    /// [`ModeView::shown_lines`] are then out of the file's order.
    fn sort(&self, _doc: &DocumentState) -> Option<(usize, bool)> {
        None
    }

    /// The paragraphs set from several source lines that show as one on
    /// their first line away from the cursor (LaTeX's, as TeX sets them),
    /// in order; the editors hide their other lines and pass each to
    /// [`ModeView::line_view`] as its `paragraph`.
    fn joined_paragraphs(&self, _doc: &DocumentState) -> std::sync::Arc<Vec<Range<usize>>> {
        std::sync::Arc::default()
    }

    /// The table starting at `start`, as the editors draw it as a grid
    /// away from the cursor: its rows, cells and columns' alignment.
    fn table_view(&self, _doc: &DocumentState, _start: usize) -> Option<view::TableView> {
        None
    }

    /// The table holding `pos`, from its start to its content's end, for a
    /// mode whose tables are not among its blocks (Markdown's); the others'
    /// are their [`view::BlockKind::Table`] blocks.
    fn table_range(&self, _doc: &DocumentState, _pos: usize) -> Option<Range<usize>> {
        None
    }

    /// The code on the line `line` in a language, to color in its syntax:
    /// each piece's range and language, and for a line of a code block
    /// (the block's range then) its index among the block's lines, whose
    /// state follows from those before it.
    fn code_spans(&self, _doc: &DocumentState, _line: Range<usize>) -> Vec<CodeSpan> {
        Vec::new()
    }

    /// Whether the line `line` is in a code block the mode finds itself
    /// (Markdown's, fences included), drawn on the code background; the
    /// others' are their code blocks ([`view::BlockKind::is_code`]).
    fn in_code_block(&self, _doc: &DocumentState, _line: Range<usize>) -> bool {
        false
    }
}

/// A piece of code to color ([`ModeView::code_spans`]): its range, its
/// language, and for a line of a code block, the line's index in it.
pub type CodeSpan = (Range<usize>, String, Option<usize>);

/// What the editors do with a mode's documents beyond drawing their lines
/// ([`ModeView::traits`], [`traits`]): the things they decided by asking
/// which mode a document is in, which a mode registered beside the
/// built-in ones answers too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewTraits {
    /// The text column is measured in the monospace font's characters
    /// (code, a source view) rather than the proportional font's.
    pub mono: bool,
    /// The language highlighter colors the lines that show their source,
    /// with indentation guides and the current line's band; a mode that
    /// styles its text itself (Org) has none.
    pub highlight: bool,
    /// Line numbers in the gutter, when `editor.line_numbers` is on (a
    /// CSV grid numbers its rows itself, Org as it reads has none).
    pub line_numbers: bool,
    /// Long lines wrap; otherwise the rows scroll sideways (a CSV grid).
    pub wraps: bool,
    /// Tab and Shift+Tab indent; otherwise they are the mode's own keys
    /// (Org's cycling and table fields).
    pub tab_indents: bool,
    /// Lines fold and hide by the mode's blocks ([`ModeView::blocks`]);
    /// otherwise every line shows (in the terminal's editor).
    pub folds: bool,
    /// Headings fold by an arrow in the margin.
    pub heading_folds: bool,
    /// The text is indented by its heading's depth (`org-indent-mode`,
    /// with `editor.outline_indent`).
    pub outline_indent: bool,
    /// The mode colors its code itself (code environments and inline code
    /// in their language, [`ModeView::code_spans`]) and the language
    /// highlighter colors none of its lines (LaTeX as it reads).
    pub own_code_colors: bool,
    /// The language highlighter colors its lines where they show their
    /// source; otherwise only a line shown wholly as its source (a BibTeX
    /// grid's rows).
    pub source_colors: bool,
}

impl ViewTraits {
    /// A document as a new mode's are by default: prose in the
    /// proportional font, wrapped, folding by its blocks, its source
    /// lines colored by its language when it has one.
    pub const DOCUMENT: ViewTraits = ViewTraits {
        mono: false,
        highlight: true,
        line_numbers: true,
        wraps: true,
        tab_indents: true,
        folds: true,
        heading_folds: false,
        outline_indent: false,
        own_code_colors: false,
        source_colors: true,
    };
}

/// How the editors show and edit `doc` beyond its lines in the view
/// `source` picks ([`view_of`]).
pub fn traits(doc: &DocumentState, source: bool) -> ViewTraits {
    view_of(doc, source).traits(doc, source)
}

/// The lines of `doc` a filter or a sort shows, in their order
/// ([`ModeView::shown_lines`]); `None` in the source view.
pub fn shown_lines(doc: &DocumentState, source: bool) -> Option<std::rc::Rc<Vec<usize>>> {
    view_of(doc, source).shown_lines(doc)
}

/// The column `doc`'s view is sorted by ([`ModeView::sort`]).
pub fn sort(doc: &DocumentState, source: bool) -> Option<(usize, bool)> {
    view_of(doc, source).sort(doc)
}

/// The table of `doc` starting at `start` ([`ModeView::table_view`]).
pub fn table_view(doc: &DocumentState, source: bool, start: usize) -> Option<view::TableView> {
    view_of(doc, source).table_view(doc, start)
}

/// The table of `doc` holding `pos` when its mode's tables are not blocks
/// ([`ModeView::table_range`]).
pub fn table_range(doc: &DocumentState, source: bool, pos: usize) -> Option<Range<usize>> {
    view_of(doc, source).table_range(doc, pos)
}

/// The code on the line `line` of `doc` ([`ModeView::code_spans`]).
pub fn code_spans(doc: &DocumentState, source: bool, line: Range<usize>) -> Vec<CodeSpan> {
    view_of(doc, source).code_spans(doc, line)
}

/// Whether the line `line` of `doc` is in a code block its mode finds
/// itself ([`ModeView::in_code_block`]).
pub fn in_code_block(doc: &DocumentState, source: bool, line: Range<usize>) -> bool {
    view_of(doc, source).in_code_block(doc, line)
}

/// The paragraphs of `doc` shown as one away from the cursor
/// ([`ModeView::joined_paragraphs`]); none in the source view.
pub fn joined_paragraphs(doc: &DocumentState, source: bool) -> std::sync::Arc<Vec<Range<usize>>> {
    view_of(doc, source).joined_paragraphs(doc)
}

/// The built-in modes (`crate::modes::ModeEntry`), found by
/// `crate::modes::entry` before the registered ones. A mode without an
/// entry (plain text and code, the file manager, a viewer's file) is drawn
/// as plain text.
pub static BUILTIN: &[crate::modes::ModeEntry] = &[
    crate::modes::ModeEntry {
        id: "org",
        spec: Some(&crate::org_mode::OrgMode),
        view: &Org,
        source_view: Some(&OrgSource),
        state: None,
        settle: None,
    },
    crate::modes::ModeEntry {
        id: "markdown",
        spec: Some(&crate::markdown::MarkdownMode),
        view: &Markdown,
        source_view: None,
        state: None,
        settle: None,
    },
    crate::modes::ModeEntry {
        id: "csv",
        spec: Some(&crate::modes::CsvMode),
        view: &Csv,
        source_view: None,
        state: Some(|_| Box::<crate::csv::CsvState>::default()),
        settle: Some(crate::csv::settle),
    },
    crate::modes::ModeEntry {
        id: "latex",
        spec: Some(&crate::latex_mode::LatexMode),
        view: &Latex,
        source_view: None,
        state: Some(|cx| {
            Box::new(crate::latex_view::LatexState::for_document(
                cx.text, cx.path,
            ))
        }),
        settle: None,
    },
    crate::modes::ModeEntry {
        id: "bib",
        spec: None,
        view: &Bib,
        source_view: None,
        state: Some(|_| Box::<crate::bibtex::BibState>::default()),
        settle: None,
    },
    // A document of flowing text has one view, its plugin's.
    crate::modes::ModeEntry {
        id: "flow",
        spec: None,
        view: &Flow,
        source_view: Some(&Flow),
        state: None,
        settle: None,
    },
];

/// The view of `doc`; with `source`, the text as it is (the source view):
/// its mode's entry's (`crate::modes::entry`), plain text without one.
pub fn view_of(doc: &DocumentState, source: bool) -> &'static dyn ModeView {
    // A document of flowing text whose plugin's document did not open
    // shows its text as it is.
    if doc.meta.mode == DocumentMode::Flow && doc.flow().is_none() {
        return &Plain;
    }
    match crate::modes::entry_of(&doc.meta.mode) {
        Some(e) if !source => e.view,
        Some(crate::modes::ModeEntry {
            source_view: Some(v),
            ..
        }) => v,
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
/// version, the layers' generation and the overlays its layer gave
/// ([`DocumentState::layer_revision`]).
pub type ViewKey = (u64, u64, u64);

/// The [`ViewKey`] of `doc`. The editors keep its blocks by it.
pub fn view_key(doc: &DocumentState) -> ViewKey {
    (
        doc.version(),
        crate::layers::generation(),
        doc.layer_revision(),
    )
}

/// The outline of `doc`: its mode's, or else its language pack's, a
/// plugin's (`crate::outlines`) or its viewer's; `None` when there is
/// none yet (a parse still running).
pub fn outline_items(doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
    view_of(doc, false)
        .outline(doc)
        .or_else(|| crate::packs::outline_items(doc))
        .or_else(|| crate::outlines::outline_items(doc))
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

/// The command a Ctrl-click (Cmd-click) on a link runs in `doc`: the
/// command for links of the layer serving it (`crate::layers`), else the
/// mode's.
pub fn open_link_command(doc: &DocumentState) -> String {
    if let Some(open) = crate::layers::layer_for(doc.meta.path.as_deref(), doc.meta.mode.name())
        .and_then(|l| l.open)
    {
        return open;
    }
    match doc.meta.mode {
        DocumentMode::Latex => "latex.link.open",
        DocumentMode::Markdown => "markdown.openLink",
        _ => "org.link.open",
    }
    .to_string()
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
        match doc.flow() {
            Some(f) => crate::flow::line_view(f, range),
            None => view::plain_line_view(doc.text().as_str(), range, cursor),
        }
    }

    fn outline(&self, doc: &mut DocumentState) -> Option<Vec<OutlineItem>> {
        doc.flow().map(crate::flow::FlowState::outline)
    }

    /// Its one view serves as its source view too, monospace there.
    fn traits(&self, _: &DocumentState, source: bool) -> ViewTraits {
        ViewTraits {
            mono: source,
            folds: false,
            ..ViewTraits::DOCUMENT
        }
    }

    /// The horizontal rules, drawn across the line away from the cursor
    /// as Org's are.
    fn blocks(&self, doc: &DocumentState) -> Vec<Block> {
        let Some(f) = doc.flow() else {
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
    /// Code and plain text, and the source view of every mode but Org:
    /// monospace, every line shown.
    fn traits(&self, doc: &DocumentState, source: bool) -> ViewTraits {
        ViewTraits {
            mono: source || matches!(doc.meta.mode, DocumentMode::Text { .. }),
            folds: false,
            ..ViewTraits::DOCUMENT
        }
    }

    fn line_view(
        &self,
        doc: &DocumentState,
        range: Range<usize>,
        cursor: Option<usize>,
        _: Option<Range<usize>>,
    ) -> LineView {
        let mut v = view::plain_line_view(doc.text().as_str(), range, cursor);
        v.mono = doc.meta.mode != DocumentMode::Org;
        // LaTeX's checks; a language server's problems and inlay hints in
        // code files.
        crate::latex_view::flag_diagnostics(doc, &mut v);
        crate::lsp::flag_diagnostics(doc, &mut v);
        crate::lsp::inlay_hints(doc, &mut v);
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
    /// Once the parse is current.
    fn table_view(&self, doc: &DocumentState, start: usize) -> Option<view::TableView> {
        let (p, current) = doc.parse()?;
        if !current {
            return None;
        }
        view::table_view(&p.syntax(), p.context(), start, None)
    }

    /// Org styles its text itself: no highlighter, no line numbers; Tab
    /// is its own key; headings fold and indent the text under them,
    /// once the parse is current.
    fn traits(&self, doc: &DocumentState, _: bool) -> ViewTraits {
        ViewTraits {
            highlight: false,
            line_numbers: false,
            tab_indents: false,
            folds: doc.parse().is_some_and(|(_, current)| current),
            heading_folds: true,
            outline_indent: true,
            ..ViewTraits::DOCUMENT
        }
    }

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
            Some((p, true)) => view::blocks(&p.syntax(), p.context(), doc.text().as_str()),
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
    /// Org's source: its own colors, line numbers, every line shown; Tab
    /// is still Org's.
    fn traits(&self, _: &DocumentState, _: bool) -> ViewTraits {
        ViewTraits {
            highlight: false,
            tab_indents: false,
            folds: false,
            ..ViewTraits::DOCUMENT
        }
    }

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
    /// Its code in its language, its own way.
    fn traits(&self, _: &DocumentState, _: bool) -> ViewTraits {
        ViewTraits {
            own_code_colors: true,
            ..ViewTraits::DOCUMENT
        }
    }

    /// `\lstinline` with a language.
    fn code_spans(&self, doc: &DocumentState, line: Range<usize>) -> Vec<CodeSpan> {
        crate::latex_view::inline_code(doc, line)
            .into_iter()
            .map(|(r, l)| (r, l, None))
            .collect()
    }

    fn table_view(&self, doc: &DocumentState, start: usize) -> Option<view::TableView> {
        crate::latex_table::table_view(doc, start)
    }

    fn joined_paragraphs(&self, doc: &DocumentState) -> std::sync::Arc<Vec<Range<usize>>> {
        crate::latex_view::joined_paragraphs(doc)
    }

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
    /// A code block's line, with the block's lines before it.
    fn code_spans(&self, doc: &DocumentState, line: Range<usize>) -> Vec<CodeSpan> {
        crate::markdown::code_block_on_line(doc, line)
            .map(|(b, i, l)| (b, l, Some(i)))
            .into_iter()
            .collect()
    }

    fn in_code_block(&self, doc: &DocumentState, line: Range<usize>) -> bool {
        crate::markdown::in_code_block(doc, line)
    }

    /// None while every marker shows (its tables are then their source).
    fn table_view(&self, doc: &DocumentState, start: usize) -> Option<view::TableView> {
        if view::source_markers() == view::Markers::Always {
            return None;
        }
        crate::markdown::table_view(doc, start)
    }

    fn table_range(&self, doc: &DocumentState, pos: usize) -> Option<Range<usize>> {
        crate::markdown::table_range(doc, pos)
    }

    /// Its headings fold by the margin's arrows.
    fn traits(&self, _: &DocumentState, _: bool) -> ViewTraits {
        ViewTraits {
            heading_folds: true,
            ..ViewTraits::DOCUMENT
        }
    }

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
    fn shown_lines(&self, doc: &DocumentState) -> Option<std::rc::Rc<Vec<usize>>> {
        crate::csv::shown_lines(doc)
    }

    fn sort(&self, doc: &DocumentState) -> Option<(usize, bool)> {
        doc.csv().and_then(|c| c.sort)
    }

    /// The grid numbers its rows itself, and they scroll sideways.
    fn traits(&self, _: &DocumentState, _: bool) -> ViewTraits {
        ViewTraits {
            line_numbers: false,
            wraps: false,
            folds: false,
            ..ViewTraits::DOCUMENT
        }
    }

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
    fn shown_lines(&self, doc: &DocumentState) -> Option<std::rc::Rc<Vec<usize>>> {
        crate::bibtex::shown_lines(doc)
    }

    fn sort(&self, doc: &DocumentState) -> Option<(usize, bool)> {
        doc.bib_sort()
    }

    /// A text file's grid: monospace, every line shown.
    fn traits(&self, _: &DocumentState, _: bool) -> ViewTraits {
        ViewTraits {
            mono: true,
            folds: false,
            source_colors: false,
            ..ViewTraits::DOCUMENT
        }
    }

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
