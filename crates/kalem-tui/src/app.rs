//! The application: one document with the shared command registry, keymap,
//! settings and event bus, driven by terminal events.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use kalem_core::command::{
    Clipboard, EditorContext, FileManagerRequest, PickKind, ProjectRequest, Request,
};
use kalem_core::dired::{Answer, Place, Question};
use kalem_core::events::{ChangeDebouncer, DocumentId, Event, EventBus, EventKind, Reply};
use kalem_core::files::{FileWatcher, OpenError};
use kalem_core::keymap::{self, Keymap, KeymapIssue, Lookup};
use kalem_core::keys::{KeyChord, KeySequence};
use kalem_core::projects::{self, After, OpenFile, Picker, ProjectSearch, ProjectState};
use kalem_core::settings::{self, Config};
use kalem_core::view::{Visibility, Widget};
use kalem_core::when::{Context, Value as WhenValue};
use kalem_core::{CommandRegistry, DocumentMode, DocumentState, LineEnding, Metadata, tr};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::caps::Caps;
use crate::editor::EditorView;
use crate::input;
use crate::panels::{Find, OutlineItem, OutlinePanel, Palette};

/// Where the list of open files shows (`ui.open_files`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesAt {
    /// A column on the left.
    Left,
    /// A line at the top.
    Top,
    /// Not at all.
    Hidden,
}

/// An open document that is not the active one: its state while another
/// is shown.
struct Buffer {
    doc: DocumentState,
    doc_id: DocumentId,
    editor: EditorView,
    global_fold: Visibility,
    words: kalem_core::stats::WordCounts,
    formula: kalem_core::formulas::FormulaCache,
}

/// Keys as people write them: `Ctrl+G`, `Alt+Shift+P`.
fn pretty_keys(k: &str) -> String {
    k.split(' ')
        .map(|chord| {
            chord
                .split('+')
                .map(|p| {
                    let mut c = p.chars();
                    match c.next() {
                        Some(f) if p.len() > 1 => f.to_uppercase().chain(c).collect(),
                        Some(f) => f.to_uppercase().collect(),
                        None => "+".into(),
                    }
                })
                .collect::<Vec<String>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a prompt is for.
#[derive(Debug, Clone, PartialEq)]
enum PromptKind {
    /// An argument of a command.
    Arg {
        command: String,
        args: Value,
        name: String,
        ty: String,
    },
    /// A file name to save to.
    SaveAs,
    /// Unsaved changes when quitting: yes, no, cancel.
    Quit,
    /// Unsaved changes when closing the document: yes, no, cancel.
    Close,
    /// The file changed on disk while there are unsaved changes.
    Reload,
    /// The file changed on disk since it was read: overwrite?
    Overwrite,
    /// A question of a file operation (`App::task`).
    FileTask,
}

#[derive(Debug, Clone, PartialEq)]
struct Prompt {
    kind: PromptKind,
    label: String,
    input: String,
    /// The cursor, as characters after it ([`kalem_core::line_edit`]).
    back: usize,
}

/// A message in the status line.
#[derive(Debug, Clone)]
struct Status {
    text: String,
    error: bool,
    at: Instant,
}

/// The terminal application.
pub struct App {
    /// The settings.
    pub config: Config,
    registry: CommandRegistry,
    keymap: Keymap,
    /// Problems in the keymap files.
    pub keymap_issues: Vec<KeymapIssue>,
    bus: EventBus,
    watcher: Option<FileWatcher>,
    changed_files: Rc<RefCell<Vec<PathBuf>>>,
    /// The document.
    pub doc: DocumentState,
    doc_id: DocumentId,
    clipboard: Clipboard,
    /// The editor view.
    pub editor: EditorView,
    /// The terminal's capabilities.
    pub caps: Caps,
    pending: Vec<KeyChord>,
    status: Option<Status>,
    prompt: Option<Prompt>,
    debouncer: ChangeDebouncer,
    global_fold: Visibility,
    last_click: Option<(Instant, u16, u16)>,
    /// The last command run by a key, for the first key in a table field.
    last_command: Option<String>,
    /// Escape sequences for the terminal (OSC 52), written between frames.
    output: Vec<String>,
    /// The open completion menu.
    completion: Option<kalem_core::completers::Menu>,
    /// The completers (built-ins, and plugins').
    completers: kalem_core::completers::Registry,
    /// The command palette, when open.
    palette: Option<Palette>,
    /// The find bar, when open.
    find: Option<Find>,
    /// The last search, offered when find opens again.
    last_query: String,
    /// Whether the last search was a regular expression.
    last_regex: bool,
    /// Word counts for the status line.
    words: kalem_core::stats::WordCounts,
    /// The table formula at the cursor.
    formula: kalem_core::formulas::FormulaCache,
    /// The entry cited under the cursor.
    cite_preview: kalem_core::cite::Preview,
    /// The Vim layer, with the Vim keymap profile.
    pub vim: Option<kalem_core::vim::Vim>,
    /// The cursor shape last set: a block (Vim outside insert mode) or not.
    cursor_block: Option<bool>,
    /// The outline panel, when shown.
    outline: Option<OutlinePanel>,
    /// The open documents in the order they were opened; the active
    /// one's slot is empty, its state being in the fields above.
    docs: Vec<Option<Buffer>>,
    /// The active document's slot.
    active: usize,
    next_doc: u64,
    /// The project list and the files of the projects in use.
    pub projects: ProjectState,
    /// Where the list of open files shows, and whether it is shown.
    files_at: FilesAt,
    files_shown: bool,
    /// Where each document is in the list of open files, as drawn.
    file_spots: Vec<(Rect, usize)>,
    /// Places that run a command when clicked (the file manager and the
    /// projects in the list of open files, the hint in the status line).
    action_spots: Vec<(Rect, &'static str)>,
    /// The folder tree's lines as last drawn, and where each is.
    tree_rows: Vec<projects::TreeRow>,
    tree_spots: Vec<(Rect, usize)>,
    /// The last document shown that is not a file manager, to go back to.
    last_text: Option<DocumentId>,
    /// A file operation asking its questions.
    task: Option<kalem_core::dired::Task>,
    /// File operations running in the background.
    jobs: Vec<kalem_core::dired::Running>,
    /// The folders of the file manager documents, watched as a whole.
    watched_dirs: Vec<PathBuf>,
    /// Whether the application should end.
    pub quit: bool,
    /// Whether the screen needs drawing.
    pub dirty: bool,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("path", &self.doc.meta.path)
            .finish_non_exhaustive()
    }
}

/// What the terminal offers the Vim layer: Kalem's clipboard, copied to
/// the terminal's with OSC 52.
struct TuiHost<'a> {
    clip: &'a mut String,
    output: &'a mut Vec<String>,
    lines: usize,
    rich: bool,
}

impl kalem_core::vim::Host for TuiHost<'_> {
    fn clipboard(&mut self) -> Option<String> {
        (!self.clip.is_empty()).then(|| self.clip.clone())
    }

    fn set_clipboard(&mut self, text: &str) {
        *self.clip = text.to_string();
        self.output.push(osc52(text));
    }

    fn page_lines(&self) -> usize {
        self.lines
    }

    fn rich_view(&self) -> bool {
        self.rich
    }
}

/// A terminal key for the Vim layer.
fn vim_key_of(k: &KeyEvent) -> kalem_core::vim::Key {
    use kalem_core::vim::Key;
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    match k.code {
        _ if alt => Key::Other,
        KeyCode::Char(c) if ctrl => Key::Ctrl(c.to_ascii_lowercase()),
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Esc => Key::Esc,
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        _ => Key::Other,
    }
}

/// OSC 52: puts `text` on the system clipboard through the terminal.
pub fn osc52(text: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = text.as_bytes();
    let mut out = String::from("\x1b]52;c;");
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out.push('\x07');
    out
}

/// Whether a change of one of `changed` concerns `doc`: its file, or for
/// a file manager, its folder or a folder listed in it.
fn watches(doc: &DocumentState, changed: &[PathBuf]) -> bool {
    doc.meta.path.as_ref().is_some_and(|p| changed.contains(p))
        || doc
            .dired
            .as_deref()
            .is_some_and(|s| s.subdirs.iter().any(|d| changed.contains(d)))
}

fn new_document(path: Option<&Path>, config: &Config) -> Result<DocumentState, OpenError> {
    let settings = Arc::new(org_model::Settings::default());
    let base = config.parse_base();
    let mut doc = match path {
        Some(p) if p.exists() => DocumentState::open(p, settings, &base)?,
        _ => {
            let mode = path.map_or(DocumentMode::Org, |p| DocumentMode::detect(Some(p), b""));
            let meta = Metadata {
                path: path.map(Path::to_path_buf),
                mode,
                line_ending: if cfg!(windows) {
                    LineEnding::CrLf
                } else {
                    LineEnding::Lf
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
        let (options, details) = kalem_core::dired::options_from(config);
        s.options = options;
        s.details = details;
        doc.refresh_listing();
        return Ok(doc);
    }
    // A mode chosen for the file comes first (§2.6).
    if let Some(p) = path
        && let Some(m) = settings::remembered_mode(config, p)
        && m != doc.meta.mode
    {
        doc.set_mode(m, &base);
    }
    Ok(doc)
}

impl App {
    /// The application for the file at `path` (a new document if it does
    /// not exist, or an empty one without a path).
    pub fn new(path: Option<&Path>, config: Config, caps: Caps) -> Result<App, OpenError> {
        let user = settings::config_dir().map(|d| d.join("keymap.json"));
        let (entries, issues) = match user.as_deref().map(std::fs::read_to_string) {
            Some(Ok(text)) => {
                keymap::parse_keymap_with(&text, keymap::Origin::User, &config.vim_leader())
            }
            _ => (Vec::new(), Vec::new()),
        };
        let mut app = App::with_keymap(path, config, caps, &entries, issues)?;
        // The user's project list (tests keep theirs in memory).
        app.projects = ProjectState::load(projects::list_file());
        if let Some(p) = app.doc.meta.path.clone().filter(|p| p.is_file()) {
            app.projects.opened(&p);
        }
        app.enter_project();
        Ok(app)
    }

    /// The application with these user key bindings.
    pub fn with_keymap(
        path: Option<&Path>,
        config: Config,
        caps: Caps,
        entries: &[keymap::Entry],
        mut issues: Vec<KeymapIssue>,
    ) -> Result<App, OpenError> {
        let doc = new_document(path, &config)?;
        let registry = CommandRegistry::with_builtins();
        let (full, more) = Keymap::build_with(
            &registry,
            config.keymap_profile(),
            entries,
            &config.vim_leader(),
        );
        issues.extend(more);
        let (keymap, more) = full.for_terminal(caps.kitty_keyboard);
        issues.extend(more);
        let mut bus = EventBus::new();
        let changed_files: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
        let queue = changed_files.clone();
        bus.subscribe(Some(EventKind::WorkspaceFileChanged), move |e| {
            if let Event::WorkspaceFileChanged { path } = e {
                queue.borrow_mut().push(path.clone());
            }
            Reply::Continue
        });
        let mut watcher = FileWatcher::new(bus.sender()).ok();
        if let (Some(w), Some(p)) = (&mut watcher, &doc.meta.path)
            && p.exists()
        {
            let _ = w.watch(p);
        }
        let doc_id = DocumentId(1);
        bus.emit(&Event::DocumentOpen {
            doc: doc_id,
            path: doc.meta.path.clone(),
        });
        let mut app = App {
            config,
            registry,
            keymap,
            keymap_issues: issues,
            bus,
            watcher,
            changed_files,
            doc,
            doc_id,
            clipboard: Clipboard::default(),
            editor: EditorView::default(),
            caps,
            pending: Vec::new(),
            status: None,
            prompt: None,
            debouncer: ChangeDebouncer::new(Duration::from_millis(300)),
            global_fold: Visibility::Subtree,
            last_click: None,
            last_command: None,
            output: Vec::new(),
            completion: None,
            completers: kalem_core::completers::Registry::with_builtins(),
            palette: None,
            find: None,
            last_query: String::new(),
            last_regex: false,
            words: Default::default(),
            formula: Default::default(),
            cite_preview: Default::default(),
            vim: None,
            cursor_block: None,
            outline: None,
            docs: vec![None],
            active: 0,
            next_doc: 2,
            projects: ProjectState::default(),
            files_at: FilesAt::Left,
            files_shown: true,
            file_spots: Vec::new(),
            action_spots: Vec::new(),
            tree_rows: Vec::new(),
            tree_spots: Vec::new(),
            last_text: None,
            task: None,
            jobs: Vec::new(),
            watched_dirs: Vec::new(),
            quit: false,
            dirty: true,
        };
        app.files_at = match app.config.str("ui.open_files") {
            "top" => FilesAt::Top,
            "hidden" => FilesAt::Hidden,
            _ => FilesAt::Left,
        };
        app.editor.line_width = u16::try_from(app.config.int("editor.line_width")).unwrap_or(0);
        app.editor.center = app.config.bool("editor.center_text");
        app.editor.wrap = app.config.bool("editor.soft_wrap");
        app.editor.line_numbers = app.config.bool("editor.line_numbers");
        app.refresh_vim();
        app.editor.images.borrow_mut().base = app
            .doc
            .meta
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        app.startup_folds();
        if let Some(m) = kalem_core::files::guessed_message(&app.doc.meta) {
            app.message(m, false);
        }
        let problems = settings::report_problems(&app.config, &app.keymap_issues, true);
        if problems > 0 {
            app.message(tr!("msg-config-problems", count = problems), true);
        }
        app.bus.emit(&Event::AppReady);
        Ok(app)
    }

    /// `#+STARTUP` folding.
    fn startup_folds(&mut self) {
        self.editor.outline_indent = self.config.bool("editor.outline_indent");
        let Some((p, true)) = self.doc.parse() else {
            return;
        };
        // `org-indent-mode` for this document, or not.
        self.editor.outline_indent =
            kalem_core::view::outline_indent(&p.keywords(), self.editor.outline_indent);
        let mut option = None;
        for (k, v) in p.keywords() {
            if k.eq_ignore_ascii_case("STARTUP") {
                for w in v.split_whitespace() {
                    if matches!(
                        w,
                        "overview" | "fold" | "content" | "showall" | "showeverything"
                    ) {
                        option = Some(w.to_string());
                    }
                }
            }
        }
        if let Some(o) = option {
            let blocks = self.editor.all_blocks(&self.doc);
            self.editor.folds = kalem_core::view::Folds::startup(&blocks, &o);
        }
    }

    fn message(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            tracing::warn!("{text}");
        }
        self.status = Some(Status {
            text,
            error,
            at: Instant::now(),
        });
        self.dirty = true;
    }

    /// The when-clause context.
    fn context(&self) -> Context {
        let mut c = self.doc.when_context();
        c.flag("editorFocus", self.prompt.is_none());
        c.flag("sourceView", self.editor.source);
        c.flag("terminal", true);
        c.flag("inProject", self.project().is_some());
        if let Some(v) = &self.vim {
            c.set("vimMode", WhenValue::Str(v.mode_name().into()));
            c.flag("vimCommand", v.idle_command() && v.command_line.is_none());
        }
        c
    }

    /// The folder of the project holding the active document.
    fn project(&self) -> Option<PathBuf> {
        self.projects
            .containing(self.doc.meta.path.as_deref())
            .map(|p| p.root.clone())
    }

    /// The open documents, as the list of open files shows them.
    pub fn open_files(&self) -> Vec<OpenFile> {
        let file = |doc: &DocumentState| OpenFile {
            // The file manager is not listed under a project: its name would
            // show twice (the project, then its folder).
            path: doc.meta.path.clone().filter(|_| doc.dired.is_none()),
            title: match doc.dired.as_deref() {
                Some(d) => d.list_title(),
                None => doc
                    .meta
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map_or_else(|| tr!("untitled"), |n| n.to_string_lossy().into_owned()),
            },
            modified: doc.is_modified(),
        };
        self.docs
            .iter()
            .map(|b| match b {
                Some(b) => file(&b.doc),
                None => file(&self.doc),
            })
            .collect()
    }

    /// The active document's index among the open ones.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// A view for a new document, set up like the others.
    fn new_view(&self, doc: &DocumentState) -> EditorView {
        let mut v = EditorView::default();
        v.line_width = u16::try_from(self.config.int("editor.line_width")).unwrap_or(0);
        v.center = self.config.bool("editor.center_text");
        v.wrap = self.config.bool("editor.soft_wrap");
        v.line_numbers = self.config.bool("editor.line_numbers");
        v.raw_math = self.editor.raw_math;
        v.outline_indent = self.config.bool("editor.outline_indent");
        {
            let old = self.editor.images.borrow();
            let mut im = v.images.borrow_mut();
            im.picker = old.picker.clone();
            im.compress = old.compress;
            im.math_colors = old.math_colors;
            im.base = doc
                .meta
                .path
                .as_ref()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf);
        }
        v.area = self.editor.area;
        v
    }

    /// Makes document `i` the active one.
    fn activate(&mut self, i: usize) {
        if i == self.active || i >= self.docs.len() {
            return;
        }
        let Some(b) = self.docs[i].take() else { return };
        if self.doc.dired.is_none() {
            self.last_text = Some(self.doc_id);
        }
        let old = Buffer {
            doc: std::mem::replace(&mut self.doc, b.doc),
            doc_id: std::mem::replace(&mut self.doc_id, b.doc_id),
            editor: std::mem::replace(&mut self.editor, b.editor),
            global_fold: std::mem::replace(&mut self.global_fold, b.global_fold),
            words: std::mem::replace(&mut self.words, b.words),
            formula: std::mem::replace(&mut self.formula, b.formula),
        };
        self.docs[self.active] = Some(old);
        self.active = i;
        self.completion = None;
        self.find = None;
        self.outline = None;
        self.pending.clear();
        self.editor.follow = true;
        if let Some(v) = &mut self.vim
            && v.takes_text()
        {
            v.mode = kalem_core::vim::Mode::Normal;
        }
        self.refresh_vim();
        // Changes on disk while it was in the background.
        if let Ok(kalem_core::document::ExternalChange::Reloaded) =
            self.doc.external_change(Instant::now())
        {
            self.message(tr!("msg-reloaded"), false);
        }
        self.enter_project();
        self.dirty = true;
    }

    /// The active document's project counts as switched to, and its files
    /// start being listed (a folder under version control becomes a
    /// project first, with `projects.auto_add`).
    fn enter_project(&mut self) {
        let auto = self.config.bool("projects.auto_add");
        if let Some(m) = self.projects.entered(self.doc.meta.path.as_deref(), auto) {
            self.message(m, false);
        }
    }

    /// Back from the file manager to the document shown before it.
    fn leave_file_manager(&mut self) {
        let doc_of =
            |b: &Option<Buffer>, id: DocumentId| b.as_ref().is_some_and(|b| b.doc_id == id);
        let target = self
            .last_text
            .and_then(|id| self.docs.iter().position(|b| doc_of(b, id)))
            .or_else(|| {
                self.docs
                    .iter()
                    .position(|b| b.as_ref().is_some_and(|b| b.doc.dired.is_none()))
            });
        match target {
            Some(i) => self.activate(i),
            None => self.message(tr!("msg-no-other-document"), false),
        }
    }

    /// Opens `path` (or shows it if it is open), the cursor at `at` (a
    /// line from 1, a byte column).
    pub fn open_path(&mut self, path: &Path, at: Option<(u64, usize)>) {
        if path.is_dir() {
            let dir = projects::normal(path);
            self.file_manager(Place::Dir(dir), None);
            return;
        }
        let target = projects::normal(path);
        let same = |d: &DocumentState| {
            d.meta
                .path
                .as_deref()
                .is_some_and(|p| projects::normal(p) == target)
        };
        let found = self.docs.iter().enumerate().find_map(|(i, b)| match b {
            Some(b) if same(&b.doc) => Some(i),
            None if same(&self.doc) => Some(i),
            _ => None,
        });
        match found {
            Some(i) => self.activate(i),
            None => {
                let doc = match new_document(Some(&target), &self.config) {
                    Ok(d) => d,
                    Err(e) => {
                        let msg = tr!(
                            "msg-cannot-open-file",
                            path = target.display().to_string(),
                            error = e.to_string()
                        );
                        self.message(msg, true);
                        return;
                    }
                };
                if let (Some(w), true) = (&mut self.watcher, target.exists()) {
                    let _ = w.watch(&target);
                }
                let doc_id = DocumentId(self.next_doc);
                self.next_doc += 1;
                self.bus.emit(&Event::DocumentOpen {
                    doc: doc_id,
                    path: Some(target.clone()),
                });
                let guessed = kalem_core::files::guessed_message(&doc.meta);
                let editor = self.new_view(&doc);
                // An untouched empty document gives way to the file.
                let replace = self.doc.meta.path.is_none()
                    && !self.doc.is_modified()
                    && self.doc.text().is_empty();
                let old = self.active;
                self.docs.push(Some(Buffer {
                    doc,
                    doc_id,
                    editor,
                    global_fold: Visibility::Subtree,
                    words: Default::default(),
                    formula: Default::default(),
                }));
                self.activate(self.docs.len() - 1);
                if replace {
                    self.docs.remove(old);
                    self.active -= 1;
                }
                self.startup_folds();
                if let Some(m) = guessed {
                    self.message(m, false);
                }
            }
        }
        if target.is_file() {
            self.projects.opened(&target);
        }
        if let Some((line, column)) = at {
            let text = self.doc.text();
            let l = (line.max(1) as usize - 1).min(text.line_count().saturating_sub(1));
            let r = text.line_range(l);
            let mut pos = (r.start + column).min(r.end);
            while !text.as_str().is_char_boundary(pos) {
                pos -= 1;
            }
            self.doc.move_cursor(pos, false);
            self.after_change(true);
        }
        self.dirty = true;
    }

    /// Shows `place` in the window's file manager (a new one if there is
    /// none), the cursor on `select`.
    fn file_manager(&mut self, place: Place, select: Option<PathBuf>) {
        if self.doc.dired.is_none() {
            let found = self
                .docs
                .iter()
                .position(|b| b.as_ref().is_some_and(|b| b.doc.dired.is_some()));
            match found {
                Some(i) => self.activate(i),
                None => {
                    let (options, details) = kalem_core::dired::options_from(&self.config);
                    let doc = DocumentState::directory(
                        place.clone(),
                        options,
                        details,
                        Arc::new(org_model::Settings::default()),
                    );
                    let doc_id = DocumentId(self.next_doc);
                    self.next_doc += 1;
                    self.bus.emit(&Event::DocumentOpen {
                        doc: doc_id,
                        path: doc.meta.path.clone(),
                    });
                    let editor = self.new_view(&doc);
                    let replace = self.doc.meta.path.is_none()
                        && self.doc.dired.is_none()
                        && !self.doc.is_modified()
                        && self.doc.text().is_empty();
                    let old = self.active;
                    self.docs.push(Some(Buffer {
                        doc,
                        doc_id,
                        editor,
                        global_fold: Visibility::Subtree,
                        words: Default::default(),
                        formula: Default::default(),
                    }));
                    self.activate(self.docs.len() - 1);
                    if replace {
                        self.docs.remove(old);
                        self.active -= 1;
                    }
                }
            }
        }
        match place {
            Place::Projects => {
                let rows = kalem_core::dired::project_rows(&self.projects.list);
                kalem_core::dired::show_projects(&mut self.doc, rows, select.as_deref());
            }
            place => self.doc.visit(place, select.as_deref()),
        }
        self.editor.follow = true;
        self.after_change(true);
        self.sync_watches();
    }

    /// Watches the folders the file manager documents show, and no others.
    fn sync_watches(&mut self) {
        let mut want: Vec<PathBuf> = std::iter::once(&self.doc)
            .chain(self.docs.iter().flatten().map(|b| &b.doc))
            .filter_map(|d| Some((d.meta.path.clone()?, d.dired.as_deref()?)))
            .flat_map(|(p, s)| std::iter::once(p).chain(s.subdirs.iter().cloned()))
            .collect();
        want.sort();
        want.dedup();
        if want == self.watched_dirs {
            return;
        }
        if let Some(w) = &mut self.watcher {
            for d in self.watched_dirs.iter().filter(|d| !want.contains(d)) {
                let _ = w.unwatch_dir(d);
            }
            for d in want.iter().filter(|d| !self.watched_dirs.contains(d)) {
                let _ = w.watch_dir(d);
            }
        }
        self.watched_dirs = want;
    }

    /// Asks the running file operation's next question, or starts it.
    fn next_question(&mut self) {
        let Some(t) = self.task.as_ref() else { return };
        match t.question() {
            None => {
                let t = self.task.take().expect("a task");
                let job = t.start();
                self.message(job.status(), false);
                self.jobs.push(job);
            }
            Some(Question::Confirm(q)) => {
                self.ask(
                    PromptKind::FileTask,
                    &format!("{q} {}", tr!("fm-confirm-keys")),
                    String::new(),
                );
            }
            Some(Question::Conflict { text, .. }) => {
                self.ask(
                    PromptKind::FileTask,
                    &format!("{text} {}", tr!("fm-conflict-keys")),
                    String::new(),
                );
            }
        }
    }

    /// Reads every file manager listing again.
    fn refresh_listings(&mut self) {
        for b in self.docs.iter_mut().flatten() {
            if b.doc.dired.is_some() {
                b.doc.refresh_listing();
            }
        }
        if self.doc.dired.is_some() {
            self.doc.refresh_listing();
            self.after_change(false);
        }
    }

    /// A new, empty document.
    fn new_empty(&mut self) {
        let Ok(doc) = new_document(None, &self.config) else {
            return;
        };
        let doc_id = DocumentId(self.next_doc);
        self.next_doc += 1;
        let editor = self.new_view(&doc);
        self.docs.push(Some(Buffer {
            doc,
            doc_id,
            editor,
            global_fold: Visibility::Subtree,
            words: Default::default(),
            formula: Default::default(),
        }));
        self.activate(self.docs.len() - 1);
    }

    /// Closes the active document (its changes were dealt with); the last
    /// one ends the application.
    fn close_document(&mut self) {
        if self.docs.len() <= 1 {
            self.close();
            return;
        }
        let order = projects::order(&self.open_files(), &self.projects.list);
        let at = order.iter().position(|&x| x == self.active).unwrap_or(0);
        let next = order
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|p| order.get(p)))
            .copied()
            .unwrap_or(0);
        self.bus.emit(&Event::DocumentClose { doc: self.doc_id });
        if let (Some(w), Some(p)) = (&mut self.watcher, &self.doc.meta.path) {
            let _ = w.unwatch(p);
        }
        let closing = self.active;
        self.activate(next);
        self.docs.remove(closing);
        if self.active > closing {
            self.active -= 1;
        }
        self.dirty = true;
    }

    /// Shows the next open document (in the list's order), or the previous.
    fn cycle(&mut self, back: bool) {
        let order = projects::order(&self.open_files(), &self.projects.list);
        let n = order.len();
        let at = order.iter().position(|&x| x == self.active).unwrap_or(0);
        let next = if back { (at + n - 1) % n } else { (at + 1) % n };
        self.activate(order[next]);
    }

    /// Opens picker `kind`; lists about a project offer the projects first
    /// when there is none.
    fn pick(&mut self, kind: PickKind, project: Option<PathBuf>, after: After) {
        let files = self.open_files();
        let current = self.doc.meta.path.clone();
        let picker = match projects::picker(
            kind,
            &files,
            current.as_deref(),
            project.as_deref(),
            &mut self.projects,
        ) {
            Some(mut p) => {
                p.after = after;
                Some(p)
            }
            None => projects::picker(PickKind::Projects, &files, None, None, &mut self.projects)
                .map(|mut p| {
                    p.after = After::Pick(kind);
                    p
                }),
        };
        if let Some(p) = picker {
            self.completion = None;
            self.palette = Some(Palette::picker(p));
        }
        self.dirty = true;
    }

    /// Searches the project at `root` (else the current one, after
    /// choosing a project when there is none).
    fn search_project(&mut self, root: Option<PathBuf>) {
        let Some(root) = root.or_else(|| self.project()) else {
            self.pick(PickKind::Projects, None, After::Search);
            if let Some(p) = self.palette.as_mut().and_then(|p| p.pick.as_mut()) {
                p.after = After::Search;
            }
            return;
        };
        let Some(project) = self.projects.list.get(&root).cloned() else {
            self.message(tr!("msg-no-project"), true);
            return;
        };
        let text = self
            .doc
            .selected_text()
            .filter(|t| !t.contains('\n'))
            .unwrap_or("")
            .to_string();
        self.completion = None;
        self.palette = Some(Palette::searching(ProjectSearch::new(&project, &text)));
        self.dirty = true;
    }

    /// Does what choosing `id` in `picker` means.
    fn picked(&mut self, picker: Picker, id: String) {
        match picker.kind {
            PickKind::Documents | PickKind::ProjectDocuments => {
                if let Ok(i) = id.parse() {
                    self.activate(i);
                }
            }
            PickKind::RecentFiles | PickKind::ProjectFiles | PickKind::ProjectRecentFiles => {
                self.open_path(Path::new(&id), None);
            }
            PickKind::Projects => {
                let root = PathBuf::from(id);
                self.projects.list.used(&root);
                if let Err(e) = self.projects.save() {
                    tracing::warn!("{e}");
                }
                let Some(p) = self.projects.list.get(&root).cloned() else {
                    return;
                };
                if !p.exists() {
                    self.message(tr!("msg-project-missing", name = p.name), true);
                    return;
                }
                let last = p.last_file.clone().filter(|f| f.is_file());
                match (picker.after, last) {
                    (After::Open, Some(f)) => self.open_path(&f, None),
                    (After::Open, None) => {
                        self.pick(PickKind::ProjectFiles, Some(root), After::Open)
                    }
                    (After::Pick(k), _) => self.pick(k, Some(root), After::Open),
                    (After::Search, _) => self.search_project(Some(root)),
                }
            }
            PickKind::RemoveProject => match self.projects.remove(Path::new(&id)) {
                Ok(m) => self.message(m, false),
                Err(m) => self.message(m, true),
            },
        }
    }

    /// Changes of the project list, and actions on the project's documents.
    fn project_request(&mut self, r: ProjectRequest) {
        let project = self.project();
        let result = match r {
            ProjectRequest::Add(Some(path)) => {
                let path = PathBuf::from(settings::expand_home(&path));
                self.projects.add(&path)
            }
            ProjectRequest::Add(None) => {
                let dir =
                    kalem_core::command::argument_default("project.add", "path", &mut self.doc);
                self.ask(
                    PromptKind::Arg {
                        command: "project.add".into(),
                        args: Value::Object(Default::default()),
                        name: "path".into(),
                        ty: "string".into(),
                    },
                    &format!("{}: ", tr!("prompt-project-add")),
                    dir,
                );
                return;
            }
            ProjectRequest::Rename(name) => match &project {
                Some(root) => self.projects.rename(root, &name),
                None => Err(tr!("msg-no-project")),
            },
            ProjectRequest::Refresh => match &project {
                Some(root) => {
                    self.projects.refresh(root);
                    Ok(String::new())
                }
                None => Err(tr!("msg-no-project")),
            },
            ProjectRequest::RevealInTree => match (&project, self.doc.meta.path.clone()) {
                (Some(root), Some(path)) => {
                    self.projects.reveal_in_tree(root, &path);
                    if self.files_at == FilesAt::Hidden {
                        self.files_at = FilesAt::Left;
                    }
                    self.dirty = true;
                    Ok(String::new())
                }
                _ => Err(tr!("msg-no-project")),
            },
            ProjectRequest::SaveAll => match &project {
                Some(root) => {
                    let n = self.save_all(Some(root));
                    Ok(tr!("msg-saved-count", count = n.to_string()))
                }
                None => Err(tr!("msg-no-project")),
            },
            ProjectRequest::CloseAll => match &project {
                Some(root) => {
                    self.close_project(root);
                    Ok(String::new())
                }
                None => Err(tr!("msg-no-project")),
            },
        };
        match result {
            Ok(m) if m.is_empty() => {}
            Ok(m) => self.message(m, false),
            Err(m) => self.message(m, true),
        }
    }

    /// Saves the modified documents with a file (of the project at
    /// `root`, or all); how many were saved.
    fn save_all(&mut self, root: Option<&Path>) -> usize {
        let under = |d: &DocumentState| {
            d.meta
                .path
                .as_deref()
                .is_some_and(|p| root.is_none_or(|r| projects::normal(p).starts_with(r)))
        };
        let options = self.config.save_options();
        let mut n = 0;
        for b in self.docs.iter_mut().flatten() {
            if b.doc.is_modified() && under(&b.doc) && b.doc.save(options, false).is_ok() {
                n += 1;
            }
        }
        if self.doc.is_modified() && under(&self.doc) {
            self.save(false);
            n += usize::from(!self.doc.is_modified());
        }
        n
    }

    /// Closes the unmodified documents of the project at `root`.
    fn close_project(&mut self, root: &Path) {
        let under = |d: &DocumentState| {
            !d.is_modified()
                && d.meta
                    .path
                    .as_deref()
                    .is_some_and(|p| projects::normal(p).starts_with(root))
        };
        loop {
            let i = self.docs.iter().enumerate().find_map(|(i, b)| match b {
                Some(b) if under(&b.doc) => Some(i),
                None if under(&self.doc) => Some(i),
                _ => None,
            });
            let Some(i) = i else { break };
            if self.docs.len() == 1 {
                break;
            }
            self.activate(i);
            self.close_document();
        }
    }

    /// How many open documents have unsaved changes.
    fn modified_count(&self) -> usize {
        self.docs
            .iter()
            .map(|b| match b {
                Some(b) => b.doc.is_modified(),
                None => self.doc.is_modified(),
            })
            .filter(|m| *m)
            .count()
    }

    /// Runs a command, asking for missing required arguments first.
    pub fn run_command(&mut self, id: &str, args: Value) {
        let Some(cmd) = self.registry.get(id) else {
            self.message(tr!("msg-unknown-command", id = id), true);
            return;
        };
        if let Some(when) = &cmd.when
            && !when.eval(&self.context())
        {
            self.message(
                tr!("msg-does-not-apply", command = cmd.display_title()),
                true,
            );
            return;
        }
        if let Some((name, ty)) = kalem_core::command::missing_argument(cmd, &args) {
            let input = kalem_core::command::argument_default_with(
                id,
                &name,
                &args,
                &mut self.doc,
                &self.config,
            );
            self.prompt = Some(Prompt {
                label: tr!(
                    "prompt-argument",
                    command = cmd.display_title(),
                    name = &name
                ),
                input,
                back: 0,
                kind: PromptKind::Arg {
                    command: id.to_string(),
                    args,
                    name,
                    ty,
                },
            });
            self.dirty = true;
            return;
        }
        let now = Instant::now();
        let clock = jiff::Zoned::now().datetime();
        let before = self.doc.selection;
        let mut ctx = EditorContext::new(
            Some(&mut self.doc),
            &mut self.clipboard,
            &self.config,
            now,
            clock,
        );
        let result = self.registry.execute(id, &mut ctx, &args);
        let requests = std::mem::take(&mut ctx.requests);
        let messages = std::mem::take(&mut ctx.messages);
        drop(ctx);
        tracing::debug!(command = id, ok = result.is_ok(), "command");
        match result {
            Ok(()) => {
                if let Some(m) = messages.last() {
                    self.message(m.clone(), false);
                }
            }
            Err(e) => self.message(e.message, true),
        }
        self.after_change(before != self.doc.selection);
        for r in requests {
            self.request(r);
        }
    }

    /// Updates the view after edits or cursor motion.
    fn after_change(&mut self, moved: bool) {
        let now = Instant::now();
        let changes = self.doc.take_changes();
        for tx in &changes {
            self.editor.map(tx);
            self.debouncer
                .record(self.doc_id, self.doc.version(), tx, now);
        }
        if moved || !changes.is_empty() {
            self.editor.follow = true;
            self.bus.emit(&Event::SelectionChanged {
                doc: self.doc_id,
                anchor: self.doc.selection.anchor,
                head: self.doc.selection.head,
            });
        }
        self.dirty = true;
    }

    fn request(&mut self, r: Request) {
        match r {
            Request::Save => self.save(false),
            Request::SaveAs => {
                let current = self
                    .doc
                    .meta
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.ask(PromptKind::SaveAs, &tr!("prompt-save-as"), current);
            }
            Request::Quit => match self.modified_count() {
                0 => self.close(),
                1 if self.doc.is_modified() => {
                    self.ask(PromptKind::Quit, &tr!("prompt-quit"), String::new());
                }
                n => self.ask(
                    PromptKind::Quit,
                    &tr!("prompt-quit-many", count = n.to_string()),
                    String::new(),
                ),
            },
            Request::Open { path: Some(p) } => {
                let path = PathBuf::from(settings::expand_home(&p));
                let path = match (&self.doc.meta.path, path.is_absolute()) {
                    (Some(doc), false) => doc.parent().map_or(path.clone(), |d| d.join(&path)),
                    _ => path,
                };
                self.open_path(&path, None);
            }
            Request::PickFile { command, arg, args } | Request::Ask { command, arg, args } => {
                let title = self
                    .registry
                    .get(&command)
                    .map_or_else(|| command.clone(), |c| c.display_title());
                let default = kalem_core::command::argument_default_with(
                    &command,
                    &arg,
                    &args,
                    &mut self.doc,
                    &self.config,
                );
                self.ask(
                    PromptKind::Arg {
                        command,
                        args,
                        name: arg.clone(),
                        ty: "string".into(),
                    },
                    &format!("{title}: {arg}: "),
                    default,
                );
            }
            Request::Open { path: None } => {
                let dir = kalem_core::command::argument_default("file.open", "path", &mut self.doc);
                self.ask(
                    PromptKind::Arg {
                        command: "file.open".into(),
                        args: Value::Object(Default::default()),
                        name: "path".into(),
                        ty: "string".into(),
                    },
                    &format!("{}: ", tr!("prompt-open")),
                    dir,
                );
            }
            Request::New => self.new_empty(),
            Request::Close => {
                if self.doc.is_modified() {
                    let name = self.open_files()[self.active].title.clone();
                    self.ask(
                        PromptKind::Close,
                        &tr!("prompt-close", name = name),
                        String::new(),
                    );
                } else {
                    self.close_document();
                }
            }
            Request::Cycle { back } => self.cycle(back),
            Request::Pick(kind) => self.pick(kind, None, After::Open),
            Request::SearchProject => self.search_project(None),
            Request::SearchIn(dir) => {
                // The project's search when the folder is one, else the
                // folder's.
                let project = self
                    .projects
                    .list
                    .get(&dir)
                    .cloned()
                    .unwrap_or_else(|| kalem_core::projects::Project::new(dir));
                self.completion = None;
                self.palette = Some(Palette::searching(ProjectSearch::new(&project, "")));
                self.dirty = true;
            }
            Request::OpenFiles => {
                if self.files_at == FilesAt::Hidden {
                    self.files_at = FilesAt::Left;
                    self.files_shown = true;
                } else {
                    self.files_shown = !self.files_shown;
                }
                self.editor.follow = true;
                self.dirty = true;
            }
            Request::Project(r) => self.project_request(r),
            Request::FileManager(r) => match r {
                FileManagerRequest::Dir { dir } => {
                    let (dir, select) = match (dir, self.doc.meta.path.clone()) {
                        (Some(d), _) => (d, None),
                        // From a listing: its parent, the cursor on it.
                        (None, Some(p)) if self.doc.dired.is_some() => {
                            (p.parent().map_or(p.clone(), Path::to_path_buf), Some(p))
                        }
                        (None, Some(p)) => {
                            let p = std::path::absolute(&p).unwrap_or(p);
                            (p.parent().map_or(p.clone(), Path::to_path_buf), Some(p))
                        }
                        (None, None) => (std::env::current_dir().unwrap_or_default(), None),
                    };
                    self.file_manager(Place::Dir(projects::normal(&dir)), select);
                }
                FileManagerRequest::ProjectRoot => match self.project() {
                    Some(root) => {
                        let select = self.doc.meta.path.clone();
                        self.file_manager(Place::Dir(root), select);
                    }
                    None => self.message(tr!("msg-no-project"), true),
                },
                FileManagerRequest::Projects { select } => {
                    self.file_manager(Place::Projects, select)
                }
                FileManagerRequest::Leave => self.leave_file_manager(),
            },
            Request::FileOp(op) => match kalem_core::dired::Task::new(&op) {
                Ok(t) => {
                    self.task = Some(t);
                    self.next_question();
                }
                Err(e) => self.message(e, true),
            },
            Request::Preview { .. } => self.message(tr!("fm-preview-graphical"), false),
            Request::Shell(op) => match kalem_core::dired::Task::shell(&op) {
                Ok(t) => {
                    self.task = Some(t);
                    self.next_question();
                }
                Err(e) => self.message(e, true),
            },
            Request::CancelFileOps => {
                for j in &self.jobs {
                    j.cancel();
                }
            }
            Request::CopyText(t) => {
                self.clipboard.text = t.clone();
                self.write_terminal(&osc52(&t));
            }
            Request::Complete => self.request_completion(),
            // The terminal's clipboard takes plain text only.
            Request::CopyRich { text, .. } => {
                self.clipboard.text = text.clone();
                self.write_terminal(&osc52(&text));
                self.message(tr!("msg-rich-copy-plain"), false);
            }
            Request::Choose(items) => {
                self.palette = Some(Palette::new(items));
                self.dirty = true;
            }
            Request::ExportDialog => {
                let items = kalem_core::export_dialog_items(&self.config);
                self.palette = Some(Palette::new(items));
                self.dirty = true;
            }
            Request::SetSetting { key, value, quiet } => self.set_setting(&key, &value, quiet),
            Request::Copy | Request::Cut => {
                let Some(text) = self.doc.copy_text() else {
                    self.message(tr!("msg-nothing-selected"), false);
                    return;
                };
                self.clipboard.text = text.clone();
                self.write_terminal(&osc52(&text));
                if r == Request::Cut {
                    self.doc.cut_selections(Instant::now());
                    self.after_change(true);
                }
            }
            Request::Paste { plain } => {
                let text = self.clipboard.text.clone();
                self.paste(&text, plain);
            }
            Request::ToggleSource => {
                self.editor.source = !self.editor.source;
                self.editor.follow = true;
                let which = if self.editor.source {
                    tr!("msg-source-view")
                } else {
                    tr!("msg-rich-view")
                };
                self.message(which, false);
            }
            Request::Split => self.message(tr!("msg-split-in-gui"), false),
            Request::ToggleWrap => {
                self.editor.wrap = !self.editor.wrap;
                self.editor.follow = true;
                let m = if self.editor.wrap {
                    "msg-wrap-on"
                } else {
                    "msg-wrap-off"
                };
                self.message(tr!(m), false);
            }
            Request::ModeChanged => {
                self.editor.reset();
                self.refresh_vim();
                self.dirty = true;
            }
            Request::Focus => {
                self.editor.focus = !self.editor.focus;
                self.editor.follow = true;
                let m = if self.editor.focus {
                    "msg-focus-on"
                } else {
                    "msg-focus-off"
                };
                self.message(tr!(m), false);
            }
            Request::Settings => {
                let path = settings::config_dir().map(|d| d.join("settings.toml"));
                let text = match path {
                    Some(p) => tr!("msg-settings-in", path = p.display().to_string()),
                    None => tr!("msg-no-settings-dir"),
                };
                self.message(text, false);
            }
            Request::Fold { global } => self.fold(global),
            Request::OpenLink(action) => self.open_link(action),
            Request::Palette => self.open_palette(),
            Request::Menus => {
                let ctx = self.context();
                let items =
                    kalem_core::palette::menu_items(&self.registry, &self.keymap, &ctx, |k| {
                        k.to_string()
                    });
                self.palette = Some(Palette::new(items));
                self.dirty = true;
            }
            Request::Find { replace } => self.open_find(replace),
            Request::Outline => self.toggle_outline(),
            Request::ToggleMath => {
                self.editor.raw_math = !self.editor.raw_math;
                self.editor.follow = true;
                let m = if self.editor.raw_math {
                    "msg-math-off"
                } else {
                    "msg-math-on"
                };
                self.message(tr!(m), false);
            }
        }
    }

    /// Saves `key` in the user's settings and reads the settings again.
    fn set_setting(&mut self, key: &str, value: &serde_json::Value, quiet: bool) {
        let Some(path) = settings::config_dir().map(|d| d.join("settings.toml")) else {
            self.message(tr!("msg-no-settings-dir"), true);
            return;
        };
        if let Err(e) = settings::save_setting(&path, key, value) {
            self.message(e, true);
            return;
        }
        let workspace = self
            .config
            .sources()
            .iter()
            .find(|(l, _)| *l == settings::Layer::Workspace)
            .and_then(|(_, p)| p.clone());
        self.config = Config::load(Some(&path), workspace.as_deref());
        self.config.apply_process_settings();
        if !quiet {
            self.message(tr!("msg-setting-saved", key = key), false);
        }
    }

    /// Opens a link target with the system's opener.
    fn open_link(&mut self, action: kalem_core::input::LinkAction) {
        use kalem_core::input::LinkAction;
        let target = match action {
            LinkAction::Url(url) => url,
            LinkAction::File { path, .. } => {
                let p = std::path::PathBuf::from(path.trim_start_matches("file:"));
                match (&self.doc.meta.path, p.is_absolute()) {
                    (Some(doc), false) => doc.parent().map_or(p.clone(), |d| d.join(&p)),
                    _ => p,
                }
                .display()
                .to_string()
            }
            LinkAction::Jump(_) => return,
            LinkAction::Missing(s) => {
                self.message(tr!("msg-no-match-for", target = s), true);
                return;
            }
            LinkAction::Print(pdf) => {
                match kalem_core::print::run(&pdf) {
                    Ok(m) => self.message(m, false),
                    Err(e) => self.message(e, true),
                }
                return;
            }
            LinkAction::System(path) => path.display().to_string(),
            LinkAction::Reveal(path) => {
                let name = path.display().to_string();
                match kalem_core::system::reveal(&path) {
                    Ok(()) => self.message(tr!("msg-opened", target = &name), false),
                    Err(e) => self.message(e, true),
                }
                return;
            }
        };
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        match std::process::Command::new(opener)
            .arg(&target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => self.message(tr!("msg-opened", target = &target), false),
            Err(e) => self.message(
                tr!("msg-cannot-open", target = &target, error = e.to_string()),
                true,
            ),
        }
    }

    fn fold(&mut self, global: bool) {
        if global {
            let blocks = self.editor.all_blocks(&self.doc);
            let (next, option) = match self.global_fold {
                Visibility::Subtree => (Visibility::Folded, "overview"),
                Visibility::Folded => (Visibility::Children, "content"),
                Visibility::Children => (Visibility::Subtree, "showall"),
            };
            self.global_fold = next;
            self.editor.folds = kalem_core::view::Folds::startup(&blocks, option);
            self.message(
                tr!(match next {
                    Visibility::Folded => "msg-visibility-overview",
                    Visibility::Children => "msg-visibility-contents",
                    Visibility::Subtree => "msg-visibility-all",
                }),
                false,
            );
        } else if let Some((blocks, i)) = self.editor.heading_at(&self.doc) {
            self.editor.folds.cycle(&blocks, i);
        }
        self.editor.follow = true;
        self.dirty = true;
    }

    fn ask(&mut self, kind: PromptKind, label: &str, input: String) {
        self.prompt = Some(Prompt {
            kind,
            label: label.to_string(),
            input,
            back: 0,
        });
        self.dirty = true;
    }

    fn write_terminal(&mut self, s: &str) {
        self.output.push(s.to_string());
    }

    /// Escape sequences to write to the terminal.
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    /// Saves, asking the event bus first.
    fn save(&mut self, force: bool) {
        let Some(path) = self.doc.meta.path.clone() else {
            self.request(Request::SaveAs);
            return;
        };
        let event = Event::DocumentBeforeSave {
            doc: self.doc_id,
            path: path.clone(),
        };
        let outcome = self.bus.emit_vetoable(&event, Instant::now()).wait();
        self.bus.settle(&event, &outcome);
        if let Some((_, reason)) = outcome.veto {
            self.message(tr!("msg-not-saved", reason = reason), true);
            return;
        }
        self.doc.before_save(&self.config, Instant::now());
        self.after_change(true);
        match self.doc.save(self.config.save_options(), force) {
            Ok(()) => {
                if let Some(w) = &mut self.watcher {
                    let _ = w.watch(&path);
                }
                self.bus.emit(&Event::DocumentAfterSave {
                    doc: self.doc_id,
                    path: path.clone(),
                });
                self.message(
                    tr!("msg-saved-as", path = path.display().to_string()),
                    false,
                );
                // A LaTeX document builds on save when asked to, one build
                // at a time.
                if self.doc.latex().is_some()
                    && self.config.bool("latex.build_on_save")
                    && !kalem_core::jobs::running()
                {
                    self.run_command("latex.build", serde_json::Value::Null);
                }
            }
            Err(kalem_core::document::SaveError::ChangedOnDisk) => {
                self.ask(
                    PromptKind::Overwrite,
                    &tr!("prompt-overwrite"),
                    String::new(),
                );
            }
            Err(e) => self.message(tr!("msg-not-saved", reason = e.to_string()), true),
        }
    }

    fn close(&mut self) {
        self.bus.emit(&Event::DocumentClose { doc: self.doc_id });
        self.quit = true;
    }

    /// Types `text` over the selection.
    pub fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let text = if self.doc.meta.line_ending != LineEnding::CrLf {
            text.replace("\r\n", "\n")
        } else {
            text.to_string()
        };
        self.doc.insert_text(&text, Instant::now());
        self.editor.viewport.goal_x = None;
        self.after_change(true);
    }

    /// Pastes `text`: tab-separated values become a table unless `plain`
    /// ([`kalem_core::DocumentState::paste`]).
    pub fn paste(&mut self, text: &str, plain: bool) {
        if text.is_empty() {
            return;
        }
        self.doc.paste(text, None, plain, Instant::now());
        self.editor.viewport.goal_x = None;
        self.after_change(true);
    }

    /// Handles a terminal event.
    pub fn event(&mut self, e: TermEvent) {
        match e {
            TermEvent::Key(k) => self.key(k),
            TermEvent::Paste(text) => {
                if let Some(p) = &mut self.prompt {
                    let line = text.lines().next().unwrap_or("");
                    kalem_core::line_edit::insert(&mut p.input, p.back, line);
                    self.dirty = true;
                } else {
                    self.paste(&text, false);
                }
            }
            TermEvent::Mouse(m) => self.mouse(m),
            TermEvent::Resize(..) | TermEvent::FocusGained => {
                self.editor.follow = true;
                self.dirty = true;
            }
            TermEvent::FocusLost => {}
        }
    }

    fn mouse(&mut self, m: crossterm::event::MouseEvent) {
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        if let MouseEventKind::Down(MouseButton::Left) = m.kind
            && let Some(&(_, id)) = self
                .action_spots
                .iter()
                .find(|(r, _)| r.contains(ratatui::layout::Position::new(m.column, m.row)))
        {
            self.run_command(id, Value::Null);
            return;
        }
        if let MouseEventKind::Down(MouseButton::Left) = m.kind
            && let Some(&(_, i)) = self
                .tree_spots
                .iter()
                .find(|(r, _)| r.contains(ratatui::layout::Position::new(m.column, m.row)))
            && let Some(row) = self.tree_rows.get(i).cloned()
        {
            if row.dir {
                if let Some(root) = self.project() {
                    self.projects.toggle_tree(&root, &row.path);
                }
                self.dirty = true;
            } else {
                self.open_path(&row.path, None);
            }
            return;
        }
        if let MouseEventKind::Down(MouseButton::Left) = m.kind
            && let Some(&(_, i)) = self
                .file_spots
                .iter()
                .find(|(r, _)| r.contains(ratatui::layout::Position::new(m.column, m.row)))
        {
            self.activate(i);
            return;
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .outline
                    .as_ref()
                    .is_some_and(|_| m.column < self.editor.area.x) =>
            {
                if let Some(o) = &mut self.outline
                    && let Some(i) = o.at_row(m.row)
                {
                    o.selected = i;
                    self.outline_jump();
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some((pos, widget)) = self.editor.hit(&self.doc, &self.caps, m.column, m.row)
                else {
                    return;
                };
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(t, x, y)| {
                    now.duration_since(t) < Duration::from_millis(400)
                        && x == m.column
                        && y == m.row
                });
                self.last_click = Some((now, m.column, m.row));
                if let Some((Widget::Checkbox(_), start, _)) = widget {
                    self.doc.move_cursor(start, false);
                    self.run_command("list.toggleCheckbox", Value::Null);
                    return;
                }
                // A row of a table of contents leads to its heading.
                if let Some((Widget::TocRow { start }, ..)) = widget {
                    self.doc.move_cursor(start, false);
                    self.after_change(true);
                    return;
                }
                // The file manager: a click on a name (or a double click on
                // its line) opens it.
                if let Some(d) = self.doc.dired.as_deref() {
                    let text = self.doc.text();
                    let line = text.line_of(pos);
                    let col = pos - text.line_start(line);
                    let on_name = d
                        .name_range(line)
                        .is_some_and(|r| r.start <= col && col <= r.end);
                    // Ctrl-click marks or unmarks an entry, Shift-click
                    // marks from the cursor to it.
                    let ctrl = m.modifiers.contains(KeyModifiers::CONTROL);
                    if (ctrl || shift) && d.path_at(line).is_some() {
                        let marked = d.path_at(line).is_some_and(|p| d.marks.contains_key(&p));
                        if shift {
                            let at = self.doc.selection.head;
                            self.doc.move_cursor(at, false);
                            self.doc.move_cursor(pos, true);
                            self.run_command("dired.mark", Value::Null);
                        } else {
                            self.doc.move_cursor(pos, false);
                            let id = if marked { "dired.unmark" } else { "dired.mark" };
                            self.run_command(id, Value::Null);
                        }
                        let start = self.doc.text().line_start(line);
                        self.doc.move_cursor(start, false);
                        self.after_change(true);
                        return;
                    }
                    if !shift && (on_name || double) && d.path_at(line).is_some() {
                        self.doc.move_cursor(pos, false);
                        self.after_change(true);
                        self.run_command("dired.open", Value::Null);
                        return;
                    }
                }
                // Alt-click: a cursor more (or one fewer).
                if m.modifiers.contains(KeyModifiers::ALT) && !shift && !double {
                    self.doc.toggle_cursor_at(pos);
                    self.editor.viewport.goal_x = None;
                    self.after_change(true);
                    return;
                }
                self.doc.clear_extra();
                if double {
                    self.select_word(pos);
                } else {
                    self.doc.move_cursor(pos, shift);
                }
                self.editor.viewport.goal_x = None;
                self.after_change(true);
            }
            // A right click in the file manager: its menu, for the entry
            // under the mouse (the marked ones when it is one of them) or
            // for the listing (T2.7e.17).
            MouseEventKind::Down(MouseButton::Right) if self.doc.dired.is_some() => {
                let hit = self
                    .editor
                    .hit(&self.doc, &self.caps, m.column, m.row)
                    .map(|(pos, _)| pos);
                let d = self.doc.dired.as_deref().expect("a listing");
                let path = hit.and_then(|pos| d.path_at(self.doc.text().line_of(pos)));
                if let (Some(p), Some(pos)) = (&path, hit)
                    && !d.marks.contains_key(p)
                {
                    self.doc.move_cursor(pos, false);
                    self.after_change(true);
                }
                let args = serde_json::json!({ "listing": path.is_none() });
                self.run_command("dired.contextMenu", args);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some((pos, _)) = self.editor.hit(&self.doc, &self.caps, m.column, m.row) {
                    self.doc.move_cursor(pos, true);
                    self.after_change(true);
                }
            }
            MouseEventKind::ScrollDown => {
                self.editor.scroll(&self.doc, &self.caps, 3);
                self.dirty = true;
            }
            MouseEventKind::ScrollUp => {
                self.editor.scroll(&self.doc, &self.caps, -3);
                self.dirty = true;
            }
            _ => {}
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
                seen |= !word || i > 0;
                if word && i == 0 {
                    seen = true;
                }
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

    fn open_palette(&mut self) {
        let ctx = self.context();
        let items =
            kalem_core::palette::items(&self.registry, &self.keymap, &ctx, |k| k.to_string());
        self.palette = Some(Palette::new(items));
        self.dirty = true;
    }

    fn palette_key(&mut self, k: &KeyEvent) {
        let Some(p) = &mut self.palette else { return };
        self.dirty = true;
        let n = p.len();
        // Alt+C, Alt+W, Alt+R: the search's switches.
        if let (Some(s), KeyCode::Char(c)) = (&mut p.search, k.code)
            && k.modifiers.contains(KeyModifiers::ALT)
            && "cwr".contains(c)
        {
            s.toggle(c);
            return;
        }
        // The arrows, Home, End and the deletions edit the typed text.
        if let Some(edit) = line_key(k) {
            if kalem_core::line_edit::apply(&mut p.input, &mut p.back, edit) {
                p.selected = 0;
                if let Some(s) = &mut p.search {
                    s.set_text(&p.input);
                }
            }
            return;
        }
        match k.code {
            KeyCode::Esc => {
                if let Some(s) = &mut p.search {
                    s.cancel();
                }
                self.palette = None;
            }
            KeyCode::Enter => {
                let Some(mut p) = self.palette.take() else {
                    return;
                };
                if let Some(mut s) = p.search.take() {
                    s.cancel();
                    if let Some(h) = s.hits.get(p.selected).cloned() {
                        self.open_path(&h.path, Some((h.line, h.column)));
                    }
                    return;
                }
                let id = p.chosen();
                if let (Some(picker), Some(id)) = (p.pick.take(), id.clone()) {
                    self.picked(picker, id);
                    return;
                }
                if let Some(id) = id {
                    // A picker's item carries its command's arguments.
                    let (command, args) = kalem_core::palette::split_invocation(&id);
                    let command = command.to_string();
                    self.run_command(&command, args);
                    self.last_command = Some(command);
                }
            }
            KeyCode::Down if n > 0 => p.selected = (p.selected + 1) % n,
            KeyCode::Up if n > 0 => p.selected = (p.selected + n - 1) % n,
            KeyCode::PageDown if n > 0 => p.selected = (p.selected + 10).min(n - 1),
            KeyCode::PageUp => p.selected = p.selected.saturating_sub(10),
            _ => {
                if let Some(c) = input::text(k) {
                    kalem_core::line_edit::insert(&mut p.input, p.back, c.encode_utf8(&mut [0; 4]));
                    p.selected = 0;
                    if let Some(s) = &mut p.search {
                        s.set_text(&p.input);
                    }
                }
            }
        }
    }

    /// Background work of an open list: search results, files found.
    fn tick_palette(&mut self) {
        let Some(p) = &mut self.palette else { return };
        if let Some(s) = &mut p.search {
            if s.poll() {
                self.dirty = true;
            }
            return;
        }
        if let Some(pick) = p.pick.as_mut().filter(|k| k.partial)
            && let Some(mut fresh) = projects::picker(
                pick.kind,
                &[],
                None,
                pick.project.as_deref(),
                &mut self.projects,
            )
        {
            fresh.after = pick.after;
            *pick = fresh;
            self.dirty = true;
        }
    }

    fn open_find(&mut self, replace: bool) {
        let selected = self
            .doc
            .selected_text()
            .filter(|t| !t.contains('\n'))
            .map(str::to_string);
        let query = selected.unwrap_or_else(|| self.last_query.clone());
        let s = self.doc.selection;
        let f = Find {
            query,
            replacement: replace.then(String::new),
            on_replacement: false,
            origin: s.anchor.min(s.head),
            regex: self.last_regex,
            ..Find::default()
        };
        self.find = Some(f);
        self.search(false, true);
    }

    /// Moves to the next (or previous) match; `from_origin`: from where
    /// the search started, for typing in the query.
    fn search(&mut self, backward: bool, from_origin: bool) {
        let Some(f) = &mut self.find else { return };
        match kalem_core::find::find_with(self.doc.text().as_str(), &f.query, f.options()) {
            Ok(m) => {
                f.matches = m;
                f.error = None;
            }
            Err(e) => {
                f.matches.clear();
                f.error = Some(e);
            }
        }
        self.editor.highlights = f.matches.clone();
        let s = self.doc.selection;
        let from = if from_origin {
            f.origin
        } else if backward {
            s.anchor.min(s.head)
        } else {
            s.anchor.max(s.head)
        };
        if let Some(m) = kalem_core::find::next(&f.matches, from, backward) {
            self.doc.move_cursor(m.start, false);
            self.doc.move_cursor(m.end, true);
            self.editor.follow = true;
        }
        self.dirty = true;
    }

    /// Keys for the find bar; `false` for keys that go to the editor.
    fn find_key(&mut self, k: &KeyEvent) -> bool {
        let Some(f) = &mut self.find else {
            return false;
        };
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        match k.code {
            KeyCode::Esc => {
                self.last_query = f.query.clone();
                self.last_regex = f.regex;
                self.find = None;
                self.editor.highlights.clear();
                self.dirty = true;
            }
            KeyCode::Char('r') if alt => {
                f.regex = !f.regex;
                self.search(false, true);
            }
            KeyCode::Enter if alt => {
                let (q, r, opts) = (f.query.clone(), f.replacement.clone(), f.options());
                self.last_regex = f.regex;
                if let Some(r) = r
                    && let Ok(Some(tx)) =
                        kalem_core::find::replace_all_with(self.doc.text().as_str(), &q, &r, opts)
                {
                    let n = tx.edits.len();
                    self.doc
                        .apply(&tx, org_edit::ChangeKind::Command, Instant::now());
                    self.after_change(true);
                    self.message(tr!("msg-replaced", count = n), false);
                }
                self.last_query = q;
                self.find = None;
                self.editor.highlights.clear();
            }
            KeyCode::Enter if f.on_replacement => {
                let s = self.doc.selection;
                let sel = s.anchor.min(s.head)..s.anchor.max(s.head);
                let r = kalem_core::find::replacement(
                    self.doc.text().as_str(),
                    sel.clone(),
                    &f.query,
                    f.replacement.as_deref().unwrap_or(""),
                    f.options(),
                );
                if f.matches.contains(&sel)
                    && let Ok(r) = r
                {
                    let mut tx = org_edit::Transaction::new("Replace");
                    tx.replace(sel.clone(), r.clone()).expect("one edit");
                    let tx = tx.select(org_edit::Selection::caret(sel.start + r.len()));
                    self.doc
                        .apply(&tx, org_edit::ChangeKind::Command, Instant::now());
                    self.after_change(true);
                }
                self.search(false, false);
            }
            KeyCode::Enter | KeyCode::Down => self.search(shift, false),
            KeyCode::Up => self.search(true, false),
            KeyCode::Tab if f.replacement.is_some() => {
                f.on_replacement = !f.on_replacement;
                self.dirty = true;
            }
            KeyCode::Backspace => {
                if f.on_replacement {
                    f.replacement.as_mut().map(String::pop);
                    self.dirty = true;
                } else {
                    f.query.pop();
                    self.search(false, true);
                }
            }
            _ => {
                let Some(c) = input::text(k) else {
                    return false;
                };
                if f.on_replacement {
                    f.replacement.get_or_insert_default().push(c);
                    self.dirty = true;
                } else {
                    f.query.push(c);
                    self.search(false, true);
                }
            }
        }
        true
    }

    fn outline_items(&mut self) -> Vec<OutlineItem> {
        self.doc
            .model()
            .map(|m| kalem_core::view::outline_items(&m))
            .or_else(|| kalem_core::latex_view::outline_items(&self.doc))
            .unwrap_or_default()
    }

    fn toggle_outline(&mut self) {
        match &mut self.outline {
            Some(o) if o.focus => self.outline = None,
            Some(o) => o.focus = true,
            None => {
                let items = self.outline_items();
                let head = self.doc.selection.head;
                let selected = items.iter().rposition(|i| i.start <= head).unwrap_or(0);
                self.outline = Some(OutlinePanel::new(items, selected, self.doc.version()));
            }
        }
        self.dirty = true;
    }

    /// Jumps to the outline's chosen heading.
    fn outline_jump(&mut self) {
        let Some(o) = &mut self.outline else { return };
        let Some(start) = o.items.get(o.selected).map(|i| i.start) else {
            return;
        };
        o.focus = false;
        self.doc.move_cursor(start, false);
        self.editor.viewport.goal_x = None;
        self.after_change(true);
    }

    /// Keys for the focused outline; `false` for keys it leaves alone.
    fn outline_key(&mut self, k: &KeyEvent) -> bool {
        let Some(o) = &mut self.outline else {
            return false;
        };
        let n = o.items.len();
        self.dirty = true;
        match k.code {
            KeyCode::Down if n > 0 => o.selected = (o.selected + 1).min(n - 1),
            KeyCode::Up => o.selected = o.selected.saturating_sub(1),
            KeyCode::PageDown if n > 0 => o.selected = (o.selected + 10).min(n - 1),
            KeyCode::PageUp => o.selected = o.selected.saturating_sub(10),
            KeyCode::Home => o.selected = 0,
            KeyCode::End => o.selected = n.saturating_sub(1),
            KeyCode::Enter | KeyCode::Right => self.outline_jump(),
            KeyCode::Esc | KeyCode::Tab | KeyCode::Left => o.focus = false,
            // Other keys run commands (to close the outline, say) but do
            // not type.
            _ => return input::text(k).is_some(),
        }
        true
    }

    /// Opens, updates or closes the completion menu after typing.
    fn update_completion(&mut self) {
        self.completion = kalem_core::completers::Menu::update(
            self.completion.take(),
            &self.completers,
            &mut self.doc,
            false,
        );
        self.dirty = true;
    }

    /// Opens the completion menu on request (Alt+/).
    fn request_completion(&mut self) {
        self.completion = kalem_core::completers::Menu::update(
            self.completion.take(),
            &self.completers,
            &mut self.doc,
            true,
        );
        if self.completion.is_none() {
            self.message(tr!("msg-no-completions"), false);
        }
        self.dirty = true;
    }

    /// Keys for the completion menu; `false` for keys it leaves alone.
    fn completion_key(&mut self, k: &KeyEvent) -> bool {
        let Some(m) = &mut self.completion else {
            return false;
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Down => m.step(true),
            KeyCode::Char('n') if ctrl => m.step(true),
            KeyCode::Up => m.step(false),
            KeyCode::Char('p') if ctrl => m.step(false),
            KeyCode::Esc => self.completion = None,
            // Words are taken with Tab: Enter goes on writing prose.
            KeyCode::Enter
                if m.current()
                    .is_some_and(|i| i.kind == kalem_core::completers::Kind::Word) =>
            {
                self.completion = None;
                return false;
            }
            KeyCode::Enter | KeyCode::Tab => {
                let item = m.current().cloned();
                self.completion = None;
                if let Some(item) = item {
                    kalem_core::completers::apply(&mut self.doc, &item, Instant::now());
                }
                self.after_change(true);
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// Draws the completion menu, or the formula preview, below the cursor.
    fn draw_popup(&self, buf: &mut ratatui::buffer::Buffer, area: Rect, cursor: (u16, u16)) {
        let bg = crate::panels::panel_style(&self.caps);
        let lines: Vec<(String, bool)> = if let Some(m) = &self.completion {
            m.rows(8)
                .into_iter()
                .map(|(label, _, chosen)| (label, chosen))
                .collect()
        } else if let Some((p, true)) = self.doc.parse()
            && !self.editor.source
            && let Some(f) = kalem_core::input::formula_at(&p.syntax(), self.doc.selection.head)
        {
            vec![(format!("= {}", kalem_core::math::unicode(&f)), false)]
        } else if !self.editor.source
            && let Some(f) = kalem_core::latex_view::formula_at(&self.doc, self.doc.selection.head)
        {
            // LaTeX: the formula the cursor is in shows its source there.
            vec![(format!("= {}", kalem_core::math::unicode(&f)), false)]
        } else {
            return;
        };
        let width = lines.iter().map(|(l, _)| l.width()).max().unwrap_or(0) as u16 + 2;
        let n = lines.len() as u16;
        let below = cursor.1 + 1 + n <= area.bottom();
        let x = cursor.0.min(area.right().saturating_sub(width));
        for (k, (label, selected)) in lines.iter().enumerate() {
            let k = k as u16;
            let y = if below {
                cursor.1 + 1 + k
            } else {
                cursor.1.saturating_sub(n - k)
            };
            if y >= area.bottom() {
                break;
            }
            let style = if *selected {
                bg.add_modifier(Modifier::REVERSED)
            } else {
                bg
            };
            for dx in 0..width.min(area.right() - x) {
                buf[(x + dx, y)].set_symbol(" ").set_style(style);
            }
            buf.set_stringn(x + 1, y, label, (width - 1) as usize, style);
        }
    }

    /// Handles a key.
    pub fn key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        if self.prompt.is_some() {
            self.prompt_key(k);
            return;
        }
        if self.palette.is_some() {
            self.palette_key(&k);
            return;
        }
        if self.find.is_some() && self.find_key(&k) {
            return;
        }
        if self.outline.as_ref().is_some_and(|o| o.focus) && self.outline_key(&k) {
            return;
        }
        if self.completion.is_some() && self.completion_key(&k) {
            return;
        }
        if self.pending.is_empty() && !self.listing_key(&k) && self.vim_key(&k) {
            return;
        }
        if let Some(chord) = input::chord(&k, self.caps.kitty_keyboard) {
            self.pending.push(chord);
            let seq = KeySequence(self.pending.clone());
            let ctx = self.context();
            match self.keymap.lookup(&seq, &ctx) {
                Lookup::Command { command, args } => {
                    let (command, args) = (command.to_string(), args.clone());
                    self.pending.clear();
                    self.status = None;
                    self.run_command(&command, args);
                    self.last_command = Some(command);
                    return;
                }
                Lookup::Prefix => {
                    // The keys that may follow show above the status line.
                    self.message(format!("{seq} -"), false);
                    return;
                }
                Lookup::None => {
                    self.pending.clear();
                    if seq.0.len() > 1 {
                        self.message(tr!("msg-not-bound", keys = seq.to_string()), true);
                        return;
                    }
                }
            }
        }
        self.edit_key(k);
    }

    /// Whether the keymap takes `k` before Vim: in a file manager listing
    /// outside Vim's insert mode and command line, the keys it binds.
    fn listing_key(&self, k: &KeyEvent) -> bool {
        let Some(v) = &self.vim else { return false };
        if v.takes_text() || v.command_line.is_some() || !v.idle_command() {
            return false;
        }
        let Some(chord) = input::chord(k, self.caps.kitty_keyboard) else {
            return false;
        };
        let seq = KeySequence(vec![chord]);
        let ctx = self.context();
        // Keys bound for the Vim layer (`-`), and the file manager's.
        self.keymap.vim_bound(&seq, &ctx)
            || (self.doc.dired.is_some() && !matches!(self.keymap.lookup(&seq, &ctx), Lookup::None))
    }

    /// A key for the Vim layer; `true` if it used it.
    fn vim_key(&mut self, k: &KeyEvent) -> bool {
        use kalem_core::vim::Key;
        let Some(mut v) = self.vim.take() else {
            return false;
        };
        let key = vim_key_of(k);
        if v.takes_text() && !matches!(key, Key::Esc | Key::Ctrl('[')) {
            self.vim = Some(v);
            return false;
        }
        let out = {
            let mut host = TuiHost {
                clip: &mut self.clipboard.text,
                output: &mut self.output,
                lines: usize::from(self.editor.area.height.max(4)),
                rich: !self.editor.source,
            };
            v.key(&mut self.doc, key, &mut host)
        };
        self.vim = Some(v);
        self.update_cursor_shape();
        self.dirty = true;
        if !out.handled {
            return false;
        }
        if let Some(h) = out.highlights {
            self.editor.highlights = h;
        }
        match out.message {
            Some((m, error)) => self.message(m, error),
            None if !out.commands.is_empty() => {}
            None => self.status = None,
        }
        self.editor.viewport.goal_x = None;
        self.after_change(true);
        for (id, args) in out.commands {
            self.run_command(&id, args);
        }
        if out.force_quit {
            self.close();
        }
        true
    }

    /// Starts or stops the Vim layer as the settings and the document's
    /// mode say.
    fn refresh_vim(&mut self) {
        let on = self.config.keymap_profile() == kalem_core::Profile::Vim
            && kalem_core::vim::Vim::applies(
                &self.config.strings("editor.vim.modes"),
                &self.doc.meta.mode,
            );
        match (on, self.vim.is_some()) {
            (true, false) => {
                let mut v = kalem_core::vim::Vim::new();
                v.leader = kalem_core::vim::Vim::leader_key(&self.config.vim_leader());
                self.vim = Some(v);
            }
            (false, true) => {
                self.vim = None;
                self.write_terminal("\x1b[0 q");
                self.cursor_block = None;
            }
            _ => {}
        }
        self.update_cursor_shape();
    }

    /// A block cursor outside Vim's insert mode, the terminal's own shape
    /// otherwise.
    fn update_cursor_shape(&mut self) {
        let block = self.vim.as_ref().map(|v| v.block_cursor());
        if block != self.cursor_block {
            self.cursor_block = block;
            match block {
                Some(true) => self.write_terminal("\x1b[2 q"),
                Some(false) => self.write_terminal("\x1b[6 q"),
                None => {}
            }
        }
    }

    /// Keys the keymap leaves to the editor: typing, deleting, motion.
    fn edit_key(&mut self, k: KeyEvent) {
        let now = Instant::now();
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let word = k
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let last = self.last_command.take();
        if let Some(c) = input::text(&k) {
            // After moving to a table field, the first key replaces it.
            let blank = matches!(
                last.as_deref(),
                Some("table.nextField" | "table.previousField" | "table.nextRow" | "table.align")
            );
            let typed = c.encode_utf8(&mut [0; 4]).to_string();
            // In a CSV grid a delimiter or quote goes into the value.
            if self.editor.source || !self.doc.type_in_grid(&typed, now) {
                self.doc.type_text(&typed, blank, now);
            }
            self.editor.viewport.goal_x = None;
            self.after_change(true);
            self.update_completion();
            return;
        }
        let head = self.doc.selection.head;
        match k.code {
            KeyCode::Enter => {
                self.insert("\n");
            }
            KeyCode::Tab | KeyCode::BackTab if !matches!(self.doc.meta.mode, DocumentMode::Org) => {
                let outdent = k.code == KeyCode::BackTab || shift;
                self.doc.indent(outdent, now);
                self.editor.viewport.goal_x = None;
                self.after_change(true);
            }
            KeyCode::Backspace => {
                if let Some(m) = self.doc.delete_backward(now) {
                    self.message(m, false);
                }
                self.editor.viewport.goal_x = None;
                self.after_change(true);
                if self.completion.is_some() {
                    self.update_completion();
                }
            }
            KeyCode::Delete => {
                if let Some(m) = self.doc.delete_forward(now) {
                    self.message(m, false);
                }
                self.editor.viewport.goal_x = None;
                self.after_change(true);
            }
            KeyCode::Esc => {
                self.doc.move_cursor(head, false);
                self.doc.clear_extra();
                self.status = None;
                self.after_change(true);
            }
            code @ (KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End) => {
                let vertical = matches!(
                    code,
                    KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
                );
                if self.doc.extra.is_empty() {
                    let Some(t) = self.motion_target(code, shift, word) else {
                        return;
                    };
                    self.doc.move_cursor(t, shift);
                } else {
                    // Every cursor moves.
                    let (all, primary) = self.doc.cursors();
                    let mut moved = Vec::with_capacity(all.len());
                    for s in all {
                        self.doc.selection = s;
                        self.editor.viewport.goal_x = None;
                        if let Some(t) = self.motion_target(code, shift, word) {
                            self.doc.move_cursor(t, shift);
                        }
                        moved.push(self.doc.selection);
                    }
                    self.doc.set_cursors(moved, primary);
                }
                if !vertical {
                    self.editor.viewport.goal_x = None;
                }
                self.after_change(true);
            }
            _ => {}
        }
    }

    /// Where a motion key takes the cursor, `None` for other keys; `word`
    /// moves by words (and Home and End to the document's ends).
    fn motion_target(&mut self, code: KeyCode, shift: bool, word: bool) -> Option<usize> {
        let head = self.doc.selection.head;
        let text = self.doc.text();
        let line = text.line_of(head);
        Some(match code {
            KeyCode::Left | KeyCode::Right if word => self.word(code == KeyCode::Right),
            KeyCode::Left => {
                let sel = self.doc.selection;
                if sel.anchor != sel.head && !shift {
                    sel.anchor.min(sel.head)
                } else {
                    self.horizontal(false)
                }
            }
            KeyCode::Right => {
                let sel = self.doc.selection;
                if sel.anchor != sel.head && !shift {
                    sel.anchor.max(sel.head)
                } else {
                    self.horizontal(true)
                }
            }
            KeyCode::Up | KeyCode::Down => {
                let d = if code == KeyCode::Up { -1 } else { 1 };
                self.editor.vertical(&self.doc, &self.caps, d)
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                let h = self.editor.area.height.saturating_sub(2).max(1) as isize;
                let d = if code == KeyCode::PageUp { -h } else { h };
                self.editor.vertical(&self.doc, &self.caps, d)
            }
            KeyCode::Home if word => 0,
            KeyCode::End if word => text.len(),
            KeyCode::Home => text.line_range(line).start,
            KeyCode::End => text.line_range(line).end,
            _ => return None,
        })
    }

    /// One grapheme left or right in the display, skipping hidden markup.
    fn horizontal(&self, right: bool) -> usize {
        let text = self.doc.text();
        let head = self.doc.selection.head;
        let line = text.line_of(head);
        let range = text.line_range(line);
        let view = match self.doc.parse() {
            _ if self.doc.latex().is_some() && !self.editor.source => Some(
                kalem_core::latex_view::line_view(&self.doc, range.clone(), Some(head)),
            ),
            Some((p, true)) if !self.editor.source => Some(kalem_core::view::line_view(
                &p.syntax(),
                p.context(),
                range.clone(),
                Some(head),
            )),
            _ => None,
        };
        let step = view.as_ref().and_then(|v| {
            if right {
                v.next_position(head)
            } else {
                v.prev_position(head)
            }
        });
        match step {
            Some(p) if p != head => p,
            _ if view.is_some() && right && head < range.end => self.doc.grapheme_after(head),
            _ if view.is_some() && !right && head > range.start => self.doc.grapheme_before(head),
            _ if right => {
                if head >= text.len() {
                    head
                } else {
                    self.doc.grapheme_after(head)
                }
            }
            _ => {
                if head == 0 {
                    0
                } else {
                    self.doc.grapheme_before(head)
                }
            }
        }
    }

    fn prompt_key(&mut self, k: KeyEvent) {
        let Some(mut p) = self.prompt.take() else {
            return;
        };
        self.dirty = true;
        if p.kind == PromptKind::FileTask {
            self.task_key(&k, p);
            return;
        }
        let yes_no = matches!(
            p.kind,
            PromptKind::Quit | PromptKind::Close | PromptKind::Reload | PromptKind::Overwrite
        );
        if !yes_no && let Some(edit) = line_key(&k) {
            kalem_core::line_edit::apply(&mut p.input, &mut p.back, edit);
            self.prompt = Some(p);
            return;
        }
        match k.code {
            KeyCode::Esc => {
                self.message(tr!("msg-cancelled"), false);
                return;
            }
            KeyCode::Char(c) if yes_no => {
                let is = |answer: &str, c: char| {
                    let l = c.to_lowercase().to_string();
                    l == tr!(answer) || l == kalem_core::l10n::tr_in("en", answer, &[])
                };
                let (yes, no) = (is("answer-yes", c), is("answer-no", c));
                match (&p.kind, yes, no) {
                    (PromptKind::Quit, true, _) => {
                        self.save_all(None);
                        if self.modified_count() == 0 {
                            self.close();
                        }
                    }
                    (PromptKind::Quit, _, true) => self.close(),
                    (PromptKind::Close, true, _) => {
                        self.save(false);
                        if !self.doc.is_modified() {
                            self.close_document();
                        }
                    }
                    (PromptKind::Close, _, true) => self.close_document(),
                    (PromptKind::Reload, true, _) => {
                        match self.doc.reload(Instant::now()) {
                            Ok(()) => self.message(tr!("msg-reloaded-from-disk"), false),
                            Err(e) => {
                                self.message(tr!("msg-cannot-reload", error = e.to_string()), true)
                            }
                        }
                        self.after_change(true);
                    }
                    (PromptKind::Overwrite, true, _) => self.save(true),
                    (_, _, true) => self.message(tr!("msg-cancelled"), false),
                    _ => self.prompt = Some(p),
                }
                return;
            }
            KeyCode::Enter => {}
            KeyCode::Char(c) if input::text(&k).is_some() => {
                kalem_core::line_edit::insert(&mut p.input, p.back, c.encode_utf8(&mut [0; 4]));
                self.prompt = Some(p);
                return;
            }
            _ => {
                self.prompt = Some(p);
                return;
            }
        }
        // Enter.
        match p.kind {
            PromptKind::Arg {
                command,
                args,
                name,
                ty,
            } => {
                let v = match kalem_core::command::parse_argument(&name, &ty, &p.input) {
                    Ok(v) => v,
                    Err(e) => {
                        self.message(e, true);
                        return;
                    }
                };
                let args = kalem_core::command::with_argument(args, &name, v);
                self.run_command(&command, args);
            }
            PromptKind::SaveAs => {
                let path = PathBuf::from(p.input.trim());
                if p.input.trim().is_empty() {
                    self.message(tr!("msg-no-file-name"), true);
                    return;
                }
                match self.doc.save_as(&path, self.config.save_options()) {
                    Ok(()) => {
                        if let Some(w) = &mut self.watcher {
                            let _ = w.watch(&path);
                        }
                        self.message(
                            tr!("msg-saved-as", path = path.display().to_string()),
                            false,
                        );
                    }
                    Err(e) => self.message(tr!("msg-not-saved", reason = e.to_string()), true),
                }
            }
            _ => {}
        }
    }

    /// An answer to a file operation's question: `y`/`n`, or `o`, `s`, `k`
    /// (`O`, `S`, `K` for every conflict left); Escape gives up.
    fn task_key(&mut self, k: &KeyEvent, p: Prompt) {
        let Some(q) = self.task.as_ref().and_then(|t| t.question()) else {
            return;
        };
        let answer = match (k.code, &q) {
            (KeyCode::Esc, _) => Some(Answer::No),
            (KeyCode::Char(c), Question::Confirm(_)) => {
                let is = |answer: &str| {
                    let l = c.to_lowercase().to_string();
                    l == tr!(answer) || l == kalem_core::l10n::tr_in("en", answer, &[])
                };
                if is("answer-yes") {
                    Some(Answer::Yes)
                } else if is("answer-no") {
                    Some(Answer::No)
                } else {
                    None
                }
            }
            (KeyCode::Char(c), Question::Conflict { .. }) => match c {
                'o' => Some(Answer::Overwrite),
                's' => Some(Answer::Skip),
                'k' => Some(Answer::KeepBoth),
                'O' => Some(Answer::OverwriteAll),
                'S' => Some(Answer::SkipAll),
                'K' => Some(Answer::KeepBothAll),
                'c' => Some(Answer::No),
                _ => None,
            },
            _ => None,
        };
        let Some(a) = answer else {
            self.prompt = Some(p);
            return;
        };
        let go_on = self.task.as_mut().is_some_and(|t| t.answer(a));
        if go_on {
            self.next_question();
        } else {
            self.task = None;
            self.message(tr!("msg-cancelled"), false);
        }
    }

    /// Background work: parses, file changes, debounced events. Returns
    /// whether something changed on screen.
    pub fn tick(&mut self, now: Instant) {
        // Items of slow completers.
        if let Some(m) = &mut self.completion
            && m.session.waiting()
            && m.session.poll()
        {
            self.dirty = true;
        }
        // Work commands started in the background (a PDF compiling).
        for f in kalem_core::jobs::take_finished() {
            self.message(f.message, f.error);
            if let Some(a) = f.open {
                self.open_link(a);
            }
            self.dirty = true;
        }
        // File operations: progress, then the result.
        if !self.jobs.is_empty() {
            let mut done = Vec::new();
            self.jobs.retain_mut(|j| match j.poll() {
                Some((_, msg, error)) => {
                    done.push((msg, error));
                    false
                }
                None => true,
            });
            if let Some(j) = self.jobs.first() {
                let s = j.status();
                self.message(s, false);
            }
            if !done.is_empty() {
                self.refresh_listings();
                for (m, error) in done {
                    self.message(m, error);
                }
            }
        }
        self.sync_watches();
        if self.doc.poll() {
            self.dirty = true;
        }
        self.tick_palette();
        for b in self.docs.iter_mut().flatten() {
            b.doc.poll();
        }
        // Word counts catch up after a pause in typing.
        if self.words.due(&self.doc) {
            self.dirty = true;
        }
        self.bus.dispatch_queued();
        let changed: Vec<PathBuf> = self.changed_files.borrow_mut().drain(..).collect();
        for b in self.docs.iter_mut().flatten() {
            if watches(&b.doc, &changed) && !b.doc.is_modified() {
                let _ = b.doc.external_change(now);
            }
        }
        if watches(&self.doc, &changed) && self.prompt.is_none() {
            match self.doc.external_change(now) {
                Ok(kalem_core::document::ExternalChange::Reloaded) => {
                    self.after_change(false);
                    self.message(tr!("msg-reloaded"), false);
                }
                Ok(kalem_core::document::ExternalChange::Conflict) => {
                    self.ask(PromptKind::Reload, &tr!("prompt-reload"), String::new());
                }
                Ok(kalem_core::document::ExternalChange::Deleted) => {
                    self.message(tr!("msg-deleted-on-disk"), true);
                }
                Ok(kalem_core::document::ExternalChange::None) => {}
                Ok(kalem_core::document::ExternalChange::Listing) => self.after_change(false),
                Err(e) => self.message(tr!("msg-cannot-read", error = e.to_string()), true),
            }
        }
        for e in self.debouncer.due(now) {
            self.bus.emit(&e);
        }
        for w in self.bus.take_warnings() {
            tracing::warn!("{w}");
        }
        if let Some(s) = &self.status
            && !s.error
            && now.duration_since(s.at) > Duration::from_secs(5)
            && self.pending.is_empty()
        {
            self.status = None;
            self.dirty = true;
        }
    }

    /// How long the event loop may wait for input.
    pub fn timeout(&self, now: Instant) -> Duration {
        let mut t = Duration::from_millis(500);
        if !self.jobs.is_empty() {
            t = t.min(Duration::from_millis(30));
        }
        if matches!(self.doc.parse(), Some((_, false))) {
            t = t.min(Duration::from_millis(20));
        }
        // LaTeX diagnostics behind the text: soon after typing stops.
        if self.doc.latex_diagnostics_due() {
            t = t.min(Duration::from_millis(100));
        }
        if let Some(d) = self.debouncer.next_due() {
            t = t.min(d.saturating_duration_since(now));
        }
        // Search results and files as they come.
        if let Some(p) = &self.palette
            && (p.search.as_ref().is_some_and(ProjectSearch::busy)
                || p.pick.as_ref().is_some_and(|k| k.partial))
        {
            t = t.min(Duration::from_millis(30));
        }
        t
    }

    /// Draws the frame.
    pub fn draw(&mut self, f: &mut Frame<'_>) {
        let area = f.area();
        let info = self.formula.get(&mut self.doc);
        let formula_status = info.and_then(kalem_core::formulas::status);
        self.editor.references = info
            .map(kalem_core::formulas::references)
            .unwrap_or_default();
        // Else the entry cited under the cursor.
        let formula_status = formula_status.or_else(|| self.cite_preview.get(&mut self.doc));
        let mut text_area = Rect {
            height: area.height.saturating_sub(1),
            ..area
        };
        // The open files: a column on the left, or a line at the top, when
        // more than one document is open or a project is.
        let files = self.open_files();
        let show_files = self.files_shown
            && self.files_at != FilesAt::Hidden
            && (files.len() > 1 || self.project().is_some())
            && area.width >= 50;
        self.file_spots.clear();
        self.action_spots.clear();
        self.tree_spots.clear();
        if show_files {
            let entries = projects::entries(&files, &self.projects.list);
            self.tree_rows = match self.project() {
                Some(root)
                    if self.config.bool("ui.folder_tree") && self.files_at == FilesAt::Left =>
                {
                    self.projects.tree_rows(&root)
                }
                _ => Vec::new(),
            };
            let tree = crate::panels::FolderView {
                rows: &self.tree_rows,
                current: self.doc.meta.path.as_deref(),
                spots: Default::default(),
            };
            if self.files_at == FilesAt::Top {
                let line = Rect {
                    height: 1,
                    ..text_area
                };
                (self.file_spots, self.action_spots) = crate::panels::draw_files(
                    f.buffer_mut(),
                    line,
                    &entries,
                    &files,
                    self.active,
                    true,
                    &tree,
                    &self.caps,
                );
                text_area.y += 1;
                text_area.height = text_area.height.saturating_sub(1);
            } else {
                let w = (area.width / 5).clamp(16, 28);
                let panel = Rect {
                    width: w,
                    ..text_area
                };
                (self.file_spots, self.action_spots) = crate::panels::draw_files(
                    f.buffer_mut(),
                    panel,
                    &entries,
                    &files,
                    self.active,
                    false,
                    &tree,
                    &self.caps,
                );
                self.tree_spots = tree.spots.take();
                text_area.x += w;
                text_area.width -= w;
            }
        }
        if self
            .outline
            .as_ref()
            .is_some_and(|o| o.version != self.doc.version())
        {
            let items = self.outline_items();
            if let Some(o) = &mut self.outline {
                o.selected = o.selected.min(items.len().saturating_sub(1));
                o.items = items;
                o.version = self.doc.version();
            }
        }
        if let Some(o) = &mut self.outline {
            let w = OutlinePanel::width(text_area.width);
            let panel = Rect {
                width: w,
                ..text_area
            };
            let head = self.doc.selection.head;
            let current = o.items.iter().rposition(|i| i.start <= head);
            o.draw(f.buffer_mut(), panel, current, &self.caps);
            text_area.x += w;
            text_area.width -= w;
        }
        self.editor.block = self
            .vim
            .as_ref()
            .and_then(|v| v.block_ranges(&self.doc))
            .unwrap_or_else(|| {
                // More cursors: their selections, and a cell for each caret.
                self.doc
                    .extra
                    .iter()
                    .map(|s| {
                        let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
                        if a == b {
                            a..self.doc.grapheme_after(a)
                        } else {
                            a..b
                        }
                    })
                    .collect()
            });
        let cursor = self
            .editor
            .draw(&self.doc, &self.caps, f.buffer_mut(), text_area);
        if let Some(c) = cursor {
            self.draw_popup(f.buffer_mut(), text_area, c);
        }
        if let Some(p) = &self.palette {
            p.draw(f.buffer_mut(), text_area, &self.caps);
        }
        if !self.pending.is_empty() {
            let seq = KeySequence(self.pending.clone());
            let items = self.keymap.which_key(&self.registry, &seq, &self.context());
            crate::panels::draw_which_key(f.buffer_mut(), text_area, &items, &self.caps);
        }
        let y = area.bottom().saturating_sub(1);
        let line = Rect::new(area.x, y, area.width, 1);
        let bar = crate::panels::panel_style(&self.caps);
        let accent = crate::panels::accent_style(&self.caps, bar);
        let buf = f.buffer_mut();
        for x in line.left()..line.right() {
            buf[(x, y)].set_symbol(" ").set_style(bar);
        }
        if let Some(fd) = &self.find {
            let s = self.doc.selection;
            let sel = s.anchor.min(s.head)..s.anchor.max(s.head);
            let text = fd.line(Some(&sel));
            buf.set_stringn(
                area.x + 1,
                y,
                &text,
                area.width.saturating_sub(1) as usize,
                bar,
            );
            let find = kalem_core::l10n::tr(if fd.regex {
                "find-regex-label"
            } else {
                "find-label"
            });
            let label = format!("{find}:");
            buf.set_stringn(
                area.x + 1,
                y,
                &label,
                label.width(),
                accent.add_modifier(Modifier::BOLD),
            );
            let field_end = if fd.on_replacement {
                format!(
                    "{find}: {}  {}: {}",
                    fd.query,
                    kalem_core::l10n::tr("replace-label"),
                    fd.replacement.as_deref().unwrap_or("")
                )
            } else {
                format!("{find}: {}", fd.query)
            };
            let x = (area.x + 1 + field_end.width() as u16).min(area.right().saturating_sub(1));
            f.set_cursor_position((x, y));
            self.dirty = false;
            return;
        }
        if let Some(p) = &self.prompt {
            buf.set_stringn(
                area.x + 1,
                y,
                &p.label,
                area.width as usize,
                accent.add_modifier(Modifier::BOLD),
            );
            let lw = p.label.width() as u16;
            // A long input is cut at the start so the cursor shows.
            let room = area.width.saturating_sub(lw + 2) as usize;
            let (before, after) = kalem_core::line_edit::split(&p.input, p.back);
            let mut before = before.to_string();
            if before.width() >= room && room > 1 {
                while before.width() > room.saturating_sub(2) {
                    before.remove(0);
                }
                before.insert(0, '…');
            }
            let shown = format!("{before}{after}");
            buf.set_stringn(area.x + 1 + lw, y, &shown, room, bar);
            let x = (area.x + 1 + lw + before.width() as u16).min(area.right().saturating_sub(1));
            f.set_cursor_position((x, y));
            self.dirty = false;
            return;
        }
        let x = self.draw_status(buf, line, formula_status);
        let _ = x;
        if let Some(c) = cursor {
            f.set_cursor_position(c);
        }
        self.dirty = false;
    }

    /// The status line: Vim's mode as a colored label, the project and
    /// file, the position and counts, and on the right the last message
    /// (errors in the theme's red) or the key of the command palette.
    fn draw_status(
        &mut self,
        buf: &mut ratatui::buffer::Buffer,
        line: Rect,
        formula_status: Option<String>,
    ) -> u16 {
        use ratatui::style::Color as C;
        let caps = &self.caps;
        let bar = crate::panels::panel_style(caps);
        let dim = bar.add_modifier(Modifier::DIM);
        let y = line.y;
        let mut x = line.x;
        // Vim's mode, or its command line.
        if let Some(v) = &self.vim {
            let label = format!(" {} ", v.status().trim_matches(|c| c == '-' || c == ' '));
            let (bg, fg) = match (&caps.colors, v.command_line.is_some(), v.mode) {
                (Some(t), cmd, mode) => {
                    let c = match (cmd, mode) {
                        (true, _) => t.link,
                        (_, kalem_core::vim::Mode::Normal) => t.link,
                        (_, kalem_core::vim::Mode::Insert) => t.done,
                        (_, kalem_core::vim::Mode::Replace) => t.todo,
                        _ => t.priority,
                    };
                    (crate::render::rgb(c), crate::render::rgb(t.background))
                }
                (None, cmd, mode) => {
                    let c = match (cmd, mode) {
                        (true, _) | (_, kalem_core::vim::Mode::Normal) => 33,
                        (_, kalem_core::vim::Mode::Insert) => 35,
                        (_, kalem_core::vim::Mode::Replace) => 160,
                        _ => 208,
                    };
                    (C::Indexed(c), C::Black)
                }
            };
            let style = if caps.no_color {
                bar.add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                bar.bg(bg).fg(fg).add_modifier(Modifier::BOLD)
            };
            buf.set_stringn(x, y, &label, line.width as usize, style);
            x += label.width() as u16;
        }
        let room = |x: u16| line.right().saturating_sub(x) as usize;
        // The project and the file.
        x += 1;
        if let Some(p) = self.projects.containing(self.doc.meta.path.as_deref()) {
            let s = format!("{} {} ", p.name, if caps.ascii { ">" } else { "▸" });
            buf.set_stringn(x, y, &s, room(x), crate::panels::accent_style(caps, bar));
            x += s.width() as u16;
        }
        let name = self.open_files()[self.active].title.clone();
        buf.set_stringn(x, y, &name, room(x), bar.add_modifier(Modifier::BOLD));
        x += name.width() as u16;
        if self.doc.is_modified() {
            let dot = if caps.ascii { " *" } else { " •" };
            let style = match (&caps.colors, caps.no_color) {
                (Some(t), false) => bar.fg(crate::render::rgb(t.priority)),
                (None, false) => bar.fg(C::Yellow),
                _ => bar,
            };
            buf.set_stringn(x, y, dot, room(x), style);
            x += 2;
        }
        // Mode, position, counts, formula.
        let (l, c) = self.doc.text().line_col(self.doc.selection.head);
        let mode = match &self.doc.meta.mode {
            // The file kind: strict Org, or a Kalem document (§3.7).
            DocumentMode::Org => match kalem_core::kinds::file_kind(&self.doc) {
                Some("klm") => tr!("kind-klm"),
                _ => tr!("kind-org"),
            },
            DocumentMode::Markdown => "Markdown".into(),
            DocumentMode::Csv => "CSV".into(),
            DocumentMode::Latex => "LaTeX".into(),
            DocumentMode::Text { language: Some(l) } => l.clone(),
            // The view: the file manager, or the projects.
            DocumentMode::Directory => match self.doc.dired.as_deref() {
                Some(d) => d.list_title(),
                None => tr!("mode-directory"),
            },
            _ => tr!("mode-text"),
        };
        let view = if self.editor.source { " source" } else { "" };
        let view = match kalem_core::files::encoding_label(&self.doc.meta) {
            Some(n) => format!("{view} {n}"),
            None => view.to_string(),
        };
        let words = match self.doc.dired.as_deref() {
            // The file manager: how many entries, and how many marked.
            Some(d) => {
                let n = d.entries.len().max(d.projects.len());
                let marked = d.marks.len();
                let mut s = format!("  {}", tr!("fm-items", count = n));
                if marked > 0 {
                    s.push_str(&format!("  {}", tr!("fm-marked", count = marked)));
                }
                s
            }
            None => self
                .words
                .get(&self.doc)
                .map(|(d, s)| {
                    format!(
                        "  {}",
                        kalem_core::stats::describe(d, s, self.words.targets())
                    )
                })
                .unwrap_or_default(),
        };
        let formula = formula_status.map(|f| format!("  {f}")).unwrap_or_default();
        let table = kalem_core::formulas::selection_stats(&self.doc)
            .map(|t| format!("  {t}"))
            .unwrap_or_default();
        let rest = format!(
            "   {mode}{view}  {}:{}{words}{formula}{table}",
            l + 1,
            c + 1
        );
        buf.set_stringn(x, y, &rest, room(x), dim);
        x += rest.width() as u16;
        // On the right: the message, or where the commands are.
        // The width of the file manager's hint, when it shows.
        let mut files_w: u16 = 0;
        let (msg, style) = match &self.status {
            Some(s) if s.error => {
                let style = match (&caps.colors, caps.no_color) {
                    (Some(t), false) => bar
                        .fg(crate::render::rgb(t.todo))
                        .add_modifier(Modifier::BOLD),
                    (None, false) => bar.fg(C::Red).add_modifier(Modifier::BOLD),
                    _ => bar.add_modifier(Modifier::BOLD),
                };
                let mark = if caps.ascii { "! " } else { "✖ " };
                (format!("{mark}{} ", s.text), style)
            }
            Some(s) => (
                format!("{} ", s.text),
                crate::panels::accent_style(caps, bar),
            ),
            None => {
                let keys = self
                    .keymap
                    .keys_for("view.palette")
                    .iter()
                    .filter(|k| k.0.len() == 1)
                    .map(|k| k.to_string())
                    .find(|k| k.starts_with("ctrl"))
                    .or_else(|| {
                        self.keymap
                            .keys_for("view.palette")
                            .first()
                            .map(|k| k.to_string())
                    });
                // And the file manager's, which a click opens too.
                let files = self.keymap.keys_for("dired.jump").first().map(|k| {
                    format!(
                        "{}  {}   ",
                        pretty_keys(&k.to_string()),
                        kalem_core::l10n::tr("status-files")
                    )
                });
                files_w = files.as_deref().map_or(0, |f| f.width() as u16);
                match keys {
                    Some(k) => (
                        format!(
                            "{}{}  {} ",
                            files.unwrap_or_default(),
                            pretty_keys(&k),
                            kalem_core::l10n::tr("status-commands")
                        ),
                        dim,
                    ),
                    None => (files.unwrap_or_default(), dim),
                }
            }
        };
        // The file manager's hint only where both fit.
        let mut msg = msg;
        if files_w > 0 && msg.width() as u16 + x + 2 > line.right() {
            msg = msg
                .chars()
                .skip_while({
                    let mut used = 0u16;
                    move |c| {
                        used += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0) as u16;
                        used <= files_w
                    }
                })
                .collect();
            files_w = 0;
        }
        let w = msg.width() as u16;
        let mx = line.right().saturating_sub(w).max(x + 2);
        if mx < line.right() {
            buf.set_stringn(mx, y, &msg, line.right().saturating_sub(mx) as usize, style);
            if self.status.is_none() {
                // The hints: a click opens the file manager, or the
                // palette.
                let room = line.right() - mx;
                let fw = files_w.min(room);
                self.action_spots
                    .push((Rect::new(mx, y, fw, 1), "dired.jump"));
                self.action_spots.push((
                    Rect::new(mx + fw, y, w.min(room).saturating_sub(fw), 1),
                    "view.palette",
                ));
            }
        }
        x
    }

    /// The value of a context key, for tests.
    pub fn context_flag(&self, key: &str) -> bool {
        self.context().get(key) == Some(&WhenValue::Bool(true))
    }
}

/// The edit a key makes in a one-line input: the arrows, Home, End,
/// Backspace and Delete, by words with Ctrl or Alt.
fn line_key(k: &KeyEvent) -> Option<kalem_core::line_edit::LineKey> {
    let name = match k.code {
        KeyCode::Left => "left",
        KeyCode::Right => "right",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::Backspace => "backspace",
        KeyCode::Delete => "delete",
        _ => return None,
    };
    let word = k
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    kalem_core::line_edit::from_key(name, word, false)
}
