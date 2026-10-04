//! The host's side of the `extension` world (design §11.2 to §11.4): a
//! plugin's commands, key bindings and subscriptions, generated from
//! `kalem-plugin`'s WIT files (D6).
//!
//! The editor is reached through [`Editor`], which the embedder implements
//! over its command registry and keymap. The host keeps what each plugin
//! registered, so that a disposal or the plugin's deactivation takes back
//! exactly that, and checks what is the plugin's to decide: a command's
//! ID is the plugin's own (`pluginId.action`) and its scope names a type
//! (§11.2). The rest (the ID's form, a new ID, the keys, the
//! when-clauses) is the editor's, as for its own commands.

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

    /// Runs command `id` with `args` as JSON; its result as JSON.
    fn run(&mut self, id: &str, args: &str) -> Result<String, String>;
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
}

/// What an extension plugin's store holds: the editor, and what the
/// plugin registered.
pub struct Session {
    plugin: String,
    editor: Box<dyn Editor>,
    table: ResourceTable,
    next: u64,
    registered: BTreeMap<u64, What>,
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
            Some(What::Subscription(_)) | None => {}
        }
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
        let own = spec
            .id
            .strip_prefix(&self.plugin)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|action| !action.is_empty());
        if !own {
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

    fn run(&mut self, id: String, args: String) -> Result<String, String> {
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
    /// Instantiates `plugin`, known as `id`, granted the `kalem`
    /// interface over `editor`, and nothing else.
    pub fn new(
        host: &crate::Host,
        plugin: &crate::Plugin,
        id: &str,
        editor: Box<dyn Editor>,
        limits: crate::Limits,
    ) -> crate::Result<Extension> {
        let mut linker = host.linker::<Session>();
        api::add_to_linker::<_, HasSelf<Session>>(&mut linker, |d: &mut crate::Data<Session>| {
            &mut d.user
        })
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        let session = Session {
            plugin: id.to_string(),
            editor,
            table: ResourceTable::new(),
            next: 0,
            registered: BTreeMap::new(),
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
