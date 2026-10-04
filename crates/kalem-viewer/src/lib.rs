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
/// thing of the file system the plugin sees. On the disk in a bundled
/// plugin; read through the host's `file` resource in a component, where
/// there is no file system ([`FileHandle::from_reader`]).
#[derive(Clone)]
pub struct FileHandle {
    path: PathBuf,
    source: Source,
}

/// Reads up to a length of bytes from an offset.
type ReadFn = Arc<dyn Fn(u64, usize) -> Vec<u8> + Send + Sync>;

#[derive(Clone)]
enum Source {
    Disk,
    Reader { len: u64, read: ReadFn },
}

impl fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileHandle")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl FileHandle {
    /// A handle on the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> FileHandle {
        FileHandle {
            path: path.into(),
            source: Source::Disk,
        }
    }

    /// A handle on the file called `name`, of `len` bytes, read by `read`
    /// (offset and length to bytes): what a component gets from the host.
    pub fn from_reader(
        name: impl Into<String>,
        len: u64,
        read: impl Fn(u64, usize) -> Vec<u8> + Send + Sync + 'static,
    ) -> FileHandle {
        FileHandle {
            path: PathBuf::from(name.into()),
            source: Source::Reader {
                len,
                read: Arc::new(read),
            },
        }
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
        match &self.source {
            Source::Disk => Ok(std::fs::metadata(&self.path)?.len()),
            Source::Reader { len, .. } => Ok(*len),
        }
    }

    /// Whether the file is empty.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Up to `len` bytes from `offset`.
    pub fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        match &self.source {
            Source::Disk => {
                let mut f = std::fs::File::open(&self.path)?;
                f.seek(SeekFrom::Start(offset))?;
                let mut buf = Vec::with_capacity(len);
                f.take(len as u64).read_to_end(&mut buf)?;
                Ok(buf)
            }
            Source::Reader { read, .. } => Ok(read(offset, len)),
        }
    }

    /// The whole file.
    pub fn read_all(&self) -> Result<Vec<u8>> {
        match &self.source {
            Source::Disk => Ok(std::fs::read(&self.path)?),
            Source::Reader { len, read } => {
                // In pieces: a component's host gives at most what fits a
                // call comfortably.
                const PIECE: usize = 4 << 20;
                let mut out = Vec::with_capacity(*len as usize);
                while (out.len() as u64) < *len {
                    let piece = read(out.len() as u64, PIECE);
                    if piece.is_empty() {
                        break;
                    }
                    out.extend_from_slice(&piece);
                }
                Ok(out)
            }
        }
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

    /// The pixels as BGRA, the order GPUs and gpui take them. Here rather
    /// than in a frontend so that debug builds, which build this crate
    /// optimized, convert a page of tens of megabytes in milliseconds.
    pub fn bgra(&self) -> Vec<u8> {
        let mut out = self.rgba.to_vec();
        for p in out.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        out
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
    /// A data bar from a conditional format: the part of the cell's width
    /// it fills, in thousandths, and its color.
    pub bar: Option<(u16, [u8; 3])>,
    /// An icon from a conditional format's icon set: the glyph and its color.
    pub icon: Option<(String, [u8; 3])>,
    /// The font size in tenths of a point, when not the sheet's default.
    pub font_size: Option<u16>,
    /// The typeface, when not the sheet's default.
    pub face: Option<String>,
    /// The vertical alignment.
    pub valign: VAlign,
    /// The borders: top, right, bottom, left, each its color when drawn.
    pub borders: [Option<[u8; 3]>; 4],
    /// Which of the borders are thick (medium or thicker), in the same
    /// order.
    pub border_thick: [bool; 4],
}

/// How a cell's text sits between its top and bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    /// At the bottom (a spreadsheet's default).
    #[default]
    Bottom,
    /// In the middle.
    Middle,
    /// At the top.
    Top,
}

/// One level of a sort: a column, its order, and a custom list whose
/// order its values follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    /// The column (of the sheet).
    pub col: u32,
    /// Largest or last first.
    pub descending: bool,
    /// The values in the order they sort in (months, a user's list); the
    /// others after them.
    pub list: Option<Vec<String>>,
}

/// How a filter's custom condition compares a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    /// Equals (`*` and `?` as wildcards in text).
    Equal,
    /// Does not equal.
    NotEqual,
    /// Greater than.
    Greater,
    /// Greater than or equal to.
    GreaterOrEqual,
    /// Less than.
    Less,
    /// Less than or equal to.
    LessOrEqual,
    /// Begins with.
    BeginsWith,
    /// Ends with.
    EndsWith,
    /// Contains.
    Contains,
    /// Does not contain.
    NotContains,
}

/// What a filter column lets show.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterRule {
    /// The rows showing one of these values (an empty text for empty
    /// cells).
    Values(Vec<String>),
    /// One condition, or two joined with and (`true`) or or.
    Custom {
        /// The first condition.
        first: (FilterOp, String),
        /// The second, and whether both must hold.
        second: Option<(bool, FilterOp, String)>,
    },
    /// The largest (or with `bottom` the smallest) items or percent.
    Top {
        /// How many, or what percent.
        count: u32,
        /// `count` is a percent.
        percent: bool,
        /// The smallest instead.
        bottom: bool,
    },
    /// Above (or below) the column's average.
    Average {
        /// Above it.
        above: bool,
    },
    /// The cells filled with this color.
    Fill([u8; 3]),
}

/// A table of a grid (a spreadsheet's Format as Table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInfo {
    /// Its name, as formulas name it (`Table1[Amount]`).
    pub name: String,
    /// Its cells: first row, first column, last row, last column, the
    /// header row and a total row included.
    pub range: [u32; 4],
    /// It has a total row.
    pub totals: bool,
    /// Its style's name (`TableStyleMedium2`).
    pub style: String,
}

/// How a sheet prints (a spreadsheet's Page Layout).
#[derive(Debug, Clone, PartialEq)]
pub struct PageSetup {
    /// Landscape rather than portrait.
    pub landscape: bool,
    /// The paper, as a spreadsheet's code: 1 Letter, 5 Legal, 8 A3, 9 A4.
    pub paper: u32,
    /// Margins in inches: left, right, top, bottom.
    pub margins: [f32; 4],
    /// Scaled to fit one page wide.
    pub fit_width: bool,
    /// What prints (first row, first column, last row, last column);
    /// `None` the used range.
    pub print_area: Option<[u32; 4]>,
    /// Rows repeated at the top of every page (first and last).
    pub title_rows: Option<(u32, u32)>,
    /// The header, in a spreadsheet's codes (`&C&A` the sheet's name in
    /// the middle, `&P` the page, `&N` the pages, `&D` the date, `&F` the
    /// file, `&L` and `&R` the sides).
    pub header: String,
    /// The footer, likewise.
    pub footer: String,
    /// The rows a new page begins at.
    pub row_breaks: Vec<u32>,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup {
            landscape: false,
            paper: 9,
            margins: [0.7, 0.7, 0.75, 0.75],
            fit_width: false,
            print_area: None,
            title_rows: None,
            header: String::new(),
            footer: String::new(),
            row_breaks: Vec::new(),
        }
    }
}

/// What Paste Special takes of the cells copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteKind {
    /// Everything: formulas (moved) or values, and formats.
    All,
    /// The values as they are now, the cells' formats kept.
    Values,
    /// The formats only.
    Formats,
    /// The formulas (moved) and constants, the cells' formats kept.
    Formulas,
}

/// A change to a workbook's sheets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetEdit {
    /// A new empty sheet put at this place, named as a spreadsheet names
    /// a new one.
    Insert(usize),
    /// The sheet removed.
    Delete(usize),
    /// The sheet given a new name, references to it following.
    Rename(usize, String),
    /// The sheet moved from one place to another.
    Move(usize, usize),
    /// The sheet hidden (`true`) or shown again.
    Hide(usize, bool),
}

/// Which borders a change draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderSet {
    /// Every cell's every side.
    All,
    /// Around the selection.
    Outside,
    /// Around the selection, thick.
    ThickOutside,
    /// Under the selection.
    Bottom,
    /// Over the selection.
    Top,
    /// Left of the selection.
    Left,
    /// Right of the selection.
    Right,
    /// None at all.
    None,
}

/// A change to the cells' format, as a spreadsheet's Format Cells makes
/// it; every field left `None` stays as each cell has it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyleChange {
    /// Bold on or off.
    pub bold: Option<bool>,
    /// Italic on or off.
    pub italic: Option<bool>,
    /// Underline on or off.
    pub underline: Option<bool>,
    /// Strikethrough on or off.
    pub strike: Option<bool>,
    /// The text color (`Some(None)`: automatic).
    pub color: Option<Option<[u8; 3]>>,
    /// The fill (`Some(None)`: no fill).
    pub fill: Option<Option<[u8; 3]>>,
    /// The font size in points.
    pub size: Option<f32>,
    /// The typeface.
    pub face: Option<String>,
    /// The horizontal alignment.
    pub align: Option<Align>,
    /// The vertical alignment.
    pub valign: Option<VAlign>,
    /// Borders drawn (or taken away), in a color (`None`: automatic).
    pub borders: Option<(BorderSet, Option<[u8; 3]>)>,
    /// The number format code (`General`, `#,##0.00`, `0%`).
    pub number_format: Option<String>,
}

/// How a conditional format compares a cell's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Greater than.
    Greater,
    /// Less than.
    Less,
    /// Greater than or equal to.
    GreaterOrEqual,
    /// Less than or equal to.
    LessOrEqual,
    /// Equal to.
    Equal,
    /// Not equal to.
    NotEqual,
    /// Between two values, both included.
    Between,
    /// Outside two values.
    NotBetween,
}

/// What kind of chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartKind {
    /// Vertical bars.
    #[default]
    Column,
    /// Horizontal bars.
    Bar,
    /// Lines through the points.
    Line,
    /// Lines with the area under them filled.
    Area,
    /// Slices of a circle.
    Pie,
    /// Slices of a ring.
    Doughnut,
    /// Points at their x and y.
    Scatter,
    /// A kind drawn as a placeholder (radar, stock, surface, bubble…).
    Other,
}

/// One series of a chart.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartSeries {
    /// Its name, for the legend.
    pub name: String,
    /// Its values; `None` where a cell holds no number.
    pub values: Vec<Option<f64>>,
    /// A scatter chart's x values.
    pub x: Vec<Option<f64>>,
    /// Its color, when the file gives one.
    pub color: Option<[u8; 3]>,
    /// Points with colors of their own (a pie's slices), by point.
    pub point_colors: Vec<(usize, [u8; 3])>,
    /// How far a pie's slices stand out from its center, in percent of
    /// its radius: every slice's.
    pub explosion: u32,
    /// Slices standing out on their own, by point.
    pub point_explosions: Vec<(usize, u32)>,
}

/// A chart on a sheet, as it reads now: values from the cells it names.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chart {
    /// What kind.
    pub kind: ChartKind,
    /// Its title.
    pub title: Option<String>,
    /// The categories (the labels along the axis, or of the slices).
    pub categories: Vec<String>,
    /// The series.
    pub series: Vec<ChartSeries>,
    /// Where it stands: first row, first column, last row, last column
    /// of the cells it covers.
    pub anchor: [u32; 4],
    /// Bars or areas stacked.
    pub stacked: bool,
    /// The horizontal axis's title.
    pub horizontal_title: Option<String>,
    /// The vertical axis's title.
    pub vertical_title: Option<String>,
    /// Where its legend is; `None` for no legend.
    pub legend: Option<LegendPosition>,
    /// What its data labels show.
    pub labels: DataLabels,
    /// Its value axis's scale.
    pub scale: AxisScale,
    /// The chart area's background.
    pub background: Paint,
    /// The chart area's border.
    pub border: Paint,
    /// The plot area's background (the part inside the axes).
    pub plot_background: Paint,
    /// The plot area's border.
    pub plot_border: Paint,
    /// Which gridlines it draws.
    pub gridlines: Gridlines,
    /// The value axis's number format (`#,##0.00`, `0%`); `None` for the
    /// cells' own.
    pub axis_format: Option<String>,
    /// The horizontal axis's labels' font.
    pub horizontal_font: AxisFont,
    /// The vertical axis's labels' font.
    pub vertical_font: AxisFont,
    /// The title's font.
    pub title_font: AxisFont,
    /// The legend's font.
    pub legend_font: AxisFont,
}

/// The font of an axis's labels; all unset, the spreadsheet's style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AxisFont {
    /// Its size in points.
    pub size: Option<f32>,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Its color.
    pub color: Option<[u8; 3]>,
    /// Its typeface (`Calibri`, `Arial`).
    pub face: Option<String>,
}

impl AxisFont {
    /// Whether nothing is set.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A chart's gridlines, by the way they run: horizontal ones come from
/// the vertical axis, vertical ones from the horizontal axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gridlines {
    /// Horizontal lines at each major unit.
    pub horizontal_major: bool,
    /// Horizontal lines between them.
    pub horizontal_minor: bool,
    /// Vertical lines at each major unit (or category).
    pub vertical_major: bool,
    /// Vertical lines between them.
    pub vertical_minor: bool,
}

/// How a chart's background or border is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paint {
    /// As the spreadsheet's style has it.
    #[default]
    Automatic,
    /// Not at all: no fill, no line.
    None,
    /// In a color.
    Color([u8; 3]),
}

/// A value axis's scale; `None` where the spreadsheet chooses.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AxisScale {
    /// The lowest value shown.
    pub min: Option<f64>,
    /// The highest value shown.
    pub max: Option<f64>,
    /// The step between gridlines.
    pub major: Option<f64>,
    /// A base-10 logarithmic scale.
    pub log: bool,
}

/// What a chart's data labels show at each point; all off, no labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DataLabels {
    /// The value.
    pub value: bool,
    /// The category's name.
    pub category: bool,
    /// The series' name.
    pub series: bool,
    /// A slice's share of the whole (pie and doughnut charts).
    pub percent: bool,
}

impl DataLabels {
    /// Whether any label shows.
    pub fn any(&self) -> bool {
        self.value || self.category || self.series || self.percent
    }
}

/// Where a chart's legend stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendPosition {
    /// Under the plot.
    Bottom,
    /// Above the plot.
    Top,
    /// Left of the plot.
    Left,
    /// Right of the plot.
    Right,
    /// In the top right corner.
    TopRight,
}

/// One of a chart's axes, by where it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartAxis {
    /// The axis along the bottom (categories of a column chart, values of
    /// a bar chart, x of a scatter chart).
    Horizontal,
    /// The axis along the side.
    Vertical,
}

/// How a pivot table summarizes a value field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Aggregate {
    /// The numbers' sum.
    #[default]
    Sum,
    /// How many values.
    Count,
    /// The numbers' average.
    Average,
    /// The largest number.
    Max,
    /// The smallest number.
    Min,
}

/// A pivot table to insert, as a spreadsheet's PivotTable dialog makes it:
/// fields are columns of the source range, counted from its first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PivotSpec {
    /// The source: first row, first column, last row, last column; its
    /// first row holds the fields' names.
    pub range: [u32; 4],
    /// The row fields, outermost first.
    pub rows: Vec<u32>,
    /// The column fields.
    pub cols: Vec<u32>,
    /// The value fields and how each is summarized.
    pub values: Vec<(u32, Aggregate)>,
}

/// What a cell's data validation allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidationKind {
    /// Any value: only an input message.
    #[default]
    Any,
    /// Whole numbers.
    Whole,
    /// Numbers.
    Decimal,
    /// One of a list's values.
    List,
    /// Dates.
    Date,
    /// Times.
    Time,
    /// Text of a length.
    TextLength,
    /// What a formula accepts.
    Custom,
}

/// What a spreadsheet does with a value its validation refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorStyle {
    /// Refuses it.
    #[default]
    Stop,
    /// Asks whether to keep it.
    Warning,
    /// Keeps it and says so.
    Information,
}

/// A data validation, as a spreadsheet's Data Validation dialog sets it.
#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    /// What is allowed.
    pub kind: ValidationKind,
    /// How a value compares with `value` (and `value2`); not for lists and
    /// formulas.
    pub op: CompareOp,
    /// As typed: `10`, `=A1`, a date; a list's values separated by commas
    /// or `=` and a range; a formula for Custom.
    pub value: String,
    /// The second value, for Between and Not Between.
    pub value2: Option<String>,
    /// An empty cell is accepted.
    pub allow_blank: bool,
    /// A list offers its values to choose from.
    pub dropdown: bool,
    /// Shown while the cell is selected: title, text.
    pub prompt: Option<(String, String)>,
    /// Said when a value is refused: style, title, text; `None` for no
    /// alert, the value kept.
    pub error: Option<(ErrorStyle, String, String)>,
    /// A list's values as they read now (read only).
    pub list: Vec<String>,
}

impl Default for Validation {
    fn default() -> Self {
        Self {
            kind: ValidationKind::Any,
            op: CompareOp::Between,
            value: String::new(),
            value2: None,
            allow_blank: true,
            dropdown: true,
            prompt: None,
            error: Some((ErrorStyle::Stop, String::new(), String::new())),
            list: Vec::new(),
        }
    }
}

/// A value a validation refused, and what to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Refuse, ask or tell.
    pub style: ErrorStyle,
    /// The alert's title.
    pub title: String,
    /// Its text.
    pub message: String,
}

/// A conditional format's rule, as a spreadsheet's Conditional Formatting
/// menu offers them.
#[derive(Debug, Clone, PartialEq)]
pub enum CondRule {
    /// The cell's value compared with one value, or two for Between; the
    /// values as typed (`100`, `=A1`, `abc`).
    Compare {
        /// The comparison.
        op: CompareOp,
        /// The first value.
        value: String,
        /// The second value, for Between and Not Between.
        value2: Option<String>,
    },
    /// The cell's text contains this.
    TextContains(String),
    /// Values found more than once in the range.
    Duplicates,
    /// Values found once in the range.
    Unique,
    /// The top (or bottom) `count` values, or percent of them.
    Top {
        /// How many.
        count: u32,
        /// The bottom ones.
        bottom: bool,
        /// `count` is a percentage.
        percent: bool,
    },
    /// Above (or below) the range's average.
    Average {
        /// Below it.
        below: bool,
    },
    /// A formula true for the cell, written for the range's first cell.
    Formula(String),
    /// A color scale from the lowest to the highest value: two or three colors.
    ColorScale(Vec<[u8; 3]>),
    /// A data bar in a color.
    DataBar([u8; 3]),
    /// An icon set by its name in the file (`3Arrows`, `3TrafficLights1`).
    IconSet(String),
}

/// The format a highlighting rule applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CondStyle {
    /// The fill.
    pub fill: Option<[u8; 3]>,
    /// The text color.
    pub color: Option<[u8; 3]>,
    /// Bold.
    pub bold: bool,
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

    /// Freezes the first `rows` rows and `cols` columns of a grid, as a
    /// spreadsheet's Freeze Panes; none of either unfreezes it.
    fn set_frozen(&mut self, _unit: usize, _rows: u32, _cols: u32) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Hides rows (`rows`) or columns `from..=to`, or shows them again.
    fn set_hidden(
        &mut self,
        _unit: usize,
        _rows: bool,
        _from: u32,
        _to: u32,
        _hidden: bool,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Changes the workbook's sheets; the unit to show after it.
    fn edit_sheets(&mut self, _edit: SheetEdit) -> Result<usize> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// The units that are hidden sheets.
    fn hidden_units(&mut self) -> Vec<usize> {
        Vec::new()
    }

    /// The numbers among a range's values (first row, first column, last
    /// row, last column), and how many of its cells hold a value at all:
    /// what a spreadsheet's status bar sums and counts.
    fn range_numbers(&mut self, _unit: usize, _range: [u32; 4]) -> (Vec<f64>, usize) {
        (Vec::new(), 0)
    }

    /// A cell's number format code (`General`, `#,##0.00`, `0%`).
    fn cell_format(&mut self, _unit: usize, _row: u32, _col: u32) -> Option<String> {
        None
    }

    /// A cell's note.
    fn cell_note(&mut self, _unit: usize, _row: u32, _col: u32) -> Option<String> {
        None
    }

    /// The functions formulas can use: each name, and its arguments as a
    /// spreadsheet writes them (`number1, [number2], ...`) when known.
    fn formula_functions(&mut self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Pastes cells copied in the document (unit, first row, first
    /// column, last row, last column) to `to` (unit, row, column), as a
    /// spreadsheet's Paste Special: what of them, and turned rows into
    /// columns (`transpose`).
    fn paste_cells(
        &mut self,
        _from: (usize, [u32; 4]),
        _to: (usize, u32, u32),
        _kind: PasteKind,
        _transpose: bool,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Clears a range's contents (values and formulas), formats, or both
    /// with its notes (Clear All), as one step.
    fn clear_range(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _contents: bool,
        _formats: bool,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Gives the cells of `to` (unit, range) the formats of `from`,
    /// repeated over it as a spreadsheet's Format Painter does.
    fn fill_formats(
        &mut self,
        _from: (usize, [u32; 4]),
        _to: (usize, [u32; 4]),
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Removes the rows of a range (first row, first column, last row,
    /// last column) that repeat an earlier row in `columns`, as a
    /// spreadsheet's Remove Duplicates: the rows after them move up within
    /// the range; its first row is left as headers when `header`. How many
    /// rows went.
    fn remove_duplicates(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _columns: &[u32],
        _header: bool,
    ) -> Result<usize> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// A cell's hyperlink: a web or mail address or a file as written, or
    /// a place in the document after `#` (`#Sheet2!A1`).
    fn cell_link(&mut self, _unit: usize, _row: u32, _col: u32) -> Option<String> {
        None
    }

    /// Gives a cell a hyperlink (as [`ViewerDocument::cell_link`] reads
    /// it), or takes it away (`None`).
    fn set_link(
        &mut self,
        _unit: usize,
        _row: u32,
        _col: u32,
        _target: Option<String>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// The document's defined names a user sees, with what each refers
    /// to as formula text (`Budget!$B$2:$D$4`).
    fn defined_names(&mut self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Defines a name as referring to `refers_to` (formula text), or
    /// deletes it (`None`).
    fn set_defined_name(&mut self, _name: &str, _refers_to: Option<&str>) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Computes every formula again (a spreadsheet's Calculate Now).
    fn recalculate(&mut self) -> Result<Vec<usize>> {
        Ok(Vec::new())
    }

    /// Enters `input` as typed into every cell of a range (first row,
    /// first column, last row, last column), as a spreadsheet's
    /// Ctrl+Enter: a formula's references moved for each cell as from
    /// `at` (row, column), where it was typed. One step.
    fn enter_in_range(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _at: (u32, u32),
        _input: &str,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// How a unit prints.
    fn page_setup(&mut self, _unit: usize) -> Option<PageSetup> {
        None
    }

    /// Sets how a unit prints.
    fn set_page_setup(&mut self, _unit: usize, _setup: &PageSetup) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// The tables of a unit.
    fn tables(&mut self, _unit: usize) -> Vec<TableInfo> {
        Vec::new()
    }

    /// Makes a range a table (Format as Table) in a style, its first row
    /// the headers (else a header row is put over it); the table's name.
    fn create_table(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _header: bool,
        _style: &str,
    ) -> Result<String> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Gives a table a total row, or takes it away.
    fn set_table_totals(&mut self, _unit: usize, _name: &str, _on: bool) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Turns a table back into a range of cells (Convert to Range).
    fn remove_table(&mut self, _unit: usize, _name: &str) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Gives a cell a note, or takes it away (`None`).
    fn set_note(
        &mut self,
        _unit: usize,
        _row: u32,
        _col: u32,
        _text: Option<String>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Enters text into a cell as typed; returns the units it changed.
    fn set_cell(&mut self, _unit: usize, _row: u32, _col: u32, _input: &str) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Changes a grid's shape.
    fn grid_edit(&mut self, _unit: usize, _edit: GridEdit) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Enters texts into scattered cells (row, column, text), each as
    /// typed. By default cell by cell through [`ViewerDocument::set_cell`];
    /// a format with its own history makes it one step.
    fn set_cell_list(&mut self, unit: usize, cells: &[(u32, u32, String)]) -> Result<Vec<usize>> {
        let mut changed = Vec::new();
        for (row, col, v) in cells {
            for u in self.set_cell(unit, *row, *col, v)? {
                if !changed.contains(&u) {
                    changed.push(u);
                }
            }
        }
        Ok(changed)
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

    /// Adds a conditional format on a range, first in priority.
    fn add_conditional_format(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _rule: CondRule,
        _style: CondStyle,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Removes the conditional formats of a range (`None`: of the whole unit).
    fn clear_conditional_formats(
        &mut self,
        _unit: usize,
        _range: Option<[u32; 4]>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// The charts on a unit.
    fn charts(&mut self, _unit: usize) -> Vec<Chart> {
        Vec::new()
    }

    /// Inserts a chart of a range (its first row or column naming the
    /// series and categories, as a spreadsheet reads it) beside it.
    fn insert_chart(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _kind: ChartKind,
        _title: Option<String>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Moves or resizes the chart at `index` of [`ViewerDocument::charts`]
    /// to cover the cells of `anchor` (first row, first column, last row,
    /// last column).
    fn move_chart(&mut self, _unit: usize, _index: usize, _anchor: [u32; 4]) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Gives the chart at `index` of [`ViewerDocument::charts`] a title, or
    /// takes its title away (`None`).
    fn set_chart_title(
        &mut self,
        _unit: usize,
        _index: usize,
        _title: Option<String>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Gives an axis of the chart at `index` of [`ViewerDocument::charts`]
    /// a title, or takes it away (`None`).
    fn set_axis_title(
        &mut self,
        _unit: usize,
        _index: usize,
        _axis: ChartAxis,
        _title: Option<String>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Puts the legend of the chart at `index` of [`ViewerDocument::charts`]
    /// at `position`, or takes it away (`None`).
    fn set_legend(
        &mut self,
        _unit: usize,
        _index: usize,
        _position: Option<LegendPosition>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets what the data labels of the chart at `index` of
    /// [`ViewerDocument::charts`] show, every series alike.
    fn set_data_labels(
        &mut self,
        _unit: usize,
        _index: usize,
        _labels: DataLabels,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the value axis's scale of the chart at `index` of
    /// [`ViewerDocument::charts`].
    fn set_axis_scale(
        &mut self,
        _unit: usize,
        _index: usize,
        _scale: AxisScale,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Changes the kind of the chart at `index` of
    /// [`ViewerDocument::charts`], its series and settings kept.
    fn set_chart_kind(
        &mut self,
        _unit: usize,
        _index: usize,
        _kind: ChartKind,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Gives series `series` of the chart at `index` of
    /// [`ViewerDocument::charts`] a color, or the theme's again (`None`).
    fn set_series_color(
        &mut self,
        _unit: usize,
        _index: usize,
        _series: usize,
        _color: Option<[u8; 3]>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Gives point `point` of series `series` (a pie's slice) of the chart
    /// at `index` of [`ViewerDocument::charts`] a color of its own, or its
    /// series' again (`None`).
    fn set_point_color(
        &mut self,
        _unit: usize,
        _index: usize,
        _series: usize,
        _point: usize,
        _color: Option<[u8; 3]>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Pulls a slice of a pie (series `series` of the chart at `index` of
    /// [`ViewerDocument::charts`]) out of it by `percent` of its radius,
    /// or every slice (`point` `None`); 0 puts it back.
    fn set_explosion(
        &mut self,
        _unit: usize,
        _index: usize,
        _series: usize,
        _point: Option<usize>,
        _percent: u32,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the background and the border of the chart area of the chart
    /// at `index` of [`ViewerDocument::charts`].
    fn set_chart_area(
        &mut self,
        _unit: usize,
        _index: usize,
        _background: Paint,
        _border: Paint,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the background and the border of the plot area (inside the
    /// axes) of the chart at `index` of [`ViewerDocument::charts`].
    fn set_plot_area(
        &mut self,
        _unit: usize,
        _index: usize,
        _background: Paint,
        _border: Paint,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Shows or hides the gridlines of the chart at `index` of
    /// [`ViewerDocument::charts`].
    fn set_gridlines(
        &mut self,
        _unit: usize,
        _index: usize,
        _lines: Gridlines,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the number format of the value axis's labels of the chart at
    /// `index` of [`ViewerDocument::charts`]; `None` takes the cells' own.
    fn set_axis_format(
        &mut self,
        _unit: usize,
        _index: usize,
        _format: Option<String>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the font of an axis's labels of the chart at `index` of
    /// [`ViewerDocument::charts`]; the default font takes the style's.
    fn set_axis_font(
        &mut self,
        _unit: usize,
        _index: usize,
        _axis: ChartAxis,
        _font: AxisFont,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the font of the title of the chart at `index` of
    /// [`ViewerDocument::charts`]; the default font takes the style's.
    fn set_title_font(
        &mut self,
        _unit: usize,
        _index: usize,
        _font: AxisFont,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the font of the legend of the chart at `index` of
    /// [`ViewerDocument::charts`]; the default font takes the style's.
    fn set_legend_font(
        &mut self,
        _unit: usize,
        _index: usize,
        _font: AxisFont,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Removes the chart at `index` of [`ViewerDocument::charts`].
    fn delete_chart(&mut self, _unit: usize, _index: usize) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Inserts a pivot table of a range of `unit` on a new unit; the new
    /// unit's index.
    fn insert_pivot(&mut self, _unit: usize, _spec: PivotSpec) -> Result<usize> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Computes every pivot table again from its source (Refresh All); the
    /// units that changed.
    fn refresh_pivots(&mut self) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// The data validation of a cell.
    fn validation(&mut self, _unit: usize, _row: u32, _col: u32) -> Option<Validation> {
        None
    }

    /// Changes the format of a range's cells (first row, first column,
    /// last row, last column).
    fn change_style(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _change: StyleChange,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// Sets the data validation of a range (`None`: removes it), replacing
    /// what its cells had.
    fn set_validation(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        _validation: Option<Validation>,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
    }

    /// What the cell's validation says of a value about to be entered, as
    /// typed; `None` when it is accepted.
    fn check_input(
        &mut self,
        _unit: usize,
        _row: u32,
        _col: u32,
        _input: &str,
    ) -> Option<ValidationError> {
        None
    }

    /// The cells in `rows` × `cols` whose values their validation refuses
    /// (a spreadsheet's Circle Invalid Data).
    fn invalid_cells(
        &mut self,
        _unit: usize,
        _rows: std::ops::Range<u32>,
        _cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32)> {
        Vec::new()
    }

    /// The user's own lists a fill goes round (a spreadsheet's Custom
    /// Lists: `North, South, East, West`), beside the months and days.
    fn set_fill_lists(&mut self, _lists: Vec<Vec<String>>) {}

    /// Fills `target` (first row, first column, last row, last column),
    /// which holds `source` and goes past it one way, from `source`, as a
    /// spreadsheet's fill handle: with `series`, numbers, dates, numbered
    /// text and month or day names continue as the source goes; else the
    /// source is copied over again (Fill Down). Formulas move their
    /// relative references either way, and cells keep the source's format.
    fn fill(
        &mut self,
        _unit: usize,
        _source: [u32; 4],
        _target: [u32; 4],
        _series: bool,
    ) -> Result<Vec<usize>> {
        Err(ViewerError("This format is not edited".into()))
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

    /// Sorts a range's rows by several columns in turn (a spreadsheet's
    /// Custom Sort), the first row kept in place when `header`. By default
    /// one key without a list as [`ViewerDocument::sort_range`].
    fn sort_range_by(
        &mut self,
        unit: usize,
        range: [u32; 4],
        keys: &[SortKey],
        header: bool,
    ) -> Result<Vec<usize>> {
        match keys {
            [k] if k.list.is_none() => self.sort_range(unit, range, k.col, k.descending, header),
            _ => Err(ViewerError("This format sorts by one column".into())),
        }
    }

    /// Filters column `col` of the filter by a rule, or clears its filter
    /// with `None`. By default values as [`ViewerDocument::filter_column`].
    fn filter_column_by(
        &mut self,
        unit: usize,
        col: u32,
        rule: Option<FilterRule>,
    ) -> Result<Vec<usize>> {
        match rule {
            None => self.filter_column(unit, col, None),
            Some(FilterRule::Values(v)) => self.filter_column(unit, col, Some(v)),
            Some(_) => Err(ViewerError("This format filters by values".into())),
        }
    }

    /// The rule a filter column has, if it is one these rules tell.
    fn column_filter(&mut self, _unit: usize, _col: u32) -> Option<FilterRule> {
        None
    }

    /// Applies the filter's rules again to the rows as they are now.
    fn reapply_filter(&mut self, _unit: usize) -> Result<Vec<usize>> {
        Ok(Vec::new())
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
    fn bgra() {
        let b = Bitmap::new(1, 2, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(b.bgra(), [3, 2, 1, 4, 7, 6, 5, 8]);
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
    fn a_handle_reads_through_a_function() {
        let data: Vec<u8> = (0..10u8).collect();
        let h = FileHandle::from_reader("dir/b.PDF", 10, move |off, len| {
            let s = (off as usize).min(data.len());
            data[s..(s + len).min(data.len())].to_vec()
        });
        assert_eq!((h.name(), h.extension().as_str()), ("b.PDF", "pdf"));
        assert_eq!(h.len().unwrap(), 10);
        assert_eq!(h.read_at(3, 4).unwrap(), [3, 4, 5, 6]);
        assert_eq!(h.read_all().unwrap(), (0..10u8).collect::<Vec<_>>());
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
