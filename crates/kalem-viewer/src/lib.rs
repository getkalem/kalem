//! The contract of a plugin that opens a file that is not text (design
//! §11.13, D54): `document-viewer`, and `document-editor` on top of it for
//! a format the plugin writes faithfully.
//!
//! These are the Rust traits the core uses (D28): a plugin implementing
//! them builds as a bundled plugin inside the binary today, and as a
//! sandboxed WebAssembly component once the WIT world of T3.1.3 carries
//! the same functions. A plugin never draws: it returns what the host
//! paints ([`Rendered`]), and it sees the file only through the
//! [`FileHandle`] the host gives it.

use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Why a viewer failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerError(pub String);

impl fmt::Display for ViewerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ViewerError {}

impl From<std::io::Error> for ViewerError {
    fn from(e: std::io::Error) -> ViewerError {
        ViewerError(e.to_string())
    }
}

/// The result of a viewer's function.
pub type Result<T> = std::result::Result<T, ViewerError>;

/// The file a viewer opened, read lazily: the host's handle, the only
/// thing of the file system the plugin sees.
#[derive(Debug, Clone)]
pub struct FileHandle {
    path: PathBuf,
}

impl FileHandle {
    /// A handle on the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> FileHandle {
        FileHandle { path: path.into() }
    }

    /// The file's name, for its extension and for labels.
    pub fn name(&self) -> &str {
        self.path.file_name().and_then(|n| n.to_str()).unwrap_or("")
    }

    /// The file's extension, in lower case.
    pub fn extension(&self) -> String {
        self.path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    }

    /// The file's path. The host uses it; a sandboxed plugin gets only
    /// the name.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's size in bytes.
    pub fn len(&self) -> Result<u64> {
        Ok(std::fs::metadata(&self.path)?.len())
    }

    /// Whether the file is empty.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Up to `len` bytes from `offset`.
    pub fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = Vec::with_capacity(len);
        f.take(len as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// The whole file.
    pub fn read_all(&self) -> Result<Vec<u8>> {
        Ok(std::fs::read(&self.path)?)
    }
}

/// How sure a viewer is that it opens a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Detection {
    /// It does not.
    No,
    /// By the extension alone.
    Extension,
    /// By the file's first bytes (its magic number).
    Magic,
}

/// What a document is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    /// A picture, the whole of an image file.
    Image,
    /// A frame of an animation: the host plays frames, it does not page
    /// through them.
    Frame,
    /// A page (PDF).
    Page,
    /// A sheet (a workbook).
    Sheet,
    /// A slide (a presentation).
    Slide,
    /// A table (a database).
    Table,
}

/// One unit of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// What it is.
    pub kind: UnitKind,
    /// Its label: a page's number as printed, a sheet's name.
    pub label: String,
    /// How long a frame shows, in milliseconds.
    pub duration_ms: Option<u32>,
}

/// An entry of a document's outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// The title.
    pub title: String,
    /// The unit it points to.
    pub unit: usize,
    /// The level, 1 at the top.
    pub level: u8,
}

/// A document's units and outline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Structure {
    /// The units, in order; at least one.
    pub units: Vec<Unit>,
    /// The outline, possibly empty.
    pub outline: Vec<OutlineEntry>,
}

impl Structure {
    /// Whether the units are frames the host plays.
    pub fn animated(&self) -> bool {
        self.units.len() > 1 && self.units.iter().all(|u| u.kind == UnitKind::Frame)
    }
}

/// The theme a unit is rendered for: a page's paper may follow it, a
/// photograph does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// A dark theme.
    pub dark: bool,
    /// The background, RGB.
    pub background: [u8; 3],
    /// The text color, RGB.
    pub foreground: [u8; 3],
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            dark: false,
            background: [255, 255, 255],
            foreground: [0, 0, 0],
        }
    }
}

/// Pixels the host paints: 8-bit RGBA, not premultiplied, rows top to
/// bottom.
#[derive(Clone, PartialEq, Eq)]
pub struct Bitmap {
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Arc<Vec<u8>>,
}

impl fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bitmap({}x{})", self.width, self.height)
    }
}

impl Bitmap {
    /// A bitmap of `rgba`, which holds `width * height * 4` bytes.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Bitmap {
        debug_assert_eq!(rgba.len(), width as usize * height as usize * 4);
        Bitmap {
            width,
            height,
            rgba: Arc::new(rgba),
        }
    }

    /// The bitmap turned clockwise by `quarters` quarter turns.
    pub fn rotated(&self, quarters: u8) -> Bitmap {
        let (w, h) = (self.width as usize, self.height as usize);
        let src = &self.rgba;
        match quarters % 4 {
            0 => self.clone(),
            2 => {
                let mut out = vec![0; src.len()];
                for (i, px) in src.as_chunks::<4>().0.iter().enumerate() {
                    let j = w * h - 1 - i;
                    out[j * 4..j * 4 + 4].copy_from_slice(px);
                }
                Bitmap::new(self.width, self.height, out)
            }
            q => {
                let mut out = vec![0; src.len()];
                for y in 0..h {
                    for x in 0..w {
                        // A clockwise turn sends (x, y) to (h - 1 - y, x);
                        // a counter-clockwise one to (y, w - 1 - x).
                        let (nx, ny) = if q == 1 {
                            (h - 1 - y, x)
                        } else {
                            (y, w - 1 - x)
                        };
                        let i = (y * w + x) * 4;
                        let j = (ny * h + nx) * 4;
                        out[j..j + 4].copy_from_slice(&src[i..i + 4]);
                    }
                }
                Bitmap::new(self.height, self.width, out)
            }
        }
    }
}

/// What a unit renders to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rendered {
    /// Pixels.
    Bitmap(Bitmap),
}

/// What the host asks a unit to be rendered at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderRequest {
    /// The scale: 1 for the unit's own size (a picture's pixels, a page at
    /// 72 dpi).
    pub scale: f32,
    /// The theme.
    pub theme: Theme,
}

impl Default for RenderRequest {
    fn default() -> RenderRequest {
        RenderRequest {
            scale: 1.0,
            theme: Theme::default(),
        }
    }
}

/// A line of a document's information: a size, a color space, a camera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoField {
    /// What it is.
    pub label: String,
    /// Its value.
    pub value: String,
}

impl InfoField {
    /// A field.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> InfoField {
        InfoField {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// An edit the format allows at a place (`document-editor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Its identifier, given back to [`ViewerDocument::apply`].
    pub id: String,
    /// Its title in menus.
    pub title: String,
    /// The edit that undoes it, when there is one: the host's undo stack
    /// applies it.
    pub inverse: Option<String>,
}

/// The bytes a document saves to, and what the format could not keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutput {
    /// The new file, written atomically by the host.
    pub bytes: Vec<u8>,
    /// What is lost by the save; empty for a faithful one.
    pub losses: Vec<String>,
}

/// A link inside a unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// Where it is, in the unit's pixels at scale 1: x, y, width, height.
    pub rect: [f32; 4],
    /// A URL or another unit (`#3`).
    pub target: String,
}

/// How a cell's text sits in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Text left, numbers right (a spreadsheet's General).
    #[default]
    General,
    /// Left.
    Left,
    /// Centered.
    Center,
    /// Right.
    Right,
}

/// A cell of a grid unit as the host draws it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GridCell {
    /// The text shown (the value through its number format).
    pub text: String,
    /// The value is a number (General alignment puts it right).
    pub numeric: bool,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strike: bool,
    /// The text color, RGB.
    pub color: Option<[u8; 3]>,
    /// The fill, RGB.
    pub fill: Option<[u8; 3]>,
    /// The alignment.
    pub align: Align,
    /// The text wraps onto more lines within the cell's width.
    pub wrap: bool,
    /// The cell holds a formula.
    pub formula: bool,
    /// The cell has a note (shown as a mark; [`ViewerDocument::cell_note`] gives it).
    pub note: bool,
}

/// The shape of a grid unit: how far it goes and how it is laid out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GridLayout {
    /// Rows that hold something (the host shows a margin past them).
    pub rows: u32,
    /// Columns that hold something.
    pub cols: u32,
    /// The widest the sheet can be scrolled: rows and columns.
    pub max_rows: u32,
    /// See [`GridLayout::max_rows`].
    pub max_cols: u32,
    /// Column widths in characters of the default font, by column; columns
    /// past the list have [`GridLayout::default_width`].
    pub widths: Vec<f32>,
    /// The default column width in characters.
    pub default_width: f32,
    /// Row heights in points, for the rows that have their own; others
    /// are [`GridLayout::default_height`] high.
    pub heights: Vec<(u32, f32)>,
    /// The default row height in points (15 in Excel's default font); 0
    /// when the format does not say.
    pub default_height: f32,
    /// Hidden rows and columns.
    pub hidden_rows: Vec<u32>,
    /// See [`GridLayout::hidden_rows`].
    pub hidden_cols: Vec<u32>,
    /// Merged ranges: first row, first column, last row, last column.
    pub merged: Vec<[u32; 4]>,
    /// Frozen rows and columns.
    pub frozen: (u32, u32),
    /// Whether cells can be edited.
    pub editable: bool,
    /// The range under a filter (a spreadsheet's AutoFilter), its first
    /// row the headers: first row, first column, last row, last column.
    pub filter: Option<[u32; 4]>,
    /// The columns whose filter hides rows.
    pub filtered: Vec<u32>,
}

/// A change of a grid's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridEdit {
    /// `count` rows inserted before row `at` (zero-based).
    InsertRows {
        /// The first new row.
        at: u32,
        /// How many.
        count: u32,
    },
    /// Rows `at..at + count` deleted.
    DeleteRows {
        /// The first deleted row.
        at: u32,
        /// How many.
        count: u32,
    },
    /// `count` columns inserted before column `at`.
    InsertCols {
        /// The first new column.
        at: u32,
        /// How many.
        count: u32,
    },
    /// Columns `at..at + count` deleted.
    DeleteCols {
        /// The first deleted column.
        at: u32,
        /// How many.
        count: u32,
    },
}

/// A macro a document carries (a workbook's VBA).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroEntry {
    /// Its name, `Module1.Name`.
    pub name: String,
    /// An event handler: listed, never run by itself.
    pub event: bool,
}

/// What a macro asks of the user: the host's dialogs. A host that cannot
/// answer while the macro runs (its prompts are not modal) returns `None`:
/// the run stops, is undone, and reports the [`MacroQuestion`]; the host
/// asks the user and runs the macro again, answering the questions asked
/// so far from what the user said.
pub trait MacroUi {
    /// A message with buttons (VBA's `MsgBox` flags); the button pressed
    /// (`1` OK, `2` Cancel, `6` Yes, `7` No).
    fn message(&mut self, prompt: &str, buttons: i64, title: &str) -> Option<i64>;
    /// A line of text, `Some(None)` when cancelled.
    fn input(&mut self, prompt: &str, title: &str, default: &str) -> Option<Option<String>>;
}

/// A question a macro asked that the host must put to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroQuestion {
    /// `MsgBox` with buttons: `buttons & 7` is VBA's set (1 OK/Cancel,
    /// 2 Abort/Retry/Ignore, 3 Yes/No/Cancel, 4 Yes/No, 5 Retry/Cancel).
    Message {
        /// The text.
        prompt: String,
        /// VBA's flags.
        buttons: i64,
        /// The title.
        title: String,
    },
    /// `InputBox`.
    Input {
        /// The text.
        prompt: String,
        /// The title.
        title: String,
        /// The text offered.
        default: String,
    },
}

/// What a macro's run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacroOutcome {
    /// `Debug.Print` lines and the messages it showed.
    pub output: Vec<String>,
    /// Statements not carried out, with their lines.
    pub skipped: Vec<String>,
    /// Why it stopped, when it did not finish.
    pub error: Option<String>,
    /// Whether it changed the document.
    pub changed: bool,
    /// The run stopped to ask this, and was undone.
    pub question: Option<MacroQuestion>,
}

/// A plugin that opens files of some formats (`document-viewer`).
pub trait Viewer: Send + Sync {
    /// The plugin's identifier, such as `image-viewer`.
    fn id(&self) -> &str;

    /// The plugin's name for people.
    fn name(&self) -> &str;

    /// The extensions it opens, in lower case, without the dot.
    fn extensions(&self) -> &[&str];

    /// Whether it opens the file named `name` that starts with `head`
    /// (its first few kilobytes).
    fn detect(&self, name: &str, head: &[u8]) -> Detection;

    /// Opens a file.
    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>>;
}

/// A file a [`Viewer`] opened.
pub trait ViewerDocument: Send {
    /// The units and the outline.
    fn structure(&self) -> Structure;

    /// A unit rendered.
    fn render(&mut self, unit: usize, request: RenderRequest) -> Result<Rendered>;

    /// A unit's size at scale 1, when the viewer knows it without
    /// rendering (a page's): the host then renders the unit at the scale
    /// it shows it at, so it stays sharp when zoomed, and fits it to the
    /// area larger than its own size. `None` for a unit whose size is its
    /// pixels' (a picture).
    fn size(&self, _unit: usize) -> Option<(f32, f32)> {
        None
    }

    /// A unit's text, for search, copy and the terminal.
    fn text(&self, unit: usize) -> String;

    /// What the information panel shows.
    fn info(&self) -> Vec<InfoField> {
        Vec::new()
    }

    /// The places `query` is found, as units and byte ranges of their
    /// [`ViewerDocument::text`].
    fn search(&self, query: &str) -> Vec<(usize, std::ops::Range<usize>)> {
        if query.is_empty() {
            return Vec::new();
        }
        let query = query.to_lowercase();
        let mut found = Vec::new();
        for unit in 0..self.structure().units.len() {
            let text = self.text(unit).to_lowercase();
            found.extend(
                text.match_indices(&query)
                    .map(|(i, m)| (unit, i..i + m.len())),
            );
        }
        found
    }

    /// The links of a unit.
    fn links(&self, _unit: usize) -> Vec<Link> {
        Vec::new()
    }

    /// Where a byte range of a unit's [`ViewerDocument::text`] stands in
    /// the unit: rectangles (x, y, width, height) in its pixels at scale
    /// 1, as [`Link::rect`], a line's run of text one rectangle; empty
    /// when the viewer does not know (the host then marks nothing).
    fn text_rects(&self, _unit: usize, _range: std::ops::Range<usize>) -> Vec<[f32; 4]> {
        Vec::new()
    }

    /// The glyph of a unit's text at (`x`, `y`) of the unit's pixels at
    /// scale 1, or the nearest one (on the point's line first): its byte
    /// range of [`ViewerDocument::text`] and its box (x, y, width,
    /// height); `None` for a unit without text, or a viewer that does not
    /// know where its text stands. The host selects text with it.
    fn text_at(
        &self,
        _unit: usize,
        _x: f32,
        _y: f32,
    ) -> Option<(std::ops::Range<usize>, [f32; 4])> {
        None
    }

    /// The edits the format allows on a unit (`document-editor`); none
    /// for a viewer only.
    fn edits(&self, _unit: usize) -> Vec<Edit> {
        Vec::new()
    }

    /// Applies an edit of [`ViewerDocument::edits`], returning the units
    /// it changed.
    fn apply(&mut self, edit: &str) -> Result<Vec<usize>> {
        Err(ViewerError(format!("No edit {edit}")))
    }

    /// Whether there are edits not saved.
    fn modified(&self) -> bool {
        false
    }

    /// The file with the edits, for the host to write; afterwards the
    /// document counts as saved.
    fn save(&mut self) -> Result<SaveOutput> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// A grid unit's shape (a sheet, a table); `None` for a unit that is
    /// not a grid, which the host renders as a bitmap.
    fn grid(&mut self, _unit: usize) -> Option<GridLayout> {
        None
    }

    /// The cells of a grid unit in `rows` × `cols` that hold something.
    fn grid_cells(
        &mut self,
        _unit: usize,
        _rows: std::ops::Range<u32>,
        _cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32, GridCell)> {
        Vec::new()
    }

    /// A cell as it is entered (`=SUM(A1:A3)`, `2026-10-03`), for editing.
    fn cell_input(&mut self, _unit: usize, _row: u32, _col: u32) -> String {
        String::new()
    }

    /// A cell's note.
    fn cell_note(&mut self, _unit: usize, _row: u32, _col: u32) -> Option<String> {
        None
    }

    /// Enters text into a cell as typed; returns the units it changed.
    fn set_cell(&mut self, _unit: usize, _row: u32, _col: u32, _input: &str) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Changes a grid's shape.
    fn grid_edit(&mut self, _unit: usize, _edit: GridEdit) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Enters rows of texts into the cells from (`row`, `col`) on, each as
    /// typed, as a spreadsheet's Paste. By default cell by cell through
    /// [`ViewerDocument::set_cell`]; a format with its own history makes it
    /// one step.
    fn set_cells(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        values: &[Vec<String>],
    ) -> Result<Vec<usize>> {
        let mut changed = Vec::new();
        for (i, line) in values.iter().enumerate() {
            for (j, v) in line.iter().enumerate() {
                for u in self.set_cell(unit, row + i as u32, col + j as u32, v)? {
                    if !changed.contains(&u) {
                        changed.push(u);
                    }
                }
            }
        }
        Ok(changed)
    }

    /// Sorts a range's rows by column `key` (a spreadsheet's Sort), the
    /// first row kept in place when `header`.
    fn sort_range(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _key: u32,
        _descending: bool,
        _header: bool,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Puts a filter on a range (its first row the headers), or takes the
    /// filter off with `None`, its hidden rows shown again.
    fn set_filter(&mut self, _unit: usize, _range: Option<[u32; 4]>) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Filters column `col` of the filter to rows showing one of `values`
    /// (as shown; an empty text for empty cells), or clears its filter with
    /// `None`.
    fn filter_column(
        &mut self,
        _unit: usize,
        _col: u32,
        _values: Option<Vec<String>>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Moves a range's cells (first row, first column, last row, last
    /// column) to start at (`row`, `col`), as a spreadsheet's Cut and
    /// Paste: the cells go, the range they leave is empty. By default their
    /// entries are copied with [`ViewerDocument::set_cells`] and the cells
    /// left cleared; a spreadsheet also moves formats and points the
    /// formulas that referred to the cells at their new place.
    fn move_cells(
        &mut self,
        unit: usize,
        range: [u32; 4],
        row: u32,
        col: u32,
    ) -> Result<Vec<usize>> {
        let values: Vec<Vec<String>> = (range[0]..=range[2])
            .map(|r| {
                (range[1]..=range[3])
                    .map(|c| self.cell_input(unit, r, c))
                    .collect()
            })
            .collect();
        let mut changed = self.clear_cells(unit, range)?;
        for u in self.set_cells(unit, row, col, &values)? {
            if !changed.contains(&u) {
                changed.push(u);
            }
        }
        Ok(changed)
    }

    /// Moves a range's cells from unit `from` to start at (`row`, `col`)
    /// of unit `to`, as Cut on one sheet and Paste on another. By default
    /// [`ViewerDocument::move_cells`] on one unit, and across units the
    /// entries copied and the cells left cleared.
    fn move_cells_between(
        &mut self,
        from: usize,
        range: [u32; 4],
        to: usize,
        row: u32,
        col: u32,
    ) -> Result<Vec<usize>> {
        if from == to {
            return self.move_cells(from, range, row, col);
        }
        let values: Vec<Vec<String>> = (range[0]..=range[2])
            .map(|r| {
                (range[1]..=range[3])
                    .map(|c| self.cell_input(from, r, c))
                    .collect()
            })
            .collect();
        let mut changed = self.set_cells(to, row, col, &values)?;
        for u in self.clear_cells(from, range)? {
            if !changed.contains(&u) {
                changed.push(u);
            }
        }
        Ok(changed)
    }

    /// Clears the values of a range (first row, first column, last row,
    /// last column), their formats kept, as a spreadsheet's Delete. By
    /// default cell by cell through [`ViewerDocument::set_cell`]; a format
    /// with its own history makes it one step.
    fn clear_cells(&mut self, unit: usize, range: [u32; 4]) -> Result<Vec<usize>> {
        let filled: Vec<(u32, u32)> = self
            .grid_cells(unit, range[0]..range[2] + 1, range[1]..range[3] + 1)
            .into_iter()
            .filter(|(_, _, c)| !c.text.is_empty())
            .map(|(r, c, _)| (r, c))
            .collect();
        let mut changed = Vec::new();
        for (r, c) in filled {
            for u in self.set_cell(unit, r, c, "")? {
                if !changed.contains(&u) {
                    changed.push(u);
                }
            }
        }
        Ok(changed)
    }

    /// Merges a range (first row, first column, last row, last column)
    /// into one cell, as a spreadsheet's Merge Cells: only the first cell's
    /// value stays; `center` centers it (Merge & Center).
    fn merge_cells(&mut self, _unit: usize, _range: [u32; 4], _center: bool) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Splits the merged range holding a cell back into cells.
    fn unmerge_cells(&mut self, _unit: usize, _row: u32, _col: u32) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Turns wrapping of a cell's text on or off (a style of the cell,
    /// as a spreadsheet's Wrap Text).
    fn set_wrap(&mut self, _unit: usize, _row: u32, _col: u32, _wrap: bool) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets a row's height in points, as the user's drag does.
    fn set_row_height(&mut self, _unit: usize, _row: u32, _height: f32) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets a column's width, in characters of the default font's digit
    /// (a spreadsheet's unit), as the user's autofit or drag does.
    fn set_col_width(&mut self, _unit: usize, _col: u32, _width: f32) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Whether the document keeps its own undo history for
    /// [`ViewerDocument::set_cell`], [`ViewerDocument::grid_edit`] and
    /// macros; the host then undoes through [`ViewerDocument::undo`].
    fn has_history(&self) -> bool {
        false
    }

    /// Undoes the document's last change; false when there is none.
    fn undo(&mut self) -> Result<bool> {
        Ok(false)
    }

    /// Redoes the last change undone; false when there is none.
    fn redo(&mut self) -> Result<bool> {
        Ok(false)
    }

    /// The macros the document carries.
    fn macros(&mut self) -> Vec<MacroEntry> {
        Vec::new()
    }

    /// Runs a macro by name, only ever on the user's command.
    fn run_macro(&mut self, name: &str, _ui: &mut dyn MacroUi) -> Result<MacroOutcome> {
        Err(ViewerError(format!("No macro {name}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(w: u32, h: u32) -> Bitmap {
        let rgba = (0..w * h).flat_map(|i| [i as u8, 0, 0, 255]).collect();
        Bitmap::new(w, h, rgba)
    }

    fn red(b: &Bitmap) -> Vec<u8> {
        b.rgba.as_chunks::<4>().0.iter().map(|p| p[0]).collect()
    }

    #[test]
    fn rotations() {
        // 0 1 2
        // 3 4 5
        let b = numbered(3, 2);
        let cw = b.rotated(1);
        assert_eq!((cw.width, cw.height), (2, 3));
        assert_eq!(red(&cw), [3, 0, 4, 1, 5, 2]);
        assert_eq!(red(&b.rotated(2)), [5, 4, 3, 2, 1, 0]);
        assert_eq!(red(&b.rotated(3)), [2, 5, 1, 4, 0, 3]);
        assert_eq!(b.rotated(1).rotated(3), b);
    }

    #[test]
    fn the_handle_reads_lazily() {
        let dir = std::env::temp_dir().join(format!("kalem-viewer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.BIN");
        std::fs::write(&p, b"0123456789").unwrap();
        let h = FileHandle::new(&p);
        assert_eq!(h.extension(), "bin");
        assert_eq!(h.len().unwrap(), 10);
        assert_eq!(h.read_at(3, 4).unwrap(), b"3456");
        assert_eq!(h.read_at(8, 10).unwrap(), b"89");
        std::fs::remove_dir_all(dir).ok();
    }
}
