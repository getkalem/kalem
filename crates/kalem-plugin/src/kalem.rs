//! The `kalem` namespace of design §11.4 over the `extension` world: a
//! plugin registers its commands with the functions that run them, and
//! its subscriptions with their handlers; [`export_plugin!`] writes the
//! world's exports, which hand the host's calls to them.
//!
//! ```ignore
//! use kalem_plugin::kalem::{self, EventKind, Plugin, Reply, Scope};
//!
//! struct Counter;
//! impl Plugin for Counter {
//!     fn activate() -> Result<(), String> {
//!         let mut spec = kalem::spec("counter.count", "Count", Scope::only(&["org"]));
//!         spec.keys = vec!["ctrl+alt+c".into()];
//!         kalem::command(spec, |_args| Ok("1".into()))?;
//!         kalem::on(EventKind::DocumentOpen, |_event| Reply::Proceed)?;
//!         Ok(())
//!     }
//! }
//! kalem_plugin::export_plugin!(Counter);
//! ```

use std::cell::RefCell;
use std::collections::HashMap;

use crate::extension::kalem::plugin::http::Response;
use crate::extension::kalem::plugin::kalem as api;
use crate::extension::kalem::plugin::ui::{Answer, PanelEvent};

pub use api::{CommandSpec, Event, EventKind, Reply, Scope, version};

/// A plugin of the `extension` world.
pub trait Plugin {
    /// Registers the plugin's commands, keys and subscriptions.
    fn activate() -> Result<(), String>;

    /// Called before the plugin is unloaded; what it registered is taken
    /// back after.
    fn deactivate() {}

    /// The overlays of the plugin's layer `layer` (its manifest's
    /// `layers`) for the document of `path` with `text` (API 0.2.10, the
    /// feature `layer`); none by default.
    #[cfg(feature = "layer")]
    fn overlays(_layer: &str, _path: Option<&str>, _text: &str) -> crate::layer::OverlaySet {
        crate::layer::OverlaySet {
            spans: Vec::new(),
            lines: Vec::new(),
        }
    }
}

type CommandFn = Box<dyn FnMut(&str) -> Result<String, String>>;
type EventFn = Box<dyn FnMut(&Event) -> Reply>;
pub(crate) type AnswerFn = Box<dyn FnOnce(Answer)>;
pub(crate) type PanelFn = Box<dyn FnMut(&str, &PanelEvent)>;
pub(crate) type SettingFn = Box<dyn FnMut(&str)>;
pub(crate) type ResponseFn = Box<dyn FnOnce(Result<Response, String>)>;
pub(crate) type ProcessFn = Box<dyn FnOnce(Result<crate::process::Exit, String>)>;

/// The plugin's handlers, by command, subscription, question and panel.
#[derive(Default)]
pub(crate) struct Handlers {
    commands: HashMap<String, Option<CommandFn>>,
    events: HashMap<u64, Option<EventFn>>,
    pub(crate) answers: HashMap<u64, AnswerFn>,
    pub(crate) panels: HashMap<String, Option<PanelFn>>,
    /// Watched settings: the key, whether it is the plugin's own, and the
    /// handler, by the watch's ID.
    pub(crate) watches: HashMap<u64, (String, bool, Option<SettingFn>)>,
    pub(crate) responses: HashMap<u64, ResponseFn>,
    pub(crate) processes: HashMap<u64, ProcessFn>,
}

thread_local! {
    pub(crate) static HANDLERS: RefCell<Handlers> = RefCell::default();
}

impl Scope {
    /// Every text type (`"all"`).
    pub fn all() -> Scope {
        Scope {
            types: None,
            except: Vec::new(),
        }
    }

    /// These types only.
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
}

/// A command's description with its ID, title and scope, in no category,
/// with no when-clause, keys or arguments.
pub fn spec(id: &str, title: &str, scope: Scope) -> CommandSpec {
    CommandSpec {
        id: id.into(),
        title: title.into(),
        category: String::new(),
        scope,
        when: None,
        keys: Vec::new(),
        args_schema: None,
    }
}

/// What a registration is, for taking it back.
#[derive(Debug)]
pub(crate) enum Local {
    Command(String),
    Event(u64),
    Panel(String),
    Watch(u64),
    Other,
}

/// Something registered. Dropping it keeps the registration; the host
/// takes back everything the plugin registered when it is deactivated.
#[derive(Debug)]
pub struct Disposable {
    pub(crate) host: api::Disposable,
    pub(crate) local: Local,
}

impl Disposable {
    /// Its number: a subscription's, as its events carry.
    pub fn id(&self) -> u64 {
        self.host.id()
    }

    /// Takes the registration back.
    pub fn dispose(self) {
        HANDLERS.with(|h| {
            let mut h = h.borrow_mut();
            match &self.local {
                Local::Command(id) => {
                    h.commands.remove(id);
                }
                Local::Event(id) => {
                    h.events.remove(id);
                }
                Local::Panel(id) => {
                    h.panels.remove(id);
                }
                Local::Watch(id) => {
                    h.watches.remove(id);
                }
                Local::Other => {}
            }
        });
        self.host.dispose();
    }
}

/// Adds a command run by `run`, which takes its arguments and returns its
/// result, both as JSON.
pub fn command(
    spec: CommandSpec,
    run: impl FnMut(&str) -> Result<String, String> + 'static,
) -> Result<Disposable, String> {
    let id = spec.id.clone();
    let host = api::command(&spec)?;
    HANDLERS.with(|h| {
        h.borrow_mut()
            .commands
            .insert(id.clone(), Some(Box::new(run)))
    });
    Ok(Disposable {
        host,
        local: Local::Command(id),
    })
}

/// Runs a command, built in or another plugin's, with arguments as JSON
/// (`"null"` for none), once the plugin's call returns. A plugin runs its
/// own commands as functions.
pub fn run(id: &str, args: &str) -> Result<(), String> {
    api::run(id, args)
}

/// Binds `keys` to command `command` when `when` holds.
pub fn keymap(keys: &str, command: &str, when: Option<&str>) -> Result<Disposable, String> {
    Ok(Disposable {
        host: api::keymap(keys, command, when)?,
        local: Local::Other,
    })
}

/// Calls `handler` with each event of `kind`; its reply vetoes a vetoable
/// one.
pub fn on(
    kind: EventKind,
    handler: impl FnMut(&Event) -> Reply + 'static,
) -> Result<Disposable, String> {
    let host = api::on(kind)?;
    let id = host.id();
    HANDLERS.with(|h| h.borrow_mut().events.insert(id, Some(Box::new(handler))));
    Ok(Disposable {
        host,
        local: Local::Event(id),
    })
}

/// Runs the plugin's command `id` (the host's `run-command`). The handler
/// is taken out while it runs, so that it may register or dispose others.
#[doc(hidden)]
pub fn dispatch_command(id: &str, args: &str) -> Result<String, String> {
    let taken = HANDLERS.with(|h| h.borrow_mut().commands.get_mut(id).and_then(Option::take));
    let Some(mut f) = taken else {
        return Err(format!("No command `{id}` in this plugin"));
    };
    let out = f(args);
    HANDLERS.with(|h| {
        if let Some(slot @ None) = h.borrow_mut().commands.get_mut(id) {
            *slot = Some(f);
        }
    });
    out
}

/// Hands an event to the handler of `subscription` (the host's
/// `on-event`).
#[doc(hidden)]
pub fn dispatch_event(subscription: u64, event: &Event) -> Reply {
    let taken = HANDLERS.with(|h| {
        h.borrow_mut()
            .events
            .get_mut(&subscription)
            .and_then(Option::take)
    });
    let Some(mut f) = taken else {
        return Reply::Proceed;
    };
    let reply = f(event);
    HANDLERS.with(|h| {
        if let Some(slot @ None) = h.borrow_mut().events.get_mut(&subscription) {
            *slot = Some(f);
        }
    });
    reply
}

/// Hands the answer to question `request` to its handler (the host's
/// `on-answer`), once.
#[doc(hidden)]
pub fn dispatch_answer(request: u64, answer: Answer) {
    let taken = HANDLERS.with(|h| h.borrow_mut().answers.remove(&request));
    if let Some(f) = taken {
        f(answer);
    }
}

/// Hands what the user did to widget `key` of panel `panel` to the
/// panel's handler (the host's `on-panel`).
#[doc(hidden)]
pub fn dispatch_panel(panel: &str, key: &str, event: &PanelEvent) {
    let taken = HANDLERS.with(|h| h.borrow_mut().panels.get_mut(panel).and_then(Option::take));
    let Some(mut f) = taken else {
        return;
    };
    f(key, event);
    HANDLERS.with(|h| {
        if let Some(slot @ None) = h.borrow_mut().panels.get_mut(panel) {
            *slot = Some(f);
        }
    });
}

/// Hands a change of setting `key` to the handlers watching it (the
/// host's `on-setting`).
#[doc(hidden)]
pub fn dispatch_setting(key: &str, own: bool) {
    let ids: Vec<u64> = HANDLERS.with(|h| {
        h.borrow()
            .watches
            .iter()
            .filter(|(_, (k, o, _))| k == key && *o == own)
            .map(|(id, _)| *id)
            .collect()
    });
    for id in ids {
        let taken = HANDLERS.with(|h| h.borrow_mut().watches.get_mut(&id).and_then(|w| w.2.take()));
        if let Some(mut f) = taken {
            f(key);
            HANDLERS.with(|h| {
                if let Some(w) = h.borrow_mut().watches.get_mut(&id)
                    && w.2.is_none()
                {
                    w.2 = Some(f);
                }
            });
        }
    }
}

/// Hands the response to request `request` to its handler, once (the
/// host's `on-response`).
#[doc(hidden)]
pub fn dispatch_response(request: u64, response: Result<Response, String>) {
    let taken = HANDLERS.with(|h| h.borrow_mut().responses.remove(&request));
    if let Some(f) = taken {
        f(response);
    }
}

/// Hands how run `run` ended to its handler, once (the host's
/// `on-process`).
#[doc(hidden)]
pub fn dispatch_process(run: u64, result: Result<crate::process::Exit, String>) {
    let taken = HANDLERS.with(|h| h.borrow_mut().processes.remove(&run));
    if let Some(f) = taken {
        f(result);
    }
}

/// Forgets every handler (after `deactivate`).
#[doc(hidden)]
pub fn clear() {
    HANDLERS.with(|h| *h.borrow_mut() = Handlers::default());
}

/// Installs, once, the panic hook that tells the host a panic's message
/// and where it happened (the `diagnostics` import, wasm_todo W10): a
/// component stops at a panic with a trap that carries neither.
/// [`export_plugin!`](crate::export_plugin)'s `activate` calls it.
pub fn report_panics() {
    #[cfg(target_arch = "wasm32")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                crate::extension::kalem::plugin::diagnostics::panicked(&info.to_string());
            }));
        });
    }
}

/// Exports a [`Plugin`] as the component's `extension` world:
/// `export_plugin!(WordCount)`.
#[macro_export]
macro_rules! export_plugin {
    ($plugin:ty) => {
        #[doc(hidden)]
        pub struct __KalemPlugin;

        impl $crate::extension::exports::kalem::plugin::plugin::Guest for __KalemPlugin {
            fn activate() -> ::std::result::Result<(), ::std::string::String> {
                $crate::kalem::report_panics();
                <$plugin as $crate::kalem::Plugin>::activate()
            }

            fn deactivate() {
                <$plugin as $crate::kalem::Plugin>::deactivate();
                $crate::kalem::clear();
            }

            fn run_command(
                id: ::std::string::String,
                args: ::std::string::String,
            ) -> ::std::result::Result<::std::string::String, ::std::string::String> {
                $crate::kalem::dispatch_command(&id, &args)
            }

            fn on_event(subscription: u64, event: $crate::kalem::Event) -> $crate::kalem::Reply {
                $crate::kalem::dispatch_event(subscription, &event)
            }

            fn on_answer(request: u64, answer: $crate::ui::Answer) {
                $crate::kalem::dispatch_answer(request, answer)
            }

            fn on_panel(
                panel: ::std::string::String,
                key: ::std::string::String,
                event: $crate::ui::PanelEvent,
            ) {
                $crate::kalem::dispatch_panel(&panel, &key, &event)
            }

            fn on_setting(key: ::std::string::String, own: bool) {
                $crate::kalem::dispatch_setting(&key, own)
            }

            fn on_response(
                request: u64,
                response: ::std::result::Result<$crate::net::Response, ::std::string::String>,
            ) {
                $crate::kalem::dispatch_response(request, response)
            }

            fn on_process(
                run: u64,
                result: ::std::result::Result<$crate::process::Exit, ::std::string::String>,
            ) {
                $crate::kalem::dispatch_process(run, result)
            }
        }

        $crate::__export_layer!($plugin);

        $crate::extension::export_extension!(__KalemPlugin);
    };
}

/// The `layer` interface of [`export_plugin!`] with the feature `layer`.
#[cfg(feature = "layer")]
#[doc(hidden)]
#[macro_export]
macro_rules! __export_layer {
    ($plugin:ty) => {
        impl $crate::extension::exports::kalem::plugin::layer::Guest for __KalemPlugin {
            fn overlays(
                layer: ::std::string::String,
                path: ::std::option::Option<::std::string::String>,
                text: ::std::string::String,
            ) -> $crate::layer::OverlaySet {
                <$plugin as $crate::kalem::Plugin>::overlays(&layer, path.as_deref(), &text)
            }
        }
    };
}

/// Nothing without the feature `layer`.
#[cfg(not(feature = "layer"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __export_layer {
    ($plugin:ty) => {};
}
