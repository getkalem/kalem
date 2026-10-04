//! The host side of the viewer contract (design §11.13, D54): the viewers
//! installed, a document opened by one ([`ViewerState`], the
//! [`DocumentMode::Viewer`](crate::DocumentMode::Viewer) kind, with no
//! text), its view (zoom, pan, turn, the unit shown) and the commands both
//! frontends run on it.
//!
//! Frontends draw [`ViewerState::bitmap`] where [`ViewerState::placement`]
//! says, after telling the state the size of its area
//! ([`ViewerState::set_area`]); the commands work in that area's pixels.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use kalem_viewer::{
    Bitmap, Edit, ErrorStyle, FileHandle, GridCell, GridEdit, GridLayout, InfoField, MacroEntry,
    MacroOutcome, MacroQuestion, MacroUi, RenderRequest, Rendered, SaveOutput, Structure, UnitKind,
    Validation, ValidationError, Viewer, ViewerDocument,
};

use crate::command::{
    Command, CommandHandler, CommandResult, CommandSource, EditorContext, Request,
};

static VIEWERS: RwLock<Vec<Arc<dyn Viewer>>> = RwLock::new(Vec::new());

/// The last generation given out: unique over every document, so a
/// frontend's texture of one file is never taken for another's.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// Installs a viewer (a bundled plugin; the plugin loader of T3.1.12 for
/// components). A viewer with the same identifier is replaced.
pub fn register(viewer: Arc<dyn Viewer>) {
    if let Ok(mut all) = VIEWERS.write() {
        all.retain(|v| v.id() != viewer.id());
        all.push(viewer);
    }
}

/// The viewers installed.
pub fn viewers() -> Vec<Arc<dyn Viewer>> {
    VIEWERS.read().map(|v| v.clone()).unwrap_or_default()
}

/// The viewer that opens the file named `name` starting with `head`: the
/// surest, the first installed among equals.
pub fn find(name: &str, head: &[u8]) -> Option<Arc<dyn Viewer>> {
    let mut best: Option<(kalem_viewer::Detection, Arc<dyn Viewer>)> = None;
    for v in viewers() {
        let d = v.detect(name, head);
        if d > kalem_viewer::Detection::No && best.as_ref().is_none_or(|(b, _)| d > *b) {
            best = Some((d, v));
        }
    }
    best.map(|(_, v)| v)
}

/// The outline of the document a viewer shows, for the outline panel:
/// each item's `start` is the unit it goes to.
pub fn outline_items(doc: &crate::DocumentState) -> Option<Vec<crate::view::OutlineItem>> {
    let v = doc.viewer.as_deref()?;
    Some(
        v.structure()
            .outline
            .iter()
            .map(|e| crate::view::OutlineItem {
                level: e.level as usize,
                todo: None,
                title: e.title.clone(),
                start: e.unit,
                file: None,
            })
            .collect(),
    )
}

/// Where the outline panel marks the reader: the unit a viewer shows,
/// else the cursor.
pub fn outline_position(doc: &crate::DocumentState) -> usize {
    doc.viewer.as_deref().map_or(doc.selection.head, |v| v.unit)
}

/// The word around the glyph at `r` of `text`: the letters, digits and
/// `_` on either side; the glyph alone when it is none of them.
fn word_around(text: &str, r: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let Some(glyph) = text.get(r.clone()) else {
        return r;
    };
    if !glyph.chars().all(word) {
        return r;
    }
    let start = text[..r.start]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| word(c))
        .last()
        .map_or(r.start, |(i, _)| i);
    let end = text[r.end..]
        .char_indices()
        .find(|&(_, c)| !word(c))
        .map_or(text.len(), |(i, _)| r.end + i);
    start..end
}

/// The line of `text` the glyph at `r` is on.
fn line_around(text: &str, r: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let start = text[..r.start.min(text.len())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let end = text[r.end.min(text.len())..]
        .find('\n')
        .map_or(text.len(), |i| r.end + i);
    start..end
}

/// The byte ranges of `needle` (lower case) in `text`, case folded where
/// folding keeps the text's length (it does not for `İ`; such a text is
/// searched as it is).
fn find_folded(text: &str, needle: &str) -> Vec<std::ops::Range<usize>> {
    let lower = text.to_lowercase();
    let hay = if lower.len() == text.len() {
        lower.as_str()
    } else {
        text
    };
    hay.match_indices(needle)
        .map(|(i, m)| i..i + m.len())
        .collect()
}

/// Renders `key` (unit, rotation, generation, scale) of `doc`, turned.
fn render(
    doc: &Mutex<Box<dyn ViewerDocument>>,
    (unit, rotation, _, scale): RenderKey,
) -> Result<Bitmap, String> {
    let request = RenderRequest {
        scale: f32::from_bits(scale),
        ..RenderRequest::default()
    };
    let Rendered::Bitmap(b) = doc
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .render(unit, request)
        .map_err(|e| e.to_string())?;
    Ok(b.rotated(rotation))
}

/// The viewer for the file at `path` when it is not text (design §2.6):
/// text files open in a document mode even when a viewer could show them
/// (an SVG drawing is XML).
pub fn for_file(path: &Path) -> Option<Arc<dyn Viewer>> {
    let head = FileHandle::new(path).read_at(0, 8192).ok()?;
    if !crate::mode::looks_binary(&head) {
        return None;
    }
    find(&name_of(path), &head)
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// How large the unit is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// Whole in the area; a picture never larger than its own size.
    Fit,
    /// As wide as the area, scrolled down (a page).
    FitWidth,
    /// This many screen pixels per pixel of the unit.
    Scale(f32),
}

/// Where a unit is drawn in its area, in the area's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The left edge of the whole bitmap (it may be outside the area).
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The bitmap's width as drawn.
    pub width: f32,
    /// Its height as drawn.
    pub height: f32,
    /// Screen pixels per bitmap pixel.
    pub scale: f32,
}

impl Placement {
    /// The part of the bitmap inside an area `w` × `h`: x, y, width and
    /// height in bitmap pixels.
    pub fn visible(&self, w: f32, h: f32) -> (f32, f32, f32, f32) {
        let x0 = (-self.x / self.scale).max(0.0);
        let y0 = (-self.y / self.scale).max(0.0);
        let x1 = ((w - self.x) / self.scale).min(self.width / self.scale);
        let y1 = ((h - self.y) / self.scale).min(self.height / self.scale);
        (x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }
}

/// The zoom steps, as a factor.
const ZOOM_STEP: f32 = 1.25;
/// The smallest and largest scales.
const MIN_SCALE: f32 = 0.01;
const MAX_SCALE: f32 = 64.0;

/// What a bitmap was rendered for: unit, rotation, generation and scale
/// (its bits).
type RenderKey = (usize, u8, u64, u32);

/// Text selected on a unit: the glyphs where the drag began and where it
/// is, as byte ranges of the unit's text, and the selection's rectangles
/// in the unit's pixels for the range they were read for.
#[derive(Debug)]
struct TextSelection {
    unit: usize,
    anchor: std::ops::Range<usize>,
    head: std::ops::Range<usize>,
    rects: Option<(std::ops::Range<usize>, Vec<[f32; 4]>)>,
    /// Where the drag went while a render held the document, in the
    /// unit's points: the head moves there once the document is free.
    pending: Option<(f32, f32)>,
}

impl TextSelection {
    /// The bytes selected: from the first glyph's start to the last one's
    /// end, whichever way the drag went.
    fn range(&self) -> std::ops::Range<usize> {
        self.anchor.start.min(self.head.start)..self.anchor.end.max(self.head.end)
    }
}

/// The search's marks on a unit: what they were read for (the unit, the
/// match shown, the count) and the rectangles, the shown match's `true`.
type Marks = ((usize, Option<usize>, usize), Vec<([f32; 4], bool)>);

/// A search of a document's text, run a unit at a time on a thread so
/// that the document's lock is held for one unit only and matches show as
/// they are found.
#[derive(Debug)]
struct Search {
    query: String,
    /// The matches found so far, by unit and byte range of its text, in
    /// document order.
    hits: Vec<(usize, std::ops::Range<usize>)>,
    /// The match shown.
    current: Option<usize>,
    /// The unit shown when the search started: the first match shown is
    /// the first from there on.
    origin: usize,
    /// The units searched so far.
    scanned: usize,
    /// What the thread finds, each unit's matches; gone when it is done.
    rx: Option<Receiver<(usize, Vec<std::ops::Range<usize>>)>>,
    /// The matches' rectangles on a unit, in its own pixels, the shown
    /// one marked: for the unit, the match shown and the count they were
    /// read for.
    marks: Option<Marks>,
}

/// A unit and the edits' generation it was read at.
type UnitVersion = (usize, u64);

/// A file opened by a viewer, and how it is shown.
pub struct ViewerState {
    /// The viewer.
    pub viewer: Arc<dyn Viewer>,
    /// The document, shared with the thread that renders it.
    doc: Arc<Mutex<Box<dyn ViewerDocument>>>,
    structure: Structure,
    /// Each unit's size at scale 1, when the viewer knows it (a page).
    sizes: Vec<Option<(f32, f32)>>,
    /// A render running on a thread: what it renders (unit, rotation,
    /// generation, scale) and where its result comes.
    pending: Option<(RenderKey, Receiver<Result<Bitmap, String>>)>,
    /// The unit shown.
    pub unit: usize,
    /// The zoom.
    pub zoom: Zoom,
    /// The point of the bitmap at the area's center, in bitmap pixels;
    /// `None` for the middle.
    pub center: Option<(f32, f32)>,
    /// Quarter turns clockwise of the view (not of the file).
    pub rotation: u8,
    /// The information panel is shown.
    pub info: bool,
    /// The frames play.
    pub playing: bool,
    area: (f32, f32),
    /// Device pixels per pixel of the area (a Retina display's 2).
    pixel_ratio: f32,
    /// How far a scroll has pushed past the unit's top or bottom edge,
    /// in area pixels: past a share of the area, the page turns.
    overscroll: f32,
    undo: Vec<(String, String)>,
    redo: Vec<(String, String)>,
    generation: u64,
    /// The last render: unit, rotation, generation, scale.
    cache: Option<(RenderKey, Bitmap)>,
    /// The neighbors of the unit shown, rendered ahead (at most two).
    ahead: Vec<(RenderKey, Bitmap)>,
    /// The find bar's search of the units' text.
    search: Option<Search>,
    /// Text selected on the unit shown by dragging over it.
    text_sel: Option<TextSelection>,
    /// The information panel's fields, for a generation.
    info_cache: Mutex<Option<(u64, Vec<InfoField>)>>,
    /// What the frontends ask on every frame or key, kept for when a
    /// render holds the document: whether it is modified, the shown
    /// unit's edits and text (by unit and generation).
    modified_cache: std::sync::atomic::AtomicBool,
    edits_cache: Mutex<Option<(UnitVersion, Vec<Edit>)>>,
    text_cache: Mutex<Option<(UnitVersion, String)>>,
    /// Which units are grids (sheets, tables).
    grids: Vec<bool>,
    /// Each grid unit's cursor and scroll.
    grid_pos: std::collections::HashMap<usize, GridPos>,
    /// The layout of the grid shown, by unit and generation.
    grid_cache: Option<(usize, u64, GridLayout)>,
    /// How many rows and columns the frontend shows, frozen ones included.
    grid_visible: (u32, u32),
    /// Cells cut: the unit, the range and the text put on the clipboard;
    /// pasting that text moves them.
    cut: Option<(usize, [u32; 4], String)>,
    /// Circle Invalid Data is on: cells their validation refuses are marked.
    pub circle_invalid: bool,
    /// The cursor's cell's validation, by unit, cell and generation.
    validation_cache: Option<(CellKey, Option<Validation>)>,
    /// The user's own lists a fill goes round.
    fill_lists: Vec<Vec<String>>,
    /// The color borders are drawn in (Line Color); `None` automatic.
    pub border_color: Option<[u8; 3]>,
    /// The selection's Average, Count and Sum, for the unit, selection
    /// and generation they were found for.
    selection_sums: Option<(SumsKey, Option<String>)>,
    /// What Find looks for.
    pub grid_search: GridSearch,
    /// The cells copied last (unit, range), for Paste Special.
    pub copied: Option<(usize, [u32; 4])>,
    /// The cells Format Painter took the format of, until it paints.
    pub painter: Option<(usize, [u32; 4])>,
    /// Show Formulas: formula cells show their formulas, not their values.
    pub show_formulas: bool,
    /// The text entries of a column, for AutoComplete: unit, column and
    /// generation they were read at.
    col_entries: Option<((usize, u32, u64), Vec<String>)>,
    /// The cells pointed at while a formula is typed, drawn as such.
    pub pointer: Option<[u32; 4]>,
    /// The functions formulas can use, read once.
    functions: Option<Vec<(String, String)>>,
    /// The outline's summary rows and whether each is collapsed, by unit
    /// and generation.
    outline_marks: Option<(UnitAt, Vec<(u32, bool)>)>,
    /// Trace Precedents' and Dependents' arrows on the sheet shown: from
    /// a range to a cell.
    pub arrows: Vec<([u32; 4], (u32, u32))>,
    /// The Watch Window's cells: unit, row, column.
    pub watches: Vec<(usize, u32, u32)>,
    /// A selection of several ranges (Go To Special's), until the cursor
    /// moves.
    pub areas: Vec<[u32; 4]>,
    /// The sheet shown's pictures and shapes, by unit and generation.
    drawings_cache: Option<(UnitAt, Vec<kalem_viewer::Drawing>)>,
    /// The sheet shown's comment threads, by unit and generation.
    threads_cache: Option<(UnitAt, Vec<kalem_viewer::CommentThread>)>,
    /// The sheets' tabs (unit, name, color), by generation.
    tabs_cache: Option<(u64, Vec<SheetTab>)>,
    /// Where the terminal last drew the grid, for the mouse.
    pub hits: Option<GridHits>,
    /// The circular references, by generation.
    circ_cache: Option<(u64, Vec<SheetCell>)>,
    /// Each sheet's view settings, read once.
    views: std::collections::HashMap<usize, kalem_viewer::SheetView>,
    /// Page Break Preview's pages, by unit and generation.
    pages_cache: Option<(UnitAt, PageBreaks)>,
    /// Pictures decoded, by unit, place and generation.
    pictures: std::collections::HashMap<(usize, usize, u64), Option<Bitmap>>,
}

/// What Find looks for in a grid, and how.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GridSearch {
    /// The text.
    pub text: String,
    /// Small and capital letters told apart (Match Case).
    pub case: bool,
    /// The whole cell must be the text (Match Entire Cell Contents).
    pub whole: bool,
    /// In what the cells hold (formulas, values as entered) rather than
    /// what they show.
    pub formulas: bool,
}

impl GridSearch {
    /// Whether a cell's text matches.
    pub fn matches(&self, hay: &str) -> bool {
        if self.text.is_empty() {
            return false;
        }
        let (h, n) = if self.case {
            (hay.to_owned(), self.text.clone())
        } else {
            (hay.to_lowercase(), self.text.to_lowercase())
        };
        if self.whole { h == n } else { h.contains(&n) }
    }

    /// A cell's text with the text replaced by `with`: all of it for a
    /// whole-cell search, else every occurrence.
    pub fn replace(&self, hay: &str, with: &str) -> String {
        if self.whole {
            return with.to_owned();
        }
        if self.case {
            return hay.replace(&self.text, with);
        }
        let (low, n) = (hay.to_lowercase(), self.text.to_lowercase());
        // Where the lowercase text matches, the original's same characters
        // go (lowercasing can change lengths, so by characters).
        let hay_chars: Vec<char> = hay.chars().collect();
        let low_chars: Vec<char> = low.chars().collect();
        let n_chars: Vec<char> = n.chars().collect();
        if hay_chars.len() != low_chars.len() || n_chars.is_empty() {
            return hay.replace(&self.text, with);
        }
        let mut out = String::new();
        let mut i = 0;
        while i < low_chars.len() {
            if low_chars[i..].starts_with(&n_chars) {
                out.push_str(with);
                i += n_chars.len();
            } else {
                out.push(hay_chars[i]);
                i += 1;
            }
        }
        out
    }
}

/// A unit at a generation.
type UnitAt = (usize, u64);

/// A grid's panes: the top pane's rows and the left pane's columns
/// (first and how many: the frozen ones, or a split's), and whether they
/// are a split, whose main pane may show any row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Panes {
    /// The top pane's first row and how many rows it shows.
    pub rows: (u32, u32),
    /// The left pane's first column and how many columns it shows.
    pub cols: (u32, u32),
    /// A split rather than frozen panes.
    pub split: bool,
}

/// Page Break Preview's pages: the area printed, and where pages begin
/// along its rows and columns (with whether a manual break starts them).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageBreaks {
    /// The area printed.
    pub area: [u32; 4],
    /// The rows a page begins at, after the first.
    pub rows: Vec<(u32, bool)>,
    /// The columns a page begins at, after the first.
    pub cols: Vec<(u32, bool)>,
}

impl PageBreaks {
    /// How many pages.
    pub fn pages(&self) -> usize {
        (self.rows.len() + 1) * (self.cols.len() + 1)
    }
}

/// Where the terminal drew a grid's parts, for the mouse: the screen
/// columns of each column shown, the screen rows of each row, the
/// letters' row, the numbers' columns and the sheets' tabs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridHits {
    /// Each column shown: its column, first screen column and width.
    pub cols: Vec<(u32, u16, u16)>,
    /// Each row shown: its row and screen row.
    pub rows: Vec<(u32, u16)>,
    /// The letters' screen row, when headings are shown.
    pub letters: Option<u16>,
    /// The row numbers' screen columns.
    pub gutter: (u16, u16),
    /// Each tab: its unit, screen row, first screen column and width.
    pub tabs: Vec<(usize, u16, u16, u16)>,
}

/// What a point of a drawn grid is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSpot {
    /// A cell.
    Cell(u32, u32),
    /// A column's letter.
    Column(u32),
    /// A row's number.
    Row(u32),
    /// A sheet's tab.
    Tab(usize),
}

impl GridHits {
    /// What the screen point (`x`, `y`) is on.
    pub fn at(&self, x: u16, y: u16) -> Option<GridSpot> {
        if let Some(&(u, ..)) = self
            .tabs
            .iter()
            .find(|(_, ty, tx, w)| *ty == y && (*tx..tx + w).contains(&x))
        {
            return Some(GridSpot::Tab(u));
        }
        let col = self
            .cols
            .iter()
            .find(|(_, cx, w)| (*cx..cx + w).contains(&x))
            .map(|c| c.0);
        let row = self.rows.iter().find(|(_, ry)| *ry == y).map(|r| r.0);
        if self.letters == Some(y) {
            return col.map(GridSpot::Column);
        }
        if (self.gutter.0..self.gutter.0 + self.gutter.1).contains(&x) {
            return row.map(GridSpot::Row);
        }
        Some(GridSpot::Cell(row?, col?))
    }
}

/// A cell of a workbook: its sheet, row and column.
pub type SheetCell = (usize, u32, u32);

/// A sheet's tab: its unit, its name, its color.
pub type SheetTab = (usize, String, Option<[u8; 3]>);

/// A selection of a unit at a generation: unit, range, generation.
type SumsKey = (usize, [u32; 4], u64);

/// A cell of a unit at a generation: unit, row, column, generation.
type CellKey = (usize, u32, u32, u64);

/// A grid unit's cursor and the first row and column scrolled to (past
/// the frozen ones), zero-based.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridPos {
    /// The cursor's row.
    pub row: u32,
    /// The cursor's column.
    pub col: u32,
    /// The first row shown below the frozen rows.
    pub top: u32,
    /// The first column shown right of the frozen columns.
    pub left: u32,
    /// The other corner of a selected range, the cursor being one; `None`
    /// for the cursor's cell alone.
    pub sel: Option<(u32, u32)>,
}

/// Answers a macro's questions from the user's answers so far; plain
/// messages are collected; a question past the answers stops the run.
struct Answers<'a> {
    answers: &'a [String],
    next: usize,
}

impl MacroUi for Answers<'_> {
    fn message(&mut self, _prompt: &str, buttons: i64, _title: &str) -> Option<i64> {
        if buttons & 7 == 0 {
            return Some(1);
        }
        let a = self.answers.get(self.next)?.parse().ok()?;
        self.next += 1;
        Some(a)
    }

    fn input(&mut self, _prompt: &str, _title: &str, _default: &str) -> Option<Option<String>> {
        let a = self.answers.get(self.next)?.clone();
        self.next += 1;
        // The palette's Escape answers nothing: VBA's Cancel is an empty string.
        Some(Some(a))
    }
}

impl std::fmt::Debug for ViewerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewerState")
            .field("viewer", &self.viewer.id())
            .field("unit", &self.unit)
            .field("zoom", &self.zoom)
            .field("rotation", &self.rotation)
            .finish_non_exhaustive()
    }
}

impl ViewerState {
    /// Opens `path` with `viewer`.
    pub fn open(viewer: Arc<dyn Viewer>, path: &Path) -> Result<ViewerState, String> {
        let mut doc = viewer
            .open(FileHandle::new(path))
            .map_err(|e| e.to_string())?;
        // An OpenDocument spreadsheet the workbook viewer only shows is
        // edited as a workbook made of it, and saved back as `.ods`.
        let ods = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ods"));
        if ods && viewer.extensions().contains(&"xlsx") && doc.grid(0).is_some_and(|l| !l.editable)
        {
            let bytes = crate::workbook_io::to_xlsx(viewer.as_ref(), doc.as_mut())?;
            doc = crate::workbook_io::open_bytes(
                viewer.as_ref(),
                &path.with_extension("xlsx"),
                bytes,
            )?;
        }
        let structure = doc.structure();
        if structure.units.is_empty() {
            return Err("The file has nothing to show".into());
        }
        let playing = structure.animated();
        let mut doc = doc;
        let grids = (0..structure.units.len())
            .map(|u| doc.grid(u).is_some())
            .collect();
        let sizes = (0..structure.units.len()).map(|u| doc.size(u)).collect();
        // Pages are read as wide as the area, from the top.
        let zoom = if doc.size(0).is_some() {
            Zoom::FitWidth
        } else {
            Zoom::Fit
        };
        Ok(ViewerState {
            viewer,
            doc: Arc::new(Mutex::new(doc)),
            structure,
            sizes,
            pending: None,
            ahead: Vec::new(),
            search: None,
            text_sel: None,
            info_cache: Mutex::new(None),
            modified_cache: std::sync::atomic::AtomicBool::new(false),
            edits_cache: Mutex::new(None),
            text_cache: Mutex::new(None),
            unit: 0,
            zoom,
            center: None,
            rotation: 0,
            info: false,
            playing,
            area: (0.0, 0.0),
            pixel_ratio: 1.0,
            overscroll: 0.0,
            undo: Vec::new(),
            redo: Vec::new(),
            generation: next_generation(),
            cache: None,
            grids,
            grid_pos: std::collections::HashMap::new(),
            grid_cache: None,
            grid_visible: (30, 10),
            cut: None,
            circle_invalid: false,
            validation_cache: None,
            fill_lists: Vec::new(),
            border_color: None,
            selection_sums: None,
            grid_search: GridSearch::default(),
            copied: None,
            painter: None,
            show_formulas: false,
            col_entries: None,
            pointer: None,
            functions: None,
            outline_marks: None,
            arrows: Vec::new(),
            watches: Vec::new(),
            areas: Vec::new(),
            drawings_cache: None,
            threads_cache: None,
            tabs_cache: None,
            views: std::collections::HashMap::new(),
            hits: None,
            circ_cache: None,
            pages_cache: None,
            pictures: std::collections::HashMap::new(),
        })
    }

    /// The document's units and outline.
    pub fn structure(&self) -> &Structure {
        &self.structure
    }

    /// Changes whenever the pixels shown change: frontends key their
    /// textures by it.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn changed(&mut self) {
        self.generation = next_generation();
    }

    /// The unit shown, rendered and turned by the view's rotation; at
    /// [`ViewerState::render_scale`], so a page is as sharp as it is
    /// shown.
    pub fn bitmap(&mut self) -> Result<Bitmap, String> {
        let key = self.render_key();
        if let Some((k, b)) = &self.cache
            && *k == key
        {
            return Ok(b.clone());
        }
        if let Some(b) = self.take_ahead(key) {
            return Ok(b);
        }
        // A thread renders it already: wait for it.
        if let Some((k, rx)) = self.pending.take()
            && let Ok(done) = rx.recv()
        {
            let b = done?;
            self.cache = Some((k, b.clone()));
            if k == key {
                return Ok(b);
            }
        }
        let b = render(&self.doc, key)?;
        self.cache = Some((key, b.clone()));
        Ok(b)
    }

    /// The unit shown as far as it is rendered, for a frontend that must
    /// not wait: the bitmap at [`ViewerState::render_scale`] when it is
    /// ready, else, while a thread renders it, the last bitmap of the same
    /// unit and turn (at another scale, drawn stretched to the
    /// placement), or `None` for a unit not rendered yet. With the scale
    /// it was rendered at, for keying textures.
    pub fn bitmap_now(&mut self) -> Result<Option<(Bitmap, f32)>, String> {
        // A picture or a frame is quick, and an animation must not blink:
        // rendered here.
        if self.vector_size().is_none() {
            return self.bitmap().map(|b| Some((b, 1.0)));
        }
        let key = self.render_key();
        if let Some((k, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(done) => {
                    let k = *k;
                    self.pending = None;
                    if k == key {
                        self.cache = Some((k, done?));
                    } else if let Ok(b) = done {
                        // A neighbor rendered ahead.
                        self.ahead.push((k, b));
                    }
                }
                Err(TryRecvError::Disconnected) => self.pending = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let shown = match &self.cache {
            Some((k, b)) if *k == key => Some(b.clone()),
            _ => self.take_ahead(key),
        };
        self.prune_ahead();
        if let Some(b) = shown {
            // The page is shown: render its neighbors while the reader reads.
            if self.pending.is_none()
                && let Some(next) = self.neighbor_to_render()
            {
                self.spawn_render(next);
            }
            return Ok(Some((b, f32::from_bits(key.3))));
        }
        // One render at a time; the latest wish starts when it ends.
        if self.pending.is_none() {
            self.spawn_render(key);
        }
        Ok(self.cache.as_ref().and_then(|(k, b)| {
            ((k.0, k.1, k.2) == (key.0, key.1, key.2)).then(|| (b.clone(), f32::from_bits(k.3)))
        }))
    }

    fn spawn_render(&mut self, key: RenderKey) {
        let (tx, rx) = channel();
        let doc = self.doc.clone();
        std::thread::spawn(move || {
            let _ = tx.send(render(&doc, key));
        });
        self.pending = Some((key, rx));
    }

    /// The render of `key` made ahead, made the one shown; the bitmap
    /// shown before it is kept as a neighbor (the page just left).
    fn take_ahead(&mut self, key: RenderKey) -> Option<Bitmap> {
        let i = self.ahead.iter().position(|(k, _)| *k == key)?;
        let (k, b) = self.ahead.remove(i);
        if let Some(old) = self.cache.replace((k, b.clone())) {
            self.ahead.push(old);
        }
        Some(b)
    }

    /// The key a neighbor of the unit shown renders at: its own fit, the
    /// same turn and edits.
    fn key_for(&mut self, unit: usize) -> RenderKey {
        let shown = self.unit;
        self.unit = unit;
        let key = self.render_key();
        self.unit = shown;
        key
    }

    /// The next page, then the previous one, when not rendered yet.
    fn neighbor_to_render(&mut self) -> Option<RenderKey> {
        if !self.paged() {
            return None;
        }
        let n = self.structure.units.len();
        let mut near = vec![self.unit + 1];
        if self.unit > 0 {
            near.push(self.unit - 1);
        }
        for u in near {
            if u >= n || self.sizes.get(u).copied().flatten().is_none() {
                continue;
            }
            let k = self.key_for(u);
            if !self.ahead.iter().any(|(a, _)| *a == k) {
                return Some(k);
            }
        }
        None
    }

    /// Keeps the renders of the shown unit's two neighbors at the scale
    /// they are shown at, nothing else: a page at a Retina display's size
    /// is tens of megabytes.
    fn prune_ahead(&mut self) {
        let n = self.structure.units.len();
        let mut wanted: Vec<RenderKey> = Vec::new();
        for u in [self.unit.checked_sub(1), Some(self.unit + 1)]
            .into_iter()
            .flatten()
        {
            if u < n {
                wanted.push(self.key_for(u));
            }
        }
        self.ahead.retain(|(k, _)| wanted.contains(k));
        self.ahead.dedup_by_key(|(k, _)| *k);
    }

    /// Whether a thread is rendering: the frontend looks again soon.
    pub fn rendering(&self) -> bool {
        self.pending.is_some()
    }

    fn render_key(&mut self) -> RenderKey {
        let scale = self.render_scale();
        (self.unit, self.rotation, self.generation, scale.to_bits())
    }

    /// The document, waited for while a thread renders it.
    fn doc(&self) -> MutexGuard<'_, Box<dyn ViewerDocument>> {
        self.doc.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The unit's size at scale 1 as turned, when the viewer knows it
    /// without rendering (a page).
    fn vector_size(&self) -> Option<(f32, f32)> {
        let (w, h) = self.sizes.get(self.unit).copied().flatten()?;
        Some(if self.rotation % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        })
    }

    /// The unit's size at scale 1 as turned: the size the viewer gives,
    /// else the bitmap's (rendered once at scale 1 and cached). Placement,
    /// zoom and pan count in these pixels.
    pub fn unit_size(&mut self) -> (f32, f32) {
        if let Some(size) = self.vector_size() {
            return size;
        }
        self.bitmap()
            .map(|b| (b.width as f32, b.height as f32))
            .unwrap_or((1.0, 1.0))
    }

    /// Sets the device pixels per pixel of the area (the window's scale
    /// factor), so pages render for the display's pixels.
    pub fn set_pixel_ratio(&mut self, ratio: f32) {
        self.pixel_ratio = ratio.clamp(0.25, 8.0);
    }

    /// The scale the unit is rendered at: 1 for a picture, whose pixels
    /// are scaled when drawn; for a page, the scale shown in device
    /// pixels, rounded up to a quarter octave so a zoom step renders
    /// again but a small change does not.
    pub fn render_scale(&mut self) -> f32 {
        if self.vector_size().is_none() {
            return 1.0;
        }
        let s = (self.scale() * self.pixel_ratio).clamp(1.0 / 16.0, 16.0);
        2f32.powf((s.log2() * 4.0 - 1e-3).ceil() / 4.0)
    }

    /// Sets the size of the area the unit is drawn in, in the frontend's
    /// pixels (a terminal's cells times their size).
    pub fn set_area(&mut self, width: f32, height: f32) {
        self.area = (width.max(1.0), height.max(1.0));
    }

    /// The area's size.
    pub fn area(&self) -> (f32, f32) {
        self.area
    }

    /// The scale [`Zoom::Fit`] means in the current area.
    pub fn fit_scale(&mut self) -> f32 {
        let (w, h) = self.unit_size();
        let (aw, ah) = self.area;
        let fit = (aw / w).min(ah / h);
        // A picture is not blown up past its pixels; a page has none.
        if self.vector_size().is_some() {
            fit
        } else {
            fit.min(1.0)
        }
    }

    /// The scale shown.
    pub fn scale(&mut self) -> f32 {
        match self.zoom {
            Zoom::Fit => self.fit_scale(),
            Zoom::FitWidth => self.area.0 / self.unit_size().0,
            Zoom::Scale(s) => s,
        }
    }

    /// Where the bitmap is drawn in the area: centered where it is
    /// smaller than the area, else at [`ViewerState::center`], kept from
    /// leaving an edge empty.
    pub fn placement(&mut self) -> Placement {
        let (w, h) = self.unit_size();
        let s = self.scale();
        let (aw, ah) = self.area;
        let (dw, dh) = (w * s, h * s);
        // A page taller than the area starts at its top.
        let top = if self.vector_size().is_some() {
            ah / s / 2.0
        } else {
            h / 2.0
        };
        let (cx, cy) = self.center.unwrap_or((w / 2.0, top));
        let along = |d: f32, a: f32, c: f32| {
            if d <= a {
                (a - d) / 2.0
            } else {
                (a / 2.0 - c * s).clamp(a - d, 0.0)
            }
        };
        Placement {
            x: along(dw, aw, cx),
            y: along(dh, ah, cy),
            width: dw,
            height: dh,
            scale: s,
        }
    }

    /// The bitmap's point at the area's center, as placed.
    fn shown_center(&mut self) -> (f32, f32) {
        let p = self.placement();
        let (aw, ah) = self.area;
        ((aw / 2.0 - p.x) / p.scale, (ah / 2.0 - p.y) / p.scale)
    }

    /// Zooms by `factor`, keeping the center where it is.
    pub fn zoom_by(&mut self, factor: f32) {
        let c = self.shown_center();
        let s = (self.scale() * factor).clamp(MIN_SCALE, MAX_SCALE);
        self.zoom = Zoom::Scale(s);
        self.center = Some(c);
    }

    /// Zooms by `factor` keeping the bitmap's point under (`x`, `y`) of
    /// the area there (the mouse wheel).
    pub fn zoom_at(&mut self, factor: f32, x: f32, y: f32) {
        let p = self.placement();
        let (bx, by) = ((x - p.x) / p.scale, (y - p.y) / p.scale);
        let s = (p.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let (aw, ah) = self.area;
        self.zoom = Zoom::Scale(s);
        self.center = Some((bx + (aw / 2.0 - x) / s, by + (ah / 2.0 - y) / s));
        // Kept inside, as placed.
        let c = self.shown_center();
        self.center = Some(c);
    }

    /// Shows the unit at its own size.
    pub fn actual_size(&mut self) {
        let c = self.shown_center();
        self.zoom = Zoom::Scale(1.0);
        self.center = Some(c);
    }

    /// Fits the unit in the area.
    pub fn fit(&mut self) {
        self.zoom = Zoom::Fit;
        self.center = None;
    }

    /// Fits the unit's width to the area, from its top.
    pub fn fit_width(&mut self) {
        self.zoom = Zoom::FitWidth;
        self.center = None;
    }

    /// Scrolls by (`dx`, `dy`) area pixels as [`ViewerState::pan`] does;
    /// pushed on past a page's bottom (top) by a fifth of the area, it
    /// shows the next page's top (the previous page's bottom).
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        let before = self.placement();
        self.pan(dx, dy);
        let after = self.placement();
        let paged_pages = self.paged() && self.vector_size().is_some();
        if !paged_pages || dy == 0.0 || (after.y - before.y).abs() > 0.5 {
            self.overscroll = 0.0;
            return;
        }
        if self.overscroll.signum() != dy.signum() {
            self.overscroll = 0.0;
        }
        self.overscroll += dy;
        if self.overscroll.abs() < self.area.1 / 5.0 {
            return;
        }
        self.overscroll = 0.0;
        if dy > 0.0 {
            self.go_to(self.unit + 1);
        } else if self.unit > 0 && self.go_to(self.unit - 1) {
            let (w, h) = self.unit_size();
            let s = self.scale();
            self.center = Some((w / 2.0, h - self.area.1 / s / 2.0));
        }
    }

    /// Moves the view by (`dx`, `dy`) area pixels: the picture moves the
    /// other way.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let s = self.scale();
        let (cx, cy) = self.shown_center();
        self.center = Some((cx + dx / s, cy + dy / s));
        let c = self.shown_center();
        self.center = Some(c);
    }

    /// The point of the unit under (`x`, `y`) of the area, in the unit's
    /// own pixels before the view's turn (PDF points for a PDF's page).
    pub fn unit_point(&mut self, x: f32, y: f32) -> (f32, f32) {
        let p = self.placement();
        let (ux, uy) = ((x - p.x) / p.scale, (y - p.y) / p.scale);
        let (tw, th) = self.unit_size();
        match self.rotation % 4 {
            1 => (uy, tw - ux),
            2 => (tw - ux, th - uy),
            3 => (th - uy, ux),
            _ => (ux, uy),
        }
    }

    /// [`ViewerState::link_at`] without waiting, for the pointer's hover:
    /// none while a render holds the document.
    pub fn link_under(&mut self, x: f32, y: f32) -> Option<String> {
        let (ox, oy) = self.unit_point(x, y);
        let doc = self.doc.try_lock().ok()?;
        doc.links(self.unit)
            .into_iter()
            .find(|l| {
                let [lx, ly, lw, lh] = l.rect;
                ox >= lx && ox <= lx + lw && oy >= ly && oy <= ly + lh
            })
            .map(|l| l.target)
    }

    /// The target of the link under (`x`, `y`) of the area, if any: a
    /// URL, a file, or `#N` for unit N.
    pub fn link_at(&mut self, x: f32, y: f32) -> Option<String> {
        let (ox, oy) = self.unit_point(x, y);
        self.doc()
            .links(self.unit)
            .into_iter()
            .find(|l| {
                let [lx, ly, lw, lh] = l.rect;
                ox >= lx && ox <= lx + lw && oy >= ly && oy <= ly + lh
            })
            .map(|l| l.target)
    }

    /// Whether (`x`, `y`) of the area is on the unit's text: a drag there
    /// selects, elsewhere it pans. False while a render holds the
    /// document.
    pub fn text_hit(&mut self, x: f32, y: f32) -> bool {
        let (ux, uy) = self.unit_point(x, y);
        let Ok(doc) = self.doc.try_lock() else {
            return false;
        };
        doc.text_at(self.unit, ux, uy)
            .is_some_and(|(_, [bx, by, bw, bh])| {
                ux >= bx - 1.0 && ux <= bx + bw + 1.0 && uy >= by - 1.0 && uy <= by + bh + 1.0
            })
    }

    /// Selects the word at (`x`, `y`) of the area (a double click):
    /// letters, digits and `_` around the glyph there, or the glyph alone
    /// when it is none of them; with `line`, the whole line (a triple
    /// click). True when the point is on the unit's text.
    pub fn select_word(&mut self, x: f32, y: f32, line: bool) -> bool {
        self.text_sel = None;
        if !self.text_hit(x, y) {
            return false;
        }
        let (ux, uy) = self.unit_point(x, y);
        let (hit, text) = {
            let doc = self.doc();
            (doc.text_at(self.unit, ux, uy), doc.text(self.unit))
        };
        let Some((r, _)) = hit else {
            return false;
        };
        let range = if line {
            line_around(&text, r)
        } else {
            word_around(&text, r)
        };
        self.text_sel = Some(TextSelection {
            unit: self.unit,
            anchor: range.clone(),
            head: range,
            rects: None,
            pending: None,
        });
        true
    }

    /// Starts selecting text at (`x`, `y`) of the area, when it is on the
    /// unit's text; true when it is.
    pub fn select_from(&mut self, x: f32, y: f32) -> bool {
        self.text_sel = None;
        if !self.text_hit(x, y) {
            return false;
        }
        let (ux, uy) = self.unit_point(x, y);
        let Some((r, _)) = self.doc().text_at(self.unit, ux, uy) else {
            return false;
        };
        self.text_sel = Some(TextSelection {
            unit: self.unit,
            anchor: r.clone(),
            head: r,
            rects: None,
            pending: None,
        });
        true
    }

    /// Extends the selection to the glyph at or nearest (`x`, `y`).
    pub fn select_to(&mut self, x: f32, y: f32) {
        if self.text_sel.is_none() {
            return;
        }
        let (ux, uy) = self.unit_point(x, y);
        if let Some(sel) = &mut self.text_sel {
            sel.pending = Some((ux, uy));
        }
        self.settle_selection();
    }

    /// Moves the selection's head to where the drag went, once no render
    /// holds the document (a drag over a page while its neighbor renders
    /// is not lost).
    fn settle_selection(&mut self) {
        let Some(sel) = &mut self.text_sel else {
            return;
        };
        let Some((ux, uy)) = sel.pending else {
            return;
        };
        let Ok(doc) = self.doc.try_lock() else {
            return;
        };
        let hit = doc.text_at(sel.unit, ux, uy);
        drop(doc);
        sel.pending = None;
        if let Some((r, _)) = hit {
            sel.head = r;
        }
    }

    /// Drops the text selection.
    pub fn clear_text_selection(&mut self) {
        self.text_sel = None;
    }

    /// The text selected, if any.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.text_sel.as_ref()?;
        let doc = self.doc();
        let mut range = sel.range();
        // A drag not applied yet, a render holding the document then.
        if let Some((ux, uy)) = sel.pending
            && let Some((head, _)) = doc.text_at(sel.unit, ux, uy)
        {
            range = sel.anchor.start.min(head.start)..sel.anchor.end.max(head.end);
        }
        doc.text(sel.unit).get(range).map(str::to_string)
    }

    /// The selection's rectangles in the area's pixels as placed (x, y,
    /// width, height); read from the viewer once per range, none in a
    /// frame a render holds the document.
    pub fn selection_marks(&mut self) -> Vec<[f32; 4]> {
        self.settle_selection();
        let Some(sel) = &mut self.text_sel else {
            return Vec::new();
        };
        let range = sel.range();
        if sel.rects.as_ref().is_none_or(|(r, _)| *r != range) {
            let Ok(doc) = self.doc.try_lock() else {
                return Vec::new();
            };
            sel.rects = Some((range.clone(), doc.text_rects(sel.unit, range)));
        }
        let rects = sel
            .rects
            .as_ref()
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        let p = self.placement();
        rects
            .into_iter()
            .map(|rc| {
                let [x, y, w, h] = self.turned(rc);
                [
                    p.x + x * p.scale,
                    p.y + y * p.scale,
                    w * p.scale,
                    h * p.scale,
                ]
            })
            .collect()
    }

    /// Follows a link's target: `#N` shows unit N (from its top) and gives
    /// `None`; anything else is given back for the frontend to open.
    pub fn follow(&mut self, target: &str) -> Option<String> {
        match target
            .strip_prefix('#')
            .and_then(|n| n.parse::<usize>().ok())
        {
            Some(unit) => {
                self.go_to(unit);
                None
            }
            None => Some(target.to_string()),
        }
    }

    /// Searches the units' text for `query` (case folded) on a thread,
    /// from the unit shown on; a search of another query stops. An empty
    /// query clears the search.
    pub fn search_start(&mut self, query: &str) {
        if self.search.as_ref().is_some_and(|s| s.query == query) {
            return;
        }
        // The old thread stops when its receiver is gone.
        self.search = None;
        if query.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        let doc = self.doc.clone();
        let n = self.structure.units.len();
        let origin = self.unit;
        let needle = query.to_lowercase();
        std::thread::spawn(move || {
            for i in 0..n {
                let unit = (origin + i) % n;
                // The lock for one unit's text at a time.
                let text = doc.lock().unwrap_or_else(|e| e.into_inner()).text(unit);
                if tx.send((unit, find_folded(&text, &needle))).is_err() {
                    return;
                }
            }
        });
        self.search = Some(Search {
            query: query.to_string(),
            hits: Vec::new(),
            current: None,
            origin,
            scanned: 0,
            rx: Some(rx),
            marks: None,
        });
    }

    /// Takes what the search thread found; the first match from the unit
    /// the search started at is shown as soon as it is found. True when
    /// anything changed.
    pub fn search_poll(&mut self) -> bool {
        let Some(s) = &mut self.search else {
            return false;
        };
        let Some(rx) = &s.rx else {
            return false;
        };
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok((unit, ranges)) => {
                    s.scanned += 1;
                    changed = true;
                    if ranges.is_empty() {
                        continue;
                    }
                    // Kept in document order; the shown match follows.
                    let shown = s.current.map(|i| s.hits[i].clone());
                    s.hits.extend(ranges.into_iter().map(|r| (unit, r)));
                    s.hits.sort_by_key(|(u, r)| (*u, r.start));
                    s.current = shown.and_then(|h| s.hits.iter().position(|x| *x == h));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    s.rx = None;
                    changed = true;
                    break;
                }
            }
        }
        if s.current.is_none() && !s.hits.is_empty() {
            // The first match at or after the origin, else the first one.
            let i = s.hits.iter().position(|(u, _)| *u >= s.origin).unwrap_or(0);
            s.current = Some(i);
            let unit = s.hits[i].0;
            self.go_to(unit);
            self.reveal_match();
        }
        changed
    }

    /// Whether the search thread is still reading units.
    pub fn searching(&self) -> bool {
        self.search.as_ref().is_some_and(|s| s.rx.is_some())
    }

    /// Shows the next match (the previous one with `backward`), round the
    /// document.
    pub fn search_next(&mut self, backward: bool) {
        let Some(s) = &mut self.search else {
            return;
        };
        let n = s.hits.len();
        if n == 0 {
            return;
        }
        let i = match (s.current, backward) {
            (None, _) => 0,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        s.current = Some(i);
        let unit = s.hits[i].0;
        self.go_to(unit);
        self.reveal_match();
    }

    /// Scrolls the match shown into view when it is not.
    fn reveal_match(&mut self) {
        let Some((unit, range)) = self
            .search
            .as_ref()
            .and_then(|s| s.hits.get(s.current?).cloned())
        else {
            return;
        };
        if unit != self.unit {
            return;
        }
        let rects = self.doc().text_rects(unit, range);
        let Some(first) = rects.first() else {
            return;
        };
        let [x, y, w, h] = self.turned(*first);
        let p = self.placement();
        let (aw, ah) = self.area;
        let (top, bottom) = (p.y + y * p.scale, p.y + (y + h) * p.scale);
        let (left, right) = (p.x + x * p.scale, p.x + (x + w) * p.scale);
        if top >= 0.0 && bottom <= ah && left >= 0.0 && right <= aw {
            return;
        }
        let (cx, _) = self.shown_center();
        let cx = if left < 0.0 || right > aw {
            x + w / 2.0
        } else {
            cx
        };
        self.center = Some((cx, y + h / 2.0));
    }

    /// A rectangle of the unit's own pixels in the view's, turned as the
    /// view is.
    fn turned(&mut self, [x, y, w, h]: [f32; 4]) -> [f32; 4] {
        let (tw, th) = self.unit_size();
        let (uw, uh) = if self.rotation % 2 == 1 {
            (th, tw)
        } else {
            (tw, th)
        };
        match self.rotation % 4 {
            1 => [uh - y - h, x, h, w],
            2 => [uw - x - w, uh - y - h, w, h],
            3 => [y, uw - x - w, h, w],
            _ => [x, y, w, h],
        }
    }

    /// The find bar's matches on the unit shown, in the area's pixels as
    /// placed (x, y, width, height), the one shown marked `true`. Read
    /// from the viewer once for each match shown; while a render holds
    /// the document, none this frame.
    pub fn search_marks(&mut self) -> Vec<([f32; 4], bool)> {
        let unit = self.unit;
        let Some(s) = &mut self.search else {
            return Vec::new();
        };
        let key = (unit, s.current, s.hits.len());
        if s.marks.as_ref().is_none_or(|(k, _)| *k != key) {
            let Ok(doc) = self.doc.try_lock() else {
                return Vec::new();
            };
            let mut marks = Vec::new();
            for (i, (u, r)) in s.hits.iter().enumerate() {
                if *u == unit {
                    let shown = s.current == Some(i);
                    marks.extend(
                        doc.text_rects(*u, r.clone())
                            .into_iter()
                            .map(|rc| (rc, shown)),
                    );
                }
            }
            s.marks = Some((key, marks));
        }
        let marks = s.marks.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
        let p = self.placement();
        marks
            .into_iter()
            .map(|(rc, shown)| {
                let [x, y, w, h] = self.turned(rc);
                (
                    [
                        p.x + x * p.scale,
                        p.y + y * p.scale,
                        w * p.scale,
                        h * p.scale,
                    ],
                    shown,
                )
            })
            .collect()
    }

    /// The find bar's count: the match shown and how many there are, with
    /// "…" while units are still being read.
    pub fn search_status(&self) -> String {
        let Some(s) = &self.search else {
            return String::new();
        };
        let at = s.current.map_or(0, |i| i + 1);
        let more = if s.rx.is_some() { "…" } else { "" };
        format!("{at}/{}{more}", s.hits.len())
    }

    /// Ends the search (the find bar closed).
    pub fn search_end(&mut self) {
        self.search = None;
    }

    /// Turns the view by `quarters` clockwise.
    pub fn rotate(&mut self, quarters: i8) {
        self.rotation = (self.rotation as i8 + quarters).rem_euclid(4) as u8;
        self.center = None;
        if let Zoom::Scale(_) = self.zoom {
            self.zoom = Zoom::Fit;
        }
    }

    /// Whether the units are paged through (pages, sheets, slides), not
    /// played (frames) or single (a picture).
    pub fn paged(&self) -> bool {
        self.structure.units.len() > 1
            && !self
                .structure
                .units
                .iter()
                .any(|u| matches!(u.kind, UnitKind::Frame | UnitKind::Image))
    }

    /// Shows unit `unit`; false when there is none.
    pub fn go_to(&mut self, unit: usize) -> bool {
        if unit >= self.structure.units.len() || unit == self.unit {
            return false;
        }
        self.unit = unit;
        self.text_sel = None;
        if self.paged() {
            self.center = None;
        }
        true
    }

    /// Brings height `y` of the unit (its own pixels from the top, PDF
    /// points for a PDF's page) to the middle of the area when the unit is
    /// larger than the area (zoomed in); the view as it is otherwise.
    pub fn show_height(&mut self, y: f32) {
        if !self.rotation.is_multiple_of(4) {
            return;
        }
        let (cx, _) = self.shown_center();
        self.center = Some((cx, y));
        // Kept inside the unit, as panning keeps it.
        let c = self.shown_center();
        self.center = Some(c);
    }

    /// The next frame of an animation, and how long the frame shown now
    /// stays; `None` when nothing plays.
    pub fn frame_delay(&self) -> Option<u32> {
        if !self.playing || !self.structure.animated() {
            return None;
        }
        Some(
            self.structure.units[self.unit]
                .duration_ms
                .unwrap_or(100)
                .max(20),
        )
    }

    /// Shows the next frame (after [`ViewerState::frame_delay`]).
    pub fn advance_frame(&mut self) {
        let n = self.structure.units.len();
        if n > 1 {
            self.unit = (self.unit + 1) % n;
        }
    }

    /// The information panel's fields.
    /// The information panel's fields: read again after a change, and
    /// while a render holds the document the last ones (the panel is
    /// drawn every frame and must not wait).
    pub fn info_fields(&self) -> Vec<InfoField> {
        let mut cache = self.info_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((g, fields)) = &*cache
            && *g == self.generation
        {
            return fields.clone();
        }
        match self.doc.try_lock() {
            Ok(doc) => {
                let fields = doc.info();
                *cache = Some((self.generation, fields.clone()));
                fields
            }
            Err(_) => cache.as_ref().map(|(_, f)| f.clone()).unwrap_or_default(),
        }
    }

    /// The unit's text.
    pub fn text(&self) -> String {
        self.doc().text(self.unit)
    }

    /// The shown unit's text without waiting: the last one read while a
    /// render holds the document (a terminal draws it on every frame).
    pub fn text_now(&self) -> String {
        let key = (self.unit, self.generation);
        let mut cache = self.text_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(doc) = self.doc.try_lock()
            && cache.as_ref().is_none_or(|(k, _)| *k != key)
        {
            *cache = Some((key, doc.text(self.unit)));
        }
        cache.as_ref().map(|(_, t)| t.clone()).unwrap_or_default()
    }

    /// The edits the format allows on the unit shown.
    pub fn edits(&self) -> Vec<Edit> {
        // Asked for every key's context (`viewerEditable`): never waits for
        // a render; the last list read stands meanwhile.
        let key = (self.unit, self.generation);
        let mut cache = self.edits_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(doc) = self.doc.try_lock()
            && cache.as_ref().is_none_or(|(k, _)| *k != key)
        {
            *cache = Some((key, doc.edits(self.unit)));
        }
        cache.as_ref().map(|(_, e)| e.clone()).unwrap_or_default()
    }

    /// Applies edit `id`, recording its inverse for undo.
    pub fn apply(&mut self, id: &str) -> Result<(), String> {
        let edit = self
            .edits()
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("No edit {id}"))?;
        self.doc().apply(id).map_err(|e| e.to_string())?;
        if let Some(inv) = edit.inverse {
            self.undo.push((inv, id.to_string()));
        } else {
            // An edit without an inverse cannot be undone past.
            self.undo.clear();
        }
        self.redo.clear();
        let structure = self.doc().structure();
        self.structure = structure;
        self.changed();
        Ok(())
    }

    /// Undoes the last edit; false when there is none.
    pub fn undo(&mut self) -> Result<bool, String> {
        if self.doc().has_history() {
            let done = self.doc().undo().map_err(|e| e.to_string())?;
            self.refresh();
            return Ok(done);
        }
        let Some((inverse, again)) = self.undo.pop() else {
            return Ok(false);
        };
        self.doc().apply(&inverse).map_err(|e| e.to_string())?;
        self.redo.push((inverse, again));
        self.changed();
        Ok(true)
    }

    /// Redoes the last edit undone; false when there is none.
    pub fn redo(&mut self) -> Result<bool, String> {
        if self.doc().has_history() {
            let done = self.doc().redo().map_err(|e| e.to_string())?;
            self.refresh();
            return Ok(done);
        }
        let Some((inverse, again)) = self.redo.pop() else {
            return Ok(false);
        };
        self.doc().apply(&again).map_err(|e| e.to_string())?;
        self.undo.push((inverse, again));
        self.changed();
        Ok(true)
    }

    /// Whether there are edits not saved.
    pub fn modified(&self) -> bool {
        // Asked on every frame (the title, the status bar, the list of
        // open files): never waits for a render.
        use std::sync::atomic::Ordering;
        if let Ok(doc) = self.doc.try_lock() {
            self.modified_cache.store(doc.modified(), Ordering::Relaxed);
        }
        self.modified_cache.load(Ordering::Relaxed)
    }

    /// Whether the viewer is the workbook viewer (it writes `.xlsx`).
    pub fn is_workbook(&self) -> bool {
        self.viewer.extensions().contains(&"xlsx")
    }

    /// The bytes of the file as `extension` asks: the viewer's own for
    /// its format, a workbook written as `.ods`, or one the viewer only
    /// shows (`.xls`, `.xlsb`, `.ods`) made an `.xlsx`.
    pub fn save_as_format(&mut self, extension: &str) -> Result<Vec<u8>, String> {
        let ext = extension.to_ascii_lowercase();
        if !self.is_workbook() {
            return self.save().map(|o| o.bytes);
        }
        let editable = self.grid_layout_of(0).is_some_and(|l| l.editable);
        match ext.as_str() {
            "ods" => {
                let mut doc = self.doc();
                let bytes = crate::workbook_io::to_ods(doc.as_mut())?;
                // The workbook's own save marks it saved; its bytes are not
                // what is written.
                doc.save().map_err(|e| e.to_string())?;
                Ok(bytes)
            }
            "xlsx" | "xlsm" | "xltx" | "xltm" if editable => self.save().map(|o| o.bytes),
            "xlsx" | "xlsm" | "xltx" | "xltm" => {
                let viewer = self.viewer.clone();
                let mut doc = self.doc();
                crate::workbook_io::to_xlsx(viewer.as_ref(), doc.as_mut())
            }
            "xls" | "xlsb" => Err(format!(
                "Kalem does not write .{ext} files: save it as .xlsx or .ods"
            )),
            _ => Err(format!("A workbook is saved as .xlsx or .ods, not .{ext}")),
        }
    }

    fn grid_layout_of(&self, unit: usize) -> Option<GridLayout> {
        self.doc().grid(unit)
    }

    /// The file with the edits.
    pub fn save(&mut self) -> Result<SaveOutput, String> {
        self.doc().save().map_err(|e| e.to_string())
    }

    /// What the status bar says: the size, the zoom, the unit; for a grid,
    /// the sheet, the cell and its note.
    pub fn status(&mut self) -> String {
        if self.is_grid() {
            let p = self.grid_pos();
            let mut parts = vec![self.structure.units[self.unit].label.clone()];
            parts.push(self.selection_name());
            let n = self.structure.units.len();
            if n > 1 {
                parts.push(format!("{}/{n}", self.unit + 1));
            }
            if let Some(sums) = self.selection_sums() {
                parts.push(sums);
            }
            if self.doc().calc_options().mode == kalem_viewer::CalcMode::Manual {
                parts.push("Manual calculation".into());
            }
            if let Some(&(u, r, c)) = self.circular_references().first() {
                let at = format!("{}{}", crate::csv_tools::column_letters(c as usize), r + 1);
                let sheet = if u == self.unit {
                    String::new()
                } else {
                    format!("{}!", self.structure.units[u].label)
                };
                parts.push(format!("Circular references: {sheet}{at}"));
            }
            let view = self.sheet_view();
            if view.zoom != 100 {
                parts.push(format!("{}%", view.zoom));
            }
            if let Some(b) = self.page_breaks() {
                let n = b.pages();
                parts.push(format!(
                    "Page Break Preview: {n} page{}",
                    if n == 1 { "" } else { "s" }
                ));
            }
            if let Some(note) = self.doc().cell_note(self.unit, p.row, p.col) {
                parts.push(note.lines().next().unwrap_or_default().to_string());
            }
            if let Some(t) = self.cursor_thread()
                && let Some(first) = t.comments.first()
            {
                let mut line = format!(
                    "{}: {}",
                    first.author,
                    first.text.lines().next().unwrap_or_default()
                );
                match t.comments.len() - 1 {
                    0 => {}
                    1 => line.push_str(" (1 reply)"),
                    n => line.push_str(&format!(" ({n} replies)")),
                }
                if t.done {
                    line.push_str(" (resolved)");
                }
                parts.push(line);
            }
            // The validation's input message, as Excel shows it by the cell.
            if let Some((title, text)) = self.cursor_validation().and_then(|v| v.prompt) {
                let text = text.lines().next().unwrap_or_default().to_string();
                parts.push(if title.is_empty() {
                    text
                } else {
                    format!("{title}: {text}")
                });
            }
            return parts.join(" · ");
        }
        let mut parts = Vec::new();
        let (w, h) = self.unit_size();
        parts.push(format!("{} × {}", w.round(), h.round()));
        parts.push(format!("{:.0}%", self.scale() * 100.0));
        let n = self.structure.units.len();
        if n > 1 {
            parts.push(format!("{}/{n}", self.unit + 1));
        }
        parts.join(" · ")
    }
}

impl ViewerState {
    /// After the document changed: its units, which are grids, the cache.
    fn refresh(&mut self) {
        let structure = self.doc().structure();
        self.structure = structure;
        let n = self.structure.units.len();
        self.grids = (0..n).map(|u| self.doc().grid(u).is_some()).collect();
        self.sizes = (0..n).map(|u| self.doc().size(u)).collect();
        if self.unit >= n {
            self.unit = n.saturating_sub(1);
        }
        self.grid_cache = None;
        self.changed();
    }
    /// Whether unit `unit` is a grid (a worksheet).
    pub fn is_grid_unit(&self, unit: usize) -> bool {
        self.grids.get(unit).copied().unwrap_or(false)
    }

    /// Whether the unit shown is a grid (a sheet, a table).
    pub fn is_grid(&self) -> bool {
        self.grids.get(self.unit).copied().unwrap_or(false)
    }

    /// The grid shown's layout.
    pub fn grid_layout(&mut self) -> Option<GridLayout> {
        if !self.is_grid() {
            return None;
        }
        if let Some((u, g, l)) = &self.grid_cache
            && (*u, *g) == (self.unit, self.generation)
        {
            return Some(l.clone());
        }
        let l = self.doc().grid(self.unit)?;
        self.grid_cache = Some((self.unit, self.generation, l.clone()));
        Some(l)
    }

    /// Whether the grid shown can be edited.
    pub fn grid_editable(&mut self) -> bool {
        self.grid_layout().is_some_and(|l| l.editable)
    }

    /// The cells of the grid shown in `rows` × `cols`; with Show Formulas,
    /// a formula cell's formula as its text, at the left as Excel shows it.
    pub fn grid_cells(
        &mut self,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32, GridCell)> {
        let mut cells = self.doc().grid_cells(self.unit, rows, cols);
        if self.show_formulas {
            let unit = self.unit;
            let mut doc = self.doc();
            for c in cells.iter_mut().filter(|c| c.2.formula) {
                c.2.text = doc.cell_input(unit, c.0, c.1);
                c.2.numeric = false;
                c.2.align = kalem_viewer::Align::General;
            }
        }
        cells
    }

    /// The cursor and scroll of the grid shown.
    pub fn grid_pos(&self) -> GridPos {
        self.grid_pos.get(&self.unit).copied().unwrap_or_default()
    }

    /// Tells the state how many rows and columns the frontend shows,
    /// frozen ones included, for paging and keeping the cursor in view.
    pub fn set_grid_visible(&mut self, rows: u32, cols: u32) {
        let now = (rows.max(1), cols.max(1));
        if now == self.grid_visible {
            // The view scrolled by the wheel stays where it is, the cursor
            // out of it as in a spreadsheet; it comes back when the cursor
            // moves.
            return;
        }
        self.grid_visible = now;
        let p = self.grid_pos();
        self.place(p.row, p.col);
    }

    /// Scrolls by whole rows and columns, the cursor kept.
    pub fn grid_scroll(&mut self, rows: i64, cols: i64) {
        let Some(l) = self.grid_layout() else { return };
        let panes = self.panes();
        let (min_top, min_left) = if panes.split {
            (0, 0)
        } else {
            (panes.rows.1, panes.cols.1)
        };
        let mut p = self.grid_pos();
        p.top = (i64::from(p.top) + rows)
            .clamp(i64::from(min_top), i64::from(l.max_rows.saturating_sub(1)))
            as u32;
        p.left = (i64::from(p.left) + cols)
            .clamp(i64::from(min_left), i64::from(l.max_cols.saturating_sub(1)))
            as u32;
        self.grid_pos.insert(self.unit, p);
    }

    /// Puts the cursor on a cell and scrolls it into view; the selection
    /// goes; a cell inside a merged one is its first cell.
    pub fn grid_move_to(&mut self, row: u32, col: u32) {
        self.areas.clear();
        let (row, col) = self.merge_at(row, col).map_or((row, col), |m| (m[0], m[1]));
        let mut p = self.grid_pos();
        p.sel = None;
        self.grid_pos.insert(self.unit, p);
        self.place(row, col);
    }

    /// The merged range holding a cell: first row, first column, last
    /// row, last column.
    pub fn merge_at(&mut self, row: u32, col: u32) -> Option<[u32; 4]> {
        self.grid_layout()?
            .merged
            .into_iter()
            .find(|m| (m[0]..=m[2]).contains(&row) && (m[1]..=m[3]).contains(&col))
    }

    /// Moves the cursor's other corner (the selection kept) and scrolls it
    /// into view.
    fn place(&mut self, row: u32, col: u32) {
        let Some(l) = self.grid_layout() else { return };
        let mut p = self.grid_pos();
        p.row = row.min(l.max_rows.saturating_sub(1));
        p.col = col.min(l.max_cols.saturating_sub(1));
        let panes = self.panes();
        let (fr, fc) = (panes.rows.1, panes.cols.1);
        let rows = self.grid_visible.0.saturating_sub(fr).max(1);
        let cols = self.grid_visible.1.saturating_sub(fc).max(1);
        // Frozen rows stay out of the main pane; a split's may show any.
        let (min_top, min_left) = if panes.split { (0, 0) } else { (fr, fc) };
        p.top = p.top.max(min_top);
        p.left = p.left.max(min_left);
        if p.row >= min_top {
            if p.row < p.top {
                p.top = p.row;
            } else if p.row >= p.top + rows {
                p.top = p.row + 1 - rows;
            }
        }
        if p.col >= min_left {
            if p.col < p.left {
                p.left = p.col;
            } else if p.col >= p.left + cols {
                p.left = p.col + 1 - cols;
            }
        }
        self.grid_pos.insert(self.unit, p);
    }

    /// Moves the cursor by rows and columns, over hidden ones.
    pub fn grid_move_by(&mut self, rows: i64, cols: i64) {
        let Some(l) = self.grid_layout() else { return };
        let p = self.grid_pos();
        let step = |v: u32, d: i64, max: u32, hidden: &[u32]| -> u32 {
            let mut x = i64::from(v);
            let dir = d.signum();
            let mut left = d.abs();
            while left > 0 {
                let n = x + dir;
                if n < 0 || n >= i64::from(max) {
                    break;
                }
                x = n;
                if !hidden.contains(&(n as u32)) {
                    left -= 1;
                }
            }
            x as u32
        };
        // From a merged cell, forward moves leave from its last row or column.
        let (fr, fc) = match self.merge_at(p.row, p.col) {
            Some(m) => (
                if rows > 0 { m[2] } else { p.row },
                if cols > 0 { m[3] } else { p.col },
            ),
            None => (p.row, p.col),
        };
        let r = step(fr, rows, l.max_rows, &l.hidden_rows);
        let c = step(fc, cols, l.max_cols, &l.hidden_cols);
        self.grid_move_to(
            if rows == 0 { p.row } else { r },
            if cols == 0 { p.col } else { c },
        );
    }

    /// Extends the selection by rows and columns (Shift and an arrow).
    pub fn grid_select_by(&mut self, rows: i64, cols: i64) {
        let Some(l) = self.grid_layout() else { return };
        let p = self.grid_pos();
        let r = (i64::from(p.row) + rows).clamp(0, i64::from(l.max_rows.saturating_sub(1))) as u32;
        let c = (i64::from(p.col) + cols).clamp(0, i64::from(l.max_cols.saturating_sub(1))) as u32;
        self.grid_extend_to(r, c);
    }

    /// Selects from the selection's start (the cursor, when none) to a cell
    /// (Shift and a click, a drag).
    pub fn grid_extend_to(&mut self, row: u32, col: u32) {
        self.areas.clear();
        let mut p = self.grid_pos();
        if p.sel.is_none() {
            p.sel = Some((p.row, p.col));
            self.grid_pos.insert(self.unit, p);
        }
        self.place(row, col);
    }

    /// The selected range, merged cells it touches included: first row,
    /// first column, last row, last column.
    pub fn selection(&mut self) -> [u32; 4] {
        let p = self.grid_pos();
        let (ar, ac) = p.sel.unwrap_or((p.row, p.col));
        let mut r = [ar.min(p.row), ac.min(p.col), ar.max(p.row), ac.max(p.col)];
        let merged = self.grid_layout().map(|l| l.merged).unwrap_or_default();
        loop {
            let mut grown = r;
            for m in &merged {
                if m[0] <= r[2] && r[0] <= m[2] && m[1] <= r[3] && r[1] <= m[3] {
                    grown = [
                        grown[0].min(m[0]),
                        grown[1].min(m[1]),
                        grown[2].max(m[2]),
                        grown[3].max(m[3]),
                    ];
                }
            }
            if grown == r {
                return r;
            }
            r = grown;
        }
    }

    /// The selection's name: `B2`, or `B2:D5`.
    pub fn selection_name(&mut self) -> String {
        let s = self.selection();
        let name =
            |r: u32, c: u32| format!("{}{}", crate::csv_tools::column_letters(c as usize), r + 1);
        if (s[0], s[1]) == (s[2], s[3]) || self.grid_pos().sel.is_none() {
            let p = self.grid_pos();
            name(p.row, p.col)
        } else {
            format!("{}:{}", name(s[0], s[1]), name(s[2], s[3]))
        }
    }

    /// Whether cells of the selection besides its first hold a value, which
    /// merging clears.
    pub fn selection_loses_values(&mut self) -> bool {
        let s = self.selection();
        self.doc()
            .grid_cells(self.unit, s[0]..s[2] + 1, s[1]..s[3] + 1)
            .iter()
            .any(|(r, c, cell)| (*r, *c) != (s[0], s[1]) && !cell.text.is_empty())
    }

    /// Pastes tab-separated text (what spreadsheets copy) from the
    /// selection's first cell on, each value as typed; one value into a
    /// larger selection fills it, as in Excel. The pasted cells are
    /// selected afterwards.
    pub fn paste_text(&mut self, text: &str) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let mut values = parse_tsv(text);
        if values.is_empty() {
            return Ok(());
        }
        let s = self.selection();
        // The text of a cut: the cells move, to this sheet from any.
        let norm = |t: &str| t.replace("\r\n", "\n").trim_end_matches('\n').to_string();
        if let Some((unit, range, cut_text)) = self.cut.clone()
            && norm(&cut_text) == norm(text)
        {
            // From this sheet or another: the cells move.
            self.doc()
                .move_cells_between(unit, range, self.unit, s[0], s[1])
                .map_err(|e| e.to_string())?;
            self.cut = None;
            self.refresh();
            self.grid_move_to(s[0], s[1]);
            let (rows, cols) = (range[2] - range[0], range[3] - range[1]);
            if rows > 0 || cols > 0 {
                self.grid_extend_to(s[0] + rows, s[1] + cols);
            }
            return Ok(());
        }
        if values.len() == 1 && values[0].len() == 1 && (s[0], s[1]) != (s[2], s[3]) {
            let v = values[0][0].clone();
            values = vec![vec![v; (s[3] - s[1] + 1) as usize]; (s[2] - s[0] + 1) as usize];
        }
        self.doc()
            .set_cells(self.unit, s[0], s[1], &values)
            .map_err(|e| e.to_string())?;
        self.refresh();
        let rows = values.len() as u32;
        let cols = values.iter().map(Vec::len).max().unwrap_or(1) as u32;
        self.grid_move_to(s[0], s[1]);
        if rows > 1 || cols > 1 {
            self.grid_extend_to(s[0] + rows - 1, s[1] + cols - 1);
        }
        Ok(())
    }

    /// Cuts the selection: its text for the clipboard, the range kept so
    /// that pasting that text moves the cells, as in Excel.
    pub fn cut_selection(&mut self) -> Result<String, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let text = self.selection_tsv();
        self.cut = Some((self.unit, self.selection(), text.clone()));
        Ok(text)
    }

    /// The range cut on the unit shown, until it is pasted or cancelled.
    pub fn cut_range(&self) -> Option<[u32; 4]> {
        self.cut.as_ref().filter(|c| c.0 == self.unit).map(|c| c.1)
    }

    /// Forgets the cut, and Format Painter's format (Escape).
    pub fn cancel_cut(&mut self) {
        self.cut = None;
        self.painter = None;
    }

    /// What Sort and Filter work on: the selection when it is more than a
    /// cell, the filter's range when the cursor is in it, else the current
    /// region; and whether its first row is the headers.
    fn table_target(&mut self) -> ([u32; 4], bool) {
        let p = self.grid_pos();
        let s = self.selection();
        if p.sel.is_some() && (s[0], s[1]) != (s[2], s[3]) {
            return (s, self.looks_like_header(s));
        }
        if let Some(f) = self.grid_layout().and_then(|l| l.filter)
            && (f[0]..=f[2]).contains(&p.row)
            && (f[1]..=f[3]).contains(&p.col)
        {
            return (f, true);
        }
        let r = self.current_region();
        (r, self.looks_like_header(r))
    }

    /// Excel's guess: the first row is headers when it holds only text
    /// over a row that holds something else, or is bold over a row that
    /// is not.
    fn looks_like_header(&mut self, r: [u32; 4]) -> bool {
        if r[0] == r[2] {
            return false;
        }
        let cells = self
            .doc()
            .grid_cells(self.unit, r[0]..r[0] + 2, r[1]..r[3] + 1);
        let first: Vec<_> = cells
            .iter()
            .filter(|c| c.0 == r[0] && !c.2.text.is_empty())
            .collect();
        let second: Vec<_> = cells
            .iter()
            .filter(|c| c.0 == r[0] + 1 && !c.2.text.is_empty())
            .collect();
        if first.is_empty() {
            return false;
        }
        let all_text = first.iter().all(|c| !c.2.numeric);
        let bold = first.iter().all(|c| c.2.bold) && !second.iter().all(|c| c.2.bold);
        (all_text && second.iter().any(|c| c.2.numeric)) || bold
    }

    /// Sorts the table at the cursor by the cursor's column.
    pub fn sort(&mut self, descending: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (r, header) = self.table_target();
        // The active cell's column: where a selection began, as in Excel.
        let p = self.grid_pos();
        let key = p.sel.map_or(p.col, |s| s.1).clamp(r[1], r[3]);
        self.doc()
            .sort_range(self.unit, r, key, descending, header)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Turns the filter on (on the table at the cursor) or off.
    pub fn toggle_filter(&mut self) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let on = self.grid_layout().and_then(|l| l.filter).is_none();
        let range = if on {
            let (r, _) = self.table_target();
            if r[0] == r[2] {
                return Err("A filter needs a header row and rows under it".into());
            }
            Some(r)
        } else {
            None
        };
        self.doc()
            .set_filter(self.unit, range)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Custom Sort of the table at the cursor by several columns in turn.
    pub fn sort_by(&mut self, keys: &[kalem_viewer::SortKey]) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (r, header) = self.table_target();
        self.doc()
            .sort_range_by(self.unit, r, keys, header)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The rows that sum up a group of rows (the row after it), each with
    /// whether its group is collapsed: where the outline's −/+ go.
    pub fn outline_marks(&mut self) -> Vec<(u32, bool)> {
        let key = (self.unit, self.generation);
        if let Some((k, m)) = &self.outline_marks
            && *k == key
        {
            return m.clone();
        }
        let (rows, _) = self.doc().outline(self.unit);
        let level = |r: u32| rows.iter().find(|x| x.0 == r).map_or(0, |x| x.1);
        let hidden = self
            .grid_layout()
            .map(|l| l.hidden_rows)
            .unwrap_or_default();
        let marks: Vec<(u32, bool)> = rows
            .iter()
            .map(|x| x.0 + 1)
            .filter(|r| level(*r) < level(r - 1))
            .map(|r| (r, hidden.contains(&(r - 1))))
            .collect();
        self.outline_marks = Some((key, marks.clone()));
        marks
    }

    /// Whether the selection is whole columns (it spans every row).
    fn selects_columns(&mut self) -> bool {
        let s = self.selection();
        let max = self.grid_layout().map_or(u32::MAX, |l| l.max_rows);
        s[0] == 0 && s[2] + 1 >= max
    }

    /// Group (`deeper`) or Ungroup the selection's rows, or its columns
    /// when it is whole columns.
    pub fn group(&mut self, deeper: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        let cols = self.selects_columns();
        let (from, to) = if cols { (s[1], s[3]) } else { (s[0], s[2]) };
        self.doc()
            .set_outline(self.unit, !cols, from, to, deeper)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Hide Detail (`shown` off) or Show Detail of the row group at the
    /// cursor (the group it is in or sums up), else its column group.
    pub fn show_detail(&mut self, shown: bool) -> Result<(), String> {
        let p = self.grid_pos();
        let rows = self.doc().set_detail_shown(self.unit, true, p.row, shown);
        if rows.is_err() {
            self.doc()
                .set_detail_shown(self.unit, false, p.col, shown)
                .map_err(|e| e.to_string())?;
        }
        self.refresh();
        Ok(())
    }

    /// The outline's −/+ of summary row `row` pressed: its group collapsed
    /// or expanded.
    pub fn toggle_detail_at(&mut self, row: u32) -> Result<(), String> {
        let collapsed = self
            .outline_marks()
            .iter()
            .find(|m| m.0 == row)
            .is_some_and(|m| m.1);
        self.doc()
            .set_detail_shown(self.unit, true, row, collapsed)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Subtotal of the table at the cursor at each change in column `by`.
    pub fn subtotal(&mut self, by: u32, function: u32, columns: &[u32]) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (r, _) = self.table_target();
        self.doc()
            .subtotal(self.unit, r, by, function, columns)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The sheet shown's name, as references name it.
    fn sheet_name(&self) -> String {
        self.structure.units[self.unit]
            .label
            .trim_end_matches(" (hidden)")
            .to_owned()
    }

    /// Whether a reference is to the sheet shown.
    fn here(&self, r: &crate::formula_edit::Reference) -> bool {
        r.sheet
            .as_ref()
            .is_none_or(|s| s.eq_ignore_ascii_case(&self.sheet_name()))
    }

    /// Trace Precedents: arrows from the cells the cursor's formula reads
    /// (on this sheet); how many.
    pub fn trace_precedents(&mut self) -> usize {
        let p = self.grid_pos();
        let input = self.cell_input();
        if !input.starts_with('=') {
            return 0;
        }
        let refs: Vec<[u32; 4]> = crate::formula_edit::references(&input)
            .into_iter()
            .filter(|r| self.here(r))
            .map(|r| r.range)
            .collect();
        for r in &refs {
            if !self.arrows.contains(&(*r, (p.row, p.col))) {
                self.arrows.push((*r, (p.row, p.col)));
            }
        }
        refs.len()
    }

    /// Trace Dependents: arrows to the cells of this sheet whose formulas
    /// read the cursor's cell; how many.
    pub fn trace_dependents(&mut self) -> usize {
        let p = self.grid_pos();
        let Some(l) = self.grid_layout() else {
            return 0;
        };
        let mut found = Vec::new();
        let mut row = 0;
        while row < l.rows {
            let to = (row + 1000).min(l.rows);
            let formulas: Vec<(u32, u32)> = self
                .grid_cells(row..to, 0..l.cols.max(1))
                .into_iter()
                .filter(|c| c.2.formula)
                .map(|c| (c.0, c.1))
                .collect();
            for (r, c) in formulas {
                let input = self.doc().cell_input(self.unit, r, c);
                let reads = crate::formula_edit::references(&input)
                    .into_iter()
                    .any(|x| {
                        self.here(&x)
                            && (x.range[0]..=x.range[2]).contains(&p.row)
                            && (x.range[1]..=x.range[3]).contains(&p.col)
                    });
                if reads {
                    found.push((r, c));
                }
            }
            row = to;
        }
        let from = [p.row, p.col, p.row, p.col];
        for d in &found {
            if !self.arrows.contains(&(from, *d)) {
                self.arrows.push((from, *d));
            }
        }
        found.len()
    }

    /// Evaluate Formula: the cursor's formula's steps to its value.
    pub fn evaluation_steps(&mut self) -> Vec<String> {
        let input = self.cell_input();
        if !input.starts_with('=') {
            return Vec::new();
        }
        let unit = self.unit;
        let doc = self.doc.clone();
        let mut eval = |fs: &[String]| -> Vec<Option<String>> {
            doc.lock()
                .map(|mut d| d.evaluate_formulas(unit, fs))
                .unwrap_or_default()
        };
        crate::formula_edit::evaluation_steps(&input, &mut eval)
    }

    /// Error Checking: the cursor to the next cell after it (round the
    /// sheet) showing an error, and what the error means.
    pub fn next_error(&mut self) -> Option<String> {
        const ERRORS: [(&str, &str); 8] = [
            ("#DIV/0!", "A number is divided by zero"),
            ("#VALUE!", "A value is of the wrong type"),
            ("#REF!", "A reference is not valid"),
            ("#NAME?", "A name or function is not recognized"),
            ("#N/A", "A value is not available"),
            ("#NUM!", "A number is not valid"),
            ("#NULL!", "Two ranges do not intersect"),
            ("#SPILL!", "A result cannot spill"),
        ];
        let p = self.grid_pos();
        let l = self.grid_layout()?;
        let mut found: Vec<(u32, u32, String)> = Vec::new();
        let mut row = 0;
        while row < l.rows {
            let to = (row + 1000).min(l.rows);
            for (r, c, cell) in self.grid_cells(row..to, 0..l.cols.max(1)) {
                if ERRORS.iter().any(|e| e.0 == cell.text) {
                    found.push((r, c, cell.text));
                }
            }
            row = to;
        }
        found.sort_by_key(|f| (f.0, f.1));
        let next = found
            .iter()
            .find(|f| (f.0, f.1) > (p.row, p.col))
            .or_else(|| found.first())?
            .clone();
        self.grid_move_to(next.0, next.1);
        let why = ERRORS.iter().find(|e| e.0 == next.2).map_or("", |e| e.1);
        let name = format!(
            "{}{}",
            crate::csv_tools::column_letters(next.1 as usize),
            next.0 + 1
        );
        Some(format!(
            "{name}: {} — {why} ({} errors)",
            next.2,
            found.len()
        ))
    }

    /// The Watch Window's lines: each watched cell's sheet and name, value
    /// and formula.
    pub fn watch_lines(&mut self) -> Vec<String> {
        let watches = self.watches.clone();
        watches
            .iter()
            .map(|&(u, r, c)| {
                let name = self
                    .structure
                    .units
                    .get(u)
                    .map_or(String::new(), |x| x.label.clone());
                let cell = format!("{}{}", crate::csv_tools::column_letters(c as usize), r + 1);
                let shown = self
                    .doc()
                    .grid_cells(u, r..r + 1, c..c + 1)
                    .first()
                    .map(|x| x.2.text.clone())
                    .unwrap_or_default();
                let input = self.doc().cell_input(u, r, c);
                let formula = if input.starts_with('=') {
                    format!("  {input}")
                } else {
                    String::new()
                };
                format!("{name}!{cell} = {shown}{formula}")
            })
            .collect()
    }

    /// How the sheet shown is protected, when it is.
    pub fn sheet_protection(&mut self) -> Option<kalem_viewer::SheetProtection> {
        self.doc().sheet_protection(self.unit)
    }

    /// Whether the cursor's cell cannot be edited: locked on a protected
    /// sheet.
    pub fn cursor_locked(&mut self) -> bool {
        self.sheet_protection().is_some() && !self.cursor_cell().unlocked
    }

    /// Protect Sheet (`Some`) or Unprotect Sheet (`None`).
    pub fn protect_sheet(
        &mut self,
        protection: Option<kalem_viewer::SheetProtection>,
        password: Option<&str>,
    ) -> Result<(), String> {
        self.doc()
            .protect_sheet(self.unit, protection, password)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Protect Workbook (its structure), or not.
    pub fn protect_workbook(&mut self, on: bool, password: Option<&str>) -> Result<(), String> {
        self.doc()
            .protect_workbook(on, password)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// How the sheet shown prints.
    pub fn page_setup(&mut self) -> kalem_viewer::PageSetup {
        self.doc().page_setup(self.unit).unwrap_or_default()
    }

    /// Sets how the sheet shown prints.
    pub fn set_page_setup(&mut self, setup: &kalem_viewer::PageSetup) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc()
            .set_page_setup(self.unit, setup)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Unit `unit` as it prints: `area`, else its print area, else its
    /// used range; `None` for an empty sheet.
    pub fn sheet_print(
        &mut self,
        unit: usize,
        area: Option<[u32; 4]>,
    ) -> Option<crate::sheet_print::SheetPrint> {
        let shown = self.unit;
        self.unit = unit;
        let out = (|| {
            let l = self.grid_layout()?;
            let setup = self.doc().page_setup(unit).unwrap_or_default();
            let used = (l.rows > 0 && l.cols > 0).then(|| [0, 0, l.rows - 1, l.cols - 1]);
            let area = area.or(setup.print_area).or(used)?;
            let columns: Vec<u32> = (area[1]..=area[3])
                .filter(|c| !l.hidden_cols.contains(c))
                .collect();
            let widths = columns
                .iter()
                .map(|c| {
                    l.widths
                        .get(*c as usize)
                        .copied()
                        .unwrap_or(l.default_width)
                })
                .collect();
            let mut rows = setup.title_rows.map_or(area[0], |t| t.0.min(area[0]))..area[2] + 1;
            rows.start = rows.start.min(area[0]);
            let cells = self
                .grid_cells(rows, area[1]..area[3] + 1)
                .into_iter()
                .map(|(r, c, g)| ((r, c), g))
                .collect();
            let name = self.structure.units[unit]
                .label
                .trim_end_matches(" (hidden)")
                .to_owned();
            Some(crate::sheet_print::SheetPrint {
                name,
                area,
                columns,
                widths,
                hidden_rows: l.hidden_rows,
                cells,
                merged: l.merged,
                setup,
            })
        })();
        self.unit = shown;
        out
    }

    /// Format as Table: the selection, else the data around the cursor,
    /// made a table in `style`; its name and range.
    pub fn format_as_table(&mut self, style: &str) -> Result<String, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (r, header) = self.table_target();
        let name = self
            .doc()
            .create_table(self.unit, r, header, style)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(name)
    }

    /// The table the cursor is in.
    pub fn table_at_cursor(&mut self) -> Option<kalem_viewer::TableInfo> {
        let p = self.grid_pos();
        self.doc().tables(self.unit).into_iter().find(|t| {
            let r = t.range;
            (r[0]..=r[2]).contains(&p.row) && (r[1]..=r[3]).contains(&p.col)
        })
    }

    /// Total Row of the table at the cursor turned on or off.
    pub fn toggle_total_row(&mut self) -> Result<(), String> {
        let t = self.table_at_cursor().ok_or("The cursor is in no table")?;
        self.doc()
            .set_table_totals(self.unit, &t.name, !t.totals)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Convert to Range: the table at the cursor made cells again.
    pub fn convert_to_range(&mut self) -> Result<(), String> {
        let t = self.table_at_cursor().ok_or("The cursor is in no table")?;
        self.doc()
            .remove_table(self.unit, &t.name)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The table Custom Sort sorts, with each column's header (or letter).
    pub fn sort_columns(&mut self) -> Vec<(u32, String)> {
        self.duplicates_target().2
    }

    /// Filters the cursor's column of the filter by a rule (`None`: clears
    /// it).
    pub fn filter_rule(&mut self, rule: Option<kalem_viewer::FilterRule>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let col = self.grid_pos().col;
        self.doc()
            .filter_column_by(self.unit, col, rule)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Reapply: the filter's rules applied to the rows as they are now.
    pub fn reapply_filter(&mut self) -> Result<(), String> {
        self.doc()
            .reapply_filter(self.unit)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The custom lists a sort can follow: the months and the days, in
    /// English and Turkish, and the user's own.
    pub fn sort_lists(&self) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = [
            "January,February,March,April,May,June,July,August,September,October,November,December",
            "Ocak,Şubat,Mart,Nisan,Mayıs,Haziran,Temmuz,Ağustos,Eylül,Ekim,Kasım,Aralık",
            "Monday,Tuesday,Wednesday,Thursday,Friday,Saturday,Sunday",
            "Pazartesi,Salı,Çarşamba,Perşembe,Cuma,Cumartesi,Pazar",
        ]
        .iter()
        .map(|l| l.split(',').map(str::to_owned).collect())
        .collect();
        out.extend(self.fill_lists.iter().cloned());
        out
    }

    /// The values a column of the filter shows, each once, for choosing:
    /// the cells under its header as shown, an empty text for empty cells.
    pub fn filter_values(&mut self, col: u32) -> Vec<String> {
        let Some(f) = self.grid_layout().and_then(|l| l.filter) else {
            return Vec::new();
        };
        let cells = self
            .doc()
            .grid_cells(self.unit, f[0] + 1..f[2] + 1, col..col + 1);
        let filled: Vec<u32> = cells.iter().map(|c| c.0).collect();
        let mut out: Vec<String> = cells.into_iter().map(|c| c.2.text).collect();
        if (f[0] + 1..=f[2]).any(|r| !filled.contains(&r)) {
            out.push(String::new());
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|v| seen.insert(v.clone()));
        out.sort_by_key(|v| (v.is_empty(), v.to_lowercase()));
        out
    }

    /// The values a column of the filter shows now (in rows no filter
    /// hides), each once: the checked ones of its checklist.
    pub fn shown_filter_values(&mut self, col: u32) -> Vec<String> {
        let Some(l) = self.grid_layout() else {
            return Vec::new();
        };
        let Some(f) = l.filter else {
            return Vec::new();
        };
        if !l.filtered.contains(&col)
            && l.hidden_rows.iter().all(|r| !(f[0] + 1..=f[2]).contains(r))
        {
            return self.filter_values(col);
        }
        let cells = self
            .doc()
            .grid_cells(self.unit, f[0] + 1..f[2] + 1, col..col + 1);
        let mut out: Vec<String> = Vec::new();
        for r in f[0] + 1..=f[2] {
            if l.hidden_rows.contains(&r) {
                continue;
            }
            let t = cells
                .iter()
                .find(|c| c.0 == r)
                .map(|c| c.2.text.clone())
                .unwrap_or_default();
            if !out.contains(&t) {
                out.push(t);
            }
        }
        out
    }

    /// Filters a column of the filter to some values, or clears it.
    pub fn set_column_filter(
        &mut self,
        col: u32,
        values: Option<Vec<String>>,
    ) -> Result<(), String> {
        self.doc()
            .filter_column(self.unit, col, values)
            .map_err(|e| e.to_string())?;
        self.refresh();
        // The cursor leaves a row the filter hid: the next one shown.
        if let Some(l) = self.grid_layout() {
            let p = self.grid_pos();
            if l.hidden_rows.contains(&p.row) {
                let next = (p.row..l.max_rows)
                    .find(|r| !l.hidden_rows.contains(r))
                    .or_else(|| (0..p.row).rev().find(|r| !l.hidden_rows.contains(r)))
                    .unwrap_or(0);
                self.grid_move_to(next, p.col);
            }
        }
        Ok(())
    }

    /// Clears every column's filter, the filter kept.
    pub fn clear_filters(&mut self) -> Result<(), String> {
        let cols = self.grid_layout().map(|l| l.filtered).unwrap_or_default();
        for c in cols {
            self.set_column_filter(c, None)?;
        }
        Ok(())
    }

    /// Adds a conditional format on the selection.
    pub fn add_conditional_format(
        &mut self,
        rule: kalem_viewer::CondRule,
        style: kalem_viewer::CondStyle,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        self.doc()
            .add_conditional_format(self.unit, s, rule, style)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Removes the conditional formats that meet the selection, or all of
    /// the sheet's.
    pub fn clear_conditional_formats(&mut self, sheet: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let range = (!sheet).then(|| self.selection());
        let changed = self
            .doc()
            .clear_conditional_formats(self.unit, range)
            .map_err(|e| e.to_string())?;
        if changed.is_empty() {
            return Err(if sheet {
                "This sheet has no conditional formats".into()
            } else {
                "The selection has no conditional formats".into()
            });
        }
        self.refresh();
        Ok(())
    }

    /// The charts of the sheet shown.
    pub fn charts(&mut self) -> Vec<kalem_viewer::Chart> {
        if !self.is_grid() {
            return Vec::new();
        }
        self.doc().charts(self.unit)
    }

    /// Inserts a chart of the selection, or of the table at the cursor,
    /// beside it.
    pub fn insert_chart(
        &mut self,
        kind: kalem_viewer::ChartKind,
        title: Option<String>,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (r, _) = self.table_target();
        self.doc()
            .insert_chart(self.unit, r, kind, title)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The chart over the cursor's cell (the top one): its place among
    /// [`ViewerState::charts`] and its anchor.
    pub fn chart_at_cursor(&mut self) -> Option<(usize, [u32; 4])> {
        let p = self.grid_pos();
        self.charts()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| {
                (c.anchor[0]..=c.anchor[2]).contains(&p.row)
                    && (c.anchor[1]..=c.anchor[3]).contains(&p.col)
            })
            .map(|(i, c)| (i, c.anchor))
    }

    /// Moves or resizes a chart to cover `anchor`'s cells.
    pub fn move_chart(&mut self, index: usize, anchor: [u32; 4]) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc()
            .move_chart(self.unit, index, anchor)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Moves the chart under the cursor by rows and columns, the cursor
    /// with it, or (`resize`) moves its bottom right corner.
    pub fn nudge_chart(&mut self, rows: i64, cols: i64, resize: bool) -> Result<(), String> {
        let (i, a) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to move or resize it")?;
        let add = |v: u32, d: i64| (i64::from(v) + d).max(0) as u32;
        let new = if resize {
            [
                a[0],
                a[1],
                add(a[2], rows).max(a[0]),
                add(a[3], cols).max(a[1]),
            ]
        } else {
            let (dr, dc) = (
                if i64::from(a[0]) + rows < 0 {
                    -i64::from(a[0])
                } else {
                    rows
                },
                if i64::from(a[1]) + cols < 0 {
                    -i64::from(a[1])
                } else {
                    cols
                },
            );
            [add(a[0], dr), add(a[1], dc), add(a[2], dr), add(a[3], dc)]
        };
        if new == a {
            return Ok(());
        }
        self.move_chart(i, new)?;
        if !resize {
            let p = self.grid_pos();
            self.grid_move_to(
                add(p.row, i64::from(new[0]) - i64::from(a[0])),
                add(p.col, i64::from(new[1]) - i64::from(a[1])),
            );
        }
        Ok(())
    }

    /// Sets or removes the title of the chart under the cursor.
    pub fn set_chart_title(&mut self, title: Option<String>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to give it a title")?;
        self.doc()
            .set_chart_title(self.unit, i, title)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets or removes an axis title of the chart under the cursor.
    pub fn set_axis_title(
        &mut self,
        axis: kalem_viewer::ChartAxis,
        title: Option<String>,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to title its axes")?;
        self.doc()
            .set_axis_title(self.unit, i, axis, title)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Puts the legend of the chart under the cursor somewhere, or takes it
    /// away.
    pub fn set_legend(
        &mut self,
        position: Option<kalem_viewer::LegendPosition>,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to place its legend")?;
        self.doc()
            .set_legend(self.unit, i, position)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets what the data labels of the chart under the cursor show.
    pub fn set_data_labels(&mut self, labels: kalem_viewer::DataLabels) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to label it")?;
        self.doc()
            .set_data_labels(self.unit, i, labels)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets the value axis's scale of the chart under the cursor.
    pub fn set_axis_scale(&mut self, scale: kalem_viewer::AxisScale) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to scale its axis")?;
        self.doc()
            .set_axis_scale(self.unit, i, scale)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Changes the kind of the chart under the cursor.
    pub fn set_chart_kind(&mut self, kind: kalem_viewer::ChartKind) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to change its kind")?;
        self.doc()
            .set_chart_kind(self.unit, i, kind)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Colors a series of the chart under the cursor, or gives it the
    /// theme's color again.
    pub fn set_series_color(
        &mut self,
        series: usize,
        color: Option<[u8; 3]>,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to color its series")?;
        self.doc()
            .set_series_color(self.unit, i, series, color)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Colors a point (a pie's slice) of the chart under the cursor, or
    /// gives it its series' color again.
    pub fn set_point_color(
        &mut self,
        series: usize,
        point: usize,
        color: Option<[u8; 3]>,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to color its slices")?;
        self.doc()
            .set_point_color(self.unit, i, series, point, color)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Pulls a slice (or every slice, `point` `None`) of the pie under the
    /// cursor out by `percent` of its radius; 0 puts it back.
    pub fn set_explosion(&mut self, point: Option<usize>, percent: u32) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a pie to pull its slices out")?;
        self.doc()
            .set_explosion(self.unit, i, 0, point, percent)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Paints the chart area (background and border) of the chart under
    /// the cursor.
    pub fn set_chart_area(
        &mut self,
        background: kalem_viewer::Paint,
        border: kalem_viewer::Paint,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to paint it")?;
        self.doc()
            .set_chart_area(self.unit, i, background, border)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Paints the plot area (background and border, inside the axes) of
    /// the chart under the cursor.
    pub fn set_plot_area(
        &mut self,
        background: kalem_viewer::Paint,
        border: kalem_viewer::Paint,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to paint it")?;
        self.doc()
            .set_plot_area(self.unit, i, background, border)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Shows or hides the gridlines of the chart under the cursor.
    pub fn set_gridlines(&mut self, lines: kalem_viewer::Gridlines) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to set its gridlines")?;
        self.doc()
            .set_gridlines(self.unit, i, lines)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets the number format of the value axis's labels of the chart
    /// under the cursor; `None` takes the cells' own.
    pub fn set_axis_format(&mut self, format: Option<String>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to format its axis")?;
        self.doc()
            .set_axis_format(self.unit, i, format)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets the font of an axis's labels of the chart under the cursor.
    pub fn set_axis_font(
        &mut self,
        axis: kalem_viewer::ChartAxis,
        font: kalem_viewer::AxisFont,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to set its axes' font")?;
        self.doc()
            .set_axis_font(self.unit, i, axis, font)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets the font of the title of the chart under the cursor.
    pub fn set_title_font(&mut self, font: kalem_viewer::AxisFont) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to set its title's font")?;
        self.doc()
            .set_title_font(self.unit, i, font)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets the font of the legend of the chart under the cursor.
    pub fn set_legend_font(&mut self, font: kalem_viewer::AxisFont) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (i, _) = self
            .chart_at_cursor()
            .ok_or("Put the cursor on a chart to set its legend's font")?;
        self.doc()
            .set_legend_font(self.unit, i, font)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Removes the chart over the cursor's cell.
    pub fn delete_chart(&mut self) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        let at = self
            .charts()
            .iter()
            .rposition(|c| {
                (c.anchor[0]..=c.anchor[2]).contains(&p.row)
                    && (c.anchor[1]..=c.anchor[3]).contains(&p.col)
            })
            .ok_or("Put the cursor on a chart to delete it")?;
        self.doc()
            .delete_chart(self.unit, at)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The range a pivot table would summarize (the selection, or the
    /// table at the cursor) and its fields' names (its first row).
    pub fn pivot_source(&mut self) -> ([u32; 4], Vec<String>) {
        let (r, _) = self.table_target();
        let mut names = vec![String::new(); (r[3] - r[1] + 1) as usize];
        for (_, c, cell) in self.grid_cells(r[0]..r[0] + 1, r[1]..r[3] + 1) {
            names[(c - r[1]) as usize] = cell.text;
        }
        (r, names)
    }

    /// Inserts a pivot table on a new sheet and shows it, the cursor on
    /// its first cell.
    pub fn insert_pivot(&mut self, spec: kalem_viewer::PivotSpec) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let unit = self
            .doc()
            .insert_pivot(self.unit, spec)
            .map_err(|e| e.to_string())?;
        self.refresh();
        self.go_to(unit);
        self.grid_move_to(2, 0);
        Ok(())
    }

    /// Computes every pivot table of the file again (Refresh All).
    pub fn refresh_pivots(&mut self) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc().refresh_pivots().map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The data validation of the cursor's cell.
    pub fn cursor_validation(&mut self) -> Option<Validation> {
        let p = self.grid_pos();
        let key = (self.unit, p.row, p.col, self.generation);
        if let Some((k, v)) = &self.validation_cache
            && *k == key
        {
            return v.clone();
        }
        let v = self.doc().validation(self.unit, p.row, p.col);
        self.validation_cache = Some((key, v.clone()));
        v
    }

    /// Whether the cursor's cell offers a list to choose from.
    pub fn cursor_has_list(&mut self) -> bool {
        self.cursor_validation()
            .is_some_and(|v| v.dropdown && v.kind == kalem_viewer::ValidationKind::List)
    }

    /// What the cell's validation says of an entry, as typed.
    pub fn check_input(&mut self, row: u32, col: u32, input: &str) -> Option<ValidationError> {
        self.doc().check_input(self.unit, row, col, input)
    }

    /// Sets the selection's data validation, or removes it.
    pub fn set_validation(&mut self, v: Option<Validation>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        self.doc()
            .set_validation(self.unit, s, v)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The cells in view whose values their validation refuses, while
    /// Circle Invalid Data is on.
    pub fn invalid_cells(
        &mut self,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32)> {
        if !self.circle_invalid {
            return Vec::new();
        }
        self.doc().invalid_cells(self.unit, rows, cols)
    }

    /// Fills `target` from `source`, as the fill handle does, and selects
    /// it.
    pub fn fill_to(
        &mut self,
        source: [u32; 4],
        target: [u32; 4],
        series: bool,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let lists = self.fill_lists.clone();
        self.doc().set_fill_lists(lists);
        self.doc()
            .fill(self.unit, source, target, series)
            .map_err(|e| e.to_string())?;
        self.refresh();
        self.grid_move_to(target[0], target[1]);
        self.grid_extend_to(target[2], target[3]);
        Ok(())
    }

    /// The last row of the filled run in column `col` from row `from` on;
    /// `None` when `from` itself is empty.
    fn run_end(&mut self, col: u32, from: u32) -> Option<u32> {
        let max = self.grid_layout()?.max_rows;
        let mut row = from;
        let mut last = None;
        // A thousand rows at a time, till a cell holds nothing.
        while row < max {
            let to = (row + 1000).min(max);
            let filled: std::collections::HashSet<u32> = self
                .grid_cells(row..to, col..col + 1)
                .into_iter()
                .filter(|c| !c.2.text.is_empty())
                .map(|c| c.0)
                .collect();
            for r in row..to {
                if !filled.contains(&r) {
                    return last;
                }
                last = Some(r);
            }
            row = to;
        }
        last
    }

    /// The fill handle's double click: the selection filled down as far as
    /// the column beside it (left, else right) has data, as Excel does.
    pub fn fill_to_end(&mut self) -> Result<(), String> {
        let s = self.selection();
        let below = s[2] + 1;
        let left = s[1].checked_sub(1).and_then(|c| self.run_end(c, below));
        let end = left.or_else(|| self.run_end(s[3] + 1, below));
        let Some(end) = end else {
            return Err("No data beside the cells to fill down along".into());
        };
        self.fill_to(s, [s[0], s[1], end, s[3]], true)
    }

    /// The user's own lists the fills go round from now on.
    pub fn set_fill_lists(&mut self, lists: Vec<Vec<String>>) {
        self.fill_lists = lists;
    }

    /// Fill Down (Ctrl+D): the selection's first row copied down it, or a
    /// single row's cells from the row above.
    pub fn fill_down(&mut self) -> Result<(), String> {
        let s = self.selection();
        if s[0] == s[2] {
            if s[0] == 0 {
                return Err("No row above to fill from".into());
            }
            return self.fill_to(
                [s[0] - 1, s[1], s[0] - 1, s[3]],
                [s[0] - 1, s[1], s[2], s[3]],
                false,
            );
        }
        self.fill_to([s[0], s[1], s[0], s[3]], s, false)
    }

    /// Fill Right (Ctrl+R): the selection's first column copied across it,
    /// or a single column's cells from the column left of it.
    pub fn fill_right(&mut self) -> Result<(), String> {
        let s = self.selection();
        if s[1] == s[3] {
            if s[1] == 0 {
                return Err("No column left to fill from".into());
            }
            return self.fill_to(
                [s[0], s[1] - 1, s[2], s[1] - 1],
                [s[0], s[1] - 1, s[2], s[3]],
                false,
            );
        }
        self.fill_to([s[0], s[1], s[2], s[1]], s, false)
    }

    /// Fill Series: the selection's filled first rows go on down the rest
    /// of it, as a series (the fill handle's drag from the keyboard).
    pub fn fill_series(&mut self) -> Result<(), String> {
        let s = self.selection();
        let filled: std::collections::HashSet<u32> = self
            .grid_cells(s[0]..s[2] + 1, s[1]..s[3] + 1)
            .into_iter()
            .filter(|c| !c.2.text.is_empty())
            .map(|c| c.0)
            .collect();
        let rows = (s[0]..=s[2]).take_while(|r| filled.contains(r)).count() as u32;
        if rows == 0 {
            return Err("The selection's first row is empty: nothing to go on from".into());
        }
        if s[0] + rows > s[2] {
            return Err("Select the empty rows to fill too".into());
        }
        self.fill_to([s[0], s[1], s[0] + rows - 1, s[3]], s, true)
    }

    /// Flash Fill: the cursor's column filled down the table beside it from
    /// the examples typed in it, as Excel's (Ctrl+E): every row's other
    /// cells tell its example, and the empty cells get what the pattern
    /// learned from the examples makes of their rows. How many it filled.
    pub fn flash_fill(&mut self) -> Result<usize, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let col = self.grid_pos().col;
        // The table at the cursor, without its header row.
        let (r, header) = self.table_target();
        let first = r[0] + u32::from(header);
        if first > r[2] || r[1] == r[3] {
            return Err("Flash Fill works beside a table: type an example next to it".into());
        }
        let mut grid: std::collections::HashMap<(u32, u32), String> =
            std::collections::HashMap::new();
        for (row, c, cell) in self.grid_cells(first..r[2] + 1, r[1]..r[3] + 1) {
            grid.insert((row, c), cell.text);
        }
        let inputs = |row: u32| -> Vec<String> {
            (r[1]..=r[3])
                .filter(|c| *c != col)
                .map(|c| grid.get(&(row, c)).cloned().unwrap_or_default())
                .collect()
        };
        let examples: Vec<(Vec<String>, String)> = (first..=r[2])
            .filter_map(|row| {
                let out = grid.get(&(row, col)).filter(|t| !t.is_empty())?;
                Some((inputs(row), out.clone()))
            })
            .collect();
        if examples.is_empty() {
            return Err("Type an example in the column first".into());
        }
        let pattern = crate::flash_fill::learn(&examples)
            .ok_or("No pattern makes those examples: type another one")?;
        let cells: Vec<(u32, u32, String)> = (first..=r[2])
            .filter(|row| grid.get(&(*row, col)).is_none_or(String::is_empty))
            .filter_map(|row| {
                let out = crate::flash_fill::apply(&pattern, &inputs(row))?;
                // As text, the way the example was typed.
                Some((row, col, format!("'{out}")))
            })
            .collect();
        if cells.is_empty() {
            return Err("Nothing left to fill in the column".into());
        }
        let n = cells.len();
        self.doc()
            .set_cell_list(self.unit, &cells)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(n)
    }

    /// Changes the selection's format (Format Cells).
    pub fn change_style(&mut self, change: kalem_viewer::StyleChange) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let areas = self.selection_areas();
        let unit = self.unit;
        let r = self.in_batch(|d| {
            for s in &areas {
                d.change_style(unit, *s, change.clone())?;
            }
            Ok(())
        });
        self.refresh();
        r
    }

    /// Which cells along a line hold something: rows `a..b` of column
    /// `fixed` (`vertical`), else columns `a..b` of row `fixed`.
    fn filled_along(
        &mut self,
        vertical: bool,
        fixed: u32,
        a: u32,
        b: u32,
    ) -> std::collections::HashSet<u32> {
        let cells = if vertical {
            self.grid_cells(a..b, fixed..fixed + 1)
        } else {
            self.grid_cells(fixed..fixed + 1, a..b)
        };
        cells
            .into_iter()
            .filter(|c| !c.2.text.is_empty())
            .map(|c| if vertical { c.0 } else { c.1 })
            .collect()
    }

    /// Where Ctrl+arrow goes from the cursor, as Excel: along data to its
    /// last cell, else to the next cell that holds something, else to the
    /// sheet's edge.
    pub fn data_edge(&mut self, rows: i64, cols: i64) -> (u32, u32) {
        let p = self.grid_pos();
        let Some(l) = self.grid_layout() else {
            return (p.row, p.col);
        };
        let vertical = rows != 0;
        let dir = if vertical {
            rows.signum()
        } else {
            cols.signum()
        };
        let (fixed, start, max, used) = if vertical {
            (p.col, p.row, l.max_rows, l.rows)
        } else {
            (p.row, p.col, l.max_cols, l.cols)
        };
        // Read a thousand cells at a time, ahead in the direction moved;
        // past the data nothing is.
        let mut chunk = (0u32, 0u32, std::collections::HashSet::new());
        let mut filled = |this: &mut Self, at: u32| -> bool {
            if at >= used {
                return false;
            }
            if !(chunk.0..chunk.1).contains(&at) {
                let (a, b) = if dir > 0 {
                    (at, (at + 1000).min(used))
                } else {
                    (at.saturating_sub(999), at + 1)
                };
                chunk = (a, b, this.filled_along(vertical, fixed, a, b));
            }
            chunk.2.contains(&at)
        };
        let step = |at: u32| -> Option<u32> {
            let n = i64::from(at) + dir;
            (n >= 0 && n < i64::from(max)).then_some(n as u32)
        };
        let Some(next) = step(start) else {
            return (p.row, p.col);
        };
        let target = if filled(self, start) && filled(self, next) {
            let mut at = next;
            while let Some(n) = step(at) {
                if !filled(self, n) {
                    break;
                }
                at = n;
            }
            at
        } else {
            let mut at = next;
            loop {
                if filled(self, at) {
                    break at;
                }
                if dir > 0 && at >= used {
                    break max - 1;
                }
                match step(at) {
                    Some(n) => at = n,
                    None => break at,
                }
            }
        };
        if vertical {
            (target, p.col)
        } else {
            (p.row, target)
        }
    }

    /// Selects from `anchor` to `cursor`, the view left where it is unless
    /// `scroll`.
    pub fn select_range(&mut self, anchor: (u32, u32), cursor: (u32, u32), scroll: bool) {
        let mut p = self.grid_pos();
        p.sel = (anchor != cursor).then_some(anchor);
        if scroll {
            self.grid_pos.insert(self.unit, p);
            self.place(cursor.0, cursor.1);
        } else {
            (p.row, p.col) = cursor;
            self.grid_pos.insert(self.unit, p);
        }
    }

    /// The data around the cursor, as Excel's current region: grown while
    /// a row or column beside it (corners too) holds something.
    pub fn current_region(&mut self) -> [u32; 4] {
        let p = self.grid_pos();
        let Some(l) = self.grid_layout() else {
            return [p.row, p.col, p.row, p.col];
        };
        let (rows, cols) = (l.rows.min(l.max_rows), l.cols.min(l.max_cols));
        let mut r = [p.row, p.col, p.row, p.col];
        loop {
            let before = r;
            let c0 = r[1].saturating_sub(1);
            let c1 = (r[3] + 1).min(cols.saturating_sub(1));
            // Down and up: up to a thousand rows at a time, to the first
            // empty one.
            if r[2] + 1 < rows {
                let (a, b) = (r[2] + 1, (r[2] + 1001).min(rows));
                let full: std::collections::HashSet<u32> = self
                    .grid_cells(a..b, c0..c1 + 1)
                    .into_iter()
                    .filter(|c| !c.2.text.is_empty())
                    .map(|c| c.0)
                    .collect();
                let first_empty = (a..b).find(|x| !full.contains(x)).unwrap_or(b);
                r[2] = r[2].max(first_empty.saturating_sub(1));
            }
            if r[0] > 0 {
                let (a, b) = (r[0].saturating_sub(1000), r[0]);
                let full: std::collections::HashSet<u32> = self
                    .grid_cells(a..b, c0..c1 + 1)
                    .into_iter()
                    .filter(|c| !c.2.text.is_empty())
                    .map(|c| c.0)
                    .collect();
                let first_empty = (a..b).rev().find(|x| !full.contains(x));
                r[0] = first_empty.map_or(a, |e| e + 1).min(r[0]);
            }
            let r0 = r[0].saturating_sub(1);
            let r1 = (r[2] + 1).min(rows.saturating_sub(1));
            if r[3] + 1 < cols {
                let (a, b) = (r[3] + 1, (r[3] + 1001).min(cols));
                let full: std::collections::HashSet<u32> = self
                    .grid_cells(r0..r1 + 1, a..b)
                    .into_iter()
                    .filter(|c| !c.2.text.is_empty())
                    .map(|c| c.1)
                    .collect();
                let first_empty = (a..b).find(|x| !full.contains(x)).unwrap_or(b);
                r[3] = r[3].max(first_empty.saturating_sub(1));
            }
            if r[1] > 0 {
                let (a, b) = (r[1].saturating_sub(1000), r[1]);
                let full: std::collections::HashSet<u32> = self
                    .grid_cells(r0..r1 + 1, a..b)
                    .into_iter()
                    .filter(|c| !c.2.text.is_empty())
                    .map(|c| c.1)
                    .collect();
                let first_empty = (a..b).rev().find(|x| !full.contains(x));
                r[1] = first_empty.map_or(a, |e| e + 1).min(r[1]);
            }
            if r == before {
                return r;
            }
        }
    }

    /// What Excel's status bar says of a selection of more than one cell:
    /// `Average: 1073.44 · Count: 4 · Sum: 4293.75`, numbers in the cursor's
    /// cell's format; only the count when no value is a number.
    pub fn selection_sums(&mut self) -> Option<String> {
        let s = self.selection();
        if s[0] == s[2] && s[1] == s[3] {
            return None;
        }
        let key = (self.unit, s, self.generation);
        if let Some((k, v)) = &self.selection_sums
            && *k == key
        {
            return v.clone();
        }
        let (numbers, count) = self.doc().range_numbers(self.unit, s);
        let code = self
            .cursor_format()
            .filter(|c| c.contains(['0', '#', '?']))
            .unwrap_or_else(|| "General".into());
        let text = (count > 0).then(|| {
            if numbers.is_empty() {
                format!("Count: {count}")
            } else {
                let sum: f64 = numbers.iter().sum();
                let average = sum / numbers.len() as f64;
                format!(
                    "Average: {} · Count: {count} · Sum: {}",
                    format_axis_number(average, &code),
                    format_axis_number(sum, &code)
                )
            }
        });
        self.selection_sums = Some((key, text.clone()));
        text
    }

    /// The cells Find matches, row by row, with what it matched in each
    /// (what the cell shows, or holds for a search in formulas).
    pub fn find_matches(&mut self) -> Vec<(u32, u32, String)> {
        let Some(l) = self.grid_layout() else {
            return Vec::new();
        };
        let (rows, cols) = (l.rows.min(l.max_rows), l.cols.min(l.max_cols).max(1));
        let search = self.grid_search.clone();
        let mut out = Vec::new();
        let mut row = 0;
        while row < rows {
            let to = (row + 1000).min(rows);
            let mut cells: Vec<(u32, u32, String)> = self
                .grid_cells(row..to, 0..cols)
                .into_iter()
                .filter(|c| !c.2.text.is_empty() || c.2.formula)
                .map(|c| (c.0, c.1, c.2.text))
                .collect();
            cells.sort_by_key(|c| (c.0, c.1));
            for (r, c, shown) in cells {
                let hay = if search.formulas {
                    self.doc().cell_input(self.unit, r, c)
                } else {
                    shown
                };
                if search.matches(&hay) {
                    out.push((r, c, hay));
                }
            }
            row = to;
        }
        out
    }

    /// Find Next or Previous: the cursor to the next match after it (or
    /// before), round the sheet; which match it is and how many there are.
    pub fn find_step(&mut self, forward: bool) -> Result<(usize, usize), String> {
        if self.grid_search.text.is_empty() {
            return Err("Find what? (Ctrl+F)".into());
        }
        let found = self.find_matches();
        if found.is_empty() {
            return Err(format!("Cannot find {}", self.grid_search.text));
        }
        let p = self.grid_pos();
        let at = (p.row, p.col);
        let i = if forward {
            found.iter().position(|m| (m.0, m.1) > at).unwrap_or(0)
        } else {
            found
                .iter()
                .rposition(|m| (m.0, m.1) < at)
                .unwrap_or(found.len() - 1)
        };
        self.grid_move_to(found[i].0, found[i].1);
        Ok((i + 1, found.len()))
    }

    /// Replace: the cursor's cell's text replaced if it matches, then on
    /// to the next match; whether a cell was replaced.
    pub fn replace_one(&mut self, with: &str) -> Result<bool, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        let input = self.doc().cell_input(self.unit, p.row, p.col);
        let hay = if self.grid_search.formulas {
            input.clone()
        } else {
            self.cursor_cell().text
        };
        let replaced = self.grid_search.matches(&hay) && self.grid_search.matches(&input);
        if replaced {
            let new = self.grid_search.replace(&input, with);
            self.doc()
                .set_cell_list(self.unit, &[(p.row, p.col, new)])
                .map_err(|e| e.to_string())?;
            self.refresh();
        }
        let _ = self.find_step(true);
        Ok(replaced)
    }

    /// Replace All: every match's text replaced, as one undo step; how many
    /// cells changed.
    pub fn replace_all(&mut self, with: &str) -> Result<usize, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let found = self.find_matches();
        let mut cells = Vec::new();
        for (r, c, _) in found {
            // What the cell holds is changed: its formula or constant.
            let input = self.doc().cell_input(self.unit, r, c);
            if self.grid_search.matches(&input) {
                let new = self.grid_search.replace(&input, with);
                if new != input {
                    cells.push((r, c, new));
                }
            }
        }
        if cells.is_empty() {
            return Err(format!("Cannot find {}", self.grid_search.text));
        }
        let n = cells.len();
        self.doc()
            .set_cell_list(self.unit, &cells)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(n)
    }

    /// Changes the workbook's sheets and shows the one it leaves shown (a
    /// visible one after hiding).
    pub fn edit_sheets(&mut self, edit: kalem_viewer::SheetEdit) -> Result<(), String> {
        use kalem_viewer::SheetEdit;
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let places_change = matches!(
            edit,
            SheetEdit::Insert(_) | SheetEdit::Delete(_) | SheetEdit::Move(..) | SheetEdit::Copy(..)
        );
        let hiding = matches!(edit, SheetEdit::Hide(_, true));
        let mut shown = self.doc().edit_sheets(edit).map_err(|e| e.to_string())?;
        if places_change {
            // Cursors were kept by sheet number.
            self.grid_pos.clear();
        }
        self.refresh();
        if hiding {
            let hidden = self.doc().hidden_units();
            let n = self.structure.units.len();
            shown = (shown..n)
                .chain((0..shown).rev())
                .find(|u| !hidden.contains(u))
                .unwrap_or(shown);
        }
        self.unit = usize::MAX;
        self.go_to(shown.min(self.structure.units.len().saturating_sub(1)));
        Ok(())
    }

    /// Hides the selection's rows (`rows`) or columns, or shows hidden
    /// ones in it again; the cursor leaves what it hid.
    pub fn set_hidden(&mut self, rows: bool, hidden: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        let (from, to) = if rows { (s[0], s[2]) } else { (s[1], s[3]) };
        self.doc()
            .set_hidden(self.unit, rows, from, to, hidden)
            .map_err(|e| e.to_string())?;
        self.refresh();
        if hidden {
            let p = self.grid_pos();
            let l = self.grid_layout().unwrap_or_default();
            let (gone, max) = if rows {
                (l.hidden_rows, l.max_rows)
            } else {
                (l.hidden_cols, l.max_cols)
            };
            let next = (to + 1..max)
                .find(|x| !gone.contains(x))
                .or_else(|| (0..from).rev().find(|x| !gone.contains(x)));
            if let Some(n) = next {
                if rows {
                    self.grid_move_to(n, p.col);
                } else {
                    self.grid_move_to(p.row, n);
                }
            }
        }
        Ok(())
    }

    /// Freezes the first `rows` and `cols` (none: unfreezes), the cursor
    /// kept in view.
    pub fn set_frozen(&mut self, rows: u32, cols: u32) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc()
            .set_frozen(self.unit, rows, cols)
            .map_err(|e| e.to_string())?;
        self.refresh();
        let p = self.grid_pos();
        let mut q = p;
        // The view starts again under and right of what is frozen.
        q.top = rows;
        q.left = cols;
        self.grid_pos.insert(self.unit, q);
        self.place(p.row, p.col);
        Ok(())
    }

    /// The cursor's cell's note.
    pub fn cursor_note(&mut self) -> Option<String> {
        let p = self.grid_pos();
        self.doc().cell_note(self.unit, p.row, p.col)
    }

    /// Gives the cursor's cell a note, or takes it away (`None`).
    pub fn set_note(&mut self, text: Option<String>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        self.doc()
            .set_note(self.unit, p.row, p.col, text)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// AutoSum of a selection of more than one cell: under each column
    /// with numbers, its sum (in the selection's last row when that is
    /// empty, else the row below), as one undo step; how many sums.
    pub fn auto_sum_selection(&mut self) -> Result<usize, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        let cells = self.grid_cells(s[0]..s[2] + 2, s[1]..s[3] + 1);
        let at = |r: u32, c: u32| cells.iter().find(|x| x.0 == r && x.1 == c).map(|x| &x.2);
        let last_empty = (s[1]..=s[3]).all(|c| at(s[2], c).is_none_or(|x| x.text.is_empty()));
        let (target, last) = if last_empty && s[2] > s[0] {
            (s[2], s[2] - 1)
        } else {
            (s[2] + 1, s[2])
        };
        let mut sums = Vec::new();
        for c in s[1]..=s[3] {
            if !(s[0]..=last).any(|r| at(r, c).is_some_and(|x| x.numeric)) {
                continue;
            }
            let col = crate::csv_tools::column_letters(c as usize);
            sums.push((
                target,
                c,
                format!("=SUM({col}{}:{col}{})", s[0] + 1, last + 1),
            ));
        }
        if sums.is_empty() {
            return Err("No numbers to sum in the selection".into());
        }
        let n = sums.len();
        self.doc()
            .set_cell_list(self.unit, &sums)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(n)
    }

    /// The SUM AutoSum proposes for the cursor's cell: of the numbers right
    /// above it, else of those left of it, else empty.
    pub fn auto_sum_formula(&mut self) -> String {
        let p = self.grid_pos();
        let numeric = |v: &mut Self, r: u32, c: u32| {
            v.grid_cells(r..r + 1, c..c + 1)
                .first()
                .is_some_and(|x| x.2.numeric)
        };
        let mut top = p.row;
        while top > 0 && numeric(self, top - 1, p.col) {
            top -= 1;
        }
        let col = |c: u32| crate::csv_tools::column_letters(c as usize);
        if top < p.row {
            let c = col(p.col);
            return format!("=SUM({c}{}:{c}{})", top + 1, p.row);
        }
        let mut left = p.col;
        while left > 0 && numeric(self, p.row, left - 1) {
            left -= 1;
        }
        if left < p.col {
            return format!(
                "=SUM({}{r}:{}{r})",
                col(left),
                col(p.col - 1),
                r = p.row + 1
            );
        }
        "=SUM()".into()
    }

    /// The functions formulas can use, with their arguments.
    pub fn formula_functions(&mut self) -> Vec<(String, String)> {
        self.doc().formula_functions()
    }

    /// Paste Special: the cells copied last pasted at the cursor's corner of
    /// the selection, as `kind`, turned when `transpose`.
    pub fn paste_special(
        &mut self,
        kind: kalem_viewer::PasteKind,
        transpose: bool,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let Some(from) = self.copied else {
            return Err("Copy cells of the workbook first".into());
        };
        let s = self.selection();
        self.doc()
            .paste_cells(from, (self.unit, s[0], s[1]), kind, transpose)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Insert Cells: the cells below the selection (in its columns), or
    /// right of it (in its rows), moved on by its size, references
    /// following them; the selection left empty. One undo step.
    pub fn insert_cells(&mut self, down: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        let l = self.grid_layout().unwrap_or_default();
        let (block, to) = if down {
            let last = l.rows.saturating_sub(1);
            if s[0] > last {
                return Ok(());
            }
            let n = s[2] - s[0] + 1;
            ([s[0], s[1], last, s[3]], (s[0] + n, s[1]))
        } else {
            let last = l.cols.saturating_sub(1);
            if s[1] > last {
                return Ok(());
            }
            let n = s[3] - s[1] + 1;
            ([s[0], s[1], s[2], last], (s[0], s[1] + n))
        };
        self.doc()
            .move_cells(self.unit, block, to.0, to.1)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Delete Cells: the selection's cells gone, those below it (in its
    /// columns) or right of it (in its rows) moved into their place. One
    /// undo step.
    pub fn delete_cells(&mut self, up: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        let l = self.grid_layout().unwrap_or_default();
        // What comes after, at least as large as the selection, so that
        // moving it over the selection leaves none of it.
        let block = if up {
            let n = s[2] - s[0] + 1;
            let end = l.rows.saturating_sub(1).max(s[2] + n).min(l.max_rows - 1);
            [s[2] + 1, s[1], end, s[3]]
        } else {
            let n = s[3] - s[1] + 1;
            let end = l.cols.saturating_sub(1).max(s[3] + n).min(l.max_cols - 1);
            [s[0], s[3] + 1, s[2], end]
        };
        if block[0] > block[2] || block[1] > block[3] {
            return self.clear_selection();
        }
        self.doc()
            .move_cells(self.unit, block, s[0], s[1])
            .map_err(|e| e.to_string())?;
        self.refresh();
        let (r, c) = (s[0], s[1]);
        self.grid_move_to(r, c);
        Ok(())
    }

    /// The table Remove Duplicates works on, whether its first row is the
    /// headers, and each column with its header (or letter).
    pub fn duplicates_target(&mut self) -> ([u32; 4], bool, Vec<(u32, String)>) {
        let (r, header) = self.table_target();
        let names = self.grid_cells(r[0]..r[0] + 1, r[1]..r[3] + 1);
        let cols = (r[1]..=r[3])
            .map(|c| {
                let letter = crate::csv_tools::column_letters(c as usize);
                let name = names
                    .iter()
                    .find(|x| x.1 == c)
                    .map(|x| x.2.text.clone())
                    .filter(|t| header && !t.is_empty());
                (
                    c,
                    name.map_or(format!("Column {letter}"), |n| format!("{n} ({letter})")),
                )
            })
            .collect();
        (r, header, cols)
    }

    /// Remove Duplicates of the table at the cursor by `columns` (none: all);
    /// how many rows went and how many stay.
    pub fn remove_duplicates(&mut self, columns: &[u32]) -> Result<(usize, usize), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let (r, header) = self.table_target();
        let removed = self
            .doc()
            .remove_duplicates(self.unit, r, columns, header)
            .map_err(|e| e.to_string())?;
        self.refresh();
        let rows = (r[2] - r[0] + 1) as usize - usize::from(header);
        Ok((removed, rows - removed))
    }

    /// Text to Columns: each cell of the selection's column split at
    /// `delimiter` into it and the cells to its right, entered as typed.
    /// One undo step; how many cells were split.
    pub fn text_to_columns(&mut self, delimiter: &str) -> Result<usize, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        if s[1] != s[3] {
            return Err("Select one column to split".into());
        }
        if delimiter.is_empty() {
            return Err("Split at what?".into());
        }
        let mut cells = Vec::new();
        let mut split = 0;
        for r in s[0]..=s[2] {
            let input = self.doc().cell_input(self.unit, r, s[1]);
            if input.starts_with('=') || !input.contains(delimiter) {
                continue;
            }
            split += 1;
            for (i, part) in input.split(delimiter).enumerate() {
                cells.push((r, s[1] + i as u32, part.trim().to_owned()));
            }
        }
        if cells.is_empty() {
            return Err(format!("No cell holds {delimiter:?}"));
        }
        self.doc()
            .set_cell_list(self.unit, &cells)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(split)
    }

    /// The selection as a defined name refers to it: the sheet (quoted when
    /// it must be) and the cells, fixed (`Budget!$B$2:$D$4`).
    pub fn selection_reference(&mut self) -> String {
        let s = self.selection();
        let sheet = self.structure.units[self.unit]
            .label
            .trim_end_matches(" (hidden)")
            .to_owned();
        let plain = sheet
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
            && sheet
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            && crate::csv_tools::parse_cell(&sheet).is_none();
        let sheet = if plain {
            sheet
        } else {
            format!("'{}'", sheet.replace('\'', "''"))
        };
        let cell = |r: u32, c: u32| {
            format!(
                "${}${}",
                crate::csv_tools::column_letters(c as usize),
                r + 1
            )
        };
        if (s[0], s[1]) == (s[2], s[3]) {
            format!("{sheet}!{}", cell(s[0], s[1]))
        } else {
            format!("{sheet}!{}:{}", cell(s[0], s[1]), cell(s[2], s[3]))
        }
    }

    /// The workbook's defined names, with what each refers to.
    pub fn defined_names(&mut self) -> Vec<(String, String)> {
        self.doc().defined_names()
    }

    /// Defines a name as `refers_to`, or deletes it (`None`).
    pub fn set_defined_name(&mut self, name: &str, refers_to: Option<&str>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc()
            .set_defined_name(name, refers_to)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The sheet shown as CSV, as Excel's CSV UTF-8 writes it: the used
    /// range's values as shown, a field quoted when it holds a comma, a
    /// quote or a line break, lines ending in CR LF.
    pub fn sheet_csv(&mut self) -> String {
        let Some(l) = self.grid_layout() else {
            return String::new();
        };
        let (rows, cols) = (l.rows, l.cols);
        let field = |t: &str| {
            if t.contains([',', '"', '\n', '\r']) {
                format!("\"{}\"", t.replace('"', "\"\""))
            } else {
                t.to_owned()
            }
        };
        let mut out = String::from("\u{feff}");
        let mut row = 0;
        while row < rows {
            let to = (row + 1000).min(rows);
            let cells = self.grid_cells(row..to, 0..cols.max(1));
            let mut grid = vec![vec![String::new(); cols as usize]; (to - row) as usize];
            for (r, c, cell) in cells {
                if let Some(slot) = grid
                    .get_mut((r - row) as usize)
                    .and_then(|line| line.get_mut(c as usize))
                {
                    *slot = cell.text;
                }
            }
            for line in grid {
                let fields: Vec<String> = line.iter().map(|t| field(t)).collect();
                out.push_str(&fields.join(","));
                out.push_str("\r\n");
            }
            row = to;
        }
        out
    }

    /// AutoComplete: the one text entry of the cursor's column that begins
    /// with what is typed (in either case), longer than it; none for
    /// numbers, formulas, or when several entries would do.
    pub fn column_completion(&mut self, typed: &str) -> Option<String> {
        if typed.is_empty() || typed.starts_with('=') || typed.parse::<f64>().is_ok() {
            return None;
        }
        let p = self.grid_pos();
        let key = (self.unit, p.col, self.generation);
        if self.col_entries.as_ref().is_none_or(|(k, _)| *k != key) {
            let rows = self.grid_layout().map_or(0, |l| l.rows);
            let mut entries: Vec<String> = Vec::new();
            let mut row = 0;
            while row < rows {
                let to = (row + 1000).min(rows);
                for (_, _, c) in self.grid_cells(row..to, p.col..p.col + 1) {
                    if !c.numeric && !c.formula && !c.text.is_empty() && !entries.contains(&c.text)
                    {
                        entries.push(c.text);
                    }
                }
                row = to;
            }
            self.col_entries = Some((key, entries));
        }
        let low = typed.to_lowercase();
        let found: Vec<&String> = self
            .col_entries
            .as_ref()?
            .1
            .iter()
            .filter(|e| {
                e.to_lowercase().starts_with(&low) && e.chars().count() > typed.chars().count()
            })
            .collect();
        // Several entries differing only in case are one.
        let first = found.first()?;
        found
            .iter()
            .all(|e| e.to_lowercase() == first.to_lowercase())
            .then(|| (*first).clone())
    }

    /// What to show while a formula is typed with the cursor `at`
    /// characters in: the names completing the word before it, and the
    /// arguments of the function it is in.
    pub fn formula_hint(&mut self, input: &str, at: usize) -> crate::formula_edit::Hint {
        if !input.starts_with('=') {
            return crate::formula_edit::Hint::default();
        }
        if self.functions.is_none() {
            let list = self.doc().formula_functions();
            self.functions = Some(list);
        }
        let names: Vec<String> = self
            .doc()
            .defined_names()
            .into_iter()
            .map(|n| n.0)
            .collect();
        let functions = self.functions.as_deref().unwrap_or_default();
        crate::formula_edit::hint(input, at, functions, &names)
    }

    /// The size of the sheet shown, rows and columns, for pointing.
    pub fn grid_max(&mut self) -> (u32, u32) {
        self.grid_layout()
            .map_or((1, 1), |l| (l.max_rows, l.max_cols))
    }

    /// Ctrl+Enter: `input` entered into every selected cell, formulas moved
    /// for each as from (`row`, `col`).
    pub fn enter_in_selection(&mut self, row: u32, col: u32, input: &str) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let areas = self.selection_areas();
        let unit = self.unit;
        let r = self.in_batch(|d| {
            for s in &areas {
                d.enter_in_range(unit, *s, (row, col), input)?;
            }
            Ok(())
        });
        self.refresh();
        r
    }

    /// Go To Special: the cells of a kind (`blanks`, `constants`,
    /// `formulas`, `errors`, `visible`, `notes`, `conditional`,
    /// `validation`) in the selection, or the used range when it is one
    /// cell, selected together; how many cells in how many ranges.
    pub fn go_to_special(&mut self, kind: &str) -> Result<(usize, usize), String> {
        const ERRORS: [&str; 8] = [
            "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#N/A", "#NUM!", "#NULL!", "#SPILL!",
        ];
        let l = self.grid_layout().ok_or("Not a sheet")?;
        if kind == "last" {
            self.grid_move_to(l.rows.saturating_sub(1), l.cols.saturating_sub(1));
            return Ok((1, 1));
        }
        let s = self.selection();
        let scope = if (s[0], s[1]) == (s[2], s[3]) {
            [0, 0, l.rows.saturating_sub(1), l.cols.saturating_sub(1)]
        } else {
            s
        };
        let mut cells: Vec<(u32, u32)> = Vec::new();
        match kind {
            "visible" => {
                for r in scope[0]..=scope[2] {
                    if l.hidden_rows.contains(&r) {
                        continue;
                    }
                    for c in scope[1]..=scope[3] {
                        if !l.hidden_cols.contains(&c) {
                            cells.push((r, c));
                        }
                    }
                }
            }
            "conditional" => {
                for m in self.doc().conditional_ranges(self.unit) {
                    for r in m[0].max(scope[0])..=m[2].min(scope[2]) {
                        for c in m[1].max(scope[1])..=m[3].min(scope[3]) {
                            cells.push((r, c));
                        }
                    }
                }
            }
            _ => {
                let got: std::collections::HashMap<(u32, u32), GridCell> = self
                    .grid_cells(scope[0]..scope[2] + 1, scope[1]..scope[3] + 1)
                    .into_iter()
                    .map(|(r, c, g)| ((r, c), g))
                    .collect();
                for r in scope[0]..=scope[2] {
                    for c in scope[1]..=scope[3] {
                        let g = got.get(&(r, c));
                        let filled = g.is_some_and(|g| !g.text.is_empty() || g.formula);
                        let take = match kind {
                            "blanks" => !filled,
                            "constants" => filled && g.is_some_and(|g| !g.formula),
                            "formulas" => g.is_some_and(|g| g.formula),
                            "errors" => g.is_some_and(|g| ERRORS.contains(&g.text.as_str())),
                            "notes" => g.is_some_and(|g| g.note),
                            "validation" => self.doc().validation(self.unit, r, c).is_some(),
                            _ => false,
                        };
                        if take {
                            cells.push((r, c));
                        }
                    }
                }
            }
        }
        if cells.is_empty() {
            return Err("No cells were found".into());
        }
        cells.sort_unstable();
        cells.dedup();
        let areas = areas_of(&cells);
        let n = (cells.len(), areas.len());
        let first = cells[0];
        self.grid_move_to(first.0, first.1);
        self.areas = areas;
        Ok(n)
    }

    /// The sheet shown's pictures and shapes.
    pub fn drawings(&mut self) -> Vec<kalem_viewer::Drawing> {
        let key = (self.unit, self.generation);
        if let Some((k, d)) = &self.drawings_cache
            && *k == key
        {
            return d.clone();
        }
        let d = self.doc().drawings(self.unit);
        self.drawings_cache = Some((key, d.clone()));
        d
    }

    /// A picture decoded for drawing (by its place among the drawings).
    pub fn drawing_bitmap(&mut self, index: usize) -> Option<Bitmap> {
        let key = (self.unit, index, self.generation);
        if let Some(b) = self.pictures.get(&key) {
            return b.clone();
        }
        let b = self
            .doc()
            .drawing_image(self.unit, index)
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| {
                let img = img.thumbnail(1600, 1600).to_rgba8();
                Bitmap::new(img.width(), img.height(), img.into_raw())
            });
        self.pictures.insert(key, b.clone());
        b
    }

    /// The picture or shape over the cursor's cell (the topmost).
    pub fn drawing_at_cursor(&mut self) -> Option<(usize, kalem_viewer::Drawing)> {
        let p = self.grid_pos();
        self.drawings()
            .into_iter()
            .enumerate()
            .rev()
            .find(|(_, d)| {
                let a = d.anchor;
                (a[0]..=a[2]).contains(&p.row) && (a[1]..=a[3]).contains(&p.col)
            })
    }

    /// Insert Picture: the image file put over cells from the cursor's,
    /// as many as its size takes (twenty columns and forty rows at most).
    pub fn insert_picture(&mut self, path: &std::path::Path) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let ext = path
            .extension()
            .map_or(String::new(), |e| e.to_string_lossy().to_lowercase());
        let img = image::load_from_memory(&bytes).map_err(|e| format!("Not a picture: {e}"))?;
        let l = self.grid_layout().ok_or("Not a sheet")?;
        let p = self.grid_pos();
        // Columns of the default width (`7 × width + 5` pixels), rows of
        // the default height (four thirds of its points).
        let col_px = l.default_width * 7.0 + 5.0;
        let row_px = l.default_height * 4.0 / 3.0;
        let cols = ((img.width() as f32 / col_px).ceil() as u32).clamp(1, 20);
        let rows = ((img.height() as f32 / row_px).ceil() as u32).clamp(1, 40);
        let anchor = [p.row, p.col, p.row + rows - 1, p.col + cols - 1];
        self.doc()
            .insert_picture(self.unit, anchor, &bytes, &ext)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Insert Shape or Text Box at the cursor: three columns by four rows.
    pub fn insert_shape(&mut self, preset: &str, text: &str, text_box: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        let anchor = [p.row, p.col, p.row + 3, p.col + 2];
        self.doc()
            .insert_shape(self.unit, anchor, preset, text, text_box)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Where Insert Sparklines puts them by default: the column right of
    /// the selection, one a row (one cell for a single row).
    pub fn sparkline_place(&mut self) -> [u32; 4] {
        let s = self.selection();
        [s[0], s[3] + 1, s[2], s[3] + 1]
    }

    /// Insert Sparklines: the selection's rows (or columns) drawn as
    /// sparklines in `location`'s cells, one each.
    pub fn insert_sparklines(
        &mut self,
        kind: kalem_viewer::SparklineKind,
        location: [u32; 4],
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let data = self.selection();
        // A line marks its highest and lowest points.
        let mark = kind == kalem_viewer::SparklineKind::Line;
        self.doc()
            .add_sparklines(self.unit, data, location, kind, mark)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Clear Sparklines: those in the selected cells.
    pub fn clear_sparklines(&mut self) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let range = self.selection();
        self.doc()
            .clear_sparklines(self.unit, range)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Goal Seek: `by`'s value that makes `set` come to `target`, put
    /// into `by`; `None` when none was found.
    pub fn goal_seek(
        &mut self,
        set: (u32, u32),
        target: f64,
        by: (u32, u32),
    ) -> Result<Option<f64>, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let found = self
            .doc()
            .goal_seek(self.unit, set, target, by)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(found)
    }

    /// Data Table: the selection, its first row and column the values put
    /// into the input cells.
    pub fn create_data_table(
        &mut self,
        row_input: Option<(u32, u32)>,
        col_input: Option<(u32, u32)>,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let range = self.selection();
        self.doc()
            .create_data_table(self.unit, range, row_input, col_input)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The sheet's what-if scenarios.
    pub fn scenarios(&mut self) -> Vec<kalem_viewer::Scenario> {
        self.doc().scenarios(self.unit)
    }

    /// A scenario added, shown or deleted (`what`: add, show, delete).
    pub fn scenario(
        &mut self,
        what: &str,
        name: &str,
        cells: &[(u32, u32)],
        comment: &str,
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let unit = self.unit;
        let mut doc = self.doc();
        match what {
            "add" => doc.add_scenario(unit, name, cells, comment),
            "show" => doc.show_scenario(unit, name),
            _ => doc.delete_scenario(unit, name),
        }
        .map_err(|e| e.to_string())?;
        drop(doc);
        self.refresh();
        Ok(())
    }

    /// How the sheet shown is shown: zoom, gridlines, headings, Page
    /// Break Preview, split.
    pub fn sheet_view(&mut self) -> kalem_viewer::SheetView {
        if let Some(v) = self.views.get(&self.unit) {
            return *v;
        }
        let v = self.doc().sheet_view(self.unit);
        self.views.insert(self.unit, v);
        v
    }

    /// Changes how the sheet shown is shown, kept in the file when it is
    /// edited (only on screen when it is shown, not edited).
    pub fn update_view(
        &mut self,
        f: impl FnOnce(&mut kalem_viewer::SheetView),
    ) -> Result<(), String> {
        let mut v = self.sheet_view();
        f(&mut v);
        v.zoom = v.zoom.clamp(10, 400);
        if self.grid_editable() {
            self.doc()
                .set_sheet_view(self.unit, v)
                .map_err(|e| e.to_string())?;
        }
        self.views.insert(self.unit, v);
        self.changed();
        let p = self.grid_pos();
        self.place(p.row, p.col);
        Ok(())
    }

    /// The grid's zoom, 1 for 100%.
    pub fn grid_zoom(&mut self) -> f32 {
        f32::from(self.sheet_view().zoom) / 100.0
    }

    /// The panes: frozen rows and columns, or a split's.
    pub fn panes(&mut self) -> Panes {
        if let Some([rows, cols, top, left]) = self.sheet_view().split {
            return Panes {
                rows: (top, rows),
                cols: (left, cols),
                split: true,
            };
        }
        let (fr, fc) = self.grid_layout().map_or((0, 0), |l| l.frozen);
        Panes {
            rows: (0, fr),
            cols: (0, fc),
            split: false,
        }
    }

    /// Split: the window in panes that scroll apart, at the cursor (the
    /// rows above it and the columns left of it in the top and left
    /// panes); again, the split taken away. Frozen panes are unfrozen.
    pub fn toggle_split(&mut self) -> Result<(), String> {
        if self.sheet_view().split.is_some() {
            return self.update_view(|v| v.split = None);
        }
        if self.grid_layout().is_some_and(|l| l.frozen != (0, 0)) {
            self.set_frozen(0, 0)?;
        }
        let p = self.grid_pos();
        let mut rows = p.row.saturating_sub(p.top);
        let cols = p.col.saturating_sub(p.left);
        if rows == 0 && cols == 0 {
            // At the view's corner: halfway down.
            rows = (self.grid_visible.0 / 2).max(1);
        }
        let (top, left) = (p.top, p.left);
        self.update_view(|v| v.split = Some([rows, cols, top, left]))?;
        // The main pane goes on from the cursor's row.
        let mut q = self.grid_pos();
        q.top = q.row.max(top + rows);
        q.left = if cols > 0 {
            q.col.max(left + cols)
        } else {
            q.left
        };
        self.grid_pos.insert(self.unit, q);
        let (r, c) = (q.row, q.col);
        self.place(r, c);
        Ok(())
    }

    /// Scrolls a split's top pane (or, `cols`, its left pane).
    pub fn scroll_split(&mut self, by: i64, cols: bool) -> Result<(), String> {
        let Some([rows, n, top, left]) = self.sheet_view().split else {
            return Err("The window is not split".into());
        };
        let at = |v: u32| (i64::from(v) + by).max(0) as u32;
        let split = if cols {
            [rows, n, top, at(left)]
        } else {
            [rows, n, at(top), left]
        };
        self.update_view(|v| v.split = Some(split))
    }

    /// Page Break Preview's pages of the sheet shown, when it is on.
    pub fn page_breaks(&mut self) -> Option<PageBreaks> {
        if !self.sheet_view().page_break_preview {
            return None;
        }
        let key = (self.unit, self.generation);
        if let Some((k, b)) = &self.pages_cache
            && *k == key
        {
            return Some(b.clone());
        }
        let l = self.grid_layout()?;
        let setup = self.page_setup();
        let area =
            setup
                .print_area
                .unwrap_or([0, 0, l.rows.saturating_sub(1), l.cols.saturating_sub(1)]);
        let heights: std::collections::HashMap<u32, f32> = l.heights.iter().copied().collect();
        let rows: Vec<(u32, f32)> = (area[0]..=area[2])
            .filter(|r| !l.hidden_rows.contains(r))
            .map(|r| (r, heights.get(&r).copied().unwrap_or(l.default_height)))
            .collect();
        let cols: Vec<(u32, f32)> = (area[1]..=area[3])
            .filter(|c| !l.hidden_cols.contains(c))
            .map(|c| {
                (
                    c,
                    l.widths.get(c as usize).copied().unwrap_or(l.default_width),
                )
            })
            .collect();
        let (r, c) = crate::sheet_print::page_breaks(&setup, &rows, &cols);
        let b = PageBreaks {
            area,
            rows: r,
            cols: c,
        };
        self.pages_cache = Some((key, b.clone()));
        Some(b)
    }

    /// Ctrl+click: the selection kept as a range of several, and a new
    /// one begun at a cell.
    pub fn add_area(&mut self, row: u32, col: u32) {
        if self.areas.is_empty() {
            let s = self.selection();
            self.areas.push(s);
        }
        let (row, col) = self.merge_at(row, col).map_or((row, col), |m| (m[0], m[1]));
        self.areas.push([row, col, row, col]);
        let mut p = self.grid_pos();
        p.sel = None;
        self.grid_pos.insert(self.unit, p);
        self.place(row, col);
    }

    /// Ctrl+drag: the range begun last made to reach a cell.
    pub fn extend_area(&mut self, row: u32, col: u32) {
        let Some(last) = self.areas.last_mut() else {
            return self.grid_extend_to(row, col);
        };
        let p = self.grid_pos.get(&self.unit).copied().unwrap_or_default();
        let (r0, c0) = (p.row, p.col);
        *last = [r0.min(row), c0.min(col), r0.max(row), c0.max(col)];
    }

    /// A selection dragged by its border and dropped with its top left
    /// cell at (`row`, `col`): moved, or with `copy` copied (formulas,
    /// values and formats), and selected there.
    pub fn drop_selection(&mut self, row: u32, col: u32, copy: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        if (row, col) == (s[0], s[1]) {
            return Ok(());
        }
        let unit = self.unit;
        if copy {
            self.doc().paste_cells(
                (unit, s),
                (unit, row, col),
                kalem_viewer::PasteKind::All,
                false,
            )
        } else {
            self.doc().move_cells_between(unit, s, unit, row, col)
        }
        .map_err(|e| e.to_string())?;
        self.refresh();
        self.grid_move_to(row, col);
        if (s[0], s[1]) != (s[2], s[3]) {
            self.grid_extend_to(row + s[2] - s[0], col + s[3] - s[1]);
        }
        Ok(())
    }

    /// The sheet shown copied into the workbook at `path` (written there),
    /// or with `keep` off moved there; the copy's name.
    pub fn copy_sheet_to_file(&mut self, path: &Path, keep: bool) -> Result<String, String> {
        if !keep && !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let viewer = for_file(path)
            .filter(|v| v.extensions().contains(&"xlsx"))
            .ok_or_else(|| format!("{} is not a workbook", path.display()))?;
        let mut dst = viewer
            .open(FileHandle::new(path))
            .map_err(|e| e.to_string())?;
        if !dst.grid(0).is_some_and(|l| l.editable) {
            return Err("Kalem writes .xlsx and .xlsm workbooks".into());
        }
        let unit = self.unit;
        let at = {
            let mut d = self.doc();
            crate::workbook_io::copy_sheet_into(d.as_mut(), unit, dst.as_mut())?
        };
        let name = dst.structure().units[at].label.clone();
        let bytes = dst.save().map_err(|e| e.to_string())?.bytes;
        crate::files::write(path, &bytes, crate::files::SaveOptions::default())
            .map_err(|e| e.to_string())?;
        if !keep {
            self.edit_sheets(kalem_viewer::SheetEdit::Delete(unit))?;
        }
        Ok(name)
    }

    /// The workbook's circular references (sheet, row, column).
    pub fn circular_references(&mut self) -> Vec<(usize, u32, u32)> {
        if let Some((g, c)) = &self.circ_cache
            && *g == self.generation
        {
            return c.clone();
        }
        let c = self.doc().circular_references();
        self.circ_cache = Some((self.generation, c.clone()));
        c
    }

    /// The workbook's calculation settings.
    pub fn doc_calc_options(&mut self) -> kalem_viewer::CalcOptions {
        self.doc().calc_options()
    }

    /// The calculation settings changed as `f` says.
    pub fn update_calc(
        &mut self,
        f: impl FnOnce(&mut kalem_viewer::CalcOptions),
    ) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let mut o = self.doc().calc_options();
        f(&mut o);
        self.doc().set_calc_options(o).map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The sheet shown's comment threads.
    pub fn threads(&mut self) -> Vec<kalem_viewer::CommentThread> {
        let key = (self.unit, self.generation);
        if let Some((k, t)) = &self.threads_cache
            && *k == key
        {
            return t.clone();
        }
        let t = self.doc().threads(self.unit);
        self.threads_cache = Some((key, t.clone()));
        t
    }

    /// The thread on the cursor's cell.
    pub fn cursor_thread(&mut self) -> Option<kalem_viewer::CommentThread> {
        let p = self.grid_pos();
        self.threads()
            .into_iter()
            .find(|t| (t.row, t.col) == (p.row, p.col))
    }

    /// A comment on the cursor's cell (a reply when it has a thread), by
    /// `author` now.
    pub fn add_comment(&mut self, author: &str, text: &str) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        let time = jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S.00")
            .to_string();
        self.doc()
            .add_thread_comment(self.unit, p.row, p.col, author, text, &time)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The cursor's thread resolved, or open again.
    pub fn resolve_comment(&mut self, done: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        self.doc()
            .resolve_thread(self.unit, p.row, p.col, done)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Comment `index` of the cursor's thread deleted; the first, the
    /// whole thread.
    pub fn delete_comment(&mut self, index: usize) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        self.doc()
            .delete_thread_comment(self.unit, p.row, p.col, index)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The sheets' tabs, hidden sheets left out.
    pub fn sheet_tabs(&mut self) -> Vec<SheetTab> {
        if let Some((g, t)) = &self.tabs_cache
            && *g == self.generation
        {
            return t.clone();
        }
        let labels: Vec<String> = self
            .structure
            .units
            .iter()
            .map(|u| u.label.clone())
            .collect();
        let mut tabs = Vec::new();
        for (u, label) in labels.into_iter().enumerate() {
            if label.ends_with(" (hidden)") {
                continue;
            }
            let color = self.doc().tab_color(u);
            tabs.push((u, label, color));
        }
        self.tabs_cache = Some((self.generation, tabs.clone()));
        tabs
    }

    /// The sheet shown's tab color set, or taken away.
    pub fn set_tab_color(&mut self, color: Option<[u8; 3]>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc()
            .set_tab_color(self.unit, color)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The picture or shape at the cursor moved (`grow` off) or made larger
    /// or smaller by rows and columns; the cursor goes with a moved one.
    pub fn nudge_drawing(&mut self, rows: i64, cols: i64, grow: bool) -> Result<(), String> {
        let (i, d) = self
            .drawing_at_cursor()
            .ok_or("No picture or shape at the cursor")?;
        let a = d.anchor;
        let at = |v: u32, by: i64| (i64::from(v) + by).max(0) as u32;
        let anchor = if grow {
            [
                a[0],
                a[1],
                at(a[2], rows).max(a[0]),
                at(a[3], cols).max(a[1]),
            ]
        } else {
            if (rows < 0 && a[0] == 0) || (cols < 0 && a[1] == 0) {
                return Ok(());
            }
            [
                at(a[0], rows),
                at(a[1], cols),
                at(a[2], rows),
                at(a[3], cols),
            ]
        };
        self.doc()
            .move_drawing(self.unit, i, anchor)
            .map_err(|e| e.to_string())?;
        self.refresh();
        if !grow {
            let p = self.grid_pos();
            self.grid_move_to(at(p.row, rows), at(p.col, cols));
        }
        Ok(())
    }

    /// The shape at the cursor given new text.
    pub fn set_shape_text(&mut self, text: &str) -> Result<(), String> {
        let (i, _) = self.drawing_at_cursor().ok_or("No shape at the cursor")?;
        self.doc()
            .set_shape_text(self.unit, i, text)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The picture or shape at the cursor deleted.
    pub fn delete_drawing(&mut self) -> Result<(), String> {
        let (i, _) = self
            .drawing_at_cursor()
            .ok_or("No picture or shape at the cursor")?;
        self.doc()
            .delete_drawing(self.unit, i)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// The cursor's cell's hyperlink.
    pub fn cursor_link(&mut self) -> Option<String> {
        let p = self.grid_pos();
        self.doc().cell_link(self.unit, p.row, p.col)
    }

    /// Gives the cursor's cell a hyperlink, or takes it away (`None`).
    pub fn set_link(&mut self, target: Option<String>) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        self.doc()
            .set_link(self.unit, p.row, p.col, target)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Clear Formats (`contents` off) or Clear All of the selection.
    pub fn clear_formats(&mut self, contents: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        self.doc()
            .clear_range(self.unit, s, contents, true)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Format Painter: the selection's format taken, or, taken already,
    /// painted over the selection; whether it painted.
    pub fn format_painter(&mut self) -> Result<bool, String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        match self.painter.take() {
            None => {
                self.painter = Some((self.unit, s));
                Ok(false)
            }
            Some(from) => {
                self.doc()
                    .fill_formats(from, (self.unit, s))
                    .map_err(|e| e.to_string())?;
                self.refresh();
                Ok(true)
            }
        }
    }

    /// The units that are hidden sheets.
    pub fn hidden_units(&mut self) -> Vec<usize> {
        self.doc().hidden_units()
    }

    /// The cursor's cell's number format code.
    pub fn cursor_format(&mut self) -> Option<String> {
        let p = self.grid_pos();
        self.doc().cell_format(self.unit, p.row, p.col)
    }

    /// The cursor's cell as drawn (its format), for toggles that follow it.
    pub fn cursor_cell(&mut self) -> GridCell {
        let p = self.grid_pos();
        self.grid_cells(p.row..p.row + 1, p.col..p.col + 1)
            .into_iter()
            .next()
            .map(|c| c.2)
            .unwrap_or_default()
    }

    /// Clears the selection's values, formats kept (Delete).
    pub fn clear_selection(&mut self) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let areas = self.selection_areas();
        let unit = self.unit;
        let r = self.in_batch(|d| {
            for s in &areas {
                d.clear_cells(unit, *s)?;
            }
            Ok(())
        });
        self.refresh();
        r
    }

    /// The ranges selected: Go To Special's several, else the selection.
    pub fn selection_areas(&mut self) -> Vec<[u32; 4]> {
        if self.areas.is_empty() {
            vec![self.selection()]
        } else {
            self.areas.clone()
        }
    }

    /// Edits of the document as one undo step.
    fn in_batch(
        &mut self,
        f: impl FnOnce(&mut dyn ViewerDocument) -> kalem_viewer::Result<()>,
    ) -> Result<(), String> {
        let mut d = self.doc();
        d.begin_batch();
        let r = f(&mut **d);
        d.end_batch();
        r.map_err(|e| e.to_string())
    }

    /// The selection as tab-separated text, rows on lines, as spreadsheets
    /// copy it: each cell as shown, quoted when it holds a tab, a line
    /// break or a quote.
    pub fn selection_tsv(&mut self) -> String {
        let s = self.selection();
        let mut grid =
            vec![vec![String::new(); (s[3] - s[1] + 1) as usize]; (s[2] - s[0] + 1) as usize];
        let cells = self
            .doc()
            .grid_cells(self.unit, s[0]..s[2] + 1, s[1]..s[3] + 1);
        for (r, c, cell) in cells {
            let t = cell.text;
            grid[(r - s[0]) as usize][(c - s[1]) as usize] = if t.contains(['\t', '\n', '\r', '"'])
            {
                format!("\"{}\"", t.replace('"', "\"\""))
            } else {
                t
            };
        }
        grid.into_iter()
            .map(|row| row.join("\t"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Merges the selection (Merge & Center with `center`); the cursor goes
    /// to its first cell.
    pub fn merge_selection(&mut self, center: bool) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        if (s[0], s[1]) == (s[2], s[3]) {
            return Err("Select the cells to merge (Shift and the arrows, or drag)".into());
        }
        self.doc()
            .merge_cells(self.unit, s, center)
            .map_err(|e| e.to_string())?;
        self.refresh();
        self.grid_move_to(s[0], s[1]);
        Ok(())
    }

    /// Splits the merged cell at the cursor.
    pub fn unmerge_at_cursor(&mut self) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        self.doc()
            .unmerge_cells(self.unit, p.row, p.col)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Moves a page down (`1`) or up (`-1`).
    pub fn grid_page(&mut self, dir: i64) {
        let fr = self.grid_layout().map_or(0, |l| l.frozen.0);
        let page = i64::from(self.grid_visible.0.saturating_sub(fr).max(1));
        self.grid_scroll(dir * page, 0);
        self.grid_move_by(dir * page, 0);
    }

    /// The cursor's cell as entered, for editing.
    pub fn cell_input(&mut self) -> String {
        let p = self.grid_pos();
        self.doc().cell_input(self.unit, p.row, p.col)
    }

    /// Enters text into a cell of the grid shown.
    pub fn set_cell(&mut self, row: u32, col: u32, input: &str) -> Result<(), String> {
        self.doc()
            .set_cell(self.unit, row, col, input)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Changes the grid's shape.
    pub fn grid_edit(&mut self, edit: GridEdit) -> Result<(), String> {
        self.doc()
            .grid_edit(self.unit, edit)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets column `col`'s width to fit its widest cell, as Excel's
    /// autofit: `measure` gives a text's width in digits of the grid's
    /// font (the spreadsheet's unit). An empty column gets the default.
    pub fn autofit_col(&mut self, col: u32, measure: &dyn Fn(&str) -> f32) -> Result<(), String> {
        let Some(layout) = self.grid_layout() else {
            return Ok(());
        };
        if !layout.editable {
            return Err("This file is shown, not edited".into());
        }
        let widest = self
            .doc()
            .grid_cells(self.unit, 0..layout.rows.max(1), col..col + 1)
            .into_iter()
            .map(|(_, _, c)| measure(&c.text))
            .fold(0.0_f32, f32::max);
        // Excel leaves about a digit of room beside the text.
        let width = if widest > 0.0 {
            (widest + 1.0).clamp(1.0, 255.0)
        } else {
            layout.default_width
        };
        self.doc()
            .set_col_width(self.unit, col, width)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets column `col`'s width, in digits of the grid's font.
    pub fn set_col_width(&mut self, col: u32, width: f32) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc()
            .set_col_width(self.unit, col, width.clamp(0.0, 255.0))
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Row `row`'s height in points; the default for a row without its own.
    pub fn row_height(&mut self, row: u32) -> f32 {
        self.grid_layout().map_or(15.0, |l| {
            l.heights
                .iter()
                .find(|(r, _)| *r == row)
                .map(|(_, h)| *h)
                .unwrap_or(if l.default_height > 0.0 {
                    l.default_height
                } else {
                    15.0
                })
        })
    }

    /// The default row height in points.
    pub fn default_row_height(&mut self) -> f32 {
        self.grid_layout()
            .map(|l| l.default_height)
            .filter(|h| *h > 0.0)
            .unwrap_or(15.0)
    }

    /// Sets row `row`'s height in points.
    pub fn set_row_height(&mut self, row: u32, height: f32) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        self.doc()
            .set_row_height(self.unit, row, height.clamp(0.0, 409.0))
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Sets row `row`'s height to fit its wrapped cells' lines, as Excel's
    /// autofit of a row: `measure` gives a text's width in digits. A row
    /// with no wrapped text gets the default height.
    pub fn fit_row_height(
        &mut self,
        row: u32,
        measure: &dyn Fn(&str) -> f32,
    ) -> Result<(), String> {
        let cols = self.used_cols().max(1);
        let cells = self.doc().grid_cells(self.unit, row..row + 1, 0..cols);
        let mut lines = 1.0_f32;
        for (_, c, cell) in cells
            .into_iter()
            .filter(|x| x.2.wrap && !x.2.text.is_empty())
        {
            // Excel leaves about a digit of room in a cell.
            let room = (self.col_width(c) - 1.0).max(1.0);
            let n: f32 = cell
                .text
                .split('\n')
                .map(|p| (measure(p) / room).ceil().max(1.0))
                .sum();
            lines = lines.max(n);
        }
        let h = self.default_row_height() * lines;
        self.set_row_height(row, h)
    }

    /// Turns Wrap Text on or off for the cursor's cell; turned on, the row
    /// grows to the text's lines, as in Excel.
    pub fn toggle_wrap(&mut self, measure: &dyn Fn(&str) -> f32) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = self.grid_pos();
        let on = !self
            .doc()
            .grid_cells(self.unit, p.row..p.row + 1, p.col..p.col + 1)
            .first()
            .is_some_and(|c| c.2.wrap);
        self.doc()
            .set_wrap(self.unit, p.row, p.col, on)
            .map_err(|e| e.to_string())?;
        self.refresh();
        if on {
            self.fit_row_height(p.row, measure)?;
        }
        Ok(())
    }

    /// Column `col`'s width as shown, in digits.
    pub fn col_width(&mut self, col: u32) -> f32 {
        self.grid_layout().map_or(8.43, |l| {
            l.widths
                .get(col as usize)
                .copied()
                .unwrap_or(l.default_width)
        })
    }

    /// The used columns of the grid shown.
    pub fn used_cols(&mut self) -> u32 {
        self.grid_layout().map_or(0, |l| l.cols)
    }

    /// The document's macros.
    pub fn macros(&mut self) -> Vec<MacroEntry> {
        self.doc().macros()
    }

    /// Runs a macro, answering its questions from `answers` in order.
    pub fn run_macro(&mut self, name: &str, answers: &[String]) -> Result<MacroOutcome, String> {
        let mut ui = Answers { answers, next: 0 };
        let out = self
            .doc()
            .run_macro(name, &mut ui)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(out)
    }
}

/// The files of `path`'s folder that `viewer` opens, by name, `path`
/// among them.
pub fn siblings(path: &Path, viewer: &dyn Viewer) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else {
        return vec![path.to_path_buf()];
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                            viewer
                                .extensions()
                                .contains(&e.to_ascii_lowercase().as_str())
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    if !files.iter().any(|p| p == path) {
        files.push(path.to_path_buf());
    }
    files.sort_by_key(|p| name_of(p).to_lowercase());
    files
}

/// The PNG of `bitmap`, for the clipboard and `kalem view --to png`.
pub fn png(bitmap: &Bitmap) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.rgba.to_vec())
        .ok_or("a broken bitmap")?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

const IN_VIEWER: &str = "editorMode == viewer";
/// The user's own lists a fill goes round (`spreadsheet.custom_lists`):
/// each setting text split at its commas, lists of fewer than two items
/// left out.
pub fn fill_lists(config: &crate::settings::Config) -> Vec<Vec<String>> {
    config
        .strings("spreadsheet.custom_lists")
        .iter()
        .map(|l| {
            l.split(',')
                .map(|i| i.trim().to_string())
                .filter(|i| !i.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|l| l.len() >= 2)
        .collect()
}

/// A number format with a decimal place more (`by` 1) or fewer (-1), as
/// Excel's Increase and Decrease Decimal make it: every section's first
/// number changed; General goes by the decimals the cell shows. `None`
/// when there is nothing to change.
pub fn change_decimals(code: &str, by: i32, shown: &str) -> Option<String> {
    if code.eq_ignore_ascii_case("General") {
        let shown_decimals = shown.rsplit_once('.').map_or(0, |(_, f)| {
            f.chars().take_while(char::is_ascii_digit).count()
        }) as i32;
        let n = shown_decimals + by;
        return (n >= 0 && n != shown_decimals).then(|| {
            if n == 0 {
                "0".to_owned()
            } else {
                format!("0.{}", "0".repeat(n as usize))
            }
        });
    }
    // Each character, and whether it is text rather than format: quoted,
    // escaped, in brackets, or the character after `_` or `*`.
    let mut chars: Vec<(char, bool)> = Vec::new();
    let (mut quoted, mut bracket, mut next_literal) = (false, false, false);
    for ch in code.chars() {
        let literal = if next_literal {
            next_literal = false;
            true
        } else if quoted {
            quoted = ch != '"';
            true
        } else if bracket {
            bracket = ch != ']';
            true
        } else {
            match ch {
                '"' => quoted = true,
                '[' => bracket = true,
                '\\' | '_' | '*' => next_literal = true,
                _ => {}
            }
            matches!(ch, '"' | '[' | '\\' | '_' | '*')
        };
        chars.push((ch, literal));
    }
    let mut out = String::new();
    let mut changed = false;
    for (i, section) in chars.split(|&(ch, lit)| ch == ';' && !lit).enumerate() {
        if i > 0 {
            out.push(';');
        }
        let mut sec: Vec<(char, bool)> = section.to_vec();
        let digit = |c: &(char, bool)| !c.1 && matches!(c.0, '0' | '#' | '?');
        // The number before an exponent.
        let end = sec
            .iter()
            .position(|c| !c.1 && matches!(c.0, 'E' | 'e'))
            .unwrap_or(sec.len());
        let point = sec[..end].iter().position(|c| !c.1 && c.0 == '.');
        let last_digit = sec[..end].iter().rposition(digit);
        match (point, last_digit) {
            (_, None) => {}
            (Some(p), Some(_)) => {
                let run = sec[p + 1..end].iter().take_while(|c| digit(c)).count();
                if by > 0 {
                    sec.insert(p + 1 + run, ('0', false));
                    changed = true;
                } else if run > 0 {
                    sec.remove(p + run);
                    if run == 1 {
                        sec.remove(p);
                    }
                    changed = true;
                }
            }
            (None, Some(l)) => {
                if by > 0 {
                    sec.insert(l + 1, ('0', false));
                    sec.insert(l + 1, ('.', false));
                    changed = true;
                }
            }
        }
        out.extend(sec.iter().map(|c| c.0));
    }
    changed.then_some(out)
}

/// A number through a spreadsheet's number format, as a chart's axis
/// shows it: the first section of the code, its digits after the point,
/// thousands separators, a percent, scientific notation, quoted or
/// escaped text and `[$₺-41F]` currencies around it, and a trailing comma
/// for each thousand it divides by. The common codes; anything else is
/// shown as the number.
pub fn format_axis_number(v: f64, code: &str) -> String {
    let code = code.split(';').next().unwrap_or("");
    if code.is_empty() || code.eq_ignore_ascii_case("General") {
        return if v.fract() == 0.0 {
            format!("{v}")
        } else {
            format!("{v:.2}").trim_end_matches('0').to_string()
        };
    }
    // The code's literal text before and after its number, and the
    // number's pattern.
    let (mut before, mut after, mut pattern) = (String::new(), String::new(), String::new());
    let mut chars = code.chars().peekable();
    let mut in_number = false;
    let mut done = false;
    while let Some(c) = chars.next() {
        let text = |s: &mut String, t: &str| s.push_str(t);
        match c {
            '"' => {
                let mut t = String::new();
                for d in chars.by_ref() {
                    if d == '"' {
                        break;
                    }
                    t.push(d);
                }
                text(
                    if in_number || done {
                        &mut after
                    } else {
                        &mut before
                    },
                    &t,
                );
                if in_number {
                    in_number = false;
                    done = true;
                }
            }
            '\\' => {
                if let Some(d) = chars.next() {
                    let target = if in_number || done {
                        &mut after
                    } else {
                        &mut before
                    };
                    target.push(d);
                }
            }
            '[' => {
                // `[$₺-41F]`: the symbol; colors and conditions dropped.
                let mut t = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    t.push(d);
                }
                if let Some(sym) = t.strip_prefix('$') {
                    let sym = sym.split('-').next().unwrap_or("");
                    text(
                        if in_number || done {
                            &mut after
                        } else {
                            &mut before
                        },
                        sym,
                    );
                }
            }
            '0' | '#' | '?' | '.' | ',' if !done => {
                in_number = true;
                pattern.push(c);
            }
            'E' | 'e' if in_number && matches!(chars.peek(), Some('+' | '-')) => {
                pattern.push('E');
                pattern.push(chars.next().unwrap_or('+'));
                while let Some(&d) = chars.peek() {
                    if matches!(d, '0' | '#') {
                        pattern.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            '_' | '*' => {
                chars.next();
            }
            c => {
                if in_number {
                    in_number = false;
                    done = true;
                }
                if done { after.push(c) } else { before.push(c) }
            }
        }
    }
    let percent = before.contains('%') || after.contains('%');
    let mut x = if percent { v * 100.0 } else { v };
    // Trailing commas divide by a thousand each.
    let mut pattern = pattern.as_str();
    while let Some(p) = pattern.strip_suffix(',') {
        x /= 1000.0;
        pattern = p;
    }
    let (mantissa, exponent) = match pattern.split_once('E') {
        Some((m, e)) => (m, Some(e)),
        None => (pattern, None),
    };
    let decimals = mantissa.split_once('.').map_or(0, |(_, d)| {
        d.chars().filter(|c| matches!(c, '0' | '#' | '?')).count()
    });
    let thousands = mantissa.split('.').next().unwrap_or("").contains(',');
    let negative = x < 0.0;
    let body = match exponent {
        Some(e) => {
            let digits = e.chars().filter(|c| matches!(c, '0' | '#')).count().max(1);
            let s = format!("{:.*e}", decimals, x.abs());
            let (m, ex) = s.split_once('e').unwrap_or((&s, "0"));
            let n: i32 = ex.parse().unwrap_or(0);
            let sign = if n < 0 { "-" } else { "+" };
            format!("{m}E{sign}{:0>digits$}", n.abs())
        }
        None => {
            // Halves away from zero, as Excel rounds; not to even.
            let scale = 10f64.powi(decimals as i32);
            let s = format!("{:.*}", decimals, (x.abs() * scale).round() / scale);
            let (int, frac) = s.split_once('.').map_or((s.as_str(), ""), |(a, b)| (a, b));
            let int = if thousands {
                let digits: Vec<char> = int.chars().collect();
                let mut out = String::new();
                for (k, d) in digits.iter().enumerate() {
                    if k > 0 && (digits.len() - k).is_multiple_of(3) {
                        out.push(',');
                    }
                    out.push(*d);
                }
                out
            } else {
                int.to_string()
            };
            if frac.is_empty() {
                int
            } else {
                format!("{int}.{frac}")
            }
        }
    };
    format!("{}{before}{body}{after}", if negative { "-" } else { "" })
}

/// A unit drawn as a picture: zoom, pan and turn apply.
const IN_IMAGE: &str = "editorMode == viewer && !viewerGrid";
/// A unit drawn as a grid of cells.
const IN_GRID: &str = "editorMode == viewer && viewerGrid";

fn cmd(
    id: &str,
    title: &str,
    keys: &[&str],
    when: &str,
    handler: fn(&mut EditorContext<'_>, &serde_json::Value) -> CommandResult,
) -> Command {
    Command {
        id: id.into(),
        title: title.into(),
        category: "Viewer".into(),
        default_keys: crate::builtin::literal_keys(keys),
        when: Some(crate::builtin::literal_when(when)),
        handler: CommandHandler::Native(handler),
        args_schema: None,
        source: CommandSource::Builtin,
        scope: Some(crate::command::Scope::only(&["viewer"])),
    }
}

/// Runs `f` on the active document's viewer.
fn with(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(&mut ViewerState) -> Result<(), String>,
) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if let Err(e) = f(v) {
        ctx.messages.push(e);
    }
    Ok(())
}

/// A tenth of the area, the step of the arrow keys.
fn step(v: &ViewerState, horizontal: bool) -> f32 {
    let (w, h) = v.area();
    (if horizontal { w } else { h } / 10.0).max(1.0)
}

fn pan(ctx: &mut EditorContext<'_>, dx: f32, dy: f32) -> CommandResult {
    with(ctx, |v| {
        let (sx, sy) = (step(v, true), step(v, false));
        v.scroll(dx * sx, dy * sy);
        Ok(())
    })
}

/// The next or previous file of the folder, or unit of a paged document.
fn turn(ctx: &mut EditorContext<'_>, delta: i64, files: bool) -> CommandResult {
    let Some(doc) = ctx.document.as_deref_mut() else {
        return Ok(());
    };
    if let Some(v) = doc.viewer.as_deref_mut()
        && !files
        && v.paged()
    {
        // A book stops at its first and last page (Page Down held to the
        // end must not open the folder's next file); `N` and `P` go to it.
        let n = v.structure().units.len() as i64;
        let u = (v.unit as i64 + delta).clamp(0, n - 1);
        v.go_to(u as usize);
        return Ok(());
    }
    if let Err(e) = doc.viewer_step_file(delta) {
        ctx.messages.push(e);
    }
    Ok(())
}

/// Cuts a grid's selection (Cut in a spreadsheet): on the clipboard, and
/// moved where it is pasted.
pub(crate) fn cut(ctx: &mut EditorContext<'_>) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let text = v
        .cut_selection()
        .map_err(crate::command::CommandError::new)?;
    let name = v.selection_name();
    ctx.requests.push(Request::CopyText(text));
    ctx.messages.push(format!("{name} cut: paste to move it"));
    Ok(())
}

/// Copies the picture shown (Copy in a viewer).
pub(crate) fn copy(ctx: &mut EditorContext<'_>) -> CommandResult {
    let Some(doc) = ctx.document.as_deref_mut() else {
        return Ok(());
    };
    let path = doc.meta.path.clone();
    let Some(v) = doc.viewer.as_deref_mut() else {
        return Ok(());
    };
    if v.is_grid() {
        // A grid copies the selection as tab-separated text, as
        // spreadsheets put it on the clipboard.
        let text = v.selection_tsv();
        let s = v.selection();
        v.copied = Some((v.unit, s));
        let n = (s[2] - s[0] + 1) * (s[3] - s[1] + 1);
        ctx.requests.push(Request::CopyText(text));
        if n > 1 {
            ctx.messages.push(format!("{} copied", v.selection_name()));
        }
        return Ok(());
    }
    // Text selected on the page: that text.
    if let Some(text) = v.selected_text() {
        ctx.requests.push(Request::CopyText(text));
        return Ok(());
    }
    let png = v
        .bitmap()
        .and_then(|b| png(&b))
        .map_err(crate::command::CommandError::new)?;
    ctx.requests.push(Request::CopyImage { png, path });
    Ok(())
}

/// The viewer's commands.
pub(crate) fn commands() -> Vec<Command> {
    let mut all = vec![
        cmd(
            "viewer.zoomIn",
            "Zoom In",
            &["=", "+"],
            IN_IMAGE,
            |ctx, _| {
                with(ctx, |v| {
                    v.zoom_by(ZOOM_STEP);
                    Ok(())
                })
            },
        ),
        cmd("viewer.zoomOut", "Zoom Out", &["-"], IN_IMAGE, |ctx, _| {
            with(ctx, |v| {
                v.zoom_by(1.0 / ZOOM_STEP);
                Ok(())
            })
        }),
        cmd(
            "viewer.fit",
            "Fit to Window",
            &["0", "f"],
            IN_IMAGE,
            |ctx, _| {
                with(ctx, |v| {
                    v.fit();
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.fitWidth",
            "Fit to Width",
            &["w"],
            IN_IMAGE,
            |ctx, _| {
                with(ctx, |v| {
                    v.fit_width();
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.actualSize",
            "Actual Size",
            &["1"],
            IN_IMAGE,
            |ctx, _| {
                with(ctx, |v| {
                    v.actual_size();
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.panLeft",
            "Pan Left",
            &["left", "h"],
            IN_IMAGE,
            |ctx, _| pan(ctx, -1.0, 0.0),
        ),
        cmd(
            "viewer.panRight",
            "Pan Right",
            &["right", "l"],
            IN_IMAGE,
            |ctx, _| pan(ctx, 1.0, 0.0),
        ),
        cmd(
            "viewer.panUp",
            "Pan Up",
            &["up", "k"],
            IN_IMAGE,
            |ctx, _| pan(ctx, 0.0, -1.0),
        ),
        cmd(
            "viewer.panDown",
            "Pan Down",
            &["down", "j"],
            IN_IMAGE,
            |ctx, _| pan(ctx, 0.0, 1.0),
        ),
        cmd(
            "viewer.rotateRight",
            "Rotate View Right",
            &["r"],
            IN_IMAGE,
            |ctx, _| {
                with(ctx, |v| {
                    v.rotate(1);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.rotateLeft",
            "Rotate View Left",
            &["shift+r"],
            IN_IMAGE,
            |ctx, _| {
                with(ctx, |v| {
                    v.rotate(-1);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.next",
            "Next",
            &["n", "pagedown"],
            IN_IMAGE,
            |ctx, _| turn(ctx, 1, false),
        ),
        cmd(
            "viewer.previous",
            "Previous",
            &["p", "pageup"],
            IN_IMAGE,
            |ctx, _| turn(ctx, -1, false),
        ),
        cmd(
            "viewer.nextFile",
            "Next File in Folder",
            &["shift+n"],
            IN_VIEWER,
            |ctx, _| turn(ctx, 1, true),
        ),
        cmd(
            "viewer.previousFile",
            "Previous File in Folder",
            &["shift+p"],
            IN_VIEWER,
            |ctx, _| turn(ctx, -1, true),
        ),
        cmd("viewer.first", "First", &["home"], IN_IMAGE, |ctx, _| {
            with(ctx, |v| {
                v.go_to(0);
                Ok(())
            })
        }),
        cmd(
            "viewer.last",
            "Last",
            &["end", "shift+g"],
            IN_IMAGE,
            |ctx, _| {
                with(ctx, |v| {
                    let n = v.structure().units.len();
                    v.go_to(n.saturating_sub(1));
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.togglePlay",
            "Play or Pause",
            &["."],
            "editorMode == viewer && viewerAnimated",
            |ctx, _| {
                with(ctx, |v| {
                    v.playing = !v.playing;
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.info",
            "Show Information",
            &["i"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    v.info = !v.info;
                    Ok(())
                })
            },
        ),
        cmd("viewer.copy", "Copy Picture", &["y"], IN_IMAGE, |ctx, _| {
            copy(ctx)
        }),
        cmd(
            "viewer.insertLink",
            "Insert Link at Point",
            &["shift+l"],
            IN_VIEWER,
            |ctx, _| {
                if let Some(path) = ctx.document.as_deref().and_then(|d| d.meta.path.clone()) {
                    ctx.requests.push(Request::InsertLink(path));
                }
                Ok(())
            },
        ),
        cmd(
            "viewer.edit",
            "Edit the File",
            &[],
            "editorMode == viewer && viewerEditable",
            |ctx, args| {
                let Some(id) = args
                    .get("edit")
                    .and_then(|e| e.as_str())
                    .map(str::to_string)
                else {
                    return Err(crate::command::CommandError {
                        message: "Which edit?".into(),
                    });
                };
                with(ctx, |v| v.apply(&id))
            },
        ),
    ];
    all.extend(grid_commands());
    for c in &mut all {
        if c.id == "viewer.edit" {
            c.args_schema = Some(serde_json::json!({
                "type": "object",
                "properties": { "edit": { "type": "string" } },
                "required": ["edit"],
            }));
        }
    }
    all
}

fn grid_move(ctx: &mut EditorContext<'_>, rows: i64, cols: i64) -> CommandResult {
    with(ctx, |v| {
        v.grid_move_by(rows, cols);
        Ok(())
    })
}

/// A row or column edit for as many rows or columns as are selected (the
/// selection: first row, first column, last row, last column).
fn grid_struct(ctx: &mut EditorContext<'_>, f: fn([u32; 4]) -> GridEdit) -> CommandResult {
    with(ctx, |v| {
        if !v.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = v.selection();
        v.grid_edit(f(s))
    })
}

/// Insert (Ctrl++) or Delete (Ctrl+-) Cells: which way the others go, or
/// whole rows or columns, from a menu as Excel's dialog.
fn cells_command(
    ctx: &mut EditorContext<'_>,
    args: &serde_json::Value,
    insert: bool,
) -> CommandResult {
    let id = if insert {
        "viewer.grid.insertCells"
    } else {
        "viewer.grid.deleteCells"
    };
    let Some(how) = args.get("how").and_then(|h| h.as_str()) else {
        let item = |how: &str, title: &str| {
            menu_item(
                id,
                serde_json::json!({ "how": how }),
                title,
                if insert { "Insert" } else { "Delete" },
            )
        };
        let items = if insert {
            vec![
                item("down", "Shift Cells Down"),
                item("right", "Shift Cells Right"),
                item("rows", "Entire Row"),
                item("cols", "Entire Column"),
            ]
        } else {
            vec![
                item("up", "Shift Cells Up"),
                item("left", "Shift Cells Left"),
                item("rows", "Entire Row"),
                item("cols", "Entire Column"),
            ]
        };
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    match (insert, how) {
        (true, "down") => with(ctx, |v| v.insert_cells(true)),
        (true, "right") => with(ctx, |v| v.insert_cells(false)),
        (false, "up") => with(ctx, |v| v.delete_cells(true)),
        (false, "left") => with(ctx, |v| v.delete_cells(false)),
        (true, "rows") => grid_struct(ctx, |s| GridEdit::InsertRows {
            at: s[0],
            count: s[2] - s[0] + 1,
        }),
        (true, _) => grid_struct(ctx, |s| GridEdit::InsertCols {
            at: s[1],
            count: s[3] - s[1] + 1,
        }),
        (false, "rows") => grid_struct(ctx, |s| GridEdit::DeleteRows {
            at: s[0],
            count: s[2] - s[0] + 1,
        }),
        (false, _) => grid_struct(ctx, |s| GridEdit::DeleteCols {
            at: s[1],
            count: s[3] - s[1] + 1,
        }),
    }
}

fn arg_u32(args: &serde_json::Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32)
}

/// Asks for a cell's new text, starting from `start` (the cell as
/// entered, or what was typed).
fn ask_cell(ctx: &mut EditorContext<'_>, start: Option<&str>) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if !v.grid_editable() {
        ctx.messages.push("This file is shown, not edited".into());
        return Ok(());
    }
    // A locked cell of a protected sheet is not edited.
    if v.cursor_locked() {
        ctx.messages
            .push("The cell is on a protected sheet: unprotect the sheet to change it".into());
        return Ok(());
    }
    let p = v.grid_pos();
    let current = match start {
        Some(s) => s.to_string(),
        None => v.cell_input(),
    };
    ctx.requests.push(Request::Ask {
        command: "viewer.grid.setCell".into(),
        args: serde_json::json!({ "row": p.row, "col": p.col, "value_default": current }),
        arg: "value".into(),
    });
    Ok(())
}

/// A macro's run, with the answers given so far; a question asked again
/// through the palette and the run made again.
fn run_macro(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some(name) = args
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::to_string)
    else {
        // Which macro: the list.
        let items: Vec<crate::palette::PaletteItem> = v
            .macros()
            .into_iter()
            .filter(|m| !m.event)
            .map(|m| crate::palette::PaletteItem {
                id: crate::palette::invocation(
                    "viewer.grid.runMacro",
                    &serde_json::json!({ "name": m.name }),
                ),
                title: m.name.clone(),
                category: "Macro".into(),
                keys: String::new(),
                also: m.name,
            })
            .collect();
        if items.is_empty() {
            ctx.messages.push("This file has no macros".into());
        } else {
            ctx.requests.push(Request::Choose(items));
        }
        return Ok(());
    };
    let mut answers: Vec<String> = args
        .get("answers")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    // An answer typed into the prompt comes under the question's own key.
    if let Some(key) = args.get("ask").and_then(|k| k.as_str())
        && let Some(answer) = args.get(key).and_then(|a| a.as_str())
    {
        answers.push(answer.to_string());
    }
    let out = v
        .run_macro(&name, &answers)
        .map_err(crate::command::CommandError::new)?;
    let again = |answers: &[String], extra: serde_json::Value| {
        let mut a = serde_json::json!({ "name": name, "answers": answers });
        if let (Some(obj), serde_json::Value::Object(more)) = (a.as_object_mut(), extra) {
            obj.extend(more);
        }
        a
    };
    match out.question {
        Some(MacroQuestion::Message {
            prompt,
            buttons,
            title,
        }) => {
            let choices: &[(&str, i64)] = match buttons & 7 {
                1 => &[("OK", 1), ("Cancel", 2)],
                2 => &[("Abort", 3), ("Retry", 4), ("Ignore", 5)],
                3 => &[("Yes", 6), ("No", 7), ("Cancel", 2)],
                4 => &[("Yes", 6), ("No", 7)],
                5 => &[("Retry", 4), ("Cancel", 2)],
                _ => &[("OK", 1)],
            };
            let items = choices
                .iter()
                .map(|(label, code)| {
                    let mut a = answers.clone();
                    a.push(code.to_string());
                    crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "viewer.grid.runMacro",
                            &again(&a, serde_json::json!({})),
                        ),
                        title: (*label).to_string(),
                        category: format!("{title}: {prompt}"),
                        keys: String::new(),
                        also: prompt.clone(),
                    }
                })
                .collect();
            ctx.requests.push(Request::Choose(items));
        }
        Some(MacroQuestion::Input {
            prompt,
            title: _,
            default,
        }) => {
            let key = if prompt.trim().is_empty() {
                "answer".to_string()
            } else {
                prompt.trim().to_string()
            };
            ctx.requests.push(Request::Ask {
                command: "viewer.grid.runMacro".into(),
                args: again(
                    &answers,
                    serde_json::json!({ "ask": key, format!("{key}_default"): default }),
                ),
                arg: key,
            });
        }
        None => {
            for line in out.output.iter().chain(&out.skipped) {
                ctx.messages.push(line.clone());
            }
            match out.error {
                Some(e) => return Err(crate::command::CommandError::new(e)),
                None if out.changed => ctx.messages.push(format!("{name} ran")),
                None => {}
            }
        }
    }
    Ok(())
}

/// A text's width in a monospaced grid's digits: wide characters count two.
pub fn text_cells(t: &str) -> f32 {
    unicode_width::UnicodeWidthStr::width(t) as f32
}

fn grid_select(ctx: &mut EditorContext<'_>, rows: i64, cols: i64) -> CommandResult {
    with(ctx, |v| {
        v.grid_select_by(rows, cols);
        Ok(())
    })
}

/// Merges the selection; when cells besides the first hold values, asks
/// first, as Excel warns that only the upper-left value stays.
fn merge(ctx: &mut EditorContext<'_>, args: &serde_json::Value, center: bool) -> CommandResult {
    let confirmed = args.get("confirmed").and_then(serde_json::Value::as_bool) == Some(true);
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if !confirmed && v.grid_editable() && v.selection_loses_values() {
        let id = if center {
            "viewer.grid.mergeCenter"
        } else {
            "viewer.grid.merge"
        };
        let question = "Merging keeps only the upper-left value".to_string();
        ctx.requests.push(Request::Choose(vec![
            crate::palette::PaletteItem {
                id: crate::palette::invocation(id, &serde_json::json!({ "confirmed": true })),
                title: "Merge".into(),
                category: question.clone(),
                keys: String::new(),
                also: question.clone(),
            },
            crate::palette::PaletteItem {
                id: crate::palette::invocation("viewer.grid.cancel", &serde_json::json!({})),
                title: "Cancel".into(),
                category: question.clone(),
                keys: String::new(),
                also: question,
            },
        ]));
        return Ok(());
    }
    with(ctx, |v| v.merge_selection(center))
}

/// Tab-separated text as rows of values: a field in double quotes may hold
/// tabs, line breaks and doubled quotes; the line break ending the last
/// row is not a row.
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    if text.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![Vec::new()];
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut at_start = true;
    while let Some(c) = chars.next() {
        match c {
            '"' if at_start => {
                // A quoted field, up to its closing quote.
                while let Some(q) = chars.next() {
                    if q == '"' {
                        if chars.peek() == Some(&'"') {
                            chars.next();
                            field.push('"');
                        } else {
                            break;
                        }
                    } else {
                        field.push(q);
                    }
                }
                at_start = false;
            }
            '\t' => {
                if let Some(row) = rows.last_mut() {
                    row.push(std::mem::take(&mut field));
                }
                at_start = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                if let Some(row) = rows.last_mut() {
                    row.push(std::mem::take(&mut field));
                }
                rows.push(Vec::new());
                at_start = true;
            }
            c => {
                field.push(c);
                at_start = false;
            }
        }
    }
    if let Some(row) = rows.last_mut() {
        row.push(field);
    }
    rows
}

/// A filter column's values as a checklist in the palette, as Excel's
/// filter menu: choosing a value checks or unchecks it and offers the list
/// again; Apply filters to the checked values. The list starts from the
/// values the column shows now.
fn choose_filter(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some(f) = v.grid_layout().and_then(|l| l.filter) else {
        ctx.messages
            .push("No filter here: turn it on with Filter (f)".into());
        return Ok(());
    };
    let col = args
        .get("col")
        .and_then(serde_json::Value::as_u64)
        .map_or(v.grid_pos().col, |c| c as u32)
        .clamp(f[1], f[3]);
    let values: Vec<String> = v.filter_values(col).into_iter().take(500).collect();
    let checked: Vec<String> = match args.get("checked").and_then(serde_json::Value::as_array) {
        Some(list) => list
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        None => v.shown_filter_values(col),
    };
    let header = crate::csv_tools::column_letters(col as usize);
    let category = format!(
        "Filter column {header}: {} of {} shown",
        values.iter().filter(|x| checked.contains(x)).count(),
        values.len()
    );
    let item = |id: String, title: String, also: String| crate::palette::PaletteItem {
        id,
        title,
        category: category.clone(),
        keys: String::new(),
        also,
    };
    let again = |checked: &[String]| {
        crate::palette::invocation(
            "viewer.grid.filterColumn",
            &serde_json::json!({ "col": col, "checked": checked }),
        )
    };
    let mut items = vec![
        item(
            crate::palette::invocation(
                "viewer.grid.setColumnFilter",
                &serde_json::json!({ "col": col, "values": checked }),
            ),
            "✓ Apply".into(),
            "apply".into(),
        ),
        item(again(&values), "Select All".into(), "all".into()),
        item(again(&[]), "Select None".into(), "none".into()),
    ];
    for value in &values {
        let on = checked.contains(value);
        let toggled: Vec<String> = if on {
            checked.iter().filter(|x| *x != value).cloned().collect()
        } else {
            let mut t = checked.clone();
            t.push(value.clone());
            t
        };
        let shown = if value.is_empty() {
            "(Empty)"
        } else {
            value.as_str()
        };
        items.push(item(
            again(&toggled),
            format!("{} {shown}", if on { "☑" } else { "☐" }),
            value.clone(),
        ));
    }
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Chart Title and the axis titles: asked for, starting from the title
/// the chart has; an empty one removes it.
fn chart_text(
    ctx: &mut EditorContext<'_>,
    args: &serde_json::Value,
    id: &str,
    axis: Option<kalem_viewer::ChartAxis>,
) -> CommandResult {
    use kalem_viewer::ChartAxis;
    let Some(value) = args
        .get("value")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        let Some(v) = ctx
            .document
            .as_deref_mut()
            .and_then(|d| d.viewer.as_deref_mut())
        else {
            return Ok(());
        };
        let Some((i, _)) = v.chart_at_cursor() else {
            ctx.messages
                .push("Put the cursor on a chart to give it a title".into());
            return Ok(());
        };
        let c = v.charts()[i].clone();
        let current = match axis {
            None => c.title,
            Some(ChartAxis::Horizontal) => c.horizontal_title,
            Some(ChartAxis::Vertical) => c.vertical_title,
        };
        return ask_more(
            ctx,
            id,
            &serde_json::json!({ "value_default": current.unwrap_or_default() }),
            "value",
        );
    };
    let title = Some(value.trim().to_string()).filter(|t| !t.is_empty());
    match axis {
        None => with(ctx, |v| v.set_chart_title(title)),
        Some(a) => with(ctx, |v| v.set_axis_title(a, title)),
    }
}

/// Data Labels: what they show as a checklist in the palette, as Excel's
/// Label Options; choosing a line checks or unchecks it and offers the
/// list again, Apply sets it. The list starts from what the chart shows.
fn data_labels(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::{ChartKind, DataLabels};
    const ID: &str = "viewer.grid.dataLabels";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to label it".into());
        return Ok(());
    };
    let chart = v.charts()[i].clone();
    let flag = |k: &str, now: bool| {
        args.get(k)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(now)
    };
    let l = DataLabels {
        value: flag("value", chart.labels.value),
        category: flag("category", chart.labels.category),
        series: flag("series", chart.labels.series),
        percent: flag("percent", chart.labels.percent),
    };
    if args.get("apply").and_then(serde_json::Value::as_bool) == Some(true) {
        return with(ctx, |v| v.set_data_labels(l));
    }
    let json = |l: DataLabels| {
        serde_json::json!({
            "value": l.value, "category": l.category, "series": l.series, "percent": l.percent
        })
    };
    let mut apply = json(l);
    apply["apply"] = serde_json::json!(true);
    let mut items = vec![
        menu_item(ID, apply, "✓ Apply", "Data Labels"),
        menu_item(
            ID,
            serde_json::json!({ "value": false, "category": false, "series": false, "percent": false, "apply": true }),
            "No Labels",
            "Data Labels",
        ),
    ];
    let pie = matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut);
    let mut lines = vec![
        (
            "Value",
            l.value,
            DataLabels {
                value: !l.value,
                ..l
            },
        ),
        (
            "Category Name",
            l.category,
            DataLabels {
                category: !l.category,
                ..l
            },
        ),
        (
            "Series Name",
            l.series,
            DataLabels {
                series: !l.series,
                ..l
            },
        ),
    ];
    if pie {
        lines.push((
            "Percentage",
            l.percent,
            DataLabels {
                percent: !l.percent,
                ..l
            },
        ));
    }
    for (title, on, toggled) in lines {
        items.push(menu_item(
            ID,
            json(toggled),
            &format!("{} {title}", if on { "☑" } else { "☐" }),
            "Data Labels",
        ));
    }
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Axis Scale: the value axis's minimum, maximum and major unit asked for
/// (empty for automatic), the logarithmic scale turned on or off, or all
/// back to automatic.
fn axis_scale(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::AxisScale;
    const ID: &str = "viewer.grid.axisScale";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to scale its axis".into());
        return Ok(());
    };
    let now = v.charts()[i].scale;
    let show = |x: Option<f64>| x.map_or(String::new(), |x| format!("{x}"));
    let field = args.get("field").and_then(|f| f.as_str()).unwrap_or("");
    let set =
        |ctx: &mut EditorContext<'_>, scale: AxisScale| with(ctx, |v| v.set_axis_scale(scale));
    match field {
        "auto" => set(ctx, AxisScale::default()),
        "log" => set(
            ctx,
            AxisScale {
                log: !now.log,
                ..now
            },
        ),
        "min" | "max" | "major" => {
            let Some(text) = args.get("value").and_then(|x| x.as_str()) else {
                let current = match field {
                    "min" => show(now.min),
                    "max" => show(now.max),
                    _ => show(now.major),
                };
                return ask_more(
                    ctx,
                    ID,
                    &serde_json::json!({ "field": field, "value_default": current }),
                    "value",
                );
            };
            let t = text.trim();
            // A decimal comma as well as a point.
            let t = if t.contains(',') && !t.contains('.') {
                t.replace(',', ".")
            } else {
                t.to_string()
            };
            let n = if t.is_empty() {
                None
            } else {
                match t.parse::<f64>() {
                    Ok(n) => Some(n),
                    Err(_) => {
                        ctx.messages.push(format!("Not a number: {text}"));
                        return Ok(());
                    }
                }
            };
            let scale = match field {
                "min" => AxisScale { min: n, ..now },
                "max" => AxisScale { max: n, ..now },
                _ => AxisScale { major: n, ..now },
            };
            set(ctx, scale)
        }
        _ => {
            let c = "Axis Scale";
            let line = |title: &str, x: Option<f64>| {
                format!(
                    "{title}… ({})",
                    x.map_or("automatic".into(), |x| format!("{x}"))
                )
            };
            let items = vec![
                menu_item(
                    ID,
                    serde_json::json!({ "field": "min" }),
                    &line("Minimum", now.min),
                    c,
                ),
                menu_item(
                    ID,
                    serde_json::json!({ "field": "max" }),
                    &line("Maximum", now.max),
                    c,
                ),
                menu_item(
                    ID,
                    serde_json::json!({ "field": "major" }),
                    &line("Major Unit", now.major),
                    c,
                ),
                menu_item(
                    ID,
                    serde_json::json!({ "field": "log" }),
                    &format!("{} Logarithmic Scale", if now.log { "☑" } else { "☐" }),
                    c,
                ),
                menu_item(ID, serde_json::json!({ "field": "auto" }), "Automatic", c),
            ];
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
    }
}

/// Excel's standard colors, as palette entries running `id` with `base`
/// and the color, then Custom… and Automatic.
fn color_menu(
    id: &str,
    base: &serde_json::Value,
    category: &str,
) -> Vec<crate::palette::PaletteItem> {
    let colors = [
        ("Blue", "#4472C4"),
        ("Orange", "#ED7D31"),
        ("Gray", "#A5A5A5"),
        ("Gold", "#FFC000"),
        ("Light Blue", "#5B9BD5"),
        ("Green", "#70AD47"),
        ("Dark Blue", "#264478"),
        ("Red", "#FF0000"),
        ("Dark Red", "#C00000"),
        ("Purple", "#7030A0"),
        ("Black", "#000000"),
    ];
    let with = |color: &str| {
        let mut a = base.clone();
        a["color"] = serde_json::json!(color);
        a
    };
    let mut items: Vec<_> = colors
        .iter()
        .map(|(title, c)| menu_item(id, with(c), &format!("{title} {c}"), category))
        .collect();
    items.push(menu_item(id, with("custom"), "Custom…", category));
    items.push(menu_item(id, with("auto"), "Automatic", category));
    items
}

/// Slice Color: a pie's slice (or a point of a series) chosen, then a
/// color of its own from Excel's standard ones, typed, or its series'.
fn point_color(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::ChartKind;
    const ID: &str = "viewer.grid.pointColor";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to color its slices".into());
        return Ok(());
    };
    let chart = v.charts()[i].clone();
    let hex = |c: [u8; 3]| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let pie = matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut);
    let series = match args.get("series").and_then(serde_json::Value::as_u64) {
        Some(n) => n as usize,
        None if pie || chart.series.len() == 1 => 0,
        None => {
            let items = chart
                .series
                .iter()
                .enumerate()
                .map(|(k, s)| {
                    let name = if s.name.is_empty() {
                        format!("Series {}", k + 1)
                    } else {
                        s.name.clone()
                    };
                    menu_item(
                        ID,
                        serde_json::json!({ "series": k }),
                        &name,
                        "Point Color: series",
                    )
                })
                .collect();
            ctx.requests.push(Request::Choose(items));
            return Ok(());
        }
    };
    let Some(s) = chart.series.get(series) else {
        return Ok(());
    };
    let Some(point) = args
        .get("point")
        .and_then(serde_json::Value::as_u64)
        .map(|p| p as usize)
    else {
        let items = (0..s.values.len())
            .map(|p| {
                let label = chart
                    .categories
                    .get(p)
                    .cloned()
                    .unwrap_or_else(|| (p + 1).to_string());
                let now = s
                    .point_colors
                    .iter()
                    .find(|c| c.0 == p)
                    .map_or("series".to_string(), |c| hex(c.1));
                menu_item(
                    ID,
                    serde_json::json!({ "series": series, "point": p }),
                    &format!("{label} ({now})"),
                    if pie { "Slice Color" } else { "Point Color" },
                )
            })
            .collect();
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let typed = args.get("value").and_then(|x| x.as_str());
    match args.get("color").and_then(|c| c.as_str()).or(typed) {
        Some("auto") => with(ctx, |v| v.set_point_color(series, point, None)),
        Some("custom") => {
            let now = s
                .point_colors
                .iter()
                .find(|c| c.0 == point)
                .map_or(String::new(), |c| hex(c.1));
            ask_more(
                ctx,
                ID,
                &serde_json::json!({ "series": series, "point": point, "value_default": now }),
                "value",
            )
        }
        Some(c) => match hex_color(c) {
            Some(rgb) => with(ctx, |v| v.set_point_color(series, point, Some(rgb))),
            None => {
                ctx.messages.push(format!("Not a color: {c} (#RRGGBB)"));
                Ok(())
            }
        },
        None => {
            let label = chart
                .categories
                .get(point)
                .cloned()
                .unwrap_or_else(|| (point + 1).to_string());
            let items = color_menu(
                ID,
                &serde_json::json!({ "series": series, "point": point }),
                &format!("Color of {label}"),
            );
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
    }
}

/// Series Color: the series chosen (when there are several), then a
/// color from Excel's standard ones, typed as `#RRGGBB`, or the theme's.
fn series_color(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.seriesColor";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to color its series".into());
        return Ok(());
    };
    let chart = v.charts()[i].clone();
    let hex = |c: [u8; 3]| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let series = match args.get("series").and_then(serde_json::Value::as_u64) {
        Some(n) => n as usize,
        None if chart.series.len() == 1 => 0,
        None => {
            let items = chart
                .series
                .iter()
                .enumerate()
                .map(|(k, s)| {
                    let now = s.color.map_or("automatic".to_string(), hex);
                    let name = if s.name.is_empty() {
                        format!("Series {}", k + 1)
                    } else {
                        s.name.clone()
                    };
                    menu_item(
                        ID,
                        serde_json::json!({ "series": k }),
                        &format!("{name} ({now})"),
                        "Series Color",
                    )
                })
                .collect();
            ctx.requests.push(Request::Choose(items));
            return Ok(());
        }
    };
    match args.get("color").and_then(|c| c.as_str()) {
        Some("auto") => with(ctx, |v| v.set_series_color(series, None)),
        Some("custom") => {
            let now = chart
                .series
                .get(series)
                .and_then(|s| s.color)
                .map_or(String::new(), hex);
            ask_more(
                ctx,
                ID,
                &serde_json::json!({ "series": series, "value_default": now }),
                "value",
            )
        }
        Some(c) => match hex_color(c) {
            Some(rgb) => with(ctx, |v| v.set_series_color(series, Some(rgb))),
            None => {
                ctx.messages.push(format!("Not a color: {c} (#RRGGBB)"));
                Ok(())
            }
        },
        None => {
            // A color typed after Custom.
            if let Some(typed) = args.get("value").and_then(|x| x.as_str()) {
                return match hex_color(typed) {
                    Some(rgb) => with(ctx, |v| v.set_series_color(series, Some(rgb))),
                    None => {
                        ctx.messages.push(format!("Not a color: {typed} (#RRGGBB)"));
                        Ok(())
                    }
                };
            }
            let name = chart
                .series
                .get(series)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            let items = color_menu(
                ID,
                &serde_json::json!({ "series": series }),
                &format!("Color of {name}"),
            );
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
    }
}

/// Explode Slice: a slice of the pie under the cursor (or all of them),
/// then how far out, as Excel's Point Explosion.
fn explode_slice(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::ChartKind;
    const ID: &str = "viewer.grid.explodeSlice";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a pie to pull its slices out".into());
        return Ok(());
    };
    let chart = v.charts()[i].clone();
    if !matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut) {
        ctx.messages
            .push("Only a pie's or a doughnut's slices stand out".into());
        return Ok(());
    }
    let Some(s) = chart.series.first() else {
        return Ok(());
    };
    // Which slice: a number, or "all".
    let Some(which) = args.get("point").cloned() else {
        let now = |p: usize| {
            s.point_explosions
                .iter()
                .find(|e| e.0 == p)
                .map_or(s.explosion, |e| e.1)
        };
        let mut items = vec![menu_item(
            ID,
            serde_json::json!({ "point": "all" }),
            &format!("All Slices ({}%)", s.explosion),
            "Explode Slice",
        )];
        for p in 0..s.values.len() {
            let label = chart
                .categories
                .get(p)
                .cloned()
                .unwrap_or_else(|| (p + 1).to_string());
            items.push(menu_item(
                ID,
                serde_json::json!({ "point": p }),
                &format!("{label} ({}%)", now(p)),
                "Explode Slice",
            ));
        }
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let point = which.as_u64().map(|p| p as usize);
    let percent = args
        .get("percent")
        .and_then(serde_json::Value::as_u64)
        .map(|p| p as u32)
        .or_else(|| {
            args.get("value")
                .and_then(|x| x.as_str())
                .and_then(|t| t.trim().trim_end_matches('%').trim().parse::<u32>().ok())
        });
    if let Some(percent) = percent {
        return with(ctx, |v| v.set_explosion(point, percent));
    }
    if args.get("custom").is_some() {
        return ask_more(ctx, ID, &serde_json::json!({ "point": which }), "value");
    }
    let mut items: Vec<_> = [
        ("Put Back", 0),
        ("10%", 10),
        ("25%", 25),
        ("50%", 50),
        ("100%", 100),
    ]
    .iter()
    .map(|(title, n)| {
        menu_item(
            ID,
            serde_json::json!({ "point": which, "percent": n }),
            title,
            "Explode Slice",
        )
    })
    .collect();
    items.push(menu_item(
        ID,
        serde_json::json!({ "point": which, "custom": true }),
        "Custom…",
        "Explode Slice",
    ));
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Chart Area: its background or its border, then a color, none, or the
/// style's again.
fn chart_area(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::Paint;
    const ID: &str = "viewer.grid.chartArea";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to paint it".into());
        return Ok(());
    };
    let chart = v.charts()[i].clone();
    let show = |p: Paint| match p {
        Paint::Automatic => "automatic".to_string(),
        Paint::None => "none".to_string(),
        Paint::Color(c) => format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
    };
    let Some(part) = args
        .get("part")
        .and_then(|p| p.as_str())
        .map(str::to_string)
    else {
        let items = vec![
            menu_item(
                ID,
                serde_json::json!({ "part": "background" }),
                &format!("Background… ({})", show(chart.background)),
                "Chart Area",
            ),
            menu_item(
                ID,
                serde_json::json!({ "part": "border" }),
                &format!("Border… ({})", show(chart.border)),
                "Chart Area",
            ),
            menu_item(
                ID,
                serde_json::json!({ "part": "plotBackground" }),
                &format!("Plot Area Background… ({})", show(chart.plot_background)),
                "Chart Area",
            ),
            menu_item(
                ID,
                serde_json::json!({ "part": "plotBorder" }),
                &format!("Plot Area Border… ({})", show(chart.plot_border)),
                "Chart Area",
            ),
        ];
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    // Which paint: the chart area's or the plot area's, its fill or line.
    let plot = part.starts_with("plot");
    let background = part == "background" || part == "plotBackground";
    let (now_bg, now_border) = if plot {
        (chart.plot_background, chart.plot_border)
    } else {
        (chart.background, chart.border)
    };
    let apply = |ctx: &mut EditorContext<'_>, p: Paint| {
        let (bg, border) = if background {
            (p, now_border)
        } else {
            (now_bg, p)
        };
        if plot {
            with(ctx, |v| v.set_plot_area(bg, border))
        } else {
            with(ctx, |v| v.set_chart_area(bg, border))
        }
    };
    let typed = args.get("value").and_then(|x| x.as_str());
    match args.get("color").and_then(|c| c.as_str()).or(typed) {
        Some("auto") => apply(ctx, Paint::Automatic),
        Some("none") => apply(ctx, Paint::None),
        Some("custom") => {
            let now = match if background { now_bg } else { now_border } {
                Paint::Color(c) => format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
                _ => String::new(),
            };
            ask_more(
                ctx,
                ID,
                &serde_json::json!({ "part": part, "value_default": now }),
                "value",
            )
        }
        Some(c) => match hex_color(c) {
            Some(rgb) => apply(ctx, Paint::Color(rgb)),
            None => {
                ctx.messages.push(format!("Not a color: {c} (#RRGGBB)"));
                Ok(())
            }
        },
        None => {
            let category = match (plot, background) {
                (false, true) => "Chart Background",
                (false, false) => "Chart Border",
                (true, true) => "Plot Area Background",
                (true, false) => "Plot Area Border",
            };
            let mut items = color_menu(ID, &serde_json::json!({ "part": part }), category);
            items.insert(
                0,
                menu_item(
                    ID,
                    serde_json::json!({ "part": part, "color": "none" }),
                    if background { "No Fill" } else { "No Border" },
                    category,
                ),
            );
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
    }
}

/// Gridlines: the four kinds as a checklist in the palette, as Excel's
/// Gridlines menu; choosing one shows or hides it at once and offers the
/// list again.
fn gridlines_menu(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::{ChartKind, Gridlines};
    const ID: &str = "viewer.grid.gridlines";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to set its gridlines".into());
        return Ok(());
    };
    let chart = v.charts()[i].clone();
    if matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut) {
        ctx.messages
            .push("A pie has no axes, and no gridlines".into());
        return Ok(());
    }
    let mut g = chart.gridlines;
    if let Some(which) = args.get("toggle").and_then(|t| t.as_str()) {
        match which {
            "horizontalMajor" => g.horizontal_major = !g.horizontal_major,
            "horizontalMinor" => g.horizontal_minor = !g.horizontal_minor,
            "verticalMajor" => g.vertical_major = !g.vertical_major,
            _ => g.vertical_minor = !g.vertical_minor,
        }
        if let Err(e) = v.set_gridlines(g) {
            ctx.messages.push(e);
            return Ok(());
        }
    }
    let line = |key: &str, title: &str, on: bool| {
        menu_item(
            ID,
            serde_json::json!({ "toggle": key }),
            &format!("{} {title}", if on { "☑" } else { "☐" }),
            "Gridlines",
        )
    };
    let Gridlines {
        horizontal_major,
        horizontal_minor,
        vertical_major,
        vertical_minor,
    } = g;
    ctx.requests.push(Request::Choose(vec![
        line("horizontalMajor", "Major Horizontal", horizontal_major),
        line("horizontalMinor", "Minor Horizontal", horizontal_minor),
        line("verticalMajor", "Major Vertical", vertical_major),
        line("verticalMinor", "Minor Vertical", vertical_minor),
    ]));
    Ok(())
}

/// Axis Number Format: the value axis's labels in one of the common
/// formats, each shown with an example, a typed code, or the cells' own.
fn axis_format(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.axisFormat";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to format its axis".into());
        return Ok(());
    };
    let now = v.charts()[i].axis_format.clone();
    let typed = args.get("value").and_then(|x| x.as_str());
    match args.get("format").and_then(|f| f.as_str()).or(typed) {
        Some("auto") => with(ctx, |v| v.set_axis_format(None)),
        Some("custom") => ask_more(
            ctx,
            ID,
            &serde_json::json!({ "value_default": now.unwrap_or_default() }),
            "value",
        ),
        Some(code) => {
            let code = code.to_string();
            with(ctx, |v| v.set_axis_format(Some(code)))
        }
        None => {
            let presets = [
                "0",
                "0.00",
                "#,##0",
                "#,##0.00",
                "0%",
                "0.0%",
                "#,##0 \"₺\"",
                "\"$\"#,##0.00",
                "#,##0,\"K\"",
                "0.00E+00",
            ];
            let mark = |on: bool| if on { "●" } else { "○" };
            let mut items = vec![menu_item(
                ID,
                serde_json::json!({ "format": "auto" }),
                &format!("{} The Cells' Own", mark(now.is_none())),
                "Axis Number Format",
            )];
            for code in presets {
                items.push(menu_item(
                    ID,
                    serde_json::json!({ "format": code }),
                    &format!(
                        "{} {code}   {}",
                        mark(now.as_deref() == Some(code)),
                        format_axis_number(1234.5, code)
                    ),
                    "Axis Number Format",
                ));
            }
            items.push(menu_item(
                ID,
                serde_json::json!({ "format": "custom" }),
                "Custom…",
                "Axis Number Format",
            ));
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
    }
}

/// Axis Font and Title Font: the axis (or both) for an axis, then the
/// size, bold, italic, color and typeface, each choice taking effect at
/// once and the menu offered again.
fn font_menu(ctx: &mut EditorContext<'_>, args: &serde_json::Value, target: &str) -> CommandResult {
    let (title, legend) = (target == "title", target == "legend");
    use kalem_viewer::{AxisFont, ChartAxis, ChartKind};
    let id = match target {
        "title" => "viewer.grid.titleFont",
        "legend" => "viewer.grid.legendFont",
        _ => "viewer.grid.axisFont",
    };
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some((i, _)) = v.chart_at_cursor() else {
        ctx.messages
            .push("Put the cursor on a chart to set its axes' font".into());
        return Ok(());
    };
    let chart = v.charts()[i].clone();
    if legend && chart.legend.is_none() {
        ctx.messages
            .push("The chart has no legend: place one first (h l)".into());
        return Ok(());
    }
    if title && chart.title.is_none() {
        ctx.messages
            .push("Give the chart a title first (h t)".into());
        return Ok(());
    }
    if target == "axis" && matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut) {
        ctx.messages.push("A pie has no axes".into());
        return Ok(());
    }
    let Some(axis) = args
        .get("axis")
        .and_then(|a| a.as_str())
        .map(str::to_string)
        .or_else(|| (title || legend).then(|| target.to_string()))
    else {
        let items = [
            ("horizontal", "Horizontal Axis"),
            ("vertical", "Vertical Axis"),
            ("both", "Both Axes"),
        ]
        .iter()
        .map(|(k, t)| menu_item(id, serde_json::json!({ "axis": k }), t, "Axis Font"))
        .collect();
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let axes: Vec<ChartAxis> = match axis.as_str() {
        "horizontal" => vec![ChartAxis::Horizontal],
        "vertical" => vec![ChartAxis::Vertical],
        _ => vec![ChartAxis::Horizontal, ChartAxis::Vertical],
    };
    let now = if title {
        chart.title_font.clone()
    } else if legend {
        chart.legend_font.clone()
    } else if axes[0] == ChartAxis::Horizontal {
        chart.horizontal_font.clone()
    } else {
        chart.vertical_font.clone()
    };
    let hex = |c: [u8; 3]| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let typed = args
        .get("value")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let op = args.get("op").and_then(|o| o.as_str()).unwrap_or("");
    // What the choice makes of the font, or a question first.
    let changed: Option<AxisFont> = match op {
        "bold" => Some(AxisFont {
            bold: !now.bold,
            ..now.clone()
        }),
        "italic" => Some(AxisFont {
            italic: !now.italic,
            ..now.clone()
        }),
        "reset" => Some(AxisFont::default()),
        "size" | "face" | "customColor" if typed.is_none() => {
            let current = match op {
                "size" => now.size.map_or(String::new(), |s| format!("{s}")),
                "face" => now.face.clone().unwrap_or_default(),
                _ => now.color.map_or(String::new(), hex),
            };
            return ask_more(
                ctx,
                id,
                &serde_json::json!({ "axis": axis, "op": op, "value_default": current }),
                "value",
            );
        }
        "size" => {
            let t = typed.unwrap_or_default().replace(',', ".");
            let t = t.trim().trim_end_matches("pt").trim();
            if t.is_empty() {
                Some(AxisFont {
                    size: None,
                    ..now.clone()
                })
            } else {
                match t.parse::<f32>() {
                    Ok(n) => Some(AxisFont {
                        size: Some(n),
                        ..now.clone()
                    }),
                    Err(_) => {
                        ctx.messages.push(format!("Not a size: {t}"));
                        return Ok(());
                    }
                }
            }
        }
        "face" => {
            let t = typed.unwrap_or_default();
            Some(AxisFont {
                face: Some(t.trim().to_string()).filter(|f| !f.is_empty()),
                ..now.clone()
            })
        }
        "customColor" | "color" => {
            let c = args
                .get("color")
                .and_then(|c| c.as_str())
                .map(str::to_string)
                .or(typed);
            match c.as_deref() {
                None => {
                    let mut items = color_menu(
                        id,
                        &serde_json::json!({ "axis": axis, "op": "color" }),
                        "Axis Font Color",
                    );
                    // Custom… asks for the code under its own op.
                    if let Some(custom) = items.iter_mut().find(|it| it.title == "Custom…") {
                        custom.id = crate::palette::invocation(
                            id,
                            &serde_json::json!({ "axis": axis, "op": "customColor" }),
                        );
                    }
                    ctx.requests.push(Request::Choose(items));
                    return Ok(());
                }
                Some("auto") => Some(AxisFont {
                    color: None,
                    ..now.clone()
                }),
                Some(c) => match hex_color(c) {
                    Some(rgb) => Some(AxisFont {
                        color: Some(rgb),
                        ..now.clone()
                    }),
                    None => {
                        ctx.messages.push(format!("Not a color: {c} (#RRGGBB)"));
                        return Ok(());
                    }
                },
            }
        }
        _ => None,
    };
    let font = match changed {
        Some(f) => {
            let done = if title {
                v.set_title_font(f.clone())
            } else if legend {
                v.set_legend_font(f.clone())
            } else {
                axes.iter().try_for_each(|a| v.set_axis_font(*a, f.clone()))
            };
            if let Err(e) = done {
                ctx.messages.push(e);
                return Ok(());
            }
            f
        }
        None => now,
    };
    let c = match target {
        "title" => "Title Font",
        "legend" => "Legend Font",
        _ => "Axis Font",
    };
    let base = |op: &str| serde_json::json!({ "axis": axis, "op": op });
    let check = |on: bool| if on { "☑" } else { "☐" };
    ctx.requests.push(Request::Choose(vec![
        menu_item(
            id,
            base("size"),
            &format!(
                "Size… ({})",
                font.size.map_or("automatic".into(), |s| format!("{s} pt"))
            ),
            c,
        ),
        menu_item(id, base("bold"), &format!("{} Bold", check(font.bold)), c),
        menu_item(
            id,
            base("italic"),
            &format!("{} Italic", check(font.italic)),
            c,
        ),
        menu_item(
            id,
            base("color"),
            &format!("Color… ({})", font.color.map_or("automatic".into(), hex)),
            c,
        ),
        menu_item(
            id,
            base("face"),
            &format!(
                "Font… ({})",
                font.face.clone().unwrap_or_else(|| "automatic".into())
            ),
            c,
        ),
        menu_item(id, base("reset"), "Default Font", c),
    ]));
    Ok(())
}

/// Axis Font: the axis (or both), then its labels' size, bold, italic,
/// color and typeface, each choice taking effect at once and the menu
/// offered again.
fn axis_font(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    font_menu(ctx, args, "axis")
}

/// A fill with the user's own lists from the settings.
fn fill_with_lists(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(&mut ViewerState) -> Result<(), String>,
) -> CommandResult {
    let lists = fill_lists(ctx.config);
    with(ctx, |v| {
        v.set_fill_lists(lists);
        f(v)
    })
}

/// Custom Lists: the user's own lists a fill goes round, each removed by
/// choosing it, and new ones made from the selected cells or typed.
fn custom_lists(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.customLists";
    const KEY: &str = "spreadsheet.custom_lists";
    let now: Vec<String> = ctx
        .config
        .strings(KEY)
        .iter()
        .map(|s| s.to_string())
        .collect();
    let save = |ctx: &mut EditorContext<'_>, lists: Vec<String>| {
        ctx.requests.push(Request::SetSetting {
            key: KEY.into(),
            value: serde_json::json!(lists),
            quiet: false,
        });
        Ok(())
    };
    match args.get("op").and_then(|o| o.as_str()) {
        Some("remove") => {
            let Some(k) = args.get("index").and_then(serde_json::Value::as_u64) else {
                return Ok(());
            };
            let mut lists = now;
            if (k as usize) < lists.len() {
                lists.remove(k as usize);
            }
            save(ctx, lists)
        }
        Some("selection") => {
            let Some(v) = ctx
                .document
                .as_deref_mut()
                .and_then(|d| d.viewer.as_deref_mut())
            else {
                return Ok(());
            };
            let s = v.selection();
            let mut cells = v.grid_cells(s[0]..s[2] + 1, s[1]..s[3] + 1);
            cells.sort_by_key(|c| (c.0, c.1));
            let items: Vec<String> = cells
                .into_iter()
                .map(|c| c.2.text.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            if items.len() < 2 {
                ctx.messages
                    .push("Select the list's cells first: two or more".into());
                return Ok(());
            }
            let mut lists = now;
            lists.push(items.join(", "));
            save(ctx, lists)
        }
        Some("typed") => {
            let Some(t) = args.get("value").and_then(|x| x.as_str()) else {
                return ask_more(ctx, ID, &serde_json::json!({ "op": "typed" }), "value");
            };
            let items: Vec<&str> = t
                .split(',')
                .map(str::trim)
                .filter(|i| !i.is_empty())
                .collect();
            if items.len() < 2 {
                ctx.messages
                    .push("A list takes two or more items, separated by commas".into());
                return Ok(());
            }
            let mut lists = now;
            lists.push(items.join(", "));
            save(ctx, lists)
        }
        _ => {
            let c = "Custom Lists";
            let mut items = vec![
                menu_item(
                    ID,
                    serde_json::json!({ "op": "selection" }),
                    "Add the Selected Cells as a List",
                    c,
                ),
                menu_item(ID, serde_json::json!({ "op": "typed" }), "Add a List…", c),
            ];
            for (k, l) in now.iter().enumerate() {
                items.push(menu_item(
                    ID,
                    serde_json::json!({ "op": "remove", "index": k }),
                    &format!("✕ Remove: {l}"),
                    c,
                ));
            }
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
    }
}

/// Bold, Italic, Underline and Strikethrough: on for the selection when
/// the cursor's cell has it off, else off, as Excel toggles them.
fn toggle_font(ctx: &mut EditorContext<'_>, which: &str) -> CommandResult {
    use kalem_viewer::StyleChange;
    with(ctx, |v| {
        let c = v.cursor_cell();
        let change = match which {
            "bold" => StyleChange {
                bold: Some(!c.bold),
                ..StyleChange::default()
            },
            "italic" => StyleChange {
                italic: Some(!c.italic),
                ..StyleChange::default()
            },
            "underline" => StyleChange {
                underline: Some(!c.underline),
                ..StyleChange::default()
            },
            _ => StyleChange {
                strike: Some(!c.strike),
                ..StyleChange::default()
            },
        };
        v.change_style(change)
    })
}

/// Ctrl+arrow, and with Shift the selection taken there.
fn data_move(ctx: &mut EditorContext<'_>, rows: i64, cols: i64, extend: bool) -> CommandResult {
    with(ctx, |v| {
        let (r, c) = v.data_edge(rows, cols);
        if extend {
            v.grid_extend_to(r, c);
        } else {
            v.grid_move_to(r, c);
        }
        Ok(())
    })
}

/// Selects the whole rows (Shift+Space) or columns (Ctrl+Space) of the
/// selection.
fn select_lines(ctx: &mut EditorContext<'_>, rows: bool) -> CommandResult {
    with(ctx, |v| {
        let Some(l) = v.grid_layout() else {
            return Ok(());
        };
        let s = v.selection();
        if rows {
            v.select_range((s[0], l.max_cols.saturating_sub(1)), (s[2], 0), false);
        } else {
            v.select_range((l.max_rows.saturating_sub(1), s[1]), (0, s[3]), false);
        }
        Ok(())
    })
}

/// Select All (Ctrl+A): the data around the cursor, and again (or where
/// there is none) the whole sheet.
fn select_all(ctx: &mut EditorContext<'_>) -> CommandResult {
    with(ctx, |v| {
        let Some(l) = v.grid_layout() else {
            return Ok(());
        };
        let region = v.current_region();
        let lone = region[0] == region[2] && region[1] == region[3];
        if v.selection() == region || (lone && v.cursor_cell().text.is_empty()) {
            v.select_range(
                (l.max_rows.saturating_sub(1), l.max_cols.saturating_sub(1)),
                (0, 0),
                false,
            );
        } else {
            v.select_range((region[2], region[3]), (region[0], region[1]), false);
        }
        Ok(())
    })
}

/// Go To (F5): a cell or a range by its reference (`B5`, `A1:C3`,
/// `Sheet2!B5`).
fn go_to(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.goTo";
    let Some(reference) = text_arg(args, "value") else {
        return ask_more(ctx, ID, &serde_json::json!({}), "value");
    };
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let (sheet, cells) = match reference.rsplit_once('!') {
        Some((sh, c)) => (Some(sh.trim().trim_matches('\'').to_owned()), c),
        None => (None, reference.as_str()),
    };
    let cell = |t: &str| {
        crate::csv_tools::parse_cell(&t.replace('$', "")).map(|(r, c)| (r as u32, c as u32))
    };
    let (a, b) = match cells.split_once(':') {
        Some((x, y)) => (cell(x), cell(y)),
        None => (cell(cells), cell(cells)),
    };
    let (Some(a), Some(b)) = (a, b) else {
        // A defined name: where it refers to.
        let named = v
            .defined_names()
            .into_iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(reference.trim()))
            .map(|(_, to)| to);
        match named {
            Some(to) if !to.eq_ignore_ascii_case(&reference) => {
                return go_to(ctx, &serde_json::json!({ "value": to }));
            }
            _ => {
                ctx.messages.push(format!("Not a reference: {reference}"));
                return Ok(());
            }
        }
    };
    if let Some(name) = sheet {
        let found = v
            .structure()
            .units
            .iter()
            .position(|u| u.label.eq_ignore_ascii_case(&name));
        match found {
            Some(u) => {
                v.go_to(u);
            }
            None => {
                ctx.messages.push(format!("No sheet named {name}"));
                return Ok(());
            }
        }
    }
    // The range's far corner shown, then its first cell the cursor.
    v.grid_move_to(b.0, b.1);
    v.select_range(b, a, true);
    Ok(())
}

/// Find (Ctrl+F): the text asked, then the cursor to its first match
/// after it.
fn grid_find(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(text) = text_arg(args, "value") else {
        return ask_more(ctx, "viewer.grid.find", &serde_json::json!({}), "value");
    };
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    v.grid_search.text = text;
    find_report(ctx, true)
}

/// Find Next and Previous, with which match of how many.
fn find_report(ctx: &mut EditorContext<'_>, forward: bool) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match v.find_step(forward) {
        Ok((i, n)) => ctx
            .messages
            .push(format!("{} {i} of {n}", v.grid_search.text)),
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Replace (Ctrl+H): what to find and what with asked, then Replace All
/// or one at a time.
fn grid_replace(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.replace";
    let Some(text) = text_arg(args, "value") else {
        return ask_more(ctx, ID, &serde_json::json!({}), "value");
    };
    let Some(with) = args.get("with").and_then(|w| w.as_str()).map(str::to_owned) else {
        return ask_more(ctx, ID, &serde_json::json!({ "value": text }), "with");
    };
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    v.grid_search.text = text.clone();
    match args.get("how").and_then(|h| h.as_str()) {
        Some("all") => match v.replace_all(&with) {
            Ok(n) => ctx.messages.push(format!(
                "{n} cell{} replaced",
                if n == 1 { "" } else { "s" }
            )),
            Err(e) => ctx.messages.push(e),
        },
        Some("one") => match v.replace_one(&with) {
            Ok(true) => ctx.messages.push(format!("{text} replaced")),
            Ok(false) => ctx.messages.push(format!("{text} found: Replace again")),
            Err(e) => ctx.messages.push(e),
        },
        _ => {
            let item = |how: &str, title: &str| {
                menu_item(
                    ID,
                    serde_json::json!({ "value": text, "with": with, "how": how }),
                    title,
                    "Replace",
                )
            };
            ctx.requests.push(Request::Choose(vec![
                item("all", "Replace All"),
                item("one", "Replace"),
            ]));
        }
    }
    Ok(())
}

/// Find's Match Case, Match Entire Cell Contents and Look in Formulas,
/// turned on or off.
fn find_option(ctx: &mut EditorContext<'_>, which: &str) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let (flag, name) = match which {
        "case" => (&mut v.grid_search.case, "Match Case"),
        "whole" => (&mut v.grid_search.whole, "Match Entire Cell Contents"),
        _ => (&mut v.grid_search.formulas, "Look in Formulas"),
    };
    *flag = !*flag;
    let state = if *flag { "on" } else { "off" };
    ctx.messages.push(format!("{name} {state}"));
    Ok(())
}

/// The sheet commands: Insert, Delete (asked first), Rename, Move Left and
/// Right, Hide and Unhide.
fn sheet_command(ctx: &mut EditorContext<'_>, args: &serde_json::Value, op: &str) -> CommandResult {
    use kalem_viewer::SheetEdit;
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let unit = v.unit;
    let n = v.structure().units.len();
    let edit = match op {
        "insert" => SheetEdit::Insert(unit),
        "delete" => {
            if args.get("confirmed").and_then(serde_json::Value::as_bool) != Some(true) {
                let name = v.structure().units[unit].label.clone();
                let question = format!("Delete the sheet {name}? This cannot be undone in Excel");
                let item =
                    |id: &str, args: serde_json::Value, title: &str| crate::palette::PaletteItem {
                        id: crate::palette::invocation(id, &args),
                        title: title.into(),
                        category: question.clone(),
                        keys: String::new(),
                        also: question.clone(),
                    };
                ctx.requests.push(Request::Choose(vec![
                    item(
                        "viewer.grid.deleteSheet",
                        serde_json::json!({ "confirmed": true }),
                        "Delete",
                    ),
                    item("viewer.grid.cancel", serde_json::json!({}), "Cancel"),
                ]));
                return Ok(());
            }
            SheetEdit::Delete(unit)
        }
        "rename" => match text_arg(args, "value") {
            Some(name) => SheetEdit::Rename(unit, name.trim().to_owned()),
            None => {
                // Asked with the name it has.
                let name = v.structure().units[unit].label.clone();
                let name = name.trim_end_matches(" (hidden)").to_owned();
                return ask_more(
                    ctx,
                    "viewer.grid.renameSheet",
                    &serde_json::json!({ "value_default": name }),
                    "value",
                );
            }
        },
        "left" | "right" => {
            let to = if op == "left" {
                unit.checked_sub(1)
            } else {
                (unit + 1 < n).then_some(unit + 1)
            };
            let Some(to) = to else { return Ok(()) };
            SheetEdit::Move(unit, to)
        }
        "hide" => SheetEdit::Hide(unit, true),
        _ => {
            let hidden = v.hidden_units();
            let pick = args.get("unit").and_then(serde_json::Value::as_u64);
            match pick {
                Some(u) => SheetEdit::Hide(u as usize, false),
                None if hidden.is_empty() => {
                    ctx.messages.push("No sheet is hidden".into());
                    return Ok(());
                }
                None => {
                    let labels: Vec<String> = v
                        .structure()
                        .units
                        .iter()
                        .map(|u| u.label.clone())
                        .collect();
                    let items = hidden
                        .iter()
                        .map(|&u| {
                            menu_item(
                                "viewer.grid.unhideSheet",
                                serde_json::json!({ "unit": u }),
                                labels[u].trim_end_matches(" (hidden)"),
                                "Unhide",
                            )
                        })
                        .collect();
                    ctx.requests.push(Request::Choose(items));
                    return Ok(());
                }
            }
        }
    };
    with(ctx, |v| v.edit_sheets(edit))
}

/// Freeze Panes at the cursor (the rows above it, the columns left of
/// it), or Unfreeze Panes when they are frozen, as Excel's one command.
fn freeze_panes(ctx: &mut EditorContext<'_>) -> CommandResult {
    with(ctx, |v| {
        let frozen = v.grid_layout().map_or((0, 0), |l| l.frozen);
        if frozen != (0, 0) {
            return v.set_frozen(0, 0);
        }
        let p = v.grid_pos();
        if (p.row, p.col) == (0, 0) {
            return Err("Put the cursor below and right of what to freeze".into());
        }
        v.set_frozen(p.row, p.col)
    })
}

/// New Note or Edit Note (Shift+F2): the note asked, with the text it has;
/// left empty, the note goes.
fn edit_note(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match args.get("value").and_then(|x| x.as_str()) {
        None => {
            let current = v.cursor_note().unwrap_or_default();
            ask_more(
                ctx,
                "viewer.grid.editNote",
                &serde_json::json!({ "value_default": current }),
                "value",
            )
        }
        Some(t) if t.trim().is_empty() => with(ctx, |v| v.set_note(None)),
        Some(t) => {
            let t = t.to_owned();
            with(ctx, |v| v.set_note(Some(t)))
        }
    }
}

/// AutoSum (Alt+=): for one cell, its SUM proposed in the cell's edit
/// prompt; for a selection, the sums put under it.
fn auto_sum(ctx: &mut EditorContext<'_>) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let s = v.selection();
    if s[0] != s[2] || s[1] != s[3] {
        return with(ctx, |v| v.auto_sum_selection().map(|_| ()));
    }
    let formula = v.auto_sum_formula();
    ask_cell(ctx, Some(&formula))
}

/// Insert Function (Shift+F3): the functions with their arguments; the one
/// chosen begins the cursor's cell's formula.
fn insert_function(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.insertFunction";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match args.get("name").and_then(|n| n.as_str()) {
        Some(name) => {
            let start = format!("={name}(");
            ask_cell(ctx, Some(&start))
        }
        None => {
            let items = v
                .formula_functions()
                .into_iter()
                .map(|(name, a)| {
                    menu_item(
                        ID,
                        serde_json::json!({ "name": name }),
                        &format!("{name}({a})"),
                        "Insert Function",
                    )
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                ctx.messages.push("No functions for this file".into());
            } else {
                ctx.requests.push(Request::Choose(items));
            }
            Ok(())
        }
    }
}

/// Paste Special (Ctrl+Alt+V): what of the cells copied, from a menu.
fn paste_special(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::PasteKind;
    const ID: &str = "viewer.grid.pasteSpecial";
    let transpose = args.get("transpose").and_then(serde_json::Value::as_bool) == Some(true);
    let kind = match args.get("what").and_then(|w| w.as_str()) {
        Some("all") => PasteKind::All,
        Some("values") => PasteKind::Values,
        Some("formats") => PasteKind::Formats,
        Some("formulas") => PasteKind::Formulas,
        _ => {
            let item = |what: &str, transpose: bool, title: &str| {
                menu_item(
                    ID,
                    serde_json::json!({ "what": what, "transpose": transpose }),
                    title,
                    "Paste Special",
                )
            };
            ctx.requests.push(Request::Choose(vec![
                item("values", false, "Values"),
                item("formulas", false, "Formulas"),
                item("formats", false, "Formats"),
                item("all", false, "All"),
                item("all", true, "Transpose"),
                item("values", true, "Values, Transposed"),
            ]));
            return Ok(());
        }
    };
    with(ctx, |v| v.paste_special(kind, transpose))
}

/// Remove Duplicates: by all the table's columns or one of them, then
/// what went said as Excel says it.
fn remove_duplicates(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.removeDuplicates";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let columns: Vec<u32> = match args.get("columns") {
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_u64().map(|c| c as u32))
            .collect(),
        _ => {
            let (_, _, cols) = v.duplicates_target();
            let mut items = vec![menu_item(
                ID,
                serde_json::json!({ "columns": [] }),
                "All Columns",
                "Remove Duplicates",
            )];
            items.extend(cols.into_iter().map(|(c, name)| {
                menu_item(
                    ID,
                    serde_json::json!({ "columns": [c] }),
                    &name,
                    "Remove Duplicates",
                )
            }));
            ctx.requests.push(Request::Choose(items));
            return Ok(());
        }
    };
    match v.remove_duplicates(&columns) {
        Ok((0, _)) => ctx.messages.push("No duplicate values found".into()),
        Ok((n, left)) => ctx.messages.push(format!(
            "{n} duplicate row{} removed; {left} unique {}",
            if n == 1 { "" } else { "s" },
            if left == 1 {
                "row remains"
            } else {
                "rows remain"
            }
        )),
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Text to Columns: the delimiter from a menu (or typed), then the split.
fn text_to_columns(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.textToColumns";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let delimiter = match args.get("delimiter").and_then(|d| d.as_str()) {
        Some("other") => return ask_more(ctx, ID, &serde_json::json!({}), "value"),
        Some(d) => d.to_owned(),
        None => match args.get("value").and_then(|d| d.as_str()) {
            Some(d) => d.to_owned(),
            None => {
                let item = |d: &str, title: &str| {
                    menu_item(
                        ID,
                        serde_json::json!({ "delimiter": d }),
                        title,
                        "Text to Columns",
                    )
                };
                ctx.requests.push(Request::Choose(vec![
                    item("\t", "Tab"),
                    item(";", "Semicolon ;"),
                    item(",", "Comma ,"),
                    item(" ", "Space"),
                    item("other", "Other…"),
                ]));
                return Ok(());
            }
        },
    };
    match v.text_to_columns(&delimiter) {
        Ok(n) => ctx
            .messages
            .push(format!("{n} cell{} split", if n == 1 { "" } else { "s" })),
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Insert Link (Ctrl+K): the address asked, with the one the cell has; a
/// place in the workbook after `#`; left empty, the link goes.
fn insert_link(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match args.get("value").and_then(|x| x.as_str()) {
        None => {
            let current = v.cursor_link().unwrap_or_default();
            ask_more(
                ctx,
                "viewer.grid.insertLink",
                &serde_json::json!({ "value_default": current }),
                "value",
            )
        }
        Some(t) if t.trim().is_empty() => with(ctx, |v| v.set_link(None)),
        Some(t) => {
            let t = t.trim().to_owned();
            with(ctx, |v| v.set_link(Some(t)))
        }
    }
}

/// Open Link (`g x`): an address with the system, a file beside the
/// workbook, a place in it by Go To.
fn open_link(ctx: &mut EditorContext<'_>) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some(link) = v.cursor_link() else {
        ctx.messages.push("The cell has no link".into());
        return Ok(());
    };
    if let Some(place) = link.strip_prefix('#') {
        return go_to(ctx, &serde_json::json!({ "value": place }));
    }
    let web = link.contains("://") || link.starts_with("mailto:");
    let action = if web {
        crate::input::LinkAction::Url(link)
    } else {
        crate::input::LinkAction::File {
            path: link,
            search: None,
        }
    };
    ctx.requests.push(Request::OpenLink(action));
    Ok(())
}

/// Define Name: a name asked for the selection.
fn define_name(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(name) = text_arg(args, "value") else {
        return ask_more(
            ctx,
            "viewer.grid.defineName",
            &serde_json::json!({}),
            "value",
        );
    };
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let to = v.selection_reference();
    let name = name.trim().to_owned();
    match v.set_defined_name(&name, Some(&to)) {
        Ok(()) => ctx.messages.push(format!("{name} = {to}")),
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Name Manager (Ctrl+F3) and Delete Name: the names with what they refer
/// to; the one chosen gone to, or deleted.
fn names_menu(
    ctx: &mut EditorContext<'_>,
    args: &serde_json::Value,
    delete: bool,
) -> CommandResult {
    let id = if delete {
        "viewer.grid.deleteName"
    } else {
        "viewer.grid.nameManager"
    };
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if let Some(name) = args.get("name").and_then(|n| n.as_str()) {
        if delete {
            let name = name.to_owned();
            return with(ctx, |v| v.set_defined_name(&name, None));
        }
        return go_to(ctx, &serde_json::json!({ "value": name }));
    }
    let names = v.defined_names();
    if names.is_empty() {
        ctx.messages.push("The workbook has no names".into());
        return Ok(());
    }
    let category = if delete {
        "Delete Name"
    } else {
        "Name Manager"
    };
    let items = names
        .into_iter()
        .map(|(n, to)| {
            menu_item(
                id,
                serde_json::json!({ "name": n }),
                &format!("{n}  {to}"),
                category,
            )
        })
        .collect();
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Calculate Now (F9): every formula computed again.
fn calculate_now(ctx: &mut EditorContext<'_>) -> CommandResult {
    with(ctx, |v| {
        v.doc().recalculate().map_err(|e| e.to_string())?;
        v.refresh();
        Ok(())
    })
}

/// Save Sheet as CSV: the sheet shown written to a CSV file (asked, beside
/// the workbook by default; one already there replaced after asking),
/// the workbook left as it is.
fn save_sheet_csv(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.saveSheetAsCsv";
    let Some(doc) = ctx.document.as_deref_mut() else {
        return Ok(());
    };
    let book = doc.meta.path.clone();
    let Some(v) = doc.viewer.as_deref_mut() else {
        return Ok(());
    };
    let sheet = v.structure().units[v.unit]
        .label
        .trim_end_matches(" (hidden)")
        .to_owned();
    let Some(path) = text_arg(args, "value") else {
        let stem = book
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or("Book".into(), |s| s.to_string_lossy().into_owned());
        let name = format!("{stem} - {sheet}.csv");
        let default = book
            .as_ref()
            .and_then(|p| p.parent())
            .map_or(name.clone(), |d| d.join(&name).display().to_string());
        return ask_more(
            ctx,
            ID,
            &serde_json::json!({ "value_default": default }),
            "value",
        );
    };
    let mut target = std::path::PathBuf::from(path.trim());
    if target.is_relative()
        && let Some(dir) = book.as_ref().and_then(|p| p.parent())
    {
        target = dir.join(target);
    }
    if target.extension().is_none() {
        target.set_extension("csv");
    }
    let confirmed = args.get("confirmed").and_then(serde_json::Value::as_bool) == Some(true);
    if target.exists() && !confirmed {
        let file = target
            .file_name()
            .map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let question = format!("{file} exists: replace it?");
        let item = |id: &str, a: serde_json::Value, title: &str| crate::palette::PaletteItem {
            id: crate::palette::invocation(id, &a),
            title: title.into(),
            category: question.clone(),
            keys: String::new(),
            also: question.clone(),
        };
        ctx.requests.push(Request::Choose(vec![
            item(
                ID,
                serde_json::json!({ "value": target.display().to_string(), "confirmed": true }),
                "Replace",
            ),
            item("viewer.grid.cancel", serde_json::json!({}), "Cancel"),
        ]));
        return Ok(());
    }
    let text = v.sheet_csv();
    std::fs::write(&target, text).map_err(|e| crate::command::CommandError::new(e.to_string()))?;
    ctx.messages
        .push(format!("{sheet} saved as {}", target.display()));
    Ok(())
}

/// Today's date (`time` off) or the time now, as typed into a cell.
fn now_entry(time: bool) -> String {
    let now = jiff::Zoned::now().datetime();
    if time {
        format!("{:02}:{:02}", now.hour(), now.minute())
    } else {
        format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day())
    }
}

/// Ctrl+; and Ctrl+Shift+;: today's date or the time entered into the
/// cursor's cell.
fn insert_now(ctx: &mut EditorContext<'_>, time: bool) -> CommandResult {
    with(ctx, |v| {
        if !v.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = v.grid_pos();
        v.set_cell(p.row, p.col, &now_entry(time))
    })
}

/// Ctrl+' and Ctrl+Shift+": the cell above's formula (as it is written)
/// or its value (as shown), to be entered into the cursor's cell.
fn from_above(ctx: &mut EditorContext<'_>, value: bool) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let p = v.grid_pos();
    if p.row == 0 {
        return Ok(());
    }
    let text = if value {
        v.grid_cells(p.row - 1..p.row, p.col..p.col + 1)
            .first()
            .map(|c| c.2.text.clone())
            .unwrap_or_default()
    } else {
        v.doc().cell_input(v.unit, p.row - 1, p.col)
    };
    ask_cell(ctx, Some(&text))
}

/// Custom Sort: a column, then its order (A to Z, Z to A, a custom
/// list), then Sort or another level, the levels so far in `keys`.
fn custom_sort(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::SortKey;
    const ID: &str = "viewer.grid.customSort";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let mut keys: Vec<serde_json::Value> = args
        .get("keys")
        .and_then(|k| k.as_array())
        .cloned()
        .unwrap_or_default();
    let key_of = |k: &serde_json::Value| SortKey {
        col: k
            .get("col")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
        descending: k.get("descending").and_then(serde_json::Value::as_bool) == Some(true),
        list: k.get("list").and_then(|l| l.as_array()).map(|l| {
            l.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        }),
    };
    if args.get("go").and_then(serde_json::Value::as_bool) == Some(true) {
        let keys: Vec<SortKey> = keys.iter().map(key_of).collect();
        return with(ctx, |v| v.sort_by(&keys));
    }
    let columns = v.sort_columns();
    let name = |c: u64| {
        columns
            .iter()
            .find(|x| u64::from(x.0) == c)
            .map_or(String::new(), |x| x.1.clone())
    };
    let level = if keys.is_empty() {
        "Sort By"
    } else {
        "Then By"
    };
    match (
        args.get("col").and_then(serde_json::Value::as_u64),
        args.get("order"),
    ) {
        (None, _) => {
            let items = columns
                .iter()
                .map(|(c, n)| {
                    menu_item(ID, serde_json::json!({ "keys": keys, "col": c }), n, level)
                })
                .collect();
            ctx.requests.push(Request::Choose(items));
        }
        (Some(c), None) => {
            let order = |o: serde_json::Value, title: &str| {
                menu_item(
                    ID,
                    serde_json::json!({ "keys": keys, "col": c, "order": o }),
                    title,
                    &format!("{level} {}", name(c)),
                )
            };
            let mut items = vec![
                order(serde_json::json!("asc"), "A to Z, Smallest to Largest"),
                order(serde_json::json!("desc"), "Z to A, Largest to Smallest"),
            ];
            for l in v.sort_lists() {
                let shown: Vec<&str> = l.iter().take(3).map(String::as_str).collect();
                items.push(order(
                    serde_json::json!(l),
                    &format!("Custom List: {}, …", shown.join(", ")),
                ));
            }
            ctx.requests.push(Request::Choose(items));
        }
        (Some(c), Some(o)) => {
            let mut key = serde_json::json!({ "col": c, "descending": o == "desc" });
            if o.is_array() {
                key["list"] = o.clone();
            }
            keys.push(key);
            let described: Vec<String> = keys
                .iter()
                .map(|k| name(k["col"].as_u64().unwrap_or(0)))
                .collect();
            let category = format!("Sort by {}", described.join(", then "));
            ctx.requests.push(Request::Choose(vec![
                menu_item(
                    ID,
                    serde_json::json!({ "keys": keys, "go": true }),
                    "Sort",
                    &category,
                ),
                menu_item(
                    ID,
                    serde_json::json!({ "keys": keys }),
                    "Then By…",
                    &category,
                ),
            ]));
        }
    }
    Ok(())
}

/// Filter by Condition: Excel's Text and Number Filters for the cursor's
/// column, their values asked.
fn filter_condition(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::{FilterOp, FilterRule};
    const ID: &str = "viewer.grid.filterCondition";
    let Some(op) = args.get("op").and_then(|o| o.as_str()) else {
        let ops = [
            ("equal", "Equals…"),
            ("notEqual", "Does Not Equal…"),
            ("beginsWith", "Begins With…"),
            ("endsWith", "Ends With…"),
            ("contains", "Contains…"),
            ("notContains", "Does Not Contain…"),
            ("greater", "Greater Than…"),
            ("greaterOrEqual", "Greater Than or Equal To…"),
            ("less", "Less Than…"),
            ("lessOrEqual", "Less Than or Equal To…"),
            ("between", "Between…"),
            ("top", "Top 10…"),
            ("bottom", "Bottom 10…"),
            ("above", "Above Average"),
            ("below", "Below Average"),
        ];
        let items = ops
            .iter()
            .map(|(o, t)| menu_item(ID, serde_json::json!({ "op": o }), t, "Filter"))
            .collect();
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let value = args
        .get("value")
        .and_then(|x| x.as_str())
        .map(str::to_owned);
    let needs_value = !matches!(op, "above" | "below");
    let Some(value) = value.or_else(|| (!needs_value).then(String::new)) else {
        let default = if matches!(op, "top" | "bottom") {
            "10"
        } else {
            ""
        };
        return ask_more(
            ctx,
            ID,
            &serde_json::json!({ "op": op, "value_default": default }),
            "value",
        );
    };
    let op_of = |o: &str| match o {
        "notEqual" => FilterOp::NotEqual,
        "beginsWith" => FilterOp::BeginsWith,
        "endsWith" => FilterOp::EndsWith,
        "contains" => FilterOp::Contains,
        "notContains" => FilterOp::NotContains,
        "greater" => FilterOp::Greater,
        "greaterOrEqual" => FilterOp::GreaterOrEqual,
        "less" => FilterOp::Less,
        "lessOrEqual" => FilterOp::LessOrEqual,
        _ => FilterOp::Equal,
    };
    let rule = match op {
        "between" => {
            let Some(to) = args.get("to").and_then(|x| x.as_str()) else {
                return ask_more(
                    ctx,
                    ID,
                    &serde_json::json!({ "op": op, "value": value }),
                    "to",
                );
            };
            FilterRule::Custom {
                first: (FilterOp::GreaterOrEqual, value),
                second: Some((true, FilterOp::LessOrEqual, to.to_owned())),
            }
        }
        "top" | "bottom" => match value.trim().parse::<u32>() {
            Ok(count) => FilterRule::Top {
                count,
                percent: false,
                bottom: op == "bottom",
            },
            Err(_) => {
                ctx.messages.push(format!("Not a number: {value}"));
                return Ok(());
            }
        },
        "above" | "below" => FilterRule::Average {
            above: op == "above",
        },
        o => FilterRule::Custom {
            first: (op_of(o), value),
            second: None,
        },
    };
    with(ctx, |v| v.filter_rule(Some(rule)))
}

/// Filter by Selected Cell's Color: the cursor's column filtered to cells
/// filled as the cursor's cell.
fn filter_by_color(ctx: &mut EditorContext<'_>) -> CommandResult {
    with(ctx, |v| {
        let Some(fill) = v.cursor_cell().fill else {
            return Err("The cell has no fill color".into());
        };
        v.filter_rule(Some(kalem_viewer::FilterRule::Fill(fill)))
    })
}

/// Format as Table (Ctrl+T): a style from the menu, then the table.
fn format_as_table(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.formatAsTable";
    let Some(style) = args.get("style").and_then(|s| s.as_str()) else {
        let styles = [
            ("TableStyleMedium2", "Blue"),
            ("TableStyleMedium3", "Orange"),
            ("TableStyleMedium4", "Gray"),
            ("TableStyleMedium5", "Gold"),
            ("TableStyleMedium6", "Light Blue"),
            ("TableStyleMedium7", "Green"),
        ];
        let items = styles
            .iter()
            .map(|(s, t)| menu_item(ID, serde_json::json!({ "style": s }), t, "Table Style"))
            .collect();
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let style = style.to_owned();
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match v.format_as_table(&style) {
        Ok(name) => ctx.messages.push(format!("{name} made")),
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Cell Styles: Excel's built-in styles, applied as their formats.
fn cell_style(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::{BorderSet, StyleChange};
    const ID: &str = "viewer.grid.cellStyle";
    let styles = [
        ("normal", "Normal"),
        ("good", "Good"),
        ("bad", "Bad"),
        ("neutral", "Neutral"),
        ("heading1", "Heading 1"),
        ("heading2", "Heading 2"),
        ("heading3", "Heading 3"),
        ("heading4", "Heading 4"),
        ("title", "Title"),
        ("total", "Total"),
        ("comma", "Comma"),
        ("currency", "Currency"),
        ("percent", "Percent"),
    ];
    let Some(name) = args.get("style").and_then(|x| x.as_str()) else {
        let items = styles
            .iter()
            .map(|(k, t)| menu_item(ID, serde_json::json!({ "style": k }), t, "Cell Styles"))
            .collect();
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let dark = Some([0x44, 0x54, 0x6A]);
    let fill_font = |fill: [u8; 3], font: [u8; 3]| StyleChange {
        fill: Some(Some(fill)),
        color: Some(Some(font)),
        ..StyleChange::default()
    };
    let heading = |size: f32, line: Option<[u8; 3]>| StyleChange {
        bold: Some(true),
        size: Some(size),
        color: Some(dark),
        borders: line.map(|c| (BorderSet::Bottom, Some(c))),
        ..StyleChange::default()
    };
    let change = match name {
        "normal" => return with(ctx, |v| v.clear_formats(false)),
        "good" => fill_font([0xC6, 0xEF, 0xCE], [0x00, 0x61, 0x00]),
        "bad" => fill_font([0xFF, 0xC7, 0xCE], [0x9C, 0x00, 0x06]),
        "neutral" => fill_font([0xFF, 0xEB, 0x9C], [0x9C, 0x57, 0x00]),
        "heading1" => heading(15.0, Some([0x44, 0x72, 0xC4])),
        "heading2" => heading(13.0, Some([0xA2, 0xB8, 0xE1])),
        "heading3" => heading(11.0, Some([0x8E, 0xA9, 0xDB])),
        "heading4" => heading(11.0, None),
        "title" => StyleChange {
            size: Some(18.0),
            color: Some(dark),
            face: Some("Calibri Light".into()),
            ..StyleChange::default()
        },
        "total" => StyleChange {
            bold: Some(true),
            borders: Some((BorderSet::Top, Some([0x44, 0x72, 0xC4]))),
            ..StyleChange::default()
        },
        "comma" => StyleChange {
            number_format: Some("#,##0.00".into()),
            ..StyleChange::default()
        },
        "currency" => StyleChange {
            number_format: Some("#,##0.00 \"₺\"".into()),
            ..StyleChange::default()
        },
        "percent" => StyleChange {
            number_format: Some("0%".into()),
            ..StyleChange::default()
        },
        other => {
            ctx.messages.push(format!("No such style: {other}"));
            return Ok(());
        }
    };
    with(ctx, |v| v.change_style(change))
}

/// Increase (`by` 1) or Decrease (-1) Indent of the selection, from the
/// cursor's cell's indent.
fn step_indent(ctx: &mut EditorContext<'_>, by: i32) -> CommandResult {
    with(ctx, |v| {
        let now = i32::from(v.cursor_cell().indent);
        let n = (now + by).clamp(0, 15) as u8;
        v.change_style(kalem_viewer::StyleChange {
            indent: Some(n),
            ..kalem_viewer::StyleChange::default()
        })
    })
}

/// Orientation: the text's angle, from Excel's menu.
fn text_rotation(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.textRotation";
    let Some(r) = args.get("rotation").and_then(serde_json::Value::as_u64) else {
        let item =
            |r: u16, t: &str| menu_item(ID, serde_json::json!({ "rotation": r }), t, "Orientation");
        ctx.requests.push(Request::Choose(vec![
            item(45, "Angle Counterclockwise"),
            item(135, "Angle Clockwise"),
            item(255, "Vertical Text"),
            item(90, "Rotate Text Up"),
            item(180, "Rotate Text Down"),
            item(0, "Horizontal"),
        ]));
        return Ok(());
    };
    with(ctx, |v| {
        v.change_style(kalem_viewer::StyleChange {
            rotation: Some(r as u16),
            ..kalem_viewer::StyleChange::default()
        })
    })
}

/// A toggle of the cursor's cell (Shrink to Fit, Center Across Selection)
/// given to the selection.
fn toggle_alignment(ctx: &mut EditorContext<'_>, which: &str) -> CommandResult {
    with(ctx, |v| {
        let c = v.cursor_cell();
        let change = if which == "shrink" {
            kalem_viewer::StyleChange {
                shrink: Some(!c.shrink),
                ..kalem_viewer::StyleChange::default()
            }
        } else {
            kalem_viewer::StyleChange {
                center_across: Some(!c.center_across),
                ..kalem_viewer::StyleChange::default()
            }
        };
        v.change_style(change)
    })
}

/// Go To Special (Ctrl+G is the palette: `g s`): the kinds of cell, then
/// those cells selected together.
fn go_to_special(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.goToSpecial";
    let Some(kind) = args.get("kind").and_then(|k| k.as_str()) else {
        let item =
            |k: &str, t: &str| menu_item(ID, serde_json::json!({ "kind": k }), t, "Go To Special");
        ctx.requests.push(Request::Choose(vec![
            item("blanks", "Blanks"),
            item("constants", "Constants"),
            item("formulas", "Formulas"),
            item("errors", "Errors"),
            item("visible", "Visible Cells Only"),
            item("last", "Last Cell"),
            item("notes", "Notes"),
            item("conditional", "Conditional Formats"),
            item("validation", "Data Validation"),
        ]));
        return Ok(());
    };
    let kind = kind.to_owned();
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match v.go_to_special(&kind) {
        Ok((cells, areas)) if kind != "last" => ctx.messages.push(format!(
            "{cells} cell{} in {areas} range{}",
            if cells == 1 { "" } else { "s" },
            if areas == 1 { "" } else { "s" }
        )),
        Ok(_) => {}
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Insert Picture: a file chosen, then the picture over the cells from
/// the cursor's.
fn insert_picture(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(path) = text_arg(args, "path") else {
        ctx.requests.push(Request::PickFile {
            command: "viewer.grid.insertPicture".into(),
            arg: "path".into(),
            args: serde_json::json!({}),
        });
        return Ok(());
    };
    let Some(doc) = ctx.document.as_deref_mut() else {
        return Ok(());
    };
    let mut file = std::path::PathBuf::from(crate::settings::expand_home(&path));
    if file.is_relative()
        && let Some(dir) = doc.meta.path.as_ref().and_then(|p| p.parent())
    {
        file = dir.join(file);
    }
    with(ctx, |v| v.insert_picture(&file))
}

/// Insert Shape: a shape (or a text box) from the menu, its text asked.
fn insert_shape(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.insertShape";
    let Some(shape) = args.get("shape").and_then(|x| x.as_str()) else {
        let item =
            |k: &str, t: &str| menu_item(ID, serde_json::json!({ "shape": k }), t, "Insert Shape");
        ctx.requests.push(Request::Choose(vec![
            item("textBox", "Text Box"),
            item("rect", "Rectangle"),
            item("roundRect", "Rounded Rectangle"),
            item("ellipse", "Oval"),
            item("rightArrow", "Right Arrow"),
        ]));
        return Ok(());
    };
    let Some(text) = args
        .get("value")
        .and_then(|x| x.as_str())
        .map(str::to_owned)
    else {
        return ask_more(ctx, ID, &serde_json::json!({ "shape": shape }), "value");
    };
    let shape = shape.to_owned();
    with(ctx, |v| {
        if shape == "textBox" {
            v.insert_shape("rect", &text, true)
        } else {
            v.insert_shape(&shape, &text, false)
        }
    })
}

/// Insert Sparklines: line, column or win/loss from the menu, then the
/// cells to draw them in (the column right of the selection offered).
fn insert_sparklines(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::SparklineKind;
    const ID: &str = "viewer.grid.insertSparklines";
    let Some(kind) = args.get("kind").and_then(|x| x.as_str()) else {
        let item = |k: &str, t: &str| {
            menu_item(ID, serde_json::json!({ "kind": k }), t, "Insert Sparklines")
        };
        ctx.requests.push(Request::Choose(vec![
            item("line", "Line"),
            item("column", "Column"),
            item("winLoss", "Win/Loss"),
        ]));
        return Ok(());
    };
    let kind = match kind {
        "column" => SparklineKind::Column,
        "winLoss" => SparklineKind::WinLoss,
        _ => SparklineKind::Line,
    };
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let name =
        |r: u32, c: u32| format!("{}{}", crate::csv_tools::column_letters(c as usize), r + 1);
    let Some(place) = text_arg(args, "value") else {
        let p = v.sparkline_place();
        let default = if p[0] == p[2] && p[1] == p[3] {
            name(p[0], p[1])
        } else {
            format!("{}:{}", name(p[0], p[1]), name(p[2], p[3]))
        };
        return ask_more(
            ctx,
            ID,
            &serde_json::json!({ "kind": args.get("kind"), "value_default": default }),
            "value",
        );
    };
    let Some(location) = area(&place) else {
        ctx.messages.push(format!("Not cells: {place}"));
        return Ok(());
    };
    with(ctx, |v| v.insert_sparklines(kind, location))
}

/// A cell or a range as typed (`B3`, `$B$3`, `B3:D5`): its corners.
fn area(text: &str) -> Option<[u32; 4]> {
    let cell = |t: &str| {
        crate::csv_tools::parse_cell(&t.trim().replace('$', "")).map(|(r, c)| (r as u32, c as u32))
    };
    let (a, b) = match text.split_once(':') {
        Some((x, y)) => (cell(x)?, cell(y)?),
        None => (cell(text)?, cell(text)?),
    };
    Some([a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)])
}

/// A cell as typed, or `None`.
fn one_cell(text: &str) -> Option<(u32, u32)> {
    area(text)
        .filter(|a| a[0] == a[2] && a[1] == a[3])
        .map(|a| (a[0], a[1]))
}

/// A cell's name, `B3`.
fn cell_name(r: u32, c: u32) -> String {
    format!("{}{}", crate::csv_tools::column_letters(c as usize), r + 1)
}

/// Goal Seek: the formula cell (the cursor's offered), the value it is to
/// come to, and the cell to change.
fn goal_seek(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.goalSeek";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some(set) = text_arg(args, "set cell") else {
        let p = v.grid_pos();
        let mut a = args.clone();
        a["set cell_default"] = serde_json::json!(cell_name(p.row, p.col));
        return ask_more(ctx, ID, &a, "set cell");
    };
    let Some(to) = text_arg(args, "to value") else {
        return ask_more(ctx, ID, args, "to value");
    };
    let Some(by) = text_arg(args, "by changing cell") else {
        return ask_more(ctx, ID, args, "by changing cell");
    };
    let (Some(set_at), Some(by_at)) = (one_cell(&set), one_cell(&by)) else {
        ctx.messages
            .push("Goal Seek: give single cells, such as B4".into());
        return Ok(());
    };
    let Ok(target) = to.trim().replace(',', "").parse::<f64>() else {
        ctx.messages
            .push(format!("Goal Seek: {to} is not a number"));
        return Ok(());
    };
    match v.goal_seek(set_at, target, by_at) {
        Ok(Some(x)) => {
            let (set, by) = (set.trim().to_uppercase(), by.trim().to_uppercase());
            ctx.messages
                .push(format!("Goal Seek: {by} = {x} makes {set} {to}"));
        }
        Ok(None) => ctx.messages.push(format!(
            "Goal Seek found no value of {by} that makes {set} {to}"
        )),
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Data Table: the selection a what-if table; its row and column input
/// cells asked (either left empty for a table of one variable).
fn data_table(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.dataTable";
    let Some(row) = args.get("row input cell").and_then(|x| x.as_str()) else {
        return ask_more(ctx, ID, args, "row input cell");
    };
    let Some(col) = args.get("column input cell").and_then(|x| x.as_str()) else {
        return ask_more(ctx, ID, args, "column input cell");
    };
    let input = |t: &str| -> Result<Option<(u32, u32)>, String> {
        if t.trim().is_empty() {
            return Ok(None);
        }
        one_cell(t)
            .map(Some)
            .ok_or_else(|| format!("Data Table: {t} is not a cell"))
    };
    match (input(row), input(col)) {
        (Ok(r), Ok(c)) => with(ctx, |v| v.create_data_table(r, c)),
        (Err(e), _) | (_, Err(e)) => {
            ctx.messages.push(e);
            Ok(())
        }
    }
}

/// Scenario Manager: each scenario shown or deleted from the menu, or one
/// added: its name, its cells (the selection offered) and a comment.
fn scenarios(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.scenarios";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some(what) = args.get("what").and_then(|x| x.as_str()) else {
        let mut items = Vec::new();
        for s in v.scenarios() {
            let cells: Vec<String> = s
                .cells
                .iter()
                .map(|(r, c, x)| format!("{}={x}", cell_name(*r, *c)))
                .collect();
            let about = if s.comment.is_empty() {
                cells.join(", ")
            } else {
                format!("{} ({})", cells.join(", "), s.comment)
            };
            items.push(menu_item(
                ID,
                serde_json::json!({ "what": "show", "name": s.name }),
                &format!("Show {}: {about}", s.name),
                "Scenarios",
            ));
            items.push(menu_item(
                ID,
                serde_json::json!({ "what": "delete", "name": s.name }),
                &format!("Delete {}", s.name),
                "Scenarios",
            ));
        }
        items.push(menu_item(
            ID,
            serde_json::json!({ "what": "add" }),
            "Add Scenario…",
            "Scenarios",
        ));
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let what = what.to_owned();
    let Some(name) = text_arg(args, "name") else {
        return ask_more(ctx, ID, args, "name");
    };
    if what != "add" {
        return with(ctx, |v| v.scenario(&what, &name, &[], ""));
    }
    let Some(cells) = text_arg(args, "changing cells") else {
        let s = v.selection();
        let mut a = args.clone();
        a["changing cells_default"] = serde_json::json!(if s[0] == s[2] && s[1] == s[3] {
            cell_name(s[0], s[1])
        } else {
            format!("{}:{}", cell_name(s[0], s[1]), cell_name(s[2], s[3]))
        });
        return ask_more(ctx, ID, &a, "changing cells");
    };
    let Some(comment) = args.get("comment").and_then(|x| x.as_str()) else {
        return ask_more(ctx, ID, args, "comment");
    };
    let mut list = Vec::new();
    for part in cells.split([',', ';']) {
        let Some(a) = area(part) else {
            ctx.messages
                .push(format!("Scenarios: {} is not cells", part.trim()));
            return Ok(());
        };
        for r in a[0]..=a[2] {
            for c in a[1]..=a[3] {
                if list.len() < 64 {
                    list.push((r, c));
                }
            }
        }
    }
    let comment = comment.to_owned();
    with(ctx, |v| v.scenario("add", &name, &list, &comment))
}

/// Zoom: the sheet shown at a percentage asked (10 to 400).
fn zoom_to(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(text) = text_arg(args, "value") else {
        let now = ctx
            .document
            .as_deref_mut()
            .and_then(|d| d.viewer.as_deref_mut())
            .map_or(100, |v| v.sheet_view().zoom);
        return ask_more(
            ctx,
            "viewer.grid.zoom",
            &serde_json::json!({ "value_default": now.to_string() }),
            "value",
        );
    };
    match text.trim().trim_end_matches('%').trim().parse::<u16>() {
        Ok(z) if (10..=400).contains(&z) => with(ctx, |v| v.update_view(|s| s.zoom = z)),
        _ => {
            ctx.messages
                .push(format!("Zoom: {text} is not 10% to 400%"));
            Ok(())
        }
    }
}

/// The menu a right click opens (or Shift+F10, for the cells): the
/// commands for the cells, the rows, the columns or a sheet's tab.
fn context_menu(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let on = args.get("on").and_then(|x| x.as_str()).unwrap_or("cells");
    if on == "tab"
        && let Some(u) = args.get("unit").and_then(serde_json::Value::as_u64)
        && let Some(v) = ctx
            .document
            .as_deref_mut()
            .and_then(|d| d.viewer.as_deref_mut())
    {
        v.go_to(u as usize);
    }
    let none = serde_json::json!({});
    let list: &[(&str, &str)] = match on {
        "rows" => &[
            ("edit.cut", "Cut"),
            ("edit.copy", "Copy"),
            ("edit.paste", "Paste"),
            ("viewer.grid.insertRow", "Insert Rows"),
            ("viewer.grid.deleteRow", "Delete Rows"),
            ("viewer.grid.clear", "Clear Contents"),
            ("viewer.grid.fitRowHeight", "Row Height to Fit"),
            ("viewer.grid.tallerRow", "Taller Row"),
            ("viewer.grid.shorterRow", "Shorter Row"),
            ("viewer.grid.hideRows", "Hide"),
            ("viewer.grid.unhideRows", "Unhide"),
            ("viewer.grid.group", "Group"),
        ],
        "cols" => &[
            ("edit.cut", "Cut"),
            ("edit.copy", "Copy"),
            ("edit.paste", "Paste"),
            ("viewer.grid.insertColumn", "Insert Columns"),
            ("viewer.grid.deleteColumn", "Delete Columns"),
            ("viewer.grid.clear", "Clear Contents"),
            ("viewer.grid.autofitColumn", "Column Width to Fit"),
            ("viewer.grid.widenColumn", "Wider Column"),
            ("viewer.grid.narrowColumn", "Narrower Column"),
            ("viewer.grid.hideColumns", "Hide"),
            ("viewer.grid.unhideColumns", "Unhide"),
            ("viewer.grid.group", "Group"),
        ],
        "tab" => &[
            ("viewer.grid.insertSheet", "Insert Sheet"),
            ("viewer.grid.deleteSheet", "Delete Sheet"),
            ("viewer.grid.renameSheet", "Rename"),
            ("viewer.grid.moveSheetLeft", "Move Left"),
            ("viewer.grid.moveSheetRight", "Move Right"),
            ("viewer.grid.moveOrCopySheet", "Move or Copy…"),
            ("viewer.grid.tabColor", "Tab Color"),
            ("viewer.grid.hideSheet", "Hide"),
            ("viewer.grid.unhideSheet", "Unhide"),
            ("viewer.grid.protectSheet", "Protect Sheet"),
            ("viewer.grid.sheetList", "All Sheets"),
        ],
        _ => &[
            ("edit.cut", "Cut"),
            ("edit.copy", "Copy"),
            ("edit.paste", "Paste"),
            ("viewer.grid.pasteSpecial", "Paste Special"),
            ("viewer.grid.insertCells", "Insert…"),
            ("viewer.grid.deleteCells", "Delete…"),
            ("viewer.grid.clear", "Clear Contents"),
            ("viewer.grid.clearFormats", "Clear Formats"),
            ("viewer.grid.sortAscending", "Sort A to Z"),
            ("viewer.grid.sortDescending", "Sort Z to A"),
            ("viewer.grid.toggleFilter", "Filter"),
            ("viewer.grid.numberFormat", "Number Format"),
            ("viewer.grid.cellStyle", "Cell Style"),
            ("viewer.grid.newComment", "New Comment"),
            ("viewer.grid.editNote", "Note"),
            ("viewer.grid.insertLink", "Link"),
            ("viewer.grid.defineName", "Define Name"),
        ],
    };
    let category = match on {
        "rows" => "Rows",
        "cols" => "Columns",
        "tab" => "Sheet",
        _ => "Cells",
    };
    let items = list
        .iter()
        .map(|(id, title)| menu_item(id, none.clone(), title, category))
        .collect();
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Move or Copy: the sheet shown copied after itself or to the end, moved
/// to the start or the end, or copied or moved into another workbook
/// (chosen as a file).
fn move_or_copy(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::SheetEdit;
    const ID: &str = "viewer.grid.moveOrCopySheet";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let Some(what) = args.get("what").and_then(|x| x.as_str()).map(str::to_owned) else {
        let item =
            |w: &str, t: &str| menu_item(ID, serde_json::json!({ "what": w }), t, "Move or Copy");
        ctx.requests.push(Request::Choose(vec![
            item("copy", "Copy (after this sheet)"),
            item("copyEnd", "Copy to the End"),
            item("first", "Move to the Beginning"),
            item("last", "Move to the End"),
            item("copyOut", "Copy to Another Workbook…"),
            item("moveOut", "Move to Another Workbook…"),
        ]));
        return Ok(());
    };
    let (unit, n) = (v.unit, v.structure().units.len());
    match what.as_str() {
        "copy" => with(ctx, |v| v.edit_sheets(SheetEdit::Copy(unit, unit + 1))),
        "copyEnd" => with(ctx, |v| v.edit_sheets(SheetEdit::Copy(unit, n))),
        "first" => with(ctx, |v| v.edit_sheets(SheetEdit::Move(unit, 0))),
        "last" => with(ctx, |v| v.edit_sheets(SheetEdit::Move(unit, n - 1))),
        "copyOut" | "moveOut" => {
            let Some(path) = text_arg(args, "workbook") else {
                ctx.requests.push(Request::PickFile {
                    command: ID.into(),
                    arg: "workbook".into(),
                    args: serde_json::json!({ "what": what }),
                });
                return Ok(());
            };
            let path = std::path::PathBuf::from(crate::settings::expand_home(&path));
            let keep = what == "copyOut";
            match v.copy_sheet_to_file(&path, keep) {
                Ok(name) => {
                    let file = path
                        .file_name()
                        .map_or(String::new(), |f| f.to_string_lossy().into_owned());
                    ctx.messages.push(format!(
                        "{} as {name} into {file}",
                        if keep { "Copied" } else { "Moved" }
                    ));
                }
                Err(e) => ctx.messages.push(e),
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Calculation Options: Automatic, Automatic except Data Tables or
/// Manual; iterative calculation on (its limits asked) or off.
fn calculation_options(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::CalcMode;
    const ID: &str = "viewer.grid.calculationOptions";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let now = v.doc_calc_options();
    let Some(what) = args.get("what").and_then(|x| x.as_str()).map(str::to_owned) else {
        let mark = |on: bool, t: &str| {
            if on {
                format!("{t} ✓")
            } else {
                t.to_string()
            }
        };
        let item = |w: &str, t: String| {
            menu_item(
                ID,
                serde_json::json!({ "what": w }),
                &t,
                "Calculation Options",
            )
        };
        let mut items = vec![
            item(
                "automatic",
                mark(now.mode == CalcMode::Automatic, "Automatic"),
            ),
            item(
                "exceptTables",
                mark(
                    now.mode == CalcMode::AutomaticExceptTables,
                    "Automatic except Data Tables",
                ),
            ),
            item("manual", mark(now.mode == CalcMode::Manual, "Manual")),
        ];
        items.push(if now.iterate {
            item(
                "iterateOff",
                format!(
                    "Iterative Calculation ✓ ({} times, {})",
                    now.max_iterations, now.max_change
                ),
            )
        } else {
            item("iterate", "Iterative Calculation…".into())
        });
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    match what.as_str() {
        "automatic" => with(ctx, |v| v.update_calc(|o| o.mode = CalcMode::Automatic)),
        "exceptTables" => with(ctx, |v| {
            v.update_calc(|o| o.mode = CalcMode::AutomaticExceptTables)
        }),
        "manual" => with(ctx, |v| v.update_calc(|o| o.mode = CalcMode::Manual)),
        "iterateOff" => with(ctx, |v| v.update_calc(|o| o.iterate = false)),
        "iterate" => {
            let Some(times) = text_arg(args, "maximum iterations") else {
                let mut a = args.clone();
                a["maximum iterations_default"] = serde_json::json!(now.max_iterations.to_string());
                return ask_more(ctx, ID, &a, "maximum iterations");
            };
            let Some(change) = text_arg(args, "maximum change") else {
                let mut a = args.clone();
                a["maximum change_default"] = serde_json::json!(now.max_change.to_string());
                return ask_more(ctx, ID, &a, "maximum change");
            };
            match (
                times.trim().parse::<u32>(),
                change.trim().replace(',', ".").parse::<f64>(),
            ) {
                (Ok(n), Ok(d)) => with(ctx, |v| {
                    v.update_calc(|o| {
                        o.iterate = true;
                        o.max_iterations = n;
                        o.max_change = d;
                    })
                }),
                _ => {
                    ctx.messages.push(format!(
                        "Iterative Calculation: {times} times, {change}: not numbers"
                    ));
                    Ok(())
                }
            }
        }
        _ => Ok(()),
    }
}

/// Circular References: the cells round a circle, one chosen to go to.
fn circular_references(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.circularReferences";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if let (Some(u), Some(r), Some(c)) = (
        args.get("unit").and_then(serde_json::Value::as_u64),
        args.get("row").and_then(serde_json::Value::as_u64),
        args.get("col").and_then(serde_json::Value::as_u64),
    ) {
        v.go_to(u as usize);
        v.grid_move_to(r as u32, c as u32);
        return Ok(());
    }
    let all = v.circular_references();
    if all.is_empty() {
        ctx.messages.push("No circular references".into());
        return Ok(());
    }
    let labels: Vec<String> = v
        .structure()
        .units
        .iter()
        .map(|u| u.label.clone())
        .collect();
    let items = all
        .into_iter()
        .map(|(u, r, c)| {
            menu_item(
                ID,
                serde_json::json!({ "unit": u, "row": r, "col": c }),
                &format!(
                    "{}!{}",
                    labels.get(u).cloned().unwrap_or_default(),
                    cell_name(r, c)
                ),
                "Circular References",
            )
        })
        .collect();
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Who writes comments: the account's name.
fn comment_author() -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "Kalem".into())
}

/// New Comment: the cursor's cell gets a comment, or its thread a reply.
fn new_comment(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(text) = text_arg(args, "value") else {
        return ask_more(ctx, "viewer.grid.newComment", args, "value");
    };
    let author = comment_author();
    with(ctx, |v| v.add_comment(&author, &text))
}

/// Comments: the cursor's thread read, replied to, resolved or deleted
/// from the menu; the sheet's other threads gone to.
fn comments_menu(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.comments";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match args.get("what").and_then(|x| x.as_str()) {
        Some("resolve") => return with(ctx, |v| v.resolve_comment(true)),
        Some("reopen") => return with(ctx, |v| v.resolve_comment(false)),
        Some("delete") => {
            let i = args
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            return with(ctx, |v| v.delete_comment(i as usize));
        }
        Some("go") => {
            let (Some(r), Some(c)) = (
                args.get("row").and_then(serde_json::Value::as_u64),
                args.get("col").and_then(serde_json::Value::as_u64),
            ) else {
                return Ok(());
            };
            v.grid_move_to(r as u32, c as u32);
            return Ok(());
        }
        _ => {}
    }
    let here = v.cursor_thread();
    let p = v.grid_pos();
    let mut items = Vec::new();
    if let Some(t) = &here {
        let cell = cell_name(t.row, t.col);
        for (i, c) in t.comments.iter().enumerate() {
            let when = c.time.get(..16).unwrap_or(&c.time).replace('T', " ");
            items.push(menu_item(
                ID,
                serde_json::json!({ "what": "delete", "index": i }),
                &format!(
                    "{} {} ({when}): {}",
                    if i == 0 {
                        "Delete thread:"
                    } else {
                        "Delete reply:"
                    },
                    c.author,
                    c.text.replace('\n', " ")
                ),
                &format!("Comments on {cell}"),
            ));
        }
        items.insert(
            0,
            menu_item(
                "viewer.grid.newComment",
                serde_json::json!({}),
                "Reply…",
                &format!("Comments on {cell}"),
            ),
        );
        items.insert(
            1,
            if t.done {
                menu_item(
                    ID,
                    serde_json::json!({ "what": "reopen" }),
                    "Reopen Thread",
                    &format!("Comments on {cell}"),
                )
            } else {
                menu_item(
                    ID,
                    serde_json::json!({ "what": "resolve" }),
                    "Resolve Thread",
                    &format!("Comments on {cell}"),
                )
            },
        );
    } else {
        items.push(menu_item(
            "viewer.grid.newComment",
            serde_json::json!({}),
            "New Comment…",
            &format!("Comments on {}", cell_name(p.row, p.col)),
        ));
    }
    for t in v.threads() {
        if (t.row, t.col) == (p.row, p.col) {
            continue;
        }
        let Some(first) = t.comments.first() else {
            continue;
        };
        items.push(menu_item(
            ID,
            serde_json::json!({ "what": "go", "row": t.row, "col": t.col }),
            &format!(
                "Go to {}: {}: {}",
                cell_name(t.row, t.col),
                first.author,
                first.text.replace('\n', " ")
            ),
            "Comments on the sheet",
        ));
    }
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Tab Color: one of Excel's standard colors, a hex color, or none.
fn tab_color(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.tabColor";
    const COLORS: [(&str, &str); 10] = [
        ("Dark Red", "C00000"),
        ("Red", "FF0000"),
        ("Orange", "FFC000"),
        ("Yellow", "FFFF00"),
        ("Light Green", "92D050"),
        ("Green", "00B050"),
        ("Light Blue", "00B0F0"),
        ("Blue", "0070C0"),
        ("Dark Blue", "002060"),
        ("Purple", "7030A0"),
    ];
    let Some(color) = args.get("color").and_then(|x| x.as_str()) else {
        let mut items: Vec<_> = COLORS
            .iter()
            .map(|(name, hex)| {
                menu_item(ID, serde_json::json!({ "color": hex }), name, "Tab Color")
            })
            .collect();
        items.push(menu_item(
            ID,
            serde_json::json!({ "color": "other" }),
            "More Colors…",
            "Tab Color",
        ));
        items.push(menu_item(
            ID,
            serde_json::json!({ "color": "none" }),
            "No Color",
            "Tab Color",
        ));
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let hex = match color {
        "none" => return with(ctx, |v| v.set_tab_color(None)),
        "other" => match text_arg(args, "value") {
            Some(h) => h,
            None => return ask_more(ctx, ID, args, "value"),
        },
        h => h.to_owned(),
    };
    let h = hex.trim().trim_start_matches('#');
    let rgb = (h.len() == 6)
        .then(|| u32::from_str_radix(h, 16).ok())
        .flatten()
        .map(|n| [(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    let Some(rgb) = rgb else {
        ctx.messages
            .push(format!("Tab Color: {hex} is not a color such as #C00000"));
        return Ok(());
    };
    with(ctx, |v| v.set_tab_color(Some(rgb)))
}

/// Sheet List: the workbook's sheets, one chosen to go to (Excel's list
/// on the tab arrows).
fn sheet_list(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if let Some(u) = args.get("unit").and_then(serde_json::Value::as_u64) {
        v.go_to(u as usize);
        return Ok(());
    }
    let items = v
        .sheet_tabs()
        .into_iter()
        .map(|(u, name, _)| {
            menu_item(
                "viewer.grid.sheetList",
                serde_json::json!({ "unit": u }),
                &name,
                "Sheets",
            )
        })
        .collect();
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Edit Shape Text: the shape at the cursor's text asked, with what it has.
fn edit_shape_text(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match args.get("value").and_then(|x| x.as_str()) {
        Some(t) => {
            let t = t.to_owned();
            with(ctx, |v| v.set_shape_text(&t))
        }
        None => {
            let now = match v.drawing_at_cursor() {
                Some((
                    _,
                    kalem_viewer::Drawing {
                        kind: kalem_viewer::DrawingKind::Shape { text, .. },
                        ..
                    },
                )) => text,
                Some(_) => {
                    ctx.messages.push("A picture has no text".into());
                    return Ok(());
                }
                None => {
                    ctx.messages.push("No shape at the cursor".into());
                    return Ok(());
                }
            };
            ask_more(
                ctx,
                "viewer.grid.editShapeText",
                &serde_json::json!({ "value_default": now }),
                "value",
            )
        }
    }
}

/// Evaluate Formula (`z e`): the cursor's formula's steps, as a list.
fn evaluate_formula(ctx: &mut EditorContext<'_>) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let steps = v.evaluation_steps();
    if steps.is_empty() {
        ctx.messages.push("The cell has no formula".into());
        return Ok(());
    }
    let items = steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            menu_item(
                "viewer.grid.cancel",
                serde_json::json!({}),
                &format!("{}. {s}", i + 1),
                "Evaluate Formula",
            )
        })
        .collect();
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Watch Window (`z w`): the watched cells with their values; Add Watch
/// for the cursor's cell, a watched one gone to or removed.
fn watch_window(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.watchWindow";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let num = |k: &str| {
        args.get(k)
            .and_then(serde_json::Value::as_u64)
            .map(|x| x as usize)
    };
    match (args.get("do").and_then(|d| d.as_str()), num("i")) {
        (Some("add"), _) => {
            let p = v.grid_pos();
            let w = (v.unit, p.row, p.col);
            if !v.watches.contains(&w) {
                v.watches.push(w);
            }
        }
        (Some("go"), Some(i)) => {
            if let Some(&(u, r, c)) = v.watches.get(i) {
                v.go_to(u);
                v.grid_move_to(r, c);
            }
            return Ok(());
        }
        (Some("remove"), Some(i)) if i < v.watches.len() => {
            v.watches.remove(i);
        }
        _ => {}
    }
    let mut items = vec![menu_item(
        ID,
        serde_json::json!({ "do": "add" }),
        "Add Watch (the cursor's cell)",
        "Watch Window",
    )];
    for (i, line) in v.watch_lines().into_iter().enumerate() {
        items.push(menu_item(
            ID,
            serde_json::json!({ "do": "go", "i": i }),
            &line,
            "Watch Window",
        ));
        items.push(menu_item(
            ID,
            serde_json::json!({ "do": "remove", "i": i }),
            &format!(
                "Delete Watch: {}",
                line.split(" = ").next().unwrap_or_default()
            ),
            "Watch Window",
        ));
    }
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Protect Sheet (`z k`): a password asked (none when left empty), then
/// what the protected sheet still allows; on a protected sheet, Unprotect
/// Sheet, its password asked when it has one.
fn protect_sheet(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::SheetProtection;
    const ID: &str = "viewer.grid.protectSheet";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let password = args
        .get("password")
        .and_then(|p| p.as_str())
        .map(str::to_owned);
    if let Some(now) = v.sheet_protection() {
        if now.has_password && password.is_none() {
            return ask_more(ctx, ID, &serde_json::json!({}), "password");
        }
        return with(ctx, |v| v.protect_sheet(None, password.as_deref()));
    }
    let Some(password) = password else {
        return ask_more(ctx, ID, &serde_json::json!({}), "password");
    };
    let Some(allow) = args.get("allow").and_then(|a| a.as_str()) else {
        let item = |a: &str, t: &str| {
            menu_item(
                ID,
                serde_json::json!({ "password": password, "allow": a }),
                t,
                "Protect Sheet",
            )
        };
        ctx.requests.push(Request::Choose(vec![
            item("none", "Protect: select cells only"),
            item(
                "format",
                "Protect, allowing formatting cells, columns and rows",
            ),
            item("sortFilter", "Protect, allowing sorting and filtering"),
            item("rows", "Protect, allowing inserting and deleting rows"),
        ]));
        return Ok(());
    };
    let mut p = SheetProtection::default();
    match allow {
        "format" => {
            p.format_cells = true;
            p.format_columns = true;
            p.format_rows = true;
        }
        "sortFilter" => {
            p.sort = true;
            p.filter = true;
        }
        "rows" => {
            p.insert_rows = true;
            p.delete_rows = true;
        }
        _ => {}
    }
    let pw = (!password.is_empty()).then_some(password);
    with(ctx, |v| v.protect_sheet(Some(p), pw.as_deref()))
}

/// Protect Workbook (`z K`): its structure, with a password asked; again
/// to unprotect it.
fn protect_workbook(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.protectWorkbook";
    let Some(password) = args
        .get("password")
        .and_then(|p| p.as_str())
        .map(str::to_owned)
    else {
        return ask_more(ctx, ID, &serde_json::json!({}), "password");
    };
    with(ctx, |v| {
        let on = !v.doc().workbook_protected();
        let pw = (!password.is_empty()).then_some(password.as_str());
        v.protect_workbook(on, pw)
    })
}

/// Subtotal: at each change in a column, a function, added to a column,
/// each chosen from a menu.
fn subtotal(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.subtotal";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let columns = v.sort_columns();
    let num = |k: &str| args.get(k).and_then(serde_json::Value::as_u64);
    let item = |a: serde_json::Value, t: &str, c: &str| menu_item(ID, a, t, c);
    match (num("by"), num("function"), num("col")) {
        (None, _, _) => {
            let items = columns
                .iter()
                .map(|(c, n)| item(serde_json::json!({ "by": c }), n, "At Each Change In"))
                .collect();
            ctx.requests.push(Request::Choose(items));
        }
        (Some(by), None, _) => {
            let f = |code: u32, t: &str| {
                item(
                    serde_json::json!({ "by": by, "function": code }),
                    t,
                    "Use Function",
                )
            };
            ctx.requests.push(Request::Choose(vec![
                f(9, "Sum"),
                f(2, "Count"),
                f(1, "Average"),
                f(4, "Max"),
                f(5, "Min"),
            ]));
        }
        (Some(by), Some(f), None) => {
            let items = columns
                .iter()
                .filter(|(c, _)| u64::from(*c) != by)
                .map(|(c, n)| {
                    item(
                        serde_json::json!({ "by": by, "function": f, "col": c }),
                        n,
                        "Add Subtotal To",
                    )
                })
                .collect();
            ctx.requests.push(Request::Choose(items));
        }
        (Some(by), Some(f), Some(col)) => {
            return with(ctx, |v| v.subtotal(by as u32, f as u32, &[col as u32]));
        }
    }
    Ok(())
}

/// Page Setup (`z p`): orientation, paper, margins, fitting one page wide,
/// the print area, the rows printed on every page, the header and the
/// footer, page breaks.
fn page_setup(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.pageSetup";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let mut s = v.page_setup();
    let sel = v.selection();
    let row = v.grid_pos().row;
    let what = args.get("what").and_then(|w| w.as_str()).unwrap_or("");
    let value = args
        .get("value")
        .and_then(|x| x.as_str())
        .map(str::to_owned);
    let item = |what: &str, title: &str| {
        menu_item(ID, serde_json::json!({ "what": what }), title, "Page Setup")
    };
    let choice = |what: &str, value: &str, title: &str| {
        menu_item(
            ID,
            serde_json::json!({ "what": what, "value": value }),
            title,
            "Page Setup",
        )
    };
    match what {
        "" => {
            let on = |b: bool| if b { "on" } else { "off" };
            let items = vec![
                item(
                    "orientation",
                    if s.landscape {
                        "Orientation: Landscape"
                    } else {
                        "Orientation: Portrait"
                    },
                ),
                item("paper", "Paper Size…"),
                item("margins", "Margins…"),
                item("fit", &format!("Fit to One Page Wide: {}", on(s.fit_width))),
                item("area", "Set Print Area (the selection)"),
                item("clearArea", "Clear Print Area"),
                item(
                    "titles",
                    "Print Titles (the selection's rows on every page)",
                ),
                item("clearTitles", "Clear Print Titles"),
                item("header", "Header…"),
                item("footer", "Footer…"),
                item("break", "Insert Page Break (above the cursor's row)"),
                item("unbreak", "Remove Page Break"),
                item("resetBreaks", "Reset All Page Breaks"),
            ];
            ctx.requests.push(Request::Choose(items));
            return Ok(());
        }
        "orientation" => s.landscape = !s.landscape,
        "paper" => match value.as_deref().and_then(|v| v.parse().ok()) {
            Some(code) => s.paper = code,
            None => {
                ctx.requests.push(Request::Choose(vec![
                    choice("paper", "9", "A4"),
                    choice("paper", "1", "Letter"),
                    choice("paper", "5", "Legal"),
                    choice("paper", "8", "A3"),
                ]));
                return Ok(());
            }
        },
        "margins" => match value.as_deref() {
            Some("normal") => s.margins = [0.7, 0.7, 0.75, 0.75],
            Some("wide") => s.margins = [1.0, 1.0, 1.0, 1.0],
            Some("narrow") => s.margins = [0.25, 0.25, 0.75, 0.75],
            _ => {
                ctx.requests.push(Request::Choose(vec![
                    choice("margins", "normal", "Normal"),
                    choice("margins", "wide", "Wide"),
                    choice("margins", "narrow", "Narrow"),
                ]));
                return Ok(());
            }
        },
        "fit" => s.fit_width = !s.fit_width,
        "area" => s.print_area = Some(sel),
        "clearArea" => s.print_area = None,
        "titles" => s.title_rows = Some((sel[0], sel[2])),
        "clearTitles" => s.title_rows = None,
        "header" | "footer" => match value {
            Some(text) => {
                if what == "header" {
                    s.header = text;
                } else {
                    s.footer = text;
                }
            }
            None => {
                let now = if what == "header" {
                    &s.header
                } else {
                    &s.footer
                };
                return ask_more(
                    ctx,
                    ID,
                    &serde_json::json!({ "what": what, "value_default": now }),
                    "value",
                );
            }
        },
        "break" => {
            if row > 0 && !s.row_breaks.contains(&row) {
                s.row_breaks.push(row);
            }
        }
        "unbreak" => s.row_breaks.retain(|r| *r != row),
        "resetBreaks" => s.row_breaks.clear(),
        _ => return Ok(()),
    }
    with(ctx, |v| v.set_page_setup(&s))
}

/// Print Preview, Export to PDF and Print: the sheet shown (or the
/// selection, or every visible sheet of the workbook) as LaTeX, compiled
/// with LuaLaTeX in the background; then shown in Kalem, written beside
/// the workbook, or handed to the system's print dialog.
fn sheet_pdf(ctx: &mut EditorContext<'_>, args: &serde_json::Value, mode: &str) -> CommandResult {
    const EXPORT: &str = "viewer.grid.exportPdf";
    let Some(doc) = ctx.document.as_deref_mut() else {
        return Ok(());
    };
    let book = doc.meta.path.clone();
    let Some(v) = doc.viewer.as_deref_mut() else {
        return Ok(());
    };
    let scope = args.get("scope").and_then(|x| x.as_str());
    if mode == "export" && scope.is_none() {
        let item = |s: &str, t: &str| {
            menu_item(
                EXPORT,
                serde_json::json!({ "scope": s }),
                t,
                "Export to PDF",
            )
        };
        ctx.requests.push(Request::Choose(vec![
            item("sheet", "Active Sheet"),
            item("selection", "Selection"),
            item("workbook", "Entire Workbook"),
        ]));
        return Ok(());
    }
    let sheets: Vec<crate::sheet_print::SheetPrint> = match scope.unwrap_or("sheet") {
        "workbook" => {
            let hidden = v.hidden_units();
            let units: Vec<usize> = (0..v.structure().units.len())
                .filter(|u| !hidden.contains(u) && v.is_grid_unit(*u))
                .collect();
            units
                .into_iter()
                .filter_map(|u| v.sheet_print(u, None))
                .collect()
        }
        "selection" => {
            let sel = v.selection();
            let unit = v.unit;
            v.sheet_print(unit, Some(sel)).into_iter().collect()
        }
        _ => {
            let unit = v.unit;
            v.sheet_print(unit, None).into_iter().collect()
        }
    };
    if sheets.is_empty() {
        ctx.messages
            .push("Nothing to print: the sheet is empty".into());
        return Ok(());
    }
    let file = book
        .as_ref()
        .and_then(|p| p.file_name())
        .map_or(String::new(), |f| f.to_string_lossy().into_owned());
    let now = jiff::Zoned::now().datetime();
    let date = format!("{:02}.{:02}.{:04}", now.day(), now.month(), now.year());
    let time = format!("{:02}:{:02}", now.hour(), now.minute());
    let tex = crate::sheet_print::document(&sheets, &file, &date, &time);
    let tool = crate::pdf::detect(crate::pdf::Engine::LuaLatex, &crate::pdf::tex_search_path())
        .ok_or_else(|| crate::command::CommandError::new(crate::l10n::tr("msg-no-latex")))?;
    let dir = std::env::temp_dir().join(format!(
        "kalem-print-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).map_err(|e| crate::command::CommandError::new(e.to_string()))?;
    let stem = book
        .as_ref()
        .and_then(|p| p.file_stem())
        .map_or("Book".into(), |s| s.to_string_lossy().into_owned());
    let name = match scope {
        Some("workbook") => stem.clone(),
        _ => format!("{stem} - {}", sheets[0].name),
    };
    let tex_path = dir.join(format!("{}.tex", name.replace(['/', '\\'], "-")));
    std::fs::write(&tex_path, tex).map_err(|e| crate::command::CommandError::new(e.to_string()))?;
    let target = book
        .as_ref()
        .and_then(|p| p.parent())
        .map(|d| d.join(format!("{name}.pdf")));
    let mode = mode.to_owned();
    ctx.messages.push(crate::l10n::tr("msg-compiling-pdf"));
    crate::jobs::spawn(crate::l10n::tr("msg-compiling-pdf"), move || {
        let failed = |message: String| crate::jobs::Finished {
            message,
            error: true,
            open: None,
        };
        // Twice: the second run knows the number of pages.
        let _ = crate::pdf::compile(&tool, crate::pdf::Engine::LuaLatex, &tex_path);
        let compiled = match crate::pdf::compile(&tool, crate::pdf::Engine::LuaLatex, &tex_path) {
            Ok(c) => c,
            Err(e) => return failed(format!("The PDF was not made: {e}")),
        };
        let Some(pdf) = compiled.pdf else {
            let first = compiled
                .problems
                .iter()
                .find(|p| p.error)
                .map_or(String::new(), |p| p.message.clone());
            return failed(format!("The PDF was not made: {first}"));
        };
        match mode.as_str() {
            "export" => {
                let Some(target) = target else {
                    return failed("Save the workbook first".into());
                };
                if let Err(e) = std::fs::copy(&pdf, &target) {
                    return failed(e.to_string());
                }
                crate::jobs::Finished {
                    message: format!("Saved as {}", target.display()),
                    error: false,
                    open: None,
                }
            }
            "print" => crate::jobs::Finished {
                message: "Printing".into(),
                error: false,
                open: Some(crate::input::LinkAction::Print(pdf)),
            },
            _ => crate::jobs::Finished {
                message: String::new(),
                error: false,
                open: Some(crate::input::LinkAction::File {
                    path: pdf.display().to_string(),
                    search: None,
                }),
            },
        }
    });
    Ok(())
}

/// Format Painter (`t p`): pressed once it takes the selection's format,
/// again it paints it over the selection then chosen.
fn format_painter(ctx: &mut EditorContext<'_>) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    match v.format_painter() {
        Ok(false) => ctx
            .messages
            .push("Format Painter: select the cells, then t p again (Escape: none)".into()),
        Ok(true) => {}
        Err(e) => ctx.messages.push(e),
    }
    Ok(())
}

/// Number Format: the selection's, from Excel's common ones or typed.
fn number_format(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::StyleChange;
    const ID: &str = "viewer.grid.numberFormat";
    let typed = args.get("value").and_then(|x| x.as_str());
    match args.get("code").and_then(|c| c.as_str()).or(typed) {
        None => {
            let formats = [
                ("General", "General"),
                ("Number", "#,##0.00"),
                ("Currency", "#,##0.00 \"₺\""),
                ("Percentage", "0.00%"),
                ("Short Date", "dd.mm.yyyy"),
                ("Long Date", "d mmmm yyyy dddd"),
                ("Time", "hh:mm:ss"),
                ("Fraction", "# ?/?"),
                ("Scientific", "0.00E+00"),
                ("Text", "@"),
            ];
            let mut items: Vec<_> = formats
                .iter()
                .map(|(title, code)| {
                    menu_item(
                        ID,
                        serde_json::json!({ "code": code }),
                        &format!("{title}  {code}"),
                        "Number Format",
                    )
                })
                .collect();
            items.push(menu_item(
                ID,
                serde_json::json!({ "code": "custom" }),
                "Custom…",
                "Number Format",
            ));
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
        Some("custom") => ask_more(ctx, ID, &serde_json::json!({}), "value"),
        Some(code) => with(ctx, |v| {
            v.change_style(StyleChange {
                number_format: Some(code.to_owned()),
                ..StyleChange::default()
            })
        }),
    }
}

/// Increase or Decrease Decimal: the cursor's cell's format with a place
/// more or fewer, given to the selection.
fn step_decimals(ctx: &mut EditorContext<'_>, by: i32) -> CommandResult {
    use kalem_viewer::StyleChange;
    with(ctx, |v| {
        let code = v.cursor_format().unwrap_or_else(|| "General".into());
        let shown = v.cursor_cell().text;
        match change_decimals(&code, by, &shown) {
            Some(code) => v.change_style(StyleChange {
                number_format: Some(code),
                ..StyleChange::default()
            }),
            None => Ok(()),
        }
    })
}

/// Borders: the selection's borders drawn in the Line Color, or taken
/// away; with no `set` the menu of them.
fn borders(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::{BorderSet, StyleChange};
    const ID: &str = "viewer.grid.borders";
    let sets = [
        ("bottom", "Bottom Border", BorderSet::Bottom),
        ("top", "Top Border", BorderSet::Top),
        ("left", "Left Border", BorderSet::Left),
        ("right", "Right Border", BorderSet::Right),
        ("none", "No Border", BorderSet::None),
        ("all", "All Borders", BorderSet::All),
        ("outside", "Outside Borders", BorderSet::Outside),
        ("thick", "Thick Outside Borders", BorderSet::ThickOutside),
    ];
    let Some(name) = args.get("set").and_then(|x| x.as_str()) else {
        let mut items: Vec<_> = sets
            .iter()
            .map(|(k, title, _)| menu_item(ID, serde_json::json!({ "set": k }), title, "Borders"))
            .collect();
        items.push(menu_item(
            "viewer.grid.borderColor",
            serde_json::json!({}),
            "Line Color…",
            "Borders",
        ));
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let Some(set) = sets.iter().find(|s| s.0 == name).map(|s| s.2) else {
        ctx.messages.push(format!("No such borders: {name}"));
        return Ok(());
    };
    with(ctx, |v| {
        let color = v.border_color;
        v.change_style(StyleChange {
            borders: Some((set, color)),
            ..StyleChange::default()
        })
    })
}

/// Line Color: the color the next borders are drawn in.
fn border_color(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    const ID: &str = "viewer.grid.borderColor";
    let typed = args.get("value").and_then(|x| x.as_str());
    let color = match args.get("color").and_then(|c| c.as_str()).or(typed) {
        None => {
            let items = color_menu(ID, &serde_json::json!({}), "Line Color");
            ctx.requests.push(Request::Choose(items));
            return Ok(());
        }
        Some("custom") => return ask_more(ctx, ID, &serde_json::json!({}), "value"),
        Some("auto") => None,
        Some(c) => match hex_color(c) {
            Some(rgb) => Some(rgb),
            None => {
                ctx.messages.push(format!("Not a color: {c} (#RRGGBB)"));
                return Ok(());
            }
        },
    };
    with(ctx, |v| {
        v.border_color = color;
        Ok(())
    })
}

/// Horizontal or vertical alignment of the selection. A horizontal one
/// the cursor's cell already has goes back to General, as Excel's
/// buttons do.
fn align_style(
    ctx: &mut EditorContext<'_>,
    align: Option<kalem_viewer::Align>,
    valign: Option<kalem_viewer::VAlign>,
) -> CommandResult {
    use kalem_viewer::{Align, StyleChange};
    with(ctx, |v| {
        let align = align.map(|a| {
            if a != Align::General && v.cursor_cell().align == a {
                Align::General
            } else {
                a
            }
        });
        v.change_style(StyleChange {
            align,
            valign,
            ..StyleChange::default()
        })
    })
}

/// Font Color and Fill Color: Excel's standard colors, a typed #RRGGBB,
/// automatic, and for a fill none.
fn color_style(ctx: &mut EditorContext<'_>, args: &serde_json::Value, fill: bool) -> CommandResult {
    use kalem_viewer::StyleChange;
    let id = if fill {
        "viewer.grid.fillColor"
    } else {
        "viewer.grid.fontColor"
    };
    let typed = args.get("value").and_then(|x| x.as_str());
    let pick = |c: Option<[u8; 3]>| {
        if fill {
            StyleChange {
                fill: Some(c),
                ..StyleChange::default()
            }
        } else {
            StyleChange {
                color: Some(c),
                ..StyleChange::default()
            }
        }
    };
    match args.get("color").and_then(|c| c.as_str()).or(typed) {
        Some("auto" | "none") => with(ctx, |v| v.change_style(pick(None))),
        Some("custom") => ask_more(ctx, id, &serde_json::json!({}), "value"),
        Some(c) => match hex_color(c) {
            Some(rgb) => with(ctx, |v| v.change_style(pick(Some(rgb)))),
            None => {
                ctx.messages.push(format!("Not a color: {c} (#RRGGBB)"));
                Ok(())
            }
        },
        None => {
            let category = if fill { "Fill Color" } else { "Font Color" };
            let mut items = color_menu(id, &serde_json::json!({}), category);
            if fill {
                items.insert(
                    0,
                    menu_item(
                        id,
                        serde_json::json!({ "color": "none" }),
                        "No Fill",
                        category,
                    ),
                );
            }
            ctx.requests.push(Request::Choose(items));
            Ok(())
        }
    }
}

/// Font Size and Font: the common ones, or typed.
fn font_choice(ctx: &mut EditorContext<'_>, args: &serde_json::Value, size: bool) -> CommandResult {
    use kalem_viewer::StyleChange;
    let id = if size {
        "viewer.grid.fontSize"
    } else {
        "viewer.grid.fontFace"
    };
    let given = args.get("value").and_then(|x| {
        x.as_str()
            .map(str::to_string)
            .or_else(|| x.as_f64().map(|n| n.to_string()))
    });
    if let Some(v) = given.clone().filter(|v| v != "custom") {
        if size {
            let t = v.replace(',', ".");
            return match t.trim().trim_end_matches("pt").trim().parse::<f32>() {
                Ok(n) if (1.0..=409.0).contains(&n) => with(ctx, |vw| {
                    vw.change_style(StyleChange {
                        size: Some(n),
                        ..StyleChange::default()
                    })
                }),
                _ => {
                    ctx.messages
                        .push(format!("Not a font size: {v} (1 to 409 points)"));
                    Ok(())
                }
            };
        }
        let face = v.trim().to_string();
        if face.is_empty() {
            return Ok(());
        }
        return with(ctx, |vw| {
            vw.change_style(StyleChange {
                face: Some(face),
                ..StyleChange::default()
            })
        });
    }
    if given.is_some() {
        return ask_more(ctx, id, &serde_json::json!({}), "value");
    }
    let mut items: Vec<_> = if size {
        [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 36, 48, 72]
            .iter()
            .map(|n| {
                menu_item(
                    id,
                    serde_json::json!({ "value": n.to_string() }),
                    &format!("{n} pt"),
                    "Font Size",
                )
            })
            .collect()
    } else {
        [
            "Calibri",
            "Arial",
            "Times New Roman",
            "Cambria",
            "Courier New",
            "Georgia",
            "Verdana",
            "Tahoma",
            "Segoe UI",
            "Aptos",
        ]
        .iter()
        .map(|f| menu_item(id, serde_json::json!({ "value": f }), f, "Font"))
        .collect()
    };
    items.push(menu_item(
        id,
        serde_json::json!({ "value": "custom" }),
        "Custom…",
        if size { "Font Size" } else { "Font" },
    ));
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Insert Chart: the kinds offered, then the chart of the selection.
fn insert_chart(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::ChartKind;
    let kinds = [
        ("column", ChartKind::Column, "Column"),
        ("bar", ChartKind::Bar, "Bar"),
        ("line", ChartKind::Line, "Line"),
        ("area", ChartKind::Area, "Area"),
        ("pie", ChartKind::Pie, "Pie"),
        ("doughnut", ChartKind::Doughnut, "Doughnut"),
        ("scatter", ChartKind::Scatter, "Scatter"),
    ];
    let Some(kind) = args
        .get("kind")
        .and_then(|k| k.as_str())
        .and_then(|k| kinds.iter().find(|x| x.0 == k))
        .map(|x| x.1)
    else {
        let items = kinds
            .iter()
            .map(|(key, _, title)| {
                menu_item(
                    "viewer.grid.insertChart",
                    serde_json::json!({ "kind": key }),
                    &format!("{title} Chart"),
                    "Insert Chart",
                )
            })
            .collect();
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let title = text_arg(args, "title");
    with(ctx, |v| v.insert_chart(kind, title))
}

/// Insert PivotTable, a step at a time in the palette as Excel's field
/// list: a row field, a column field or none, a value field and how it is
/// summarized, then Create, another row field or another value field.
fn insert_pivot(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::{Aggregate, PivotSpec};
    const ID: &str = "viewer.grid.insertPivot";
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if !v.grid_editable() {
        ctx.messages.push("This file is shown, not edited".into());
        return Ok(());
    }
    let (range, names) = match args.get("range").and_then(|r| r.as_array()) {
        Some(r) if r.len() == 4 => {
            let r: Vec<u32> = r.iter().map(|x| x.as_u64().unwrap_or(0) as u32).collect();
            let (_, names) = v.pivot_source();
            ([r[0], r[1], r[2], r[3]], names)
        }
        _ => v.pivot_source(),
    };
    if range[0] == range[2] {
        ctx.messages
            .push("A pivot table needs a header row and rows under it".into());
        return Ok(());
    }
    let list = |key: &str| -> Vec<u32> {
        args.get(key)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default()
    };
    let rows = list("rows");
    let cols = list("cols");
    let aggs = ["sum", "count", "average", "max", "min"];
    let values: Vec<(u32, Aggregate)> = args
        .get("values")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let f = p.get(0)?.as_u64()? as u32;
                    let agg = match p.get(1)?.as_str()? {
                        "count" => Aggregate::Count,
                        "average" => Aggregate::Average,
                        "max" => Aggregate::Max,
                        "min" => Aggregate::Min,
                        _ => Aggregate::Sum,
                    };
                    Some((f, agg))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut base = args.clone();
    base["range"] = serde_json::json!(range);
    let arg = |key: &str, value: serde_json::Value| {
        let mut a = base.clone();
        a[key] = value;
        if key != "step" {
            a.as_object_mut().map(|o| o.remove("step"));
        }
        a
    };
    let name = |f: u32| {
        let n = names.get(f as usize).cloned().unwrap_or_default();
        if n.trim().is_empty() {
            crate::csv_tools::column_letters((range[1] + f) as usize)
        } else {
            n
        }
    };
    let fields: Vec<u32> = (0..names.len() as u32).collect();
    let step = args.get("step").and_then(|s| s.as_str()).unwrap_or("");
    let mut items = Vec::new();
    if rows.is_empty() || step == "row" {
        for &f in fields
            .iter()
            .filter(|f| !rows.contains(f) && !cols.contains(f))
        {
            let mut r = rows.clone();
            r.push(f);
            items.push(menu_item(
                ID,
                arg("rows", serde_json::json!(r)),
                &name(f),
                "Row Field",
            ));
        }
    } else if args.get("cols").is_none() {
        items.push(menu_item(
            ID,
            arg("cols", serde_json::json!([])),
            "(No Column Field)",
            "Column Field",
        ));
        for &f in fields.iter().filter(|f| !rows.contains(f)) {
            items.push(menu_item(
                ID,
                arg("cols", serde_json::json!([f])),
                &name(f),
                "Column Field",
            ));
        }
    } else if let Some(f) = args.get("value").and_then(|x| x.as_u64()) {
        for (agg, title) in aggs.iter().zip(["Sum", "Count", "Average", "Max", "Min"]) {
            let mut vals: Vec<serde_json::Value> = args
                .get("values")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default();
            vals.push(serde_json::json!([f, agg]));
            let mut a = arg("values", serde_json::json!(vals));
            a.as_object_mut().map(|o| o.remove("value"));
            items.push(menu_item(
                ID,
                a,
                &format!("{title} of {}", name(f as u32)),
                "Summarize Values By",
            ));
        }
    } else if values.is_empty() || step == "value" {
        for &f in fields
            .iter()
            .filter(|f| !rows.contains(f) && !cols.contains(f))
        {
            items.push(menu_item(
                ID,
                arg("value", serde_json::json!(f)),
                &name(f),
                "Value Field",
            ));
        }
    } else if step == "create" {
        let spec = PivotSpec {
            range,
            rows,
            cols,
            values,
        };
        return with(ctx, |v| v.insert_pivot(spec));
    } else {
        let what: Vec<String> = values
            .iter()
            .map(|(f, a)| {
                let t = aggs[*a as usize];
                format!("{t} of {}", name(*f))
            })
            .collect();
        let summary = format!(
            "Rows: {}{} · Values: {}",
            rows.iter()
                .map(|f| name(*f))
                .collect::<Vec<_>>()
                .join(" > "),
            cols.first()
                .map_or(String::new(), |f| format!(" · Columns: {}", name(*f))),
            what.join(", ")
        );
        items.push(menu_item(
            ID,
            arg("step", serde_json::json!("create")),
            "✓ Create PivotTable",
            &summary,
        ));
        if fields
            .iter()
            .any(|f| !rows.contains(f) && !cols.contains(f))
        {
            items.push(menu_item(
                ID,
                arg("step", serde_json::json!("row")),
                "Add a Row Field…",
                &summary,
            ));
            if cols.is_empty() {
                items.push(menu_item(
                    ID,
                    arg("step", serde_json::json!("value")),
                    "Add a Value Field…",
                    &summary,
                ));
            }
        }
    }
    if items.is_empty() {
        ctx.messages.push("No field is left to use".into());
        return Ok(());
    }
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// Checks an entry against its cell's validation, as Excel does on Enter:
/// a Stop alert says why and asks again, a Warning asks whether to keep
/// it, an Information alert keeps it and says so. Whether the entry waits.
fn refused(ctx: &mut EditorContext<'_>, row: u32, col: u32, value: &str) -> bool {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return false;
    };
    let Some(e) = v.check_input(row, col, value) else {
        return false;
    };
    let said = if e.title.is_empty() {
        e.message.clone()
    } else {
        format!("{}: {}", e.title, e.message)
    };
    let again = serde_json::json!({ "row": row, "col": col, "value_default": value });
    match e.style {
        ErrorStyle::Information => {
            ctx.messages.push(said);
            false
        }
        ErrorStyle::Stop => {
            ctx.messages.push(said);
            ctx.requests.push(Request::Ask {
                command: "viewer.grid.setCell".into(),
                args: again,
                arg: "value".into(),
            });
            true
        }
        ErrorStyle::Warning => {
            let question = format!("{said} Continue?");
            ctx.requests.push(Request::Choose(vec![
                menu_item(
                    "viewer.grid.setCell",
                    serde_json::json!({ "row": row, "col": col, "value": value, "force": true }),
                    "Yes, keep the value",
                    &question,
                ),
                menu_item("viewer.grid.editCellAgain", again, "No, edit it", &question),
                menu_item(
                    "viewer.grid.cancel",
                    serde_json::json!({}),
                    "Cancel",
                    &question,
                ),
            ]));
            true
        }
    }
}

/// A list validation's values in the palette, as Excel's drop-down.
fn pick_from_list(ctx: &mut EditorContext<'_>, _args: &serde_json::Value) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    let p = v.grid_pos();
    let list = v
        .cursor_validation()
        .filter(|x| x.kind == kalem_viewer::ValidationKind::List)
        .map(|x| x.list)
        .unwrap_or_default();
    if list.is_empty() {
        ctx.messages
            .push("This cell has no list to choose from".into());
        return Ok(());
    }
    let category = format!(
        "{}{}",
        crate::csv_tools::column_letters(p.col as usize),
        p.row + 1
    );
    let items = list
        .iter()
        .take(1000)
        .map(|item| {
            menu_item(
                "viewer.grid.setCell",
                serde_json::json!({ "row": p.row, "col": p.col, "value": item, "force": true }),
                item,
                &category,
            )
        })
        .collect();
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// The Data Validation menu, as Excel's dialog: what the selection allows,
/// its message and alert, and the circles.
fn validation_menu(ctx: &mut EditorContext<'_>, _args: &serde_json::Value) -> CommandResult {
    let c = "Data Validation";
    let none = serde_json::json!({});
    let number = |kind: &str| serde_json::json!({ "kind": kind });
    let circles = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
        .is_some_and(|v| v.circle_invalid);
    let items = vec![
        menu_item("viewer.grid.validateList", none.clone(), "Allow a List…", c),
        menu_item(
            "viewer.grid.validateNumber",
            number("whole"),
            "Allow Whole Numbers…",
            c,
        ),
        menu_item(
            "viewer.grid.validateNumber",
            number("decimal"),
            "Allow Decimals…",
            c,
        ),
        menu_item(
            "viewer.grid.validateNumber",
            number("date"),
            "Allow Dates…",
            c,
        ),
        menu_item(
            "viewer.grid.validateNumber",
            number("time"),
            "Allow Times…",
            c,
        ),
        menu_item(
            "viewer.grid.validateNumber",
            number("textLength"),
            "Allow a Text Length…",
            c,
        ),
        menu_item(
            "viewer.grid.validateFormula",
            none.clone(),
            "Allow by a Formula…",
            c,
        ),
        menu_item(
            "viewer.grid.validationMessage",
            none.clone(),
            "Input Message…",
            c,
        ),
        menu_item(
            "viewer.grid.validationAlert",
            serde_json::json!({ "style": "stop" }),
            "Error Alert: Stop…",
            c,
        ),
        menu_item(
            "viewer.grid.validationAlert",
            serde_json::json!({ "style": "warning" }),
            "Error Alert: Warning…",
            c,
        ),
        menu_item(
            "viewer.grid.validationAlert",
            serde_json::json!({ "style": "information" }),
            "Error Alert: Information…",
            c,
        ),
        menu_item(
            "viewer.grid.validationAlert",
            serde_json::json!({ "style": "none" }),
            "No Error Alert",
            c,
        ),
        menu_item(
            "viewer.grid.circleInvalid",
            none.clone(),
            if circles {
                "Clear Validation Circles"
            } else {
                "Circle Invalid Data"
            },
            c,
        ),
        menu_item("viewer.grid.clearValidation", none, "Clear Validation", c),
    ];
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// The selection's new validation: the cursor's as it is, with `f`'s
/// change, so its message and alert stay.
fn revalidate(ctx: &mut EditorContext<'_>, f: impl FnOnce(&mut Validation)) -> CommandResult {
    with(ctx, |v| {
        let mut x = v.cursor_validation().unwrap_or_default();
        f(&mut x);
        v.set_validation(Some(x))
    })
}

/// Asks for the argument `arg` of `id`, the rest of `args` kept.
fn ask_more(
    ctx: &mut EditorContext<'_>,
    id: &str,
    args: &serde_json::Value,
    arg: &str,
) -> CommandResult {
    ctx.requests.push(Request::Ask {
        command: id.into(),
        args: args.clone(),
        arg: arg.into(),
    });
    Ok(())
}

fn text_arg(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .map(str::to_string)
}

/// Whole numbers, decimals, dates, times or text lengths: the comparison
/// chosen, then its values asked.
fn validate_number(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    use kalem_viewer::{CompareOp, ValidationKind};
    const ID: &str = "viewer.grid.validateNumber";
    let kind = match args.get("kind").and_then(|k| k.as_str()) {
        Some("decimal") => ValidationKind::Decimal,
        Some("date") => ValidationKind::Date,
        Some("time") => ValidationKind::Time,
        Some("textLength") => ValidationKind::TextLength,
        _ => ValidationKind::Whole,
    };
    let ops = [
        ("between", CompareOp::Between, "Between"),
        ("notBetween", CompareOp::NotBetween, "Not Between"),
        ("equal", CompareOp::Equal, "Equal To"),
        ("notEqual", CompareOp::NotEqual, "Not Equal To"),
        ("greaterThan", CompareOp::Greater, "Greater Than"),
        ("lessThan", CompareOp::Less, "Less Than"),
        (
            "greaterThanOrEqual",
            CompareOp::GreaterOrEqual,
            "Greater Than or Equal To",
        ),
        (
            "lessThanOrEqual",
            CompareOp::LessOrEqual,
            "Less Than or Equal To",
        ),
    ];
    let Some(op) = args
        .get("op")
        .and_then(|o| o.as_str())
        .and_then(|o| ops.iter().find(|x| x.0 == o))
        .map(|x| x.1)
    else {
        let items = ops
            .iter()
            .map(|(key, _, title)| {
                let mut a = args.clone();
                a["op"] = serde_json::json!(key);
                menu_item(ID, a, &format!("{title}…"), "Data Validation")
            })
            .collect();
        ctx.requests.push(Request::Choose(items));
        return Ok(());
    };
    let Some(value) = text_arg(args, "value") else {
        return ask_more(ctx, ID, args, "value");
    };
    let two = matches!(op, CompareOp::Between | CompareOp::NotBetween);
    let value2 = text_arg(args, "and");
    if two && value2.is_none() {
        return ask_more(ctx, ID, args, "and");
    }
    revalidate(ctx, |x| {
        x.kind = kind;
        x.op = op;
        x.value = value;
        x.value2 = if two { value2 } else { None };
    })
}

/// A highlighting style by name, as Excel's presets: light red fill with
/// dark red text unless asked otherwise.
fn cond_style(args: &serde_json::Value) -> kalem_viewer::CondStyle {
    let (fill, color, bold) = match args.get("style").and_then(|s| s.as_str()) {
        Some("yellow") => (Some([0xFF, 0xEB, 0x9C]), Some([0x9C, 0x57, 0x00]), false),
        Some("green") => (Some([0xC6, 0xEF, 0xCE]), Some([0x00, 0x61, 0x00]), false),
        Some("red text") => (None, Some([0x9C, 0x00, 0x06]), false),
        Some("bold") => (None, None, true),
        _ => (Some([0xFF, 0xC7, 0xCE]), Some([0x9C, 0x00, 0x06]), false),
    };
    kalem_viewer::CondStyle { fill, color, bold }
}

/// A `#RRGGBB` or `RRGGBB` color.
fn hex_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// A palette entry running `id` with `args`.
fn menu_item(
    id: &str,
    args: serde_json::Value,
    title: &str,
    category: &str,
) -> crate::palette::PaletteItem {
    crate::palette::PaletteItem {
        id: crate::palette::invocation(id, &args),
        title: title.into(),
        category: category.into(),
        keys: String::new(),
        also: title.into(),
    }
}

/// The Conditional Formatting menu, as Excel's: its rules for the selection.
fn conditional_menu(ctx: &mut EditorContext<'_>, _args: &serde_json::Value) -> CommandResult {
    let none = serde_json::json!({});
    let c = "Conditional Formatting";
    let items = vec![
        menu_item(
            "viewer.grid.highlightGreater",
            none.clone(),
            "Greater Than…",
            c,
        ),
        menu_item("viewer.grid.highlightLess", none.clone(), "Less Than…", c),
        menu_item("viewer.grid.highlightBetween", none.clone(), "Between…", c),
        menu_item("viewer.grid.highlightEqual", none.clone(), "Equal To…", c),
        menu_item(
            "viewer.grid.highlightText",
            none.clone(),
            "Text That Contains…",
            c,
        ),
        menu_item(
            "viewer.grid.highlightDuplicates",
            none.clone(),
            "Duplicate Values",
            c,
        ),
        menu_item(
            "viewer.grid.highlightUnique",
            none.clone(),
            "Unique Values",
            c,
        ),
        menu_item(
            "viewer.grid.highlightTop",
            serde_json::json!({ "value": "10" }),
            "Top 10 Items",
            c,
        ),
        menu_item(
            "viewer.grid.highlightTop",
            serde_json::json!({ "value": "10", "percent": true }),
            "Top 10%",
            c,
        ),
        menu_item(
            "viewer.grid.highlightBottom",
            serde_json::json!({ "value": "10" }),
            "Bottom 10 Items",
            c,
        ),
        menu_item(
            "viewer.grid.highlightBottom",
            serde_json::json!({ "value": "10", "percent": true }),
            "Bottom 10%",
            c,
        ),
        menu_item(
            "viewer.grid.highlightAboveAverage",
            none.clone(),
            "Above Average",
            c,
        ),
        menu_item(
            "viewer.grid.highlightBelowAverage",
            none.clone(),
            "Below Average",
            c,
        ),
        menu_item(
            "viewer.grid.highlightFormula",
            none.clone(),
            "Use a Formula…",
            c,
        ),
        menu_item("viewer.grid.dataBars", none.clone(), "Data Bars", c),
        menu_item("viewer.grid.colorScale", none.clone(), "Color Scales", c),
        menu_item("viewer.grid.iconSet", none.clone(), "Icon Sets", c),
        menu_item(
            "viewer.grid.clearConditionalFormats",
            none.clone(),
            "Clear Rules from Selection",
            c,
        ),
        menu_item(
            "viewer.grid.clearSheetConditionalFormats",
            none,
            "Clear Rules from Sheet",
            c,
        ),
    ];
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// A rule taking values: asked for each one missing, then added.
fn highlight(ctx: &mut EditorContext<'_>, args: &serde_json::Value, id: &str) -> CommandResult {
    use kalem_viewer::{CompareOp, CondRule};
    let text = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let ask = |ctx: &mut EditorContext<'_>, arg: &str| {
        ctx.requests.push(Request::Ask {
            command: id.into(),
            args: args.clone(),
            arg: arg.into(),
        });
        Ok(())
    };
    let needs_value = !matches!(
        id,
        "viewer.grid.highlightDuplicates"
            | "viewer.grid.highlightUnique"
            | "viewer.grid.highlightAboveAverage"
            | "viewer.grid.highlightBelowAverage"
    );
    let Some(value) = text("value")
        .filter(|v| !v.trim().is_empty())
        .or((!needs_value).then(String::new))
    else {
        return ask(ctx, "value");
    };
    let compare = |op| CondRule::Compare {
        op,
        value: value.clone(),
        value2: None,
    };
    let rule = match id {
        "viewer.grid.highlightGreater" => compare(CompareOp::Greater),
        "viewer.grid.highlightLess" => compare(CompareOp::Less),
        "viewer.grid.highlightEqual" => compare(CompareOp::Equal),
        "viewer.grid.highlightBetween" => {
            let Some(and) = text("and").filter(|v| !v.trim().is_empty()) else {
                return ask(ctx, "and");
            };
            CondRule::Compare {
                op: CompareOp::Between,
                value: value.clone(),
                value2: Some(and),
            }
        }
        "viewer.grid.highlightText" => CondRule::TextContains(value.clone()),
        "viewer.grid.highlightDuplicates" => CondRule::Duplicates,
        "viewer.grid.highlightUnique" => CondRule::Unique,
        "viewer.grid.highlightTop" | "viewer.grid.highlightBottom" => {
            let Ok(count) = value.trim().trim_end_matches('%').parse::<u32>() else {
                ctx.messages.push(format!("Not a count: {value}"));
                return Ok(());
            };
            CondRule::Top {
                count: count.max(1),
                bottom: id == "viewer.grid.highlightBottom",
                percent: args.get("percent").and_then(serde_json::Value::as_bool) == Some(true)
                    || value.trim().ends_with('%'),
            }
        }
        "viewer.grid.highlightAboveAverage" => CondRule::Average { below: false },
        "viewer.grid.highlightBelowAverage" => CondRule::Average { below: true },
        _ => CondRule::Formula(value.clone()),
    };
    let style = cond_style(args);
    with(ctx, |v| v.add_conditional_format(rule, style))
}

/// Data bars, color scales and icon sets: their kinds offered when none
/// is named.
fn graded(ctx: &mut EditorContext<'_>, args: &serde_json::Value, id: &str) -> CommandResult {
    use kalem_viewer::CondRule;
    let rule = match id {
        "viewer.grid.dataBars" => args
            .get("color")
            .and_then(|c| c.as_str())
            .and_then(hex_color)
            .map(CondRule::DataBar),
        "viewer.grid.colorScale" => args.get("colors").and_then(|c| c.as_array()).and_then(|a| {
            let colors: Option<Vec<[u8; 3]>> =
                a.iter().map(|c| c.as_str().and_then(hex_color)).collect();
            colors.filter(|c| c.len() >= 2).map(CondRule::ColorScale)
        }),
        _ => args
            .get("name")
            .and_then(|n| n.as_str())
            .map(|n| CondRule::IconSet(n.to_string())),
    };
    if let Some(rule) = rule {
        return with(ctx, |v| {
            v.add_conditional_format(rule, kalem_viewer::CondStyle::default())
        });
    }
    let items = match id {
        "viewer.grid.dataBars" => [
            ("Blue Data Bar", "#638EC6"),
            ("Green Data Bar", "#63C384"),
            ("Red Data Bar", "#FF555A"),
            ("Orange Data Bar", "#FFB628"),
            ("Light Blue Data Bar", "#008AEF"),
            ("Purple Data Bar", "#D6007B"),
        ]
        .iter()
        .map(|(t, c)| menu_item(id, serde_json::json!({ "color": c }), t, "Data Bars"))
        .collect(),
        "viewer.grid.colorScale" => [
            (
                "Green - Yellow - Red",
                &["#63BE7B", "#FFEB84", "#F8696B"][..],
            ),
            (
                "Red - Yellow - Green",
                &["#F8696B", "#FFEB84", "#63BE7B"][..],
            ),
            (
                "Green - White - Red",
                &["#63BE7B", "#FCFCFF", "#F8696B"][..],
            ),
            (
                "Red - White - Green",
                &["#F8696B", "#FCFCFF", "#63BE7B"][..],
            ),
            ("Blue - White - Red", &["#5A8AC6", "#FCFCFF", "#F8696B"][..]),
            ("White - Red", &["#FCFCFF", "#F8696B"][..]),
            ("White - Green", &["#FCFCFF", "#63BE7B"][..]),
            ("Green - Yellow", &["#63BE7B", "#FFEF9C"][..]),
        ]
        .iter()
        .map(|(t, c)| menu_item(id, serde_json::json!({ "colors": c }), t, "Color Scales"))
        .collect(),
        _ => [
            ("▼ ► ▲ 3 Arrows", "3Arrows"),
            ("● ● ● 3 Traffic Lights", "3TrafficLights1"),
            ("✖ ! ✔ 3 Symbols", "3Symbols"),
            ("⚑ ⚑ ⚑ 3 Flags", "3Flags"),
            ("☆ ⯪ ★ 3 Stars", "3Stars"),
            ("▼ ↘ ↗ ▲ 4 Arrows", "4Arrows"),
            ("▁ ▃ ▅ ▇ 4 Ratings", "4Rating"),
            ("▼ ↘ ► ↗ ▲ 5 Arrows", "5Arrows"),
            ("○ ◔ ◑ ◕ ● 5 Quarters", "5Quarters"),
        ]
        .iter()
        .map(|(t, n)| menu_item(id, serde_json::json!({ "name": n }), t, "Icon Sets"))
        .collect(),
    };
    ctx.requests.push(Request::Choose(items));
    Ok(())
}

/// A row three points taller or shorter: the terminal's drag.
fn grid_height(ctx: &mut EditorContext<'_>, by: f32) -> CommandResult {
    with(ctx, |v| {
        let row = v.grid_pos().row;
        let h = (v.row_height(row) + by).max(3.0);
        v.set_row_height(row, h)
    })
}

/// A column a digit wider or narrower: the terminal's drag.
fn grid_width(ctx: &mut EditorContext<'_>, by: f32) -> CommandResult {
    with(ctx, |v| {
        let col = v.grid_pos().col;
        let w = (v.col_width(col) + by).max(1.0);
        v.set_col_width(col, w.round())
    })
}

/// Cells (sorted by row, then column) as few ranges: runs along each row,
/// the same runs on rows after one another joined.
pub fn areas_of(cells: &[(u32, u32)]) -> Vec<[u32; 4]> {
    let mut runs: Vec<[u32; 4]> = Vec::new();
    for &(r, c) in cells {
        match runs.last_mut() {
            Some(run) if run[0] == r && run[3] + 1 == c => run[3] = c,
            _ => runs.push([r, c, r, c]),
        }
    }
    let mut out: Vec<[u32; 4]> = Vec::new();
    for run in runs {
        // The same columns on the row right above: one taller range.
        if let Some(a) = out
            .iter_mut()
            .find(|a| a[2] + 1 == run[0] && a[1] == run[1] && a[3] == run[3])
        {
            a[2] = run[0];
        } else {
            out.push(run);
        }
    }
    out
}

/// Whether a prompt's answer is a cell's entry: where Ctrl+Enter enters
/// it into every selected cell and AutoComplete offers the column's text.
pub fn cell_entry_prompt(command: &str, arg: &str) -> bool {
    command == "viewer.grid.setCell" && arg == "value"
}

/// Whether a prompt's answer may hold line breaks (Alt+Enter): a cell's
/// entry, a note.
pub fn multiline_prompt(command: &str, arg: &str) -> bool {
    cell_entry_prompt(command, arg) || (command == "viewer.grid.editNote" && arg == "value")
}

/// The commands of grid units (a workbook's sheets).
fn grid_commands() -> Vec<Command> {
    let all = vec![
        cmd(
            "viewer.grid.up",
            "Cell Up",
            &["up", "k"],
            IN_GRID,
            |ctx, _| grid_move(ctx, -1, 0),
        ),
        cmd(
            "viewer.grid.down",
            "Cell Down",
            &["down", "j"],
            IN_GRID,
            |ctx, _| grid_move(ctx, 1, 0),
        ),
        cmd(
            "viewer.grid.left",
            "Cell Left",
            &["left", "h", "shift+tab"],
            IN_GRID,
            |ctx, _| grid_move(ctx, 0, -1),
        ),
        cmd(
            "viewer.grid.right",
            "Cell Right",
            &["right", "l", "tab"],
            IN_GRID,
            |ctx, _| grid_move(ctx, 0, 1),
        ),
        cmd(
            "viewer.grid.dataUp",
            "Data Edge Up",
            &["ctrl+up"],
            IN_GRID,
            |ctx, _| data_move(ctx, -1, 0, false),
        ),
        cmd(
            "viewer.grid.dataDown",
            "Data Edge Down",
            &["ctrl+down"],
            IN_GRID,
            |ctx, _| data_move(ctx, 1, 0, false),
        ),
        cmd(
            "viewer.grid.dataLeft",
            "Data Edge Left",
            &["ctrl+left"],
            IN_GRID,
            |ctx, _| data_move(ctx, 0, -1, false),
        ),
        cmd(
            "viewer.grid.dataRight",
            "Data Edge Right",
            &["ctrl+right"],
            IN_GRID,
            |ctx, _| data_move(ctx, 0, 1, false),
        ),
        cmd(
            "viewer.grid.selectDataUp",
            "Select to Data Edge Up",
            &["ctrl+shift+up"],
            IN_GRID,
            |ctx, _| data_move(ctx, -1, 0, true),
        ),
        cmd(
            "viewer.grid.selectDataDown",
            "Select to Data Edge Down",
            &["ctrl+shift+down"],
            IN_GRID,
            |ctx, _| data_move(ctx, 1, 0, true),
        ),
        cmd(
            "viewer.grid.selectDataLeft",
            "Select to Data Edge Left",
            &["ctrl+shift+left"],
            IN_GRID,
            |ctx, _| data_move(ctx, 0, -1, true),
        ),
        cmd(
            "viewer.grid.selectDataRight",
            "Select to Data Edge Right",
            &["ctrl+shift+right"],
            IN_GRID,
            |ctx, _| data_move(ctx, 0, 1, true),
        ),
        cmd(
            "viewer.grid.selectRow",
            "Select Row",
            &["shift+space", "g r"],
            IN_GRID,
            |ctx, _| select_lines(ctx, true),
        ),
        cmd(
            "viewer.grid.selectColumn",
            "Select Column",
            &["ctrl+space", "g c"],
            IN_GRID,
            |ctx, _| select_lines(ctx, false),
        ),
        cmd(
            "viewer.grid.selectAll",
            "Select All",
            &["ctrl+a"],
            IN_GRID,
            |ctx, _| select_all(ctx),
        ),
        cmd("viewer.grid.goTo", "Go To", &["f5", "g o"], IN_GRID, go_to),
        cmd(
            "viewer.grid.insertPicture",
            "Insert Picture",
            &["o p"],
            IN_GRID,
            insert_picture,
        ),
        cmd(
            "viewer.grid.insertShape",
            "Insert Shape",
            &["o s"],
            IN_GRID,
            insert_shape,
        ),
        cmd(
            "viewer.grid.editShapeText",
            "Edit Shape Text",
            &["o t"],
            IN_GRID,
            edit_shape_text,
        ),
        cmd(
            "viewer.grid.deleteDrawing",
            "Delete Picture or Shape",
            &["o d"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.delete_drawing()),
        ),
        cmd(
            "viewer.grid.moveDrawingUp",
            "Move Picture Up",
            &["o k"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(-1, 0, false)),
        ),
        cmd(
            "viewer.grid.moveDrawingDown",
            "Move Picture Down",
            &["o j"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(1, 0, false)),
        ),
        cmd(
            "viewer.grid.moveDrawingLeft",
            "Move Picture Left",
            &["o h"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(0, -1, false)),
        ),
        cmd(
            "viewer.grid.moveDrawingRight",
            "Move Picture Right",
            &["o l"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(0, 1, false)),
        ),
        cmd(
            "viewer.grid.drawingTaller",
            "Picture Taller",
            &["o shift+j"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(1, 0, true)),
        ),
        cmd(
            "viewer.grid.drawingShorter",
            "Picture Shorter",
            &["o shift+k"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(-1, 0, true)),
        ),
        cmd(
            "viewer.grid.drawingWider",
            "Picture Wider",
            &["o shift+l"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(0, 1, true)),
        ),
        cmd(
            "viewer.grid.drawingNarrower",
            "Picture Narrower",
            &["o shift+h"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_drawing(0, -1, true)),
        ),
        cmd(
            "viewer.grid.insertSparklines",
            "Insert Sparklines",
            &["p i"],
            IN_GRID,
            insert_sparklines,
        ),
        cmd(
            "viewer.grid.clearSparklines",
            "Clear Sparklines",
            &["p shift+i"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.clear_sparklines()),
        ),
        cmd(
            "viewer.grid.goalSeek",
            "Goal Seek",
            &["z g"],
            IN_GRID,
            goal_seek,
        ),
        cmd(
            "viewer.grid.dataTable",
            "Data Table",
            &["z d"],
            IN_GRID,
            data_table,
        ),
        cmd(
            "viewer.grid.scenarios",
            "Scenario Manager",
            &["z m"],
            IN_GRID,
            scenarios,
        ),
        cmd(
            "viewer.grid.newComment",
            "New Comment",
            &["c m"],
            IN_GRID,
            new_comment,
        ),
        cmd(
            "viewer.grid.comments",
            "Comments",
            &["c t"],
            IN_GRID,
            comments_menu,
        ),
        cmd(
            "viewer.grid.tabColor",
            "Tab Color",
            &["shift+s c"],
            IN_GRID,
            tab_color,
        ),
        cmd(
            "viewer.grid.sheetList",
            "Sheet List",
            &["shift+s s"],
            IN_GRID,
            sheet_list,
        ),
        cmd(
            "viewer.grid.zoomIn",
            "Zoom In",
            &["z ="],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.update_view(|s| s.zoom = (s.zoom / 10 + 1) * 10)),
        ),
        cmd(
            "viewer.grid.zoomOut",
            "Zoom Out",
            &["z -"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.update_view(|s| s.zoom = (s.zoom.div_ceil(10) - 1) * 10)
                })
            },
        ),
        cmd(
            "viewer.grid.zoom100",
            "Zoom to 100%",
            &["z 0"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.update_view(|s| s.zoom = 100)),
        ),
        cmd("viewer.grid.zoom", "Zoom", &[], IN_GRID, zoom_to),
        cmd(
            "viewer.grid.toggleGridlines",
            "Gridlines",
            &["z l"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.update_view(|s| s.gridlines = !s.gridlines)),
        ),
        cmd(
            "viewer.grid.toggleHeadings",
            "Headings",
            &["z shift+h"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.update_view(|s| s.headings = !s.headings)),
        ),
        cmd(
            "viewer.grid.pageBreakPreview",
            "Page Break Preview",
            &["z b"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.update_view(|s| s.page_break_preview = !s.page_break_preview)
                })
            },
        ),
        cmd(
            "viewer.grid.split",
            "Split",
            &["z shift+s"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.toggle_split()),
        ),
        cmd(
            "viewer.grid.splitScrollUp",
            "Scroll Top Pane Up",
            &["z ["],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.scroll_split(-1, false)),
        ),
        cmd(
            "viewer.grid.splitScrollDown",
            "Scroll Top Pane Down",
            &["z ]"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.scroll_split(1, false)),
        ),
        cmd(
            "viewer.grid.contextMenu",
            "Context Menu",
            &["shift+f10", "menu"],
            IN_GRID,
            context_menu,
        ),
        cmd(
            "viewer.grid.goToSpecial",
            "Go To Special",
            &["g s"],
            IN_GRID,
            go_to_special,
        ),
        cmd(
            "viewer.grid.selectVisible",
            "Select Visible Cells",
            &["alt+;"],
            IN_GRID,
            |ctx, _| go_to_special(ctx, &serde_json::json!({ "kind": "visible" })),
        ),
        cmd(
            "viewer.grid.tracePrecedents",
            "Trace Precedents",
            &["z ,"],
            IN_GRID,
            |ctx, _| {
                let Some(v) = ctx
                    .document
                    .as_deref_mut()
                    .and_then(|d| d.viewer.as_deref_mut())
                else {
                    return Ok(());
                };
                let n = v.trace_precedents();
                ctx.messages.push(match n {
                    0 => "The formula reads no cells of this sheet".into(),
                    n => format!("{n} precedent{}", if n == 1 { "" } else { "s" }),
                });
                Ok(())
            },
        ),
        cmd(
            "viewer.grid.traceDependents",
            "Trace Dependents",
            &["z ."],
            IN_GRID,
            |ctx, _| {
                let Some(v) = ctx
                    .document
                    .as_deref_mut()
                    .and_then(|d| d.viewer.as_deref_mut())
                else {
                    return Ok(());
                };
                let n = v.trace_dependents();
                ctx.messages.push(match n {
                    0 => "No formula of this sheet reads the cell".into(),
                    n => format!("{n} dependent{}", if n == 1 { "" } else { "s" }),
                });
                Ok(())
            },
        ),
        cmd(
            "viewer.grid.removeArrows",
            "Remove Arrows",
            &["z x"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.arrows.clear();
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.evaluateFormula",
            "Evaluate Formula",
            &["z e"],
            IN_GRID,
            |ctx, _| evaluate_formula(ctx),
        ),
        cmd(
            "viewer.grid.errorChecking",
            "Error Checking",
            &["z n"],
            IN_GRID,
            |ctx, _| {
                let Some(v) = ctx
                    .document
                    .as_deref_mut()
                    .and_then(|d| d.viewer.as_deref_mut())
                else {
                    return Ok(());
                };
                let m = v
                    .next_error()
                    .unwrap_or_else(|| "No errors on this sheet".into());
                ctx.messages.push(m);
                Ok(())
            },
        ),
        cmd(
            "viewer.grid.watchWindow",
            "Watch Window",
            &["z w"],
            IN_GRID,
            watch_window,
        ),
        cmd(
            "viewer.grid.lockCells",
            "Lock Cell",
            &["t shift+l"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    let locked = !v.cursor_cell().unlocked;
                    v.change_style(kalem_viewer::StyleChange {
                        locked: Some(!locked),
                        ..kalem_viewer::StyleChange::default()
                    })
                })
            },
        ),
        cmd(
            "viewer.grid.protectSheet",
            "Protect Sheet",
            &["z k"],
            IN_GRID,
            protect_sheet,
        ),
        cmd(
            "viewer.grid.protectWorkbook",
            "Protect Workbook",
            &["z shift+k"],
            IN_GRID,
            protect_workbook,
        ),
        cmd(
            "viewer.grid.cellStyle",
            "Cell Styles",
            &["t y"],
            IN_GRID,
            cell_style,
        ),
        cmd(
            "viewer.grid.increaseIndent",
            "Increase Indent",
            &["ctrl+alt+tab", "t ]"],
            IN_GRID,
            |ctx, _| step_indent(ctx, 1),
        ),
        cmd(
            "viewer.grid.decreaseIndent",
            "Decrease Indent",
            &["ctrl+alt+shift+tab", "t ["],
            IN_GRID,
            |ctx, _| step_indent(ctx, -1),
        ),
        cmd(
            "viewer.grid.textRotation",
            "Orientation",
            &["t o"],
            IN_GRID,
            text_rotation,
        ),
        cmd(
            "viewer.grid.shrinkToFit",
            "Shrink to Fit",
            &["t k"],
            IN_GRID,
            |ctx, _| toggle_alignment(ctx, "shrink"),
        ),
        cmd(
            "viewer.grid.centerAcrossSelection",
            "Center Across Selection",
            &["t a"],
            IN_GRID,
            |ctx, _| toggle_alignment(ctx, "across"),
        ),
        cmd(
            "viewer.grid.group",
            "Group",
            &["alt+shift+right"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.group(true)),
        ),
        cmd(
            "viewer.grid.ungroup",
            "Ungroup",
            &["alt+shift+left"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.group(false)),
        ),
        cmd(
            "viewer.grid.hideDetail",
            "Hide Detail",
            &["z h"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.show_detail(false)),
        ),
        cmd(
            "viewer.grid.showDetail",
            "Show Detail",
            &["z s"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.show_detail(true)),
        ),
        cmd("viewer.grid.subtotal", "Subtotal", &[], IN_GRID, subtotal),
        cmd(
            "viewer.grid.pageSetup",
            "Page Setup",
            &["z p"],
            IN_GRID,
            page_setup,
        ),
        cmd(
            "viewer.grid.printPreview",
            "Print Preview",
            &["ctrl+f2"],
            IN_GRID,
            |ctx, args| sheet_pdf(ctx, args, "preview"),
        ),
        cmd(
            "viewer.grid.exportPdf",
            "Export to PDF",
            &[],
            IN_GRID,
            |ctx, args| sheet_pdf(ctx, args, "export"),
        ),
        cmd("viewer.grid.print", "Print", &[], IN_GRID, |ctx, args| {
            sheet_pdf(ctx, args, "print")
        }),
        cmd(
            "viewer.grid.formatAsTable",
            "Format as Table",
            &["ctrl+t", "s t"],
            IN_GRID,
            format_as_table,
        ),
        cmd(
            "viewer.grid.totalRow",
            "Total Row",
            &["ctrl+shift+t", "s shift+t"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.toggle_total_row()),
        ),
        cmd(
            "viewer.grid.convertToRange",
            "Convert to Range",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.convert_to_range()),
        ),
        cmd(
            "viewer.grid.customSort",
            "Custom Sort",
            &["s c"],
            IN_GRID,
            custom_sort,
        ),
        cmd(
            "viewer.grid.filterCondition",
            "Filter by Condition",
            &["s f"],
            IN_GRID,
            filter_condition,
        ),
        cmd(
            "viewer.grid.filterByColor",
            "Filter by Selected Cell's Color",
            &[],
            IN_GRID,
            |ctx, _| filter_by_color(ctx),
        ),
        cmd(
            "viewer.grid.reapplyFilter",
            "Reapply Filter",
            &["ctrl+alt+l", "s r"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.reapply_filter()),
        ),
        cmd(
            "viewer.grid.insertDate",
            "Insert Today's Date",
            &["ctrl+;", "g ;"],
            IN_GRID,
            |ctx, _| insert_now(ctx, false),
        ),
        cmd(
            "viewer.grid.insertTime",
            "Insert the Time",
            &["ctrl+shift+;", "g ,"],
            IN_GRID,
            |ctx, _| insert_now(ctx, true),
        ),
        cmd(
            "viewer.grid.formulaFromAbove",
            "Copy Formula from Above",
            &["ctrl+'", "g '"],
            IN_GRID,
            |ctx, _| from_above(ctx, false),
        ),
        cmd(
            "viewer.grid.valueFromAbove",
            "Copy Value from Above",
            &["ctrl+shift+'", "g v"],
            IN_GRID,
            |ctx, _| from_above(ctx, true),
        ),
        cmd(
            "viewer.grid.saveSheetAsCsv",
            "Save Sheet as CSV",
            &[],
            IN_GRID,
            save_sheet_csv,
        ),
        cmd(
            "viewer.grid.showFormulas",
            "Show Formulas",
            &["ctrl+`", "g f"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.show_formulas = !v.show_formulas;
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.calculateNow",
            "Calculate Now",
            &["f9"],
            IN_GRID,
            |ctx, _| calculate_now(ctx),
        ),
        cmd(
            "viewer.grid.defineName",
            "Define Name",
            &[],
            IN_GRID,
            define_name,
        ),
        cmd(
            "viewer.grid.nameManager",
            "Name Manager",
            &["ctrl+f3"],
            IN_GRID,
            |ctx, args| names_menu(ctx, args, false),
        ),
        cmd(
            "viewer.grid.deleteName",
            "Delete Name",
            &[],
            IN_GRID,
            |ctx, args| names_menu(ctx, args, true),
        ),
        cmd(
            "viewer.grid.insertLink",
            "Insert Link",
            &["ctrl+k"],
            IN_GRID,
            insert_link,
        ),
        cmd(
            "viewer.grid.openLink",
            "Open Link",
            &["g x"],
            IN_GRID,
            |ctx, _| open_link(ctx),
        ),
        cmd(
            "viewer.grid.removeLink",
            "Remove Link",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_link(None)),
        ),
        cmd(
            "viewer.grid.removeDuplicates",
            "Remove Duplicates",
            &[],
            IN_GRID,
            remove_duplicates,
        ),
        cmd(
            "viewer.grid.textToColumns",
            "Text to Columns",
            &[],
            IN_GRID,
            text_to_columns,
        ),
        cmd(
            "viewer.grid.clearFormats",
            "Clear Formats",
            &["t x"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.clear_formats(false)),
        ),
        cmd(
            "viewer.grid.clearAll",
            "Clear All",
            &["t shift+x"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.clear_formats(true)),
        ),
        cmd(
            "viewer.grid.formatPainter",
            "Format Painter",
            &["t p"],
            IN_GRID,
            |ctx, _| format_painter(ctx),
        ),
        cmd(
            "viewer.grid.insertCells",
            "Insert Cells",
            &["ctrl+shift+=", "g i"],
            IN_GRID,
            |ctx, args| cells_command(ctx, args, true),
        ),
        cmd(
            "viewer.grid.deleteCells",
            "Delete Cells",
            &["ctrl+-", "g d"],
            IN_GRID,
            |ctx, args| cells_command(ctx, args, false),
        ),
        cmd(
            "viewer.grid.pasteSpecial",
            "Paste Special",
            &["ctrl+alt+v"],
            IN_GRID,
            paste_special,
        ),
        cmd(
            "viewer.grid.autoSum",
            "AutoSum",
            &["alt+="],
            IN_GRID,
            |ctx, _| auto_sum(ctx),
        ),
        cmd(
            "viewer.grid.insertFunction",
            "Insert Function",
            &["shift+f3"],
            IN_GRID,
            insert_function,
        ),
        cmd(
            "viewer.grid.editNote",
            "Edit Note",
            &["shift+f2"],
            IN_GRID,
            edit_note,
        ),
        cmd(
            "viewer.grid.deleteNote",
            "Delete Note",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_note(None)),
        ),
        cmd(
            "viewer.grid.freezePanes",
            "Freeze Panes",
            &["z f"],
            IN_GRID,
            |ctx, _| freeze_panes(ctx),
        ),
        cmd(
            "viewer.grid.freezeTopRow",
            "Freeze Top Row",
            &["z t"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_frozen(1, 0)),
        ),
        cmd(
            "viewer.grid.freezeFirstColumn",
            "Freeze First Column",
            &["z shift+f"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_frozen(0, 1)),
        ),
        cmd(
            "viewer.grid.unfreezePanes",
            "Unfreeze Panes",
            &["z u"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_frozen(0, 0)),
        ),
        cmd(
            "viewer.grid.hideRows",
            "Hide Rows",
            &["ctrl+9", "z r"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_hidden(true, true)),
        ),
        cmd(
            "viewer.grid.unhideRows",
            "Unhide Rows",
            &["ctrl+shift+9", "z shift+r"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_hidden(true, false)),
        ),
        cmd(
            "viewer.grid.hideColumns",
            "Hide Columns",
            &["ctrl+0", "z c"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_hidden(false, true)),
        ),
        cmd(
            "viewer.grid.unhideColumns",
            "Unhide Columns",
            &["ctrl+shift+0", "z shift+c"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_hidden(false, false)),
        ),
        cmd(
            "viewer.grid.calculationOptions",
            "Calculation Options",
            &["z o"],
            IN_GRID,
            calculation_options,
        ),
        cmd(
            "viewer.grid.circularReferences",
            "Circular References",
            &[],
            IN_GRID,
            circular_references,
        ),
        cmd(
            "viewer.grid.moveOrCopySheet",
            "Move or Copy Sheet",
            &["shift+s m"],
            IN_GRID,
            move_or_copy,
        ),
        cmd(
            "viewer.grid.insertSheet",
            "Insert Sheet",
            &["shift+f11", "shift+s i"],
            IN_GRID,
            |ctx, args| sheet_command(ctx, args, "insert"),
        ),
        cmd(
            "viewer.grid.deleteSheet",
            "Delete Sheet",
            &["shift+s d"],
            IN_GRID,
            |ctx, args| sheet_command(ctx, args, "delete"),
        ),
        cmd(
            "viewer.grid.renameSheet",
            "Rename Sheet",
            &["shift+s r"],
            IN_GRID,
            |ctx, args| sheet_command(ctx, args, "rename"),
        ),
        cmd(
            "viewer.grid.moveSheetLeft",
            "Move Sheet Left",
            &["shift+s h"],
            IN_GRID,
            |ctx, args| sheet_command(ctx, args, "left"),
        ),
        cmd(
            "viewer.grid.moveSheetRight",
            "Move Sheet Right",
            &["shift+s l"],
            IN_GRID,
            |ctx, args| sheet_command(ctx, args, "right"),
        ),
        cmd(
            "viewer.grid.hideSheet",
            "Hide Sheet",
            &["shift+s x"],
            IN_GRID,
            |ctx, args| sheet_command(ctx, args, "hide"),
        ),
        cmd(
            "viewer.grid.unhideSheet",
            "Unhide Sheet",
            &["shift+s u"],
            IN_GRID,
            |ctx, args| sheet_command(ctx, args, "unhide"),
        ),
        cmd("viewer.grid.find", "Find", &["ctrl+f"], IN_GRID, grid_find),
        cmd(
            "viewer.grid.findNext",
            "Find Next",
            &["f3", "shift+f4"],
            IN_GRID,
            |ctx, _| find_report(ctx, true),
        ),
        cmd(
            "viewer.grid.findPrevious",
            "Find Previous",
            &["ctrl+shift+f4"],
            IN_GRID,
            |ctx, _| find_report(ctx, false),
        ),
        cmd(
            "viewer.grid.replace",
            "Replace",
            &["ctrl+h"],
            IN_GRID,
            grid_replace,
        ),
        cmd(
            "viewer.grid.findMatchCase",
            "Find: Match Case",
            &[],
            IN_GRID,
            |ctx, _| find_option(ctx, "case"),
        ),
        cmd(
            "viewer.grid.findWholeCell",
            "Find: Match Entire Cell Contents",
            &[],
            IN_GRID,
            |ctx, _| find_option(ctx, "whole"),
        ),
        cmd(
            "viewer.grid.findInFormulas",
            "Find: Look in Formulas",
            &[],
            IN_GRID,
            |ctx, _| find_option(ctx, "formulas"),
        ),
        cmd(
            "viewer.grid.pageDown",
            "Page Down",
            &["pagedown"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.grid_page(1);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.pageUp",
            "Page Up",
            &["pageup"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.grid_page(-1);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.rowStart",
            "First Column",
            &["home", "0"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    let p = v.grid_pos();
                    v.grid_move_to(p.row, 0);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.start",
            "First Cell",
            &["ctrl+home", "g g"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.grid_move_to(0, 0);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.end",
            "Last Cell",
            &["ctrl+end", "shift+g"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    let l = v.grid_layout().unwrap_or_default();
                    v.grid_move_to(l.rows.saturating_sub(1), l.cols.saturating_sub(1));
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.nextSheet",
            "Next Sheet",
            &["ctrl+pagedown", "g t"],
            IN_GRID,
            |ctx, _| turn(ctx, 1, false),
        ),
        cmd(
            "viewer.grid.previousSheet",
            "Previous Sheet",
            &["ctrl+pageup", "g shift+t"],
            IN_GRID,
            |ctx, _| turn(ctx, -1, false),
        ),
        cmd(
            "viewer.grid.edit",
            "Edit Cell",
            &["enter", "f2", "i"],
            IN_GRID,
            |ctx, _| ask_cell(ctx, None),
        ),
        cmd(
            "viewer.grid.editFormula",
            "Enter a Formula",
            &["="],
            IN_GRID,
            |ctx, _| ask_cell(ctx, Some("=")),
        ),
        cmd(
            "viewer.grid.setCell",
            "Set Cell",
            &[],
            IN_GRID,
            |ctx, args| {
                let (Some(row), Some(col)) = (arg_u32(args, "row"), arg_u32(args, "col")) else {
                    return Err(crate::command::CommandError::new("Which cell?"));
                };
                let value = args
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let force = args.get("force").and_then(serde_json::Value::as_bool) == Some(true);
                // Ctrl+Enter: into every selected cell, the selection kept.
                if args.get("inRange").and_then(serde_json::Value::as_bool) == Some(true) {
                    return with(ctx, |v| v.enter_in_selection(row, col, &value));
                }
                if !force && refused(ctx, row, col, &value) {
                    return Ok(());
                }
                with(ctx, |v| {
                    v.set_cell(row, col, &value)?;
                    // Enter moves down, as in a spreadsheet.
                    v.grid_move_to(row, col);
                    v.grid_move_by(1, 0);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.clear",
            "Clear Cells",
            &["delete", "backspace", "x"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.clear_selection()),
        ),
        cmd(
            "viewer.grid.copy",
            "Copy Cells",
            &["y"],
            IN_GRID,
            |ctx, _| copy(ctx),
        ),
        cmd(
            "viewer.grid.insertRow",
            "Insert Row Above",
            &["shift+o"],
            IN_GRID,
            |ctx, _| {
                grid_struct(ctx, |s| GridEdit::InsertRows {
                    at: s[0],
                    count: s[2] - s[0] + 1,
                })
            },
        ),
        cmd(
            "viewer.grid.deleteRow",
            "Delete Row",
            &["d d"],
            IN_GRID,
            |ctx, _| {
                grid_struct(ctx, |s| GridEdit::DeleteRows {
                    at: s[0],
                    count: s[2] - s[0] + 1,
                })
            },
        ),
        cmd(
            "viewer.grid.insertColumn",
            "Insert Column Left",
            &["c o"],
            IN_GRID,
            |ctx, _| {
                grid_struct(ctx, |s| GridEdit::InsertCols {
                    at: s[1],
                    count: s[3] - s[1] + 1,
                })
            },
        ),
        cmd(
            "viewer.grid.deleteColumn",
            "Delete Column",
            &["d c"],
            IN_GRID,
            |ctx, _| {
                grid_struct(ctx, |s| GridEdit::DeleteCols {
                    at: s[1],
                    count: s[3] - s[1] + 1,
                })
            },
        ),
        cmd(
            "viewer.grid.runMacro",
            "Run Macro",
            &["alt+f8"],
            IN_GRID,
            run_macro,
        ),
        cmd(
            "viewer.grid.autofitColumn",
            "Fit Column Width",
            &["c f"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    let col = v.grid_pos().col;
                    v.autofit_col(col, &text_cells)
                })
            },
        ),
        cmd("viewer.grid.cut", "Cut Cells", &[], IN_GRID, |ctx, _| {
            cut(ctx)
        }),
        cmd(
            "viewer.grid.sortAscending",
            "Sort A to Z",
            &["s a"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.sort(false)),
        ),
        cmd(
            "viewer.grid.sortDescending",
            "Sort Z to A",
            &["s d"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.sort(true)),
        ),
        cmd(
            "viewer.grid.toggleFilter",
            "Filter",
            &["f"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.toggle_filter()),
        ),
        cmd(
            "viewer.grid.filterColumn",
            "Filter Column",
            &["shift+f"],
            IN_GRID,
            choose_filter,
        ),
        cmd(
            "viewer.grid.setColumnFilter",
            "Set Column Filter",
            &[],
            IN_GRID,
            |ctx, args| {
                let Some(col) = args.get("col").and_then(serde_json::Value::as_u64) else {
                    return Err(crate::command::CommandError::new("Which column?"));
                };
                let list = |key: &str| {
                    args.get(key)
                        .and_then(serde_json::Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect::<Vec<String>>()
                        })
                };
                let values = if args.get("all").and_then(serde_json::Value::as_bool) == Some(true) {
                    None
                } else if let Some(vs) = list("values") {
                    Some(vs)
                } else {
                    Some(vec![
                        args.get("value")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    ])
                };
                with(ctx, |v| {
                    // Every value checked is no filter at all.
                    let values = values.filter(|vs| {
                        let all = v.filter_values(col as u32);
                        !all.iter().all(|x| vs.contains(x))
                    });
                    v.set_column_filter(col as u32, values)
                })
            },
        ),
        cmd(
            "viewer.grid.clearFilters",
            "Clear Filters",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.clear_filters()),
        ),
        cmd(
            "viewer.grid.pasteText",
            "Paste into Cells",
            &[],
            IN_GRID,
            |ctx, args| {
                let text = args
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                with(ctx, |v| v.paste_text(&text))
            },
        ),
        cmd(
            "viewer.grid.cancel",
            "Clear Selection",
            &["escape"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    let p = v.grid_pos();
                    v.grid_move_to(p.row, p.col);
                    v.cancel_cut();
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.selectUp",
            "Select Up",
            &["shift+up"],
            IN_GRID,
            |ctx, _| grid_select(ctx, -1, 0),
        ),
        cmd(
            "viewer.grid.selectDown",
            "Select Down",
            &["shift+down"],
            IN_GRID,
            |ctx, _| grid_select(ctx, 1, 0),
        ),
        cmd(
            "viewer.grid.selectLeft",
            "Select Left",
            &["shift+left"],
            IN_GRID,
            |ctx, _| grid_select(ctx, 0, -1),
        ),
        cmd(
            "viewer.grid.selectRight",
            "Select Right",
            &["shift+right"],
            IN_GRID,
            |ctx, _| grid_select(ctx, 0, 1),
        ),
        cmd(
            "viewer.grid.mergeCenter",
            "Merge and Center",
            &["m"],
            IN_GRID,
            |ctx, args| merge(ctx, args, true),
        ),
        cmd(
            "viewer.grid.merge",
            "Merge Cells",
            &[],
            IN_GRID,
            |ctx, args| merge(ctx, args, false),
        ),
        cmd(
            "viewer.grid.unmerge",
            "Unmerge Cells",
            &["shift+m"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.unmerge_at_cursor()),
        ),
        cmd(
            "viewer.grid.wrapText",
            "Wrap Text",
            &["w"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.toggle_wrap(&text_cells)),
        ),
        cmd(
            "viewer.grid.fitRowHeight",
            "Fit Row Height",
            &["r f"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    let row = v.grid_pos().row;
                    v.fit_row_height(row, &text_cells)
                })
            },
        ),
        cmd(
            "viewer.grid.tallerRow",
            "Taller Row",
            &["r +"],
            IN_GRID,
            |ctx, _| grid_height(ctx, 3.0),
        ),
        cmd(
            "viewer.grid.shorterRow",
            "Shorter Row",
            &["r -"],
            IN_GRID,
            |ctx, _| grid_height(ctx, -3.0),
        ),
        cmd(
            "viewer.grid.widenColumn",
            "Widen Column",
            &["c +"],
            IN_GRID,
            |ctx, _| grid_width(ctx, 1.0),
        ),
        cmd(
            "viewer.grid.narrowColumn",
            "Narrow Column",
            &["c -"],
            IN_GRID,
            |ctx, _| grid_width(ctx, -1.0),
        ),
        cmd(
            "viewer.grid.conditionalFormat",
            "Conditional Formatting",
            &["shift+c"],
            IN_GRID,
            conditional_menu,
        ),
        cmd(
            "viewer.grid.highlightGreater",
            "Highlight Cells Greater Than",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightGreater"),
        ),
        cmd(
            "viewer.grid.highlightLess",
            "Highlight Cells Less Than",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightLess"),
        ),
        cmd(
            "viewer.grid.highlightBetween",
            "Highlight Cells Between",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightBetween"),
        ),
        cmd(
            "viewer.grid.highlightEqual",
            "Highlight Cells Equal To",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightEqual"),
        ),
        cmd(
            "viewer.grid.highlightText",
            "Highlight Cells Containing Text",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightText"),
        ),
        cmd(
            "viewer.grid.highlightDuplicates",
            "Highlight Duplicate Values",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightDuplicates"),
        ),
        cmd(
            "viewer.grid.highlightUnique",
            "Highlight Unique Values",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightUnique"),
        ),
        cmd(
            "viewer.grid.highlightTop",
            "Highlight Top Items",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightTop"),
        ),
        cmd(
            "viewer.grid.highlightBottom",
            "Highlight Bottom Items",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightBottom"),
        ),
        cmd(
            "viewer.grid.highlightAboveAverage",
            "Highlight Above Average",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightAboveAverage"),
        ),
        cmd(
            "viewer.grid.highlightBelowAverage",
            "Highlight Below Average",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightBelowAverage"),
        ),
        cmd(
            "viewer.grid.highlightFormula",
            "Highlight Cells by Formula",
            &[],
            IN_GRID,
            |ctx, args| highlight(ctx, args, "viewer.grid.highlightFormula"),
        ),
        cmd(
            "viewer.grid.dataBars",
            "Data Bars",
            &[],
            IN_GRID,
            |ctx, args| graded(ctx, args, "viewer.grid.dataBars"),
        ),
        cmd(
            "viewer.grid.colorScale",
            "Color Scale",
            &[],
            IN_GRID,
            |ctx, args| graded(ctx, args, "viewer.grid.colorScale"),
        ),
        cmd(
            "viewer.grid.iconSet",
            "Icon Set",
            &[],
            IN_GRID,
            |ctx, args| graded(ctx, args, "viewer.grid.iconSet"),
        ),
        cmd(
            "viewer.grid.clearConditionalFormats",
            "Clear Rules from Selection",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.clear_conditional_formats(false)),
        ),
        cmd(
            "viewer.grid.clearSheetConditionalFormats",
            "Clear Rules from Sheet",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.clear_conditional_formats(true)),
        ),
        cmd(
            "viewer.grid.insertChart",
            "Insert Chart",
            &["alt+f1"],
            IN_GRID,
            insert_chart,
        ),
        cmd(
            "viewer.grid.moveChartUp",
            "Move Chart Up",
            &["p up"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(-1, 0, false)),
        ),
        cmd(
            "viewer.grid.moveChartDown",
            "Move Chart Down",
            &["p down"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(1, 0, false)),
        ),
        cmd(
            "viewer.grid.moveChartLeft",
            "Move Chart Left",
            &["p left"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(0, -1, false)),
        ),
        cmd(
            "viewer.grid.moveChartRight",
            "Move Chart Right",
            &["p right"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(0, 1, false)),
        ),
        cmd(
            "viewer.grid.chartTaller",
            "Make Chart Taller",
            &["p shift+down"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(1, 0, true)),
        ),
        cmd(
            "viewer.grid.chartShorter",
            "Make Chart Shorter",
            &["p shift+up"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(-1, 0, true)),
        ),
        cmd(
            "viewer.grid.chartWider",
            "Make Chart Wider",
            &["p shift+right"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(0, 1, true)),
        ),
        cmd(
            "viewer.grid.chartNarrower",
            "Make Chart Narrower",
            &["p shift+left"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.nudge_chart(0, -1, true)),
        ),
        cmd(
            "viewer.grid.chartTitle",
            "Chart Title",
            &["p t"],
            IN_GRID,
            |ctx, args| chart_text(ctx, args, "viewer.grid.chartTitle", None),
        ),
        cmd(
            "viewer.grid.horizontalAxisTitle",
            "Horizontal Axis Title",
            &["p x"],
            IN_GRID,
            |ctx, args| {
                chart_text(
                    ctx,
                    args,
                    "viewer.grid.horizontalAxisTitle",
                    Some(kalem_viewer::ChartAxis::Horizontal),
                )
            },
        ),
        cmd(
            "viewer.grid.verticalAxisTitle",
            "Vertical Axis Title",
            &["p y"],
            IN_GRID,
            |ctx, args| {
                chart_text(
                    ctx,
                    args,
                    "viewer.grid.verticalAxisTitle",
                    Some(kalem_viewer::ChartAxis::Vertical),
                )
            },
        ),
        cmd(
            "viewer.grid.chartLegend",
            "Chart Legend",
            &["p l"],
            IN_GRID,
            |ctx, args| {
                use kalem_viewer::LegendPosition as L;
                let places = [
                    ("bottom", Some(L::Bottom), "Bottom"),
                    ("top", Some(L::Top), "Top"),
                    ("left", Some(L::Left), "Left"),
                    ("right", Some(L::Right), "Right"),
                    ("topRight", Some(L::TopRight), "Top Right"),
                    ("none", None, "None"),
                ];
                match args
                    .get("position")
                    .and_then(|p| p.as_str())
                    .and_then(|p| places.iter().find(|x| x.0 == p))
                {
                    Some((_, position, _)) => with(ctx, |v| v.set_legend(*position)),
                    None => {
                        let items = places
                            .iter()
                            .map(|(key, _, title)| {
                                menu_item(
                                    "viewer.grid.chartLegend",
                                    serde_json::json!({ "position": key }),
                                    title,
                                    "Legend",
                                )
                            })
                            .collect();
                        ctx.requests.push(Request::Choose(items));
                        Ok(())
                    }
                }
            },
        ),
        cmd(
            "viewer.grid.dataLabels",
            "Data Labels",
            &["p d"],
            IN_GRID,
            data_labels,
        ),
        cmd(
            "viewer.grid.axisScale",
            "Axis Scale",
            &["p s"],
            IN_GRID,
            axis_scale,
        ),
        cmd(
            "viewer.grid.chartKind",
            "Change Chart Type",
            &["p k"],
            IN_GRID,
            |ctx, args| {
                use kalem_viewer::ChartKind as K;
                let kinds = [
                    ("column", K::Column, "Column"),
                    ("bar", K::Bar, "Bar"),
                    ("line", K::Line, "Line"),
                    ("area", K::Area, "Area"),
                    ("pie", K::Pie, "Pie"),
                    ("doughnut", K::Doughnut, "Doughnut"),
                    ("scatter", K::Scatter, "Scatter"),
                ];
                if let Some((_, kind, _)) = args
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .and_then(|k| kinds.iter().find(|x| x.0 == k))
                {
                    return with(ctx, |v| v.set_chart_kind(*kind));
                }
                let Some(v) = ctx
                    .document
                    .as_deref_mut()
                    .and_then(|d| d.viewer.as_deref_mut())
                else {
                    return Ok(());
                };
                let Some((i, _)) = v.chart_at_cursor() else {
                    ctx.messages
                        .push("Put the cursor on a chart to change its kind".into());
                    return Ok(());
                };
                let now = v.charts()[i].kind;
                let items = kinds
                    .iter()
                    .map(|(key, kind, title)| {
                        menu_item(
                            "viewer.grid.chartKind",
                            serde_json::json!({ "kind": key }),
                            &format!("{} {title}", if *kind == now { "●" } else { "○" }),
                            "Change Chart Type",
                        )
                    })
                    .collect();
                ctx.requests.push(Request::Choose(items));
                Ok(())
            },
        ),
        cmd(
            "viewer.grid.seriesColor",
            "Series Color",
            &["p c"],
            IN_GRID,
            series_color,
        ),
        cmd(
            "viewer.grid.pointColor",
            "Slice Color",
            &["p p"],
            IN_GRID,
            point_color,
        ),
        cmd(
            "viewer.grid.explodeSlice",
            "Explode Slice",
            &["p e"],
            IN_GRID,
            explode_slice,
        ),
        cmd(
            "viewer.grid.chartArea",
            "Chart Area",
            &["p b"],
            IN_GRID,
            chart_area,
        ),
        cmd(
            "viewer.grid.gridlines",
            "Gridlines",
            &["p g"],
            IN_GRID,
            gridlines_menu,
        ),
        cmd(
            "viewer.grid.axisFormat",
            "Axis Number Format",
            &["p n"],
            IN_GRID,
            axis_format,
        ),
        cmd(
            "viewer.grid.axisFont",
            "Axis Font",
            &["p f"],
            IN_GRID,
            axis_font,
        ),
        cmd(
            "viewer.grid.titleFont",
            "Title Font",
            &["p shift+t"],
            IN_GRID,
            |ctx, args| font_menu(ctx, args, "title"),
        ),
        cmd(
            "viewer.grid.legendFont",
            "Legend Font",
            &["p shift+l"],
            IN_GRID,
            |ctx, args| font_menu(ctx, args, "legend"),
        ),
        cmd(
            "viewer.grid.deleteChart",
            "Delete Chart",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.delete_chart()),
        ),
        cmd(
            "viewer.grid.insertPivot",
            "Insert PivotTable",
            &["shift+t"],
            IN_GRID,
            insert_pivot,
        ),
        cmd(
            "viewer.grid.refreshPivots",
            "Refresh All PivotTables",
            &["alt+f5"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.refresh_pivots()),
        ),
        cmd(
            "viewer.grid.bold",
            "Bold",
            &["ctrl+b", "t b"],
            IN_GRID,
            |ctx, _| toggle_font(ctx, "bold"),
        ),
        cmd(
            "viewer.grid.italic",
            "Italic",
            &["ctrl+i", "t i"],
            IN_GRID,
            |ctx, _| toggle_font(ctx, "italic"),
        ),
        cmd(
            "viewer.grid.underline",
            "Underline",
            &["ctrl+u", "t u"],
            IN_GRID,
            |ctx, _| toggle_font(ctx, "underline"),
        ),
        cmd(
            "viewer.grid.strikethrough",
            "Strikethrough",
            &["ctrl+5", "t s"],
            IN_GRID,
            |ctx, _| toggle_font(ctx, "strike"),
        ),
        cmd(
            "viewer.grid.fontColor",
            "Font Color",
            &["t c"],
            IN_GRID,
            |ctx, args| color_style(ctx, args, false),
        ),
        cmd(
            "viewer.grid.fillColor",
            "Fill Color",
            &["t f"],
            IN_GRID,
            |ctx, args| color_style(ctx, args, true),
        ),
        cmd(
            "viewer.grid.fontSize",
            "Font Size",
            &["t z"],
            IN_GRID,
            |ctx, args| font_choice(ctx, args, true),
        ),
        cmd(
            "viewer.grid.fontFace",
            "Font",
            &["t n"],
            IN_GRID,
            |ctx, args| font_choice(ctx, args, false),
        ),
        cmd(
            "viewer.grid.numberFormat",
            "Number Format",
            &["t 1"],
            IN_GRID,
            number_format,
        ),
        cmd(
            "viewer.grid.increaseDecimal",
            "Increase Decimal",
            &["t ."],
            IN_GRID,
            |ctx, _| step_decimals(ctx, 1),
        ),
        cmd(
            "viewer.grid.decreaseDecimal",
            "Decrease Decimal",
            &["t ,"],
            IN_GRID,
            |ctx, _| step_decimals(ctx, -1),
        ),
        cmd("viewer.grid.borders", "Borders", &["t d"], IN_GRID, borders),
        cmd(
            "viewer.grid.borderColor",
            "Line Color",
            &["t shift+d"],
            IN_GRID,
            border_color,
        ),
        cmd(
            "viewer.grid.alignLeft",
            "Align Left",
            &["t l"],
            IN_GRID,
            |ctx, _| align_style(ctx, Some(kalem_viewer::Align::Left), None),
        ),
        cmd(
            "viewer.grid.alignCenter",
            "Center",
            &["t e"],
            IN_GRID,
            |ctx, _| align_style(ctx, Some(kalem_viewer::Align::Center), None),
        ),
        cmd(
            "viewer.grid.alignRight",
            "Align Right",
            &["t r"],
            IN_GRID,
            |ctx, _| align_style(ctx, Some(kalem_viewer::Align::Right), None),
        ),
        cmd(
            "viewer.grid.alignGeneral",
            "General Alignment",
            &["t g"],
            IN_GRID,
            |ctx, _| align_style(ctx, Some(kalem_viewer::Align::General), None),
        ),
        cmd(
            "viewer.grid.alignTop",
            "Top Align",
            &["t shift+t"],
            IN_GRID,
            |ctx, _| align_style(ctx, None, Some(kalem_viewer::VAlign::Top)),
        ),
        cmd(
            "viewer.grid.alignMiddle",
            "Middle Align",
            &["t m"],
            IN_GRID,
            |ctx, _| align_style(ctx, None, Some(kalem_viewer::VAlign::Middle)),
        ),
        cmd(
            "viewer.grid.alignBottom",
            "Bottom Align",
            &["t shift+b"],
            IN_GRID,
            |ctx, _| align_style(ctx, None, Some(kalem_viewer::VAlign::Bottom)),
        ),
        cmd(
            "viewer.grid.fillDown",
            "Fill Down",
            &["ctrl+d"],
            IN_GRID,
            |ctx, _| fill_with_lists(ctx, |v| v.fill_down()),
        ),
        cmd(
            "viewer.grid.fillRight",
            "Fill Right",
            &["ctrl+r"],
            IN_GRID,
            |ctx, _| fill_with_lists(ctx, |v| v.fill_right()),
        ),
        cmd(
            "viewer.grid.fillToEnd",
            "Fill Down Along the Data",
            &[],
            IN_GRID,
            |ctx, _| fill_with_lists(ctx, |v| v.fill_to_end()),
        ),
        cmd(
            "viewer.grid.flashFill",
            "Flash Fill",
            &["ctrl+e"],
            IN_GRID,
            |ctx, _| {
                let Some(v) = ctx
                    .document
                    .as_deref_mut()
                    .and_then(|d| d.viewer.as_deref_mut())
                else {
                    return Ok(());
                };
                match v.flash_fill() {
                    Ok(n) => ctx.messages.push(format!("Flash Fill: {n} cells filled")),
                    Err(e) => ctx.messages.push(e),
                }
                Ok(())
            },
        ),
        cmd(
            "viewer.grid.customLists",
            "Custom Lists",
            &[],
            IN_GRID,
            custom_lists,
        ),
        cmd(
            "viewer.grid.fillSeries",
            "Fill Series",
            &[],
            IN_GRID,
            |ctx, args| {
                // A source and a target given (a script, a test), else the
                // selection.
                let range = |k: &str| -> Option<[u32; 4]> {
                    let a = args.get(k)?.as_array()?;
                    let v: Vec<u32> = a
                        .iter()
                        .filter_map(|x| x.as_u64().map(|n| n as u32))
                        .collect();
                    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                };
                match (range("source"), range("target")) {
                    (Some(s), Some(t)) => fill_with_lists(ctx, |v| v.fill_to(s, t, true)),
                    _ => fill_with_lists(ctx, |v| v.fill_series()),
                }
            },
        ),
        cmd(
            "viewer.grid.dataValidation",
            "Data Validation",
            &["shift+v"],
            IN_GRID,
            validation_menu,
        ),
        cmd(
            "viewer.grid.pickFromList",
            "Pick from List",
            &["alt+down"],
            IN_GRID,
            pick_from_list,
        ),
        cmd(
            "viewer.grid.editCellAgain",
            "Edit Cell Again",
            &[],
            IN_GRID,
            |ctx, args| {
                let (Some(row), Some(col)) = (arg_u32(args, "row"), arg_u32(args, "col")) else {
                    return Ok(());
                };
                ask_more(
                    ctx,
                    "viewer.grid.setCell",
                    &serde_json::json!({ "row": row, "col": col, "value_default": args.get("value_default") }),
                    "value",
                )
            },
        ),
        cmd(
            "viewer.grid.validateList",
            "Allow a List",
            &[],
            IN_GRID,
            |ctx, args| {
                let Some(value) = text_arg(args, "value") else {
                    return ask_more(ctx, "viewer.grid.validateList", args, "value");
                };
                revalidate(ctx, |x| {
                    x.kind = kalem_viewer::ValidationKind::List;
                    x.value = value;
                    x.value2 = None;
                    x.dropdown = true;
                })
            },
        ),
        cmd(
            "viewer.grid.validateNumber",
            "Allow Numbers",
            &[],
            IN_GRID,
            validate_number,
        ),
        cmd(
            "viewer.grid.validateFormula",
            "Allow by a Formula",
            &[],
            IN_GRID,
            |ctx, args| {
                let Some(value) = text_arg(args, "value") else {
                    return ask_more(ctx, "viewer.grid.validateFormula", args, "value");
                };
                revalidate(ctx, |x| {
                    x.kind = kalem_viewer::ValidationKind::Custom;
                    x.value = value;
                    x.value2 = None;
                })
            },
        ),
        cmd(
            "viewer.grid.validationMessage",
            "Validation Input Message",
            &[],
            IN_GRID,
            |ctx, args| {
                let Some(value) = args
                    .get("value")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                else {
                    return ask_more(ctx, "viewer.grid.validationMessage", args, "value");
                };
                revalidate(ctx, |x| {
                    x.prompt = (!value.trim().is_empty()).then(|| (String::new(), value));
                })
            },
        ),
        cmd(
            "viewer.grid.validationAlert",
            "Validation Error Alert",
            &[],
            IN_GRID,
            |ctx, args| {
                let style = match args.get("style").and_then(|s| s.as_str()) {
                    Some("none") => None,
                    Some("warning") => Some(ErrorStyle::Warning),
                    Some("information") => Some(ErrorStyle::Information),
                    _ => Some(ErrorStyle::Stop),
                };
                let Some(style) = style else {
                    return revalidate(ctx, |x| x.error = None);
                };
                let Some(value) = args
                    .get("value")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                else {
                    return ask_more(ctx, "viewer.grid.validationAlert", args, "value");
                };
                revalidate(ctx, |x| x.error = Some((style, String::new(), value)))
            },
        ),
        cmd(
            "viewer.grid.clearValidation",
            "Clear Validation",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.set_validation(None)),
        ),
        cmd(
            "viewer.grid.circleInvalid",
            "Circle Invalid Data",
            &[],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    v.circle_invalid = !v.circle_invalid;
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.grid.autofitColumns",
            "Fit All Column Widths",
            &["c a"],
            IN_GRID,
            |ctx, _| {
                with(ctx, |v| {
                    for col in 0..v.used_cols() {
                        v.autofit_col(col, &text_cells)?;
                    }
                    Ok(())
                })
            },
        ),
    ];
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_as_ranges() {
        let cells = [(0, 0), (0, 1), (1, 0), (1, 1), (1, 3), (3, 2)];
        assert_eq!(
            areas_of(&cells),
            vec![[0, 0, 1, 1], [1, 3, 1, 3], [3, 2, 3, 2]]
        );
    }

    #[test]
    fn decimals_increased_and_decreased() {
        let more = |c: &str| change_decimals(c, 1, "").unwrap();
        let fewer = |c: &str| change_decimals(c, -1, "");
        assert_eq!(more("0"), "0.0");
        assert_eq!(more("#,##0.00"), "#,##0.000");
        assert_eq!(more("0%"), "0.0%");
        assert_eq!(more("0.00E+00"), "0.000E+00");
        assert_eq!(more("#,##0.00 \"₺\""), "#,##0.000 \"₺\"");
        assert_eq!(more("#,##0_);[Red](#,##0)"), "#,##0.0_);[Red](#,##0.0)");
        assert_eq!(fewer("0.0").as_deref(), Some("0"));
        assert_eq!(fewer("#,##0.00").as_deref(), Some("#,##0.0"));
        assert_eq!(fewer("0").as_deref(), None);
        // Dates and text have no decimals.
        assert_eq!(change_decimals("dd.mm.yyyy", 1, "04.10.2026"), None);
        assert_eq!(change_decimals("@", 1, "x"), None);
        // General: from what the cell shows.
        assert_eq!(
            change_decimals("General", 1, "1200").as_deref(),
            Some("0.0")
        );
        assert_eq!(
            change_decimals("General", -1, "3.25").as_deref(),
            Some("0.0")
        );
        assert_eq!(change_decimals("General", -1, "3"), None);
    }

    #[test]
    fn axis_numbers_formatted() {
        let f = format_axis_number;
        assert_eq!(f(1234.5, "General"), "1234.5");
        assert_eq!(f(1234.5, "0"), "1235");
        assert_eq!(f(1234.5, "#,##0.00"), "1,234.50");
        assert_eq!(f(0.256, "0%"), "26%");
        assert_eq!(f(0.256, "0.0%"), "25.6%");
        assert_eq!(f(1234.5, "#,##0 \"TL\""), "1,235 TL");
        assert_eq!(f(1234.5, "\"$\"#,##0.00"), "$1,234.50");
        assert_eq!(f(1234.5, "[$₺-41F]#,##0"), "₺1,235");
        assert_eq!(f(1_250_000.0, "#,##0,\"K\""), "1,250K");
        assert_eq!(f(1234.5, "0.00E+00"), "1.23E+03");
        assert_eq!(f(-1500.0, "#,##0;(#,##0)"), "-1,500");
        assert_eq!(f(7.0, "0.0 \\h"), "7.0 h");
    }
    use kalem_viewer::{Detection, Result as VResult, Unit};

    /// Numbered pages 100 × 50, each one color.
    #[derive(Debug)]
    struct Pages(usize);

    struct PagesDoc(usize, u8);

    impl Viewer for Pages {
        fn id(&self) -> &str {
            "pages"
        }
        fn name(&self) -> &str {
            "Pages"
        }
        fn extensions(&self) -> &[&str] {
            &["pages"]
        }
        fn detect(&self, name: &str, head: &[u8]) -> Detection {
            if head.starts_with(b"PAGES\0") {
                Detection::Magic
            } else if name.ends_with(".pages") {
                Detection::Extension
            } else {
                Detection::No
            }
        }
        fn open(&self, _file: FileHandle) -> VResult<Box<dyn ViewerDocument>> {
            Ok(Box::new(PagesDoc(self.0, 0)))
        }
    }

    impl ViewerDocument for PagesDoc {
        fn structure(&self) -> Structure {
            Structure {
                units: (0..self.0)
                    .map(|i| Unit {
                        kind: UnitKind::Page,
                        label: format!("{}", i + 1),
                        duration_ms: None,
                    })
                    .collect(),
                outline: Vec::new(),
            }
        }
        fn render(&mut self, unit: usize, _: RenderRequest) -> VResult<Rendered> {
            let px = [unit as u8, self.1, 0, 255];
            Ok(Rendered::Bitmap(Bitmap::new(100, 50, px.repeat(100 * 50))))
        }
        fn text(&self, unit: usize) -> String {
            format!("page {}", unit + 1)
        }
        fn text_rects(&self, _: usize, r: std::ops::Range<usize>) -> Vec<[f32; 4]> {
            // Each byte 10 pixels wide on a line 10 high at y 30.
            vec![[
                r.start as f32 * 10.0,
                30.0,
                (r.end - r.start) as f32 * 10.0,
                10.0,
            ]]
        }
        fn text_at(
            &self,
            unit: usize,
            x: f32,
            _: f32,
        ) -> Option<(std::ops::Range<usize>, [f32; 4])> {
            let n = self.text(unit).len();
            let i = ((x / 10.0).max(0.0) as usize).min(n - 1);
            Some((i..i + 1, [i as f32 * 10.0, 30.0, 10.0, 10.0]))
        }
    }

    fn state(n: usize) -> ViewerState {
        let dir = std::env::temp_dir();
        ViewerState::open(Arc::new(Pages(n)), &dir.join("x.pages")).unwrap()
    }

    #[test]
    fn fit_and_zoom() {
        let mut v = state(1);
        v.set_area(400.0, 400.0);
        // Never larger than its own size when fitting.
        assert_eq!(v.scale(), 1.0);
        let p = v.placement();
        assert_eq!((p.x, p.y, p.width, p.height), (150.0, 175.0, 100.0, 50.0));
        v.set_area(50.0, 50.0);
        assert_eq!(v.scale(), 0.5);
        v.zoom_by(4.0);
        assert_eq!(v.scale(), 2.0);
        // 200 × 100 in a 50 × 50 area, centered on the middle.
        let p = v.placement();
        assert_eq!((p.x, p.y), (-75.0, -25.0));
        v.pan(1000.0, 0.0);
        let p = v.placement();
        // Panned to the right edge, no further.
        assert_eq!(p.x, 50.0 - 200.0);
        assert_eq!(p.visible(50.0, 50.0), (75.0, 12.5, 25.0, 25.0));
        v.fit();
        assert_eq!(v.scale(), 0.5);
    }

    #[test]
    fn the_wheel_zooms_at_the_mouse() {
        let mut v = state(1);
        v.set_area(100.0, 50.0);
        // The point under the mouse (the bitmap's (25, 25)) stays there.
        v.zoom_at(2.0, 25.0, 25.0);
        let p = v.placement();
        assert_eq!(p.scale, 2.0);
        assert_eq!(
            ((25.0 - p.x) / p.scale, (25.0 - p.y) / p.scale),
            (25.0, 25.0)
        );
    }

    #[test]
    fn rotation_turns_the_bitmap() {
        let mut v = state(1);
        v.rotate(1);
        let b = v.bitmap().unwrap();
        assert_eq!((b.width, b.height), (50, 100));
        v.rotate(-2);
        assert_eq!(v.rotation, 3);
    }

    #[test]
    fn pages_are_paged() {
        let mut v = state(3);
        assert!(v.paged());
        assert!(v.go_to(2));
        assert!(!v.go_to(3));
        assert_eq!(v.bitmap().unwrap().rgba[0], 2);
        assert_eq!(v.text(), "page 3");
        v.set_area(100.0, 50.0);
        assert_eq!(v.status(), "100 × 50 · 100% · 3/3");
    }

    /// Pages 100 × 50 at scale 1 that know their size, rendered at the
    /// scale asked.
    #[derive(Debug)]
    struct Vector(usize);

    struct VectorDoc(usize);

    impl Viewer for Vector {
        fn id(&self) -> &str {
            "vector"
        }
        fn name(&self) -> &str {
            "Vector"
        }
        fn extensions(&self) -> &[&str] {
            &["vector"]
        }
        fn detect(&self, _: &str, _: &[u8]) -> Detection {
            Detection::No
        }
        fn open(&self, _file: FileHandle) -> VResult<Box<dyn ViewerDocument>> {
            Ok(Box::new(VectorDoc(self.0)))
        }
    }

    impl ViewerDocument for VectorDoc {
        fn structure(&self) -> Structure {
            Structure {
                units: (0..self.0)
                    .map(|i| Unit {
                        kind: UnitKind::Page,
                        label: format!("{}", i + 1),
                        duration_ms: None,
                    })
                    .collect(),
                outline: Vec::new(),
            }
        }
        fn size(&self, _: usize) -> Option<(f32, f32)> {
            Some((100.0, 50.0))
        }
        fn render(&mut self, _: usize, r: RenderRequest) -> VResult<Rendered> {
            let (w, h) = (
                (100.0 * r.scale).ceil() as u32,
                (50.0 * r.scale).ceil() as u32,
            );
            Ok(Rendered::Bitmap(Bitmap::new(
                w,
                h,
                vec![0; (w * h * 4) as usize],
            )))
        }
        fn text(&self, _: usize) -> String {
            String::new()
        }
        fn links(&self, _: usize) -> Vec<kalem_viewer::Link> {
            vec![
                kalem_viewer::Link {
                    rect: [10.0, 10.0, 10.0, 10.0],
                    target: "#2".into(),
                },
                kalem_viewer::Link {
                    rect: [70.0, 10.0, 20.0, 10.0],
                    target: "https://example.org/".into(),
                },
            ]
        }
    }

    #[test]
    fn a_page_renders_at_the_scale_shown() {
        let dir = std::env::temp_dir();
        let mut v = ViewerState::open(Arc::new(Vector(1)), &dir.join("x.vector")).unwrap();
        v.set_area(400.0, 400.0);
        // A page fits larger than its own size, and is drawn at the
        // display's pixels.
        assert_eq!(v.scale(), 4.0);
        v.set_pixel_ratio(2.0);
        assert_eq!(v.render_scale(), 8.0);
        let b = v.bitmap().unwrap();
        assert_eq!((b.width, b.height), (800, 400));
        let p = v.placement();
        assert_eq!((p.width, p.height), (400.0, 200.0));
        assert_eq!(v.status(), "100 × 50 · 400%");
        // A small zoom renders at the next quarter octave up.
        v.zoom_by(1.1);
        let s = v.render_scale();
        assert!(s >= v.scale() * 2.0 && s < v.scale() * 2.0 * 1.19, "{s}");
        // Turned, the page is 50 × 100.
        v.rotate(1);
        assert_eq!(v.unit_size(), (50.0, 100.0));
        let b = v.bitmap().unwrap();
        assert!(b.height > b.width);
    }

    #[test]
    fn pages_fill_the_width_and_scroll_on_to_the_next() {
        let dir = std::env::temp_dir();
        let mut v = ViewerState::open(Arc::new(Vector(3)), &dir.join("x.vector")).unwrap();
        // A 100 × 50 page in a 200 × 40 area: twice as large, its top shown.
        v.set_area(200.0, 40.0);
        assert_eq!(v.zoom, Zoom::FitWidth);
        assert_eq!(v.scale(), 2.0);
        let p = v.placement();
        assert_eq!((p.x, p.y, p.width, p.height), (0.0, 0.0, 200.0, 100.0));
        // Scrolled to the bottom: no turn yet.
        v.scroll(0.0, 60.0);
        assert_eq!((v.unit, v.placement().y), (0, -60.0));
        // Pushed on past a fifth of the area: the next page's top.
        v.scroll(0.0, 5.0);
        assert_eq!(v.unit, 0);
        v.scroll(0.0, 5.0);
        assert_eq!((v.unit, v.placement().y), (1, 0.0));
        // Back up past the top: the previous page's bottom.
        v.scroll(0.0, -10.0);
        assert_eq!((v.unit, v.placement().y), (0, -60.0));
        // The whole page, and back to the width.
        v.fit();
        assert_eq!(v.scale(), 0.8);
        v.fit_width();
        assert_eq!(v.scale(), 2.0);
    }

    #[test]
    fn a_click_on_a_link_follows_it() {
        let dir = std::env::temp_dir();
        let mut v = ViewerState::open(Arc::new(Vector(3)), &dir.join("x.vector")).unwrap();
        // The page is drawn at twice its size in a 200 × 100 area.
        v.set_area(200.0, 100.0);
        assert_eq!(v.link_at(30.0, 30.0).as_deref(), Some("#2"));
        assert_eq!(
            v.link_at(150.0, 30.0).as_deref(),
            Some("https://example.org/")
        );
        assert_eq!(v.link_at(100.0, 90.0), None);
        assert_eq!(v.follow("#2"), None);
        assert_eq!(v.unit, 2);
        assert_eq!(
            v.follow("https://example.org/").as_deref(),
            Some("https://example.org/")
        );
        // Turned a quarter: the unit's (10, 10) is now at the area's
        // right; its first link is there.
        v.go_to(0);
        v.rotate(1);
        v.fit_width();
        let p = v.placement();
        let (x, y) = (p.x + (50.0 - 15.0) * p.scale, p.y + 15.0 * p.scale);
        assert_eq!(v.link_at(x, y).as_deref(), Some("#2"));
    }

    /// [`Vector`]'s pages, rendered slowly, with an edit and unsaved
    /// changes: a big page through a component.
    #[derive(Debug)]
    struct Slow;

    struct SlowDoc(VectorDoc);

    impl Viewer for Slow {
        fn id(&self) -> &str {
            "slow"
        }
        fn name(&self) -> &str {
            "Slow"
        }
        fn extensions(&self) -> &[&str] {
            &["slow"]
        }
        fn detect(&self, _: &str, _: &[u8]) -> Detection {
            Detection::No
        }
        fn open(&self, _file: FileHandle) -> VResult<Box<dyn ViewerDocument>> {
            Ok(Box::new(SlowDoc(VectorDoc(3))))
        }
    }

    impl ViewerDocument for SlowDoc {
        fn structure(&self) -> Structure {
            self.0.structure()
        }
        fn size(&self, u: usize) -> Option<(f32, f32)> {
            self.0.size(u)
        }
        fn render(&mut self, u: usize, r: RenderRequest) -> VResult<Rendered> {
            std::thread::sleep(std::time::Duration::from_millis(400));
            self.0.render(u, r)
        }
        fn text(&self, _: usize) -> String {
            "page".into()
        }
        fn links(&self, u: usize) -> Vec<kalem_viewer::Link> {
            self.0.links(u)
        }
        fn edits(&self, _: usize) -> Vec<Edit> {
            vec![Edit {
                id: "turn".into(),
                title: "Turn".into(),
                inverse: None,
            }]
        }
        fn modified(&self) -> bool {
            true
        }
    }

    #[test]
    fn a_render_never_holds_up_the_frontend() {
        // What a frontend asks on every frame or key (whether the document
        // is modified, the edits its keys' context reads, the link under
        // the pointer, a terminal's text) answers at once while a slow
        // render holds the document: a page turn never freezes the window.
        let dir = std::env::temp_dir();
        let mut v = ViewerState::open(Arc::new(Slow), &dir.join("x.slow")).unwrap();
        v.set_area(200.0, 100.0);
        // Read once, before any render.
        assert!(v.modified());
        assert_eq!(v.edits().len(), 1);
        assert_eq!(v.text_now(), "page");
        assert!(v.bitmap_now().unwrap().is_none());
        assert!(v.rendering());
        std::thread::sleep(std::time::Duration::from_millis(30));
        let t = std::time::Instant::now();
        assert!(v.modified(), "the last answer");
        assert_eq!(v.edits().len(), 1);
        assert_eq!(v.text_now(), "page");
        let _ = v.link_under(10.0, 10.0);
        let took = t.elapsed();
        assert!(took < std::time::Duration::from_millis(100), "{took:?}");
        assert!(v.rendering(), "the render still runs");
    }

    #[test]
    fn a_page_renders_on_a_thread() {
        let dir = std::env::temp_dir();
        let mut v = ViewerState::open(Arc::new(Vector(3)), &dir.join("x.vector")).unwrap();
        v.set_area(200.0, 100.0);
        let ready = |v: &mut ViewerState| loop {
            if let Some(b) = v.bitmap_now().unwrap()
                && !v.rendering()
            {
                return b;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        // Nothing yet: a thread renders the page.
        assert!(v.bitmap_now().unwrap().is_none());
        assert!(v.rendering());
        let (b, s) = ready(&mut v);
        assert_eq!((b.width, s), (200, 2.0));
        // Zoomed: the page at its old scale while the new one renders.
        v.zoom_by(2.0);
        let (b, s) = v.bitmap_now().unwrap().expect("the old render");
        assert_eq!((b.width, s), (200, 2.0));
        assert!(v.rendering());
        let (b, s) = ready(&mut v);
        assert_eq!((b.width, s), (400, 4.0));
        // A page that is not a neighbor: nothing until it is rendered.
        v.go_to(2);
        assert!(v.bitmap_now().unwrap().is_none());
        // The waiting call takes the thread's result.
        assert_eq!(v.bitmap().unwrap().width, 400);
    }

    #[test]
    fn neighbors_are_rendered_ahead() {
        let dir = std::env::temp_dir();
        let mut v = ViewerState::open(Arc::new(Vector(3)), &dir.join("x.vector")).unwrap();
        v.set_area(200.0, 100.0);
        // Shown, then its neighbor rendered while it is read.
        let settle = |v: &mut ViewerState| {
            for _ in 0..1000 {
                let _ = v.bitmap_now().unwrap();
                if !v.rendering() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        settle(&mut v);
        // The next page is there at once, and the first stays near.
        v.go_to(1);
        let (b, s) = v.bitmap_now().unwrap().expect("rendered ahead");
        assert_eq!((b.width, s), (200, 2.0));
        settle(&mut v);
        v.go_to(0);
        assert!(v.bitmap_now().unwrap().is_some(), "the page just left");
        // Zoomed, the neighbors at the old scale are dropped.
        v.zoom_by(2.0);
        let _ = v.bitmap_now();
        assert!(v.ahead.iter().all(|(k, _)| f32::from_bits(k.3) == 4.0));
        assert!(v.ahead.len() <= 2);
    }

    #[test]
    fn tab_separated_text() {
        assert_eq!(
            parse_tsv("a\tb\r\nc\td\r\n"),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
        assert_eq!(
            parse_tsv("\"x\ty\"\t\"say \"\"hi\"\"\"\n\"two\nlines\""),
            vec![
                vec!["x\ty".to_string(), "say \"hi\"".into()],
                vec!["two\nlines".into()],
            ]
        );
        assert_eq!(parse_tsv("1,300.00"), vec![vec!["1,300.00"]]);
        assert!(parse_tsv("").is_empty());
        assert_eq!(parse_tsv("a\t\tc"), vec![vec!["a", "", "c"]]);
    }

    #[test]
    fn a_search_runs_on_a_thread_and_goes_to_its_matches() {
        let mut v = state(12);
        v.go_to(4);
        v.search_start("PAGE 1");
        let wait = |v: &mut ViewerState| {
            while v.searching() {
                v.search_poll();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            v.search_poll();
        };
        wait(&mut v);
        // "page 1", "page 10", "page 11", "page 12": from page 5 on, the
        // first is page 10.
        assert_eq!(v.search_status(), "2/4");
        assert_eq!(v.unit, 9);
        v.search_next(false);
        assert_eq!((v.unit, v.search_status().as_str()), (10, "3/4"));
        v.search_next(false);
        v.search_next(false);
        // Round the document to page 1.
        assert_eq!((v.unit, v.search_status().as_str()), (0, "1/4"));
        v.search_next(true);
        assert_eq!(v.unit, 11);
        // Another query replaces it; an empty one clears it.
        v.search_start("nothing");
        wait(&mut v);
        assert_eq!(v.search_status(), "0/0");
        v.search_start("");
        assert_eq!(v.search_status(), "");
    }

    #[test]
    fn text_is_selected_by_dragging_over_it() {
        let mut v = state(3);
        v.set_area(100.0, 50.0);
        // Off the text: no selection (a drag there pans).
        assert!(!v.select_from(5.0, 5.0));
        // "page 1": from the space (x 45) back to the a (x 15).
        assert!(v.select_from(45.0, 35.0));
        v.select_to(15.0, 35.0);
        assert_eq!(v.selected_text().as_deref(), Some("age "));
        assert_eq!(v.selection_marks(), [[10.0, 30.0, 40.0, 10.0]]);
        // Past the line's end: to its last glyph.
        v.select_to(500.0, 35.0);
        assert_eq!(v.selected_text().as_deref(), Some(" 1"));
        // Another page drops it.
        v.go_to(1);
        assert_eq!(v.selected_text(), None);
        assert!(v.selection_marks().is_empty());
    }

    #[test]
    fn a_double_click_selects_a_word_and_a_triple_one_the_line() {
        assert_eq!(word_around("say hello, world", 6..7), 4..9);
        assert_eq!(word_around("say hello, world", 9..10), 9..10);
        assert_eq!(word_around("çağrı_2 x", 0..2), 0..10);
        assert_eq!(line_around("one\ntwo three\nfour", 5..6), 4..13);
        let mut v = state(1);
        v.set_area(100.0, 50.0);
        // "page 1": a double click on the g (x 25) selects "page".
        assert!(v.select_word(25.0, 35.0, false));
        assert_eq!(v.selected_text().as_deref(), Some("page"));
        assert!(v.select_word(25.0, 35.0, true));
        assert_eq!(v.selected_text().as_deref(), Some("page 1"));
        // Off the text: nothing.
        assert!(!v.select_word(5.0, 5.0, false));
        assert_eq!(v.selected_text(), None);
    }

    #[test]
    fn matches_are_marked_where_they_stand() {
        let mut v = state(3);
        v.set_area(100.0, 50.0);
        v.search_start("2");
        while v.searching() {
            v.search_poll();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        v.search_poll();
        assert_eq!(v.unit, 1);
        // "page 2": the 2 at byte 5, 10 wide at x 50, y 30; the page is
        // drawn at its size, from the area's corner.
        assert_eq!(v.search_marks(), [([50.0, 30.0, 10.0, 10.0], true)]);
        // Turned a quarter: the page is 50 × 100; x is 50 - 30 - 10.
        v.rotate(1);
        v.set_area(50.0, 100.0);
        assert_eq!(v.search_marks(), [([10.0, 50.0, 10.0, 10.0], true)]);
        v.search_end();
        assert!(v.search_marks().is_empty());
    }

    #[test]
    fn detection_prefers_magic() {
        register(Arc::new(Pages(1)));
        assert!(find("a.pages", b"PAGES\0").is_some());
        assert!(find("a.txt", b"PAGES\0").is_some());
        assert!(find("a.txt", b"hello").is_none());
    }
}
