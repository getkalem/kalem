//! The command registry (design §11.2): every action of the editor is a
//! command with an ID, a title, a category, default keys and an optional
//! when-clause. Built-in commands are native functions; plugin commands
//! will be script callbacks (phase 3). A command's change is one undo step.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

use jiff::civil::DateTime;
use org_edit::{EditError, Transaction};
use org_model::Document;
use serde_json::Value;

use crate::document::DocumentState;
use crate::keys::KeySequence;
use crate::when::WhenClause;

/// Text cut or copied by commands (the subtree clipboard).
#[derive(Debug, Clone, Default)]
pub struct Clipboard {
    /// The text.
    pub text: String,
}

/// What a command works with.
#[derive(Debug)]
pub struct EditorContext<'a> {
    /// The active document, if any.
    pub document: Option<&'a mut DocumentState>,
    /// The clipboard.
    pub clipboard: &'a mut Clipboard,
    /// The settings.
    pub config: &'a crate::settings::Config,
    /// The time, for undo grouping.
    pub now: Instant,
    /// The local date and time, for timestamps.
    pub clock: DateTime,
    /// Messages for the status bar.
    pub messages: Vec<String>,
    /// What the command asks of the frontend, handled after it returns.
    pub requests: Vec<Request>,
}

/// Something only the frontend can do: file dialogs, the system clipboard,
/// its own panels, quitting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Save the active document (after `document:before-save`).
    Save,
    /// Save the active document under a new name.
    SaveAs,
    /// Quit, asking about unsaved changes.
    Quit,
    /// Copy the selection to the system clipboard.
    Copy,
    /// Cut the selection to the system clipboard.
    Cut,
    /// Paste from the system clipboard; `plain` without converting tables
    /// and HTML.
    Paste {
        /// Insert the text as it is.
        plain: bool,
    },
    /// Open the command palette.
    Palette,
    /// List the menus' items to choose from (`palette::menu_items`).
    Menus,
    /// Open find, or find and replace.
    Find {
        /// With a replacement field.
        replace: bool,
    },
    /// Show or hide the outline panel.
    Outline,
    /// Show or hide the file manager's preview pane (graphical): the file
    /// at the cursor, or with `thumbnails` the listing's pictures.
    Preview {
        /// The pictures of the listing.
        thumbnails: bool,
    },
    /// Switch between the rich view and the source view.
    ToggleSource,
    /// Show a second view of the document beside the first (the source
    /// beside the rich view), or close it.
    Split,
    /// Show the settings.
    Settings,
    /// Show only the section holding the cursor, or everything again.
    Focus,
    /// The window full screen, or back.
    FullScreen,
    /// The system's terminal at this folder.
    Terminal(std::path::PathBuf),
    /// A new window.
    NewWindow,
    /// The document's mode changed: views start again.
    ModeChanged,
    /// Wrap long lines, or not.
    ToggleWrap,
    /// Show formulas as rendered math or as their source.
    ToggleMath,
    /// Open a link target outside the document.
    OpenLink(crate::input::LinkAction),
    /// Cycle folding: the headline at point, or the whole document.
    Fold {
        /// The whole document (`S-TAB`).
        global: bool,
    },
    /// Open a file in the window: `path`, or ask for one.
    Open {
        /// The file.
        path: Option<String>,
    },
    /// Ask for a file (a picture, say) and run `command` again with its
    /// path, relative to the document's folder, as argument `arg`.
    PickFile {
        /// The command.
        command: String,
        /// Its argument that takes the path.
        arg: String,
        /// Its other arguments.
        args: serde_json::Value,
    },
    /// Ask for argument `arg` of `command` (a line of text) and run it with
    /// `args` and the answer: the steps of a dialog, in the palette.
    Ask {
        /// The command.
        command: String,
        /// Its arguments so far.
        args: serde_json::Value,
        /// The argument asked for.
        arg: String,
    },
    /// A new, empty document.
    New,
    /// Close the active document, asking about unsaved changes.
    Close,
    /// Show the next open document, or the previous one.
    Cycle {
        /// The previous one.
        back: bool,
    },
    /// Choose from a list: open documents, recent files, projects, a
    /// project's files.
    Pick(PickKind),
    /// Search the text of the project's files.
    SearchProject,
    /// Search the text of the files under a folder.
    SearchIn(std::path::PathBuf),
    /// Show or hide the list of open files.
    OpenFiles,
    /// Change the project list, or act on the project's documents.
    Project(ProjectRequest),
    /// The live search of lines ([`crate::line_search`]): this document's,
    /// or every open one's; headings only; with this text typed.
    SearchLines {
        /// Every open document.
        all: bool,
        /// Headings only.
        headings: bool,
        /// The query to start with.
        text: String,
    },
    /// Search another project: choose it first.
    SearchOtherProject,
    /// Act on the open documents (Doom's `SPC b`, T2.7i.2).
    Documents(DocumentsRequest),
    /// Show the file manager.
    FileManager(FileManagerRequest),
    /// Run a file operation in the background, after the questions it
    /// needs (`crate::dired::Task`).
    FileOp(FileOp),
    /// Run a shell command on files, after asking (`crate::dired::Task`).
    Shell(ShellOp),
    /// Stop the file operations that are running.
    CancelFileOps,
    /// Open the completion menu at the cursor (`crate::completers`).
    Complete,
    /// Put this text on the system clipboard.
    CopyText(String),
    /// Put rich text on the system clipboard: `html`, with `text` for
    /// places that take plain text.
    CopyRich {
        /// The HTML.
        html: String,
        /// The plain text.
        text: String,
    },
    /// Show the export dialog: formats and export settings.
    ExportDialog,
    /// Offer a choice of commands, as the palette shows them.
    Choose(Vec<crate::palette::PaletteItem>),
    /// Save `key` in the user's settings and apply the settings.
    SetSetting {
        /// The setting.
        key: String,
        /// Its new value.
        value: serde_json::Value,
        /// Without a message (remembered choices such as recent colors).
        quiet: bool,
    },
}

/// What [`Request::FileManager`] shows. The window's file manager is used
/// when there is one, else a new one opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileManagerRequest {
    /// A folder: `dir`, else the active document's (with the cursor on
    /// it), else the working folder.
    Dir {
        /// The folder.
        dir: Option<std::path::PathBuf>,
    },
    /// The folder of the active document's project.
    ProjectRoot,
    /// Every project, the cursor on `select`.
    Projects {
        /// The project to put the cursor on.
        select: Option<std::path::PathBuf>,
    },
    /// Back to the document shown before the file manager.
    Leave,
}

/// A file operation for the frontend to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOp {
    /// Copy, move, trash or delete.
    pub kind: kalem_fs::OpKind,
    /// The files and folders.
    pub sources: Vec<std::path::PathBuf>,
    /// Where they go (copy and move).
    pub target: Option<std::path::PathBuf>,
}

/// A shell command on files (Dired's `!`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOp {
    /// The command, with `*` or `?` for the files.
    pub command: String,
    /// The files, relative to `dir` or absolute.
    pub files: Vec<std::path::PathBuf>,
    /// Where it runs.
    pub dir: std::path::PathBuf,
}

/// The lists [`Request::Pick`] offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PickKind {
    /// The open documents.
    Documents,
    /// The open documents of the current project.
    ProjectDocuments,
    /// Files opened recently, anywhere.
    RecentFiles,
    /// The projects, most recently used first; choosing one opens its
    /// last file or its file picker.
    Projects,
    /// The files of the current project (after choosing a project when
    /// there is none).
    ProjectFiles,
    /// Files opened recently in the current project.
    ProjectRecentFiles,
    /// The projects, to remove one from the list.
    RemoveProject,
}

/// Changes of the project list and actions on a project's documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRequest {
    /// Add a folder: `path`, or the active document's folder.
    Add(Option<String>),
    /// Rename the current project.
    Rename(String),
    /// Walk the current project's files again.
    Refresh,
    /// Save the current project's modified documents.
    SaveAll,
    /// Close the current project's documents.
    CloseAll,
    /// Open the folder tree down to the active document.
    RevealInTree,
}

/// What to do with the open documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentsRequest {
    /// Save every modified document that has a file.
    SaveAll,
    /// Close the other documents that have no unsaved changes.
    CloseOthers,
    /// Close every document that has no unsaved changes; an empty one
    /// stays when none would.
    CloseAll,
    /// Show the document shown before this one.
    Last,
    /// Put this document at the end of the list and show the next one.
    Bury,
    /// Open the scratch document ([`scratch_path`]), the project's with
    /// `project`.
    Scratch {
        /// The current project's own.
        project: bool,
    },
}

/// The scratch document: `scratch.klm` in the state directory, or for
/// the project at `project` one named after it there, so nothing is
/// written into the project.
pub fn scratch_path(project: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let dir = crate::logging::state_dir()?.join("scratch");
    Some(match project {
        None => dir.join("scratch.klm"),
        Some(root) => {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            root.hash(&mut h);
            let name = root
                .file_name()
                .map_or_else(|| "project".into(), |n| n.to_string_lossy().into_owned());
            dir.join(format!("{name}-{:08x}.klm", h.finish() as u32))
        }
    })
}

/// Why a command failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    /// The message for the user.
    pub message: String,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<EditError> for CommandError {
    fn from(e: EditError) -> Self {
        CommandError { message: e.message }
    }
}

impl CommandError {
    /// An error with this message.
    pub fn new(message: impl Into<String>) -> CommandError {
        CommandError {
            message: message.into(),
        }
    }
}

/// The result of a command.
pub type CommandResult = Result<(), CommandError>;

/// A script function registered by a plugin (phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScriptCallbackId(pub u64);

/// What runs a command.
#[derive(Clone)]
pub enum CommandHandler {
    /// A Rust function.
    Native(fn(&mut EditorContext<'_>, &Value) -> CommandResult),
    /// A script callback.
    Script(ScriptCallbackId),
}

impl fmt::Debug for CommandHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandHandler::Native(_) => f.write_str("Native"),
            CommandHandler::Script(id) => write!(f, "Script({})", id.0),
        }
    }
}

/// A command.
#[derive(Debug, Clone)]
pub struct Command {
    /// `area.action`, or `pluginId.action` for plugins.
    pub id: String,
    /// The title in the palette and menus.
    pub title: String,
    /// The group in the palette.
    pub category: String,
    /// Key bindings of the default profile.
    pub default_keys: Vec<KeySequence>,
    /// When the command applies.
    pub when: Option<WhenClause>,
    /// What runs it.
    pub handler: CommandHandler,
    /// A JSON schema for the arguments, if it takes any. Frontends ask for
    /// required arguments that a key binding or the palette does not give.
    pub args_schema: Option<Value>,
    /// Who registered the command.
    pub source: CommandSource,
    /// The text types it serves (§11.2); registration refuses a command
    /// without one, and folds it into `when` as a clause over `textType`.
    pub scope: Option<Scope>,
}

/// The text type a language name stands for (§11.2): one name for each
/// language, whatever the extension or source block says (`py` and
/// `python` are `python`), lower case.
pub fn canonical_type(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    let canonical = match n.as_str() {
        "py" | "pyw" | "python3" => "python",
        "js" | "mjs" | "cjs" | "node" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "rs" => "rust",
        "rb" => "ruby",
        "pl" | "pm" => "perl",
        "sh" | "bash" | "zsh" | "ksh" => "shell",
        "elisp" | "el" => "emacs-lisp",
        "md" | "mdown" | "mkd" | "gfm" => "markdown",
        "yml" => "yaml",
        "htm" | "xhtml" => "html",
        "tex" | "ltx" => "latex",
        "c++" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "h" => "c",
        "cs" => "csharp",
        "kt" | "kts" => "kotlin",
        "hs" => "haskell",
        "ml" | "mli" => "ocaml",
        "jl" => "julia",
        "golang" => "go",
        "ps1" => "powershell",
        "bat" | "cmd" => "batch",
        "clj" | "cljs" => "clojure",
        "ex" | "exs" => "elixir",
        "erl" => "erlang",
        "scm" | "ss" => "scheme",
        "tsv" | "tab" => "csv",
        "txt" => "text",
        other => other,
    };
    canonical.to_string()
}

/// Whether `t` names a text type Kalem knows: its modes, `latex` and the
/// back-ends of export blocks, and the languages of files and source
/// blocks it has comment markers for, or common data formats.
pub fn known_text_type(t: &str) -> bool {
    const TYPES: &[&str] = &[
        "org",
        "klm",
        "markdown",
        "csv",
        "text",
        "directory",
        "latex",
        "html",
        "json",
        "txt",
        "log",
        "tsv",
        "diff",
        "patch",
        "bib",
        "rtf",
        "srt",
        "ascii",
    ];
    let t = t.to_ascii_lowercase();
    TYPES.contains(&t.as_str()) || crate::code::comment_style(&t).is_some()
}

/// The text types a command serves: all, or a list of them, less some
/// (§11.2). A type is the innermost at the cursor: the file's (`org`,
/// `klm`, `markdown`, `csv`, `text`, a language such as `rs`), or inside an
/// Org document a source block's language, an export block's back-end or
/// `latex` in a formula. `klm` is a kind of `org`: `org` takes it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// The types, `None` for all.
    pub types: Option<Vec<String>>,
    /// Types left out.
    pub except: Vec<String>,
}

impl Scope {
    /// Every type.
    pub fn all() -> Scope {
        Scope {
            types: None,
            except: Vec::new(),
        }
    }

    /// Only these types.
    pub fn only(types: &[&str]) -> Scope {
        Scope {
            types: Some(types.iter().map(|t| t.to_string()).collect()),
            except: Vec::new(),
        }
    }

    /// Every type but these.
    pub fn except(types: &[&str]) -> Scope {
        Scope {
            types: None,
            except: types.iter().map(|t| t.to_string()).collect(),
        }
    }

    /// The types a scope type stands for: `org` takes in `klm`.
    fn expand(t: &str) -> Vec<&str> {
        if t == "org" {
            vec!["org", "klm"]
        } else {
            vec![t]
        }
    }

    /// Whether the scope serves `text_type`.
    pub fn serves(&self, text_type: &str) -> bool {
        let is = |t: &String| Scope::expand(t).contains(&text_type);
        self.types.as_ref().is_none_or(|ts| ts.iter().any(is)) && !self.except.iter().any(is)
    }

    /// The scope as a when-clause over `textType`; `None` for all types.
    pub fn clause(&self) -> Option<WhenClause> {
        let eq = |t: &str| WhenClause::Eq("textType".into(), crate::when::Value::Str(t.into()));
        let any = |ts: &[String]| {
            ts.iter()
                .flat_map(|t| Scope::expand(t))
                .map(eq)
                .reduce(|a, b| WhenClause::Or(Box::new(a), Box::new(b)))
        };
        let only = self.types.as_deref().and_then(any);
        let not = any(&self.except).map(|e| WhenClause::Not(Box::new(e)));
        match (only, not) {
            (Some(a), Some(b)) => Some(WhenClause::And(Box::new(a), Box::new(b))),
            (a, b) => a.or(b),
        }
    }

    /// How the manual and `kalem commands` name it: `all`, `org`,
    /// `all except org`.
    pub fn describe(&self) -> String {
        let t = match &self.types {
            None => "all".to_string(),
            Some(ts) => ts.join(", "),
        };
        if self.except.is_empty() {
            t
        } else {
            format!("{t} except {}", self.except.join(", "))
        }
    }
}

impl Command {
    /// The title in the interface language (built-in commands; others
    /// bring their own).
    pub fn display_title(&self) -> String {
        if self.source == CommandSource::Builtin {
            crate::l10n::tr(&crate::l10n::command_key(&self.id))
        } else {
            self.title.clone()
        }
    }

    /// The category in the interface language.
    pub fn display_category(&self) -> String {
        if self.source == CommandSource::Builtin {
            crate::l10n::tr(&format!("category-{}", self.category.to_lowercase()))
        } else {
            self.category.clone()
        }
    }
}

/// Who registered a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSource {
    /// Kalem. Keymap profiles replace the default keys of built-in
    /// commands.
    Builtin,
    /// A plugin, by ID.
    Plugin(String),
    /// The user's own definitions (a macro recorded or a command
    /// defined in settings; there is no script file, D9, D28).
    User,
}

/// Whether `id` follows the convention: two or more dot-separated parts,
/// each a lower case letter followed by letters and digits (`org.todo.cycle`,
/// `table.insertRow`).
pub fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            let mut c = p.chars();
            c.next().is_some_and(|f| f.is_ascii_lowercase()) && c.all(|x| x.is_ascii_alphanumeric())
        })
}

/// The commands of the editor.
#[derive(Debug, Clone, Default)]
pub struct CommandRegistry {
    commands: BTreeMap<String, Command>,
}

/// The first required argument of `cmd` missing from `args`: its name and
/// JSON type (`string` when the schema does not say). Frontends ask for it.
pub fn missing_argument(cmd: &Command, args: &Value) -> Option<(String, String)> {
    let schema = cmd.args_schema.as_ref()?;
    let name = schema["required"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find(|n| args.get(*n).is_none())?;
    let ty = schema["properties"][name]["type"]
        .as_str()
        .unwrap_or("string");
    Some((name.to_string(), ty.to_string()))
}

/// What the prompt for argument `name` of command `id`, given `args`
/// already, starts with: a property's value when Set Property knows the
/// key, the caption or name of the element at the cursor, the color used last for Text Color and Highlight, else as
/// [`argument_default`].
pub fn argument_default_with(
    id: &str,
    name: &str,
    args: &serde_json::Value,
    doc: &mut crate::document::DocumentState,
    config: &crate::settings::Config,
) -> String {
    // A default the command put in its arguments (`label_default`).
    if let Some(v) = args
        .get(format!("{name}_default"))
        .and_then(serde_json::Value::as_str)
    {
        return v.to_string();
    }
    if (id, name) == ("org.property.set", "value")
        && let Some(key) = args.get("key").and_then(serde_json::Value::as_str)
    {
        let pos = doc.selection.head;
        if let Some(v) = doc
            .model()
            .and_then(|m| crate::properties::value(&m, pos, key))
        {
            return v;
        }
    }
    // The caption or name of the element at the cursor.
    let key = match (id, name) {
        ("org.caption.set", "caption") => Some("CAPTION"),
        ("org.name.set", "name") => Some("NAME"),
        _ => None,
    };
    if let Some(key) = key {
        let pos = doc.selection.head;
        return doc
            .model()
            .and_then(|m| crate::affiliated::value_at(&m, pos, key))
            .unwrap_or_default();
    }
    // The color used last.
    let recent = match (id, name) {
        ("format.color", "color") => config.strings("format.recent_colors").first().copied(),
        ("format.highlight", "color") => {
            config.strings("format.recent_highlights").first().copied()
        }
        _ => None,
    };
    match recent {
        Some(c) => c.to_string(),
        None => argument_default(id, name, doc),
    }
}

/// What the prompt for argument `name` of command `id` starts with: the
/// current formula for Edit Formula, the document's folder for Open.
pub fn argument_default(id: &str, name: &str, doc: &mut crate::document::DocumentState) -> String {
    if let Some(d) = crate::dired::argument_default(id, name, doc) {
        return d;
    }
    match (id, name) {
        // The link stored last.
        ("org.insert.link", "link") => crate::links::latest()
            .first()
            .map(|l| {
                let dir = doc
                    .meta
                    .path
                    .as_deref()
                    .and_then(|p| std::path::absolute(p).ok())
                    .and_then(|p| p.parent().map(std::path::Path::to_path_buf));
                crate::links::file_target(&l.path, dir.as_deref(), l.search.as_deref())
            })
            .unwrap_or_default(),
        ("latex.insert.figure", "width") => "0.8".to_string(),
        ("table.setFormula", "formula") => {
            let mut cache = crate::formulas::FormulaCache::default();
            crate::formulas::prompt(cache.get(doc))
        }
        // The document's folder (a listing's own), to add as a project or
        // open a file from.
        ("project.add", "path") | ("file.open", "path") => {
            let dir = folder_of(doc).or_else(|| std::env::current_dir().ok());
            dir.map(|d| {
                let mut s = d.display().to_string();
                if name == "path" && id == "file.open" && !s.ends_with(std::path::MAIN_SEPARATOR) {
                    s.push(std::path::MAIN_SEPARATOR);
                }
                s
            })
            .unwrap_or_default()
        }
        // The file's own path, to change.
        ("file.rename" | "file.copy", "target") => doc
            .meta
            .path
            .as_deref()
            .and_then(|p| std::path::absolute(p).ok())
            .map(|p| crate::projects::tilde(&p))
            .unwrap_or_default(),
        ("project.rename", "name") => doc
            .meta
            .path
            .as_deref()
            .and_then(crate::projects::current_name)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// The folder a document's relative paths start from: a file manager
/// listing's folder, else the folder of the document's file.
pub fn folder_of(doc: &DocumentState) -> Option<std::path::PathBuf> {
    if let Some(d) = doc.dired.as_deref().and_then(|s| s.dir()) {
        return Some(d.to_path_buf());
    }
    doc.meta
        .path
        .as_deref()
        .and_then(|p| std::path::absolute(p).ok())
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
}

/// An argument the user typed, as the JSON type `ty`: numbers for
/// `integer`, `y`, `yes`, `true` or `t` for `boolean`, words separated by
/// blanks or colons for `array`.
pub fn parse_argument(name: &str, ty: &str, input: &str) -> Result<Value, String> {
    Ok(match ty {
        "integer" => Value::from(
            input
                .trim()
                .parse::<i64>()
                .map_err(|_| crate::tr!("msg-argument-number", name = name))?,
        ),
        "boolean" => Value::Bool(matches!(input.trim(), "y" | "yes" | "true" | "t")),
        "array" => Value::Array(
            input
                .split([' ', ':'])
                .filter(|s| !s.is_empty())
                .map(Value::from)
                .collect(),
        ),
        _ => Value::from(input),
    })
}

/// `args` with `name` set to `value` (an object if it was not one).
pub fn with_argument(mut args: Value, name: &str, value: Value) -> Value {
    if !args.is_object() {
        args = Value::Object(serde_json::Map::new());
    }
    args[name] = value;
    args
}

impl CommandRegistry {
    /// An empty registry.
    pub fn new() -> CommandRegistry {
        CommandRegistry::default()
    }

    /// A registry with the built-in commands.
    pub fn with_builtins() -> CommandRegistry {
        let mut r = CommandRegistry::new();
        for mut c in crate::builtin::commands() {
            if c.scope.is_none() {
                c.scope = Some(crate::builtin::default_scope(&c));
            }
            r.register(c).expect("built-in commands are valid");
        }
        r
    }

    /// Adds a command. IDs must follow the convention and be new.
    pub fn register(&mut self, mut command: Command) -> Result<(), CommandError> {
        let Some(scope) = &command.scope else {
            return Err(CommandError::new(format!(
                "Command `{}` has no scope",
                command.id
            )));
        };
        // The scope as part of the when-clause.
        if let Some(c) = scope.clause() {
            command.when = Some(match command.when.take() {
                Some(w) => WhenClause::And(Box::new(c), Box::new(w)),
                None => c,
            });
        }
        if !valid_id(&command.id) {
            return Err(CommandError::new(format!(
                "Invalid command ID `{}`",
                command.id
            )));
        }
        if self.commands.contains_key(&command.id) {
            return Err(CommandError::new(format!(
                "Command `{}` is already registered",
                command.id
            )));
        }
        self.commands.insert(command.id.clone(), command);
        Ok(())
    }

    /// Removes a command (when its plugin unloads).
    pub fn unregister(&mut self, id: &str) -> Option<Command> {
        self.commands.remove(id)
    }

    /// A command by ID.
    pub fn get(&self, id: &str) -> Option<&Command> {
        self.commands.get(id)
    }

    /// Whether command `id` is offered for a document with context `doc`
    /// (`DocumentState::document_context`): its when-clause, which takes in
    /// its scope, can hold there, whatever the cursor is on. Menus and
    /// toolbars leave out the commands that are not; an unknown command is
    /// not.
    pub fn offered(&self, id: &str, doc: &crate::when::Context) -> bool {
        self.get(id)
            .is_some_and(|c| c.when.as_ref().is_none_or(|w| w.possible(doc)))
    }

    /// Every command, by ID.
    pub fn commands(&self) -> impl Iterator<Item = &Command> {
        self.commands.values()
    }

    /// Runs a command.
    pub fn execute(&self, id: &str, ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
        let Some(c) = self.commands.get(id) else {
            return Err(CommandError::new(format!("Unknown command `{id}`")));
        };
        match &c.handler {
            CommandHandler::Native(f) => f(ctx, args),
            CommandHandler::Script(_) => {
                Err(CommandError::new("Script commands are not available yet"))
            }
        }
    }
}

impl<'a> EditorContext<'a> {
    /// A context without messages or requests.
    pub fn new(
        document: Option<&'a mut DocumentState>,
        clipboard: &'a mut Clipboard,
        config: &'a crate::settings::Config,
        now: Instant,
        clock: DateTime,
    ) -> EditorContext<'a> {
        EditorContext {
            document,
            clipboard,
            config,
            now,
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        }
    }
}

impl EditorContext<'_> {
    /// The active document, or an error.
    pub fn doc(&mut self) -> Result<&mut DocumentState, CommandError> {
        self.document
            .as_deref_mut()
            .ok_or_else(|| CommandError::new("No document"))
    }

    /// Runs an `org-edit` command on the active Org document.
    pub fn org(
        &mut self,
        command: impl FnOnce(&Document, usize, Option<usize>) -> Result<Transaction, EditError>,
    ) -> CommandResult {
        let now = self.now;
        self.doc()?.run(now, command).map_err(CommandError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_name_for_each_language() {
        for (name, t) in [
            ("py", "python"),
            ("Python", "python"),
            ("js", "javascript"),
            ("sh", "shell"),
            ("md", "markdown"),
            ("h", "c"),
            ("go", "go"),
        ] {
            assert_eq!(canonical_type(name), t, "{name}");
            assert!(known_text_type(t), "{t}");
        }
    }

    #[test]
    fn scratch_documents() {
        let Some(own) = scratch_path(None) else {
            return;
        };
        assert!(own.ends_with("scratch/scratch.klm"), "{}", own.display());
        let a = scratch_path(Some(std::path::Path::new("/w/notes"))).unwrap();
        let b = scratch_path(Some(std::path::Path::new("/x/notes"))).unwrap();
        assert!(
            a.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("notes-")
        );
        assert_ne!(a, b);
        assert_eq!(a.parent(), own.parent());
    }
}
