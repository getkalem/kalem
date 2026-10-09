//! Documents of flowing text and their annotations (API 0.2.7, the WIT
//! interfaces `flow` and `annotations`): the types of the contract's
//! functions [`crate::ViewerDocument::flow`] and
//! [`crate::ViewerDocument::annotations`] and those around them.
//!
//! A unit's flow is a list of [`FlowItem`]s: paragraphs, and the starts
//! and ends that put paragraphs in tables, notes and frames. A
//! paragraph's *edit text* ([`FlowParagraph::text`]) is its text as edits
//! count it: one character per tab, line break ([`LINE_BREAK`]), page
//! break ([`PAGE_BREAK`]), column break ([`COLUMN_BREAK`]) and object
//! ([`OBJECT`]); [`FlowPlace`]s are bytes of it. Nothing here is one
//! format's: Word's, OpenDocument's, an e-book's and an e-mail's
//! paragraphs are all said in these words.

/// A line break in a paragraph's edit text.
pub const LINE_BREAK: char = '\u{B}';
/// A page break in a paragraph's edit text.
pub const PAGE_BREAK: char = '\u{C}';
/// A column break in a paragraph's edit text.
pub const COLUMN_BREAK: char = '\u{E}';
/// An object (a picture, a note's mark) in a paragraph's edit text.
pub const OBJECT: char = '\u{FFFC}';

/// A unit's flow, when the unit is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlowLayout {
    /// How many items it has.
    pub items: u32,
    /// Changes whenever the flow does.
    pub version: u64,
    /// Whether its text is edited ([`crate::ViewerDocument::flow_replace`]
    /// and the rest).
    pub editable: bool,
}

/// What a paragraph is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowRole {
    /// Body text.
    #[default]
    Body,
    /// A document's title.
    Title,
    /// Its subtitle.
    Subtitle,
    /// A heading; [`FlowParagraph::level`] says which.
    Heading,
    /// An item of a list; [`FlowParagraph::level`] says how deep.
    ListItem,
    /// A quotation.
    Quote,
    /// Code.
    Code,
    /// A caption.
    Caption,
}

/// A paragraph's alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowAlign {
    /// At the start of the line.
    #[default]
    Start,
    /// Centered.
    Center,
    /// At the end.
    End,
    /// Justified.
    Justify,
}

/// Raised or lowered text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Script {
    /// On the baseline.
    #[default]
    Baseline,
    /// Superscript.
    Superscript,
    /// Subscript.
    Subscript,
}

/// How a run looks, every value resolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marks {
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// The underline's kind (`single`, `double`, `wave`…), when underlined.
    pub underline: Option<String>,
    /// Struck through.
    pub strike: bool,
    /// Struck through twice.
    pub double_strike: bool,
    /// In capitals.
    pub caps: bool,
    /// In small capitals.
    pub small_caps: bool,
    /// Hidden text.
    pub hidden: bool,
    /// Raised or lowered.
    pub script: Script,
    /// The text's color; `None` for the theme's.
    pub color: Option<[u8; 3]>,
    /// The highlight behind it.
    pub highlight: Option<[u8; 3]>,
    /// The size in points; `None` for the body text's.
    pub size: Option<f32>,
    /// The typeface; `None` for the editor's.
    pub face: Option<String>,
}

/// A picture in a flow.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowPicture {
    /// Its name for [`crate::ViewerDocument::flow_picture`].
    pub id: String,
    /// Its width in points.
    pub width: f32,
    /// Its height in points.
    pub height: f32,
    /// Its alternative text.
    pub alt: String,
}

/// What a run stands for.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Piece {
    /// Text.
    #[default]
    Text,
    /// A tab.
    Tab,
    /// A line break.
    LineBreak,
    /// A page break.
    PageBreak,
    /// A column break.
    ColumnBreak,
    /// A note's mark: the note's ID (an [`Aside`]'s).
    NoteMark(String),
    /// A picture.
    Picture(FlowPicture),
    /// A construct shown by name: its text says what.
    Placeholder,
}

/// A run: text of one look.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowRun {
    /// What is shown.
    pub text: String,
    /// What it stands for.
    pub piece: Piece,
    /// How it looks.
    pub marks: Marks,
    /// The bytes of its paragraph's edit text it stands for.
    pub source: std::ops::Range<u32>,
    /// A link: a URL, or `#name` for a place in the document.
    pub link: Option<String>,
    /// The annotations it is in, by ID.
    pub annotations: Vec<String>,
    /// Why it is not edited as text, when it is not.
    pub locked: Option<String>,
}

/// A paragraph of a flow.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowParagraph {
    /// Its number among the unit's edited paragraphs; `None` for one that
    /// is not edited.
    pub index: Option<u32>,
    /// What it is for.
    pub role: FlowRole,
    /// A heading's level or a list item's depth, from 1; 0 otherwise.
    pub level: u8,
    /// Its style's name for people.
    pub style: String,
    /// A list item's label and how it looks.
    pub label: Option<(String, Marks)>,
    /// Its alignment.
    pub align: FlowAlign,
    /// Start, end and first line's indents in points.
    pub indent: (f32, f32, f32),
    /// Space before and after, in points.
    pub spacing: (f32, f32),
    /// Its background.
    pub background: Option<[u8; 3]>,
    /// Its edit text.
    pub text: String,
    /// Its runs, in order.
    pub runs: Vec<FlowRun>,
    /// The annotations on its end (a paragraph inserted or deleted).
    pub annotations: Vec<String>,
}

/// A table's grid and style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowTable {
    /// The grid's column widths in points.
    pub columns: Vec<f32>,
    /// Its style's name.
    pub style: String,
}

/// A row of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlowRow {
    /// Repeated at the top of each page.
    pub header: bool,
}

/// A border line.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowBorder {
    /// Its color; `None` for automatic.
    pub color: Option<[u8; 3]>,
    /// Its width in points.
    pub width: f32,
    /// Its line (`single`, `double`…).
    pub style: String,
}

/// A cell of a row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowCell {
    /// How many grid columns it spans.
    pub columns: u32,
    /// It continues the cell above (merged down).
    pub merged: bool,
    /// Its background.
    pub background: Option<[u8; 3]>,
    /// Top, start, bottom and end.
    pub borders: [Option<FlowBorder>; 4],
}

/// What an aside is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AsideKind {
    /// A page header.
    Header,
    /// A page footer.
    Footer,
    /// A footnote.
    Footnote,
    /// An endnote.
    Endnote,
    /// A text box, a slide's shape.
    #[default]
    Frame,
    /// A sidebar.
    Sidebar,
}

/// Blocks apart from the main flow.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Aside {
    /// What it is.
    pub kind: AsideKind,
    /// Its ID (a note's, which a [`Piece::NoteMark`] names).
    pub id: String,
    /// What it is labeled with: a note's mark, a frame's name.
    pub label: String,
}

/// An item of a flow.
// Paragraphs are most items: boxing them would cost an allocation each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum FlowItem {
    /// A paragraph.
    Paragraph(FlowParagraph),
    /// A table begins.
    TableStart(FlowTable),
    /// A row begins.
    RowStart(FlowRow),
    /// A cell begins.
    CellStart(FlowCell),
    /// The cell ends.
    CellEnd,
    /// The row ends.
    RowEnd,
    /// The table ends.
    TableEnd,
    /// An aside begins.
    AsideStart(Aside),
    /// It ends.
    AsideEnd,
    /// A break between blocks: `page`, `column`, a section's kind, `line`.
    Rule(String),
    /// A block shown by name.
    Placeholder(String),
}

/// A place in a flow: an edited paragraph by its index, a byte of its edit
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FlowPlace {
    /// The paragraph's [`FlowParagraph::index`].
    pub paragraph: u32,
    /// A byte of its edit text.
    pub offset: u32,
}

/// A change of marks over a range.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkChange {
    /// Bold on or off.
    Bold(bool),
    /// Italic on or off.
    Italic(bool),
    /// Underlined in a kind, or not.
    Underline(Option<String>),
    /// Struck through or not.
    Strike(bool),
    /// Raised, lowered or on the baseline.
    Script(Script),
    /// A color, or the theme's.
    Color(Option<[u8; 3]>),
    /// A highlight, or none.
    Highlight(Option<[u8; 3]>),
    /// A size in points, or the style's.
    Size(Option<f32>),
    /// A typeface, or the style's.
    Face(Option<String>),
    /// Back to the style's look.
    Clear,
}

/// What a style applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowStyleKind {
    /// Paragraphs.
    #[default]
    Paragraph,
    /// Runs.
    Character,
}

/// A style a user may give.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlowStyle {
    /// The plugin's name for it.
    pub id: String,
    /// Its name for people.
    pub name: String,
    /// What it applies to.
    pub kind: FlowStyleKind,
    /// Shown among the styles a user picks.
    pub shown: bool,
}

/// What an annotation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnnotationKind {
    /// A remark on a part of the document.
    #[default]
    Comment,
    /// Inserted, a tracked change.
    Insertion,
    /// Deleted, a tracked change.
    Deletion,
    /// Formatting changed, a tracked change.
    Formatting,
    /// Moved away from here.
    MoveFrom,
    /// Moved here.
    MoveTo,
}

impl AnnotationKind {
    /// Whether it is a tracked change (accepted or rejected), not a
    /// comment.
    pub fn is_change(self) -> bool {
        self != AnnotationKind::Comment
    }
}

/// What an annotation is about.
#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
    /// Text of a flow unit, from a place to a later one.
    Flow {
        /// The unit.
        unit: usize,
        /// Where it starts.
        from: FlowPlace,
        /// Where it ends.
        to: FlowPlace,
    },
    /// Bytes of a unit's text ([`crate::ViewerDocument::text`]).
    Text {
        /// The unit.
        unit: usize,
        /// The bytes.
        range: std::ops::Range<usize>,
    },
    /// A cell of a grid unit.
    Cell {
        /// The unit.
        unit: usize,
        /// The row.
        row: u32,
        /// The column.
        col: u32,
    },
    /// An area of a unit, in its pixels at scale 1: x, y, width, height.
    Area {
        /// The unit.
        unit: usize,
        /// The area.
        rect: [f32; 4],
    },
    /// A whole unit.
    Unit(usize),
}

/// A comment or a tracked change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Annotation {
    /// Its ID.
    pub id: String,
    /// What it is.
    pub kind: AnnotationKind,
    /// Its author.
    pub author: String,
    /// When, ISO 8601, when the format keeps it.
    pub date: Option<String>,
    /// A comment's text; how formatting changed.
    pub text: String,
    /// The comment it answers.
    pub parent: Option<String>,
    /// A comment marked done.
    pub resolved: bool,
    /// What it is about.
    pub anchors: Vec<Anchor>,
}
