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

use crate::extension::kalem::plugin::kalem as api;

pub use api::{CommandSpec, Event, EventKind, Reply, Scope, version};

/// A plugin of the `extension` world.
pub trait Plugin {
    /// Registers the plugin's commands, keys and subscriptions.
    fn activate() -> Result<(), String>;

    /// Called before the plugin is unloaded; what it registered is taken
    /// back after.
    fn deactivate() {}
}

type CommandFn = Box<dyn FnMut(&str) -> Result<String, String>>;
type EventFn = Box<dyn FnMut(&Event) -> Reply>;

/// The plugin's handlers, by command and by subscription.
#[derive(Default)]
struct Handlers {
    commands: HashMap<String, Option<CommandFn>>,
    events: HashMap<u64, Option<EventFn>>,
}

thread_local! {
    static HANDLERS: RefCell<Handlers> = RefCell::default();
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
enum Local {
    Command(String),
    Event(u64),
    Binding,
}

/// Something registered. Dropping it keeps the registration; the host
/// takes back everything the plugin registered when it is deactivated.
#[derive(Debug)]
pub struct Disposable {
    host: api::Disposable,
    local: Local,
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
                Local::Binding => {}
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
/// (`"null"` for none). A plugin runs its own commands as functions.
pub fn run(id: &str, args: &str) -> Result<String, String> {
    api::run(id, args)
}

/// Binds `keys` to command `command` when `when` holds.
pub fn keymap(keys: &str, command: &str, when: Option<&str>) -> Result<Disposable, String> {
    Ok(Disposable {
        host: api::keymap(keys, command, when)?,
        local: Local::Binding,
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

/// Forgets every handler (after `deactivate`).
#[doc(hidden)]
pub fn clear() {
    HANDLERS.with(|h| *h.borrow_mut() = Handlers::default());
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
        }

        $crate::extension::export_extension!(__KalemPlugin);
    };
}
