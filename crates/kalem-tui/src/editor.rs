//! The editor view: the document's visible lines, wrapped into rows,
//! from a top line; the cursor, the selection, scrolling, vertical motion
//! and mouse hits.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use kalem_core::DocumentState;
use kalem_core::view::{self, Block, BlockKind, Folds, TableRow, TableView};
use org_syntax::Parse;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use crate::caps::Caps;
use tui_rich_text::{Drawn, Lines, Options, Viewport};

use crate::render::{self, Glyph, WidgetAt};

/// The editor view's state, kept across frames.
#[derive(Debug, Default)]
pub struct EditorView {
    /// The first shown line and row, and the column vertical motion keeps.
    pub viewport: Viewport,
    /// Folded headlines.
    pub folds: Folds,
    /// The source view: plain text, no folding.
    pub source: bool,
    /// Scroll to the cursor on the next frame.
    pub follow: bool,
    /// Ranges to mark, such as search matches (sorted).
    pub highlights: Vec<Range<usize>>,
    /// The fields the table formula at the cursor refers to (sorted),
    /// marked when there are no search matches.
    pub references: Vec<Range<usize>>,
    /// Vim's block selection, a range per line, painted as selected.
    pub block: Vec<Range<usize>>,
    drawn: Option<Drawn<WidgetAt>>,
    blocks: Option<(u64, Arc<Vec<Block>>)>,
    /// Tables drawn as grids, by their start, for the text version.
    grids: GridCache,
    /// Highlighted source blocks, by their start, for the text version.
    code: CodeCache,
    /// Images.
    pub images: RefCell<Images>,
    /// The model tables of contents are made from.
    toc: TocCache,
    /// The text area of the last frame.
    pub area: Rect,
    /// Focus mode: only the section holding the cursor shows.
    pub focus: bool,
    /// The text column's width in characters; 0 for the whole window.
    pub line_width: u16,
    /// The text column in the middle of the window (`editor.center_text`),
    /// else at its left edge.
    pub center: bool,
    /// Long lines wrap (`editor.soft_wrap`, Alt+Z).
    pub wrap: bool,
    /// Line numbers in plain text files and the source view.
    pub line_numbers: bool,
    /// Formulas shown as their source (`view.toggleMath`).
    pub raw_math: bool,
    /// Text under a heading indented to its title, as Org's
    /// `org-indent-mode` (`editor.outline_indent`, `#+STARTUP: indent`).
    pub outline_indent: bool,
    /// Columns scrolled out at the left when lines do not wrap.
    pub hscroll: u16,
    /// A plain text document's highlighting and indentation step, for its
    /// text version.
    plain: PlainCache,
    /// The highlighting of a file too large for `plain`'s: the lines on
    /// screen, in windows.
    windowed: RefCell<kalem_highlight::Windowed>,
}

/// See [`EditorView`]'s `plain`.
type PlainCache = RefCell<Option<(u64, Option<kalem_highlight::Highlighter>, usize)>>;

/// What an image in the terminal shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ImageKey {
    /// An image file, at most so many cells wide.
    File(std::path::PathBuf, u16),
    /// A formula (a fragment with its delimiters, or an environment),
    /// with the document's macros.
    Math { source: String, macros: String },
}

/// Images shown in the terminal, by file or formula, with their size in
/// cells.
#[derive(Default)]
pub struct Images {
    /// The graphics protocol, if the terminal has one (kitty, iTerm2,
    /// sixel); without one, images show as `[image: …]`.
    pub picker: Option<ratatui_image::picker::Picker>,
    /// The directory relative links start from.
    pub base: Option<std::path::PathBuf>,
    /// Compress kitty transmissions (over SSH, to kitty).
    pub compress: bool,
    /// The colors formulas are drawn in and on: the text and the
    /// terminal's background.
    pub math_colors: ([u8; 3], [u8; 3]),
    cache: HashMap<ImageKey, Option<ImageEntry>>,
    /// The macros of `#+LATEX_HEADER` lines, by document version.
    macros: Option<(u64, String)>,
}

struct ImageEntry {
    protocol: ratatui_image::protocol::StatefulProtocol,
    rows: u16,
    cols: u16,
}

impl std::fmt::Debug for Images {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Images")
            .field("protocol", &self.picker.as_ref().map(|p| p.protocol_type()))
            .field("cached", &self.cache.len())
            .finish()
    }
}

impl Images {
    /// The file an image link points to, shown at most `cols` cells wide.
    fn file(&self, path: &str, cols: u16) -> ImageKey {
        ImageKey::File(
            kalem_core::images::resolve(path, self.base.as_deref()),
            cols,
        )
    }

    /// The cells image `path` may take in a text `text` cells wide: the
    /// width `#+ATTR_ORG: :width` (or LaTeX's `width=`, `height=`, `scale=`)
    /// asks for, in the terminal's cells.
    fn cols(&self, width: Option<view::ImageWidth>, text: u16, path: &str) -> u16 {
        let cell = self
            .picker
            .as_ref()
            .map_or(8.0, |p| p.font_size().width.max(1) as f32);
        match width {
            Some(w) => {
                // A scale or a height needs the picture's own size.
                let natural = matches!(w, view::ImageWidth::Scale(_) | view::ImageWidth::Height(_))
                    .then(|| {
                        let file = kalem_core::images::resolve(path, self.base.as_deref());
                        image::image_dimensions(file).ok()
                    })
                    .flatten();
                let px = w.resolve(text as f32 * cell, natural);
                ((px / cell).round() as u16).clamp(1, text.max(1))
            }
            None => text,
        }
    }

    /// A formula's key, with the definitions of a LaTeX document.
    fn latex_math(&mut self, source: &str, doc: &DocumentState) -> ImageKey {
        let version = doc.version();
        if self.macros.as_ref().is_none_or(|(v, _)| *v != version) {
            // The packages' commands the renderer lacks, and the document's.
            let defs = kalem_core::latex_view::math_definitions(doc);
            self.macros = Some((version, org_math::source::macros(&defs)));
        }
        ImageKey::Math {
            source: source.to_string(),
            macros: self
                .macros
                .as_ref()
                .map(|(_, m)| m.clone())
                .unwrap_or_default(),
        }
    }

    /// A formula's key, with the document's macros.
    fn math(&mut self, source: &str, parse: &Parse, version: u64) -> ImageKey {
        if self.macros.as_ref().is_none_or(|(v, _)| *v != version) {
            let headers: Vec<String> = parse
                .keywords()
                .into_iter()
                .filter(|(k, _)| {
                    k.eq_ignore_ascii_case("LATEX_HEADER")
                        || k.eq_ignore_ascii_case("LATEX_HEADER_EXTRA")
                })
                .map(|(_, v)| v)
                .collect();
            self.macros = Some((version, org_math::source::macros(&headers)));
        }
        ImageKey::Math {
            source: source.to_string(),
            macros: self
                .macros
                .as_ref()
                .map(|(_, m)| m.clone())
                .unwrap_or_default(),
        }
    }

    /// The pixels of `key`: the file read, or the formula rendered at the
    /// size of the terminal's text, on its background.
    fn load(&self, key: &ImageKey, cell_height: f32) -> Option<image::DynamicImage> {
        match key {
            ImageKey::File(f, _) => kalem_core::images::decode(f, 2400)
                .ok()
                .map(image::DynamicImage::ImageRgba8),
            ImageKey::Math { source, macros } => {
                use org_math::MathEngine;
                let (body, display) = org_math::source::body(source);
                let (fg, bg) = self.math_colors;
                let request = org_math::Request {
                    latex: org_math::source::prepare(body, macros),
                    display,
                    size: cell_height * 0.8,
                    scale: 1.,
                    color: [fg[0], fg[1], fg[2], 255],
                };
                let img = org_math::Ratex.render(&request).ok()?;
                // On the background: sixel has no transparency.
                let mut rgba = img.rgba;
                for px in rgba.as_chunks_mut::<4>().0 {
                    let a = u32::from(px[3]);
                    for i in 0..3 {
                        px[i] = ((u32::from(px[i]) * a + u32::from(bg[i]) * (255 - a)) / 255) as u8;
                    }
                    px[3] = 255;
                }
                image::RgbaImage::from_raw(img.width, img.height, rgba)
                    .map(image::DynamicImage::ImageRgba8)
            }
        }
    }

    /// The size in cells of the image `key`, loading it first, for a text
    /// `width` cells wide.
    fn size(&mut self, key: &ImageKey, width: u16) -> Option<(u16, u16)> {
        let picker = self.picker.as_ref()?;
        if !self.cache.contains_key(key) {
            let f = picker.font_size();
            let entry = self.load(key, f.height.max(1) as f32).map(|img| {
                let picker = self.picker.as_ref().expect("a picker");
                let (cw, ch) = (f.width.max(1) as f32, f.height.max(1) as f32);
                let mut cols = (img.width() as f32 / cw).ceil().max(1.0);
                let mut rows = (img.height() as f32 / ch).ceil().max(1.0);
                // Fit the width and 20 rows, keeping the aspect ratio.
                let k = (width.max(1) as f32 / cols).min(20.0 / rows).min(1.0);
                cols = (cols * k).max(1.0);
                rows = (rows * k).max(1.0);
                let protocol = if self.compress
                    && picker.protocol_type() == ratatui_image::picker::ProtocolType::Kitty
                {
                    use ratatui_image::protocol::{
                        StatefulProtocol, StatefulProtocolType, kitty::StatefulKitty,
                    };
                    let id = self.cache.len() as u32 + 1;
                    let kitty = StatefulKitty::new(id, false, true);
                    StatefulProtocol::new(
                        img,
                        picker.font_size(),
                        None,
                        StatefulProtocolType::Kitty(kitty),
                    )
                } else {
                    picker.new_resize_protocol(img)
                };
                ImageEntry {
                    protocol,
                    rows: rows as u16,
                    cols: cols as u16,
                }
            });
            self.cache.insert(key.clone(), entry);
        }
        self.cache.get(key)?.as_ref().map(|e| (e.rows, e.cols))
    }

    /// Draws the image `key` into `rect`.
    fn render(&mut self, key: &ImageKey, rect: Rect, buf: &mut Buffer) {
        if let Some(Some(e)) = self.cache.get_mut(key) {
            use ratatui::widgets::StatefulWidget;
            ratatui_image::StatefulImage::default().render(rect, buf, &mut e.protocol);
        }
    }
}

/// Grids of tables by their start, for a text version.
type GridCache = RefCell<(u64, HashMap<usize, Arc<Grid>>)>;

/// The document model tables of contents are made from, for a text
/// version.
#[derive(Default)]
struct TocCache(RefCell<Option<(u64, Arc<org_model::Document>)>>);

impl std::fmt::Debug for TocCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TocCache")
    }
}

/// Highlighted source blocks by their start, for a text version.
type CodeCache = RefCell<(u64, HashMap<usize, Option<Arc<Code>>>)>;

/// A highlighted source block.
#[derive(Debug)]
pub(crate) struct Code {
    /// Where its code starts.
    start: usize,
    /// The spans of each code line.
    lines: Vec<Vec<kalem_highlight::Span>>,
}

/// A table drawn as an aligned grid.
#[derive(Debug)]
pub(crate) struct Grid {
    view: TableView,
    widths: Vec<u16>,
    /// Glyphs of each cell, by row.
    cells: Vec<Vec<Vec<Glyph>>>,
}

/// The editor view's state that layouts read.
struct Shared<'a> {
    folds: &'a Folds,
    grids: &'a GridCache,
    code: &'a CodeCache,
    images: &'a RefCell<Images>,
    toc: &'a TocCache,
    source: bool,
    focus: bool,
    plain: &'a PlainCache,
    windowed: &'a RefCell<kalem_highlight::Windowed>,
    raw_math: bool,
    outline_indent: bool,
}

/// Lines in the order shown, and each line's place.
type Order = (std::rc::Rc<Vec<usize>>, HashMap<usize, usize>);

/// What is needed to lay out lines.
pub(crate) struct Layout<'a> {
    doc: &'a DocumentState,
    parse: Option<&'a Parse>,
    folds: &'a Folds,
    blocks: &'a [Block],
    /// Visible byte ranges, merged.
    visible: Vec<Range<usize>>,
    /// The lines in the order shown, when it is not the text's (a CSV view
    /// sorted by a column), with each line's place.
    order: Option<Order>,
    /// Starts of blocks folded to their first line away from the cursor:
    /// drawers and runs of setting keywords.
    folded: HashSet<usize>,
    caps: &'a Caps,
    /// The text width; narrower while the rows of an indented line are
    /// made.
    width: std::cell::Cell<u16>,
    cursor: usize,
    source: bool,
    /// Text under headings indented to their titles.
    outline_indent: bool,
    /// Formulas shown as their source, not approximated.
    raw_math: bool,
    grids: &'a GridCache,
    code: &'a CodeCache,
    images: &'a RefCell<Images>,
    toc: &'a TocCache,
    plain: &'a PlainCache,
    windowed: &'a RefCell<kalem_highlight::Windowed>,
    /// The cursor's line, shown with a background in plain text and the
    /// source view.
    current: Option<usize>,
    /// LaTeX: the paragraphs over several lines shown as one, away from
    /// the cursor.
    paragraphs: Vec<Range<usize>>,
}

impl<'a> Layout<'a> {
    fn new(
        doc: &'a DocumentState,
        shared: Shared<'a>,
        blocks: &'a [Block],
        caps: &'a Caps,
        width: u16,
    ) -> Layout<'a> {
        let Shared {
            folds,
            grids,
            code,
            images,
            toc,
            source,
            focus,
            plain,
            windowed,
            raw_math,
            outline_indent,
        } = shared;
        let is_plain = doc.meta.mode != kalem_core::DocumentMode::Org;
        if is_plain && doc.dired.is_none() {
            let mut p = plain.borrow_mut();
            if p.as_ref().is_none_or(|(v, ..)| *v != doc.version()) {
                let text = doc.text().as_str();
                let lang = match &doc.meta.mode {
                    kalem_core::DocumentMode::Text { language: Some(l) } => Some(l.as_str()),
                    kalem_core::DocumentMode::Markdown => Some("md"),
                    kalem_core::DocumentMode::Latex => Some("latex"),
                    _ => None,
                };
                // Very large files are colored a window at a time (T2.7a.3).
                let found = lang.and_then(kalem_highlight::Language::find);
                let large = text.len() > 4 << 20;
                let language = found.filter(|_| !large);
                {
                    let mut w = windowed.borrow_mut();
                    let wanted = found.filter(|_| large);
                    if w.language.map(|l| l.name()) != wanted.map(|l| l.name()) {
                        *w = kalem_highlight::Windowed::new(wanted);
                    }
                }
                let old = p.take().and_then(|(_, h, _)| h);
                let h = language.map(|l| match old {
                    Some(mut h) if h.language().name() == l.name() => {
                        h.update(text);
                        h
                    }
                    _ => kalem_highlight::Highlighter::new(l, text),
                });
                let step = match kalem_core::text::detect_indent(text) {
                    Some(kalem_core::text::Indent::Spaces(n)) => n,
                    _ => 0,
                };
                *p = Some((doc.version(), h, step));
            }
        }
        {
            let mut g = grids.borrow_mut();
            if g.0 != doc.version() {
                *g = (doc.version(), HashMap::new());
            }
            let mut c = code.borrow_mut();
            if c.0 != doc.version() {
                *c = (doc.version(), HashMap::new());
            }
        }
        let parse = doc.parse().filter(|(_, current)| *current).map(|(p, _)| p);
        let len = doc.text().len();
        // A CSV filter or sort: the rows shown, and their order.
        let shown = (!source)
            .then(|| {
                kalem_core::csv::shown_lines(doc).or_else(|| kalem_core::bibtex::shown_lines(doc))
            })
            .flatten();
        let order = shown
            .as_ref()
            .filter(|_| doc.csv_sort.is_some() || doc.bib_sort.is_some())
            .map(|lines| {
                let at: HashMap<usize, usize> =
                    lines.iter().enumerate().map(|(i, &l)| (l, i)).collect();
                (lines.clone(), at)
            });
        let (mut visible, folded) = if let Some(lines) = &shown {
            let text = doc.text();
            let mut sorted: Vec<usize> = lines.to_vec();
            sorted.sort_unstable();
            let mut ranges: Vec<Range<usize>> = Vec::new();
            for l in sorted {
                let r = text.line_start(l)..(text.line_range(l).end + 1).min(len + 1);
                match ranges.last_mut() {
                    Some(last) if last.end >= r.start => last.end = last.end.max(r.end),
                    _ => ranges.push(r),
                }
            }
            (ranges, HashSet::new())
        } else if source
            || (parse.is_none()
                && doc.latex().is_none()
                && doc.meta.mode != kalem_core::DocumentMode::Markdown)
            || blocks.is_empty()
        {
            (std::iter::once(0..len + 1).collect(), HashSet::new())
        } else {
            let v = view::visible(doc.text().as_str(), blocks, folds, doc.selection.head);
            (v.ranges, v.folded)
        };
        // The empty last line after a final line feed.
        if let Some(r) = visible.last_mut()
            && r.end == len
        {
            r.end = len + 1;
        }
        // LaTeX environments drawn as images show on their first line (in
        // Org documents and LaTeX documents).
        let latex = doc.latex().is_some();
        // Without images, the formula shows its Unicode approximation on
        // its first line all the same.
        let pictures = images.borrow().picker.is_some();
        if (parse.is_some() || latex) && !source && !raw_math {
            let text = doc.text();
            let c = doc.selection.head;
            for b in blocks.iter().filter(|b| b.kind == BlockKind::Math) {
                if b.range.start <= c && c <= b.content_end {
                    continue;
                }
                let first = text.line_of(b.range.start);
                let last = text.line_of(b.content_end.saturating_sub(1).max(b.range.start));
                if last == first {
                    continue;
                }
                let src = text.as_str()[b.range.start..b.content_end].trim_end();
                if pictures {
                    let mut im = images.borrow_mut();
                    let key = match parse {
                        Some(p) => im.math(src, p, doc.version()),
                        None => {
                            let src = kalem_core::latex_view::math_source(doc, b.range.clone())
                                .unwrap_or_else(|| src.to_string());
                            im.latex_math(&src, doc)
                        }
                    };
                    if im.size(&key, width).is_none() && !latex {
                        continue;
                    }
                }
                let hide = text.line_start(first + 1)..text.line_range(last).end + 1;
                visible = visible
                    .iter()
                    .flat_map(|r| {
                        [r.start..r.end.min(hide.start), r.start.max(hide.end)..r.end]
                            .into_iter()
                            .filter(|x| x.start < x.end)
                    })
                    .collect();
            }
        }
        // A LaTeX paragraph over several lines away from the cursor shows as
        // one, on its first line.
        let mut paragraphs = Vec::new();
        if latex && !source {
            let text = doc.text();
            let c = doc.selection.head;
            for p in kalem_core::latex_view::joined_paragraphs(doc).iter() {
                if p.start <= c && c <= p.end {
                    continue;
                }
                let first = text.line_of(p.start);
                let last = text.line_of(p.end);
                if last == first {
                    continue;
                }
                paragraphs.push(p.clone());
                let hide = text.line_start(first + 1)..text.line_range(last).end + 1;
                visible = visible
                    .iter()
                    .flat_map(|r| {
                        [r.start..r.end.min(hide.start), r.start.max(hide.end)..r.end]
                            .into_iter()
                            .filter(|x| x.start < x.end)
                    })
                    .collect();
            }
        }
        // The narrowed part, or the section in focus.
        if let Some(lim) = view::limit(doc, focus) {
            let end = if lim.end >= len { len + 1 } else { lim.end };
            visible = view::clip(&visible, &(lim.start..end));
            if visible.is_empty() {
                visible.push(lim.start..(lim.start + 1).min(len + 1));
            }
        }
        Layout {
            doc,
            parse,
            folds,
            blocks,
            visible,
            order,
            folded,
            caps,
            width: std::cell::Cell::new(width),
            cursor: doc.selection.head,
            outline_indent: outline_indent && !source && parse.is_some(),
            raw_math,
            source,
            grids,
            code,
            images,
            toc,
            plain,
            windowed,
            current: (is_plain || source).then(|| doc.text().line_of(doc.selection.head)),
            paragraphs,
        }
    }

    /// The image a line shows away from the cursor, when the terminal can
    /// draw images: an image link alone on it, a displayed formula alone
    /// on it, or a LaTeX environment (on its first line). Returns what it
    /// shows, a label for when it cannot show, and its size.
    fn image(&self, line: usize) -> Option<(ImageKey, String, u16, u16)> {
        if self.doc.latex().is_some() && !self.source {
            return self.latex_image(line);
        }
        let p = self.parse.filter(|_| !self.source)?;
        self.images.borrow().picker.as_ref()?;
        let range = self.range(line);
        if let Some(src) = self.math_block(range.start) {
            let mut images = self.images.borrow_mut();
            let key = images.math(src, p, self.doc.version());
            let (rows, cols) = images.size(&key, self.width.get())?;
            let label = src.lines().next().unwrap_or("").to_string();
            return Some((key, label, rows, cols));
        }
        if range.start <= self.cursor && self.cursor <= range.end {
            return None;
        }
        let v = view::line_view(&p.syntax(), p.context(), range, Some(self.cursor));
        let mut found = None;
        for r in &v.runs {
            match &r.widget {
                Some(
                    w @ (view::Widget::Image { .. } | view::Widget::Math { display: true, .. }),
                ) if found.is_none() => found = Some(w.clone()),
                None if r.text.trim().is_empty() => {}
                _ => return None,
            }
        }
        let mut images = self.images.borrow_mut();
        let (key, label) = match found? {
            view::Widget::Image { path, width } => {
                let cols = images.cols(width, self.width.get(), &path);
                (images.file(&path, cols), format!("[image: {path}]"))
            }
            view::Widget::Math { source, .. } if !self.raw_math => {
                let label = kalem_core::math::unicode(&source);
                (images.math(&source, p, self.doc.version()), label)
            }
            _ => return None,
        };
        let limit = match &key {
            ImageKey::File(_, cols) => *cols,
            ImageKey::Math { .. } => self.width.get(),
        };
        let (rows, cols) = images.size(&key, limit)?;
        Some((key, label, rows, cols))
    }

    /// [`Layout::image`] in a LaTeX document: a math environment on its
    /// first line, a formula or a picture alone on its line.
    fn latex_image(&self, line: usize) -> Option<(ImageKey, String, u16, u16)> {
        self.images.borrow().picker.as_ref()?;
        let range = self.range(line);
        if let Some(src) = self.math_block(range.start) {
            let b = self.block_at(range.start)?;
            let source = kalem_core::latex_view::math_source(self.doc, b.range.clone())
                .unwrap_or_else(|| src.to_string());
            let mut images = self.images.borrow_mut();
            let key = images.latex_math(&source, self.doc);
            let (rows, cols) = images.size(&key, self.width.get())?;
            let label = src.lines().next().unwrap_or("").to_string();
            return Some((key, label, rows, cols));
        }
        if range.start <= self.cursor && self.cursor <= range.end {
            return None;
        }
        let v = kalem_core::latex_view::line_view(self.doc, range, Some(self.cursor));
        let mut found = None;
        for r in &v.runs {
            match &r.widget {
                Some(
                    w @ (view::Widget::Image { .. } | view::Widget::Math { display: true, .. }),
                ) if found.is_none() => {
                    found = Some(w.clone());
                }
                None if r.text.trim().is_empty() => {}
                _ => return None,
            }
        }
        let mut images = self.images.borrow_mut();
        let (key, label) = match found? {
            view::Widget::Image { path, width } => {
                let cols = images.cols(width, self.width.get(), &path);
                (images.file(&path, cols), format!("[image: {path}]"))
            }
            view::Widget::Math { source, .. } if !self.raw_math => {
                let label = kalem_core::math::unicode(&source);
                (images.latex_math(&source, self.doc), label)
            }
            _ => return None,
        };
        let limit = match &key {
            ImageKey::File(_, cols) => *cols,
            ImageKey::Math { .. } => self.width.get(),
        };
        let (rows, cols) = images.size(&key, limit)?;
        Some((key, label, rows, cols))
    }

    /// The source of the LaTeX environment starting at `start`, when it
    /// shows as an image: away from the cursor, with formulas shown.
    fn math_block(&self, start: usize) -> Option<&'a str> {
        if self.raw_math {
            return None;
        }
        let b = self.block_at(start)?;
        (b.kind == BlockKind::Math
            && b.range.start == start
            && !(b.range.start <= self.cursor && self.cursor <= b.content_end))
            .then(|| self.doc.text().as_str()[b.range.start..b.content_end].trim_end())
    }

    /// The highlighting of the source block `b`, if its language is known.
    fn code(&self, b: &Block) -> Option<Arc<Code>> {
        let lang = b.kind.highlight_language()?;
        if let Some(c) = self.code.borrow().1.get(&b.range.start) {
            return c.clone();
        }
        let text = self.text();
        let first = text.line_of(b.range.start);
        let last = text.line_of(b.content_end.saturating_sub(1).max(b.range.start));
        let start = text.line_start(first + 1).min(b.content_end);
        let end = text.line_start(last).max(start);
        let c = kalem_highlight::Language::find(lang).map(|l| {
            Arc::new(Code {
                start,
                lines: kalem_highlight::highlight(l, &text.as_str()[start..end]),
            })
        });
        self.code.borrow_mut().1.insert(b.range.start, c.clone());
        c
    }

    /// Colors the glyphs of a source block's code line.
    fn color_code(&self, line: &Range<usize>, glyphs: &mut [Glyph]) {
        let Some(b) = self.block_at(line.start) else {
            return;
        };
        let Some(code) = self.code(b) else { return };
        if line.start < code.start {
            return;
        }
        let text = self.text();
        let i = text.line_of(line.start) - text.line_of(code.start);
        let Some(spans) = code.lines.get(i) else {
            return;
        };
        for g in glyphs {
            if g.src_end <= g.src {
                continue;
            }
            let rel = g.src - line.start;
            if let Some(sp) = spans.iter().find(|s| s.range.contains(&rel)) {
                g.style = render::code_style(sp.kind, g.style, self.caps);
            }
        }
    }

    /// Colors LaTeX's inline code of a known language (`\lstinline`).
    fn color_inline_code(&self, line: &Range<usize>, glyphs: &mut [Glyph]) {
        let text = self.text();
        // LaTeX's inline code; a Markdown code block's line with the
        // state of the block's lines before it.
        let block = kalem_core::markdown::code_block_on_line(self.doc, line.clone());
        let code = kalem_core::latex_view::inline_code(self.doc, line.clone())
            .into_iter()
            .map(|(r, l)| (r, l, None))
            .chain(block.map(|(b, i, l)| (b, l, Some(i))));
        for (code, lang, nth) in code {
            let Some(l) = kalem_highlight::Language::find(&lang) else {
                continue;
            };
            let all = kalem_highlight::highlight_block(l, &text.as_str()[code.clone()]);
            let Some(spans) = all.get(nth.unwrap_or(0)) else {
                continue;
            };
            // The block's line starts where the line does.
            let code = match nth {
                Some(_) => line.start..line.end,
                None => code,
            };
            for g in glyphs.iter_mut() {
                if g.src_end <= g.src || g.src < code.start || g.src >= code.end {
                    continue;
                }
                let rel = g.src - code.start;
                if let Some(sp) = spans.iter().find(|s| s.range.contains(&rel)) {
                    g.style = render::code_style(sp.kind, g.style, self.caps);
                }
            }
        }
    }

    /// The table block holding line start `s`, if any.
    fn table_block(&self, s: usize) -> Option<&'a Block> {
        let i = self.blocks.partition_point(|b| b.range.end <= s);
        self.blocks
            .get(i)
            .filter(|b| b.kind == BlockKind::Table && s < b.content_end)
    }

    /// The grid of the table starting at `start`.
    fn grid(&self, start: usize) -> Option<Arc<Grid>> {
        if let Some(g) = self.grids.borrow().1.get(&start) {
            return Some(g.clone());
        }
        // A LaTeX table's cells are drawn with an empty Org tree.
        let empty;
        let (view, p) = match self.parse {
            Some(p) if self.doc.latex().is_none() => {
                (view::table_view(&p.syntax(), p.context(), start, None)?, p)
            }
            _ => {
                empty = org_syntax::parse("");
                (
                    kalem_core::latex_table::table_view(self.doc, start)?,
                    &empty,
                )
            }
        };
        let root = p.syntax();
        let n = view.align.len();
        let mut widths = vec![1u16; n];
        let mut cells = Vec::new();
        // Spans are fitted once the columns they cover are measured.
        let mut spanned = Vec::new();
        for (ri, row) in view.rows.iter().enumerate() {
            let mut glyph_row = Vec::new();
            if let TableRow::Data { cells: cs, .. } = row {
                for (i, c) in cs.iter().enumerate() {
                    let lv = view::LineView {
                        range: c.range.clone(),
                        runs: c.runs.clone(),
                        ..view::LineView::default()
                    };
                    let g = render::glyphs(
                        &lv,
                        &root,
                        p.context(),
                        true,
                        false,
                        self.caps,
                        self.raw_math,
                    )
                    .glyphs;
                    let w: u16 = g.iter().map(|g| g.width).sum();
                    match view.spans.iter().find(|s| s.0 == ri && s.1 == i) {
                        Some(&(_, _, n, _)) => spanned.push((i, n, w)),
                        None => widths[i] = widths[i].max(w),
                    }
                    glyph_row.push(g);
                }
            }
            cells.push(glyph_row);
        }
        for (i, n, w) in spanned {
            let last = (i + n).min(widths.len()) - 1;
            let covered: u16 = widths[i..=last].iter().sum::<u16>() + 3 * (last - i) as u16;
            if w > covered {
                widths[last] += w - covered;
            }
        }
        let g = Arc::new(Grid {
            view,
            widths,
            cells,
        });
        self.grids.borrow_mut().1.insert(start, g.clone());
        Some(g)
    }

    /// A table line away from the cursor: one row of the aligned grid.
    fn grid_row(&self, grid: &Grid, line: &Range<usize>) -> Option<Vec<Glyph>> {
        let ri = grid
            .view
            .rows
            .iter()
            .position(|r| r.line().start == line.start)?;
        let ascii = self.caps.ascii;
        let text = self.text().as_str();
        let deco = |t: &str, at: usize, style: ratatui::style::Style| Glyph {
            text: t.to_string(),
            width: unicode_width::UnicodeWidthStr::width(t) as u16,
            style,
            src: at,
            src_end: at,
            link: None,
            data: None,
        };
        let dim = ratatui::style::Style::default().add_modifier(Modifier::DIM);
        let mut out = Vec::new();
        let indent =
            text[line.clone()].len() - text[line.clone()].trim_start_matches([' ', '\t']).len();
        for _ in 0..indent {
            out.push(deco(" ", line.start, ratatui::style::Style::default()));
        }
        match &grid.view.rows[ri] {
            TableRow::Rule { .. } => {
                let (l, m, r, h) = if ascii {
                    ("|", "+", "|", "-")
                } else {
                    ("├", "┼", "┤", "─")
                };
                out.push(deco(l, line.start, dim));
                for (i, w) in grid.widths.iter().enumerate() {
                    for _ in 0..w + 2 {
                        out.push(deco(h, line.start, dim));
                    }
                    out.push(deco(
                        if i + 1 == grid.widths.len() { r } else { m },
                        line.start,
                        dim,
                    ));
                }
            }
            TableRow::Data { cells, .. } => {
                let bar = if ascii { "|" } else { "│" };
                out.push(deco(bar, line.start, dim));
                let mut i = 0;
                while i < grid.widths.len() {
                    // A span: one cell as wide as the columns it covers.
                    let span = grid
                        .view
                        .spans
                        .iter()
                        .find(|s| s.0 == ri && s.1 == i)
                        .map(|&(_, _, n, a)| (n.min(grid.widths.len() - i), a));
                    let (n, align) =
                        span.unwrap_or((1, grid.view.align.get(i).copied().unwrap_or('l')));
                    let w = grid.widths[i..i + n].iter().sum::<u16>() + 3 * (n as u16 - 1);
                    let glyphs = grid.cells[ri].get(i).cloned().unwrap_or_default();
                    let at = cells.get(i).map_or(line.end, |c| c.range.start);
                    let end = cells.get(i + n - 1).map_or(line.end, |c| c.range.end);
                    let cw: u16 = glyphs.iter().map(|g| g.width).sum();
                    let pad = w.saturating_sub(cw);
                    i += n;
                    let (left, right) = match align {
                        'r' => (pad, 0),
                        'c' => (pad / 2, pad - pad / 2),
                        _ => (0, pad),
                    };
                    out.push(deco(" ", at, ratatui::style::Style::default()));
                    for _ in 0..left {
                        out.push(deco(" ", at, ratatui::style::Style::default()));
                    }
                    out.extend(glyphs);
                    for _ in 0..right + 1 {
                        out.push(deco(" ", end, ratatui::style::Style::default()));
                    }
                    out.push(deco(bar, end, dim));
                }
                // A rule written after the row's `\\\\`: under the row.
                if grid.view.ruled.contains(&ri) {
                    for g in &mut out {
                        g.style = g.style.add_modifier(Modifier::UNDERLINED);
                    }
                }
            }
        }
        Some(out)
    }

    fn text(&self) -> &kalem_core::Text {
        self.doc.text()
    }

    /// The line's range without a carriage return before its line feed.
    fn range(&self, line: usize) -> Range<usize> {
        let r = self.text().line_range(line);
        let t = self.text().as_str();
        if r.end > r.start && t.as_bytes()[r.end - 1] == b'\r' {
            r.start..r.end - 1
        } else {
            r
        }
    }

    /// The table of contents a `#+TOC:` line shows away from the cursor:
    /// a title row, then a row a heading, each leading to it.
    fn toc_rows(&self, range: &Range<usize>) -> Option<Vec<Vec<Glyph>>> {
        if self.source {
            return None;
        }
        let listing = if self.doc.latex().is_some() {
            // `\tableofcontents` and the footnotes in LaTeX.
            kalem_core::toc::latex_listing(self.doc, range.clone())?
        } else {
            let p = self.parse?;
            if !kalem_core::toc::wanted(self.text().as_str(), self.cursor, range) {
                return None;
            }
            let version = self.doc.version();
            let doc = {
                let mut c = self.toc.0.borrow_mut();
                match &*c {
                    Some((v, d)) if *v == version => d.clone(),
                    _ => {
                        let d = Arc::new(org_model::Document::new(p.clone()));
                        *c = Some((version, d.clone()));
                        d
                    }
                }
            };
            kalem_core::toc::contents(&kalem_core::toc::toc_at(&doc, range.clone())?)
        };
        let style = |s: view::Style| render::style_base(&s, 0, self.caps);
        let dim = style(view::Style {
            dim: true,
            ..Default::default()
        });
        let link = style(view::Style {
            link: true,
            ..Default::default()
        });
        let title = listing.title.as_str();
        let glyphs = |t: &str, st, data: Option<WidgetAt>| -> Vec<Glyph> {
            use unicode_segmentation::UnicodeSegmentation;
            t.graphemes(true)
                .map(|g| Glyph {
                    data: data.clone(),
                    ..Glyph::decoration(g, st, range.start)
                })
                .collect()
        };
        let width = self.width.get();
        let mut rows = tui_rich_text::wrap(glyphs(title, dim, None), 0, width);
        for (text, start) in listing.rows.clone() {
            let data = (view::Widget::TocRow { start }, range.start, range.end);
            let hang = (text.len() - text.trim_start().len()) as u16;
            rows.extend(tui_rich_text::wrap(
                glyphs(&text, link, Some(data)),
                hang,
                width,
            ));
        }
        Some(rows)
    }

    /// The line's rows.
    fn line_rows(&self, line: usize) -> Vec<Vec<Glyph>> {
        let range = self.range(line);
        if let Some(rows) = self.toc_rows(&range) {
            return rows;
        }
        if let Some((_, _, rows, _)) = self.image(line) {
            let blank = Glyph {
                text: " ".into(),
                width: 1,
                style: ratatui::style::Style::default(),
                src: range.start,
                src_end: range.start,
                link: None,
                data: None,
            };
            return vec![vec![blank]; rows as usize];
        }
        let on_line = range.start <= self.cursor && self.cursor <= range.end;
        // A LaTeX table away from the cursor: a row of the grid.
        if self.doc.latex().is_some()
            && !self.source
            && let Some(block) = self.table_block(range.start)
            && !(block.range.start <= self.cursor && self.cursor <= block.content_end)
            && let Some(row) = self
                .grid(block.range.start)
                .and_then(|g| self.grid_row(&g, &range))
        {
            // A row wider than the screen wraps, as in the graphical
            // editor, rather than losing its end.
            return tui_rich_text::wrap(row, 0, self.width.get());
        }
        if let (Some(p), false) = (self.parse, self.source)
            && let Some(block) = self.table_block(range.start)
        {
            let editing = block.range.start <= self.cursor && self.cursor <= block.content_end;
            if !editing
                && let Some(row) = self
                    .grid(block.range.start)
                    .and_then(|g| self.grid_row(&g, &range))
            {
                return tui_rich_text::wrap(row, 0, self.width.get());
            }
            // Being edited: the source, all markup shown, with box bars.
            let root = p.syntax();
            let v =
                view::line_view_with(&root, p.context(), range.clone(), Some(self.cursor), true);
            let mut lg = render::glyphs(
                &v,
                &root,
                p.context(),
                on_line,
                false,
                self.caps,
                self.raw_math,
            );
            if !self.caps.ascii {
                let rule = self.text().as_str()[range.clone()]
                    .trim_start()
                    .starts_with("|-");
                let last = lg.glyphs.iter().rposition(|g| g.text == "|");
                let first = lg.glyphs.iter().position(|g| g.text == "|");
                for (i, g) in lg.glyphs.iter_mut().enumerate() {
                    let new = match (g.text.as_str(), rule) {
                        ("|", false) => "│",
                        ("|", true) if Some(i) == first => "├",
                        ("|", true) if Some(i) == last => "┤",
                        ("+", true) => "┼",
                        ("-", true) => "─",
                        _ => continue,
                    };
                    g.text = new.to_string();
                    g.style = g.style.add_modifier(Modifier::DIM);
                }
            }
            return tui_rich_text::wrap(lg.glyphs, lg.hang, self.width.get());
        }
        let mut justify = false;
        // A very long line, in any mode: the part around the cursor, as it
        // is.
        let parse = self.parse.filter(|_| range.len() <= view::LONG_LINE);
        let lg = match parse {
            Some(p) if !self.source => {
                let root = p.syntax();
                // A LaTeX environment without the pictures of formulas: its
                // Unicode approximation on its first line.
                let v = match self.math_text(&range) {
                    Some(u) => view::LineView {
                        range: range.clone(),
                        runs: vec![view::Run {
                            src: range.clone(),
                            text: u,
                            verbatim: false,
                            style: view::Style::default(),
                            widget: None,
                        }],
                        ..view::LineView::default()
                    },
                    None => view::line_view(&root, p.context(), range.clone(), Some(self.cursor)),
                };
                if let Some(frame) = self.frame(&v) {
                    return vec![frame];
                }
                let folded = (v.heading > 0
                    && self.folds.get(range.start).is_some()
                    && self.hides(range.start))
                    || self.folded.contains(&range.start);
                let mut lg = render::glyphs(
                    &v,
                    &root,
                    p.context(),
                    on_line,
                    folded,
                    self.caps,
                    self.raw_math,
                );
                if v.mono && v.role == view::LineRole::Content {
                    self.color_code(&range, &mut lg.glyphs);
                }
                justify = v.align == kalem_core::rich::Align::Justify;
                // Kalem's alignment: a line that fits moves right or to the
                // middle.
                let used: u16 = lg.glyphs.iter().map(|g| g.width).sum();
                let pad = match v.align {
                    kalem_core::rich::Align::Right => self.width.get().saturating_sub(used + 1),
                    kalem_core::rich::Align::Center => self.width.get().saturating_sub(used) / 2,
                    _ => 0,
                };
                if pad > 0 && used < self.width.get() {
                    let blank = Glyph {
                        text: " ".into(),
                        width: 1,
                        style: ratatui::style::Style::default(),
                        src: range.start,
                        src_end: range.start,
                        link: None,
                        data: None,
                    };
                    lg.glyphs
                        .splice(0..0, std::iter::repeat_n(blank, pad as usize));
                }
                lg
            }
            // The source view: the text as it is, with Org highlighting.
            Some(p) => {
                let root = p.syntax();
                let v =
                    view::source_line_view(&root, p.context(), self.text().as_str(), range.clone());
                let mut lg = render::glyphs(
                    &v,
                    &root,
                    p.context(),
                    true,
                    false,
                    self.caps,
                    self.raw_math,
                );
                self.color_code(&range, &mut lg.glyphs);
                lg
            }
            None => {
                // A very long line shows the part around the cursor.
                let v = if self.doc.meta.mode == kalem_core::DocumentMode::Latex
                    && !self.source
                    && range.len() <= view::LONG_LINE
                {
                    // LaTeX as the document reads; a formula over several
                    // lines without its picture, its Unicode approximation.
                    match self.math_text(&range) {
                        Some(u) => {
                            // Centered as LaTeX centers it (`fleqn`: flush
                            // left, indented).
                            let align = kalem_core::latex_view::display_align(self.doc);
                            let text = if align == kalem_core::rich::Align::Center {
                                u.trim_start().to_string()
                            } else {
                                u
                            };
                            view::LineView {
                                range: range.clone(),
                                runs: vec![view::Run {
                                    src: range.clone(),
                                    text,
                                    verbatim: false,
                                    style: view::Style::default(),
                                    widget: None,
                                }],
                                align,
                                ..view::LineView::default()
                            }
                        }
                        None if let Some(p) =
                            self.paragraphs.iter().find(|p| p.start == range.start) =>
                        {
                            kalem_core::latex_view::paragraph_view(
                                self.doc,
                                p.clone(),
                                Some(self.cursor),
                            )
                        }
                        None => kalem_core::latex_view::line_view(
                            self.doc,
                            range.clone(),
                            Some(self.cursor),
                        ),
                    }
                } else if kalem_core::bibtex::is_bib(self.doc)
                    && !self.source
                    && range.len() <= view::LONG_LINE
                {
                    // A BibTeX entry as a row of the grid.
                    kalem_core::bibtex::line_view(self.doc, range.clone(), Some(self.cursor))
                } else if self.doc.meta.mode == kalem_core::DocumentMode::Csv
                    && !self.source
                    && range.len() <= view::LONG_LINE
                {
                    // A CSV row as a row of the grid.
                    let layout = kalem_core::csv::layout(self.doc);
                    kalem_core::csv::line_view(
                        &layout,
                        self.text().as_str(),
                        range.clone(),
                        Some(self.cursor),
                    )
                } else if self.doc.meta.mode == kalem_core::DocumentMode::Markdown
                    && !self.source
                    && range.len() <= view::LONG_LINE
                {
                    // Markdown as it reads, markers hidden away from the
                    // cursor.
                    kalem_core::markdown::line_view(self.doc, range.clone(), Some(self.cursor))
                } else {
                    let mut v = view::plain_line_view(
                        self.text().as_str(),
                        range.clone(),
                        Some(self.cursor),
                    );
                    // LaTeX's source view: the diagnostics flagged too.
                    kalem_core::latex_view::flag_diagnostics(self.doc, &mut v);
                    v
                };
                let empty = org_syntax::parse("");
                let mut lg = render::glyphs(
                    &v,
                    &empty.syntax(),
                    empty.context(),
                    on_line,
                    false,
                    self.caps,
                    self.raw_math,
                );
                if self.doc.latex().is_some() && !self.source {
                    // LaTeX as it reads: code in its language, the rest as
                    // the view styles it.
                    self.color_code(&range, &mut lg.glyphs);
                    self.color_inline_code(&range, &mut lg.glyphs);
                } else if !(kalem_core::bibtex::is_bib(self.doc) && !self.source)
                    || v.runs.iter().all(|r| r.verbatim)
                {
                    // Syntax colors where the line shows its source (not a
                    // BibTeX grid row).
                    self.plain_colors(line, &range, &mut lg.glyphs);
                    if self.doc.meta.mode == kalem_core::DocumentMode::Markdown && !self.source {
                        self.color_inline_code(&range, &mut lg.glyphs);
                    }
                }
                lg
            }
        };
        let mut rows = tui_rich_text::wrap(lg.glyphs, lg.hang, self.width.get());
        if justify {
            tui_rich_text::justify(&mut rows, self.width.get());
        }
        rows
    }

    /// Whether the headline starting at `start` has anything under its
    /// heading line to hide.
    fn hides(&self, start: usize) -> bool {
        let i = self.blocks.partition_point(|b| b.range.start < start);
        let Some(BlockKind::Heading { level }) = self.blocks.get(i).map(|b| &b.kind) else {
            return false;
        };
        self.blocks
            .get(i + 1)
            .is_some_and(|b| !matches!(b.kind, BlockKind::Heading { level: l } if l <= *level))
    }

    /// The block holding line start `s`.
    /// The Unicode approximation of the LaTeX formula over several lines
    /// that starts on line `range`, shown there when it has no picture and
    /// the cursor is elsewhere (its other lines are hidden).
    fn math_text(&self, range: &Range<usize>) -> Option<String> {
        if self.source || self.raw_math {
            return None;
        }
        let b = self
            .block_at(range.start)
            .filter(|b| b.kind == BlockKind::Math && b.range.start == range.start)?;
        let c = self.cursor;
        if (b.range.start <= c && c <= b.content_end) || b.content_end <= range.end + 1 {
            return None;
        }
        if self.image(self.text().line_of(range.start)).is_some() {
            return None;
        }
        let src = kalem_core::latex_view::math_source(self.doc, b.range.clone())
            .unwrap_or_else(|| self.text().as_str()[b.range.start..b.content_end].to_string());
        // The body: the environment's `\\begin` and `\\end` out, its number
        // (a `\\tag`) after it.
        let mut body = src.trim().to_string();
        if body.starts_with("\\begin{")
            && let Some(close) = body.find('}')
        {
            body = body[close + 1..].to_string();
        }
        if let Some(end) = body.rfind("\\end{") {
            body.truncate(end);
        }
        let mut tags = Vec::new();
        while let Some(at) = body.find("\\tag{") {
            let Some(len) = body[at..].find('}') else {
                break;
            };
            tags.push(body[at + 5..at + len].to_string());
            body.replace_range(at..at + len + 1, "");
        }
        // Alignment points go; rows are separated by semicolons.
        let body = body.replace("\\\\", " ; ").replace('&', "");
        let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut out = format!("  {}", kalem_core::math::unicode(&body));
        for t in tags {
            out.push_str(&format!("   ({t})"));
        }
        Some(out)
    }

    fn block_at(&self, s: usize) -> Option<&'a Block> {
        let i = self.blocks.partition_point(|b| b.range.end <= s);
        self.blocks.get(i).filter(|b| b.range.start <= s)
    }

    /// A block's first or last line away from the cursor, drawn as a frame
    /// with the block's type (or a source block's language).
    fn frame(&self, v: &view::LineView) -> Option<Vec<Glyph>> {
        // A `#+begin_…` or `#+end_…` line of the block, as the graphical
        // editor decides (its role may be content deep in nested blocks).
        let lower = self.text().as_str()[v.range.clone()]
            .trim_start()
            .to_ascii_lowercase();
        if v.role != view::LineRole::Delimiter
            && !lower.starts_with("#+begin")
            && !lower.starts_with("#+end")
        {
            return None;
        }
        let b = self.block_at(v.range.start)?;
        if v.range.start >= b.content_end {
            return None;
        }
        let framed = matches!(
            b.kind,
            BlockKind::Code { .. }
                | BlockKind::Export { .. }
                | BlockKind::CommentBlock
                | BlockKind::Verbatim
                | BlockKind::Quote
                | BlockKind::Center
                | BlockKind::Verse
                | BlockKind::Special
                | BlockKind::Dynamic
        );
        if !framed || (b.range.start <= self.cursor && self.cursor <= b.content_end) {
            return None;
        }
        let line = self.text().as_str()[v.range.clone()].trim();
        let begin = line.len() >= 7 && line.as_bytes()[..7].eq_ignore_ascii_case(b"#+begin");
        let label = if begin {
            match &b.kind {
                BlockKind::Code { language: None } => String::new(),
                k => k.label(line),
            }
        } else {
            String::new()
        };
        let (corner, h) = match (begin, self.caps.ascii) {
            (true, false) => ("╭─", "─"),
            (false, false) => ("╰─", "─"),
            (_, true) => ("+-", "-"),
        };
        let head = if label.is_empty() {
            corner.to_string()
        } else {
            format!("{corner} {label} ")
        };
        let w = unicode_width::UnicodeWidthStr::width(head.as_str());
        let fill = (self.width.get() as usize).saturating_sub(w).min(60);
        let text = format!("{head}{}", h.repeat(fill));
        let style = ratatui::style::Style::default().add_modifier(Modifier::DIM);
        Some(
            unicode_segmentation::UnicodeSegmentation::graphemes(text.as_str(), true)
                .map(|g| Glyph {
                    text: g.to_string(),
                    width: unicode_width::UnicodeWidthStr::width(g) as u16,
                    style,
                    src: v.range.start,
                    src_end: v.range.start,
                    link: None,
                    data: None,
                })
                .collect(),
        )
    }

    /// Syntax colors and indentation guides for a plain text line.
    fn plain_colors(&self, line: usize, range: &Range<usize>, glyphs: &mut [Glyph]) {
        // A file manager listing: its own styles.
        if let Some(d) = self.doc.dired.as_deref() {
            let styles = d.styles(line);
            for g in glyphs.iter_mut() {
                if g.src_end <= g.src {
                    continue;
                }
                let rel = g.src - range.start;
                for (r, st) in styles {
                    if r.contains(&rel) {
                        g.style = render::dir_style(*st, g.style, self.caps);
                    }
                }
            }
            return;
        }
        let p = self.plain.borrow();
        let Some((_, h, step)) = p.as_ref() else {
            return;
        };
        // A very large file: a window of lines.
        let windowed = h.is_none().then(|| {
            let mut w = self.windowed.borrow_mut();
            w.language?;
            let t = self.text();
            Some(
                w.line(
                    self.doc.version(),
                    line,
                    t.as_str(),
                    |n| t.line_range(n),
                    t.line_count(),
                )
                .to_vec(),
            )
        });
        let spans = match (h, windowed.flatten()) {
            (Some(h), _) => Some(h.line(line).to_vec()),
            (None, w) => w,
        };
        if let Some(spans) = spans {
            for g in glyphs.iter_mut() {
                if g.src_end <= g.src {
                    continue;
                }
                let rel = g.src - range.start;
                if let Some(sp) = spans.iter().find(|s| s.range.contains(&rel)) {
                    g.style = render::code_style(sp.kind, g.style, self.caps);
                }
            }
        }
        // A guide at each indentation step of the leading blanks.
        if *step > 1 && !self.caps.ascii {
            for (col, g) in glyphs.iter_mut().enumerate() {
                if g.text != " " {
                    break;
                }
                if col > 0 && col % step == 0 {
                    g.text = "│".into();
                    g.style = g.style.add_modifier(Modifier::DIM);
                }
            }
        }
    }

    /// The columns line `line` moves right under a heading of level `n`
    /// (as `org-indent-mode`): to the heading's title, after its level
    /// glyph; 0 for headings and text before the first one.
    fn outline_indent_of(&self, line: usize) -> u16 {
        if !self.outline_indent {
            return 0;
        }
        let start = self.text().line_start(line);
        let i = self.blocks.partition_point(|b| b.range.start <= start);
        let Some((b, level)) = self.blocks[..i].iter().rev().find_map(|b| match b.kind {
            BlockKind::Heading { level } => Some((b, level)),
            _ => None,
        }) else {
            return 0;
        };
        if b.range.start == start {
            return 0;
        }
        (2 * level.saturating_sub(1).min(4) + 2) as u16
    }

    /// Whether line `line` is monospace code (for the background).
    pub(crate) fn is_code(&self, line: usize) -> bool {
        if self.source || self.parse.is_none() {
            return false;
        }
        let s = self.text().line_start(line);
        let i = self.blocks.partition_point(|b| b.range.end <= s);
        self.blocks.get(i).is_some_and(|b| {
            (b.kind.is_code() || b.kind == BlockKind::CommentBlock) && s < b.content_end
        })
    }
}

impl Lines for Layout<'_> {
    type Data = WidgetAt;

    fn line_of(&self, offset: usize) -> usize {
        self.text().line_of(offset.min(self.text().len()))
    }

    fn line_start(&self, line: usize) -> usize {
        self.text().line_start(line)
    }

    fn line_end(&self, line: usize) -> usize {
        self.range(line).end
    }

    /// Whether line `line` shows.
    fn is_visible(&self, line: usize) -> bool {
        let s = self.text().line_start(line);
        let i = self.visible.partition_point(|r| r.end <= s);
        self.visible.get(i).is_some_and(|r| r.start <= s)
    }

    /// The next visible line after `line`.
    fn next_line(&self, line: usize) -> Option<usize> {
        if let Some((lines, at)) = &self.order {
            return at.get(&line).and_then(|&i| lines.get(i + 1)).copied();
        }
        let n = self.text().line_count();
        let next = line + 1;
        if next >= n {
            return None;
        }
        if self.is_visible(next) {
            return Some(next);
        }
        let s = self.text().line_start(next);
        let i = self.visible.partition_point(|r| r.end <= s);
        let r = self.visible.get(i)?;
        Some(self.text().line_of(r.start.max(s)))
    }

    /// The visible line before `line`.
    fn prev_line(&self, line: usize) -> Option<usize> {
        if let Some((lines, at)) = &self.order {
            return at
                .get(&line)
                .and_then(|&i| i.checked_sub(1))
                .and_then(|i| lines.get(i))
                .copied();
        }
        let prev = line.checked_sub(1)?;
        if self.is_visible(prev) {
            return Some(prev);
        }
        let s = self.text().line_start(prev);
        let i = self.visible.partition_point(|r| r.start <= s);
        let r = self.visible[..i].last()?;
        Some(self.text().line_of(r.end.saturating_sub(1).max(r.start)))
    }

    fn rows(&self, line: usize, _width: u16) -> Vec<Vec<Glyph>> {
        let indent = self.outline_indent_of(line);
        if indent == 0 {
            return self.line_rows(line);
        }
        // Rows made for the narrower column, then moved right.
        let full = self.width.get();
        self.width.set(full.saturating_sub(indent).max(8));
        let rows = self.line_rows(line);
        self.width.set(full);
        let at = self.range(line).start;
        rows.into_iter()
            .map(|row| {
                let mut out: Vec<Glyph> = (0..indent)
                    .map(|_| Glyph {
                        text: " ".into(),
                        width: 1,
                        style: ratatui::style::Style::default(),
                        src: at,
                        src_end: at,
                        link: None,
                        data: None,
                    })
                    .collect();
                out.extend(row);
                out
            })
            .collect()
    }

    fn background_start(&self, line: usize) -> u16 {
        self.outline_indent_of(line)
    }

    fn background(&self, line: usize) -> Option<ratatui::style::Color> {
        if self.caps.no_color {
            return None;
        }
        if self.current == Some(line) {
            return Some(match &self.caps.colors {
                Some(t) => render::solid(t.bar, t),
                None => ratatui::style::Color::Indexed(235),
            });
        }
        self.is_code(line).then(|| render::code_bg(self.caps))
    }
}

/// The view state layouts share, borrowing only the fields they need.
macro_rules! shared {
    ($v:expr) => {
        Shared {
            folds: &$v.folds,
            grids: &$v.grids,
            code: &$v.code,
            images: &$v.images,
            toc: &$v.toc,
            source: $v.source,
            focus: $v.focus,
            plain: &$v.plain,
            windowed: &$v.windowed,
            raw_math: $v.raw_math,
            outline_indent: $v.outline_indent,
        }
    };
}

impl EditorView {
    /// Starts the view again after the document's mode changed.
    pub fn reset(&mut self) {
        self.blocks = None;
        self.folds = Folds::default();
        self.highlights.clear();
        self.follow = true;
    }

    /// The text column in `area`: `line_width` characters (and the margins)
    /// at the left edge or in the middle, or all of it.
    fn column(&self, area: Rect) -> Rect {
        let w = self.line_width.saturating_add(2);
        if self.line_width == 0 || area.width <= w {
            return area;
        }
        Rect {
            x: if self.center {
                area.x + (area.width - w) / 2
            } else {
                area.x
            },
            width: w,
            ..area
        }
    }

    /// The blocks of the document's current text.
    fn blocks(&mut self, doc: &DocumentState) -> Arc<Vec<Block>> {
        let version = doc.version();
        if let Some((v, b)) = &self.blocks
            && *v == version
        {
            return b.clone();
        }
        let b = match doc.parse() {
            // LaTeX: its displayed formulas and code, the text between.
            _ if doc.meta.mode == kalem_core::DocumentMode::Markdown => {
                Arc::new(kalem_core::markdown::blocks(doc))
            }
            _ if doc.latex().is_some() => Arc::new(kalem_core::latex_view::blocks(doc)),
            Some((p, true)) => Arc::new(view::blocks(&p.syntax(), p.context())),
            _ => Arc::new(Vec::new()),
        };
        if !b.is_empty() {
            self.folds.retain(&b);
            self.blocks = Some((version, b.clone()));
        }
        b
    }

    /// Unfolds the headlines that hide the cursor.
    fn reveal(&mut self, doc: &DocumentState, blocks: &[Block]) {
        let c = doc.selection.head;
        loop {
            let hidden: Vec<usize> = {
                let visible = self.folds.visible(blocks);
                if visible
                    .iter()
                    .any(|b| b.range.contains(&c) || (b.range.end == c && c == doc.text().len()))
                {
                    return;
                }
                // The folded headlines whose subtree holds the cursor.
                blocks
                    .iter()
                    .filter(|b| matches!(b.kind, BlockKind::Heading { .. }) && b.range.start <= c)
                    .filter(|b| self.folds.get(b.range.start).is_some())
                    .map(|b| b.range.start)
                    .collect()
            };
            let Some(h) = hidden.last() else { return };
            self.folds.set(*h, None);
        }
    }

    /// Scrolls by `delta` rows.
    pub fn scroll(&mut self, doc: &DocumentState, caps: &Caps, delta: isize) {
        let blocks = self.blocks(doc);
        let width = self.width();
        let l = Layout::new(doc, shared!(self), &blocks, caps, width);
        self.viewport.scroll(&l, delta, width);
    }

    fn width(&self) -> u16 {
        if self.wrap {
            self.area.width.saturating_sub(2).max(1)
        } else {
            // No wrapping: one row a line.
            u16::MAX / 2
        }
    }

    /// The cursor position `delta` rows down (or up), at the kept column.
    pub fn vertical(&mut self, doc: &DocumentState, caps: &Caps, delta: isize) -> usize {
        let blocks = self.blocks(doc);
        let width = self.width();
        let l = Layout::new(doc, shared!(self), &blocks, caps, width);
        self.viewport.vertical(&l, doc.selection.head, delta, width)
    }

    /// The source offset under screen cell (`col`, `row`), and the widget
    /// there, if any.
    pub fn hit(
        &mut self,
        doc: &DocumentState,
        caps: &Caps,
        col: u16,
        row: u16,
    ) -> Option<(usize, Option<WidgetAt>)> {
        let blocks = self.blocks(doc);
        let l = Layout::new(doc, shared!(self), &blocks, caps, self.width());
        self.drawn.as_ref()?.hit(&l, col, row)
    }

    /// Draws the document into `area` of `buf`; returns the cursor's cell.
    pub fn draw(
        &mut self,
        doc: &DocumentState,
        caps: &Caps,
        buf: &mut Buffer,
        area: Rect,
    ) -> Option<(u16, u16)> {
        let area = self.column(area);
        // Line numbers in a gutter.
        // (A CSV grid numbers its rows itself.)
        let numbers = self.line_numbers
            && doc.dired.is_none()
            && (doc.meta.mode != kalem_core::DocumentMode::Org || self.source)
            && (doc.meta.mode != kalem_core::DocumentMode::Csv || self.source);
        let digits = doc.text().line_count().to_string().len() as u16;
        let gutter = if numbers && area.width > digits + 10 {
            digits + 1
        } else {
            0
        };
        let full = area;
        let area = Rect {
            x: area.x + gutter,
            width: area.width - gutter,
            ..area
        };
        self.area = area;
        // A view that starts outside the narrowed part or the section in
        // focus starts at its beginning.
        if let Some(lim) = view::limit(doc, self.focus)
            && !(lim.start..=lim.end).contains(&self.viewport.top)
        {
            self.viewport.top = doc.text().line_start(doc.text().line_of(lim.start));
            self.viewport.top_row = 0;
        }
        // A spreadsheet-looking CSV grid: the column letters in a bar on
        // the first row, above everything.
        let sheet =
            (doc.meta.mode == kalem_core::DocumentMode::Csv && !self.source && area.height > 3)
                .then(|| kalem_core::csv::layout(doc))
                .filter(|l| l.view.sheet);
        let letters_area = Rect { height: 1, ..area };
        let area = if sheet.is_some() {
            Rect {
                y: area.y + 1,
                height: area.height - 1,
                ..area
            }
        } else {
            area
        };
        let blocks = self.blocks(doc);
        if self.follow {
            self.reveal(doc, &blocks);
        }
        let width = self.width();
        let l = Layout::new(doc, shared!(self), &blocks, caps, width);
        // A CSV file's header row stays on the first row when its rows
        // scroll: the rows below get one row less.
        let header = doc.meta.mode == kalem_core::DocumentMode::Csv
            && !self.source
            && area.height > 2
            && kalem_core::csv::layout(doc).dialect.header;
        if self.follow {
            let height = if header { area.height - 1 } else { area.height };
            self.viewport
                .scroll_to(&l, doc.selection.head, width, height);
            self.follow = false;
        }
        let pinned = header && (self.viewport.top > 0 || self.viewport.top_row > 0);
        let header_area = Rect { height: 1, ..area };
        let area = if pinned {
            Rect {
                y: area.y + 1,
                height: area.height - 1,
                ..area
            }
        } else {
            area
        };
        let sel = doc.selection;
        let mark_style = match (&caps.colors, caps.no_color) {
            (_, true) => ratatui::style::Style::default().add_modifier(Modifier::UNDERLINED),
            (Some(t), false) => ratatui::style::Style::default()
                .bg(render::solid(t.mark, t))
                .fg(render::rgb(t.foreground)),
            (None, false) => ratatui::style::Style::default()
                .bg(ratatui::style::Color::Yellow)
                .fg(ratatui::style::Color::Black),
        };
        // Without wrapping, the view scrolls sideways to keep the cursor.
        if self.wrap {
            self.hscroll = 0;
        } else {
            let line = l.line_of(sel.head);
            let rows = l.rows(line, width);
            let (_, cx) = tui_rich_text::cursor_in(&rows, sel.head);
            let w = area.width.saturating_sub(3).max(1);
            if cx < self.hscroll {
                self.hscroll = cx.saturating_sub(w / 4);
            } else if cx >= self.hscroll + w {
                self.hscroll = cx + 1 - w + w / 4;
            }
        }
        let (marks, mark_style) = if !self.block.is_empty() {
            (
                &self.block,
                ratatui::style::Style::default().add_modifier(Modifier::REVERSED),
            )
        } else if self.highlights.is_empty() {
            (&self.references, mark_style)
        } else {
            (&self.highlights, mark_style)
        };
        // The bracket matching the one at the cursor, and it.
        let with_pair: Vec<Range<usize>>;
        let marks = match kalem_core::code::pair_at_cursor(doc).filter(|_| self.block.is_empty()) {
            Some((o, c)) => {
                let mut v: Vec<Range<usize>> =
                    marks.iter().cloned().chain([o..o + 1, c..c + 1]).collect();
                v.sort_by_key(|r| r.start);
                with_pair = v;
                &with_pair
            }
            None => marks,
        };
        let options = Options {
            cursor: sel.head,
            selection: sel.anchor.min(sel.head)..sel.anchor.max(sel.head),
            marks,
            mark_style,
            margin: 1,
            hscroll: self.hscroll,
        };
        let drawn = tui_rich_text::draw(&l, &self.viewport, buf, area, &options);
        if pinned {
            let top = Viewport {
                top: 0,
                top_row: 0,
                goal_x: None,
            };
            let plain = Options {
                marks: &[],
                ..options
            };
            tui_rich_text::draw(&l, &top, buf, header_area, &plain);
            buf.set_style(
                header_area,
                ratatui::style::Style::default().add_modifier(Modifier::UNDERLINED),
            );
        }
        // Images over their rows when they fit on screen whole, else their
        // names.
        let x0 = area.x + 1;
        for dl in &drawn.lines {
            let Some((key, label, h, w)) = l.image(dl.line) else {
                continue;
            };
            if dl.skipped == 0 && dl.y + h <= area.bottom() {
                let rect = Rect::new(x0, dl.y, w.min(area.right() - x0), h);
                self.images.borrow_mut().render(&key, rect, buf);
            } else if dl.y < area.bottom() {
                let style = ratatui::style::Style::default().add_modifier(Modifier::DIM);
                buf.set_stringn(
                    x0,
                    dl.y,
                    &label,
                    area.right().saturating_sub(x0) as usize,
                    style,
                );
            }
        }
        if gutter > 0 {
            let current = doc.text().line_of(sel.head);
            for dl in drawn.lines.iter().filter(|d| d.skipped == 0) {
                let mut style = ratatui::style::Style::default().add_modifier(Modifier::DIM);
                if dl.line == current {
                    style = ratatui::style::Style::default().add_modifier(Modifier::BOLD);
                }
                let n = format!("{:>w$}", dl.line + 1, w = usize::from(digits));
                buf.set_stringn(full.x, dl.y, &n, usize::from(digits), style);
            }
            if pinned {
                let style = ratatui::style::Style::default().add_modifier(Modifier::DIM);
                let n = format!("{:>w$}", 1, w = usize::from(digits));
                buf.set_stringn(full.x, header_area.y, &n, usize::from(digits), style);
            }
        }
        if let Some(layout) = &sheet {
            let current = kalem_core::csv::cell_at(doc).map(|(_, _, _, c)| c);
            let shade = |c: u32| {
                ratatui::style::Color::Rgb((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8)
            };
            let gray = ratatui::style::Style::default()
                .bg(shade(kalem_core::csv::SHEET_GRAY))
                .fg(ratatui::style::Color::Black);
            let on = gray
                .bg(shade(kalem_core::csv::SHEET_ACTIVE))
                .add_modifier(Modifier::BOLD);
            for x in letters_area.left()..letters_area.right() {
                buf[(x, letters_area.y)].set_symbol(" ").set_style(gray);
            }
            // Lined up with the rows: after the margin, scrolled with them.
            let mut x = i64::from(letters_area.x) + 1 - self.hscroll as i64;
            for (piece, current_col) in kalem_core::csv::letters_bar(layout, current) {
                for ch in piece.chars() {
                    if x >= i64::from(letters_area.x) && x < i64::from(letters_area.right()) {
                        let style = if current_col { on } else { gray };
                        buf[(x as u16, letters_area.y)]
                            .set_symbol(&ch.to_string())
                            .set_style(style);
                    }
                    x += 1;
                }
            }
        }
        let cursor = drawn.cursor;
        self.drawn = Some(drawn);
        cursor
    }

    /// Moves positions through an edit.
    pub fn map(&mut self, tx: &org_edit::Transaction) {
        self.viewport.top = tx.map(self.viewport.top, org_edit::Assoc::Before);
        self.folds.map(tx);
    }

    /// The heading block that holds the cursor's line, for folding.
    pub fn heading_at(&mut self, doc: &DocumentState) -> Option<(Arc<Vec<Block>>, usize)> {
        let blocks = self.blocks(doc);
        let c = doc.selection.head;
        let i = blocks.iter().position(|b| {
            matches!(b.kind, BlockKind::Heading { .. }) && b.range.start <= c && c <= b.content_end
        })?;
        Some((blocks, i))
    }

    /// The blocks, for folding all.
    pub fn all_blocks(&mut self, doc: &DocumentState) -> Arc<Vec<Block>> {
        self.blocks(doc)
    }
}
