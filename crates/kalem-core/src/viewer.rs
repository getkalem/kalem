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
use crate::keys::KeySequence;
use crate::when::WhenClause;

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
}

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
        let doc = viewer
            .open(FileHandle::new(path))
            .map_err(|e| e.to_string())?;
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
        });
        true
    }

    /// Extends the selection to the glyph at or nearest (`x`, `y`).
    pub fn select_to(&mut self, x: f32, y: f32) {
        if self.text_sel.is_none() {
            return;
        }
        let (ux, uy) = self.unit_point(x, y);
        let Ok(doc) = self.doc.try_lock() else {
            return;
        };
        let hit = doc.text_at(self.unit, ux, uy);
        drop(doc);
        if let (Some((r, _)), Some(sel)) = (hit, &mut self.text_sel) {
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
        let text = self.doc().text(sel.unit);
        text.get(sel.range()).map(str::to_string)
    }

    /// The selection's rectangles in the area's pixels as placed (x, y,
    /// width, height); read from the viewer once per range, none in a
    /// frame a render holds the document.
    pub fn selection_marks(&mut self) -> Vec<[f32; 4]> {
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

    /// The edits the format allows on the unit shown.
    pub fn edits(&self) -> Vec<Edit> {
        self.doc().edits(self.unit)
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
        self.doc().modified()
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
            if let Some(note) = self.doc().cell_note(self.unit, p.row, p.col) {
                parts.push(note.lines().next().unwrap_or_default().to_string());
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

    /// The cells of the grid shown in `rows` × `cols`.
    pub fn grid_cells(
        &mut self,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32, GridCell)> {
        self.doc().grid_cells(self.unit, rows, cols)
    }

    /// The cursor and scroll of the grid shown.
    pub fn grid_pos(&self) -> GridPos {
        self.grid_pos.get(&self.unit).copied().unwrap_or_default()
    }

    /// Tells the state how many rows and columns the frontend shows,
    /// frozen ones included, for paging and keeping the cursor in view.
    pub fn set_grid_visible(&mut self, rows: u32, cols: u32) {
        self.grid_visible = (rows.max(1), cols.max(1));
        let p = self.grid_pos();
        self.place(p.row, p.col);
    }

    /// Scrolls by whole rows and columns, the cursor kept.
    pub fn grid_scroll(&mut self, rows: i64, cols: i64) {
        let Some(l) = self.grid_layout() else { return };
        let mut p = self.grid_pos();
        p.top = (i64::from(p.top) + rows).clamp(
            i64::from(l.frozen.0),
            i64::from(l.max_rows.saturating_sub(1)),
        ) as u32;
        p.left = (i64::from(p.left) + cols).clamp(
            i64::from(l.frozen.1),
            i64::from(l.max_cols.saturating_sub(1)),
        ) as u32;
        self.grid_pos.insert(self.unit, p);
    }

    /// Puts the cursor on a cell and scrolls it into view; the selection
    /// goes; a cell inside a merged one is its first cell.
    pub fn grid_move_to(&mut self, row: u32, col: u32) {
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
        let (fr, fc) = l.frozen;
        let rows = self.grid_visible.0.saturating_sub(fr).max(1);
        let cols = self.grid_visible.1.saturating_sub(fc).max(1);
        p.top = p.top.max(fr);
        p.left = p.left.max(fc);
        if p.row >= fr {
            if p.row < p.top {
                p.top = p.row;
            } else if p.row >= p.top + rows {
                p.top = p.row + 1 - rows;
            }
        }
        if p.col >= fc {
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

    /// Forgets the cut (Escape).
    pub fn cancel_cut(&mut self) {
        self.cut = None;
    }

    /// The block of filled cells around the cursor, bounded by empty rows
    /// and columns, as Excel's current region: first row, first column,
    /// last row, last column.
    pub fn current_region(&mut self) -> [u32; 4] {
        let p = self.grid_pos();
        let Some(l) = self.grid_layout() else {
            return [p.row, p.col, p.row, p.col];
        };
        let filled: std::collections::HashSet<(u32, u32)> = self
            .doc()
            .grid_cells(self.unit, 0..l.rows.max(1), 0..l.cols.max(1))
            .into_iter()
            .filter(|(_, _, c)| !c.text.is_empty())
            .map(|(r, c, _)| (r, c))
            .collect();
        let mut b = [p.row, p.col, p.row, p.col];
        loop {
            let mut g = b;
            let row_has = |r: i64, c0: u32, c1: u32| {
                r >= 0 && (c0.saturating_sub(1)..=c1 + 1).any(|c| filled.contains(&(r as u32, c)))
            };
            let col_has = |c: i64, r0: u32, r1: u32| {
                c >= 0 && (r0.saturating_sub(1)..=r1 + 1).any(|r| filled.contains(&(r, c as u32)))
            };
            if row_has(i64::from(b[0]) - 1, b[1], b[3]) {
                g[0] = b[0] - 1;
            }
            if row_has(i64::from(b[2]) + 1, b[1], b[3]) {
                g[2] = b[2] + 1;
            }
            if col_has(i64::from(b[1]) - 1, b[0], b[2]) {
                g[1] = b[1] - 1;
            }
            if col_has(i64::from(b[3]) + 1, b[0], b[2]) {
                g[3] = b[3] + 1;
            }
            if g == b {
                return b;
            }
            b = g;
        }
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

    /// Clears the selection's values, formats kept (Delete).
    pub fn clear_selection(&mut self) -> Result<(), String> {
        if !self.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let s = self.selection();
        self.doc()
            .clear_cells(self.unit, s)
            .map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
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
        default_keys: keys
            .iter()
            .map(|k| KeySequence::parse(k).expect("valid default key"))
            .collect(),
        when: Some(WhenClause::parse(when).expect("valid when-clause")),
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

fn grid_struct(ctx: &mut EditorContext<'_>, f: fn(GridPos) -> GridEdit) -> CommandResult {
    with(ctx, |v| {
        if !v.grid_editable() {
            return Err("This file is shown, not edited".into());
        }
        let p = v.grid_pos();
        v.grid_edit(f(p))
    })
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
                rows.last_mut()
                    .expect("a row")
                    .push(std::mem::take(&mut field));
                at_start = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                rows.last_mut()
                    .expect("a row")
                    .push(std::mem::take(&mut field));
                rows.push(Vec::new());
                at_start = true;
            }
            c => {
                field.push(c);
                at_start = false;
            }
        }
    }
    rows.last_mut().expect("a row").push(field);
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
                grid_struct(ctx, |p| GridEdit::InsertRows {
                    at: p.row,
                    count: 1,
                })
            },
        ),
        cmd(
            "viewer.grid.deleteRow",
            "Delete Row",
            &["d d"],
            IN_GRID,
            |ctx, _| {
                grid_struct(ctx, |p| GridEdit::DeleteRows {
                    at: p.row,
                    count: 1,
                })
            },
        ),
        cmd(
            "viewer.grid.insertColumn",
            "Insert Column Left",
            &["c o"],
            IN_GRID,
            |ctx, _| {
                grid_struct(ctx, |p| GridEdit::InsertCols {
                    at: p.col,
                    count: 1,
                })
            },
        ),
        cmd(
            "viewer.grid.deleteColumn",
            "Delete Column",
            &["d c"],
            IN_GRID,
            |ctx, _| {
                grid_struct(ctx, |p| GridEdit::DeleteCols {
                    at: p.col,
                    count: 1,
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
            "viewer.grid.fillDown",
            "Fill Down",
            &["ctrl+d"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.fill_down()),
        ),
        cmd(
            "viewer.grid.fillRight",
            "Fill Right",
            &["ctrl+r"],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.fill_right()),
        ),
        cmd(
            "viewer.grid.fillToEnd",
            "Fill Down Along the Data",
            &[],
            IN_GRID,
            |ctx, _| with(ctx, |v| v.fill_to_end()),
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
                    (Some(s), Some(t)) => with(ctx, |v| v.fill_to(s, t, true)),
                    _ => with(ctx, |v| v.fill_series()),
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
