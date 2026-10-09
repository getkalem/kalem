//! The application: one document with the shared command registry, keymap,
//! settings and event bus, driven by terminal events.

use std::cell::RefCell;
use std::collections::HashMap;
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
use kalem_core::settings_list::{self, Entry, Field};
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
/// A workspace's panes while another one shows.
type Stash = (
    kalem_core::layout::Layout,
    HashMap<kalem_core::layout::PaneId, (DocumentId, EditorView)>,
);

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
    /// Save As would replace this other file: replace it?
    ReplaceFile(PathBuf),
    /// The folder of a file to save does not exist: make it, then save
    /// (`true`: Save As to the file).
    CreateFolder(PathBuf, bool),
    /// Unsaved changes when quitting: yes, no, cancel.
    Quit,
    /// Quitting without saving the unsaved changes: yes, no.
    QuitDiscard,
    /// Unsaved changes when closing the document: yes, no, cancel.
    Close,
    /// The file changed on disk while there are unsaved changes.
    Reload,
    /// The file changed on disk since it was read: overwrite?
    Overwrite,
    /// The file was converted as it opened: save it in its own format,
    /// keeping what the conversion keeps?
    SaveConverted,
    /// A question of a file operation (`App::task`).
    FileTask,
    /// A setting's text typed in the settings panel, or (`at` given) an
    /// item of its list or table: item `at`, or a new one.
    Setting {
        field: Box<Field>,
        at: Option<Option<usize>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Prompt {
    kind: PromptKind,
    label: String,
    input: String,
    /// The cursor, as characters after it ([`kalem_core::line_edit`]).
    back: usize,
    /// AutoComplete's offer turned down (Delete) until the input changes.
    declined: bool,
    /// The cells a formula's arrows point at.
    pointing: Option<kalem_core::formula_edit::Pointing>,
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
    /// The folder of the user's `settings.toml` and `keymap.json`: Kalem's
    /// configuration folder, or none (an application made by
    /// [`App::with_keymap`] until it is set, as tests do, so they never
    /// read or write the user's own settings).
    pub config_dir: Option<PathBuf>,
    registry: CommandRegistry,
    keymap: Keymap,
    /// Problems in the keymap files.
    pub keymap_issues: Vec<KeymapIssue>,
    bus: EventBus,
    /// The plugins' registrations the commands and keys were built with
    /// (`kalem_core::extensions::generation`).
    plugins_seen: u64,
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
    /// When the half-typed sequence began, for the which-key delay.
    pending_at: Option<Instant>,
    /// Whether the last frame showed the which-key panel.
    hints_drawn: bool,
    /// When an open list of files still being walked was last made again.
    pick_refreshed: Option<Instant>,
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
    /// A language server's documentation at the cursor, until a key.
    hover: Option<String>,
    /// The signature of the call being typed, and the line it was asked
    /// on: shown while the cursor stays there.
    signature: Option<(String, usize)>,
    /// The completers (built-ins, and plugins').
    completers: kalem_core::completers::Registry,
    /// The command palette, when open.
    palette: Option<Palette>,
    /// The settings panel, when open.
    settings: Option<crate::settings_panel::SettingsPanel>,
    /// The request that opened the last list to choose from, and what was
    /// typed in it (`SPC '`).
    last_picker: Option<(Request, String)>,
    /// What to type into the list opening now, resumed.
    resume_input: Option<String>,
    /// A list to choose from was asked for: the next palette is one.
    mark_picker: bool,
    /// The universal argument being typed (`SPC u`).
    prefix: Option<kalem_core::prefix_arg::PrefixArg>,
    /// The window's panes (`SPC w`, T2.7i.5); the focused one shows the
    /// active document.
    layout: kalem_core::layout::Layout,
    /// What the other panes show, each with a view of its own.
    panes: HashMap<kalem_core::layout::PaneId, (DocumentId, EditorView)>,
    /// Where each pane was drawn, for clicks.
    pane_areas: Vec<(kalem_core::layout::PaneId, Rect)>,
    /// The workspaces (`SPC TAB`, T2.7i.15).
    workspaces: kalem_core::workspaces::Workspaces,
    /// The panes of the workspaces not shown, by workspace.
    stashed: HashMap<u64, Stash>,
    /// The panel shown or hidden last (`SPC ~`).
    last_panel: Option<Request>,
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
    /// The plugin's panel shown (`kalem_core::extensions`), by ID.
    plugin_panel: Option<String>,
    /// The plugins' status items and panels drawn
    /// (`kalem_core::extensions::shown`).
    shown_seen: u64,
    /// The plugins' writes of their documents shown
    /// (`kalem_core::extensions::generated_writes`).
    written_seen: u64,
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
    /// projects and the plugins' buttons in the list of open files, the
    /// hint in the status line).
    action_spots: Vec<(Rect, String)>,
    /// The folder tree's lines as last drawn, and where each is.
    tree_rows: Vec<projects::TreeRow>,
    tree_spots: Vec<(Rect, usize)>,
    /// The last document shown that is not a file manager, to go back to.
    last_text: Option<DocumentId>,
    /// The text document (not a listing, not a viewer's file) shown last,
    /// where a viewer's Insert Link at Point inserts.
    last_document: Option<DocumentId>,
    /// The image a viewer's document shows.
    viewer_image: crate::viewer::ViewerImage,
    /// The document shown before the active one (`SPC b l`).
    previous: Option<DocumentId>,
    /// The next keys are described, not run (`SPC h k`).
    describing: bool,
    /// A file operation asking its questions.
    task: Option<kalem_core::dired::Task>,
    /// File operations running in the background.
    jobs: Vec<kalem_core::dired::Running>,
    /// The folders of the file manager documents, watched as a whole.
    watched_dirs: Vec<PathBuf>,
    /// Whether the application should end.
    pub quit: bool,
    /// Whether Kalem starts again after it ends (`SPC q r`, `SPC q R`).
    pub restart: bool,
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
    /// The first document line shown.
    top: usize,
    /// A new first line asked for (CTRL-E, `zt`).
    scroll: Option<usize>,
    /// The folded headings, as Vim's closed folds.
    folds: Vec<(usize, usize)>,
}

impl kalem_core::vim::Host for TuiHost<'_> {
    fn closed_fold(&self, line: usize) -> Option<(usize, usize)> {
        self.folds
            .iter()
            .find(|(a, b)| *a <= line && line <= *b)
            .copied()
    }

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

    fn visible_lines(&self) -> Option<(usize, usize)> {
        let top = self.scroll.unwrap_or(self.top);
        Some((top, top + self.lines.saturating_sub(1)))
    }

    fn scroll(&mut self, by: isize, last: usize) -> Option<(usize, usize)> {
        let top = self.scroll.unwrap_or(self.top) as isize + by;
        self.scroll = Some(top.clamp(0, last as isize) as usize);
        self.visible_lines()
    }

    fn scroll_to(&mut self, line: usize, at: u8) {
        let above = match at {
            0 => 0,
            1 => self.lines / 2,
            _ => self.lines.saturating_sub(1),
        };
        self.scroll = Some(line.saturating_sub(above));
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

/// The folder of `path` when it is not there.
fn missing_folder(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|d| !d.as_os_str().is_empty() && std::fs::symlink_metadata(d).is_err())
        .map(Path::to_path_buf)
}

fn new_document(path: Option<&Path>, config: &Config) -> Result<DocumentState, OpenError> {
    let settings = Arc::new(org_model::Settings::default());
    let base = config.parse_base();
    let mut doc = match path {
        Some(p) if p.exists() => DocumentState::open(p, settings, &base)?,
        _ => {
            let mode = path.map_or(DocumentMode::Org, |p| DocumentMode::detect(Some(p), b""));
            let meta = Metadata {
                path: path.map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())),
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
        app.config_dir = settings::config_dir();
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
        // A file with a password (a PDF): an empty document, and the
        // password asked for once the editor stands.
        let (doc, needs_password) = match new_document(path, &config) {
            Err(OpenError::NeedsPassword) => (new_document(None, &config)?, path),
            r => (r?, None),
        };
        // What plugins read (`kalem.settings`).
        kalem_core::extensions::set_config(&config);
        // The plugins' registrations counted before the keymap is built:
        // a plugin that starts meanwhile (from the compiled components'
        // cache, at once) is seen at the first tick. Counted after, its
        // keys were never bound.
        let plugins_seen = kalem_core::extensions::generation();
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
        // The plugins hear every event (`kalem_core::extensions`).
        bus.subscribe(None, kalem_core::extensions::event);
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
            config_dir: None,
            config,
            registry,
            keymap,
            keymap_issues: issues,
            bus,
            plugins_seen,
            watcher,
            changed_files,
            doc,
            doc_id,
            clipboard: Clipboard::default(),
            editor: EditorView::default(),
            caps,
            pending: Vec::new(),
            pending_at: None,
            hints_drawn: false,
            pick_refreshed: None,
            last_picker: None,
            resume_input: None,
            mark_picker: false,
            prefix: None,
            layout: kalem_core::layout::Layout::new(),
            panes: HashMap::new(),
            pane_areas: Vec::new(),
            workspaces: kalem_core::workspaces::Workspaces::new(),
            stashed: HashMap::new(),
            last_panel: None,
            status: None,
            prompt: None,
            debouncer: ChangeDebouncer::new(Duration::from_millis(300)),
            global_fold: Visibility::Subtree,
            last_click: None,
            last_command: None,
            output: Vec::new(),
            completion: None,
            hover: None,
            signature: None,
            completers: kalem_core::completers::Registry::with_builtins(),
            palette: None,
            settings: None,
            find: None,
            last_query: String::new(),
            last_regex: false,
            words: Default::default(),
            formula: Default::default(),
            cite_preview: Default::default(),
            vim: None,
            cursor_block: None,
            outline: None,
            plugin_panel: None,
            shown_seen: kalem_core::extensions::shown(),
            written_seen: kalem_core::extensions::generated_writes(),
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
            last_document: None,
            viewer_image: Default::default(),
            previous: None,
            describing: false,
            task: None,
            jobs: Vec::new(),
            watched_dirs: Vec::new(),
            quit: false,
            restart: false,
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
        app.editor.highlight_changes = app.config.bool("editor.highlight_changes");
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
        app.workspaces.showing(app.doc_id.0);
        app.bus.emit(&Event::AppReady);
        if let Some(p) = needs_password {
            app.request(Request::Ask {
                command: "file.openWithPassword".into(),
                args: serde_json::json!({ "path": p.display().to_string() }),
                arg: "password".into(),
            });
        }
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
            // Shift+Tab goes on from the startup visibility, as
            // `org-cycle-global-status` does: from an overview to the
            // contents.
            self.global_fold = kalem_core::view::startup_visibility(&o);
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
            c.flag("vimActive", true);
            c.flag("vimOwnsCtrl", true);
        }
        c
    }

    /// The folder of the project holding the active document.
    fn project(&self) -> Option<PathBuf> {
        // A listing is in the project holding its folder.
        let listed = self.doc.dired.as_deref().and_then(|s| s.dir());
        self.projects
            .containing(self.doc.meta.path.as_deref().or(listed))
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
                None if doc.generated.is_some() => {
                    doc.generated_title().unwrap_or_default().to_string()
                }
                None => doc
                    .meta
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map_or_else(|| tr!("untitled"), |n| n.to_string_lossy().into_owned()),
            },
            modified: doc.is_modified(),
            hidden: false,
        };
        self.docs
            .iter()
            .map(|b| {
                let (doc, id) = match b {
                    Some(b) => (&b.doc, b.doc_id),
                    None => (&self.doc, self.doc_id),
                };
                OpenFile {
                    hidden: !self.workspaces.shows(id.0),
                    ..file(doc)
                }
            })
            .collect()
    }

    /// Setting `key`'s value.
    pub fn config_bool(&self, key: &str) -> bool {
        self.config.bool(key)
    }

    /// The active document's index among the open ones.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The command the last key ran, until the next key that types.
    pub fn last_command(&self) -> Option<&str> {
        self.last_command.as_deref()
    }

    /// The status line's message, and whether it is an error.
    pub fn status_message(&self) -> Option<(&str, bool)> {
        self.status.as_ref().map(|s| (s.text.as_str(), s.error))
    }

    /// A view for a new document, set up like the others.
    fn new_view(&self, doc: &DocumentState) -> EditorView {
        let mut v = EditorView::default();
        v.line_width = u16::try_from(self.config.int("editor.line_width")).unwrap_or(0);
        v.center = self.config.bool("editor.center_text");
        v.wrap = self.config.bool("editor.soft_wrap");
        v.line_numbers = self.config.bool("editor.line_numbers");
        v.highlight_changes = self.config.bool("editor.highlight_changes");
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
        if self.doc.dired.is_none() && self.doc.viewer.is_none() {
            self.last_document = Some(self.doc_id);
        }
        self.previous = Some(self.doc_id);
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
        // Changes on disk while it was in the background: reloaded, a
        // conflict to decide, or the file gone.
        let change = self.doc.external_change(Instant::now());
        self.disk_outcome(change);
        self.enter_project();
        self.workspaces.showing(self.doc_id.0);
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
                    // A PDF with a password: asked for, then opened again.
                    Err(OpenError::NeedsPassword) => {
                        self.request(Request::Ask {
                            command: "file.openWithPassword".into(),
                            args: serde_json::json!({ "path": target.display().to_string() }),
                            arg: "password".into(),
                        });
                        return;
                    }
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
        // A PDF or another paged file: the line is the page.
        if let (Some((line, column)), Some(v)) = (at, self.doc.viewer.as_deref_mut()) {
            v.go_to(line.max(1) as usize - 1);
            if column > 0 {
                v.show_height(column as f32);
            }
            self.dirty = true;
            return;
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
                let Some(t) = self.task.take() else { return };
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

    /// A plugin's document that stopped (a trap, its time or memory
    /// spent) answers no more: closed, saying why (wasm_todo W8).
    fn close_stopped_viewer(&mut self) {
        let file = self
            .doc
            .meta
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let modified = self.doc.is_modified();
        let Some(text) = self
            .doc
            .viewer
            .as_deref()
            .and_then(|v| v.stopped_message(&file, modified))
        else {
            return;
        };
        self.close_document();
        self.message(text, true);
    }

    /// Closes the active document (its changes were dealt with); the last
    /// one leaves an empty document (quitting is Quit's).
    fn close_document(&mut self) {
        if self.docs.len() <= 1 {
            if self.doc.meta.path.is_none() && self.doc.text().is_empty() {
                return;
            }
            // An empty document to show, then this one closes.
            let closing = self.active;
            self.new_empty();
            if self.docs.len() <= 1 {
                return;
            }
            self.activate(closing);
        }
        let order = projects::order(&self.open_files(), &self.projects.list);
        let at = order.iter().position(|&x| x == self.active).unwrap_or(0);
        let next = order
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|p| order.get(p)))
            .copied()
            .unwrap_or(0);
        self.bus.emit(&Event::DocumentClose { doc: self.doc_id });
        if let Some(p) = &self.doc.meta.path {
            kalem_core::lsp::closed(p);
        }
        // A plugin's document is forgotten: the plugin's next write is
        // refused.
        if let Some(g) = &self.doc.generated {
            kalem_core::extensions::generated_closed(g.number);
        }
        let closing_id = self.doc_id;
        if let (Some(w), Some(p)) = (&mut self.watcher, &self.doc.meta.path) {
            let _ = w.unwatch(p);
        }
        let closing = self.active;
        self.activate(next);
        self.docs.remove(closing);
        if self.active > closing {
            self.active -= 1;
        }
        self.forget_document(closing_id);
        self.workspaces.leave(closing_id.0);
        self.dirty = true;
    }

    /// The open document that is plugin document `number`.
    fn generated_index(&self, number: u64) -> Option<usize> {
        let is = |d: &DocumentState| d.generated.as_ref().is_some_and(|g| g.number == number);
        self.docs.iter().enumerate().find_map(|(i, b)| match b {
            Some(b) if is(&b.doc) => Some(i),
            None if is(&self.doc) => Some(i),
            _ => None,
        })
    }

    /// Shows plugin document `number` as its plugin last wrote it: the open
    /// one, else a new one.
    fn show_generated(&mut self, number: u64) {
        let Some(g) = kalem_core::extensions::generated(number) else {
            return;
        };
        match self.generated_index(number) {
            Some(i) => {
                self.activate(i);
                self.generated_written();
            }
            None => {
                let doc = DocumentState::generated(
                    g.doc(),
                    g.language.clone(),
                    &g.text,
                    g.cursor,
                    &g.styles,
                    Arc::new(org_model::Settings::default()),
                );
                let doc_id = DocumentId(self.next_doc);
                self.next_doc += 1;
                self.bus.emit(&Event::DocumentOpen {
                    doc: doc_id,
                    path: None,
                });
                let editor = self.new_view(&doc);
                // An untouched empty document gives way.
                let replace = self.doc.meta.path.is_none()
                    && self.doc.dired.is_none()
                    && self.doc.generated.is_none()
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
                self.after_change(true);
            }
        }
        self.dirty = true;
    }

    /// Plugins wrote their documents again: the open ones take the text
    /// newer than theirs.
    fn generated_written(&mut self) {
        let newer = |d: &DocumentState| {
            let shown = d.generated.as_ref()?;
            kalem_core::extensions::generated(shown.number).filter(|g| g.version > shown.version)
        };
        if let Some(g) = newer(&self.doc) {
            self.doc
                .show_generated(g.doc(), &g.text, g.cursor, &g.styles);
            self.after_change(true);
            self.dirty = true;
        }
        for b in self.docs.iter_mut().flatten() {
            if let Some(g) = newer(&b.doc) {
                b.doc.show_generated(g.doc(), &g.text, g.cursor, &g.styles);
            }
        }
    }

    /// Closes plugin document `number` where it is open, the document
    /// shown staying so.
    fn close_generated(&mut self, number: u64) {
        let Some(i) = self.generated_index(number) else {
            kalem_core::extensions::generated_closed(number);
            return;
        };
        let shown = self.doc_id;
        self.activate(i);
        self.close_document();
        if let Some(j) = self
            .docs
            .iter()
            .position(|b| b.as_ref().is_some_and(|b| b.doc_id == shown))
        {
            self.activate(j);
        }
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
                let first = !self.projects.visited(&root);
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
                    // The first time in a session: the project's folder.
                    (After::Open, _) if first => self.file_manager(Place::Dir(root), None),
                    (After::Open, Some(f)) => self.open_path(&f, None),
                    (After::Open, None) => {
                        self.pick(PickKind::ProjectFiles, Some(root), After::Open)
                    }
                    (After::Pick(k), _) => self.pick(k, Some(root), After::Open),
                    (After::Search, _) => self.search_project(Some(root)),
                    (After::Browse, _) => self.file_manager(Place::Dir(root), None),
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
            ProjectRequest::Add(path) => {
                let path = PathBuf::from(settings::expand_home(&path));
                self.projects.add(&path)
            }
            // No folder dialog in a terminal: the folder is typed.
            ProjectRequest::AddChosen => {
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
            ProjectRequest::Remove(root) => self.projects.remove(&root),
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
        let changed = result.is_ok();
        match result {
            Ok(m) if m.is_empty() => {}
            Ok(m) => self.message(m, false),
            Err(m) => self.message(m, true),
        }
        // The projects view lists the projects as they are now.
        if changed
            && self
                .doc
                .dired
                .as_deref()
                .is_some_and(|d| d.place == Place::Projects)
        {
            self.run_command("dired.refresh", Value::Null);
        }
    }

    /// The open documents' ids, by their place in `docs`.
    fn doc_ids(&self) -> Vec<(DocumentId, bool)> {
        self.docs
            .iter()
            .map(|b| match b {
                Some(b) => (b.doc_id, b.doc.is_modified()),
                None => (self.doc_id, self.doc.is_modified()),
            })
            .collect()
    }

    /// Closes the documents without unsaved changes other than `keep`,
    /// then shows `keep` again.
    fn close_others(&mut self, keep: DocumentId) {
        loop {
            let i = self
                .doc_ids()
                .iter()
                .position(|&(id, modified)| id != keep && !modified);
            let Some(i) = i else { break };
            self.activate(i);
            self.close_document();
        }
        if let Some(i) = self.doc_ids().iter().position(|&(id, _)| id == keep) {
            self.activate(i);
        }
    }

    /// Open documents follow their files after an operation: to where
    /// they were moved; closed when trashed or deleted, unless they have
    /// unsaved changes.
    fn follow_files(
        &mut self,
        kind: kalem_core::kalem_fs::OpKind,
        out: &kalem_core::kalem_fs::Outcome,
    ) {
        use kalem_core::dired::{Followed, follow};
        let mut removed = Vec::new();
        let ids = self.doc_ids();
        for (i, b) in self.docs.iter_mut().enumerate() {
            let doc = match b {
                Some(b) => &mut b.doc,
                None => &mut self.doc,
            };
            let Some(path) = doc
                .meta
                .path
                .as_deref()
                .and_then(|p| std::path::absolute(p).ok())
            else {
                continue;
            };
            match follow(kind, out, &path) {
                Some(Followed::Moved(to)) => doc.meta.path = Some(to),
                Some(Followed::Removed) if !doc.has_unsaved_edits() => {
                    doc.accept_removal();
                    removed.push(ids[i].0);
                }
                _ => {}
            }
        }
        for id in removed {
            if let Some(i) = self.doc_ids().iter().position(|&(d, _)| d == id) {
                if self.docs.len() == 1 {
                    self.new_empty();
                }
                let i = self
                    .doc_ids()
                    .iter()
                    .position(|&(d, _)| d == id)
                    .unwrap_or(i);
                self.activate(i);
                self.close_document();
            }
        }
        self.dirty = true;
    }

    /// Doom's `SPC b` commands on the open documents (T2.7i.2).
    fn documents_request(&mut self, r: kalem_core::command::DocumentsRequest) {
        use kalem_core::command::DocumentsRequest as D;
        match r {
            D::SaveAll => {
                let n = self.save_all(None);
                self.message(tr!("msg-saved-count", count = n.to_string()), false);
            }
            D::CloseOthers => self.close_others(self.doc_id),
            D::CloseAll => {
                let keep = self.doc_id;
                self.close_others(keep);
                // The last one gives way to an empty document.
                if !self.doc.is_modified() {
                    self.new_empty();
                    if let Some(i) = self.doc_ids().iter().position(|&(id, _)| id == keep) {
                        self.activate(i);
                        self.close_document();
                    }
                }
            }
            D::Last => {
                let at = self
                    .previous
                    .and_then(|p| self.doc_ids().iter().position(|&(id, _)| id == p));
                match at {
                    Some(i) => self.activate(i),
                    None => self.message(tr!("msg-no-last-document"), true),
                }
            }
            D::Bury => {
                if self.docs.len() < 2 {
                    return;
                }
                let buried = self.doc_id;
                self.cycle(false);
                if let Some(i) = self.doc_ids().iter().position(|&(id, _)| id == buried) {
                    let b = self.docs.remove(i);
                    self.docs.push(b);
                    if self.active > i {
                        self.active -= 1;
                    }
                }
            }
            D::Scratch { project } => {
                let root = if project {
                    match self.project() {
                        Some(r) => Some(r),
                        None => {
                            self.message(tr!("msg-no-project"), true);
                            return;
                        }
                    }
                } else {
                    None
                };
                let Some(path) = kalem_core::command::scratch_path(root.as_deref()) else {
                    self.message(tr!("msg-no-state-dir"), true);
                    return;
                };
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                self.open_path(&path, None);
            }
        }
        self.dirty = true;
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
        // The universal argument's count, for any command but itself.
        let times = if id == kalem_core::prefix_arg::COMMAND {
            1
        } else {
            self.prefix.take().map_or(1, |p| p.count())
        };
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
                declined: false,
                pointing: None,
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
        self.clipboard.registers = self
            .vim
            .as_ref()
            .map(|v| v.register_texts())
            .unwrap_or_default();
        let mut ctx = EditorContext::new(
            Some(&mut self.doc),
            &mut self.clipboard,
            &self.config,
            now,
            clock,
        );
        let result = self.registry.execute_times(id, &mut ctx, &args, times);
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
        if !changes.is_empty() {
            kalem_core::lsp::sync(&self.doc);
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
        let picker = r.is_picker();
        self.request_inner(r);
        if picker {
            self.apply_resume();
        }
    }

    fn request_inner(&mut self, r: Request) {
        if r.is_picker() {
            self.last_picker = Some((r.clone(), String::new()));
            self.mark_picker = true;
        }
        if r.is_panel_toggle() {
            self.last_panel = Some(r.clone());
        }
        match r {
            Request::ResumePicker => match self.last_picker.clone() {
                Some((r, input)) => {
                    self.resume_input = Some(input.clone());
                    self.request(r.clone());
                    self.last_picker = Some((r, input));
                }
                None => self.message(tr!("msg-no-picker"), false),
            },
            Request::QuitWithoutSaving => match self.modified_count() {
                0 => self.close(),
                n => self.ask(
                    PromptKind::QuitDiscard,
                    &tr!("prompt-quit-discard", count = n.to_string()),
                    String::new(),
                ),
            },
            // One window here.
            Request::CloseWindow => self.request(Request::Quit),
            Request::Restart { restore } => {
                if self.modified_count() > 0 {
                    self.message(tr!("msg-restart-unsaved"), true);
                    return;
                }
                if restore && let Err(e) = kalem_core::sessions::restore_on_next_start() {
                    self.message(e, true);
                    return;
                }
                self.restart = true;
                self.close();
            }
            Request::Pane(op) => self.pane_op(&op),
            Request::Workspace(op) => self.workspace_op(op),
            Request::SaveSession(name) => {
                match kalem_core::sessions::save(&name, &self.session()) {
                    Ok(p) => self.message(
                        tr!("msg-session-saved", path = p.display().to_string()),
                        false,
                    ),
                    Err(e) => self.message(e, true),
                }
            }
            Request::RestoreSession(name) => match kalem_core::sessions::load(&name) {
                Ok(s) => self.restore_session(s),
                Err(e) => self.message(e, true),
            },
            Request::UniversalArgument => {
                match &mut self.prefix {
                    Some(p) => p.again(),
                    None => self.prefix = Some(Default::default()),
                }
                let label = self.prefix.map(|p| p.label()).unwrap_or_default();
                self.message(label, false);
            }
            Request::ToggleLastPanel => match self.last_panel.clone() {
                Some(r) => self.request(r),
                None => self.message(tr!("msg-no-panel"), false),
            },
            Request::Save => self.save(false),
            Request::SaveAs if self.doc.generated.is_some() => self.save(false),
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
                let path = match kalem_core::command::folder_of(&self.doc) {
                    Some(d) if !path.is_absolute() => d.join(&path),
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
                let label = kalem_core::command::argument_label(&command, &arg);
                self.ask(
                    PromptKind::Arg {
                        command,
                        args,
                        name: arg.clone(),
                        ty: "string".into(),
                    },
                    &format!("{title}: {label}: "),
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
            Request::ShowGenerated(n) => self.show_generated(n),
            Request::CloseGenerated(n) => self.close_generated(n),
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
            Request::SearchLines {
                all,
                headings,
                text,
            } => self.search_lines(all, headings, &text),
            Request::SearchOtherProject => self.pick(PickKind::Projects, None, After::Search),
            Request::PickProject(after) => self.pick(PickKind::Projects, None, after),
            Request::SearchProjectFor(text) => {
                self.search_project(None);
                if let Some(p) = self.palette.as_mut().filter(|p| p.search.is_some()) {
                    p.input = text;
                    p.back = 0;
                    p.input_changed();
                }
            }
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
            Request::Documents(r) => self.documents_request(r),
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
                self.clipboard.record(t.clone());
                self.write_terminal(&osc52(&t));
            }
            // The terminal's clipboard takes text: the picture's path.
            Request::CopyImage { path, .. } => {
                if let Some(p) = path {
                    let t = p.display().to_string();
                    self.clipboard.record(t.clone());
                    self.write_terminal(&osc52(&t));
                }
            }
            Request::InsertLink(path) => self.insert_link(&path),
            Request::Complete => self.request_completion(),
            // The terminal's clipboard takes plain text only.
            Request::CopyRich { text, .. } => {
                self.clipboard.record(text.clone());
                self.write_terminal(&osc52(&text));
                self.message(tr!("msg-rich-copy-plain"), false);
            }
            Request::Choose(items) => {
                let mut p = Palette::new(items);
                p.ordered = true;
                self.palette = Some(p);
                self.dirty = true;
            }
            Request::OpenAt { path, line, column } => {
                self.open_path(Path::new(&path), Some((line, column)));
            }
            Request::ExportDialog => {
                let items = kalem_core::export_dialog_items(&self.config);
                self.palette = Some(Palette::new(items));
                self.dirty = true;
            }
            Request::SetSetting { key, value, quiet } => self.set_setting(&key, &value, quiet),
            Request::Copy | Request::Cut
                if !self.editor.source && kalem_core::csv::copies_cells(&self.doc) =>
            {
                // A rectangle of cells copies as cells, and no selection
                // the cursor's cell.
                let id = if r == Request::Cut {
                    "csv.cutCells"
                } else {
                    "csv.copyCells"
                };
                self.run_command(id, serde_json::json!({}));
            }
            Request::Copy | Request::Cut => {
                let Some(text) = self.doc.copy_text() else {
                    self.message(tr!("msg-nothing-selected"), false);
                    return;
                };
                self.clipboard.record(text.clone());
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
            Request::FullScreen => self.message(tr!("msg-full-screen-terminal"), false),
            Request::Terminal(dir) => {
                if let Err(e) = kalem_core::system::open_terminal(&dir) {
                    self.message(tr!("msg-no-terminal", error = e), true);
                }
            }
            Request::NewWindow => self.message(tr!("msg-one-window"), false),
            Request::HelpBindings => {
                let items = kalem_core::palette::binding_items(&self.registry, &self.keymap, |k| {
                    k.to_string()
                });
                self.completion = None;
                self.palette = Some(Palette::new(items));
            }
            Request::DescribeKey => self.describing = true,
            Request::ReloadSettings => self.reload_settings(),
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
                self.palette = None;
                self.completion = None;
                let mut panel = crate::settings_panel::SettingsPanel::default();
                // The plugins of this editor's settings folder.
                if let Some(dir) = &self.config_dir {
                    panel
                        .list
                        .set_plugins(kalem_core::plugin_settings::installed_in(dir));
                }
                kalem_core::fonts::prefetch();
                self.settings = Some(panel);
                self.dirty = true;
            }
            Request::Fold { global } => self.fold(global),
            Request::FoldOp(op) => self.fold_op(op),
            Request::Run { command, args } => self.run_command(&command, args),
            Request::VimInsert => {
                if let Some(v) = &mut self.vim {
                    v.insert_at_cursor(&mut self.doc);
                    self.update_cursor_shape();
                }
            }
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
            Request::PluginPanel(id) => {
                self.plugin_panel = match id {
                    Some(id) if self.plugin_panel.as_deref() != Some(id.as_str()) => Some(id),
                    _ => None,
                };
                self.dirty = true;
            }
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

    /// Reads the settings and the user's keymap again (`SPC h r r`).
    fn reload_settings(&mut self) {
        let path = self.config_dir.as_ref().map(|d| d.join("settings.toml"));
        let workspace = self
            .config
            .sources()
            .iter()
            .find(|(l, _)| *l == settings::Layer::Workspace)
            .and_then(|(_, p)| p.clone());
        let old = std::mem::replace(
            &mut self.config,
            Config::load(path.as_deref(), workspace.as_deref()),
        );
        self.config.apply_process_settings();
        kalem_core::extensions::set_config(&self.config);
        self.rebuild_keys();
        self.apply_settings(&old);
        self.message(tr!("msg-reloaded-settings"), false);
    }

    /// Builds the keymap again from the commands, the profile and the
    /// user's `keymap.json`.
    fn rebuild_keys(&mut self) {
        let user = self.config_dir.as_ref().map(|d| d.join("keymap.json"));
        let entries = match user.as_deref().map(std::fs::read_to_string) {
            Some(Ok(text)) => {
                keymap::parse_keymap_with(&text, keymap::Origin::User, &self.config.vim_leader()).0
            }
            _ => Vec::new(),
        };
        let (full, _) = Keymap::build_with(
            &self.registry,
            self.config.keymap_profile(),
            &entries,
            &self.config.vim_leader(),
        );
        self.keymap = full.for_terminal(self.caps.kitty_keyboard).0;
    }

    /// Saves `key` in the user's settings (takes it out for `null`, back
    /// to its default), reads the settings again and applies what
    /// changed. A workspace's setting of the same key wins, and says so.
    fn set_setting(&mut self, key: &str, value: &serde_json::Value, quiet: bool) {
        match settings::spec(key) {
            Some(spec) => self.set_field(&Field::of(spec), value, quiet),
            None => self.message(format!("Unknown setting `{key}`"), true),
        }
    }

    /// [`App::set_setting`] for a setting of the settings panel: one of
    /// Kalem's, or a plugin's under `[plugins."ID"]`.
    fn set_field(&mut self, field: &Field, value: &serde_json::Value, quiet: bool) {
        let Some(path) = self.config_dir.as_ref().map(|d| d.join("settings.toml")) else {
            self.message(tr!("msg-no-settings-dir"), true);
            return;
        };
        if let Err(e) = settings_list::save(&path, field, value) {
            self.message(e, true);
            return;
        }
        let workspace = self
            .config
            .sources()
            .iter()
            .find(|(l, _)| *l == settings::Layer::Workspace)
            .and_then(|(_, p)| p.clone());
        let old = std::mem::replace(
            &mut self.config,
            Config::load(Some(&path), workspace.as_deref()),
        );
        self.config.apply_process_settings();
        self.apply_settings(&old);
        let key = field.key.as_str();
        if !value.is_null() && settings_list::value(&self.config, field) != *value {
            self.message(tr!("msg-setting-overridden", key = key), true);
        } else if !quiet {
            let m = if value.is_null() {
                "msg-setting-reset"
            } else {
                "msg-setting-saved"
            };
            self.message(tr!(m, key = key), false);
        }
    }

    /// Applies the settings that differ from `old`: the views' line
    /// width, centering, wrapping and line numbers, where the open files
    /// show, the keys and the Vim layer, the theme's colors, and the
    /// plugins' copy of the settings.
    fn apply_settings(&mut self, old: &Config) {
        let changed = self.config.changed_keys(old);
        if changed.is_empty() {
            return;
        }
        let has = |key: &str| {
            changed
                .iter()
                .any(|c| c == key || c.strip_prefix(key).is_some_and(|r| r.starts_with('.')))
        };
        kalem_core::extensions::set_config(&self.config);
        let width = u16::try_from(self.config.int("editor.line_width")).unwrap_or(0);
        let center = self.config.bool("editor.center_text");
        let wrap = self.config.bool("editor.soft_wrap");
        let numbers = self.config.bool("editor.line_numbers");
        let tint = self.config.bool("editor.highlight_changes");
        let tinted = has("editor.highlight_changes");
        let (w, c, s, n) = (
            has("editor.line_width"),
            has("editor.center_text"),
            has("editor.soft_wrap"),
            has("editor.line_numbers"),
        );
        let views = std::iter::once(&mut self.editor)
            .chain(self.panes.values_mut().map(|(_, v)| v))
            .chain(self.docs.iter_mut().flatten().map(|b| &mut b.editor));
        for v in views {
            if w {
                v.line_width = width;
            }
            if c {
                v.center = center;
            }
            if s {
                v.wrap = wrap;
            }
            if n {
                v.line_numbers = numbers;
            }
            if tinted {
                v.highlight_changes = tint;
            }
        }
        if has("ui.open_files") {
            self.files_at = match self.config.str("ui.open_files") {
                "top" => FilesAt::Top,
                "hidden" => FilesAt::Hidden,
                _ => FilesAt::Left,
            };
            self.files_shown = self.files_at != FilesAt::Hidden;
        }
        if has("editor.keymap_profile") || has("editor.vim") {
            self.rebuild_keys();
            self.refresh_vim();
        }
        if has("editor.theme")
            && let Some(colors) = crate::theme_colors(&self.config, &self.caps)
        {
            self.caps.colors = Some(colors);
        }
        self.dirty = true;
    }

    /// Keys for the settings panel, lazygit's way: `j` and `k` choose,
    /// `h` and `l` (or Space) change in place, Enter edits or opens a
    /// page (the installed plugins, a plugin), `/` filters, `d` goes back
    /// to the default, `e` opens `settings.toml`, Escape goes back a page
    /// and `q` closes.
    fn settings_key(&mut self, k: &KeyEvent) {
        enum Act {
            Move(isize),
            First,
            Last,
            Step(bool),
            Edit,
            Reset,
            File,
            Back,
            Close,
        }
        let Some(panel) = &mut self.settings else {
            return;
        };
        self.dirty = true;
        if panel.list.item.is_some() {
            self.settings_item_key(k);
            return;
        }
        let config = &self.config;
        let list = &mut panel.list;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if list.filtering {
            match k.code {
                KeyCode::Esc => {
                    list.set_filter("");
                    list.filtering = false;
                }
                KeyCode::Enter => list.filtering = false,
                KeyCode::Backspace => {
                    let mut f = list.filter.clone();
                    if f.pop().is_none() {
                        list.filtering = false;
                    }
                    list.set_filter(&f);
                }
                KeyCode::Up => list.move_by(-1, config),
                KeyCode::Down => list.move_by(1, config),
                KeyCode::Char(c) if !ctrl => {
                    let f = format!("{}{c}", list.filter);
                    list.set_filter(&f);
                }
                _ => {}
            }
            return;
        }
        let act = match k.code {
            KeyCode::Esc if !list.filter.is_empty() => {
                list.set_filter("");
                return;
            }
            KeyCode::Char('/') => {
                list.filtering = true;
                return;
            }
            KeyCode::Esc => Act::Back,
            KeyCode::Char('q') => Act::Close,
            KeyCode::Char('g') if ctrl => Act::Close,
            KeyCode::Char('p') if ctrl => Act::Move(-1),
            KeyCode::Char('n') if ctrl => Act::Move(1),
            KeyCode::Char('u') if ctrl => Act::Move(-10),
            KeyCode::Char('d') if ctrl => Act::Move(10),
            KeyCode::Up | KeyCode::Char('k') => Act::Move(-1),
            KeyCode::Down | KeyCode::Char('j') => Act::Move(1),
            KeyCode::PageUp => Act::Move(-10),
            KeyCode::PageDown => Act::Move(10),
            KeyCode::Home | KeyCode::Char('g') => Act::First,
            KeyCode::End | KeyCode::Char('G') => Act::Last,
            KeyCode::Right | KeyCode::Char('l' | ' ') => Act::Step(true),
            KeyCode::Left | KeyCode::Char('h') => Act::Step(false),
            KeyCode::Enter => Act::Edit,
            KeyCode::Char('d') => Act::Reset,
            KeyCode::Char('e') => Act::File,
            _ => return,
        };
        match act {
            Act::Move(by) => list.move_by(by, config),
            Act::First => list.selected = 0,
            Act::Last => list.last(config),
            Act::Step(forward) => self.settings_step(forward),
            Act::Edit => self.settings_edit(),
            Act::Reset => {
                if let Some(f) = list.current_field(config) {
                    self.set_field(&f, &Value::Null, false);
                }
            }
            Act::File => self.settings_file(),
            Act::Back => {
                if !list.back() {
                    self.settings = None;
                }
            }
            Act::Close => self.settings = None,
        }
    }

    /// Keys for a list's or a table's items in the settings panel: `j`
    /// and `k` choose, Space puts a choice in or out, Enter edits an item
    /// and `a` adds one, `x` removes it, `J` and `K` move a text, `h` and
    /// `l` step an entry's mode, Escape goes back to the settings.
    fn settings_item_key(&mut self, k: &KeyEvent) {
        use kalem_core::settings_list::FieldKind as K;
        let Some(panel) = &mut self.settings else {
            return;
        };
        let Some(field) = panel.list.current_field(&self.config) else {
            panel.list.item = None;
            return;
        };
        let config = &self.config;
        let count = settings_list::items(config, &field).len();
        let i = panel.list.item.unwrap_or(0).min(count.saturating_sub(1));
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let texts = matches!(field.kind, K::Texts | K::Table(_));
        let value = match (k.code, &field.kind) {
            (KeyCode::Esc | KeyCode::Backspace, _) => {
                panel.list.item = None;
                return;
            }
            (KeyCode::Char('q'), _) => {
                self.settings = None;
                return;
            }
            (KeyCode::Char('g'), _) if ctrl => {
                self.settings = None;
                return;
            }
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                panel.list.move_item(-1, count);
                return;
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                panel.list.move_item(1, count);
                return;
            }
            (KeyCode::Home | KeyCode::Char('g'), _) => {
                panel.list.item = Some(0);
                return;
            }
            (KeyCode::End | KeyCode::Char('G'), _) => {
                panel.list.item = Some(count.saturating_sub(1));
                return;
            }
            (
                KeyCode::Char(' ' | 'l' | 'h') | KeyCode::Enter | KeyCode::Left | KeyCode::Right,
                K::Choices(_),
            ) => settings_list::toggle(config, &field, i),
            (KeyCode::Enter, _) if count > 0 => {
                self.settings_ask(field, Some(i));
                return;
            }
            (KeyCode::Enter | KeyCode::Char('a' | 'o'), _) if texts => {
                self.settings_ask(field, None);
                return;
            }
            (KeyCode::Char('x' | 'd') | KeyCode::Delete, _) if texts => {
                settings_list::remove(config, &field, i)
            }
            (KeyCode::Char('K'), K::Texts) => {
                settings_list::shift(config, &field, i, true).map(|(v, to)| {
                    panel.list.item = Some(to);
                    v
                })
            }
            (KeyCode::Char('J'), K::Texts) => {
                settings_list::shift(config, &field, i, false).map(|(v, to)| {
                    panel.list.item = Some(to);
                    v
                })
            }
            (KeyCode::Char(' ' | 'l') | KeyCode::Right, K::Table(_)) => {
                settings_list::cycle(config, &field, i, true)
            }
            (KeyCode::Char('h') | KeyCode::Left, K::Table(_)) => {
                settings_list::cycle(config, &field, i, false)
            }
            _ => return,
        };
        if let Some(v) = value {
            self.set_field(&field, &v, false);
        }
    }

    /// Asks for a value of `field` at the bottom: its text (offered), or,
    /// its items shown, item `at` edited or a new one.
    fn settings_ask(&mut self, field: Field, at: Option<usize>) {
        let items = self
            .settings
            .as_ref()
            .is_some_and(|p| p.list.item.is_some());
        let current = if items {
            at.and_then(|i| {
                settings_list::items(&self.config, &field)
                    .into_iter()
                    .nth(i)
            })
            .map(|it| it.text)
            .unwrap_or_default()
        } else {
            match settings_list::value(&self.config, &field) {
                Value::String(s) => s,
                Value::Null => String::new(),
                v => v.to_string(),
            }
        };
        let label = if items && at.is_none() {
            format!("{} · {}: ", field.key, tr!("settings-item-new"))
        } else {
            format!("{}: ", field.key)
        };
        let kind = PromptKind::Setting {
            field: Box::new(field),
            at: if items { Some(at) } else { None },
        };
        self.ask(kind, &label, current);
    }

    /// The mouse in the settings panel: the wheel moves the choice, a
    /// click chooses an entry (or an item), a click on the chosen one
    /// edits or opens it.
    fn settings_mouse(&mut self, m: crossterm::event::MouseEvent) {
        let Some(panel) = &mut self.settings else {
            return;
        };
        let config = &self.config;
        let items = panel
            .list
            .item
            .and_then(|_| panel.list.current_field(config))
            .map(|f| settings_list::items(config, &f).len());
        match (m.kind, items) {
            (MouseEventKind::ScrollUp, Some(n)) => panel.list.move_item(-3, n),
            (MouseEventKind::ScrollDown, Some(n)) => panel.list.move_item(3, n),
            (MouseEventKind::ScrollUp, None) => panel.list.move_by(-3, config),
            (MouseEventKind::ScrollDown, None) => panel.list.move_by(3, config),
            (MouseEventKind::Down(MouseButton::Left), Some(_)) => match panel.at(m.column, m.row) {
                Some(n) if Some(n) == panel.list.item => {
                    self.settings_item_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                }
                Some(n) => panel.list.item = Some(n),
                None => return,
            },
            (MouseEventKind::Down(MouseButton::Left), None) => match panel.at(m.column, m.row) {
                Some(n) if n == panel.list.selected => self.settings_edit(),
                Some(n) => panel.list.selected = n,
                None => return,
            },
            _ => return,
        }
        self.dirty = true;
    }

    /// The chosen setting a step forward or back: a switch flipped, the
    /// next choice or usual text, a number up or down; a page or a list's
    /// items opened instead, and a text without usual values typed.
    fn settings_step(&mut self, forward: bool) {
        let Some(panel) = &self.settings else {
            return;
        };
        match panel.list.current(&self.config) {
            Some(Entry::Field(f)) => match settings_list::step(&self.config, &f, forward) {
                Some(v) => self.set_field(&f, &v, false),
                None if forward && settings_list::edit(&f) != settings_list::Edit::Step => {
                    self.settings_edit();
                }
                None => {}
            },
            Some(Entry::Plugins(_) | Entry::Plugin(_)) if forward => self.settings_edit(),
            _ => {}
        }
    }

    /// Edits or opens the chosen entry: a setting stepped forward, its
    /// text asked for (the current text offered) or its items shown; the
    /// installed plugins' or a plugin's page; an action run.
    fn settings_edit(&mut self) {
        let Some(panel) = &mut self.settings else {
            return;
        };
        let Some(entry) = panel.list.current(&self.config) else {
            return;
        };
        match entry {
            Entry::Field(f) => match settings_list::edit(&f) {
                settings_list::Edit::Step => self.settings_step(true),
                settings_list::Edit::Type => self.settings_ask(f, None),
                settings_list::Edit::Items => {
                    panel.list.open(&self.config);
                }
            },
            Entry::Plugins(_) | Entry::Plugin(_) => {
                panel.list.open(&self.config);
            }
            Entry::Action { command, args, .. } => {
                self.settings = None;
                self.run_command(&command, args);
            }
        }
    }

    /// Closes the settings panel and opens the user's `settings.toml`.
    fn settings_file(&mut self) {
        let Some(path) = self.config_dir.as_ref().map(|d| d.join("settings.toml")) else {
            self.message(tr!("msg-no-settings-dir"), true);
            return;
        };
        self.settings = None;
        self.open_path(&path, None);
    }

    /// Opens a link target with the system's opener.
    fn open_link(&mut self, action: kalem_core::input::LinkAction) {
        use kalem_core::input::LinkAction;
        let target = match action {
            LinkAction::Url(url) => url,
            LinkAction::File { path, search } => {
                let p = std::path::PathBuf::from(path.trim_start_matches("file:"));
                let p = match (&self.doc.meta.path, p.is_absolute()) {
                    (Some(doc), false) => doc.parent().map_or(p.clone(), |d| d.join(&p)),
                    _ => p,
                };
                // Text files open here, as in the graphical editor; a line
                // number as the search puts the cursor on that line.
                let text = matches!(
                    DocumentMode::detect(Some(&p), b""),
                    DocumentMode::Org
                        | DocumentMode::Markdown
                        | DocumentMode::Text { .. }
                        | DocumentMode::Csv
                        | DocumentMode::Latex
                );
                if text {
                    let at = search
                        .as_deref()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(|l| (l, 0));
                    self.open_path(&p, at);
                    return;
                }
                p.display().to_string()
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

    /// Vim's `z` keys: open or close the fold at the cursor, or all.
    fn fold_op(&mut self, op: kalem_core::view::FoldOp) {
        use kalem_core::view::FoldOp;
        let blocks = self.editor.all_blocks(&self.doc);
        let head = self.doc.selection.head;
        let to = self.editor.folds.apply(&blocks, head, op);
        if to != head {
            self.doc.move_cursor(to, false);
        }
        match op {
            FoldOp::CloseAll => self.global_fold = Visibility::Folded,
            FoldOp::OpenAll => self.global_fold = Visibility::Subtree,
            _ => {}
        }
        self.editor.follow = true;
        self.dirty = true;
    }

    fn fold(&mut self, global: bool) {
        if global {
            let blocks = self.editor.all_blocks(&self.doc);
            let (next, option) = kalem_core::view::next_global(self.global_fold);
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
            declined: false,
            pointing: None,
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
        if let Some(title) = self.doc.generated_title() {
            let m = tr!("msg-plugin-document-not-saved", title = title.to_string());
            self.message(m, false);
            return;
        }
        let Some(path) = self.doc.meta.path.clone() else {
            self.request(Request::SaveAs);
            return;
        };
        // A file opened in a folder that is not there (`kalem tui new/a.txt`,
        // or the folder deleted since).
        if self.doc.dired.is_none()
            && let Some(dir) = missing_folder(&path)
        {
            self.ask_create_folder(path, &dir, false);
            return;
        }
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
                kalem_core::lsp::saved(&self.doc);
                self.message(
                    tr!("msg-saved-as", path = path.display().to_string()),
                    false,
                );
                // A LaTeX document builds on save when asked to; one
                // saved while a build runs is built when it ends.
                if self.doc.latex().is_some() && self.config.bool("latex.build_on_save") {
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
            Err(kalem_core::document::SaveError::Converted { format }) => {
                self.ask(
                    PromptKind::SaveConverted,
                    &tr!("prompt-save-converted", format = format),
                    String::new(),
                );
            }
            Err(e) => self.message(tr!("msg-not-saved", reason = e.to_string()), true),
        }
    }

    /// Shows what a language server answered.
    fn lsp_outcome(&mut self, o: kalem_core::lsp::Outcome) {
        use kalem_core::lsp::Outcome;
        self.dirty = true;
        match o {
            Outcome::Message { text, error } => self.message(text, error),
            Outcome::Hover { path, text } => {
                if self.doc.meta.path.as_deref() == Some(path.as_path()) {
                    self.hover = Some(text);
                }
            }
            Outcome::Signature { path, text } => {
                if self.doc.meta.path.as_deref() == Some(path.as_path()) {
                    let line = self.doc.text().line_of(self.doc.selection.head);
                    self.signature = text.map(|t| (t, line));
                }
            }
            Outcome::Jump(p) => self.open_path(&p.path, Some((p.line, p.column))),
            Outcome::Places { places, .. } => {
                self.request(Request::Choose(kalem_core::lsp::place_items(&places)));
            }
            Outcome::Choose(items) => self.request(Request::Choose(items)),
            Outcome::Edits {
                path,
                version,
                edits,
                label,
            } => {
                if self.doc.meta.path.as_deref() != Some(path.as_path())
                    || self.doc.version() != version
                {
                    self.message(tr!("lsp-format-stale"), true);
                    return;
                }
                if let Some(tx) = kalem_core::lsp::transaction(&edits, &label) {
                    self.doc
                        .apply(&tx, org_edit::ChangeKind::Command, Instant::now());
                    self.after_change(true);
                }
            }
        }
    }

    fn close(&mut self) {
        self.bus.emit(&Event::DocumentClose { doc: self.doc_id });
        kalem_core::lsp::shutdown_all();
        self.quit = true;
    }

    /// The index among the open documents of document `id`.
    fn doc_index(&self, id: DocumentId) -> Option<usize> {
        if id == self.doc_id {
            return Some(self.active);
        }
        self.docs
            .iter()
            .position(|b| b.as_ref().is_some_and(|b| b.doc_id == id))
    }

    /// Focuses pane `p`: its document becomes the active one, and the pane
    /// left keeps showing the document it showed.
    fn focus_pane(&mut self, p: kalem_core::layout::PaneId) {
        let old = self.layout.focus();
        if p == old {
            return;
        }
        let Some((doc, view)) = self.panes.remove(&p) else {
            return;
        };
        let here = self.doc_id;
        let mut left = self.new_view(&self.doc);
        left.viewport = self.editor.viewport.clone();
        self.panes.insert(old, (here, left));
        self.layout.set_focus(p);
        if let Some(i) = self.doc_index(doc) {
            self.activate(i);
        }
        let _ = view;
        self.dirty = true;
    }

    /// A change of the panes (`SPC w`).
    fn pane_op(&mut self, op: &kalem_core::layout::PaneOp) {
        use kalem_core::layout::PaneOp;
        if let PaneOp::CloseOrQuit { force } = *op {
            if self.layout.is_split() {
                return self.pane_op(&PaneOp::Close(false));
            }
            // Then the document, as a tab closes, while others are open;
            // from the last one Kalem quits, as Vim does.
            match (self.docs.len() > 1, force) {
                (true, false) => self.request(Request::Close),
                (true, true) => self.close_document(),
                (false, false) => self.request(Request::Quit),
                (false, true) => self.close(),
            }
            return;
        }
        let old_focus = self.layout.focus();
        let before = self.layout.panes();
        let (changed, new) = self.layout.apply(op);
        if let PaneOp::Close(_) = op
            && !changed
        {
            self.message(tr!("msg-last-pane"), false);
            return;
        }
        if let Some(new) = new {
            // The pane left shows the document; the new one, focused, too
            // (or a new empty one).
            let mut left = self.new_view(&self.doc);
            left.viewport = self.editor.viewport.clone();
            self.panes.insert(old_focus, (self.doc_id, left));
            if *op == PaneOp::New {
                self.new_empty();
            }
            let _ = new;
        }
        if let PaneOp::Close(with_doc) = op {
            // The closed pane was the focused one: the active document is
            // the pane's; the newly focused pane's document becomes active.
            let closing = self.doc_id;
            // Kept, for an undo to show again.
            let mut kept = self.new_view(&self.doc);
            kept.viewport = self.editor.viewport.clone();
            self.panes.insert(old_focus, (closing, kept));
            let focus = self.layout.focus();
            if let Some((doc, _)) = self.panes.remove(&focus)
                && let Some(i) = self.doc_index(doc)
            {
                self.activate(i);
            }
            if *with_doc && let Some(i) = self.doc_index(closing) {
                // Closed as Close does, asking about unsaved changes.
                self.activate(i);
                self.request(Request::Close);
            }
        } else if self.layout.focus() != old_focus && new.is_none() {
            // Focus moved (or undo, rotation): the newly focused pane's
            // document becomes active.
            let focus = self.layout.focus();
            if let Some((doc, _)) = self.panes.remove(&focus) {
                let mut left = self.new_view(&self.doc);
                left.viewport = self.editor.viewport.clone();
                if self.layout.panes().contains(&old_focus)
                    || self.layout.hidden().contains(&old_focus)
                {
                    self.panes.insert(old_focus, (self.doc_id, left));
                }
                if let Some(i) = self.doc_index(doc) {
                    self.activate(i);
                }
            }
        }
        // Swaps and rotations rename the panes: what each showed moves
        // with its place, so the focused pane keeps the active document.
        // Closed panes are kept for an undo, a few.
        let _ = before;
        if self.panes.len() > 32 {
            let live = self.layout.panes();
            self.panes.retain(|p, _| live.contains(p));
        }
        let focus = self.layout.focus();
        self.panes.remove(&focus);
        let _ = changed;
        self.dirty = true;
    }

    /// Shows workspace `i` after `change` (which made it current): its
    /// panes and the document it showed last, else one of its documents,
    /// else a new empty one.
    fn show_workspace(
        &mut self,
        change: impl FnOnce(&mut kalem_core::workspaces::Workspaces) -> bool,
    ) {
        let old = self.workspaces.current().id;
        self.workspaces.showing(self.doc_id.0);
        if !change(&mut self.workspaces) {
            return;
        }
        let stash = (
            std::mem::take(&mut self.layout),
            std::mem::take(&mut self.panes),
        );
        if self.workspaces.list().iter().any(|w| w.id == old) {
            self.stashed.insert(old, stash);
        }
        let ws = self.workspaces.current().clone();
        let (layout, panes) = self.stashed.remove(&ws.id).unwrap_or_default();
        self.layout = layout;
        self.panes = panes;
        let ids: Vec<DocumentId> = self
            .docs
            .iter()
            .map(|b| b.as_ref().map_or(self.doc_id, |b| b.doc_id))
            .collect();
        let target = ws
            .active
            .map(DocumentId)
            .filter(|d| ids.contains(d) && self.workspaces.shows(d.0))
            .or_else(|| ids.iter().copied().find(|d| self.workspaces.shows(d.0)));
        match target.and_then(|d| self.doc_index(d)) {
            Some(i) if i != self.active => self.activate(i),
            Some(_) => {}
            None => self.new_empty(),
        }
        self.message(tr!("msg-workspace", name = ws.name), false);
        self.dirty = true;
    }

    /// A workspace request (`SPC TAB`).
    fn workspace_op(&mut self, op: kalem_core::workspaces::WorkspaceOp) {
        use kalem_core::workspaces::{WorkspaceOp as W, Workspaces};
        match op {
            W::List => self.request(Request::Choose(self.workspaces.items())),
            W::New(name) => self.show_workspace(|w| {
                w.add(name.as_deref());
                true
            }),
            W::Delete => {
                if !self.workspaces.several() {
                    self.message(tr!("msg-last-workspace"), false);
                    return;
                }
                let gone = self.workspaces.current().id;
                self.show_workspace(|w| w.delete().is_some());
                self.stashed.remove(&gone);
            }
            W::Rename(name) => {
                self.workspaces.rename(&name);
                self.dirty = true;
            }
            W::Cycle(back) => self.show_workspace(|w| w.cycle(back)),
            W::Switch(i) => self.show_workspace(|w| w.switch(i)),
            W::Final => self.show_workspace(|w| w.switch_final()),
            W::Last => self.show_workspace(|w| w.switch_last()),
            W::Save => {
                let mut s = self.session();
                let shown: Vec<PathBuf> = self
                    .docs
                    .iter()
                    .filter_map(|b| {
                        let (doc, id) = match b {
                            Some(b) => (&b.doc, b.doc_id),
                            None => (&self.doc, self.doc_id),
                        };
                        doc.meta
                            .path
                            .clone()
                            .filter(|_| self.workspaces.shows(id.0))
                    })
                    .collect();
                let active = s.documents.get(s.active).map(|d| d.path.clone());
                s.documents.retain(|d| shown.contains(&d.path));
                s.active = active
                    .and_then(|a| s.documents.iter().position(|d| d.path == a))
                    .unwrap_or(0);
                let name = Workspaces::session_name(&self.workspaces.current().name);
                match kalem_core::sessions::save(&name, &s) {
                    Ok(p) => self.message(
                        tr!("msg-session-saved", path = p.display().to_string()),
                        false,
                    ),
                    Err(e) => self.message(e, true),
                }
            }
            W::Load(None) | W::DeleteSaved(None) => {
                let delete = matches!(op, W::DeleteSaved(_));
                let names = Workspaces::saved();
                if names.is_empty() {
                    self.message(tr!("msg-no-saved-workspaces"), false);
                    return;
                }
                let command = if delete {
                    "workspace.deleteSaved"
                } else {
                    "workspace.load"
                };
                let items = names
                    .into_iter()
                    .map(|n| kalem_core::palette::PaletteItem {
                        id: kalem_core::palette::invocation(
                            command,
                            &serde_json::json!({ "name": n }),
                        ),
                        title: n,
                        category: tr!("category-workspaces"),
                        keys: String::new(),
                        also: String::new(),
                    })
                    .collect();
                self.request(Request::Choose(items));
            }
            W::Load(Some(name)) => {
                match kalem_core::sessions::load(&Workspaces::session_name(&name)) {
                    Ok(s) => {
                        self.show_workspace(|w| {
                            w.add(Some(&name));
                            true
                        });
                        self.restore_session(s);
                    }
                    Err(e) => self.message(e, true),
                }
            }
            W::DeleteSaved(Some(name)) => {
                match kalem_core::sessions::delete(&Workspaces::session_name(&name)) {
                    Ok(()) => self.message(tr!("msg-workspace-deleted", name = name), false),
                    Err(e) => self.message(e, true),
                }
            }
        }
    }

    /// The panes showing document `id` other than the focused one switch
    /// to the active document; called when `id` closes.
    fn forget_document(&mut self, id: DocumentId) {
        let shown = self.doc_id;
        let gone: Vec<_> = self
            .panes
            .iter()
            .filter(|(_, (d, _))| *d == id)
            .map(|(p, _)| *p)
            .collect();
        for p in gone {
            if self.layout.panes().len() > 1 && self.layout.close(p) {
                self.panes.remove(&p);
            } else if let Some(e) = self.panes.get_mut(&p) {
                e.0 = shown;
            }
        }
    }

    /// The open documents with a file, as a session.
    pub fn session(&self) -> kalem_core::sessions::Session {
        let mut s = kalem_core::sessions::Session {
            project: self.project(),
            ..Default::default()
        };
        for (i, b) in self.docs.iter().enumerate() {
            let doc = b.as_ref().map_or(&self.doc, |b| &b.doc);
            let Some(path) = doc.meta.path.clone().filter(|_| doc.dired.is_none()) else {
                continue;
            };
            if i == self.active {
                s.active = s.documents.len();
            }
            let t = doc.text();
            let line = t.line_of(doc.selection.head);
            s.documents.push(kalem_core::sessions::SessionDoc {
                path,
                line: line as u64 + 1,
                column: doc.selection.head - t.line_range(line).start,
            });
        }
        s
    }

    /// Opens the documents of `session`, showing its active one.
    fn restore_session(&mut self, session: kalem_core::sessions::Session) {
        let s = session.existing();
        for d in &s.documents {
            self.open_path(&d.path, Some((d.line, d.column)));
        }
        if let Some(d) = s.documents.get(s.active) {
            self.open_path(&d.path, None);
        }
        self.message(
            tr!("msg-session-restored", count = s.documents.len()),
            false,
        );
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
        // A spreadsheet takes the text as rows of cells.
        if self.doc.viewer.as_deref().is_some_and(|v| v.is_grid()) {
            self.run_command("viewer.grid.pasteText", serde_json::json!({ "text": text }));
            return;
        }
        // A CSV grid takes it as cells.
        if self.editor.source || plain || !self.doc.paste_in_grid(text, Instant::now()) {
            self.doc.paste(text, None, plain, Instant::now());
        }
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
        if self.settings.is_some() {
            self.settings_mouse(m);
            return;
        }
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        // A click in another pane focuses it first.
        if let MouseEventKind::Down(_) = m.kind
            && let Some(&(p, _)) = self
                .pane_areas
                .iter()
                .find(|(_, r)| r.contains(ratatui::layout::Position::new(m.column, m.row)))
            && p != self.layout.focus()
        {
            self.focus_pane(p);
        }
        if let MouseEventKind::Down(MouseButton::Left) = m.kind
            && let Some((_, id)) = self
                .action_spots
                .iter()
                .find(|(r, _)| r.contains(ratatui::layout::Position::new(m.column, m.row)))
                .cloned()
        {
            self.run_command(&id, Value::Null);
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
        if self.grid_mouse(&m) {
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
                // A CSV grid's letter selects its column, a row's number the
                // row, as in a spreadsheet.
                if let Some(h) = self.editor.csv_header_hit(&self.doc, m.column, m.row) {
                    let (id, args) = match h {
                        crate::editor::CsvHeader::Column(c) => {
                            ("csv.selectColumn", serde_json::json!({ "column": c }))
                        }
                        crate::editor::CsvHeader::Row(r) => {
                            ("csv.selectRow", serde_json::json!({ "row": r }))
                        }
                    };
                    self.run_command(id, args);
                    self.after_change(true);
                    return;
                }
                let Some((mut pos, widget)) =
                    self.editor.hit(&self.doc, &self.caps, m.column, m.row)
                else {
                    return;
                };
                // A CSV grid's cell: by the bars around the click.
                let mut cell = None;
                if let Some((to, past)) = self.editor.csv_hit(&self.doc, m.column, m.row, pos) {
                    pos = to;
                    cell = past;
                }
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(t, x, y)| {
                    now.duration_since(t) < Duration::from_millis(400)
                        && x == m.column
                        && y == m.row
                });
                self.last_click = Some((now, m.column, m.row));
                if let Some((Widget::Checkbox(_), start, _)) = widget {
                    self.doc.move_cursor(start, false);
                    let id = kalem_core::mode_view::checkbox_command(&self.doc);
                    self.run_command(id, Value::Null);
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
                if double
                    && self.doc.meta.mode == kalem_core::DocumentMode::Csv
                    && !self.editor.source
                {
                    // A CSV grid's cell: edited where it was clicked.
                    self.doc.move_cursor(pos, false);
                    self.run_command("csv.editCell", serde_json::json!({ "here": true }));
                } else if double {
                    self.select_word(pos);
                } else {
                    self.doc.move_cursor(pos, shift);
                    if let Some(c) = cell.filter(|_| !shift) {
                        self.doc.select_csv_virtual(c);
                    }
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
                #[expect(clippy::expect_used, reason = "the arm's guard checked it")]
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

    /// The mouse on a workbook's grid: a click on a cell, a letter, a
    /// number or a tab; Shift to extend, Ctrl to add a range; a drag to
    /// select; a right click for the menu; the wheel to scroll. Whether it
    /// was the grid's.
    fn grid_mouse(&mut self, m: &crossterm::event::MouseEvent) -> bool {
        use kalem_core::viewer::GridSpot;
        let Some(v) = self.doc.viewer.as_deref_mut().filter(|v| v.is_grid()) else {
            return false;
        };
        let Some(spot) = v.hits.as_ref().and_then(|h| h.at(m.column, m.row)) else {
            return false;
        };
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = m.modifiers.contains(KeyModifiers::CONTROL);
        let top = v.grid_pos().top;
        let left = v.grid_pos().left;
        match (m.kind, spot) {
            (MouseEventKind::Down(MouseButton::Left), GridSpot::Tab(u)) => {
                v.go_to(u);
            }
            (MouseEventKind::Down(MouseButton::Right), GridSpot::Tab(u)) => {
                self.run_command(
                    "viewer.grid.contextMenu",
                    serde_json::json!({ "on": "tab", "unit": u }),
                );
            }
            (MouseEventKind::Down(MouseButton::Left), GridSpot::Cell(r, c)) => {
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(t, x, y)| {
                    now.duration_since(t) < Duration::from_millis(400)
                        && x == m.column
                        && y == m.row
                });
                self.last_click = Some((now, m.column, m.row));
                if ctrl {
                    v.add_area(r, c);
                } else if shift {
                    v.grid_extend_to(r, c);
                } else {
                    v.grid_move_to(r, c);
                    if double {
                        self.run_command("viewer.grid.edit", Value::Null);
                    }
                }
            }
            (MouseEventKind::Drag(MouseButton::Left), GridSpot::Cell(r, c)) => {
                if ctrl {
                    v.extend_area(r, c);
                } else {
                    v.grid_extend_to(r, c);
                }
            }
            (MouseEventKind::Down(MouseButton::Left), GridSpot::Column(c)) => {
                v.grid_move_to(top, c);
                self.run_command("viewer.grid.selectColumn", Value::Null);
            }
            (MouseEventKind::Down(MouseButton::Left), GridSpot::Row(r)) => {
                v.grid_move_to(r, left);
                self.run_command("viewer.grid.selectRow", Value::Null);
            }
            (MouseEventKind::Down(MouseButton::Right), spot) => {
                let on = match spot {
                    GridSpot::Column(c) => {
                        let s = v.selection();
                        if !(s[1]..=s[3]).contains(&c) {
                            v.grid_move_to(top, c);
                            self.run_command("viewer.grid.selectColumn", Value::Null);
                        }
                        "cols"
                    }
                    GridSpot::Row(r) => {
                        let s = v.selection();
                        if !(s[0]..=s[2]).contains(&r) {
                            v.grid_move_to(r, left);
                            self.run_command("viewer.grid.selectRow", Value::Null);
                        }
                        "rows"
                    }
                    GridSpot::Cell(r, c) => {
                        // Outside the selection, the cell is selected first.
                        let s = v.selection();
                        if !((s[0]..=s[2]).contains(&r) && (s[1]..=s[3]).contains(&c)) {
                            v.grid_move_to(r, c);
                        }
                        "cells"
                    }
                    GridSpot::Tab(_) => "tab",
                };
                self.run_command("viewer.grid.contextMenu", serde_json::json!({ "on": on }));
            }
            (MouseEventKind::ScrollDown, _) => v.grid_scroll(3, 0),
            (MouseEventKind::ScrollUp, _) => v.grid_scroll(-3, 0),
            (MouseEventKind::ScrollRight, _) => v.grid_scroll(0, 2),
            (MouseEventKind::ScrollLeft, _) => v.grid_scroll(0, -2),
            _ => return false,
        }
        self.dirty = true;
        true
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

    /// What was typed in a list resumed (`SPC '`), once it is open.
    fn apply_resume(&mut self) {
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
    fn remember_picker(&mut self) {
        if let (Some(p), Some(last)) = (&self.palette, &mut self.last_picker)
            && p.resumable
        {
            last.1 = p.input.clone();
        }
    }

    fn palette_key(&mut self, k: &KeyEvent) {
        self.remember_picker();
        self.palette_key_and_remember(k);
        self.remember_picker();
    }

    fn palette_key_and_remember(&mut self, k: &KeyEvent) {
        if self.palette.as_ref().is_some_and(|p| p.lines.is_some()) {
            match k.code {
                KeyCode::Esc => self.end_line_search(false),
                KeyCode::Enter => self.end_line_search(true),
                _ => {
                    self.palette_key_inner(k);
                    self.preview_line();
                }
            }
            return;
        }
        self.palette_key_inner(k);
    }

    /// Opens the live search of lines: this document's, or every open
    /// one's (T2.7i.4).
    fn search_lines(&mut self, all: bool, headings: bool, text: &str) {
        use kalem_core::line_search::{LineSearch, Lines, Source};
        let mut sources = Vec::new();
        let mut here = 0;
        let files = self.open_files();
        for (i, b) in self.docs.iter().enumerate() {
            let doc = match b {
                Some(b) => &b.doc,
                None => &self.doc,
            };
            if doc.dired.is_some() || (!all && i != self.active) {
                continue;
            }
            if i == self.active {
                here = sources.len();
            }
            sources.push(Source {
                doc: i,
                name: files.get(i).map(|f| f.title.clone()).unwrap_or_default(),
                text: doc.text().as_str().into(),
            });
        }
        if sources.is_empty() {
            return;
        }
        let lines = if headings {
            Lines::Headings
        } else {
            Lines::All
        };
        let sel = self.doc.selection;
        let search = LineSearch::new(sources, lines, text);
        self.completion = None;
        self.palette = Some(Palette::searching_lines(
            search,
            (self.active, here, sel.anchor, sel.head),
        ));
        self.preview_line();
        self.dirty = true;
    }

    /// The cursor follows the chosen line while it is in this document.
    fn preview_line(&mut self) {
        let Some(p) = &self.palette else { return };
        let Some(l) = &p.lines else { return };
        let Some(h) = l.hits.get(p.selected) else {
            return;
        };
        if l.sources[h.source].doc == self.active {
            let at = h.at;
            self.doc.move_cursor(at, false);
            self.editor.follow = true;
            self.dirty = true;
        }
    }

    /// Ends the line search: at the chosen line, or back where it began.
    fn end_line_search(&mut self, jump: bool) {
        let Some(p) = self.palette.take() else { return };
        let (Some(l), Some((doc, _, anchor, head))) = (p.lines, p.origin) else {
            return;
        };
        match l.hits.get(p.selected).filter(|_| jump) {
            Some(h) => {
                let target = l.sources[h.source].doc;
                if target != self.active {
                    self.activate(target);
                }
                self.doc.move_cursor(h.at, false);
            }
            None => {
                if doc == self.active {
                    self.doc.move_cursor(anchor, false);
                    self.doc.move_cursor(head, true);
                }
            }
        }
        self.editor.follow = true;
        self.after_change(false);
        self.dirty = true;
    }

    fn palette_key_inner(&mut self, k: &KeyEvent) {
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
                p.input_changed();
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
                    p.input_changed();
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
        // The files found so far, a few times a second: the list is made
        // again whole, four texts a file.
        if let Some(pick) = p.pick.as_mut().filter(|k| k.partial)
            && self
                .pick_refreshed
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(250))
            && let Some(mut fresh) = projects::picker(
                pick.kind,
                &[],
                None,
                pick.project.as_deref(),
                &mut self.projects,
            )
        {
            self.pick_refreshed = Some(Instant::now());
            fresh.after = pick.after;
            let changed = fresh.items.len() != pick.items.len() || !fresh.partial;
            *pick = fresh;
            if changed {
                p.items_changed();
                self.dirty = true;
            }
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
        // A file a viewer shows: its units' text, searched on a thread.
        if let Some(v) = self.doc.viewer.as_deref_mut() {
            let Some(f) = &self.find else { return };
            if from_origin {
                v.search_start(&f.query);
            } else {
                v.search_next(backward);
            }
            self.dirty = true;
            return;
        }
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
                if let Some(v) = self.doc.viewer.as_deref_mut() {
                    v.search_end();
                }
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
                    tx.edit(sel.clone(), r.clone());
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
        kalem_core::mode_view::outline_items(&mut self.doc).unwrap_or_default()
    }

    fn toggle_outline(&mut self) {
        match &mut self.outline {
            Some(o) if o.focus => self.outline = None,
            Some(o) => o.focus = true,
            None => {
                let items = self.outline_items();
                let head = kalem_core::viewer::outline_position(&self.doc);
                let selected = items
                    .iter()
                    .rposition(|i| i.file.is_none() && i.start <= head)
                    .unwrap_or(0);
                self.outline = Some(OutlinePanel::new(items, selected, self.doc.version()));
            }
        }
        self.dirty = true;
    }

    /// Jumps to the outline's chosen heading.
    fn outline_jump(&mut self) {
        let Some(o) = &mut self.outline else { return };
        let Some((start, file)) = o.items.get(o.selected).map(|i| (i.start, i.file.clone())) else {
            return;
        };
        o.focus = false;
        // A viewer's outline goes to a unit.
        if let Some(v) = self.doc.viewer.as_deref_mut() {
            v.go_to(start);
            self.dirty = true;
            return;
        }
        // A heading of an included file: that file, at the heading.
        if let Some(file) = file {
            let at = kalem_core::view::position_in_file(&file, start);
            self.open_path(&file, at);
            return;
        }
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
            // In Vim, one Escape closes the menu and leaves insert mode.
            KeyCode::Esc if self.vim.is_some() => {
                self.completion = None;
                return false;
            }
            KeyCode::Esc => self.completion = None,
            // Words are taken with Tab: Enter goes on writing prose.
            KeyCode::Enter
                if m.current()
                    .is_some_and(|i| i.kind == kalem_core::completers::Kind::Word) =>
            {
                self.completion = None;
                return false;
            }
            // Nothing to choose yet (a server still answering): the key
            // does what it does without a menu.
            KeyCode::Enter | KeyCode::Tab if m.current().is_none() => {
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
        let line_now = self.doc.text().line_of(self.doc.selection.head);
        let signature = self
            .signature
            .as_ref()
            .filter(|(_, l)| *l == line_now)
            .map(|(t, _)| t);
        let lines: Vec<(String, bool)> = if let Some(h) = self.hover.as_ref().or(signature)
            && self.completion.is_none()
        {
            let width = (area.width.saturating_sub(4) as usize).clamp(20, 80);
            kalem_core::lsp::hover_lines(h, width, 16)
                .into_iter()
                .map(|l| (l, false))
                .collect()
        } else if let Some(m) = &self.completion {
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
            vec![(
                format!(
                    "= {}",
                    kalem_core::latex_view::formula_unicode(&self.doc, &f)
                ),
                false,
            )]
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
        // The chosen completion's documentation: right of the list where
        // it fits, else on the other side of the cursor's line.
        let Some(doc) = self.completion.as_ref().and_then(|m| m.documentation()) else {
            return;
        };
        let top = if below {
            cursor.1 + 1
        } else {
            cursor.1.saturating_sub(n)
        };
        let right = x + width + 1;
        let room_right = area.right().saturating_sub(right);
        let (dx, dy, dw, max_lines) = if room_right >= 30 {
            (
                right,
                top,
                room_right.min(72),
                area.bottom().saturating_sub(top).min(16),
            )
        } else if below {
            // Above the cursor's line.
            let h = cursor.1.saturating_sub(area.top()).min(12);
            (
                area.left(),
                cursor.1.saturating_sub(h),
                area.width.min(72),
                h,
            )
        } else {
            let start = cursor.1 + 1;
            (
                area.left(),
                start,
                area.width.min(72),
                area.bottom().saturating_sub(start).min(12),
            )
        };
        if dw < 10 || max_lines == 0 {
            return;
        }
        let lines =
            kalem_core::lsp::hover_lines(doc, dw.saturating_sub(2) as usize, max_lines as usize);
        for (k, line) in lines.iter().enumerate() {
            let y = dy + k as u16;
            if y >= area.bottom() {
                break;
            }
            for c in 0..dw.min(area.right() - dx) {
                buf[(dx + c, y)].set_symbol(" ").set_style(bg);
            }
            buf.set_stringn(dx + 1, y, line, dw.saturating_sub(2) as usize, bg);
        }
    }

    /// Handles a key.
    pub fn key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        if self.hover.take().is_some() {
            self.dirty = true;
            if k.code == KeyCode::Esc {
                return;
            }
        }
        if k.code == KeyCode::Esc && self.signature.take().is_some() {
            self.dirty = true;
        }
        if self.prompt.is_some() {
            self.prompt_key(k);
            return;
        }
        if self.settings.is_some() {
            self.settings_key(&k);
            return;
        }
        if self.palette.is_some() {
            self.palette_key(&k);
            return;
        }
        // After `SPC u`: digits give the count, Escape drops it.
        if self.pending.is_empty()
            && let Some(p) = &mut self.prefix
        {
            let plain = k.modifiers.difference(KeyModifiers::SHIFT).is_empty();
            if let KeyCode::Char(c) = k.code
                && plain
                && p.key(&c.to_string())
            {
                let label = p.label();
                self.message(label, false);
                return;
            }
            if k.code == KeyCode::Esc {
                self.prefix = None;
                self.status = None;
                self.dirty = true;
                return;
            }
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
        if !self.describing && self.pending.is_empty() && !self.listing_key(&k) && self.vim_key(&k)
        {
            return;
        }
        if let Some(chord) = input::chord(&k, self.caps.kitty_keyboard) {
            if self.pending.is_empty() {
                self.pending_at = Some(Instant::now());
            }
            self.pending.push(chord);
            let seq = KeySequence(self.pending.clone());
            let ctx = self.context();
            match self.keymap.lookup(&seq, &ctx) {
                Lookup::Command { command, .. } if self.describing => {
                    let title = self
                        .registry
                        .get(command)
                        .map_or_else(|| command.to_string(), |c| c.display_title());
                    let msg = tr!(
                        "help-key",
                        keys = seq.to_string(),
                        title = title,
                        id = command
                    );
                    self.pending.clear();
                    self.describing = false;
                    self.message(msg, false);
                    return;
                }
                Lookup::None if self.describing => {
                    self.pending.clear();
                    self.describing = false;
                    self.message(tr!("help-key-none", keys = seq.to_string()), false);
                    return;
                }
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
        // A character typed on a workbook's cell starts its entry with it,
        // as in Excel.
        if let crossterm::event::KeyCode::Char(c) = k.code
            && !k.modifiers.intersects(
                crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT,
            )
            && !c.is_control()
            && self.doc.viewer.as_deref().is_some_and(|v| v.is_grid())
        {
            self.run_command(
                "viewer.grid.typeInto",
                serde_json::json!({ "text": c.to_string() }),
            );
            return;
        }
        self.edit_key(k);
    }

    /// Whether the keymap takes `k` before Vim: in a file manager listing
    /// outside Vim's insert mode and command line, the keys it binds.
    fn listing_key(&self, k: &KeyEvent) -> bool {
        // A viewer's document has no text for Vim to edit: only its
        // command line, `:` opening it (`:q` closes the file as elsewhere).
        if self.doc.viewer.is_some() {
            let line = self.vim.as_ref().is_some_and(|v| v.command_line.is_some());
            let colon = self.vim.is_some() && k.code == crossterm::event::KeyCode::Char(':');
            return !(line || colon);
        }
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
        // Insert mode's own keys go to the layer; typed characters and
        // keys it does not know go to the editor.
        if v.takes_text() && matches!(key, Key::Char(_) | Key::Other) {
            self.vim = Some(v);
            return false;
        }
        let top = self
            .doc
            .text()
            .line_of(self.editor.viewport.top.min(self.doc.text().len()));
        let folds = if self.editor.folds == kalem_core::view::Folds::default() {
            Vec::new()
        } else {
            let blocks = self.editor.all_blocks(&self.doc);
            let text = self.doc.text();
            kalem_core::view::closed_folds(&self.editor.folds, &blocks, text.len(), |p| {
                text.line_of(p)
            })
        };
        let (out, scroll) = {
            let mut host = TuiHost {
                clip: &mut self.clipboard.text,
                output: &mut self.output,
                lines: usize::from(self.editor.area.height.max(4)),
                rich: !self.editor.source,
                top,
                scroll: None,
                folds,
            };
            let out = v.key(&mut self.doc, key, &mut host);
            (out, host.scroll)
        };
        // CTRL-E, CTRL-Y, `zt`, `zz`, `zb`: the view moves.
        if let Some(l) = scroll {
            self.editor.viewport.top = self.doc.text().line_start(l);
            self.editor.viewport.top_row = 0;
        }
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
        self.doc.csv_vim = self.vim.is_some();
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
                if (self.editor.source || !self.doc.delete_in_grid(false, now))
                    && let Some(m) = self.doc.delete_backward(now)
                {
                    self.message(m, false);
                }
                self.editor.viewport.goal_x = None;
                self.after_change(true);
                if self.completion.is_some() {
                    self.update_completion();
                }
            }
            KeyCode::Delete => {
                if (self.editor.source || !self.doc.delete_in_grid(true, now))
                    && let Some(m) = self.doc.delete_forward(now)
                {
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
                // Vim's visual mode: the key moves the selection's moving
                // end, from where it is, and the selection grows.
                if let Some(c) = self
                    .vim
                    .as_ref()
                    .and_then(kalem_core::vim::Vim::visual_cursor)
                {
                    let before = self.doc.selection;
                    self.doc.selection = org_edit::Selection::caret(c);
                    let target = self.motion_target(code, true, word);
                    self.doc.selection = before;
                    if let (Some(t), Some(v)) = (target, self.vim.as_mut()) {
                        v.move_visual(&mut self.doc, t);
                        if !vertical {
                            self.editor.viewport.goal_x = None;
                        }
                        self.after_change(true);
                    }
                    return;
                }
                if self.doc.extra.is_empty() {
                    // A CSV grid keeps its column into short records.
                    let column = (vertical && !shift && !self.editor.source)
                        .then(|| self.doc.csv_column())
                        .flatten();
                    let Some(t) = self.motion_target(code, shift, word) else {
                        return;
                    };
                    self.doc.move_cursor(t, shift);
                    if let Some(c) = column {
                        self.doc.keep_csv_column(c);
                    }
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
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                let h = self.editor.area.height.saturating_sub(2).max(1) as isize;
                let d = match code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::PageUp => -h,
                    _ => h,
                };
                // A CSV grid: the same column of the row shown, a record
                // of several lines one row (by the text's lines, Down on
                // the last row went to its last field and typing replaced
                // it).
                if self.doc.meta.mode == DocumentMode::Csv && !self.editor.source {
                    return Some(
                        kalem_core::csv::view_vertical(&self.doc, head, d).unwrap_or(head),
                    );
                }
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
            PromptKind::Quit
                | PromptKind::QuitDiscard
                | PromptKind::Close
                | PromptKind::Reload
                | PromptKind::Overwrite
                | PromptKind::SaveConverted
                | PromptKind::ReplaceFile(_)
                | PromptKind::CreateFolder(..)
        );
        // A cell's entry: Alt+Enter a line break, Ctrl+Enter into every
        // selected cell, AutoComplete's offer taken with Enter or turned
        // down with Delete.
        let entry = match &p.kind {
            PromptKind::Arg {
                command,
                name,
                args,
                ..
            } => Some((command.clone(), name.clone(), args.clone())),
            _ => None,
        };
        if let Some((command, name, args)) = entry {
            // A path: Tab completes it as far as the folder's entries
            // agree, which the line over the prompt lists.
            if k.code == KeyCode::Tab
                && let Some(folders) = kalem_core::path_prompt::path_argument(&command, &name)
            {
                let base = kalem_core::command::folder_of(&self.doc);
                let found = kalem_core::path_prompt::entries(&p.input, base.as_deref(), folders);
                p.input = kalem_core::path_prompt::complete(&p.input, &found);
                p.back = 0;
                self.prompt = Some(p);
                return;
            }
            let alt = k.modifiers.contains(KeyModifiers::ALT);
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            if k.code == KeyCode::Enter
                && alt
                && kalem_core::viewer::multiline_prompt(&command, &name)
            {
                kalem_core::line_edit::insert(&mut p.input, p.back, "\n");
                p.declined = false;
                self.prompt = Some(p);
                return;
            }
            if kalem_core::viewer::cell_entry_prompt(&command, &name)
                && self.formula_key(&mut p, &k)
            {
                self.prompt = Some(p);
                return;
            }
            if kalem_core::viewer::cell_entry_prompt(&command, &name) {
                let offer = self.completion_offer(&p);
                if k.code == KeyCode::Delete && offer.is_some() {
                    p.declined = true;
                    self.prompt = Some(p);
                    return;
                }
                if k.code == KeyCode::Enter && ctrl {
                    let mut args = args;
                    args["inRange"] = serde_json::Value::Bool(true);
                    let args =
                        kalem_core::command::with_argument(args, &name, p.input.clone().into());
                    self.run_command(&command, args);
                    return;
                }
                if k.code == KeyCode::Enter
                    && let Some(full) = offer
                {
                    p.input = full;
                    p.back = 0;
                }
            }
        }
        if !yes_no && let Some(edit) = line_key(&k) {
            kalem_core::line_edit::apply(&mut p.input, &mut p.back, edit);
            p.declined = false;
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
                    (PromptKind::QuitDiscard, true, _) => self.close(),
                    (PromptKind::QuitDiscard, _, true) => self.message(tr!("msg-cancelled"), false),
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
                    (PromptKind::ReplaceFile(path), true, _) => {
                        let path = path.clone();
                        self.save_as_to(path);
                    }
                    (PromptKind::CreateFolder(path, save_as), true, _) => {
                        let (path, save_as) = (path.clone(), *save_as);
                        if let Some(dir) = missing_folder(&path)
                            && let Err(e) = std::fs::create_dir_all(&dir)
                        {
                            self.message(tr!("msg-not-saved", reason = e.to_string()), true);
                        } else if save_as {
                            self.save_as_to(path);
                        } else {
                            self.save(false);
                        }
                    }
                    (PromptKind::SaveConverted, true, _) => {
                        self.doc.conversion_accepted = true;
                        self.save(false);
                    }
                    (_, _, true) => self.message(tr!("msg-cancelled"), false),
                    _ => self.prompt = Some(p),
                }
                return;
            }
            KeyCode::Enter => {}
            KeyCode::Char(c) if input::text(&k).is_some() => {
                kalem_core::line_edit::insert(&mut p.input, p.back, c.encode_utf8(&mut [0; 4]));
                p.declined = false;
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
            PromptKind::Setting { field, at } => {
                let value = match at {
                    Some(at) => settings_list::put(&self.config, &field, at, &p.input),
                    None => settings_list::typed(&field, &p.input),
                };
                match value {
                    Ok(v) => self.set_field(&field, &v, false),
                    Err(e) => {
                        self.message(e, true);
                        // Asked again, the text kept.
                        self.prompt = Some(Prompt {
                            kind: PromptKind::Setting { field, at },
                            ..p
                        });
                    }
                }
            }
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
                if p.input.trim().is_empty() {
                    self.message(tr!("msg-no-file-name"), true);
                    return;
                }
                // `~` for the home folder; a relative name beside the
                // document, as Open takes it.
                let path = PathBuf::from(settings::expand_home(p.input.trim()));
                let path = match kalem_core::command::folder_of(&self.doc) {
                    Some(d) if !path.is_absolute() => d.join(&path),
                    _ => path,
                };
                // Another file there is replaced only when asked.
                if path.exists() && self.doc.meta.path.as_deref() != Some(path.as_path()) {
                    let label = tr!("prompt-replace-file", path = path.display().to_string());
                    self.ask(PromptKind::ReplaceFile(path), &label, String::new());
                    return;
                }
                if let Some(dir) = missing_folder(&path) {
                    self.ask_create_folder(path, &dir, true);
                    return;
                }
                self.save_as_to(path);
            }
            _ => {}
        }
    }

    /// Shows what a look at the active document's file found: a reload,
    /// a conflict to decide, the file deleted.
    fn disk_outcome(
        &mut self,
        change: Result<kalem_core::document::ExternalChange, kalem_core::files::OpenError>,
    ) {
        use kalem_core::document::ExternalChange;
        match change {
            Ok(ExternalChange::Reloaded) => {
                self.after_change(false);
                self.message(tr!("msg-reloaded"), false);
            }
            Ok(ExternalChange::Conflict) => {
                self.ask(PromptKind::Reload, &tr!("prompt-reload"), String::new());
            }
            Ok(ExternalChange::Deleted) => self.message(tr!("msg-deleted-on-disk"), true),
            Ok(ExternalChange::None) => {}
            Ok(ExternalChange::Listing) => self.after_change(false),
            Err(e) => self.message(tr!("msg-cannot-read", error = e.to_string()), true),
        }
    }

    /// Asks before making `dir`, the missing folder of `path`, to save
    /// there (`save_as`: Save As to `path`), as Emacs does.
    fn ask_create_folder(&mut self, path: PathBuf, dir: &Path, save_as: bool) {
        let label = tr!("prompt-create-folder", path = dir.display().to_string());
        self.ask(
            PromptKind::CreateFolder(path, save_as),
            &label,
            String::new(),
        );
    }

    /// Saves the document as `path` (Save As, once its name is settled).
    fn save_as_to(&mut self, path: PathBuf) {
        // The steps of a save: `document:before-save` (which may refuse
        // it) and the document's own.
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
        match self.doc.save_as(&path, self.config.save_options()) {
            Ok(()) => {
                if let Some(w) = &mut self.watcher {
                    let _ = w.watch(&path);
                }
                self.bus.emit(&Event::DocumentAfterSave {
                    doc: self.doc_id,
                    path: path.clone(),
                });
                kalem_core::lsp::saved(&self.doc);
                if self.doc.latex().is_some() && self.config.bool("latex.build_on_save") {
                    self.run_command("latex.build", serde_json::Value::Null);
                }
                // The new name's mode (`x.py` is Python).
                if self.doc.mode_for_name(&self.config.parse_base()) {
                    self.editor.reset();
                    self.refresh_vim();
                    self.dirty = true;
                }
                self.message(
                    tr!("msg-saved-as", path = path.display().to_string()),
                    false,
                );
            }
            Err(e) => self.message(tr!("msg-not-saved", reason = e.to_string()), true),
        }
    }

    /// What a formula being typed into a cell shows over the prompt: the
    /// arguments of the function it is in, else the names completing it.
    fn formula_hint_line(&mut self, p: &Prompt) -> Option<String> {
        let PromptKind::Arg { command, name, .. } = &p.kind else {
            return None;
        };
        // A path's: the entries of the folder typed that it may become.
        if let Some(folders) = kalem_core::path_prompt::path_argument(command, name) {
            let base = kalem_core::command::folder_of(&self.doc);
            let found = kalem_core::path_prompt::entries(&p.input, base.as_deref(), folders);
            return (!found.is_empty())
                .then(|| format!("Tab: {}", kalem_core::path_prompt::hint(&found)));
        }
        if !kalem_core::viewer::cell_entry_prompt(command, name) || !p.input.starts_with('=') {
            return None;
        }
        let len = p.input.chars().count();
        let at = len - p.back.min(len);
        let h = self.doc.viewer.as_deref_mut()?.formula_hint(&p.input, at);
        if !h.completions.is_empty() {
            return Some(format!("Tab: {}", h.completions.join("  ")));
        }
        h.tip
    }

    /// A formula's keys in a cell's entry: the arrows pointing at cells
    /// (Shift to a range), F4 cycling the reference's `$`, Tab completing a
    /// function or a name; any other key ends the pointing. `true` when
    /// the key was taken.
    fn formula_key(&mut self, p: &mut Prompt, k: &KeyEvent) -> bool {
        use kalem_core::formula_edit;
        let PromptKind::Arg { args, .. } = &p.kind else {
            return false;
        };
        let from = (
            args.get("row").and_then(Value::as_u64).unwrap_or(0) as u32,
            args.get("col").and_then(Value::as_u64).unwrap_or(0) as u32,
        );
        let Some(v) = self.doc.viewer.as_deref_mut() else {
            return false;
        };
        let len = p.input.chars().count();
        let at = len - p.back.min(len);
        let set = |p: &mut Prompt, text: String, cursor: usize| {
            p.back = text.chars().count() - cursor;
            p.input = text;
        };
        let arrow = match k.code {
            KeyCode::Up => Some((-1, 0)),
            KeyCode::Down => Some((1, 0)),
            KeyCode::Left => Some((0, -1)),
            KeyCode::Right => Some((0, 1)),
            _ => None,
        };
        if let Some(d) = arrow {
            let extend = k.modifiers.contains(KeyModifiers::SHIFT);
            let max = v.grid_max();
            if let Some((text, cursor)) =
                formula_edit::point(&mut p.pointing, &p.input, at, from, d, extend, max)
            {
                set(p, text, cursor);
                v.pointer = p.pointing.map(|x| x.range());
                return true;
            }
        }
        p.pointing = None;
        v.pointer = None;
        match k.code {
            KeyCode::F(4) => {
                if let Some((text, cursor)) = formula_edit::toggle_absolute(&p.input, at) {
                    set(p, text, cursor);
                }
                true
            }
            KeyCode::Tab => {
                let h = v.formula_hint(&p.input, at);
                if let Some(c) = h.completions.first() {
                    let (text, cursor) = formula_edit::complete(&p.input, at, h.typed, c);
                    set(p, text, cursor);
                }
                true
            }
            _ => false,
        }
    }

    /// What AutoComplete offers for a cell's entry being typed: the column's
    /// one entry it begins, with the cursor at the end and the offer not
    /// turned down.
    fn completion_offer(&mut self, p: &Prompt) -> Option<String> {
        let PromptKind::Arg { command, name, .. } = &p.kind else {
            return None;
        };
        if p.declined || p.back != 0 || !kalem_core::viewer::cell_entry_prompt(command, name) {
            return None;
        }
        self.doc.viewer.as_deref_mut()?.column_completion(&p.input)
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
    /// A completion menu is open with its items in (none still coming).
    pub fn completion_ready(&self) -> bool {
        self.completion
            .as_ref()
            .is_some_and(|m| !m.session.waiting() && !m.items().is_empty())
    }

    /// Adds a completer (a plugin's, a test's).
    pub fn register_completer(&mut self, c: std::sync::Arc<dyn kalem_core::completers::Completer>) {
        self.completers.register(c);
    }

    pub fn tick(&mut self, now: Instant) {
        self.close_stopped_viewer();
        // The plugins' commands and keys changed: built again; the
        // commands their event handlers asked for, run.
        let plugins = kalem_core::extensions::generation();
        if plugins != self.plugins_seen {
            self.plugins_seen = plugins;
            self.registry = CommandRegistry::with_builtins();
            self.rebuild_keys();
            self.dirty = true;
        }
        for (id, args) in kalem_core::extensions::take_runs() {
            self.run_command(&id, args);
        }
        // Their questions, asked; their status items and panels, drawn
        // again when they change.
        for r in kalem_core::extensions::take_requests() {
            self.request(r);
        }
        // The marks plugins set beside the lines (the git plugin's).
        if self.doc.sync_gutter() {
            self.dirty = true;
        }
        for b in self.docs.iter_mut().flatten() {
            b.doc.sync_gutter();
        }
        let written = kalem_core::extensions::generated_writes();
        if written != self.written_seen {
            self.written_seen = written;
            self.generated_written();
        }
        let shown = kalem_core::extensions::shown();
        if shown != self.shown_seen {
            self.shown_seen = shown;
            self.dirty = true;
        }
        // Language servers: the document in step, their answers shown.
        kalem_core::lsp::sync(&self.doc);
        if kalem_core::lsp::tick() {
            self.dirty = true;
        }
        if let Some(p) = self.doc.meta.path.clone() {
            for o in kalem_core::lsp::take_outcomes(&p, self.doc.version()) {
                self.lsp_outcome(o);
            }
        }
        // Documents in the background take the edits meant for them (a
        // rename across files); the rest of their answers are dropped.
        for b in self.docs.iter_mut().flatten() {
            let Some(p) = b.doc.meta.path.clone() else {
                continue;
            };
            for o in kalem_core::lsp::take_outcomes(&p, b.doc.version()) {
                if let kalem_core::lsp::Outcome::Edits {
                    version,
                    edits,
                    label,
                    ..
                } = o
                    && version == b.doc.version()
                    && let Some(tx) = kalem_core::lsp::transaction(&edits, &label)
                {
                    b.doc.apply(&tx, org_edit::ChangeKind::Command, now);
                    kalem_core::lsp::sync(&b.doc);
                }
            }
        }
        // Items of slow completers, and the chosen one's documentation.
        if let Some(m) = &mut self.completion {
            if m.session.waiting() && m.session.poll() {
                self.dirty = true;
            }
            if m.fetch_documentation() {
                self.dirty = true;
            }
        }
        // Lists background work offers (a plugin to confirm).
        for items in kalem_core::jobs::take_offers() {
            self.request(Request::Choose(items));
        }
        // What background work tells the user (a plugin update found).
        for (text, error) in kalem_core::jobs::take_notices() {
            self.message(text, error);
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
            let mut outcomes = Vec::new();
            self.jobs.retain_mut(|j| {
                let kind = j.kind();
                match j.poll() {
                    Some((out, msg, error)) => {
                        done.push((msg, error));
                        outcomes.extend(kind.map(|k| (k, out)));
                        false
                    }
                    None => true,
                }
            });
            for (kind, out) in outcomes {
                self.follow_files(kind, &out);
            }
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
        // A viewer's search: its matches as they are found.
        if let Some(v) = self.doc.viewer.as_deref_mut()
            && v.search_poll()
        {
            self.dirty = true;
        }
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
        // The which-key panel once its delay has passed.
        if !self.pending.is_empty()
            && !self.hints_drawn
            && kalem_core::keymap::hints_due(&self.config, self.pending_at, now)
                .is_some_and(|d| d.is_zero())
        {
            self.dirty = true;
        }
        self.bus.dispatch_queued();
        let changed: Vec<PathBuf> = self.changed_files.borrow_mut().drain(..).collect();
        for b in self.docs.iter_mut().flatten() {
            if watches(&b.doc, &changed) && !b.doc.is_modified() {
                let _ = b.doc.external_change(now);
            }
        }
        if watches(&self.doc, &changed) {
            if self.prompt.is_some() {
                // Looked at once the question is answered, not dropped.
                self.changed_files.borrow_mut().extend(changed);
            } else {
                let change = self.doc.external_change(now);
                self.disk_outcome(change);
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
        // The which-key panel when its delay has passed.
        if !self.pending.is_empty()
            && let Some(d) = kalem_core::keymap::hints_due(&self.config, self.pending_at, now)
            && !d.is_zero()
        {
            t = t.min(d);
        }
        if !self.jobs.is_empty() || self.doc.viewer.as_deref().is_some_and(|v| v.searching()) {
            t = t.min(Duration::from_millis(30));
        }
        // A language server's answers and completions as they come.
        if kalem_core::lsp::busy()
            || self
                .completion
                .as_ref()
                .is_some_and(|m| m.session.waiting())
        {
            t = t.min(Duration::from_millis(30));
        } else if kalem_core::lsp::active() {
            // Diagnostics and progress come when they come.
            t = t.min(Duration::from_millis(100));
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
    /// Draws the panes in `area`, a rule between them; the cursor of the
    /// focused one.
    /// Draws the active document into `area`: its text, or what a viewer
    /// shows.
    fn draw_document(
        &mut self,
        buf: &mut ratatui::buffer::Buffer,
        area: Rect,
    ) -> Option<(u16, u16)> {
        if self.doc.viewer.is_none() {
            return self.editor.draw(&self.doc, &self.caps, buf, area);
        }
        let picker = self.editor.images.borrow().picker.clone();
        crate::viewer::draw(
            &mut self.doc,
            &mut self.viewer_image,
            picker.as_ref(),
            &self.caps,
            buf,
            area,
        );
        self.draw_cell_entry(buf, area);
        None
    }

    /// A cell's entry being typed, shown in its cell as typed (as Excel
    /// does), running on to the right as it grows; the cursor marked.
    fn draw_cell_entry(&mut self, buf: &mut ratatui::buffer::Buffer, area: Rect) {
        let Some(p) = &self.prompt else { return };
        let PromptKind::Arg {
            command,
            args,
            name,
            ..
        } = &p.kind
        else {
            return;
        };
        if command != "viewer.grid.setCell" || name != "value" {
            return;
        }
        let (Some(r), Some(c)) = (
            args.get("row").and_then(Value::as_u64),
            args.get("col").and_then(Value::as_u64),
        ) else {
            return;
        };
        let Some(hits) = self.doc.viewer.as_deref().and_then(|v| v.hits.as_ref()) else {
            return;
        };
        let (Some(&(_, x, w)), Some(&(_, y))) = (
            hits.cols.iter().find(|h| u64::from(h.0) == c),
            hits.rows.iter().find(|h| u64::from(h.0) == r),
        ) else {
            return;
        };
        let (before, after) = kalem_core::line_edit::split(&p.input, p.back);
        let text = format!("{before}{after}").replace('\n', "↵");
        let caret = before.replace('\n', "↵").chars().count();
        let width = (text.chars().count() as u16 + 1)
            .max(w)
            .min(area.right().saturating_sub(x));
        let style = ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD);
        let mut chars = text.chars();
        for i in 0..width {
            let ch = chars.next().unwrap_or(' ');
            if let Some(cell) = buf.cell_mut((x + i, y)) {
                cell.reset();
                cell.set_char(ch);
                let mut st = style;
                if i as usize == caret {
                    st = st.add_modifier(ratatui::style::Modifier::REVERSED);
                }
                cell.set_style(st);
            }
        }
    }

    /// Inserts a link to `file` in the text document shown last, which is
    /// shown.
    fn insert_link(&mut self, file: &std::path::Path) {
        let text = |d: &DocumentState| d.dired.is_none() && d.viewer.is_none();
        let target = self
            .last_document
            .and_then(|id| self.doc_index(id))
            .filter(|&i| i != self.active && self.docs[i].as_ref().is_some_and(|b| text(&b.doc)))
            .or_else(|| {
                (0..self.docs.len()).rev().find(|&i| {
                    i != self.active && self.docs[i].as_ref().is_some_and(|b| text(&b.doc))
                })
            });
        let Some(i) = target else {
            self.message(kalem_core::l10n::tr("msg-viewer-no-document"), true);
            return;
        };
        self.activate(i);
        if let Err(e) = self
            .doc
            .drop_pictures(std::slice::from_ref(&file.to_path_buf()), Instant::now())
        {
            self.message(e, true);
        }
    }

    fn draw_panes(&mut self, buf: &mut ratatui::buffer::Buffer, area: Rect) -> Option<(u16, u16)> {
        use kalem_core::layout::Rect as R;
        let rects = self.layout.rects(R {
            x: f32::from(area.x),
            y: f32::from(area.y),
            w: f32::from(area.width),
            h: f32::from(area.height),
        });
        let rule = crate::panels::panel_style(&self.caps);
        let focus = self.layout.focus();
        let mut cursor = None;
        self.pane_areas.clear();
        for (p, r) in rects {
            let (x0, y0) = (r.x.round() as u16, r.y.round() as u16);
            let (x1, y1) = ((r.x + r.w).round() as u16, (r.y + r.h).round() as u16);
            let mut a = Rect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0));
            // A rule on the right and at the bottom, inside the window.
            if x1 < area.right() && a.width > 1 {
                a.width -= 1;
                for y in a.top()..a.bottom() {
                    buf[(a.right(), y)].set_symbol("│").set_style(rule);
                }
            }
            if y1 < area.bottom() && a.height > 1 {
                a.height -= 1;
                let title = if p == focus {
                    format!(" {} ", self.open_files()[self.active].title)
                } else {
                    self.panes
                        .get(&p)
                        .and_then(|(d, _)| self.doc_index(*d))
                        .map(|i| format!(" {} ", self.open_files()[i].title))
                        .unwrap_or_default()
                };
                for x in a.left()..a.right() {
                    buf[(x, a.bottom())].set_symbol("─").set_style(rule);
                }
                buf.set_stringn(a.x + 1, a.bottom(), &title, a.width as usize, rule);
            }
            self.pane_areas.push((p, a));
            if p == focus {
                cursor = self.draw_document(buf, a);
                continue;
            }
            // A pane an undo brought back shows the active document.
            let (doc, mut view) = match self.panes.remove(&p) {
                Some(e) => e,
                None => (self.doc_id, self.new_view(&self.doc)),
            };
            let caps = self.caps.clone();
            match self.doc_index(doc) {
                Some(i) if i == self.active => {
                    view.draw(&self.doc, &caps, buf, a);
                }
                Some(i) => {
                    if let Some(b) = &self.docs[i] {
                        view.draw(&b.doc, &caps, buf, a);
                    }
                }
                None => {}
            }
            self.panes.insert(p, (doc, view));
        }
        cursor
    }

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
                    // Not beside the file manager, which lists the files.
                    if self.config.bool("ui.folder_tree")
                        && self.files_at == FilesAt::Left
                        && self.doc.dired.is_none() =>
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
            // The plugins' buttons where their when-clauses hold (the git
            // plugin's Git in a repository).
            let ctx = self.doc.document_context();
            let buttons: Vec<(String, String)> = kalem_core::extensions::buttons()
                .into_iter()
                .filter(|b| b.shows(&ctx) && self.registry.offered(&b.command, &ctx))
                .map(|b| (b.command, b.title))
                .collect();
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
                    &buttons,
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
                    &buttons,
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
            let head = kalem_core::viewer::outline_position(&self.doc);
            let current = o
                .items
                .iter()
                .rposition(|i| i.file.is_none() && i.start <= head);
            o.draw(f.buffer_mut(), panel, current, &self.caps);
            text_area.x += w;
            text_area.width -= w;
        }
        // A plugin's panel: at the side, or at the bottom.
        if let Some(p) = self
            .plugin_panel
            .as_deref()
            .and_then(kalem_core::extensions::panel)
        {
            if p.bottom {
                let h = (p.lines().len() as u16 + 3)
                    .min(text_area.height / 3)
                    .max(3)
                    .min(text_area.height.saturating_sub(3));
                let panel = Rect {
                    y: text_area.bottom() - h,
                    height: h,
                    ..text_area
                };
                crate::panels::draw_plugin_panel(f.buffer_mut(), panel, &p, &self.caps);
                text_area.height -= h;
            } else {
                let w = OutlinePanel::width(text_area.width);
                let panel = Rect {
                    width: w,
                    ..text_area
                };
                crate::panels::draw_plugin_panel(f.buffer_mut(), panel, &p, &self.caps);
                text_area.x += w;
                text_area.width -= w;
            }
        }
        self.editor.block = self
            .vim
            .as_ref()
            .and_then(|v| v.block_ranges(&self.doc))
            .or_else(|| {
                // A CSV grid's rectangle of cells.
                (!self.editor.source)
                    .then(|| kalem_core::csv::rectangle_ranges(&self.doc))
                    .flatten()
            })
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
        let cursor = if self.layout.is_split() {
            self.draw_panes(f.buffer_mut(), text_area)
        } else {
            self.pane_areas.clear();
            self.draw_document(f.buffer_mut(), text_area)
        };
        if let Some(c) = cursor {
            self.draw_popup(f.buffer_mut(), text_area, c);
        }
        if let Some(p) = &self.palette {
            p.draw(f.buffer_mut(), text_area, &self.caps);
        }
        if let Some(s) = &self.settings {
            let over = Rect {
                height: area.height.saturating_sub(1),
                ..area
            };
            s.draw(f.buffer_mut(), over, &self.config, &self.caps);
        }
        let due = kalem_core::keymap::hints_due(&self.config, self.pending_at, Instant::now());
        self.hints_drawn = !self.pending.is_empty() && due.is_some_and(|d| d.is_zero());
        if self.hints_drawn {
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
            let text = match self.doc.viewer.as_deref() {
                Some(v) => fd.line_counted(v.search_status()),
                None => fd.line(Some(&sel)),
            };
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
        let (offer, hint) = match self.prompt.take() {
            Some(p) => {
                let o = self.completion_offer(&p);
                let h = self.formula_hint_line(&p);
                self.prompt = Some(p);
                (o, h)
            }
            None => (None, None),
        };
        // A formula's argument tip or completions, over the prompt.
        if let Some(h) = &hint
            && y > area.y
        {
            buf.set_stringn(
                area.x + 1,
                y - 1,
                format!("{h:width$}", width = area.width.saturating_sub(1) as usize),
                area.width.saturating_sub(1) as usize,
                bar,
            );
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
            // A line break in a cell's entry shows as ↵; a password as dots.
            let masked = matches!(&p.kind, PromptKind::Arg { name, .. } if name == "password");
            let mask = |s: &str| {
                if masked {
                    "•".repeat(s.chars().count())
                } else {
                    s.replace('\n', "↵")
                }
            };
            let before = mask(&before);
            let shown = format!("{before}{}", mask(after));
            buf.set_stringn(area.x + 1 + lw, y, &shown, room, bar);
            // AutoComplete's offer after what is typed, faint.
            if let Some(full) = &offer {
                let rest: String = full.chars().skip(p.input.chars().count()).collect();
                let at = area.x + 1 + lw + shown.width() as u16;
                if at < area.right() {
                    buf.set_stringn(
                        at,
                        y,
                        &rest,
                        (area.right() - at) as usize,
                        bar.add_modifier(Modifier::DIM),
                    );
                }
            }
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
        // The workspace, when there are several.
        if self.workspaces.several() {
            let s = format!(" [{}]", self.workspaces.current().name);
            buf.set_stringn(x, y, &s, room(x), crate::panels::accent_style(caps, bar));
            x += s.width() as u16;
        }
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
            DocumentMode::Org => tr!("kind-org"),
            DocumentMode::Markdown => "Markdown".into(),
            DocumentMode::Csv => "CSV".into(),
            DocumentMode::Latex => "LaTeX".into(),
            DocumentMode::Text { language: Some(l) } => l.clone(),
            // The view: the file manager, or the projects.
            DocumentMode::Directory => match self.doc.dired.as_deref() {
                Some(d) => d.list_title(),
                None => tr!("mode-directory"),
            },
            // A viewer's file: the plugin's name (Image).
            DocumentMode::Viewer => self
                .doc
                .viewer
                .as_deref()
                .map_or_else(|| tr!("mode-viewer"), |v| v.viewer.name().to_string()),
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
        // The plugins' items: the left ones, then the right ones.
        let (left, right) = kalem_core::extensions::status_items();
        for item in left.iter().chain(right.iter().rev()) {
            let s = format!("  {}", item.text);
            buf.set_stringn(x, y, &s, room(x), crate::panels::accent_style(caps, bar));
            x += s.width() as u16;
        }
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
                    .push((Rect::new(mx, y, fw, 1), "dired.jump".into()));
                self.action_spots.push((
                    Rect::new(mx + fw, y, w.min(room).saturating_sub(fw), 1),
                    "view.palette".into(),
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
