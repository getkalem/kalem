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
    wasmtime::component::bindgen!({
        path: "../kalem-plugin/wit",
        world: "extension",
        with: {
            "kalem:plugin/kalem.disposable": super::Registration,
        },
    });
}

pub use api::{CommandSpec, Event, EventKind, Reply, Scope};
pub use bindings::kalem::plugin::kalem as api;
pub use bindings::kalem::plugin::ui;
pub use bindings::kalem::plugin::{fs, http, net, settings};
pub use ui::{
    Answer, Level, PanelEvent, PanelSpec, PickItem, PickOptions, PromptOptions, StatusOptions,
    WidgetKind, WidgetTree,
};

/// The most a file read through `fs`, or a response, holds.
pub const MAX_BYTES: usize = 16 << 20;

/// What a plugin's manifest permits (§11.6), from its `permissions`:
/// `fs:read:workspace`, `fs:write:workspace`, `fs:read:all`,
/// `net:fetch:DOMAIN` (`*` for any). The `fs` and `net` interfaces are
/// granted only with one of theirs; others (`subprocess`) are not the
/// host's.
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
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
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

    /// Sends `request` (its URL granted) for plugin `plugin`; the response
    /// goes to [`Extension::respond`] with `id`.
    fn fetch(&mut self, plugin: &str, id: u64, request: http::Request);
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
        // The plugin is inside a call: running its own command would enter
        // it again, which a component does not allow.
        if self.owns(&id) {
            return Err(format!(
                "Command `{id}` is the plugin's own: call its function"
            ));
        }
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
    api: bindings::Extension,
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
    /// `settings` interfaces over `editor`, and `fs` and `net` when
    /// `grants` permit; a plugin importing what it was not granted is
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
        api::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        ui::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        settings::add_to_linker::<_, HasSelf<Session>>(
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
        }
        if grants.net() {
            net::add_to_linker::<_, HasSelf<Session>>(
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
        };
        let mut instance = plugin.instantiate(host, &linker, session, limits)?;
        let api = instance.bindings(|store, i| bindings::Extension::new(store, i))?;
        Ok(Extension { instance, api })
    }

    /// Calls the plugin's `activate`: it registers its commands, keys and
    /// subscriptions. Its own refusal is the inner error.
    pub fn activate(&mut self) -> crate::Result<Result<(), String>> {
        let p = self.api.kalem_plugin_plugin();
        self.instance.run(|s| p.call_activate(s))
    }

    /// Calls the plugin's `deactivate`, then takes back everything it
    /// registered, whether the call succeeded or not.
    pub fn deactivate(&mut self) -> crate::Result<()> {
        let p = self.api.kalem_plugin_plugin();
        let out = self.instance.run(|s| p.call_deactivate(s));
        let session = self.instance.data_mut();
        let ids: Vec<u64> = session.registered.keys().copied().collect();
        for id in ids {
            session.dispose(id);
        }
        for request in std::mem::take(&mut session.asked) {
            session.editor.withdraw(request);
        }
        out
    }

    /// Runs the plugin's command `id` with `args` as JSON.
    pub fn run_command(&mut self, id: &str, args: &str) -> crate::Result<Result<String, String>> {
        let p = self.api.kalem_plugin_plugin();
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
        let p = self.api.kalem_plugin_plugin();
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
        let p = self.api.kalem_plugin_plugin();
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
        let p = self.api.kalem_plugin_plugin();
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
        let p = self.api.kalem_plugin_plugin();
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
        let p = self.api.kalem_plugin_plugin();
        self.instance
            .run(|s| p.call_on_response(s, id, response.as_ref().map_err(String::as_str)))?;
        Ok(true)
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
