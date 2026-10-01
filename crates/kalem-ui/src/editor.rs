//! The editor: a document shown as a virtualized list of its visible
//! lines, edited through the shared commands, keymap and typing rules.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    App, AppContext, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, FocusHandle,
    Focusable, KeyDownEvent, ListAlignment, ListState, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Point, SharedString, UTF16Selection, Window, px,
};
use kalem_core::command::{Clipboard, EditorContext, Request};
use kalem_core::keymap::{Keymap, KeymapIssue, Lookup};
use kalem_core::keys::{KeyChord, KeySequence};
use kalem_core::settings::Config;
use kalem_core::view::{self, Block, BlockKind, Folds, LineView, Widget};
use kalem_core::when::{Context as WhenContext, Value as WhenValue};
use kalem_core::{CommandRegistry, DocumentMode, DocumentState, tr};
use org_edit::{Assoc, Transaction};
use serde_json::Value;

use crate::theme::Theme;
use gpui_rich_text::InlineLayout;

/// Runs a command of the registry (from menus and the toolbar).
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = kalem, no_json)]
pub struct RunCommand {
    /// The command.
    pub id: SharedString,
    /// Its arguments as JSON, or empty.
    pub args: SharedString,
}

impl RunCommand {
    /// The command `id` without arguments.
    pub fn new(id: &str) -> RunCommand {
        RunCommand {
            id: id.to_string().into(),
            args: SharedString::default(),
        }
    }

    /// The command `id` with arguments.
    pub fn with(id: &str, args: Value) -> RunCommand {
        RunCommand {
            id: id.to_string().into(),
            args: args.to_string().into(),
        }
    }

    /// The arguments.
    pub fn args(&self) -> Value {
        serde_json::from_str(&self.args).unwrap_or(Value::Null)
    }
}

/// What every window shares: settings, commands and keys.
#[derive(Debug)]
pub struct Shared {
    /// The settings.
    pub config: Config,
    /// The commands.
    pub registry: CommandRegistry,
    /// The keymap.
    pub keymap: Keymap,
    /// Problems in the keymap files.
    pub issues: Vec<KeymapIssue>,
    /// Command and Control trade places (macOS, Word-like profile).
    pub swap_primary: bool,
    /// Reads the HTML on the system clipboard (tests replace it).
    pub html_clipboard: fn() -> Option<String>,
    /// The user's settings file, which the settings panel writes.
    pub settings_path: Option<std::path::PathBuf>,
    /// Rendered formulas.
    pub math: crate::math::Formulas,
    /// Pictures of image links.
    pub pictures: crate::pictures::Pictures,
    /// The project list and the files of the projects in use.
    pub projects: RefCell<kalem_core::projects::ProjectState>,
    /// File operations running in the background (the file manager).
    pub jobs: Rc<RefCell<Vec<kalem_core::dired::Running>>>,
    /// The completers (built-ins, and plugins').
    pub completers: kalem_core::completers::Registry,
    /// Problems in the settings and keymap files, told once in the first
    /// window.
    pub problems: std::cell::Cell<usize>,
    /// The event bus (design §11.3), shared by the windows: events to
    /// plugins and the frontend's listeners, as the terminal editor sends
    /// them.
    pub bus: Rc<RefCell<kalem_core::events::EventBus>>,
    /// The request that opened the last list to choose from, and what was
    /// typed in it (`SPC '`), and the panel shown or hidden last (`SPC ~`),
    /// whichever document it was in.
    pub last: Rc<RefCell<Last>>,
}

/// What `SPC '` and `SPC ~` do again.
#[derive(Debug, Default)]
pub struct Last {
    /// The request that opened the last list, and what was typed in it.
    pub picker: Option<(Request, String)>,
    /// The panel shown or hidden last.
    pub panel: Option<Request>,
}

/// The next document's number on the event bus.
static NEXT_DOC: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// What an editor asks of its window: documents to open, show or close.
#[derive(Debug, Clone, PartialEq)]
pub enum DocEvent {
    /// Open `path` (or show it if open), at a line (from 1) and a byte
    /// column.
    Open {
        /// The file.
        path: std::path::PathBuf,
        /// Where to put the cursor.
        at: Option<(u64, usize)>,
    },
    /// A new, empty document.
    New,
    /// Close this document (unsaved changes were dealt with).
    Close,
    /// Show the next open document, or the previous one.
    Cycle(bool),
    /// Show the open document with this index.
    Activate(usize),
    /// Open a picker: about a project (else the current one), and what
    /// choosing a project leads to.
    Pick(
        kalem_core::command::PickKind,
        Option<std::path::PathBuf>,
        kalem_core::projects::After,
    ),
    /// Search a project (else the current one, after choosing one when
    /// there is none).
    Search(Option<std::path::PathBuf>),
    /// Show or hide the list of open files.
    ToggleFiles,
    /// Save the modified documents of the project at this folder.
    SaveProject(std::path::PathBuf),
    /// Close the documents of the project at this folder.
    CloseProject(std::path::PathBuf),
    /// The live search of lines: this document's or every open one's.
    SearchLines {
        /// Every open document.
        all: bool,
        /// Headings only.
        headings: bool,
        /// The query to start with.
        text: String,
    },
    /// Show open document `doc` (an index), the cursor at byte `at`.
    Jump {
        /// The document's index.
        doc: usize,
        /// Where the cursor goes.
        at: usize,
    },
    /// Act on the open documents (Doom's `SPC b`).
    Documents(kalem_core::command::DocumentsRequest),
    /// Quit, asking about every unsaved document.
    Quit,
    /// Show `place` in the window's file manager (a new one if there is
    /// none), the cursor on `select`.
    FileManager {
        /// A folder, or the projects.
        place: kalem_core::dired::Place,
        /// The entry to put the cursor on.
        select: Option<std::path::PathBuf>,
    },
    /// Back to the document shown before the file manager.
    LeaveFileManager,
    /// A file operation finished: listings are read again, and the message
    /// shows.
    FilesChanged {
        /// What it did.
        message: String,
        /// Whether it failed.
        error: bool,
        /// What a file operation did, for the open documents to follow.
        outcome: Option<(kalem_core::kalem_fs::OpKind, kalem_core::kalem_fs::Outcome)>,
    },
}

impl gpui::EventEmitter<DocEvent> for Editor {}

/// Highlighting of a source block, by line.
pub type CodeSpans = Arc<Vec<Vec<kalem_highlight::Span>>>;

/// Highlighted source blocks by start, for a text version.
pub type CodeCache = RefCell<(u64, HashMap<usize, Option<CodeSpans>>)>;

/// Tables drawn as grids, by start and font size, for a text version.
pub type GridCache = RefCell<(u64, HashMap<(usize, u32), Rc<crate::line::Grid>>)>;

/// A popup under the caret: where, and its lines, each with whether it is
/// the chosen one.
pub type Popup = (Point<Pixels>, Vec<(String, bool)>);

/// A plain text document's highlighting and indentation step, for a text
/// version.
pub type PlainCache = RefCell<Option<(u64, Option<kalem_highlight::Highlighter>, usize)>>;

/// What is under the mouse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The source offset.
    pub pos: usize,
    /// A widget there, with its source range.
    pub widget: Option<(Range<usize>, Widget)>,
    /// A fold arrow there, with its headline's start.
    pub fold: Option<usize>,
    /// A copy button there, with its block's start.
    pub copy: Option<usize>,
    /// A table of contents row there, with its heading's start.
    pub jump: Option<usize>,
}

/// A line as last painted: for hit testing, the caret and IME.
#[derive(Debug, Clone)]
pub struct Painted {
    /// Where, in window coordinates.
    pub bounds: Bounds<Pixels>,
    /// Its layout.
    pub layout: Rc<InlineLayout>,
    /// Its view.
    pub view: Rc<LineView>,
    /// Widgets, in window coordinates, with their source ranges.
    pub widgets: Vec<(Bounds<Pixels>, Range<usize>, Widget)>,
    /// The fold arrow of a heading line, with the headline start.
    pub fold: Option<(Bounds<Pixels>, usize)>,
    /// Copy buttons, with the start of their block.
    pub buttons: Vec<(Bounds<Pixels>, usize)>,
    /// Table of contents rows, with the start of their heading.
    pub jumps: Vec<(Bounds<Pixels>, usize)>,
}

/// The editor.
pub struct Editor {
    /// The document.
    pub doc: DocumentState,
    /// Settings, commands and keys.
    pub shared: Rc<Shared>,
    clipboard: Clipboard,
    focus: FocusHandle,
    /// The list of visible lines.
    pub list: ListState,
    /// The source line of each list item.
    pub visible: Vec<usize>,
    line_count: usize,
    /// Folded headlines.
    pub folds: Folds,
    blocks: Option<(u64, Arc<Vec<Block>>)>,
    /// Blocks shown as their first line away from the cursor.
    pub folded_blocks: HashSet<usize>,
    /// Colors and fonts.
    pub theme: Theme,
    /// Keys of a sequence typed so far (`space p`, `C-c`).
    pub pending: Vec<KeyChord>,
    /// When the half-typed sequence began, for the which-key delay.
    pub pending_at: Option<Instant>,
    /// Whether the which-key panel was asked for since `pending_at`.
    pub hints_drawn: bool,
    /// IME composition in progress.
    pub marked: Option<Range<usize>>,
    /// Lines as last painted, by source line.
    pub painted: Rc<RefCell<HashMap<usize, Painted>>>,
    /// A message for the status bar, and whether it is an error.
    pub status: Option<(String, bool)>,
    pub(crate) goal_x: Option<Pixels>,
    /// The last command run, for commands that act on repeats.
    pub(crate) last_command: Option<String>,
    /// The next keys are described, not run (`SPC h k`).
    pub(crate) describing: bool,
    cursor_line: usize,
    /// The first line of the block holding the cursor, drawn differently
    /// while the cursor is inside.
    cursor_block: Option<usize>,
    /// The source view: plain text.
    pub source: bool,
    /// Highlighted source blocks by start, for the text version.
    pub code: CodeCache,
    /// Tables drawn as grids, by start and font size, for the text version.
    pub grids: GridCache,
    /// The open completion menu.
    pub completion: Option<kalem_core::completers::Menu>,
    dragging: bool,
    /// Whether this pane is on the left in a split.
    left: bool,
    /// The outline sidebar, when shown.
    pub outline: Option<crate::outline::Outline>,
    /// The file manager's preview pane, when shown: `true` for the
    /// listing's thumbnails.
    pub preview: Option<bool>,
    /// What the preview pane shows, for the file it was read from.
    pub preview_cache: crate::preview::Cache,
    /// The command palette, when open.
    pub palette: Option<crate::panels::Palette>,
    /// What to type into the list opening now, resumed.
    pub resume_input: Option<String>,
    /// A list to choose from was asked for: the next palette is one.
    pub mark_picker: bool,
    /// The find bar, when open.
    pub find: Option<crate::panels::FindBar>,
    /// The date picker, when open.
    pub date_picker: Option<crate::datepicker::DatePicker>,
    /// The settings panel, when open.
    pub settings: Option<crate::preferences::SettingsPanel>,
    /// Search matches, marked in the text.
    pub highlights: Vec<Range<usize>>,
    /// The last search and whether it was a regular expression.
    pub last_search: (String, bool),
    /// Word counts for the status bar.
    pub words: RefCell<kalem_core::stats::WordCounts>,
    /// The table formula at the cursor.
    pub formula: kalem_core::formulas::FormulaCache,
    /// What the status bar says about it, and the fields it refers to
    /// (updated when drawing).
    pub formula_status: Option<String>,
    /// The fields the formula at the cursor refers to, highlighted.
    pub formula_refs: Vec<Range<usize>>,
    /// The entry cited under the cursor, for the status bar.
    pub cite_preview: kalem_core::cite::Preview,
    /// The entry cited under the mouse, and where the mouse is.
    pub cite_hover: Option<(Point<Pixels>, String)>,
    /// The file manager's context menu, where it opened (T2.7e.17).
    pub context_menu: Option<(Point<Pixels>, Vec<kalem_core::dired::ContextItem>)>,
    /// Formulas drawn rendered (else as their source).
    pub math: bool,
    /// The document's `\newcommand`s for formulas, for a text version.
    pub math_macros: RefCell<(u64, Rc<str>)>,
    /// Focus mode: only the section holding the cursor shows.
    pub focus_mode: bool,
    /// The Vim layer, with the Vim keymap profile.
    pub vim: Option<kalem_core::vim::Vim>,
    /// Long lines wrap (`editor.soft_wrap`, Alt+Z).
    pub wrap: bool,
    /// `org-indent-mode` (`editor.outline_indent`, `#+STARTUP: indent`):
    /// headings move right by level, and the text under one with it.
    pub outline_indent: bool,
    /// The document's number on the event bus.
    pub doc_id: kalem_core::events::DocumentId,
    /// Edits, sent as `document:changed` after a pause.
    debouncer: kalem_core::events::ChangeDebouncer,
    /// The selection last sent as `selection:changed`.
    sent_selection: Option<(usize, usize)>,
    /// How far lines are scrolled sideways when they do not wrap.
    pub hscroll: Pixels,
    /// A plain text document's highlighting and indentation step.
    pub plain: PlainCache,
    /// The highlighting of a file too large for [`Editor::plain`]'s: the
    /// lines on screen, in windows.
    pub windowed: RefCell<kalem_highlight::Windowed>,
    /// When the file was last compared with the disk.
    disk_checked: Instant,
    /// A change on disk was reported and is not resolved.
    disk_conflict: bool,
    /// The other pane of a split view (T1.5.14). The fields above from
    /// `list` to `source` are the active pane's; the two trade places when
    /// the other one is drawn or clicked.
    pub other: Option<Pane>,
}

/// The state of one view of the document in a split: what the editor holds
/// for its active view.
#[derive(Debug)]
pub struct Pane {
    list: ListState,
    visible: Vec<usize>,
    line_count: usize,
    folded_blocks: HashSet<usize>,
    /// Lines as last painted, by source line.
    pub painted: Rc<RefCell<HashMap<usize, Painted>>>,
    cursor_line: usize,
    cursor_block: Option<usize>,
    /// The source view.
    pub source: bool,
    left: bool,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("path", &self.doc.meta.path)
            .finish_non_exhaustive()
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Editor {
    /// An editor for `doc`.
    pub fn new(
        doc: DocumentState,
        shared: Rc<Shared>,
        theme: Theme,
        cx: &mut Context<'_, Self>,
    ) -> Editor {
        let mut e = Editor {
            doc,
            shared,
            clipboard: Clipboard::default(),
            focus: cx.focus_handle(),
            list: ListState::new(0, ListAlignment::Top, px(600.)),
            visible: Vec::new(),
            line_count: 0,
            folds: Folds::default(),
            blocks: None,
            folded_blocks: HashSet::new(),
            theme,
            pending: Vec::new(),
            pending_at: None,
            resume_input: None,
            mark_picker: false,
            hints_drawn: false,
            marked: None,
            painted: Rc::default(),
            status: None,
            goal_x: None,
            last_command: None,
            describing: false,
            cursor_line: 0,
            cursor_block: None,
            source: false,
            code: RefCell::new((0, HashMap::new())),
            grids: RefCell::new((0, HashMap::new())),
            completion: None,
            dragging: false,
            left: true,
            other: None,
            outline: None,
            preview: None,
            preview_cache: Default::default(),
            palette: None,
            find: None,
            date_picker: None,
            settings: None,
            highlights: Vec::new(),
            last_search: (String::new(), false),
            words: RefCell::default(),
            formula: Default::default(),
            formula_status: None,
            formula_refs: Vec::new(),
            cite_preview: Default::default(),
            cite_hover: None,
            context_menu: None,
            math: true,
            math_macros: RefCell::new((u64::MAX, Rc::from(""))),
            focus_mode: false,
            vim: None,
            wrap: true,
            outline_indent: false,
            doc_id: kalem_core::events::DocumentId(
                NEXT_DOC.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ),
            debouncer: kalem_core::events::ChangeDebouncer::new(std::time::Duration::from_millis(
                300,
            )),
            sent_selection: None,
            hscroll: px(0.),
            plain: RefCell::default(),
            windowed: RefCell::default(),
            disk_checked: Instant::now(),
            disk_conflict: false,
        };
        e.refresh_vim();
        e.shared
            .bus
            .borrow_mut()
            .emit(&kalem_core::events::Event::DocumentOpen {
                doc: e.doc_id,
                path: e.doc.meta.path.clone(),
            });
        e.wrap = e.shared.config.bool("editor.soft_wrap");
        let problems = e.shared.problems.take();
        if problems > 0 {
            e.message(tr!("msg-config-problems", count = problems), true);
        }
        e.startup_folds();
        e.visible = e.compute_visible();
        e.line_count = e.doc.text().line_count();
        e.list.reset(e.visible.len());
        e
    }

    fn startup_folds(&mut self) {
        self.outline_indent = self.shared.config.bool("editor.outline_indent");
        let Some((p, true)) = self.doc.parse() else {
            return;
        };
        self.outline_indent = view::outline_indent(&p.keywords(), self.outline_indent);
        let option = p
            .keywords()
            .into_iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("STARTUP"))
            .flat_map(|(_, v)| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .rfind(|w| {
                matches!(
                    w.as_str(),
                    "overview" | "fold" | "content" | "showall" | "showeverything"
                )
            });
        if let Some(o) = option {
            let blocks = self.blocks();
            self.folds = Folds::startup(&blocks, &o);
        }
    }

    /// How far line `line` moves right with `org-indent-mode`: the
    /// width of two characters for each level of its heading below the
    /// first, at most four.
    pub fn outline_indent_of(&mut self, line: usize) -> Pixels {
        if !self.outline_indent
            || self.source
            || self.doc.meta.mode != kalem_core::DocumentMode::Org
        {
            return px(0.);
        }
        let start = self.doc.text().line_start(line);
        let depth = view::outline_depth(&self.blocks(), start);
        let base = px(self.doc_theme().size);
        base * 1.2 * depth.saturating_sub(1).min(4) as f32
    }

    /// The blocks of the current text (none while a full parse runs).
    pub fn blocks(&mut self) -> Arc<Vec<Block>> {
        let version = self.doc.version();
        if let Some((v, b)) = &self.blocks
            && *v == version
        {
            return b.clone();
        }
        let b = match self.doc.parse() {
            // LaTeX: its displayed formulas, and the text between them.
            _ if self.doc.latex().is_some() => Arc::new(kalem_core::latex_view::blocks(&self.doc)),
            Some((p, true)) => Arc::new(view::blocks(&p.syntax(), p.context())),
            _ => Arc::new(Vec::new()),
        };
        if !b.is_empty() {
            self.folds.retain(&b);
            self.blocks = Some((version, b.clone()));
        }
        b
    }

    /// The source lines that show.
    fn compute_visible(&mut self) -> Vec<usize> {
        let lines = self.compute_lines();
        self.limit_lines(lines)
    }

    /// The lines within the narrowed part or, in focus mode, the section
    /// holding the cursor.
    fn limit_lines(&self, lines: Vec<usize>) -> Vec<usize> {
        let Some(lim) = view::limit(&self.doc, self.focus_mode) else {
            return lines;
        };
        let text = self.doc.text();
        let len = text.len();
        let first = text.line_of(lim.start.min(len));
        let last = if lim.end >= len {
            usize::MAX
        } else {
            text.line_of(lim.end.saturating_sub(1).max(lim.start))
        };
        let mut out: Vec<usize> = lines
            .into_iter()
            .filter(|l| (first..=last).contains(l))
            .collect();
        if out.is_empty() {
            out.push(first);
        }
        out
    }

    fn compute_lines(&mut self) -> Vec<usize> {
        let n = self.doc.text().line_count();
        // A CSV filter or sort: the lines of the rows shown, in their order.
        if !self.source
            && let Some(lines) = kalem_core::csv::shown_lines(&self.doc)
                .or_else(|| kalem_core::bibtex::shown_lines(&self.doc))
        {
            return lines.to_vec();
        }
        let blocks = self.blocks();
        if self.source || blocks.is_empty() {
            self.folded_blocks.clear();
            return (0..n).collect();
        }
        let v = view::visible(
            self.doc.text().as_str(),
            &blocks,
            &self.folds,
            self.doc.selection.head,
        );
        self.folded_blocks = v.folded;
        let text = self.doc.text();
        let len = text.len();
        let mut out = Vec::new();
        for r in &v.ranges {
            let first = text.line_of(r.start);
            let last = if r.end >= len {
                n - 1
            } else {
                text.line_of(r.end.saturating_sub(1))
            };
            for l in first..=last {
                if out.last() != Some(&l) {
                    out.push(l);
                }
            }
        }
        // A LaTeX environment away from the cursor shows as one formula, on
        // its first line.
        if self.math {
            let c = self.doc.selection.head;
            for b in blocks.iter().filter(|b| b.kind == BlockKind::Math) {
                if b.range.start <= c && c <= b.content_end {
                    continue;
                }
                let first = text.line_of(b.range.start);
                let last = text.line_of(b.content_end.saturating_sub(1).max(b.range.start));
                if last > first {
                    out.retain(|l| *l <= first || *l > last);
                }
            }
        }
        if out.is_empty() {
            out.push(0);
        }
        out
    }

    /// The list item of source line `line`, if it shows.
    pub fn item_of(&self, line: usize) -> Option<usize> {
        // A CSV view sorted by a column shows its lines out of order.
        if self.doc.csv_sort.is_some() && self.doc.meta.mode == DocumentMode::Csv {
            return self.visible.iter().position(|&l| l == line);
        }
        self.visible.binary_search(&line).ok()
    }

    /// Makes the other pane of a split the active one.
    pub fn swap_panes(&mut self) {
        let Some(mut p) = self.other.take() else {
            return;
        };
        std::mem::swap(&mut self.list, &mut p.list);
        std::mem::swap(&mut self.visible, &mut p.visible);
        std::mem::swap(&mut self.line_count, &mut p.line_count);
        std::mem::swap(&mut self.folded_blocks, &mut p.folded_blocks);
        std::mem::swap(&mut self.painted, &mut p.painted);
        std::mem::swap(&mut self.cursor_line, &mut p.cursor_line);
        std::mem::swap(&mut self.cursor_block, &mut p.cursor_block);
        std::mem::swap(&mut self.source, &mut p.source);
        std::mem::swap(&mut self.left, &mut p.left);
        self.other = Some(p);
    }

    /// Runs `f` with the other pane of a split active, if there is one.
    pub fn with_other<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        self.other.as_ref()?;
        self.swap_panes();
        let r = f(self);
        self.swap_panes();
        Some(r)
    }

    /// Splits the view: the source beside the rich view (or the rich view
    /// beside the source), or closes the other view.
    pub fn toggle_split(&mut self, cx: &mut Context<'_, Self>) {
        if self.other.take().is_some() {
            self.left = true;
        } else {
            let source = !self.source;
            self.left = !source;
            self.other = Some(Pane {
                list: ListState::new(0, ListAlignment::Top, px(600.)),
                visible: Vec::new(),
                line_count: self.doc.text().line_count(),
                folded_blocks: HashSet::new(),
                painted: Rc::default(),
                cursor_line: self.cursor_line,
                cursor_block: self.cursor_block,
                source,
                left: source,
            });
            self.with_other(|e| {
                e.visible = e.compute_visible();
                e.list.reset(e.visible.len());
                if let Some(i) = e.item_of(e.doc.text().line_of(e.doc.selection.head)) {
                    e.list.scroll_to_reveal_item(i);
                }
            });
        }
        cx.notify();
    }

    /// Measures every line again (after a theme or font change).
    pub fn relayout(&mut self) {
        self.list.reset(self.visible.len());
        self.with_other(|e| e.list.reset(e.visible.len()));
    }

    /// Updates the lists of both panes after edits `changes` (or folding).
    fn sync_list(&mut self, changes: &[Transaction]) {
        self.sync_pane(changes);
        self.with_other(|e| e.sync_pane(changes));
    }

    /// Updates the list after edits `changes` (or folding): only the items
    /// that changed are measured again, and the lines the cursor left and
    /// entered, whose markup shows or hides.
    fn sync_pane(&mut self, changes: &[Transaction]) {
        let new = self.compute_visible();
        let head = self.doc.selection.head;
        let block = self
            .block_at(head)
            .filter(|b| b.range.start <= head && head <= b.content_end)
            .map(|b| b.range.start);
        let text = self.doc.text();
        let (old_count, new_count) = (self.line_count, text.line_count());
        // The changed span in the final text.
        let mut first = usize::MAX;
        let mut last = 0usize;
        let mut ends: Vec<usize> = Vec::new();
        let mut starts: Vec<usize> = Vec::new();
        for tx in changes {
            for p in &mut starts {
                *p = tx.map(*p, Assoc::Before);
            }
            for p in &mut ends {
                *p = tx.map(*p, Assoc::After);
            }
            for e in &tx.edits {
                starts.push(tx.map(e.range.start, Assoc::Before));
                ends.push(tx.map(e.range.end, Assoc::After));
            }
        }
        let len = text.len();
        if let Some(s) = starts.iter().min() {
            first = text.line_of((*s).min(len));
        }
        if let Some(e) = ends.iter().max() {
            last = text.line_of((*e).min(len));
        }
        let old = std::mem::take(&mut self.visible);
        let mut prefix = 0;
        while prefix < old.len()
            && prefix < new.len()
            && old[prefix] == new[prefix]
            && new[prefix] < first
        {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < old.len() - prefix && suffix < new.len() - prefix {
            let (o, n) = (old[old.len() - 1 - suffix], new[new.len() - 1 - suffix]);
            if old_count - o != new_count - n || (n <= last && !changes.is_empty()) {
                break;
            }
            suffix += 1;
        }
        let changed_items = old.len() - prefix - suffix;
        let new_items = new.len() - prefix - suffix;
        if changed_items > 0 || new_items > 0 {
            self.list.splice(prefix..prefix + changed_items, new_items);
        }
        self.visible = new;
        self.line_count = new_count;
        // Lines whose markup changes with the cursor.
        let now = text.line_of(self.doc.selection.head.min(len));
        let block = block.map(|b| text.line_of(b.min(len)));
        let mut lines = vec![self.cursor_line, now];
        if block != self.cursor_block {
            lines.extend(self.cursor_block);
            lines.extend(block);
        }
        lines.sort_unstable();
        lines.dedup();
        for l in lines {
            if let Some(i) = self.item_of(l)
                && (i < prefix || i >= prefix + new_items)
            {
                self.list.splice(i..i + 1, 1);
            }
        }
        self.cursor_line = now;
        self.cursor_block = block;
    }

    /// After edits or motion: moves the folds, updates the list, shows the
    /// cursor.
    pub fn after_change(&mut self, cx: &mut Context<'_, Self>) {
        let changes = self.doc.take_changes();
        let now = Instant::now();
        for tx in &changes {
            self.folds.map(tx);
            self.debouncer
                .record(self.doc_id, self.doc.version(), tx, now);
        }
        let sel = (self.doc.selection.anchor, self.doc.selection.head);
        if self.sent_selection != Some(sel) || !changes.is_empty() {
            self.sent_selection = Some(sel);
            self.shared
                .bus
                .borrow_mut()
                .emit(&kalem_core::events::Event::SelectionChanged {
                    doc: self.doc_id,
                    anchor: sel.0,
                    head: sel.1,
                });
        }
        if let Some(o) = &mut self.outline {
            o.map(&changes);
        }
        if !changes.is_empty() && self.find.is_some() {
            self.refresh_matches();
        }
        self.reveal();
        self.sync_list(&changes);
        let line = self.doc.text().line_of(self.doc.selection.head);
        if let Some(i) = self.item_of(line) {
            self.list.scroll_to_reveal_item(i);
        }
        self.with_other(|e| {
            if let Some(i) = e.item_of(line) {
                e.list.scroll_to_reveal_item(i);
            }
        });
        cx.notify();
    }

    /// Unfolds the headlines that hide the cursor.
    fn reveal(&mut self) {
        let blocks = self.blocks();
        let c = self.doc.selection.head;
        let len = self.doc.text().len();
        loop {
            let visible = self.folds.visible(&blocks);
            if visible
                .iter()
                .any(|b| b.range.contains(&c) || (b.range.end == c && c == len))
            {
                return;
            }
            let Some(h) = blocks
                .iter()
                .filter(|b| matches!(b.kind, BlockKind::Heading { .. }) && b.range.start <= c)
                .filter(|b| self.folds.get(b.range.start).is_some())
                .map(|b| b.range.start)
                .next_back()
            else {
                return;
            };
            self.folds.set(h, None);
        }
    }

    /// What was typed in a list resumed (`SPC '`), once it is open.
    pub(crate) fn apply_resume(&mut self) {
        let input = self.resume_input.take();
        let Some(p) = &mut self.palette else {
            self.mark_picker = false;
            return;
        };
        if std::mem::take(&mut self.mark_picker) {
            p.resumable = true;
        }
        if let Some(input) = input {
            p.input = input;
            p.back = 0;
            p.input_changed();
        }
    }

    /// Keeps what is typed in the open list for `SPC '`.
    pub(crate) fn remember_picker(&mut self) {
        if let (Some(p), Some(last)) = (&self.palette, &mut self.shared.last.borrow_mut().picker)
            && p.resumable
        {
            last.1 = p.input.clone();
        }
    }

    pub(crate) fn message(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            tracing::warn!("{text}");
        }
        self.status = Some((text, error));
    }

    /// The when-clause context.
    pub fn context(&self) -> WhenContext {
        let mut c = self.doc.when_context();
        c.flag("editorFocus", true);
        c.flag("sourceView", self.source);
        c.flag("inProject", self.project().is_some());
        if let Some(v) = &self.vim {
            c.set("vimMode", WhenValue::Str(v.mode_name().into()));
            c.flag("vimCommand", v.idle_command() && v.command_line.is_none());
        }
        c
    }

    /// The theme the document is shown in.
    pub fn doc_theme(&self) -> Theme {
        self.theme.clone()
    }

    /// The folder of the project holding the document.
    pub fn project(&self) -> Option<std::path::PathBuf> {
        let projects = self.shared.projects.borrow();
        // A listing is in the project holding its folder.
        let listed = self.doc.dired.as_deref().and_then(|s| s.dir());
        projects
            .containing(self.doc.meta.path.as_deref().or(listed))
            .map(|p| p.root.clone())
    }

    /// Runs a command.
    pub fn run_command(
        &mut self,
        id: &str,
        args: Value,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let shared = self.shared.clone();
        let Some(cmd) = shared.registry.get(id) else {
            self.message(tr!("msg-unknown-command", id = id), true);
            return;
        };
        if let Some(w) = &cmd.when
            && !w.eval(&self.context())
        {
            self.message(
                tr!("msg-does-not-apply", command = cmd.display_title()),
                true,
            );
            cx.notify();
            return;
        }
        if let Some((name, ty)) = kalem_core::command::missing_argument(cmd, &args) {
            let date = cmd
                .args_schema
                .as_ref()
                .is_some_and(|s| s["properties"][name.as_str()]["format"].as_str() == Some("date"));
            if date {
                self.open_date_picker(id, args, name, cx);
            } else {
                self.ask_argument(id, &cmd.display_title(), args, name, ty, cx);
            }
            return;
        }
        let now = Instant::now();
        let clock = jiff::Zoned::now().datetime();
        let mut ctx = EditorContext::new(
            Some(&mut self.doc),
            &mut self.clipboard,
            &shared.config,
            now,
            clock,
        );
        let result = shared.registry.execute(id, &mut ctx, &args);
        let requests = std::mem::take(&mut ctx.requests);
        let messages = std::mem::take(&mut ctx.messages);
        drop(ctx);
        match result {
            Ok(()) => {
                self.status = messages.last().map(|m| (m.clone(), false));
            }
            Err(e) => self.message(e.message, true),
        }
        self.goal_x = None;
        self.after_change(cx);
        for r in requests {
            self.request(r, window, cx);
        }
    }

    fn request(&mut self, r: Request, window: &mut Window, cx: &mut Context<'_, Self>) {
        if r.is_picker() {
            self.shared.last.borrow_mut().picker = Some((r.clone(), String::new()));
            self.mark_picker = true;
        }
        if r.is_panel_toggle() {
            self.shared.last.borrow_mut().panel = Some(r.clone());
        }
        match r {
            Request::ResumePicker => {
                let last = self.shared.last.borrow().picker.clone();
                match last {
                    Some((r, input)) => {
                        self.resume_input = Some(input.clone());
                        self.request(r.clone(), window, cx);
                        self.shared.last.borrow_mut().picker = Some((r, input));
                    }
                    None => self.message(kalem_core::tr!("msg-no-picker"), false),
                }
            }
            Request::ToggleLastPanel => {
                let last = self.shared.last.borrow().panel.clone();
                match last {
                    Some(r) => self.request(r, window, cx),
                    None => self.message(kalem_core::tr!("msg-no-panel"), false),
                }
            }
            Request::Save => self.save(window, cx),
            Request::SaveAs => self.save_as(window, cx),
            Request::Quit => cx.emit(DocEvent::Quit),
            Request::Copy | Request::Cut => {
                let Some(text) = self.doc.copy_text() else {
                    return;
                };
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                self.clipboard.text = text;
                if r == Request::Cut {
                    self.doc.cut_selections(Instant::now());
                    self.after_change(cx);
                }
            }
            Request::Paste { plain } => {
                let item = cx.read_from_clipboard();
                // A picture without text (a screenshot), or copied picture
                // files: kept beside the document and linked.
                if !plain && let Some(item) = &item {
                    let has_text = item
                        .entries()
                        .iter()
                        .any(|e| matches!(e, gpui::ClipboardEntry::String(_)));
                    for e in item.entries() {
                        let done = match e {
                            gpui::ClipboardEntry::Image(img) if !has_text => {
                                Some(self.doc.paste_picture(
                                    &img.bytes,
                                    img.format.extension(),
                                    Instant::now(),
                                ))
                            }
                            gpui::ClipboardEntry::ExternalPaths(p)
                                if p.paths().iter().all(|f| kalem_core::images::is_image(f)) =>
                            {
                                Some(self.doc.drop_pictures(p.paths(), Instant::now()))
                            }
                            _ => None,
                        };
                        if let Some(r) = done {
                            if let Err(e) = r {
                                self.message(e, true);
                            }
                            self.after_change(cx);
                            return;
                        }
                    }
                }
                let text = item
                    .and_then(|i| i.text())
                    .unwrap_or_else(|| self.clipboard.text.clone());
                let html = if plain {
                    None
                } else {
                    (self.shared.html_clipboard)()
                };
                self.paste(&text, html.as_deref(), plain, cx);
            }
            Request::ToggleSource => {
                self.source = !self.source;
                self.list.reset(0);
                self.visible.clear();
                self.sync_pane(&[]);
                cx.notify();
            }
            Request::Split => self.toggle_split(cx),
            Request::ModeChanged => {
                self.blocks = None;
                self.folds = Folds::default();
                self.refresh_vim();
                self.list.reset(0);
                self.visible.clear();
                self.sync_pane(&[]);
                self.with_other(|e| {
                    e.list.reset(0);
                    e.visible.clear();
                    e.sync_pane(&[]);
                });
                cx.notify();
            }
            Request::Settings => self.open_settings(window, cx),
            Request::ToggleMath => {
                self.math = !self.math;
                self.message(
                    tr!(if self.math {
                        "msg-math-on"
                    } else {
                        "msg-math-off"
                    }),
                    false,
                );
                self.list.reset(0);
                self.visible.clear();
                self.sync_pane(&[]);
                self.with_other(|e| {
                    e.list.reset(0);
                    e.visible.clear();
                    e.sync_pane(&[]);
                });
                cx.notify();
            }
            Request::ToggleWrap => {
                self.wrap = !self.wrap;
                self.hscroll = px(0.);
                let m = if self.wrap {
                    "msg-wrap-on"
                } else {
                    "msg-wrap-off"
                };
                self.message(tr!(m), false);
                self.relayout();
                cx.notify();
            }
            Request::FullScreen => window.toggle_fullscreen(),
            Request::Terminal(dir) => {
                if let Err(e) = kalem_core::system::open_terminal(&dir) {
                    self.message(tr!("msg-no-terminal", error = e), true);
                }
            }
            Request::NewWindow => crate::workspace::open_window(None, self.shared.clone(), cx),
            Request::HelpBindings => {
                let items = kalem_core::palette::binding_items(
                    &self.shared.registry,
                    &self.shared.keymap,
                    |k| k.to_string(),
                );
                self.open_choice(items, cx);
            }
            Request::DescribeKey => self.describing = true,
            Request::ReloadSettings => self.reload_settings(cx),
            Request::Focus => {
                self.focus_mode = !self.focus_mode;
                let m = if self.focus_mode {
                    "msg-focus-on"
                } else {
                    "msg-focus-off"
                };
                self.message(tr!(m), false);
                self.sync_list(&[]);
                cx.notify();
            }
            Request::Fold { global } => self.fold(global, cx),
            Request::OpenLink(action) => self.open_link(action, cx),
            Request::Outline => self.toggle_outline(cx),
            Request::Preview { thumbnails } => {
                self.preview = (self.preview != Some(thumbnails)).then_some(thumbnails);
                cx.notify();
            }
            Request::Palette => self.open_palette(cx),
            Request::Menus => {
                let ctx = self.context();
                let swap = self.shared.swap_primary;
                let items = kalem_core::palette::menu_items(
                    &self.shared.registry,
                    &self.shared.keymap,
                    &ctx,
                    |k| crate::panels::show_keys(k, swap),
                );
                self.open_choice(items, cx);
            }
            Request::Find { replace } => self.open_find(replace, cx),
            Request::Open { path: Some(p) } => {
                let path = std::path::PathBuf::from(kalem_core::settings::expand_home(&p));
                let path = match kalem_core::command::folder_of(&self.doc) {
                    Some(d) if !path.is_absolute() => d.join(&path),
                    _ => path,
                };
                cx.emit(DocEvent::Open { path, at: None });
            }
            Request::Ask { command, args, arg } => {
                let title = self
                    .shared
                    .registry
                    .get(&command)
                    .map_or_else(|| command.clone(), |c| c.display_title());
                self.ask_argument(&command, &title, args, arg, "string".into(), cx);
            }
            Request::PickFile { command, arg, args } => {
                let dir = self
                    .doc
                    .meta
                    .path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(std::path::Path::to_path_buf);
                let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: None,
                });
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = paths.await
                        && let Some(p) = paths.into_iter().next()
                    {
                        // Relative to the document's folder, as LaTeX reads it.
                        let rel = dir
                            .as_deref()
                            .and_then(|d| kalem_core::kinds::relative(d, &p))
                            .unwrap_or_else(|| p.to_string_lossy().replace('\\', "/"));
                        let args = kalem_core::command::with_argument(args, &arg, rel.into());
                        let _ = this.update_in(cx, |e, window, cx| {
                            e.run_command(&command, args, window, cx);
                        });
                    }
                })
                .detach();
            }
            Request::Open { path: None } => {
                let dir = self
                    .doc
                    .meta
                    .path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(std::path::Path::to_path_buf);
                let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
                    files: true,
                    directories: true,
                    multiple: true,
                    prompt: None,
                });
                let _ = dir;
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = paths.await {
                        let _ = this.update(cx, |_, cx| {
                            for path in paths {
                                cx.emit(DocEvent::Open { path, at: None });
                            }
                        });
                    }
                })
                .detach();
            }
            Request::New => cx.emit(DocEvent::New),
            Request::Close => self.close(window, cx),
            Request::Cycle { back } => cx.emit(DocEvent::Cycle(back)),
            Request::Pick(kind) => cx.emit(DocEvent::Pick(
                kind,
                None,
                kalem_core::projects::After::Open,
            )),
            Request::SearchProject => cx.emit(DocEvent::Search(None)),
            Request::SearchIn(dir) => self.open_search_in(&dir, cx),
            Request::OpenFiles => cx.emit(DocEvent::ToggleFiles),
            Request::Project(r) => self.project_request(r, cx),
            Request::Documents(r) => cx.emit(DocEvent::Documents(r)),
            Request::SearchLines {
                all,
                headings,
                text,
            } => cx.emit(DocEvent::SearchLines {
                all,
                headings,
                text,
            }),
            Request::PickProject(after) => cx.emit(DocEvent::Pick(
                kalem_core::command::PickKind::Projects,
                None,
                after,
            )),
            Request::SearchProjectFor(text) => match self.project() {
                Some(root) => {
                    self.open_search(&root, cx);
                    if let Some(p) = self.palette.as_mut().filter(|p| p.search.is_some()) {
                        p.input = text;
                        p.back = 0;
                        p.input_changed();
                    }
                }
                None => self.message(tr!("msg-no-project"), true),
            },
            Request::SearchOtherProject => cx.emit(DocEvent::Pick(
                kalem_core::command::PickKind::Projects,
                None,
                kalem_core::projects::After::Search,
            )),
            Request::FileManager(r) => self.file_manager_request(r, cx),
            Request::FileOp(op) => match kalem_core::dired::Task::new(&op) {
                Ok(t) => self.ask_task(t, window, cx),
                Err(e) => self.message(e, true),
            },
            Request::Shell(op) => match kalem_core::dired::Task::shell(&op) {
                Ok(t) => self.ask_task(t, window, cx),
                Err(e) => self.message(e, true),
            },
            Request::CancelFileOps => {
                for j in self.shared.jobs.borrow().iter() {
                    j.cancel();
                }
            }
            Request::CopyText(t) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(t)),
            Request::Complete => {
                self.request_completion();
                cx.notify();
            }
            Request::CopyRich { html, text } => {
                if !crate::clipboard::write_rich(&html, &text) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.message(tr!("msg-rich-copy-plain"), false);
                }
            }
            Request::SetSetting { key, value, quiet } => {
                self.set_setting(&key, value, cx);
                if quiet {
                    self.status = None;
                }
            }
            // After a setting it changes is applied (`set_setting` defers).
            Request::Choose(items) => self.open_choice(items, cx),
            Request::ExportDialog => {
                let this = cx.entity();
                cx.defer(move |cx| this.update(cx, |e, cx| e.open_export_dialog(cx)));
            }
        }
    }

    /// Where the file manager goes for `r`.
    fn file_manager_request(
        &mut self,
        r: kalem_core::command::FileManagerRequest,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::command::FileManagerRequest as F;
        use kalem_core::dired::Place;
        let (place, select) = match r {
            F::Dir { dir: Some(d) } => (Place::Dir(d), None),
            F::Dir { dir: None } => match self.doc.meta.path.clone() {
                // From a listing: its parent, the cursor on it.
                Some(p) => {
                    let p = std::path::absolute(&p).unwrap_or(p);
                    (
                        Place::Dir(p.parent().map_or(p.clone(), std::path::Path::to_path_buf)),
                        Some(p),
                    )
                }
                None => (
                    Place::Dir(std::env::current_dir().unwrap_or_default()),
                    None,
                ),
            },
            F::ProjectRoot => match self.project() {
                Some(root) => (Place::Dir(root), self.doc.meta.path.clone()),
                None => {
                    self.message(tr!("msg-no-project"), true);
                    cx.notify();
                    return;
                }
            },
            F::Projects { select } => (Place::Projects, select),
            F::Leave => {
                cx.emit(DocEvent::LeaveFileManager);
                return;
            }
        };
        let place = match place {
            Place::Dir(d) => Place::Dir(kalem_core::projects::normal(&d)),
            p => p,
        };
        cx.emit(DocEvent::FileManager { place, select });
    }

    /// Asks a file operation's questions in dialogs, then starts it.
    fn ask_task(
        &mut self,
        mut task: kalem_core::dired::Task,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::dired::{Answer, Question};
        let (text, answers): (String, Vec<(String, Answer)>) = match task.question() {
            None => {
                let job = task.start();
                self.message(job.status(), false);
                self.shared.jobs.borrow_mut().push(job);
                cx.notify();
                return;
            }
            Some(Question::Confirm(q)) => (
                q,
                vec![
                    (tr!("fm-answer-yes"), Answer::Yes),
                    (tr!("fm-answer-no"), Answer::No),
                ],
            ),
            Some(Question::Conflict { text, more, .. }) => {
                let mut a = vec![
                    (tr!("fm-answer-overwrite"), Answer::Overwrite),
                    (tr!("fm-answer-skip"), Answer::Skip),
                    (tr!("fm-answer-keep-both"), Answer::KeepBoth),
                ];
                if more > 0 {
                    a.push((tr!("fm-answer-overwrite-all"), Answer::OverwriteAll));
                    a.push((tr!("fm-answer-skip-all"), Answer::SkipAll));
                    a.push((tr!("fm-answer-keep-both-all"), Answer::KeepBothAll));
                }
                a.push((tr!("dialog-cancel"), Answer::No));
                (text, a)
            }
        };
        let labels: Vec<&str> = answers.iter().map(|(l, _)| l.as_str()).collect();
        let reply = window.prompt(gpui::PromptLevel::Warning, &text, None, &labels, cx);
        let answers: Vec<Answer> = answers.iter().map(|(_, a)| *a).collect();
        cx.spawn_in(window, async move |this, cx| {
            let chosen = reply
                .await
                .ok()
                .and_then(|i| answers.get(i).copied())
                .unwrap_or(Answer::No);
            let _ = this.update_in(cx, |e, window, cx| {
                if task.answer(chosen) {
                    e.ask_task(task, window, cx);
                } else {
                    e.message(tr!("msg-cancelled"), false);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Opens a link target outside the document: addresses in the system,
    /// Org and text files in a new window, other files with their
    /// application.
    fn open_link(&mut self, action: kalem_core::input::LinkAction, cx: &mut Context<'_, Self>) {
        use kalem_core::input::LinkAction;
        match action {
            LinkAction::Url(url) => cx.open_url(&url),
            LinkAction::File { path, search } => {
                let path = std::path::PathBuf::from(path.trim_start_matches("file:"));
                let path = match (&self.doc.meta.path, path.is_absolute()) {
                    (Some(doc), false) => doc.parent().map_or(path.clone(), |d| d.join(&path)),
                    _ => path,
                };
                let text = matches!(
                    DocumentMode::detect(Some(&path), b""),
                    DocumentMode::Org
                        | DocumentMode::Markdown
                        | DocumentMode::Text { .. }
                        | DocumentMode::Csv
                        | DocumentMode::Latex
                );
                if text {
                    // A line number as the search: the cursor on that line.
                    let at = search
                        .as_deref()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(|l| (l, 0));
                    cx.emit(DocEvent::Open { path, at });
                } else {
                    cx.open_with_system(&path);
                }
            }
            LinkAction::Jump(p) => {
                self.doc.move_cursor(p, false);
                self.after_change(cx);
            }
            LinkAction::Missing(s) => self.message(tr!("msg-no-match-for", target = s), true),
            LinkAction::Print(pdf) => match kalem_core::print::run(&pdf) {
                Ok(m) => self.message(m, false),
                Err(e) => self.message(e, true),
            },
            LinkAction::System(path) => cx.open_with_system(&path),
            LinkAction::Reveal(path) => cx.reveal_path(&path),
        }
    }

    /// Pastes `text`, and `html` when the clipboard has it
    /// ([`kalem_core::DocumentState::paste`]).
    pub fn paste(
        &mut self,
        text: &str,
        html: Option<&str>,
        plain: bool,
        cx: &mut Context<'_, Self>,
    ) {
        self.completion = None;
        self.doc.paste(text, html, plain, Instant::now());
        self.goal_x = None;
        self.after_change(cx);
    }

    fn fold(&mut self, global: bool, cx: &mut Context<'_, Self>) {
        let blocks = self.blocks();
        if global {
            let any = self.folds.visible(&blocks).len() < blocks.len();
            self.folds = Folds::startup(&blocks, if any { "showall" } else { "overview" });
        } else {
            let c = self.doc.selection.head;
            if let Some(i) = blocks.iter().position(|b| {
                matches!(b.kind, BlockKind::Heading { .. })
                    && b.range.start <= c
                    && c <= b.content_end
            }) {
                self.folds.cycle(&blocks, i);
            }
        }
        self.sync_list(&[]);
        cx.notify();
    }

    /// Folds or unfolds the headline starting at `start`.
    pub fn toggle_fold(&mut self, start: usize, cx: &mut Context<'_, Self>) {
        let blocks = self.blocks();
        if let Some(i) = blocks.iter().position(|b| b.range.start == start) {
            self.folds.cycle(&blocks, i);
        }
        self.sync_list(&[]);
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.doc.meta.path.is_none() {
            self.save_as(window, cx);
            return;
        }
        let path = self.doc.meta.path.clone().unwrap_or_default();
        let event = kalem_core::events::Event::DocumentBeforeSave {
            doc: self.doc_id,
            path: path.clone(),
        };
        let outcome = self
            .shared
            .bus
            .borrow_mut()
            .emit_vetoable(&event, Instant::now())
            .wait();
        self.shared.bus.borrow_mut().settle(&event, &outcome);
        if let Some((_, reason)) = outcome.veto {
            self.message(tr!("msg-not-saved", reason = reason), true);
            return;
        }
        self.doc.before_save(&self.shared.config, Instant::now());
        self.after_change(cx);
        match self.doc.save(self.shared.config.save_options(), false) {
            Ok(()) => {
                self.shared
                    .bus
                    .borrow_mut()
                    .emit(&kalem_core::events::Event::DocumentAfterSave {
                        doc: self.doc_id,
                        path,
                    });
                self.disk_conflict = false;
                self.message(tr!("msg-saved"), false);
                // A LaTeX document builds on save when asked to, one build
                // at a time.
                if self.doc.latex().is_some()
                    && self.shared.config.bool("latex.build_on_save")
                    && !kalem_core::jobs::running()
                {
                    self.run_command("latex.build", serde_json::Value::Null, window, cx);
                }
            }
            Err(kalem_core::document::SaveError::ChangedOnDisk) => {
                let (overwrite, cancel) = (tr!("dialog-overwrite"), tr!("dialog-cancel"));
                let answer = window.prompt(
                    gpui::PromptLevel::Warning,
                    &tr!("dialog-changed-on-disk"),
                    Some(&tr!("dialog-overwrite-detail")),
                    &[overwrite.as_str(), cancel.as_str()],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    if answer.await == Ok(0) {
                        let _ = this.update(cx, |e, cx| {
                            if let Err(err) = e.doc.save(e.shared.config.save_options(), true) {
                                e.message(tr!("msg-not-saved", reason = err.to_string()), true);
                            }
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Err(e) => self.message(tr!("msg-not-saved", reason = e.to_string()), true),
        }
        cx.notify();
    }

    fn save_as(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) {
        let dir = self
            .doc
            .meta
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map_or_else(
                || std::env::current_dir().unwrap_or_default(),
                std::path::Path::to_path_buf,
            );
        let name = self
            .doc
            .meta
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        let chosen = cx.prompt_for_new_path(&dir, name.as_deref());
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = chosen.await {
                let _ = this.update(cx, |e, cx| {
                    match e.doc.save_as(&path, e.shared.config.save_options()) {
                        Ok(()) => e.message(
                            tr!("msg-saved-as", path = path.display().to_string()),
                            false,
                        ),
                        Err(err) => e.message(tr!("msg-not-saved", reason = err.to_string()), true),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Closes the document, asking about unsaved changes first.
    fn close(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !self.doc.is_modified() {
            cx.emit(DocEvent::Close);
            return;
        }
        let (save, discard, cancel) = (
            tr!("dialog-save"),
            tr!("dialog-dont-save"),
            tr!("dialog-cancel"),
        );
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &tr!("dialog-save-before-close", name = self.title()),
            None,
            &[save.as_str(), discard.as_str(), cancel.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let a = answer.await;
            let _ = this.update_in(cx, |e, window, cx| match a {
                Ok(0) => {
                    e.save(window, cx);
                    if !e.doc.is_modified() {
                        cx.emit(DocEvent::Close);
                    }
                }
                Ok(1) => cx.emit(DocEvent::Close),
                _ => {}
            });
        })
        .detach();
    }

    /// The document's title: its file name, or "Untitled".
    pub fn title(&self) -> String {
        if let Some(d) = self.doc.dired.as_deref() {
            return d.title();
        }
        self.doc
            .meta
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| tr!("untitled"), |n| n.to_string_lossy().into_owned())
    }

    /// The document as the list of open files shows it.
    pub fn open_file(&self) -> kalem_core::projects::OpenFile {
        match self.doc.dired.as_deref() {
            // The file manager is not listed under a project: its name
            // would show twice (the project, then its folder).
            Some(d) => kalem_core::projects::OpenFile {
                path: None,
                title: d.list_title(),
                modified: false,
            },
            None => kalem_core::projects::OpenFile {
                path: self.doc.meta.path.clone(),
                title: self.title(),
                modified: self.doc.is_modified(),
            },
        }
    }

    /// Saves the document if it has a file and unsaved changes; whether it
    /// is saved now.
    pub fn save_quietly(&mut self) -> bool {
        if !self.doc.is_modified() {
            return true;
        }
        if self.doc.meta.path.is_none() {
            return false;
        }
        match self.doc.save(self.shared.config.save_options(), false) {
            Ok(()) => true,
            Err(e) => {
                self.message(tr!("msg-not-saved", reason = e.to_string()), true);
                false
            }
        }
    }

    /// Changes of the project list, and actions on the project's documents.
    fn project_request(
        &mut self,
        r: kalem_core::command::ProjectRequest,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::command::ProjectRequest as P;
        let project = self.project();
        let result = match r {
            P::Add(Some(path)) => {
                let path = std::path::PathBuf::from(kalem_core::settings::expand_home(&path));
                self.shared.projects.borrow_mut().add(&path)
            }
            P::Add(None) => match self
                .doc
                .meta
                .path
                .as_deref()
                .and_then(std::path::Path::parent)
            {
                Some(dir) => {
                    let dir = dir.to_path_buf();
                    self.shared.projects.borrow_mut().add(&dir)
                }
                None => {
                    // An unsaved document: choose a folder.
                    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: None,
                    });
                    cx.spawn(async move |this, cx| {
                        if let Ok(Ok(Some(paths))) = paths.await
                            && let Some(p) = paths.into_iter().next()
                        {
                            let _ = this.update(cx, |e, cx| {
                                let r = e.shared.projects.borrow_mut().add(&p);
                                match r {
                                    Ok(m) => e.message(m, false),
                                    Err(m) => e.message(m, true),
                                }
                                cx.notify();
                            });
                        }
                    })
                    .detach();
                    return;
                }
            },
            P::Rename(name) => match &project {
                Some(root) => self.shared.projects.borrow_mut().rename(root, &name),
                None => Err(tr!("msg-no-project")),
            },
            P::Refresh => match &project {
                Some(root) => {
                    self.shared.projects.borrow_mut().refresh(root);
                    Ok(String::new())
                }
                None => Err(tr!("msg-no-project")),
            },
            P::RevealInTree => match (&project, self.doc.meta.path.clone()) {
                (Some(root), Some(path)) => {
                    self.shared
                        .projects
                        .borrow_mut()
                        .reveal_in_tree(root, &path);
                    cx.notify();
                    Ok(String::new())
                }
                _ => Err(tr!("msg-no-project")),
            },
            P::SaveAll => match project {
                Some(root) => {
                    cx.emit(DocEvent::SaveProject(root));
                    return;
                }
                None => Err(tr!("msg-no-project")),
            },
            P::CloseAll => match project {
                Some(root) => {
                    cx.emit(DocEvent::CloseProject(root));
                    return;
                }
                None => Err(tr!("msg-no-project")),
            },
        };
        match result {
            Ok(m) if m.is_empty() => {}
            Ok(m) => self.message(m, false),
            Err(m) => self.message(m, true),
        }
        cx.notify();
    }

    /// Handles a key press through the keymap; unbound keys type (through
    /// the input handler) or move the cursor.
    pub fn key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(chord) = crate::keys::chord_typed(&ev.keystroke, self.shared.swap_primary) else {
            return;
        };
        if self.marked.is_some() {
            return;
        }
        // Escape closes the context menu.
        if self.context_menu.is_some() {
            self.context_menu = None;
            cx.notify();
            if ev.keystroke.key == "escape" {
                cx.stop_propagation();
                return;
            }
        }
        // The palette and the date picker take every key; the focused find
        // bar its own.
        if self.date_picker.is_some() {
            if self.date_key(&ev.keystroke, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if self.settings.is_some() {
            if self.settings_key(&ev.keystroke, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if self.palette.is_some() {
            if self.palette_key(&ev.keystroke, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if self.find_key(&ev.keystroke, cx) {
            cx.stop_propagation();
            return;
        }
        if !self.describing
            && self.pending.is_empty()
            && !self.listing_key(&chord)
            && self.vim_key_down(ev, window, cx)
        {
            cx.stop_propagation();
            return;
        }
        if self.completion.is_some() && self.completion_key(&ev.keystroke, cx) {
            cx.stop_propagation();
            return;
        }
        if self.pending.is_empty() {
            self.pending_at = Some(Instant::now());
            self.hints_drawn = false;
        }
        self.pending.push(chord);
        let seq = KeySequence(self.pending.clone());
        let ctx = self.context();
        let shared = self.shared.clone();
        match shared.keymap.lookup(&seq, &ctx) {
            Lookup::Command { command, .. } if self.describing => {
                let title = shared
                    .registry
                    .get(command)
                    .map_or_else(|| command.to_string(), |c| c.display_title());
                self.pending.clear();
                self.describing = false;
                cx.stop_propagation();
                self.message(
                    tr!(
                        "help-key",
                        keys = seq.to_string(),
                        title = title,
                        id = command
                    ),
                    false,
                );
                cx.notify();
                return;
            }
            Lookup::None if self.describing => {
                self.pending.clear();
                self.describing = false;
                cx.stop_propagation();
                self.message(tr!("help-key-none", keys = seq.to_string()), false);
                cx.notify();
                return;
            }
            Lookup::Command { command, args } => {
                self.pending.clear();
                cx.stop_propagation();
                self.run_command(command, args.clone(), window, cx);
                self.last_command = Some(command.to_string());
                return;
            }
            Lookup::Prefix => {
                cx.stop_propagation();
                // The keys that may follow show in a panel at the bottom.
                self.message(format!("{seq} -"), false);
                cx.notify();
                return;
            }
            Lookup::None => {
                self.pending.clear();
                if seq.0.len() > 1 {
                    cx.stop_propagation();
                    self.message(tr!("msg-not-bound", keys = seq.to_string()), true);
                    cx.notify();
                    return;
                }
            }
        }
        if self.edit_key(&ev.keystroke, window, cx) {
            cx.stop_propagation();
        }
    }

    /// Whether the keymap takes `chord` before Vim: in a file manager
    /// listing outside Vim's insert mode and command line, the keys it
    /// binds.
    fn listing_key(&self, chord: &kalem_core::keys::KeyChord) -> bool {
        let Some(v) = &self.vim else { return false };
        if v.takes_text() || v.command_line.is_some() || !v.idle_command() {
            return false;
        }
        let seq = KeySequence(vec![chord.clone()]);
        let ctx = self.context();
        // Keys bound for the Vim layer (`-`), and the file manager's.
        self.shared.keymap.vim_bound(&seq, &ctx)
            || (self.doc.dired.is_some()
                && !matches!(self.shared.keymap.lookup(&seq, &ctx), Lookup::None))
    }

    /// Keys the keymap leaves alone: deleting, motion. Returns whether the
    /// key was used.
    fn edit_key(
        &mut self,
        k: &gpui::Keystroke,
        _window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let now = Instant::now();
        let m = k.modifiers;
        let shift = m.shift;
        let word = m.alt || (m.control && !self.shared.swap_primary);
        let line_motion = m.platform;
        let head = self.doc.selection.head;
        match k.key.as_str() {
            "enter" => {
                self.doc.type_text("\n", false, now);
                self.after_change(cx);
                true
            }
            "tab" if !matches!(self.doc.meta.mode, DocumentMode::Org) => {
                self.doc.indent(k.modifiers.shift, now);
                self.after_change(cx);
                true
            }
            "backspace" => {
                if let Some(m) = self.doc.delete_backward(now) {
                    self.message(m, false);
                }
                self.goal_x = None;
                self.after_change(cx);
                if self.completion.is_some() {
                    self.update_completion();
                }
                true
            }
            "delete" => {
                if let Some(m) = self.doc.delete_forward(now) {
                    self.message(m, false);
                }
                self.goal_x = None;
                self.after_change(cx);
                if self.completion.is_some() {
                    self.update_completion();
                }
                true
            }
            "escape" => {
                self.doc.move_cursor(head, false);
                self.doc.clear_extra();
                self.status = None;
                self.after_change(cx);
                true
            }
            key @ ("left" | "right" | "up" | "down" | "pageup" | "pagedown" | "home" | "end") => {
                let vertical = matches!(key, "up" | "down" | "pageup" | "pagedown") && !line_motion;
                if self.doc.extra.is_empty() {
                    let Some(t) = self.motion_target(key, shift, word, line_motion) else {
                        return false;
                    };
                    self.doc.move_cursor(t, shift);
                } else {
                    // Every cursor moves.
                    let (all, primary) = self.doc.cursors();
                    let mut moved = Vec::with_capacity(all.len());
                    for s in all {
                        self.doc.selection = s;
                        self.goal_x = None;
                        if let Some(t) = self.motion_target(key, shift, word, line_motion) {
                            self.doc.move_cursor(t, shift);
                        }
                        moved.push(self.doc.selection);
                    }
                    self.doc.set_cursors(moved, primary);
                }
                if !vertical {
                    self.goal_x = None;
                }
                self.after_change(cx);
                true
            }
            _ => false,
        }
    }

    /// Where a motion key takes the cursor, `None` for other keys:
    /// `word` moves by words, `line_motion` to the line's (or the
    /// document's) ends.
    fn motion_target(
        &mut self,
        key: &str,
        shift: bool,
        word: bool,
        line_motion: bool,
    ) -> Option<usize> {
        let head = self.doc.selection.head;
        let text = self.doc.text();
        let line = text.line_of(head);
        Some(match key {
            "left" | "right" if line_motion => {
                let r = text.line_range(line);
                if key == "left" { r.start } else { r.end }
            }
            "left" | "right" if word => self.word(key == "right"),
            "left" => {
                let s = self.doc.selection;
                if s.anchor != s.head && !shift {
                    s.anchor.min(s.head)
                } else {
                    self.horizontal(false)
                }
            }
            "right" => {
                let s = self.doc.selection;
                if s.anchor != s.head && !shift {
                    s.anchor.max(s.head)
                } else {
                    self.horizontal(true)
                }
            }
            "up" | "down" if line_motion => {
                if key == "up" {
                    0
                } else {
                    text.len()
                }
            }
            "up" | "down" => self.vertical(if key == "up" { -1 } else { 1 }),
            "pageup" | "pagedown" => {
                let rows = (f32::from(self.list.viewport_bounds().size.height)
                    / (self.theme.size * 1.45)) as isize;
                let d = rows.max(1) * if key == "pageup" { -1 } else { 1 };
                self.vertical(d)
            }
            "home" => text.line_range(line).start,
            "end" => text.line_range(line).end,
            _ => return None,
        })
    }

    /// Word motion: the start of the next or previous word.
    fn word(&self, forward: bool) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        let text = self.doc.text().as_str();
        let pos = self.doc.selection.head;
        if forward {
            let mut seen = false;
            for (i, w) in text[pos..].split_word_bound_indices() {
                let word = w.chars().any(char::is_alphanumeric);
                if seen && word {
                    return pos + i;
                }
                seen = true;
            }
            text.len()
        } else {
            text[..pos]
                .split_word_bound_indices()
                .rev()
                .find(|(_, w)| w.chars().any(char::is_alphanumeric))
                .map_or(0, |(i, _)| i)
        }
    }

    /// Brings the plain text highlighting and indentation step up to date,
    /// for documents that are not Org.
    pub fn update_plain(&self) {
        if self.doc.meta.mode == DocumentMode::Org {
            return;
        }
        // A file manager listing has its own colors (`line::color_listing`).
        if self.doc.dired.is_some() {
            *self.plain.borrow_mut() = Some((self.doc.version(), None, 0));
            return;
        }
        let mut p = self.plain.borrow_mut();
        if p.as_ref().is_some_and(|(v, ..)| *v == self.doc.version()) {
            return;
        }
        let text = self.doc.text().as_str();
        let lang = match &self.doc.meta.mode {
            DocumentMode::Text { language: Some(l) } => Some(l.as_str()),
            DocumentMode::Markdown => Some("md"),
            DocumentMode::Latex => Some("latex"),
            _ => None,
        };
        // Very large files are colored a window at a time (T2.7a.3).
        let found = lang.and_then(kalem_highlight::Language::find);
        let large = text.len() > 4 << 20;
        let language = found.filter(|_| !large);
        {
            let mut w = self.windowed.borrow_mut();
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
        *p = Some((self.doc.version(), h, step));
    }

    /// Whether lines show numbers: plain text and the source view.
    pub fn line_numbers(&self) -> bool {
        self.shared.config.bool("editor.line_numbers")
            && self.doc.dired.is_none()
            && (self.doc.meta.mode != DocumentMode::Org || self.source)
    }

    /// Keeps the caret in view sideways when lines do not wrap, from the
    /// last frame's layout; `true` if the view moved.
    fn follow_sideways(&mut self) -> bool {
        if self.wrap {
            let moved = self.hscroll != px(0.);
            self.hscroll = px(0.);
            return moved;
        }
        let head = self.doc.selection.head;
        let line = self.doc.text().line_of(head);
        let painted = self.painted.borrow();
        let Some(p) = painted.get(&line) else {
            return false;
        };
        let x = p.layout.caret(p.view.display_offset(head)).origin.x;
        let w = p.bounds.size.width;
        let old = self.hscroll;
        let new = if x < old + px(8.) {
            (x - w / 4.).max(px(0.))
        } else if x > old + w - px(16.) {
            x - w + w / 4.
        } else {
            old
        };
        drop(painted);
        self.hscroll = new;
        new != old
    }

    /// The line view of source line `line` with the cursor, as displayed.
    pub fn line_view(&self, line: usize) -> LineView {
        // A fault in a view shows the line as its text rather than ending
        // the program (a panic cannot unwind out of the frame callback).
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.line_view_of(line))) {
            Ok(v) => v,
            Err(_) => {
                tracing::error!(line, "the line's view failed; shown as text");
                let text = self.doc.text();
                let mut v = view::plain_line_view(text.as_str(), text.line_range(line), None);
                v.mono = true;
                v
            }
        }
    }

    fn line_view_of(&self, line: usize) -> LineView {
        let text = self.doc.text();
        let mut range = text.line_range(line);
        if range.end > range.start && text.as_str().as_bytes()[range.end - 1] == b'\r' {
            range.end -= 1;
        }
        // A very long line, in any mode: the part around the cursor, as
        // it is (laying out all of it would take seconds).
        if range.len() > view::LONG_LINE {
            let mut v = view::plain_line_view(text.as_str(), range, Some(self.doc.selection.head));
            v.mono = self.doc.meta.mode != DocumentMode::Org;
            return v;
        }
        // LaTeX: the document as it reads (the source view shows the text).
        if self.doc.meta.mode == DocumentMode::Latex && !self.source {
            return kalem_core::latex_view::line_view(
                &self.doc,
                range,
                Some(self.doc.selection.head),
            );
        }
        // CSV: a row of the grid (the source view shows the text).
        // BibTeX: an entry as a row of the grid, away from the cursor.
        if kalem_core::bibtex::is_bib(&self.doc) && !self.source {
            return kalem_core::bibtex::line_view(&self.doc, range, Some(self.doc.selection.head));
        }
        if self.doc.meta.mode == DocumentMode::Csv && !self.source {
            let layout = kalem_core::csv::layout(&self.doc);
            return kalem_core::csv::line_view(&layout, text.as_str(), range);
        }
        // Markdown: as it reads, markers hidden away from the cursor.
        if self.doc.meta.mode == DocumentMode::Markdown && !self.source {
            return kalem_core::markdown::line_view(
                &self.doc,
                range,
                Some(self.doc.selection.head),
            );
        }
        match self.doc.parse() {
            Some((p, true)) if self.source => {
                view::source_line_view(&p.syntax(), p.context(), text.as_str(), range)
            }
            Some((p, true)) => {
                let table = text.as_str()[range.clone()].trim_start().starts_with('|');
                let root = p.syntax();
                view::line_view_with(
                    &root,
                    p.context(),
                    range,
                    Some(self.doc.selection.head),
                    table,
                )
            }
            // Plain text is monospace, as the source view; a very long
            // line shows the part around the cursor.
            _ => {
                let mut v =
                    view::plain_line_view(text.as_str(), range, Some(self.doc.selection.head));
                v.mono = self.doc.meta.mode != DocumentMode::Org;
                // LaTeX's source view: the diagnostics flagged too.
                kalem_core::latex_view::flag_diagnostics(&self.doc, &mut v);
                v
            }
        }
    }

    /// One grapheme left or right in the display, skipping hidden markup.
    fn horizontal(&self, right: bool) -> usize {
        let head = self.doc.selection.head;
        let text = self.doc.text();
        let line = text.line_of(head);
        let range = text.line_range(line);
        let v = self.line_view(line);
        let step = if right {
            v.next_position(head)
        } else {
            v.prev_position(head)
        };
        match step {
            Some(p) if p != head => p,
            _ if right && head < range.end => self.doc.grapheme_after(head),
            _ if !right && head > range.start => self.doc.grapheme_before(head),
            _ if right => self.doc.grapheme_after(head).min(text.len()),
            _ => self.doc.grapheme_before(head),
        }
    }

    /// The source offset `delta` rows down (or up) at the kept column,
    /// from the painted lines; lines off screen are entered at their start.
    fn vertical(&mut self, delta: isize) -> usize {
        let head = self.doc.selection.head;
        let text = self.doc.text();
        let line = text.line_of(head);
        let painted = self.painted.borrow();
        let Some(p) = painted.get(&line) else {
            return self.vertical_by_lines(delta);
        };
        let caret = p.layout.caret(p.view.display_offset(head));
        let x = *self.goal_x.get_or_insert(caret.origin.x);
        let mut y = p.bounds.origin.y + caret.origin.y + caret.size.height / 2.;
        let step = caret.size.height;
        for _ in 0..delta.unsigned_abs() {
            y += if delta > 0 { step } else { -step };
        }
        let point = Point::new(p.bounds.origin.x + x, y);
        let hit = painted
            .values()
            .find(|q| q.bounds.top() <= y && y < q.bounds.bottom());
        match hit {
            Some(q) => {
                let d = q.layout.index_for_position(point - q.bounds.origin);
                q.view.source_offset(d)
            }
            None => {
                drop(painted);
                self.vertical_by_lines(delta)
            }
        }
    }

    fn vertical_by_lines(&self, delta: isize) -> usize {
        let text = self.doc.text();
        let line = text.line_of(self.doc.selection.head);
        let Some(i) = self.item_of(line) else {
            return self.doc.selection.head;
        };
        let j = (i as isize + delta).clamp(0, self.visible.len() as isize - 1) as usize;
        let target = self.visible[j];
        if delta > 0 && j == i {
            return text.line_range(line).end;
        }
        text.line_start(target)
    }

    /// The source offset at a window position, and the widget there.
    pub fn hit(&self, pos: Point<Pixels>) -> Option<Hit> {
        let painted = self.painted.borrow();
        for p in painted.values() {
            if let Some((b, start)) = &p.fold
                && b.contains(&pos)
            {
                return Some(Hit {
                    pos: p.view.range.start,
                    widget: None,
                    fold: Some(*start),
                    copy: None,
                    jump: None,
                });
            }
            if let Some((_, start)) = p.buttons.iter().find(|(b, _)| b.contains(&pos)) {
                return Some(Hit {
                    pos: p.view.range.start,
                    widget: None,
                    fold: None,
                    copy: Some(*start),
                    jump: None,
                });
            }
            if let Some((_, start)) = p.jumps.iter().find(|(b, _)| b.contains(&pos)) {
                return Some(Hit {
                    pos: p.view.range.start,
                    widget: None,
                    fold: None,
                    copy: None,
                    jump: Some(*start),
                });
            }
        }
        let p = painted
            .values()
            .find(|p| p.bounds.top() <= pos.y && pos.y < p.bounds.bottom())
            .or_else(|| {
                // Below the last line: its end.
                painted
                    .values()
                    .max_by(|a, b| {
                        a.bounds
                            .top()
                            .partial_cmp(&b.bounds.top())
                            .expect("ordered")
                    })
                    .filter(|p| pos.y >= p.bounds.bottom())
            })?;
        for (b, src, w) in &p.widgets {
            if b.contains(&pos) {
                return Some(Hit {
                    pos: src.start,
                    widget: Some((src.clone(), w.clone())),
                    fold: None,
                    copy: None,
                    jump: None,
                });
            }
        }
        let d = p.layout.index_for_position(pos - p.bounds.origin);
        Some(Hit {
            pos: p.view.source_offset(d),
            widget: None,
            fold: None,
            copy: None,
            jump: None,
        })
    }

    /// Mouse down: place the cursor, toggle a checkbox, fold a heading.
    pub fn mouse_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        window.focus(&self.focus, cx);
        self.palette = None;
        self.date_picker = None;
        self.settings = None;
        self.context_menu = None;
        if let Some(f) = &mut self.find {
            f.focused = false;
        }
        let Some(Hit {
            pos,
            widget,
            fold,
            copy,
            jump,
        }) = self.hit(ev.position)
        else {
            return;
        };
        if let Some(start) = jump {
            self.jump_to(start, window, cx);
            return;
        }
        if let Some(start) = copy {
            self.copy_block(start, cx);
            return;
        }
        if let Some(start) = fold {
            self.toggle_fold(start, cx);
            return;
        }
        // The file manager: Command-click (Control-click elsewhere) marks
        // or unmarks an entry, Shift-click marks the entries from the
        // cursor to it.
        let toggle = if cfg!(target_os = "macos") {
            ev.modifiers.platform
        } else {
            ev.modifiers.control
        };
        if let Some(d) = self.doc.dired.as_deref()
            && (toggle || ev.modifiers.shift)
            && d.path_at(self.doc.text().line_of(pos)).is_some()
        {
            let line = self.doc.text().line_of(pos);
            let marked = d.path_at(line).is_some_and(|p| d.marks.contains_key(&p));
            let at = self.doc.selection.head;
            if ev.modifiers.shift {
                self.doc.move_cursor(at, false);
                self.doc.move_cursor(pos, true);
                self.run_command("dired.mark", Value::Null, window, cx);
            } else {
                self.doc.move_cursor(pos, false);
                let id = if marked { "dired.unmark" } else { "dired.mark" };
                self.run_command(id, Value::Null, window, cx);
            }
            let line_start = self.doc.text().line_start(line);
            self.doc.move_cursor(line_start, false);
            self.after_change(cx);
            return;
        }
        // Command-click (Control-click elsewhere) opens a link.
        let open = if cfg!(target_os = "macos") {
            ev.modifiers.platform
        } else {
            ev.modifiers.control
        };
        if open && widget.is_none() {
            self.doc.move_cursor(pos, false);
            let id = if self.doc.latex().is_some() {
                "latex.link.open"
            } else if self.doc.meta.mode == DocumentMode::Markdown {
                "markdown.openLink"
            } else {
                "org.link.open"
            };
            self.run_command(id, Value::Null, window, cx);
            return;
        }
        if let Some((src, Widget::Checkbox(_))) = widget {
            self.doc.move_cursor(src.start, false);
            let id = if self.doc.meta.mode == DocumentMode::Markdown {
                "markdown.toggleCheckbox"
            } else {
                "list.toggleCheckbox"
            };
            self.run_command(id, Value::Null, window, cx);
            return;
        }
        // A click on a formula edits its source: the cursor goes inside.
        if let Some((src, Widget::Math { source, .. })) = &widget {
            let open = if ["$$", "\\(", "\\["].iter().any(|d| source.starts_with(d)) {
                2
            } else {
                usize::from(source.starts_with('$'))
            };
            self.doc.move_cursor((src.start + open).min(src.end), false);
            self.goal_x = None;
            self.after_change(cx);
            return;
        }
        // The file manager: a click on a name (or a double click on its
        // line) opens it.
        if let Some(d) = self.doc.dired.as_deref()
            && !ev.modifiers.shift
        {
            let text = self.doc.text();
            let line = text.line_of(pos);
            let col = pos - text.line_start(line);
            let on_name = d
                .name_range(line)
                .is_some_and(|r| r.start <= col && col <= r.end);
            if (on_name || ev.click_count >= 2) && d.path_at(line).is_some() {
                self.doc.move_cursor(pos, false);
                self.goal_x = None;
                self.after_change(cx);
                self.run_command("dired.open", Value::Null, window, cx);
                return;
            }
        }
        // Alt-click: a cursor more (or one fewer).
        if ev.modifiers.alt && ev.click_count == 1 && !ev.modifiers.shift {
            self.doc.toggle_cursor_at(pos);
            self.goal_x = None;
            self.after_change(cx);
            return;
        }
        self.doc.clear_extra();
        match ev.click_count {
            2 => self.select_word(pos),
            3 => {
                let text = self.doc.text();
                let r = text.line_range(text.line_of(pos));
                let end = (r.end + 1).min(text.len());
                self.doc.move_cursor(r.start, false);
                self.doc.move_cursor(end, true);
            }
            _ => self.doc.move_cursor(pos, ev.modifiers.shift),
        }
        self.dragging = true;
        self.goal_x = None;
        self.after_change(cx);
    }

    /// A right click in the file manager: the context menu for the entry
    /// under the mouse (the marked entries when it is one of them), or for
    /// the listing (T2.7e.17).
    pub fn right_mouse_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(d) = self.doc.dired.as_deref() else {
            return;
        };
        let at = self.hit(ev.position).map(|h| h.pos);
        let path = at.and_then(|p| d.path_at(self.doc.text().line_of(p)));
        let on_entry = path.is_some();
        // A right click on an unmarked entry acts on it alone.
        if let (Some(p), Some(pos)) = (&path, at)
            && !d.marks.contains_key(p)
        {
            self.doc.move_cursor(pos, false);
            self.after_change(cx);
        }
        let items = kalem_core::dired::context_menu(&self.doc, on_entry);
        self.context_menu = Some((ev.position, items));
        cx.notify();
    }

    /// The context menu, when open.
    fn context_menu_view(&self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        use gpui::{
            InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div,
        };
        let (at, items) = self.context_menu.clone()?;
        let theme = self.theme.clone();
        let hover = gpui::hsla(0., 0., 0.5, 0.15);
        let mut list = div()
            .id("context-menu")
            .occlude()
            .min_w(px(220.))
            .py(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.bar)
            .text_size(px(theme.size * 0.85));
        for (n, it) in items.into_iter().enumerate() {
            match it {
                kalem_core::dired::ContextItem::Separator => {
                    list = list.child(div().h(px(1.)).my(px(3.)).bg(theme.border));
                }
                kalem_core::dired::ContextItem::Command {
                    label,
                    id,
                    args,
                    enabled,
                } => {
                    let keys = self
                        .shared
                        .keymap
                        .keys_for(id)
                        .first()
                        .map(|k| crate::panels::show_keys(k, self.shared.swap_primary))
                        .unwrap_or_default();
                    let mut row = div()
                        .id(("context-item", n))
                        .debug_selector(move || format!("context-{n}"))
                        .flex()
                        .flex_row()
                        .justify_between()
                        .gap(px(24.))
                        .px(px(10.))
                        .py(px(2.))
                        .child(label)
                        .child(div().text_color(theme.muted).child(keys));
                    if enabled {
                        row =
                            row.cursor_pointer()
                                .hover(move |s| s.bg(hover))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.context_menu = None;
                                    this.run_command(id, args.clone(), window, cx);
                                }));
                    } else {
                        row = row.text_color(theme.muted);
                    }
                    list = list.child(row);
                }
            }
        }
        Some(
            gpui::deferred(gpui::anchored().position(at).child(list.on_mouse_down_out(
                cx.listener(|this, _, _, cx| {
                    this.context_menu = None;
                    cx.notify();
                }),
            )))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// Copies the content of the block starting at `start`.
    pub fn copy_block(&mut self, start: usize, cx: &mut Context<'_, Self>) {
        let Some(b) = self.block_at(start) else {
            return;
        };
        let text = self.doc.text();
        let first = text.line_of(b.range.start);
        let last = text.line_of(b.content_end.saturating_sub(1).max(b.range.start));
        let (a, z) = (text.line_start(first + 1), text.line_start(last));
        if a < z {
            let code = text.as_str()[a..z].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
            self.clipboard.text = code;
            self.message(tr!("msg-copied"), false);
            cx.notify();
        }
    }

    /// Files dropped on the editor: pictures are linked where they fall
    /// (copied beside the document when they are elsewhere), other files
    /// open.
    pub fn drop_paths(
        &mut self,
        paths: &[std::path::PathBuf],
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let pictures = self.doc.meta.mode == kalem_core::mode::DocumentMode::Org
            && !paths.is_empty()
            && paths.iter().all(|f| kalem_core::images::is_image(f));
        if !pictures {
            for p in paths {
                cx.emit(DocEvent::Open {
                    path: p.clone(),
                    at: None,
                });
            }
            return;
        }
        if let Some(Hit { pos, .. }) = self.hit(window.mouse_position()) {
            self.doc.move_cursor(pos, false);
        }
        if let Err(e) = self.doc.drop_pictures(paths, Instant::now()) {
            self.message(e, true);
        }
        self.after_change(cx);
    }

    /// The picture an image link's `path` names, relative to the
    /// document's folder.
    pub fn picture(&self, path: &str) -> Option<crate::pictures::Picture> {
        let base = self
            .doc
            .meta
            .path
            .as_deref()
            .and_then(std::path::Path::parent);
        self.shared
            .pictures
            .get(&kalem_core::images::resolve(path, base))
    }

    /// Dragging extends the selection.
    pub fn mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.dragging || ev.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            // The entry a citation under the mouse cites, or a footnote's
            // text.
            let hover = self
                .hit(ev.position)
                .and_then(|h| {
                    let path = self.doc.meta.path.clone();
                    match self.doc.model() {
                        Some(model) => kalem_core::cite::note_at(&model, path.as_deref(), h.pos),
                        None => kalem_core::latex_view::note_at(&self.doc, h.pos)
                            .or_else(|| kalem_core::latex_view::diagnostic_at(&self.doc, h.pos)),
                    }
                })
                .map(|t| (ev.position, t));
            if hover.as_ref().map(|h| &h.1) != self.cite_hover.as_ref().map(|h| &h.1) {
                self.cite_hover = hover;
                cx.notify();
            }
            return;
        }
        if let Some(Hit { pos, .. }) = self.hit(ev.position)
            && pos != self.doc.selection.head
        {
            self.doc.move_cursor(pos, true);
            self.after_change(cx);
        }
    }

    fn select_word(&mut self, pos: usize) {
        use unicode_segmentation::UnicodeSegmentation;
        let text = self.doc.text();
        let line = text.line_range(text.line_of(pos));
        let s = &text.as_str()[line.clone()];
        for (i, w) in s.split_word_bound_indices() {
            let (a, b) = (line.start + i, line.start + i + w.len());
            if a <= pos && pos < b && w.chars().any(char::is_alphanumeric) {
                self.doc.move_cursor(a, false);
                self.doc.move_cursor(b, true);
                return;
            }
        }
        self.doc.move_cursor(pos, false);
    }

    /// The highlighting of the source block `b`, if its language is known.
    pub fn code_spans(&self, b: &Block) -> Option<(usize, CodeSpans)> {
        let lang = b.kind.highlight_language()?.to_string();
        let lang = &lang;
        let text = self.doc.text();
        let first = text.line_of(b.range.start);
        let last = text.line_of(b.content_end.saturating_sub(1).max(b.range.start));
        let start = text.line_start(first + 1).min(b.content_end);
        {
            let mut c = self.code.borrow_mut();
            if c.0 != self.doc.version() {
                *c = (self.doc.version(), HashMap::new());
            }
            if let Some(v) = c.1.get(&b.range.start) {
                return v.clone().map(|v| (start, v));
            }
        }
        let end = text.line_start(last).max(start);
        let spans = kalem_highlight::Language::find(lang)
            .map(|l| Arc::new(kalem_highlight::highlight(l, &text.as_str()[start..end])));
        self.code
            .borrow_mut()
            .1
            .insert(b.range.start, spans.clone());
        spans.map(|s| (start, s))
    }

    /// The block holding line start `s`.
    pub fn block_at(&mut self, s: usize) -> Option<Block> {
        let blocks = self.blocks();
        let i = blocks.partition_point(|b| b.range.end <= s);
        blocks.get(i).filter(|b| b.range.start <= s).cloned()
    }

    /// Background work: a finished background parse restyles the lines.
    pub fn tick(&mut self, cx: &mut Context<'_, Self>) {
        self.tick_palette(cx);
        // The which-key panel once its delay has passed.
        if !self.pending.is_empty()
            && let Some(d) =
                kalem_core::keymap::hints_due(&self.shared.config, self.pending_at, Instant::now())
            && d.is_zero()
            && !self.hints_drawn
        {
            self.hints_drawn = true;
            cx.notify();
        }
        // Events: edits after their pause, and those raised elsewhere.
        {
            let mut bus = self.shared.bus.borrow_mut();
            for e in self.debouncer.due(Instant::now()) {
                bus.emit(&e);
            }
            bus.dispatch_queued();
            for w in bus.take_warnings() {
                tracing::warn!("{w}");
            }
        }
        // Items of slow completers.
        if let Some(m) = &mut self.completion
            && m.session.waiting()
            && m.session.poll()
        {
            cx.notify();
        }
        // Work commands started in the background (a PDF compiling).
        for f in kalem_core::jobs::take_finished() {
            self.message(f.message, f.error);
            if let Some(a) = f.open {
                self.open_link(a, cx);
            }
            cx.notify();
        }
        // File operations: progress while they run, then the result.
        if !self.shared.jobs.borrow().is_empty() {
            let mut done = Vec::new();
            self.shared.jobs.borrow_mut().retain_mut(|j| {
                let kind = j.kind();
                match j.poll() {
                    Some((out, message, error)) => {
                        done.push((message, error, kind.map(|k| (k, out))));
                        false
                    }
                    None => true,
                }
            });
            for (message, error, outcome) in done {
                cx.emit(DocEvent::FilesChanged {
                    message,
                    error,
                    outcome,
                });
            }
            cx.notify();
        }
        // Word counts catch up after a pause in typing.
        if self.words.borrow().due(&self.doc) {
            cx.notify();
        }
        if self.disk_checked.elapsed() >= std::time::Duration::from_secs(1) {
            self.check_disk(cx);
        }
        if self.doc.poll() {
            self.blocks = None;
            self.visible = self.compute_visible();
            self.list.reset(self.visible.len());
            cx.notify();
        }
    }

    /// Reloads the file when another program changed it and the document
    /// has no unsaved changes; says so when it has some.
    pub fn check_disk(&mut self, cx: &mut Context<'_, Self>) {
        use kalem_core::document::ExternalChange;
        self.disk_checked = Instant::now();
        let version = self.doc.version();
        let change = self.doc.external_change(Instant::now());
        // Another program changed the file: `workspace:file-changed`.
        if let (
            Ok(ExternalChange::Reloaded | ExternalChange::Conflict | ExternalChange::Deleted),
            Some(path),
        ) = (&change, &self.doc.meta.path)
            && !self.disk_conflict
        {
            self.shared
                .bus
                .borrow_mut()
                .emit(&kalem_core::events::Event::WorkspaceFileChanged { path: path.clone() });
        }
        match change {
            // Saved or reverted since a conflict.
            Ok(ExternalChange::None) => self.disk_conflict = false,
            Ok(ExternalChange::Reloaded) => {
                self.disk_conflict = false;
                self.message(tr!("msg-reloaded"), false);
                self.after_change(cx);
            }
            Ok(ExternalChange::Listing) => {
                if self.doc.version() != version {
                    self.after_change(cx);
                }
            }
            Ok(ExternalChange::Conflict) => {
                if !self.disk_conflict {
                    self.disk_conflict = true;
                    self.message(tr!("msg-disk-conflict"), true);
                    cx.notify();
                }
            }
            Ok(ExternalChange::Deleted) => {
                if !self.disk_conflict {
                    self.disk_conflict = true;
                    self.message(tr!("msg-deleted-on-disk"), true);
                    cx.notify();
                }
            }
            Err(e) => {
                self.message(tr!("msg-cannot-read", error = e.to_string()), true);
                cx.notify();
            }
        }
    }

    // IME support: offsets in the input handler are UTF-16 offsets within
    // the cursor's line.

    fn ime_line(&self) -> Range<usize> {
        let text = self.doc.text();
        text.line_range(text.line_of(self.doc.selection.head))
    }

    fn utf16_offset(&self, line: &Range<usize>, offset: usize) -> usize {
        self.doc.text().as_str()[line.start..offset.clamp(line.start, line.end)]
            .encode_utf16()
            .count()
    }

    fn offset_from_utf16(&self, line: &Range<usize>, u: usize) -> usize {
        let s = &self.doc.text().as_str()[line.clone()];
        let mut n = 0;
        for (i, c) in s.char_indices() {
            if n >= u {
                return line.start + i;
            }
            n += c.len_utf16();
        }
        line.end
    }

    fn replace(&mut self, range: Range<usize>, text: &str, cx: &mut Context<'_, Self>) {
        let now = Instant::now();
        let blank = matches!(
            self.last_command.as_deref(),
            Some("table.nextField" | "table.previousField" | "table.nextRow" | "table.align")
        );
        self.last_command = None;
        if range.is_empty() && range.start == self.doc.selection.head && text.chars().count() == 1 {
            // A typed character: Org's typing rules.
            if self.doc.selection.anchor == self.doc.selection.head {
                // In a CSV grid a delimiter or quote goes into the value.
                if self.source || !self.doc.type_in_grid(text, now) {
                    self.doc.type_text(text, blank, now);
                }
            } else {
                self.doc.insert_text(text, now);
            }
        } else {
            self.doc.move_cursor(range.start, false);
            self.doc.move_cursor(range.end, true);
            self.doc.insert_text(text, now);
        }
        self.goal_x = None;
        self.after_change(cx);
        self.update_completion();
    }

    /// Opens, updates or closes the completion menu after typing.
    pub fn update_completion(&mut self) {
        self.completion = kalem_core::completers::Menu::update(
            self.completion.take(),
            &self.shared.completers,
            &mut self.doc,
            false,
        );
    }

    /// Opens the completion menu on request (Alt+/).
    pub fn request_completion(&mut self) {
        self.completion = kalem_core::completers::Menu::update(
            self.completion.take(),
            &self.shared.completers,
            &mut self.doc,
            true,
        );
        if self.completion.is_none() {
            self.message(tr!("msg-no-completions"), false);
        }
    }

    /// Keys for the completion menu; `true` if used.
    fn completion_key(&mut self, k: &gpui::Keystroke, cx: &mut Context<'_, Self>) -> bool {
        let Some(m) = &mut self.completion else {
            return false;
        };
        match k.key.as_str() {
            "down" => m.step(true),
            "up" => m.step(false),
            "escape" => self.completion = None,
            // Words are taken with Tab: Enter goes on writing prose.
            "enter"
                if m.current()
                    .is_some_and(|i| i.kind == kalem_core::completers::Kind::Word) =>
            {
                self.completion = None;
                return false;
            }
            "enter" | "tab" => {
                let item = m.current().cloned();
                self.completion = None;
                if let Some(item) = item {
                    kalem_core::completers::apply(&mut self.doc, &item, Instant::now());
                }
                self.after_change(cx);
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The definitions of `#+LATEX_HEADER` for this text version.
    pub fn math_macros(&self) -> Rc<str> {
        let version = self.doc.version();
        let mut m = self.math_macros.borrow_mut();
        if m.0 != version {
            let text = match self.doc.parse() {
                Some((p, _)) => crate::math::macros(p),
                // LaTeX: the document's own definitions.
                // LaTeX: the packages' commands the renderer lacks, and the
                // document's own definitions.
                None if self.doc.latex().is_some() => {
                    org_math::source::macros(&kalem_core::latex_view::math_definitions(&self.doc))
                }
                None => String::new(),
            };
            *m = (version, Rc::from(text));
        }
        m.1.clone()
    }

    /// The formula under the caret, rendered, with where to show it.
    pub fn formula_preview(
        &self,
        window: &Window,
    ) -> Option<(Point<Pixels>, crate::math::Formula)> {
        if !self.math || self.completion.is_some() {
            return None;
        }
        let f = self.formula_under_caret()?;
        let at = self.popup().map(|(at, _)| at)?;
        let formula = self.shared.math.get(
            &f,
            &self.math_macros(),
            px(self.theme.size * 1.1),
            window.scale_factor(),
            self.theme.foreground,
        );
        Some((at, formula))
    }

    /// The popup under the caret: the completion menu, or a formula's
    /// preview.
    fn popup(&self) -> Option<Popup> {
        let text = self.doc.text();
        let line = text.line_of(self.doc.selection.head);
        let painted = self.painted.borrow();
        let p = painted.get(&line)?;
        let caret = p
            .layout
            .caret(p.view.display_offset(self.doc.selection.head));
        let at = p.bounds.origin + caret.origin + gpui::point(px(0.), caret.size.height + px(4.));
        if let Some(m) = &self.completion {
            let items = m
                .rows(8)
                .into_iter()
                .map(|(label, _, chosen)| (label, chosen))
                .collect();
            return Some((at, items));
        }
        let f = self.formula_under_caret()?;
        Some((
            at,
            vec![(format!("= {}", kalem_core::math::unicode(&f)), false)],
        ))
    }

    /// The formula the caret is in: in Org text, or in a LaTeX document
    /// (where it shows its source at the caret).
    fn formula_under_caret(&self) -> Option<String> {
        let head = self.doc.selection.head;
        if self.doc.latex().is_some() {
            return (!self.source)
                .then(|| kalem_core::latex_view::formula_at(&self.doc, head))
                .flatten();
        }
        let (parse, true) = self.doc.parse()? else {
            return None;
        };
        kalem_core::input::formula_at(&parse.syntax(), head)
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        r: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<'_, Self>,
    ) -> Option<String> {
        let line = self.ime_line();
        let (a, b) = (
            self.offset_from_utf16(&line, r.start),
            self.offset_from_utf16(&line, r.end),
        );
        actual.replace(self.utf16_offset(&line, a)..self.utf16_offset(&line, b));
        Some(self.doc.text().as_str()[a..b].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<'_, Self>,
    ) -> Option<UTF16Selection> {
        let line = self.ime_line();
        let s = self.doc.selection;
        let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
        Some(UTF16Selection {
            range: self.utf16_offset(&line, a)..self.utf16_offset(&line, b),
            reversed: s.head < s.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<'_, Self>) -> Option<Range<usize>> {
        let line = self.ime_line();
        self.marked
            .as_ref()
            .map(|m| self.utf16_offset(&line, m.start)..self.utf16_offset(&line, m.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<'_, Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.panel_input(text, cx) {
            self.marked = None;
            return;
        }
        // Outside insert mode Vim keys are commands, not text.
        if self.vim.as_ref().is_some_and(|v| !v.takes_text()) {
            self.marked = None;
            return;
        }
        let line = self.ime_line();
        let s = self.doc.selection;
        let range = r
            .map(|r| self.offset_from_utf16(&line, r.start)..self.offset_from_utf16(&line, r.end))
            .or(self.marked.take())
            .unwrap_or(s.anchor.min(s.head)..s.anchor.max(s.head));
        self.marked = None;
        self.replace(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _sel: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        // Panels show a composition once it is committed.
        if self.palette.is_some()
            || self.date_picker.is_some()
            || self.settings.is_some()
            || self.vim.as_ref().is_some_and(|v| !v.takes_text())
            || self.find.as_ref().is_some_and(|f| f.focused)
        {
            return;
        }
        let line = self.ime_line();
        let s = self.doc.selection;
        let range = r
            .map(|r| self.offset_from_utf16(&line, r.start)..self.offset_from_utf16(&line, r.end))
            .or(self.marked.clone())
            .unwrap_or(s.anchor.min(s.head)..s.anchor.max(s.head));
        let start = range.start;
        self.doc.move_cursor(range.start, false);
        self.doc.move_cursor(range.end, true);
        self.doc.insert_text(text, Instant::now());
        self.marked = (!text.is_empty()).then(|| start..start + text.len());
        self.after_change(cx);
    }

    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<'_, Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.ime_line();
        let text = self.doc.text();
        let l = text.line_of(self.doc.selection.head);
        let painted = self.painted.borrow();
        let p = painted.get(&l)?;
        let a = self.offset_from_utf16(&line, r.start);
        let caret = p.layout.caret(p.view.display_offset(a));
        Some(Bounds::new(p.bounds.origin + caret.origin, caret.size))
    }

    fn character_index_for_point(
        &mut self,
        pt: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<'_, Self>,
    ) -> Option<usize> {
        let line = self.ime_line();
        let pos = self.hit(pt)?.pos;
        Some(self.utf16_offset(&line, pos))
    }
}

/// A document being read and parsed on another thread while the window
/// system starts (see [`prefetch`]).
type Prefetch = (
    std::path::PathBuf,
    std::thread::JoinHandle<Result<DocumentState, String>>,
);

static PREFETCH: std::sync::Mutex<Option<Prefetch>> = std::sync::Mutex::new(None);

/// Starts reading and parsing `path` on another thread, for the first
/// [`open`] of it: start-up and parsing overlap (§15).
pub fn prefetch(path: &std::path::Path, base: org_syntax::ParseContext) {
    if !path.is_file() {
        return;
    }
    let p = path.to_path_buf();
    let handle = std::thread::spawn(move || {
        let settings = Arc::new(org_model::Settings::default());
        DocumentState::open(&p, settings, &base).map_err(|e| e.to_string())
    });
    if let Ok(mut slot) = PREFETCH.lock() {
        *slot = Some((path.to_path_buf(), handle));
    }
}

/// The prefetched document for `path`, if there is one.
fn prefetched(path: &std::path::Path) -> Option<Result<DocumentState, String>> {
    let mut slot = PREFETCH.lock().ok()?;
    if slot.as_ref().is_some_and(|(p, _)| p == path) {
        let (_, handle) = slot.take()?;
        return handle.join().ok();
    }
    None
}

/// An editor for a file, sharing `shared`.
pub fn open(
    path: Option<&std::path::Path>,
    shared: Rc<Shared>,
    theme: Theme,
    cx: &mut App,
) -> Result<Entity<Editor>, String> {
    let settings = Arc::new(org_model::Settings::default());
    let base = shared.config.parse_base();
    let mut doc = match path {
        Some(p) if let Some(d) = prefetched(p) => d?,
        Some(p) if p.exists() => {
            DocumentState::open(p, settings, &base).map_err(|e| e.to_string())?
        }
        _ => {
            let meta = kalem_core::Metadata {
                path: path.map(std::path::Path::to_path_buf),
                mode: path.map_or(DocumentMode::Org, |p| DocumentMode::detect(Some(p), b"")),
                line_ending: if cfg!(windows) {
                    kalem_core::LineEnding::CrLf
                } else {
                    kalem_core::LineEnding::Lf
                },
                bom: false,
                encoding: kalem_core::encoding_rs::UTF_8,
                lossy: false,
            };
            DocumentState::with_base("", meta, settings, &base)
        }
    };
    // A folder: the file manager, set up as the settings say.
    if let Some(s) = doc.dired.as_deref_mut() {
        let (options, details) = kalem_core::dired::options_from(&shared.config);
        s.options = options;
        s.details = details;
        doc.refresh_listing();
        return Ok(cx.new(|cx| Editor::new(doc, shared, theme, cx)));
    }
    // A mode chosen for the file comes first (§2.6).
    if let Some(p) = path
        && let Some(m) = kalem_core::settings::remembered_mode(&shared.config, p)
        && m != doc.meta.mode
    {
        doc.set_mode(m, &base);
    }
    let guessed = kalem_core::files::guessed_message(&doc.meta);
    Ok(cx.new(|cx| {
        let mut e = Editor::new(doc, shared, theme, cx);
        if let Some(m) = guessed {
            e.message(m, false);
        }
        e
    }))
}

/// A new file manager editor showing `place`.
pub fn open_listing(
    place: kalem_core::dired::Place,
    shared: Rc<Shared>,
    theme: Theme,
    cx: &mut App,
) -> Entity<Editor> {
    let (options, details) = kalem_core::dired::options_from(&shared.config);
    let doc = DocumentState::directory(
        place,
        options,
        details,
        Arc::new(org_model::Settings::default()),
    );
    cx.new(|cx| Editor::new(doc, shared, theme, cx))
}

impl Editor {
    /// While a key sequence is half typed: the keys that may follow, in
    /// columns at the bottom (Doom Emacs's which-key).
    fn which_key_view(&self) -> Option<gpui::AnyElement> {
        use gpui::{IntoElement, ParentElement, Styled, div};
        if self.pending.is_empty() {
            return None;
        }
        // After `keys.hints_delay`, unless `keys.hints` is off.
        let due =
            kalem_core::keymap::hints_due(&self.shared.config, self.pending_at, Instant::now());
        if due.is_none_or(|d| !d.is_zero()) {
            return None;
        }
        let seq = KeySequence(self.pending.clone());
        let items = self
            .shared
            .keymap
            .which_key(&self.shared.registry, &seq, &self.context());
        if items.is_empty() {
            return None;
        }
        let theme = &self.theme;
        let cells = items.into_iter().map(|(k, l)| {
            div()
                .w(px(220.))
                .flex()
                .flex_row()
                .gap(px(6.))
                .child(div().text_color(theme.link).child(k))
                .child(div().text_color(theme.muted).child("→"))
                .child(l)
        });
        Some(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_x(px(12.))
                .gap_y(px(2.))
                .px(px(16.))
                .py(px(8.))
                .bg(theme.bar)
                .border_t_1()
                .border_color(theme.border)
                .text_size(px(theme.size * 0.8))
                .children(cells)
                .into_any_element(),
        )
    }
}

/// Whether a when-clause value is true (for tests).
pub fn flag(ctx: &WhenContext, key: &str) -> bool {
    ctx.get(key) == Some(&WhenValue::Bool(true))
}

/// The text a screen reader gets for a line: its display text, with
/// checkboxes as ☐ ☑ ◐ and formulas and images as their text, and the
/// character index of each display offset at the start of a run.
fn a11y_line(view: &LineView) -> (String, Vec<(usize, usize, bool)>) {
    let mut text = String::new();
    // (display offset, character index, verbatim) at each run start.
    let mut starts = Vec::new();
    let mut d = 0;
    for r in &view.runs {
        starts.push((d, text.chars().count(), r.verbatim && r.widget.is_none()));
        match &r.widget {
            Some(Widget::Checkbox(c)) => text.push(match c {
                view::CheckState::Checked => '☑',
                view::CheckState::Partial => '◐',
                view::CheckState::Unchecked => '☐',
            }),
            Some(Widget::Math { source, .. }) => text.push_str(&kalem_core::math::unicode(source)),
            Some(Widget::Image { path, .. }) => text.push_str(&format!("image {path}")),
            Some(Widget::TocRow { .. }) | None => text.push_str(&r.text),
        }
        d += r.text.len();
    }
    (text, starts)
}

/// The character index in a line's accessible text of display offset `d`.
fn a11y_index(view: &LineView, starts: &[(usize, usize, bool)], d: usize) -> usize {
    let i = starts.partition_point(|s| s.0 <= d).saturating_sub(1);
    match starts.get(i) {
        Some(&(s, c, true)) => {
            let run = &view.runs[i];
            c + run.text[..(d - s).min(run.text.len())].chars().count()
        }
        Some(&(s, c, false)) if d > s => c + 1,
        Some(&(_, c, _)) => c,
        None => 0,
    }
}

/// What the editor tells assistive technology: its lines on screen, the
/// caret and the selection.
#[derive(Debug, Clone, Default)]
pub struct A11yText {
    /// Source line and accessible text of each line, in order.
    pub lines: Vec<(usize, String)>,
    /// The selection: (line, character) of its anchor and of the caret.
    pub selection: Option<((usize, usize), (usize, usize))>,
}

impl Editor {
    /// The accessible text: the lines painted in the last frame (or those
    /// around the cursor before the first one), and the selection within
    /// them.
    pub fn a11y_text(&self) -> A11yText {
        let mut lines: Vec<usize> = self.painted.borrow().keys().copied().collect();
        lines.sort_unstable();
        if lines.is_empty() {
            let c = self
                .item_of(self.doc.text().line_of(self.doc.selection.head))
                .unwrap_or(0);
            let (a, b) = (c.saturating_sub(20), (c + 40).min(self.visible.len()));
            lines = self.visible[a..b].to_vec();
        }
        let mut out = A11yText::default();
        let mut positions = std::collections::HashMap::new();
        for &l in &lines {
            let view = self.line_view(l);
            let (text, starts) = a11y_line(&view);
            positions.insert(l, (view, starts));
            out.lines.push((l, text));
        }
        let text = self.doc.text();
        let at = |offset: usize| -> Option<(usize, usize)> {
            let l = text.line_of(offset.min(text.len()));
            let (view, starts) = positions.get(&l)?;
            Some((l, a11y_index(view, starts, view.display_offset(offset))))
        };
        let s = self.doc.selection;
        if let (Some(a), Some(h)) = (at(s.anchor), at(s.head)) {
            out.selection = Some((a, h));
        } else if let Some(h) = at(s.head) {
            out.selection = Some((h, h));
        }
        out
    }
}

/// Adds the accessible text as synthetic text runs of the editor's node.
fn build_a11y(b: &mut gpui::A11ySubtreeBuilder<'_>, t: A11yText) {
    use gpui::accesskit::{Node, Role, TextPosition, TextSelection};
    let mut ids = HashMap::new();
    for (line, text) in &t.lines {
        let id = b.synthetic_node_id(line);
        let mut run = Node::new(Role::TextRun);
        run.set_character_lengths(text.chars().map(|c| c.len_utf8() as u8).collect::<Vec<_>>());
        run.set_value(text.clone());
        b.push_child(id, run);
        ids.insert(*line, id);
    }
    if let Some(((al, ac), (hl, hc))) = t.selection
        && let (Some(a), Some(h)) = (ids.get(&al), ids.get(&hl))
    {
        b.parent_node().set_text_selection(TextSelection {
            anchor: TextPosition {
                node: *a,
                character_index: ac,
            },
            focus: TextPosition {
                node: *h,
                character_index: hc,
            },
        });
    }
}

impl gpui::Render for Editor {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl gpui::IntoElement {
        self.apply_resume();
        use gpui::{
            InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
            div, list,
        };
        let a11y = self.a11y_text();
        // Under the caret: a formula rendered, or the completion menu (and a
        // formula's Unicode approximation when it cannot be rendered).
        let (math_popup, text_popup) = match self.formula_preview(window) {
            Some((at, crate::math::Formula::Image { image, size, .. })) => {
                let theme = self.theme.clone();
                let popup = gpui::deferred(
                    gpui::anchored().position(at).child(
                        div()
                            .p(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.background)
                            .child(gpui::img(image).w(size.width).h(size.height)),
                    ),
                );
                (Some(popup), None)
            }
            _ => (None, self.popup()),
        };
        let label = self
            .doc
            .meta
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("untitled".into(), |n| n.to_string_lossy().into_owned());
        if self.follow_sideways() {
            cx.notify();
        }
        self.update_plain();
        let info = self.formula.get(&mut self.doc);
        self.formula_status = info.and_then(kalem_core::formulas::status);
        self.formula_refs = info
            .map(kalem_core::formulas::references)
            .unwrap_or_default();
        // Else the entry cited under the cursor.
        if self.formula_status.is_none() {
            self.formula_status = self.cite_preview.get(&mut self.doc);
        }
        self.painted.borrow_mut().clear();
        if let Some(o) = &self.other {
            o.painted.borrow_mut().clear();
        }
        let entity = cx.entity();
        let theme = self.theme.clone();
        let _ = window;
        // A pane: the list of its visible lines.
        // A readable text column: `editor.line_width` characters, centered
        // (a character is about half the font size wide).
        let chars = self.shared.config.int("editor.line_width");
        let center = self.shared.config.bool("editor.center_text");
        let column = (chars > 0).then(|| px(chars as f32 * theme.size * 0.5 + 96.));
        let focus = self.focus.clone();
        // A CSV file's header row stays at the top when its rows scroll.
        let header = self.doc.meta.mode == DocumentMode::Csv
            && !self.source
            && kalem_core::csv::layout(&self.doc).dialect.header;
        let background = theme.background;
        let border = theme.border;
        let pane = |state: ListState, visible: Vec<usize>, other: bool| {
            let entity = entity.clone();
            let header_editor = entity.clone();
            let top = state.logical_scroll_top();
            let pinned = header
                && visible.first() == Some(&0)
                && (top.item_ix > 0 || top.offset_in_item > px(0.));
            let mut text = div().h_full().w_full().px(px(48.)).py(px(16.)).relative();
            if let Some(w) = column {
                text = text.max_w(w);
            }
            // Typing reaches the editor even when the caret's line is not on
            // screen (after scrolling away): a fallback input handler, painted
            // before the lines so that the caret's line, when painted, wins.
            if !other {
                let (entity, focus) = (entity.clone(), focus.clone());
                text = text.child(
                    gpui::canvas(
                        |_, _, _| {},
                        move |bounds, _, window, cx| {
                            window.handle_input(
                                &focus,
                                gpui::ElementInputHandler::new(bounds, entity.clone()),
                                cx,
                            );
                        },
                    )
                    .absolute()
                    .size_full(),
                );
            }
            let row = div().flex_1().h_full().flex().flex_row();
            // The column in the middle, like a page, or at the left.
            let row = if center {
                row.justify_center()
            } else {
                row.justify_start()
            };
            row.child(
                text.child(
                    list(state, move |ix, _window, cx| {
                        let line = visible.get(ix).copied().unwrap_or(0);
                        // `org-indent-mode`: the line moved right by the
                        // level of its heading, two characters a level.
                        let indent = entity.update(cx, |e, _| e.outline_indent_of(line));
                        let element = crate::line::LineElement {
                            editor: entity.clone(),
                            line,
                            other,
                        };
                        if indent == px(0.) {
                            element.into_any_element()
                        } else {
                            div().pl(indent).child(element).into_any_element()
                        }
                    })
                    .size_full(),
                )
                .children(pinned.then(|| {
                    div()
                        .debug_selector(|| "csv-header".into())
                        .absolute()
                        .top(px(0.))
                        .left(px(48.))
                        .right(px(48.))
                        .pt(px(16.))
                        .bg(background)
                        .border_b_1()
                        .border_color(border)
                        .child(crate::line::LineElement {
                            editor: header_editor,
                            line: 0,
                            other,
                        })
                })),
            )
        };
        let active = pane(self.list.clone(), self.visible.clone(), false)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::right_mouse_down));
        let other = self.other.as_ref().map(|o| {
            pane(o.list.clone(), o.visible.clone(), true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.swap_panes();
                    this.mouse_down(ev, window, cx);
                }),
            )
        });
        let outline = self.outline_panel(cx);
        let preview = self.preview_panel(cx);
        let panes = match other {
            Some(o) if self.left => vec![active, o.border_l_1().border_color(theme.border)],
            Some(o) => vec![o, active.border_l_1().border_color(theme.border)],
            None => vec![active],
        };
        div()
            .id("editor")
            .role(gpui::Role::MultilineTextInput)
            .aria_label(SharedString::from(label))
            .a11y_synthetic_children(move |b| build_a11y(b, a11y))
            .key_context("Editor")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, a: &RunCommand, window, cx| {
                this.run_command(&a.id, a.args(), window, cx);
                this.last_command = Some(a.id.to_string());
            }))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                    this.drop_paths(paths.paths(), window, cx);
                }),
            )
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(theme.size))
            .font_family(SharedString::from(theme.font.clone()))
            .children(math_popup)
            .children(text_popup.map(|(at, items)| {
                let theme = self.theme.clone();
                gpui::deferred(
                    gpui::anchored().position(at).child(
                        div()
                            .flex()
                            .flex_col()
                            .py(px(4.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.bar)
                            .text_size(px(theme.size * 0.85))
                            .children(items.into_iter().map(move |(label, chosen)| {
                                let row = div().px(px(10.)).py(px(1.)).child(label);
                                if chosen { row.bg(theme.selection) } else { row }
                            })),
                    ),
                )
            }))
            .children(self.cite_hover.clone().map(|(at, text)| {
                let theme = self.theme.clone();
                gpui::deferred(
                    gpui::anchored()
                        .position(at + gpui::point(px(12.), px(18.)))
                        .child(
                            div()
                                .max_w(px(520.))
                                .px(px(10.))
                                .py(px(6.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.bar)
                                .text_size(px(theme.size * 0.85))
                                .child(text),
                        ),
                )
            }))
            .children(outline)
            .children(panes)
            .children(preview)
            .children(self.find_view(cx))
            .children(self.which_key_view())
            .children(self.palette_view(cx))
            .children(self.date_picker_view(cx))
            .children(self.settings_view(cx))
            .children(self.context_menu_view(cx))
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        if let Ok(mut bus) = self.shared.bus.try_borrow_mut() {
            bus.emit(&kalem_core::events::Event::DocumentClose { doc: self.doc_id });
        }
    }
}
