//! The host's side of the `extension` world (design §11.2 to §11.4): a
//! plugin's commands, key bindings and subscriptions, and what it shows
//! (notifications, questions, status bar items, panels), generated from
//! `kalem-plugin`'s WIT files (D6).
//!
//! The editor is reached through [`Editor`], which the embedder implements
//! over its command registry and keymap. The host keeps what each plugin
//! registered, so that a disposal or the plugin's deactivation takes back
//! exactly that, and checks what is the plugin's to decide: a command's
//! ID is the plugin's own (`pluginId.action`) and its scope names a type
//! (§11.2). The rest (the ID's form, a new ID, the keys, the
//! when-clauses) is the editor's, as for its own commands. A panel's
//! widget tree is checked here too ([`check_tree`]), so the editors render
//! only trees.

use std::collections::{BTreeMap, BTreeSet};

use wasmtime::component::{HasSelf, Resource, ResourceTable};

mod bindings {
    // `extension-layer-4`: the `extension` world and the `layer` (API
    // 0.2.10), `completer` (API 0.2.11), `layer-2` (API 0.2.13), `layer-3`
    // (API 0.2.15), `formatter` (API 0.2.16), `outline`, `focus` and `layer-4`
    // (API 0.2.18) interfaces it may export, bound one by one (`Extension::new`).
    wasmtime::component::bindgen!({
        path: "../kalem-plugin/wit",
        world: "extension-layer-4",
        with: {
            "kalem:plugin/kalem.disposable": super::Registration,
        },
    });
}

pub use api::{CommandSpec, Event, EventKind, Reply, Scope};
pub use bindings::exports::kalem::plugin::{
    completer, focus, formatter, layer, layer_2, layer_3, layer_4, outline,
};
pub use bindings::kalem::plugin::kalem as api;
pub use bindings::kalem::plugin::ui;
pub use bindings::kalem::plugin::{
    canvas_view, clock, clock_2, decorations, diagnostics, documents, editor, fs, fs_2, graph_view,
    http, later, layers, net, problems, process, settings, state, styled_documents,
};

/// An edit a plugin asked for, applied when its command returns, its
/// places those of the document as the command found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// `text` at `at`, or at the cursor.
    Insert {
        /// Where.
        at: Option<u64>,
        /// What.
        text: String,
    },
    /// `start..end` replaced by `text`.
    Replace {
        /// From.
        start: u64,
        /// To.
        end: u64,
        /// By.
        text: String,
    },
    /// A headline's TODO keyword, `None` removing it.
    SetTodo {
        /// The headline's start.
        headline: u64,
        /// The keyword.
        state: Option<String>,
    },
    /// A headline's title.
    SetTitle {
        /// The headline's start.
        headline: u64,
        /// The title.
        title: String,
    },
    /// A headline's own tags.
    SetTags {
        /// The headline's start.
        headline: u64,
        /// The tags.
        tags: Vec<String>,
    },
    /// A headline's property, `None` removing it.
    SetProperty {
        /// The headline's start.
        headline: u64,
        /// The property.
        key: String,
        /// Its value.
        value: Option<String>,
    },
    /// A headline promoted with its subtree.
    Promote(u64),
    /// A headline demoted with its subtree.
    Demote(u64),
    /// A headline moved above its sibling with its subtree.
    MoveUp(u64),
    /// A headline moved below its sibling with its subtree.
    MoveDown(u64),
    /// A field of a table's data row, from 0.
    SetCell {
        /// The table's start.
        table: u64,
        /// The data row.
        row: u32,
        /// The field.
        col: u32,
        /// Its text.
        value: String,
    },
    /// A table's formulas recalculated.
    Recalc(u64),
    /// The undo step's name.
    Label(String),
    /// The document saved after the edits.
    Save,
}

/// The document a plugin's command runs in, as the embedder offers it:
/// read as the command found it; the edits wait for its end.
pub trait DocumentAccess {
    /// What the document is.
    fn info(&self) -> editor::DocumentInfo;
    /// Its selection.
    fn selection(&self) -> editor::Selection;
    /// Its text, or a range of it (clamped to the text).
    fn text(&self, range: Option<(u64, u64)>) -> String;
    /// Its headlines, in order; none outside Org.
    fn headlines(&self) -> Vec<editor::Headline>;
    /// The text under headline `start`'s line, without its subheadlines.
    fn body(&self, start: u64) -> Option<String>;
    /// The TODO keywords in force.
    fn todo_keywords(&self) -> Vec<String>;
    /// Its `#+KEY: value` lines.
    fn keywords(&self) -> Vec<(String, String)>;
    /// The innermost element at `offset`.
    fn node_at(&self, offset: u64) -> Option<editor::Node>;
    /// The table holding `offset`.
    fn table_at(&self, offset: u64) -> Option<editor::Table>;
    /// Queues `edit`.
    fn edit(&mut self, edit: Edit);
    /// Its number when a plugin writes it (`documents`).
    fn generated(&self) -> Option<u64> {
        None
    }
}
pub use ui::{
    Answer, Level, PanelEvent, PanelSpec, PickItem, PickOptions, PromptOptions, StatusOptions,
    WidgetKind, WidgetTree,
};

/// The most a file read through `fs`, or a response, holds.
pub const MAX_BYTES: usize = 16 << 20;

/// What a plugin's manifest permits (§11.6), from its `permissions`:
/// `fs:read:workspace`, `fs:write:workspace`, `fs:read:all`,
/// `net:fetch:DOMAIN` (`*` for any), `subprocess:PROGRAM` (a bare name).
/// The `fs`, `net` and `process` interfaces are granted only with one of
/// theirs. A bare `subprocess` is a language plugin's: the core runs its
/// servers, and it grants no interface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    /// Read in the projects' folders.
    pub read_workspace: bool,
    /// Read and write in the projects' folders.
    pub write_workspace: bool,
    /// Read anywhere.
    pub read_all: bool,
    /// The domains fetched from.
    pub domains: Vec<String>,
    /// The programs run, by name (`subprocess:git`).
    pub programs: Vec<String>,
}

/// The most runs of `process` a plugin has at a time.
pub const MAX_RUNS: usize = 16;

/// Whether `name` may be a granted program's: a bare name, no path.
pub fn program_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Grants {
    /// The grants of a manifest's permissions; unknown ones grant nothing.
    pub fn from_permissions<S: AsRef<str>>(permissions: &[S]) -> Grants {
        let mut g = Grants::default();
        for p in permissions {
            match p.as_ref() {
                "fs:read:workspace" => g.read_workspace = true,
                "fs:write:workspace" => g.write_workspace = true,
                "fs:read:all" => g.read_all = true,
                p => {
                    if let Some(d) = p.strip_prefix("net:fetch:")
                        && !d.is_empty()
                    {
                        g.domains.push(d.to_ascii_lowercase());
                    } else if let Some(name) = p.strip_prefix("subprocess:")
                        && program_name(name)
                    {
                        g.programs.push(name.to_string());
                    }
                }
            }
        }
        g
    }

    /// Whether the `fs` interface is granted.
    pub fn fs(&self) -> bool {
        self.read_workspace || self.write_workspace || self.read_all
    }

    /// Whether the `net` interface is granted.
    pub fn net(&self) -> bool {
        !self.domains.is_empty()
    }

    /// Whether the `process` interface is granted.
    pub fn process(&self) -> bool {
        !self.programs.is_empty()
    }

    /// Whether `url` (`http` or `https`) is on a granted domain or under
    /// one.
    pub fn allows_url(&self, url: &str) -> bool {
        let Some(rest) = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
        else {
            return false;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host_port = authority.rsplit('@').next().unwrap_or("");
        let host = host_port
            .split(':')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        !host.is_empty()
            && self
                .domains
                .iter()
                .any(|d| d == "*" || host == *d || host.ends_with(&format!(".{d}")))
    }
}

/// `path` as it is on disk: absolute, its links followed for the part
/// that exists, no `..` in it.
fn real_path(path: &str) -> Result<std::path::PathBuf, String> {
    use std::path::{Component, Path};
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err(format!("`{path}` is not an absolute path"));
    }
    // In a Windows verbatim path (`\\?\C:\…`, what `canonicalize`
    // gives) `..` is not parsed as a parent but as a name, and `/` is no
    // separator (`\\?\C:\a\../b` has a part `../b`): every segment
    // between either separator is checked.
    if p.components().any(|c| matches!(c, Component::ParentDir))
        || path.split(['/', '\\']).any(|seg| seg == "..")
    {
        return Err(format!("`{path}` goes up with `..`"));
    }
    // The longest part that exists, followed through its links, and the
    // rest as written.
    let mut existing = p.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(n), Some(parent)) => {
                rest.push(n.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut real = existing
        .canonicalize()
        .map_err(|e| format!("{path}: {e}"))?;
    for n in rest.into_iter().rev() {
        real.push(n);
    }
    Ok(real)
}

/// The most widgets a panel's tree holds.
pub const MAX_WIDGETS: usize = 10_000;

/// A question a plugin asks the user.
#[derive(Debug, Clone)]
pub enum Question {
    /// A line of text; answered with [`Answer::Text`].
    Prompt {
        /// What is asked.
        title: String,
        /// The text it starts with, and how it shows.
        options: PromptOptions,
    },
    /// Yes or no; answered with [`Answer::Confirmed`].
    Confirm(String),
    /// A choice from a list; answered with [`Answer::Picked`].
    Pick {
        /// The entries.
        items: Vec<PickItem>,
        /// How they are offered.
        options: PickOptions,
    },
}

/// The API's version, as `kalem.version` gives it.
pub const VERSION: &str = "0.1.0";

/// The editor as a plugin reaches it.
pub trait Editor: Send + 'static {
    /// Adds command `spec` of plugin `plugin`, run by the plugin's
    /// `run-command`; refused as the registry refuses one.
    fn add_command(&mut self, plugin: &str, spec: &CommandSpec) -> Result<(), String>;

    /// Removes a command added.
    fn remove_command(&mut self, id: &str);

    /// Binds `keys` to `command` when `when` holds, as binding `id`.
    fn add_binding(
        &mut self,
        id: u64,
        keys: &str,
        command: &str,
        when: Option<&str>,
    ) -> Result<(), String>;

    /// Removes binding `id`.
    fn remove_binding(&mut self, id: u64);

    /// Runs command `id` with `args` as JSON once the plugin's call
    /// returns; refused when there is no such command.
    fn run(&mut self, id: &str, args: &str) -> Result<(), String>;

    /// Shows `message` from plugin `plugin`.
    fn notify(&mut self, plugin: &str, message: &str, level: Level);

    /// Asks the user `question`; the answer goes to
    /// [`Extension::answer`] with `request`.
    fn ask(&mut self, plugin: &str, request: u64, question: Question);

    /// Closes question `request` unanswered (its plugin is going).
    fn withdraw(&mut self, request: u64);

    /// Shows plugin `plugin`'s status bar item `id`, or changes it.
    fn set_status(&mut self, plugin: &str, id: &str, text: &str, options: &StatusOptions);

    /// Takes plugin `plugin`'s status bar item `id` away.
    fn remove_status(&mut self, plugin: &str, id: &str);

    /// Adds panel `spec`, empty; what the user does in it goes to
    /// [`Extension::panel_event`].
    fn add_panel(&mut self, plugin: &str, spec: &PanelSpec) -> Result<(), String>;

    /// Replaces panel `id`'s content with `tree`, checked by
    /// [`check_tree`].
    fn set_panel(&mut self, id: &str, tree: &WidgetTree);

    /// Removes panel `id`.
    fn remove_panel(&mut self, id: &str);

    /// Kalem's setting `key` as JSON.
    fn setting(&mut self, key: &str) -> Option<String>;

    /// Plugin `plugin`'s own setting `key` as JSON.
    fn own_setting(&mut self, plugin: &str, key: &str) -> Option<String>;

    /// Sets plugin `plugin`'s own setting `key` to `value` (JSON, `null`
    /// removing it) in the user's settings.
    fn set_own_setting(&mut self, plugin: &str, key: &str, value: &str) -> Result<(), String>;

    /// The folders of the projects, where `fs:*:workspace` reaches.
    fn workspace(&mut self) -> Vec<std::path::PathBuf>;

    /// Plugin `plugin` moved the file or folder `from` to `to` (`fs-2`,
    /// API 0.2.12): open documents follow it.
    fn moved(&mut self, _plugin: &str, _from: &std::path::Path, _to: &std::path::Path) {}

    /// Moves `path` to the system's trash for plugin `plugin` (`fs-2`,
    /// API 0.2.12), open documents of it closed unless they have unsaved
    /// changes.
    fn trash(&mut self, _plugin: &str, path: &std::path::Path) -> Result<(), String> {
        Err(format!("{} cannot go to the trash here", path.display()))
    }

    /// What plugin `plugin`'s layers show changed: the documents they
    /// serve ask again (`layers.refresh`, API 0.2.10).
    fn refresh_layers(&mut self, _plugin: &str) {}

    /// Sends `request` (its URL granted) for plugin `plugin`; the response
    /// goes to [`Extension::respond`] with `id`.
    fn fetch(&mut self, plugin: &str, id: u64, request: http::Request);

    /// Starts `command` for plugin `plugin` (its program granted, `cwd`
    /// checked to be in a project); how it ends goes to
    /// [`Extension::process_done`] with `run`.
    fn spawn(&mut self, plugin: &str, run: u64, cwd: std::path::PathBuf, command: process::Command);

    /// Stops run `run`, started by [`Editor::spawn`].
    fn kill(&mut self, run: u64);

    /// Shows plugin `plugin`'s document `spec` with `text`, or writes again
    /// the one of the same ID and key; its number. Refused as the editor
    /// refuses one (a kind not the plugin's).
    fn open_document(
        &mut self,
        plugin: &str,
        spec: documents::DocumentSpec,
        text: String,
        cursor: Option<u64>,
        styles: Vec<styled_documents::StyledSpan>,
    ) -> Result<u64, String>;

    /// Writes plugin `plugin`'s document `doc` anew.
    fn set_document(
        &mut self,
        plugin: &str,
        doc: u64,
        text: String,
        cursor: Option<u64>,
        styles: Vec<styled_documents::StyledSpan>,
    ) -> Result<(), String>;

    /// Closes plugin `plugin`'s document `doc`.
    fn close_document(&mut self, plugin: &str, doc: u64);

    /// Shows plugin `plugin`'s document `spec` drawn as graph `g`, `text`
    /// its source (`graph-view`, API 0.2.12); its number.
    fn open_graph(
        &mut self,
        _plugin: &str,
        _spec: documents::DocumentSpec,
        _g: graph_view::Graph,
        _text: String,
    ) -> Result<u64, String> {
        Err("This editor draws no graphs".into())
    }

    /// Writes plugin `plugin`'s graph document `doc` anew.
    fn set_graph(
        &mut self,
        _plugin: &str,
        _doc: u64,
        _g: graph_view::Graph,
        _text: String,
    ) -> Result<(), String> {
        Err("This editor draws no graphs".into())
    }

    /// Shows plugin `plugin`'s document `spec` drawn as canvas `c`, `text`
    /// its source (`canvas-view`, API 0.2.16); its number.
    fn open_canvas(
        &mut self,
        _plugin: &str,
        _spec: documents::DocumentSpec,
        _c: canvas_view::Canvas,
        _text: String,
    ) -> Result<u64, String> {
        Err("This editor draws no canvases".into())
    }

    /// Writes plugin `plugin`'s canvas document `doc` anew.
    fn set_canvas(
        &mut self,
        _plugin: &str,
        _doc: u64,
        _c: canvas_view::Canvas,
        _text: String,
    ) -> Result<(), String> {
        Err("This editor draws no canvases".into())
    }

    /// The value plugin `plugin` keeps under `key` (`state`, API 0.2.17).
    fn state_get(&mut self, _plugin: &str, _key: &str) -> Option<String> {
        None
    }

    /// Keeps `value` under `key` for plugin `plugin`, or forgets it.
    fn state_set(&mut self, _plugin: &str, _key: &str, _value: Option<&str>) -> Result<(), String> {
        Err("This editor keeps no plugin state".into())
    }

    /// The keys plugin `plugin` keeps values under.
    fn state_keys(&mut self, _plugin: &str) -> Vec<String> {
        Vec::new()
    }

    /// Runs plugin `plugin`'s command `id` with `args` as JSON once
    /// `after_ms` milliseconds have passed, at an editor's tick (`later`,
    /// API 0.2.17); its number.
    fn later(
        &mut self,
        _plugin: &str,
        _id: &str,
        _args: &str,
        _after_ms: u32,
    ) -> Result<u64, String> {
        Err("This editor runs nothing later".into())
    }

    /// Drops plugin `plugin`'s run `run` if it has not run yet.
    fn cancel_later(&mut self, _plugin: &str, _run: u64) {}

    /// Plugin `plugin` marks the lines of the file at `path` (lines from
    /// 1), in place of its earlier marks there.
    fn set_gutter(
        &mut self,
        plugin: &str,
        path: std::path::PathBuf,
        marks: Vec<decorations::LineMark>,
    ) -> Result<(), String>;

    /// Plugin `plugin` takes its marks away from the file at `path`, or
    /// from every file.
    fn clear_gutter(&mut self, plugin: &str, path: Option<std::path::PathBuf>);

    /// Plugin `plugin` sets the problems of the file at `path` (API
    /// 0.2.14), in place of its earlier ones there.
    fn set_problems(
        &mut self,
        _plugin: &str,
        _path: std::path::PathBuf,
        _problems: Vec<problems::Problem>,
    ) -> Result<(), String> {
        Err("This editor shows no problems".into())
    }

    /// Plugin `plugin` takes its problems away from the file at `path`,
    /// or from every file.
    fn clear_problems(&mut self, _plugin: &str, _path: Option<std::path::PathBuf>) {}

    /// The document the plugin's command runs in; `None` outside one.
    fn document(&mut self) -> Option<Box<dyn DocumentAccess + '_>>;
}

/// A plugin's registration, behind its `disposable` handle.
#[derive(Debug)]
pub struct Registration {
    id: u64,
}

/// What a registration is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum What {
    Command(String),
    Binding,
    Subscription(EventKind),
    Status(String),
    Panel(String),
    Watch(String, bool),
}

/// What an extension plugin's store holds: the editor, and what the
/// plugin registered.
pub struct Session {
    plugin: String,
    editor: Box<dyn Editor>,
    table: ResourceTable,
    next: u64,
    registered: BTreeMap<u64, What>,
    /// The questions not yet answered.
    asked: BTreeSet<u64>,
    grants: Grants,
    /// The requests not yet answered.
    fetching: BTreeSet<u64>,
    /// The runs not yet ended.
    running: BTreeSet<u64>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("plugin", &self.plugin)
            .field("registered", &self.registered)
            .finish_non_exhaustive()
    }
}

impl Session {
    fn register(&mut self, what: What) -> wasmtime::Result<Resource<Registration>> {
        self.next += 1;
        let id = self.next;
        self.registered.insert(id, what);
        Ok(self.table.push(Registration { id })?)
    }

    /// Takes registration `id` back from the editor.
    fn dispose(&mut self, id: u64) {
        match self.registered.remove(&id) {
            Some(What::Command(c)) => self.editor.remove_command(&c),
            Some(What::Binding) => self.editor.remove_binding(id),
            Some(What::Status(s)) => self.editor.remove_status(&self.plugin, &s),
            Some(What::Panel(p)) => self.editor.remove_panel(&p),
            Some(What::Subscription(_) | What::Watch(..)) | None => {}
        }
    }

    /// Whether `id` is the plugin's by its form: `pluginId.name`.
    fn named_own(&self, id: &str) -> bool {
        id.strip_prefix(&self.plugin)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|name| !name.is_empty())
    }

    /// The registration that is `what`, if the plugin made it.
    fn find(&self, what: &What) -> Option<u64> {
        self.registered
            .iter()
            .find(|(_, w)| *w == what)
            .map(|(id, _)| *id)
    }

    fn handle(&mut self, id: u64) -> Result<Resource<Registration>, String> {
        self.table
            .push(Registration { id })
            .map_err(|e| e.to_string())
    }

    fn ask(&mut self, question: Question) -> u64 {
        self.next += 1;
        let request = self.next;
        self.asked.insert(request);
        self.editor.ask(&self.plugin, request, question);
        request
    }

    /// Whether the plugin registered command `id`.
    fn owns(&self, id: &str) -> bool {
        self.registered
            .values()
            .any(|w| matches!(w, What::Command(c) if c == id))
    }
}

impl api::Host for Session {
    fn version(&mut self) -> String {
        VERSION.into()
    }

    fn command(&mut self, spec: CommandSpec) -> Result<Resource<Registration>, String> {
        if !self.named_own(&spec.id) {
            return Err(format!(
                "Command `{}` is not the plugin's: its ID starts `{}.`",
                spec.id, self.plugin
            ));
        }
        if spec.scope.types.as_ref().is_some_and(Vec::is_empty) {
            return Err(format!(
                "Command `{}` has no scope: name its types, or all",
                spec.id
            ));
        }
        if self.owns(&spec.id) {
            return Err(format!("Command `{}` is already registered", spec.id));
        }
        self.editor.add_command(&self.plugin, &spec)?;
        self.register(What::Command(spec.id))
            .map_err(|e| e.to_string())
    }

    fn run(&mut self, id: String, args: String) -> Result<(), String> {
        // The command is queued, run once the plugin's call returns (the
        // editors' `Editor::run`), so a plugin's own command does not enter
        // the plugin again: the way it edits the document after a
        // question's answer, as its commands alone reach the editor.
        self.editor.run(&id, &args)
    }

    fn keymap(
        &mut self,
        keys: String,
        command: String,
        when: Option<String>,
    ) -> Result<Resource<Registration>, String> {
        let id = self.next + 1;
        self.editor
            .add_binding(id, &keys, &command, when.as_deref())?;
        self.register(What::Binding).map_err(|e| e.to_string())
    }

    fn on(&mut self, kind: EventKind) -> Result<Resource<Registration>, String> {
        self.register(What::Subscription(kind))
            .map_err(|e| e.to_string())
    }
}

impl api::HostDisposable for Session {
    fn id(&mut self, r: Resource<Registration>) -> u64 {
        self.table.get(&r).map_or(0, |r| r.id)
    }

    fn dispose(&mut self, r: Resource<Registration>) {
        if let Ok(id) = self.table.get(&r).map(|r| r.id) {
            Session::dispose(self, id);
        }
    }

    /// The handle goes, the registration stays.
    fn drop(&mut self, r: Resource<Registration>) -> wasmtime::Result<()> {
        self.table.delete(r)?;
        Ok(())
    }
}

impl ui::Host for Session {
    fn notify(&mut self, message: String, level: Level) {
        self.editor.notify(&self.plugin, &message, level);
    }

    fn prompt(&mut self, title: String, options: PromptOptions) -> u64 {
        self.ask(Question::Prompt { title, options })
    }

    fn confirm(&mut self, message: String) -> u64 {
        self.ask(Question::Confirm(message))
    }

    fn quick_pick(&mut self, items: Vec<PickItem>, options: PickOptions) -> u64 {
        self.ask(Question::Pick { items, options })
    }

    fn status(
        &mut self,
        id: String,
        text: String,
        options: StatusOptions,
    ) -> Result<Resource<Registration>, String> {
        if id.is_empty() {
            return Err("A status bar item needs an ID".into());
        }
        self.editor.set_status(&self.plugin, &id, &text, &options);
        // Set again, the same item changes: a handle to it.
        let what = What::Status(id);
        match self.find(&what) {
            Some(r) => self.handle(r),
            None => self.register(what).map_err(|e| e.to_string()),
        }
    }

    fn register_panel(&mut self, spec: PanelSpec) -> Result<Resource<Registration>, String> {
        if !self.named_own(&spec.id) {
            return Err(format!(
                "Panel `{}` is not the plugin's: its ID starts `{}.`",
                spec.id, self.plugin
            ));
        }
        if self.find(&What::Panel(spec.id.clone())).is_some() {
            return Err(format!("Panel `{}` is already registered", spec.id));
        }
        self.editor.add_panel(&self.plugin, &spec)?;
        self.register(What::Panel(spec.id))
            .map_err(|e| e.to_string())
    }

    fn set_panel(&mut self, id: String, tree: WidgetTree) -> Result<(), String> {
        if self.find(&What::Panel(id.clone())).is_none() {
            return Err(format!("No panel `{id}` of this plugin"));
        }
        check_tree(&tree)?;
        self.editor.set_panel(&id, &tree);
        Ok(())
    }
}

impl settings::Host for Session {
    fn get(&mut self, key: String) -> Option<String> {
        self.editor.setting(&key)
    }

    fn own(&mut self, key: String) -> Option<String> {
        self.editor.own_setting(&self.plugin, &key)
    }

    fn set(&mut self, key: String, value: String) -> Result<(), String> {
        if key.is_empty() || key.contains(['.', '"', '[', ']']) {
            return Err(format!("`{key}` is not a setting's name"));
        }
        check_value(&value)?;
        self.editor.set_own_setting(&self.plugin, &key, &value)
    }

    fn watch(&mut self, key: String, own: bool) -> Result<Resource<Registration>, String> {
        self.register(What::Watch(key, own))
            .map_err(|e| e.to_string())
    }
}

/// Whether `text` is JSON, without a JSON library: what the editor parses
/// later reports the rest.
fn check_value(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("No value: `null` removes a setting".into());
    }
    Ok(())
}

impl Session {
    /// `path` when the plugin's grants reach it, for writing or reading.
    fn reach(&mut self, path: &str, write: bool) -> Result<std::path::PathBuf, String> {
        let real = real_path(path)?;
        if !write && self.grants.read_all {
            return Ok(real);
        }
        let granted = if write {
            self.grants.write_workspace
        } else {
            self.grants.read_workspace || self.grants.write_workspace
        };
        let inside = granted
            && self
                .editor
                .workspace()
                .iter()
                .filter_map(|r| r.canonicalize().ok())
                .any(|r| real.starts_with(r));
        if inside {
            Ok(real)
        } else {
            Err(format!(
                "`{path}` is outside what the plugin may {}",
                if write { "write" } else { "read" }
            ))
        }
    }
}

impl clock::Host for Session {
    fn now(&mut self) -> i64 {
        crate::time::now()
    }

    fn timezone(&mut self) -> String {
        crate::time::timezone()
    }

    fn random(&mut self) -> u64 {
        crate::time::random()
    }
}

impl clock_2::Host for Session {
    fn offset(&mut self, at: i64) -> i32 {
        crate::time::offset(at)
    }
}

impl layers::Host for Session {
    fn refresh(&mut self) {
        let plugin = self.plugin.clone();
        self.editor.refresh_layers(&plugin);
    }
}

impl fs::Host for Session {
    fn read(&mut self, path: String) -> Result<String, String> {
        let real = self.reach(&path, false)?;
        let len = std::fs::metadata(&real)
            .map_err(|e| format!("{path}: {e}"))?
            .len();
        if len > MAX_BYTES as u64 {
            return Err(format!("{path} is larger than {} MB", MAX_BYTES >> 20));
        }
        let bytes = std::fs::read(&real).map_err(|e| format!("{path}: {e}"))?;
        String::from_utf8(bytes).map_err(|_| format!("{path} is not UTF-8 text"))
    }

    fn write(&mut self, path: String, text: String) -> Result<(), String> {
        let real = self.reach(&path, true)?;
        if let Some(dir) = real.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{path}: {e}"))?;
        }
        std::fs::write(&real, text).map_err(|e| format!("{path}: {e}"))
    }

    fn list(&mut self, dir: String) -> Result<Vec<String>, String> {
        let real = self.reach(&dir, false)?;
        let mut out: Vec<String> = std::fs::read_dir(&real)
            .map_err(|e| format!("{dir}: {e}"))?
            .filter_map(Result::ok)
            .map(|e| {
                let p = e.path().to_string_lossy().into_owned();
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    format!("{p}/")
                } else {
                    p
                }
            })
            .collect();
        out.sort();
        Ok(out)
    }
}

impl fs_2::Host for Session {
    fn rename(&mut self, from: String, to: String) -> Result<(), String> {
        let real_from = self.reach(&from, true)?;
        let real_to = self.reach(&to, true)?;
        if !real_from.exists() {
            return Err(format!("{from}: no such file or folder"));
        }
        if real_to.exists() {
            return Err(format!("{to} exists already"));
        }
        if let Some(dir) = real_to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{to}: {e}"))?;
        }
        std::fs::rename(&real_from, &real_to).map_err(|e| format!("{from}: {e}"))?;
        let plugin = self.plugin.clone();
        self.editor.moved(&plugin, &real_from, &real_to);
        Ok(())
    }

    fn trash(&mut self, path: String) -> Result<(), String> {
        let real = self.reach(&path, true)?;
        if !real.exists() {
            return Err(format!("{path}: no such file or folder"));
        }
        let plugin = self.plugin.clone();
        self.editor.trash(&plugin, &real)
    }
}

impl net::Host for Session {
    fn fetch(&mut self, request: http::Request) -> Result<u64, String> {
        if !self.grants.allows_url(&request.url) {
            return Err(format!(
                "{} is not on a domain the plugin may fetch from",
                request.url
            ));
        }
        self.next += 1;
        let id = self.next;
        self.fetching.insert(id);
        self.editor.fetch(&self.plugin, id, request);
        Ok(id)
    }
}

/// Styled stretches of a plugin's document at most.
pub const MAX_STYLES: usize = 100_000;

impl Session {
    /// A plugin's document opened: its ID the plugin's, its text and its
    /// styles within their limits.
    fn open_styled(
        &mut self,
        spec: documents::DocumentSpec,
        text: String,
        cursor: Option<u64>,
        styles: Vec<styled_documents::StyledSpan>,
    ) -> Result<u64, String> {
        if !self.named_own(&spec.id) {
            return Err(format!(
                "Document `{}` is not the plugin's: its ID starts `{}.`",
                spec.id, self.plugin
            ));
        }
        check_document(&text, &styles)?;
        let plugin = self.plugin.clone();
        self.editor
            .open_document(&plugin, spec, text, cursor, styles)
    }

    fn set_styled(
        &mut self,
        doc: u64,
        text: String,
        cursor: Option<u64>,
        styles: Vec<styled_documents::StyledSpan>,
    ) -> Result<(), String> {
        check_document(&text, &styles)?;
        let plugin = self.plugin.clone();
        self.editor.set_document(&plugin, doc, text, cursor, styles)
    }
}

/// A document's text and styles within their limits.
fn check_document(text: &str, styles: &[styled_documents::StyledSpan]) -> Result<(), String> {
    if text.len() > MAX_BYTES {
        return Err(format!("A document holds at most {} MB", MAX_BYTES >> 20));
    }
    if styles.len() > MAX_STYLES {
        return Err(format!(
            "A document has at most {MAX_STYLES} styled stretches, not {}",
            styles.len()
        ));
    }
    Ok(())
}

impl documents::Host for Session {
    fn open(
        &mut self,
        spec: documents::DocumentSpec,
        text: String,
        cursor: Option<u64>,
    ) -> Result<u64, String> {
        self.open_styled(spec, text, cursor, Vec::new())
    }

    fn set(&mut self, doc: u64, text: String, cursor: Option<u64>) -> Result<(), String> {
        self.set_styled(doc, text, cursor, Vec::new())
    }

    fn current(&mut self) -> Option<u64> {
        self.editor.document()?.generated()
    }

    fn close(&mut self, doc: u64) {
        let plugin = self.plugin.clone();
        self.editor.close_document(&plugin, doc);
    }
}

impl state::Host for Session {
    fn get(&mut self, key: String) -> Option<String> {
        let plugin = self.plugin.clone();
        self.editor.state_get(&plugin, &key)
    }

    fn set(&mut self, key: String, value: Option<String>) -> Result<(), String> {
        let plugin = self.plugin.clone();
        self.editor.state_set(&plugin, &key, value.as_deref())
    }

    fn keys(&mut self) -> Vec<String> {
        let plugin = self.plugin.clone();
        self.editor.state_keys(&plugin)
    }
}

impl later::Host for Session {
    fn run(&mut self, id: String, args: String, after_ms: u32) -> Result<u64, String> {
        // Its own commands alone: another's may be gone when the run is
        // due, and was not written to come back on its own.
        if !self.owns(&id) {
            return Err(format!("`{id}` is not the plugin's own command"));
        }
        let plugin = self.plugin.clone();
        self.editor.later(&plugin, &id, &args, after_ms)
    }

    fn cancel(&mut self, run: u64) {
        let plugin = self.plugin.clone();
        self.editor.cancel_later(&plugin, run);
    }
}

/// Shapes and connectors a canvas has at most.
pub const MAX_CANVAS: usize = 200_000;

impl canvas_view::Host for Session {
    fn open(
        &mut self,
        spec: documents::DocumentSpec,
        c: canvas_view::Canvas,
        text: String,
    ) -> Result<u64, String> {
        if !self.named_own(&spec.id) {
            return Err(format!(
                "Document `{}` is not the plugin's: its ID starts `{}.`",
                spec.id, self.plugin
            ));
        }
        check_document(&text, &[])?;
        check_canvas(&c)?;
        let plugin = self.plugin.clone();
        self.editor.open_canvas(&plugin, spec, c, text)
    }

    fn set(&mut self, doc: u64, c: canvas_view::Canvas, text: String) -> Result<(), String> {
        check_document(&text, &[])?;
        check_canvas(&c)?;
        let plugin = self.plugin.clone();
        self.editor.set_canvas(&plugin, doc, c, text)
    }
}

/// A canvas within its limits: so many shapes and connectors, and points
/// of strokes in all.
fn check_canvas(c: &canvas_view::Canvas) -> Result<(), String> {
    let points: usize = c
        .shapes
        .iter()
        .map(|s| match &s.kind {
            canvas_view::ShapeKind::Stroke(p) => p.len(),
            _ => 0,
        })
        .sum();
    if c.shapes.len() > MAX_CANVAS || c.connectors.len() > MAX_CANVAS || points > 4 * MAX_CANVAS {
        return Err(format!(
            "A canvas has at most {MAX_CANVAS} shapes, {MAX_CANVAS} connectors and {} points of strokes",
            4 * MAX_CANVAS
        ));
    }
    Ok(())
}

/// Nodes and edges a graph has at most.
pub const MAX_GRAPH: usize = 200_000;

impl graph_view::Host for Session {
    fn open(
        &mut self,
        spec: documents::DocumentSpec,
        g: graph_view::Graph,
        text: String,
    ) -> Result<u64, String> {
        if !self.named_own(&spec.id) {
            return Err(format!(
                "Document `{}` is not the plugin's: its ID starts `{}.`",
                spec.id, self.plugin
            ));
        }
        check_document(&text, &[])?;
        check_graph(&g)?;
        let plugin = self.plugin.clone();
        self.editor.open_graph(&plugin, spec, g, text)
    }

    fn set(&mut self, doc: u64, g: graph_view::Graph, text: String) -> Result<(), String> {
        check_document(&text, &[])?;
        check_graph(&g)?;
        let plugin = self.plugin.clone();
        self.editor.set_graph(&plugin, doc, g, text)
    }
}

/// A graph within its limits.
fn check_graph(g: &graph_view::Graph) -> Result<(), String> {
    if g.nodes.len() > MAX_GRAPH || g.edges.len() > MAX_GRAPH {
        return Err(format!(
            "A graph has at most {MAX_GRAPH} nodes and {MAX_GRAPH} edges"
        ));
    }
    Ok(())
}

impl styled_documents::Host for Session {
    fn open(
        &mut self,
        spec: documents::DocumentSpec,
        text: String,
        cursor: Option<u64>,
        styles: Vec<styled_documents::StyledSpan>,
    ) -> Result<u64, String> {
        self.open_styled(spec, text, cursor, styles)
    }

    fn set(
        &mut self,
        doc: u64,
        text: String,
        cursor: Option<u64>,
        styles: Vec<styled_documents::StyledSpan>,
    ) -> Result<(), String> {
        self.set_styled(doc, text, cursor, styles)
    }
}

/// Marks a plugin sets in a file at most.
pub const MAX_MARKS: usize = 10_000;

impl decorations::Host for Session {
    fn set_gutter(
        &mut self,
        path: String,
        marks: Vec<decorations::LineMark>,
    ) -> Result<(), String> {
        let p = std::path::PathBuf::from(&path);
        if !p.is_absolute() {
            return Err(format!("`{path}` is not an absolute path"));
        }
        if marks.len() > MAX_MARKS {
            return Err(format!(
                "At most {MAX_MARKS} marks a file, not {}",
                marks.len()
            ));
        }
        let plugin = self.plugin.clone();
        self.editor.set_gutter(&plugin, p, marks)
    }

    fn clear_gutter(&mut self, path: Option<String>) {
        let plugin = self.plugin.clone();
        self.editor
            .clear_gutter(&plugin, path.map(std::path::PathBuf::from));
    }
}

impl problems::Host for Session {
    fn set(&mut self, path: String, list: Vec<problems::Problem>) -> Result<(), String> {
        let p = std::path::PathBuf::from(&path);
        if !p.is_absolute() {
            return Err(format!("`{path}` is not an absolute path"));
        }
        if list.len() > MAX_MARKS {
            return Err(format!(
                "At most {MAX_MARKS} problems a file, not {}",
                list.len()
            ));
        }
        let plugin = self.plugin.clone();
        self.editor.set_problems(&plugin, p, list)
    }

    fn clear(&mut self, path: Option<String>) {
        let plugin = self.plugin.clone();
        self.editor
            .clear_problems(&plugin, path.map(std::path::PathBuf::from));
    }
}

impl process::Host for Session {
    fn run(&mut self, command: process::Command) -> Result<u64, String> {
        if !self.grants.programs.contains(&command.program) {
            return Err(format!(
                "`{}` is not a program the plugin may run",
                command.program
            ));
        }
        if self.running.len() >= MAX_RUNS {
            return Err(format!(
                "{MAX_RUNS} programs of the plugin are running already"
            ));
        }
        let roots: Vec<std::path::PathBuf> = self
            .editor
            .workspace()
            .iter()
            .filter_map(|r| r.canonicalize().ok())
            .collect();
        let cwd = match &command.cwd {
            Some(c) => {
                let real = real_path(c)?;
                if !real.is_dir() {
                    return Err(format!("`{c}` is not a folder"));
                }
                if !roots.iter().any(|r| real.starts_with(r)) {
                    return Err(format!("`{c}` is outside the projects"));
                }
                real
            }
            None => roots
                .first()
                .cloned()
                .ok_or("No project: a program runs in a project's folder")?,
        };
        self.next += 1;
        let run = self.next;
        self.running.insert(run);
        self.editor.spawn(&self.plugin, run, cwd, command);
        Ok(run)
    }

    fn kill(&mut self, run: u64) {
        if self.running.contains(&run) {
            self.editor.kill(run);
        }
    }
}

/// A new `ID`: 128 bits from the clock and a counter, written as a UUID.
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = N.fetch_add(1, Ordering::Relaxed) as u128;
    // Mixed so that IDs made close together differ in every part.
    let mut x = t ^ (n.wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c835));
    x ^= x >> 67;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9_94d0_49bb_1331_11eb);
    // Version 4, variant 1.
    x = (x & !(0xf << 76)) | (0x4 << 76);
    x = (x & !(0x3 << 62)) | (0x2 << 62);
    let h = format!("{x:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

impl Session {
    /// The headline starting at `start`, or why there is none.
    fn headline(&mut self, start: u64) -> Result<editor::Headline, String> {
        let doc = self
            .editor
            .document()
            .ok_or("No document: the plugin's command is not running")?;
        doc.headlines()
            .into_iter()
            .find(|h| h.start == start)
            .ok_or_else(|| format!("No headline starts at {start}"))
    }

    /// Queues `edit` on the headline at `headline`, which must be one.
    fn edit_headline(&mut self, headline: u64, edit: Edit) -> Result<(), String> {
        self.headline(headline)?;
        self.queue(edit)
    }

    fn queue(&mut self, edit: Edit) -> Result<(), String> {
        let mut doc = self
            .editor
            .document()
            .ok_or("No document: the plugin's command is not running")?;
        doc.edit(edit);
        Ok(())
    }

    fn table(&mut self, start: u64) -> Result<(), String> {
        let doc = self.editor.document().ok_or("No document")?;
        doc.table_at(start)
            .filter(|t| t.start == start)
            .map(drop)
            .ok_or_else(|| format!("No table starts at {start}"))
    }
}

impl editor::Host for Session {
    fn document(&mut self) -> Option<editor::DocumentInfo> {
        self.editor.document().map(|d| d.info())
    }

    fn selected(&mut self) -> editor::Selection {
        self.editor
            .document()
            .map_or(editor::Selection { anchor: 0, head: 0 }, |d| d.selection())
    }

    fn text(&mut self, range: Option<api::Range>) -> String {
        self.editor
            .document()
            .map(|d| d.text(range.map(|r| (r.start, r.end))))
            .unwrap_or_default()
    }

    fn headlines(&mut self) -> Vec<editor::Headline> {
        self.editor
            .document()
            .map(|d| d.headlines())
            .unwrap_or_default()
    }

    fn headline_at(&mut self, offset: u64) -> Option<editor::Headline> {
        // The innermost: the last that starts before and still holds it.
        self.headlines()
            .into_iter()
            .rfind(|h| h.range.start <= offset && offset < h.range.end.max(h.range.start + 1))
    }

    fn headline_by_id(&mut self, id: String) -> Option<editor::Headline> {
        self.headlines().into_iter().find(|h| {
            h.properties.iter().any(|(k, v)| {
                (k.eq_ignore_ascii_case("ID") || k.eq_ignore_ascii_case("CUSTOM_ID")) && *v == id
            })
        })
    }

    fn find(&mut self, query: editor::Query) -> Vec<editor::Headline> {
        self.headlines()
            .into_iter()
            .filter(|h| query.tag.as_ref().is_none_or(|t| h.tags.contains(t)))
            .filter(|h| {
                query
                    .todo
                    .as_ref()
                    .is_none_or(|t| h.todo.as_ref() == Some(t))
            })
            .filter(|h| {
                query.property.as_ref().is_none_or(|(k, v)| {
                    h.properties
                        .iter()
                        .any(|(hk, hv)| hk.eq_ignore_ascii_case(k) && hv == v)
                })
            })
            .collect()
    }

    fn body(&mut self, headline: u64) -> Result<String, String> {
        self.headline(headline)?;
        self.editor
            .document()
            .and_then(|d| d.body(headline))
            .ok_or_else(|| format!("No headline starts at {headline}"))
    }

    fn todo_keywords(&mut self) -> Vec<String> {
        self.editor
            .document()
            .map(|d| d.todo_keywords())
            .unwrap_or_default()
    }

    fn keywords(&mut self) -> Vec<(String, String)> {
        self.editor
            .document()
            .map(|d| d.keywords())
            .unwrap_or_default()
    }

    fn node_at(&mut self, offset: u64) -> Option<editor::Node> {
        self.editor.document().and_then(|d| d.node_at(offset))
    }

    fn table_at(&mut self, offset: u64) -> Option<editor::Table> {
        self.editor.document().and_then(|d| d.table_at(offset))
    }

    fn id(&mut self, headline: u64) -> Result<String, String> {
        let h = self.headline(headline)?;
        if let Some((_, v)) = h
            .properties
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("ID"))
        {
            return Ok(v.clone());
        }
        let id = new_id();
        self.queue(Edit::SetProperty {
            headline,
            key: "ID".into(),
            value: Some(id.clone()),
        })?;
        Ok(id)
    }

    fn insert(&mut self, at: Option<u64>, text: String) {
        let _ = self.queue(Edit::Insert { at, text });
    }

    fn replace(&mut self, range: api::Range, text: String) -> Result<(), String> {
        let len = self.editor.document().map_or(0, |d| d.info().length);
        if range.start > range.end || range.end > len {
            return Err(format!(
                "{}..{} is not a range of the text ({len} bytes)",
                range.start, range.end
            ));
        }
        self.queue(Edit::Replace {
            start: range.start,
            end: range.end,
            text,
        })
    }

    fn set_todo(&mut self, headline: u64, state: Option<String>) -> Result<(), String> {
        self.edit_headline(headline, Edit::SetTodo { headline, state })
    }

    fn set_title(&mut self, headline: u64, title: String) -> Result<(), String> {
        if title.contains('\n') {
            return Err("A title is one line".into());
        }
        self.edit_headline(headline, Edit::SetTitle { headline, title })
    }

    fn set_tags(&mut self, headline: u64, tags: Vec<String>) -> Result<(), String> {
        if let Some(t) = tags
            .iter()
            .find(|t| t.is_empty() || t.contains(|c: char| c.is_whitespace() || c == ':'))
        {
            return Err(format!("`{t}` is not a tag"));
        }
        self.edit_headline(headline, Edit::SetTags { headline, tags })
    }

    fn set_property(
        &mut self,
        headline: u64,
        key: String,
        value: Option<String>,
    ) -> Result<(), String> {
        if key.is_empty() || key.contains(|c: char| c.is_whitespace() || c == ':') {
            return Err(format!("`{key}` is not a property's name"));
        }
        self.edit_headline(
            headline,
            Edit::SetProperty {
                headline,
                key,
                value,
            },
        )
    }

    fn promote(&mut self, headline: u64) -> Result<(), String> {
        self.edit_headline(headline, Edit::Promote(headline))
    }

    fn demote(&mut self, headline: u64) -> Result<(), String> {
        self.edit_headline(headline, Edit::Demote(headline))
    }

    fn move_up(&mut self, headline: u64) -> Result<(), String> {
        self.edit_headline(headline, Edit::MoveUp(headline))
    }

    fn move_down(&mut self, headline: u64) -> Result<(), String> {
        self.edit_headline(headline, Edit::MoveDown(headline))
    }

    fn set_cell(&mut self, table: u64, row: u32, col: u32, value: String) -> Result<(), String> {
        self.table(table)?;
        if value.contains(['\n', '|']) {
            return Err("A field holds no line break and no `|`".into());
        }
        self.queue(Edit::SetCell {
            table,
            row,
            col,
            value,
        })
    }

    fn recalc(&mut self, table: u64) -> Result<(), String> {
        self.table(table)?;
        self.queue(Edit::Recalc(table))
    }

    fn transact(&mut self, label: String) {
        let _ = self.queue(Edit::Label(label));
    }

    fn save(&mut self) {
        let _ = self.queue(Edit::Save);
    }
}

/// Whether `tree` is one, as the editors render it: widgets there are, at
/// most [`MAX_WIDGETS`], each child after its parent and under one parent
/// only, every widget but the root under one, children only in columns,
/// rows and items, the keys of the widgets the user acts on given and
/// unique, a progress between 0 and 1.
pub fn check_tree(tree: &WidgetTree) -> Result<(), String> {
    let w = &tree.widgets;
    if w.is_empty() {
        return Err("The tree has no widgets".into());
    }
    if w.len() > MAX_WIDGETS {
        return Err(format!(
            "The tree has {} widgets, more than {MAX_WIDGETS}",
            w.len()
        ));
    }
    let mut parent = vec![None; w.len()];
    let mut keys = BTreeSet::new();
    for (i, widget) in w.iter().enumerate() {
        let holds = matches!(
            widget.kind,
            WidgetKind::Column | WidgetKind::Row | WidgetKind::Item(_)
        );
        if !widget.children.is_empty() && !holds {
            return Err(format!("Widget {i} cannot hold others"));
        }
        for &c in &widget.children {
            let c = c as usize;
            if c <= i || c >= w.len() {
                return Err(format!(
                    "Widget {i} has child {c}, not after it in the tree"
                ));
            }
            if parent[c].replace(i).is_some() {
                return Err(format!("Widget {c} is under two widgets"));
            }
        }
        if i > 0 && parent[i].is_none() {
            return Err(format!("Widget {i} is under none"));
        }
        let acted_on = matches!(
            widget.kind,
            WidgetKind::Button(_)
                | WidgetKind::Input(_)
                | WidgetKind::Checkbox(_)
                | WidgetKind::Item(_)
        );
        if acted_on && (widget.key.is_empty() || !keys.insert(widget.key.as_str())) {
            return Err(format!(
                "Widget {i}'s key `{}` is empty or used twice",
                widget.key
            ));
        }
        if let WidgetKind::Progress(p) = &widget.kind
            && p.value.is_some_and(|v| !(0.0..=1.0).contains(&v))
        {
            return Err(format!("Widget {i}'s progress is not between 0 and 1"));
        }
    }
    Ok(())
}

/// The kind of `event`.
pub fn kind(event: &Event) -> EventKind {
    match event {
        Event::AppReady => EventKind::AppReady,
        Event::DocumentOpen(_) => EventKind::DocumentOpen,
        Event::DocumentClose(_) => EventKind::DocumentClose,
        Event::DocumentBeforeSave(_) => EventKind::DocumentBeforeSave,
        Event::DocumentAfterSave(_) => EventKind::DocumentAfterSave,
        Event::DocumentChanged(_) => EventKind::DocumentChanged,
        Event::SelectionChanged(_) => EventKind::SelectionChanged,
        Event::HeadlineTodoChanged(_) => EventKind::HeadlineTodoChanged,
        Event::HeadlineTagsChanged(_) => EventKind::HeadlineTagsChanged,
        Event::HeadlineScheduled(_) => EventKind::HeadlineScheduled,
        Event::TableBeforeRecalc(_) => EventKind::TableBeforeRecalc,
        Event::TableRecalculated(_) => EventKind::TableRecalculated,
        Event::BabelBeforeExecute(_) => EventKind::BabelBeforeExecute,
        Event::BabelAfterExecute(_) => EventKind::BabelAfterExecute,
        Event::ExportBefore(_) => EventKind::ExportBefore,
        Event::ExportAfter(_) => EventKind::ExportAfter,
        Event::WorkspaceFileChanged(_) => EventKind::WorkspaceFileChanged,
    }
}

/// An extension plugin instantiated for the editor: activated, its
/// commands run and its events delivered through the host's budget and
/// limits. After a call fails with an [`Error`](crate::Error) the instance
/// is spent; [`Extension::deactivate`] still takes back what it
/// registered.
pub struct Extension {
    instance: crate::Instance<Session>,
    api: bindings::exports::kalem::plugin::plugin::Guest,
    /// The `layer` interface, when the component exports it (API 0.2.10).
    layer: Option<layer::Guest>,
    /// The `layer-2` interface, when the component exports it (API
    /// 0.2.13): asked in place of `layer`.
    layer_2: Option<layer_2::Guest>,
    /// The `layer-3` interface, when the component exports it (API
    /// 0.2.15): asked in place of `layer-2` and `layer`.
    layer_3: Option<layer_3::Guest>,
    /// The `layer-4` interface, when the component exports it (API
    /// 0.2.18): asked in place of `layer-3` and those before it.
    layer_4: Option<layer_4::Guest>,
    /// The `completer` interface, when the component exports it (API
    /// 0.2.11).
    completer: Option<completer::Guest>,
    /// The `formatter` interface, when the component exports it (API
    /// 0.2.16).
    formatter: Option<formatter::Guest>,
    /// The `outline` interface, when the component exports it (API
    /// 0.2.18).
    outline: Option<outline::Guest>,
    /// The `focus` interface, when the component exports it (API 0.2.18).
    focus: Option<focus::Guest>,
}

impl std::fmt::Debug for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extension")
            .field("session", self.instance.data())
            .finish_non_exhaustive()
    }
}

impl Extension {
    /// Instantiates `plugin`, known as `id`, granted the `kalem`, `ui` and
    /// `settings` interfaces over `editor`, and `fs`, `net` and `process`
    /// when `grants` permit; a plugin importing what it was not granted is
    /// refused, the interface named.
    pub fn new(
        host: &crate::Host,
        plugin: &crate::Plugin,
        id: &str,
        editor: Box<dyn Editor>,
        grants: Grants,
        limits: crate::Limits,
    ) -> crate::Result<Extension> {
        let mut linker = host.linker::<Session>();
        diagnostics::add_to_linker::<_, HasSelf<crate::Diagnostics>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.diagnostics,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        api::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        ui::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        editor::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        settings::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        documents::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        // A plugin's problems in files (API 0.2.14), without a permission:
        // they are drawn, never written.
        problems::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        decorations::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        styled_documents::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        // What plugins keep between sessions (API 0.2.17), each its own.
        state::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        // Its own commands run later (API 0.2.17).
        later::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        // Documents drawn as canvases (API 0.2.16), as documents are.
        canvas_view::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        // Documents drawn as graphs (API 0.2.12), as documents are.
        graph_view::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        // The time and layers' refresh (API 0.2.10) and the local time
        // (0.2.11), without a permission.
        clock::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        clock_2::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        layers::add_to_linker::<_, HasSelf<Session>>(
            &mut linker,
            |d: &mut crate::Data<Session>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        if grants.fs() {
            fs::add_to_linker::<_, HasSelf<Session>>(
                &mut linker,
                |d: &mut crate::Data<Session>| &mut d.user,
            )
            .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
            // Moves and the trash (API 0.2.12), which write as `fs.write`.
            fs_2::add_to_linker::<_, HasSelf<Session>>(
                &mut linker,
                |d: &mut crate::Data<Session>| &mut d.user,
            )
            .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        }
        if grants.net() {
            net::add_to_linker::<_, HasSelf<Session>>(
                &mut linker,
                |d: &mut crate::Data<Session>| &mut d.user,
            )
            .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        }
        if grants.process() {
            process::add_to_linker::<_, HasSelf<Session>>(
                &mut linker,
                |d: &mut crate::Data<Session>| &mut d.user,
            )
            .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        }
        let session = Session {
            plugin: id.to_string(),
            editor,
            table: ResourceTable::new(),
            next: 0,
            registered: BTreeMap::new(),
            asked: BTreeSet::new(),
            grants,
            fetching: BTreeSet::new(),
            running: BTreeSet::new(),
        };
        let mut instance = plugin.instantiate(host, &linker, session, limits)?;
        // `plugin` alone, not the whole world: a component of an earlier
        // 0.2.x exports no `layer`, one of now may (as the viewers' extra
        // interfaces are bound).
        let api = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            bindings::exports::kalem::plugin::plugin::GuestIndices::new(&pre)?.load(&mut *store, i)
        })?;
        let layer = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/layer@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            layer::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        let layer_2 = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/layer-2@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            layer_2::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        let layer_3 = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/layer-3@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            layer_3::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        let layer_4 = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/layer-4@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            layer_4::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        let completer = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/completer@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            completer::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        let formatter = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/formatter@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            formatter::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        let outline = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/outline@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            outline::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        let focus = instance.bindings(|store, i| {
            let pre = i.instance_pre(&*store);
            let name = format!("kalem:plugin/focus@{}", crate::API_VERSION);
            if pre.component().get_export_index(None, &name).is_none() {
                return Ok(None);
            }
            focus::GuestIndices::new(&pre)?
                .load(&mut *store, i)
                .map(Some)
        })?;
        Ok(Extension {
            instance,
            api,
            layer,
            layer_2,
            layer_3,
            layer_4,
            completer,
            formatter,
            outline,
            focus,
        })
    }

    /// Calls the plugin's `activate`: it registers its commands, keys and
    /// subscriptions. Its own refusal is the inner error.
    pub fn activate(&mut self) -> crate::Result<Result<(), String>> {
        let p = &self.api;
        self.instance.run(|s| p.call_activate(s))
    }

    /// Whether the plugin exports layers (API 0.2.10, or `layer-2` of
    /// API 0.2.13, `layer-3` of API 0.2.15, `layer-4` of API 0.2.18).
    pub fn has_layers(&self) -> bool {
        self.layer.is_some()
            || self.layer_2.is_some()
            || self.layer_3.is_some()
            || self.layer_4.is_some()
    }

    /// The overlays of the plugin's layer `id` for a document of `path`
    /// with `text`: by `layer-4` when the plugin exports it, else by
    /// `layer-3` with no line starting a block of an outline, else by
    /// `layer-2` with no line showing a file's text, else by `layer`, with
    /// no lines under lines; `None` when the plugin exports no layer.
    pub fn overlays(
        &mut self,
        id: &str,
        path: Option<&str>,
        text: &str,
    ) -> crate::Result<Option<layer_4::OverlaySet>> {
        let path = path.map(str::to_string);
        if let Some(l) = &self.layer_4 {
            return self
                .instance
                .run(|s| l.call_overlays(s, id, path.as_deref(), text))
                .map(Some);
        }
        if let Some(l) = &self.layer_3 {
            return self
                .instance
                .run(|s| l.call_overlays(s, id, path.as_deref(), text))
                .map(|o| {
                    let below = o
                        .below
                        .into_iter()
                        .map(|b| {
                            let lines = b.lines.into_iter().map(|l| layer_4::VirtualLine {
                                indent: l.indent,
                                pieces: l.pieces,
                                source: l.source.map(|s| layer_4::Source {
                                    path: s.path,
                                    line: s.line,
                                    column: s.column,
                                    text: s.text,
                                    outline: None,
                                }),
                            });
                            layer_4::Below {
                                at: b.at,
                                lines: lines.collect(),
                            }
                        })
                        .collect();
                    Some(layer_4::OverlaySet {
                        spans: o.spans,
                        lines: o.lines,
                        below,
                    })
                });
        }
        if let Some(l) = &self.layer_2 {
            return self
                .instance
                .run(|s| l.call_overlays(s, id, path.as_deref(), text))
                .map(|o| {
                    Some(layer_4::OverlaySet {
                        spans: o.spans,
                        lines: o.lines,
                        below: o
                            .below
                            .into_iter()
                            .map(|b| layer_4::Below {
                                at: b.at,
                                lines: b
                                    .lines
                                    .into_iter()
                                    .map(|l| layer_4::VirtualLine {
                                        indent: l.indent,
                                        pieces: l.pieces,
                                        source: None,
                                    })
                                    .collect(),
                            })
                            .collect(),
                    })
                });
        }
        let Some(l) = &self.layer else {
            return Ok(None);
        };
        self.instance
            .run(|s| l.call_overlays(s, id, path.as_deref(), text))
            .map(|o| {
                Some(layer_4::OverlaySet {
                    spans: o.spans,
                    lines: o.lines,
                    below: Vec::new(),
                })
            })
    }

    /// Whether the plugin exports completers (API 0.2.11).
    pub fn has_completers(&self) -> bool {
        self.completer.is_some()
    }

    /// The items of the plugin's completer `id` for `request`; `None`
    /// when the plugin exports no completer.
    pub fn complete(
        &mut self,
        id: &str,
        request: &completer::Request,
    ) -> crate::Result<Option<Vec<completer::Item>>> {
        let Some(c) = &self.completer else {
            return Ok(None);
        };
        self.instance
            .run(|s| c.call_complete(s, id, request))
            .map(Some)
    }

    /// Whether the plugin exports formatters (API 0.2.16).
    pub fn has_formatters(&self) -> bool {
        self.formatter.is_some()
    }

    /// The text of `request` as the plugin's formatter `id` writes it, or
    /// its refusal; `None` when the plugin exports no formatter.
    pub fn format(
        &mut self,
        id: &str,
        request: &formatter::Request,
    ) -> crate::Result<Option<Result<String, formatter::Refusal>>> {
        let Some(f) = &self.formatter else {
            return Ok(None);
        };
        self.instance
            .run(|s| f.call_format(s, id, request))
            .map(Some)
    }

    /// Whether the plugin exports outlines (API 0.2.18).
    pub fn has_outlines(&self) -> bool {
        self.outline.is_some()
    }

    /// The headings of `request` as the plugin's outline `id` reads
    /// them; `None` when the plugin exports no outline.
    pub fn outline(
        &mut self,
        id: &str,
        request: &outline::Request,
    ) -> crate::Result<Option<Vec<outline::Heading>>> {
        let Some(o) = &self.outline else {
            return Ok(None);
        };
        self.instance
            .run(|s| o.call_outline(s, id, request))
            .map(Some)
    }

    /// Whether the plugin exports `focus` (API 0.2.18): it is told which
    /// document has the focus.
    pub fn has_focus(&self) -> bool {
        self.focus.is_some()
    }

    /// Tells the plugin that document `document`, of `path`, got the
    /// focus; `false` when it does not export `focus`.
    pub fn focused(&mut self, document: u64, path: Option<&str>) -> crate::Result<bool> {
        let Some(f) = &self.focus else {
            return Ok(false);
        };
        self.instance.run(|s| f.call_focused(s, document, path))?;
        Ok(true)
    }

    /// Calls the plugin's `deactivate`, then takes back everything it
    /// registered, whether the call succeeded or not.
    pub fn deactivate(&mut self) -> crate::Result<()> {
        let p = &self.api;
        let out = self.instance.run(|s| p.call_deactivate(s));
        let session = self.instance.data_mut();
        let ids: Vec<u64> = session.registered.keys().copied().collect();
        for id in ids {
            session.dispose(id);
        }
        for request in std::mem::take(&mut session.asked) {
            session.editor.withdraw(request);
        }
        // Its programs stop with it; their ends are not delivered.
        for run in std::mem::take(&mut session.running) {
            session.editor.kill(run);
        }
        out
    }

    /// Runs the plugin's command `id` with `args` as JSON.
    pub fn run_command(&mut self, id: &str, args: &str) -> crate::Result<Result<String, String>> {
        let p = &self.api;
        self.instance.run(|s| p.call_run_command(s, id, args))
    }

    /// Whether the plugin subscribed to events of `kind`.
    pub fn wants(&self, kind: EventKind) -> bool {
        self.instance
            .data()
            .registered
            .values()
            .any(|w| *w == What::Subscription(kind))
    }

    /// Hands `event` to each of the plugin's subscriptions to its kind, in
    /// the order they were made; the first veto is the answer.
    pub fn event(&mut self, event: &Event) -> crate::Result<Reply> {
        let k = kind(event);
        let subscriptions: Vec<u64> = self
            .instance
            .data()
            .registered
            .iter()
            .filter(|(_, w)| **w == What::Subscription(k))
            .map(|(id, _)| *id)
            .collect();
        let p = &self.api;
        for id in subscriptions {
            if let Reply::Veto(why) = self.instance.run(|s| p.call_on_event(s, id, event))? {
                return Ok(Reply::Veto(why));
            }
        }
        Ok(Reply::Proceed)
    }

    /// Hands the user's answer to question `request` to the plugin;
    /// `false` when the plugin did not ask it, or it was answered.
    pub fn answer(&mut self, request: u64, answer: Answer) -> crate::Result<bool> {
        if !self.instance.data_mut().asked.remove(&request) {
            return Ok(false);
        }
        let p = &self.api;
        self.instance
            .run(|s| p.call_on_answer(s, request, &answer))?;
        Ok(true)
    }

    /// Tells the plugin what the user did to widget `key` of its panel
    /// `panel`; `false` when it has no such panel.
    pub fn panel_event(
        &mut self,
        panel: &str,
        key: &str,
        event: &PanelEvent,
    ) -> crate::Result<bool> {
        if self
            .instance
            .data()
            .find(&What::Panel(panel.to_string()))
            .is_none()
        {
            return Ok(false);
        }
        let p = &self.api;
        self.instance
            .run(|s| p.call_on_panel(s, panel, key, event))?;
        Ok(true)
    }

    /// Tells the plugin that setting `key` (Kalem's, or with `own` its
    /// own) changed, when it watches it; `false` when it does not.
    pub fn setting_changed(&mut self, key: &str, own: bool) -> crate::Result<bool> {
        if self
            .instance
            .data()
            .find(&What::Watch(key.to_string(), own))
            .is_none()
        {
            return Ok(false);
        }
        let p = &self.api;
        self.instance.run(|s| p.call_on_setting(s, key, own))?;
        Ok(true)
    }

    /// The settings the plugin watches: Kalem's, and (`true`) its own.
    pub fn watches(&self) -> Vec<(String, bool)> {
        self.instance
            .data()
            .registered
            .values()
            .filter_map(|w| match w {
                What::Watch(k, own) => Some((k.clone(), *own)),
                _ => None,
            })
            .collect()
    }

    /// Hands the response to request `id` to the plugin; `false` when it
    /// did not send it, or it was answered.
    pub fn respond(
        &mut self,
        id: u64,
        response: Result<http::Response, String>,
    ) -> crate::Result<bool> {
        if !self.instance.data_mut().fetching.remove(&id) {
            return Ok(false);
        }
        let p = &self.api;
        self.instance
            .run(|s| p.call_on_response(s, id, response.as_ref().map_err(String::as_str)))?;
        Ok(true)
    }

    /// Hands how run `run` ended to the plugin; `false` when it did not
    /// start it, or its end was delivered.
    pub fn process_done(
        &mut self,
        run: u64,
        result: Result<process::Exit, String>,
    ) -> crate::Result<bool> {
        if !self.instance.data_mut().running.remove(&run) {
            return Ok(false);
        }
        let p = &self.api;
        self.instance
            .run(|s| p.call_on_process(s, run, result.as_ref().map_err(String::as_str)))?;
        Ok(true)
    }

    /// The runs started and not ended.
    pub fn running(&self) -> BTreeSet<u64> {
        self.instance.data().running.clone()
    }

    /// The IDs of the commands the plugin registered.
    pub fn commands(&self) -> BTreeSet<String> {
        self.instance
            .data()
            .registered
            .values()
            .filter_map(|w| match w {
                What::Command(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn verbatim_paths_cannot_go_up() {
        // `canonicalize` gives verbatim paths, where `..` is a name.
        let e = super::real_path(r"\\?\C:\Users\a\..\b.txt").unwrap_err();
        assert!(e.contains(".."), "{e}");
        // And with `/`, which a verbatim path does not split on.
        let e = super::real_path(r"\\?\C:\Users\a\../b.txt").unwrap_err();
        assert!(e.contains(".."), "{e}");
    }
}
