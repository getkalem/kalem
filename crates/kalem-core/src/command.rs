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
    /// Open find, or find and replace.
    Find {
        /// With a replacement field.
        replace: bool,
    },
    /// Show or hide the outline panel.
    Outline,
    /// Switch between the rich view and the source view.
    ToggleSource,
    /// Show a second view of the document beside the first (the source
    /// beside the rich view), or close it.
    Split,
    /// Show the settings.
    Settings,
    /// Show only the section holding the cursor, or everything again.
    Focus,
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
    /// Show or hide the list of open files.
    OpenFiles,
    /// Change the project list, or act on the project's documents.
    Project(ProjectRequest),
    /// Show the file manager.
    FileManager(FileManagerRequest),
    /// Run a file operation in the background, after the questions it
    /// needs (`crate::dired::Task`).
    FileOp(FileOp),
    /// Stop the file operations that are running.
    CancelFileOps,
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
    /// The user's `init.js`.
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
        ("table.setFormula", "formula") => {
            let mut cache = crate::formulas::FormulaCache::default();
            crate::formulas::prompt(cache.get(doc))
        }
        // The document's folder, to add as a project or open a file from.
        ("project.add", "path") | ("file.open", "path") => {
            let dir = doc
                .meta
                .path
                .as_deref()
                .and_then(|p| std::path::absolute(p).ok())
                .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
                .or_else(|| std::env::current_dir().ok());
            dir.map(|d| {
                let mut s = d.display().to_string();
                if name == "path" && id == "file.open" && !s.ends_with(std::path::MAIN_SEPARATOR) {
                    s.push(std::path::MAIN_SEPARATOR);
                }
                s
            })
            .unwrap_or_default()
        }
        ("project.rename", "name") => doc
            .meta
            .path
            .as_deref()
            .and_then(crate::projects::current_name)
            .unwrap_or_default(),
        _ => String::new(),
    }
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
        for c in crate::builtin::commands() {
            r.register(c).expect("built-in commands are valid");
        }
        r
    }

    /// Adds a command. IDs must follow the convention and be new.
    pub fn register(&mut self, command: Command) -> Result<(), CommandError> {
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
