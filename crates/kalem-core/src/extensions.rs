//! Extension plugins as the editors see them (design §11.2 to §11.4,
//! T3.1.12): what the plugins registered (commands, key bindings) and the
//! way to them (running a command, handing an event).
//!
//! The plugins run in `kalem-script`'s WebAssembly host, which the core
//! does not depend on: the binary installs an [`Extensions`] over the host
//! at start ([`install`]), as it registers viewers. A plugin's
//! registrations land here; the editors build their command registry and
//! keymap with them (`CommandRegistry::with_builtins`, `Keymap::build`)
//! and build them again when [`generation`] changes. A command a plugin
//! runs (`kalem.run`) waits in a queue: one asked for inside a plugin's
//! command runs right after it, in the same context; one asked for by an
//! event's handler runs at the editor's next tick ([`take_runs`]).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::command::{Command, CommandError};
use crate::events::{Event, Reply};
use crate::keymap::{Binding, Entry, Origin};
use crate::keys::KeySequence;
use crate::when::WhenClause;

/// The plugins, as the host runs them.
pub trait Extensions: Send {
    /// Runs plugin command `id` with `args` as JSON.
    fn run(&mut self, id: &str, args: &str) -> Result<(), String>;

    /// Hands `event` to the plugins subscribed to it, activating those
    /// waiting for it; the first veto's reason.
    fn event(&mut self, event: &Event) -> Option<String>;
}

static INSTALLED: Mutex<Option<Box<dyn Extensions>>> = Mutex::new(None);

/// What the plugins registered.
#[derive(Default)]
struct State {
    commands: BTreeMap<String, Command>,
    bindings: BTreeMap<u64, Binding>,
    runs: Vec<(String, Value)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    commands: BTreeMap::new(),
    bindings: BTreeMap::new(),
    runs: Vec::new(),
});

static GENERATION: AtomicU64 = AtomicU64::new(0);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn changed() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Installs the plugins (once, at start).
pub fn install(extensions: Box<dyn Extensions>) {
    *INSTALLED.lock().unwrap_or_else(|e| e.into_inner()) = Some(extensions);
}

/// Changes each time the plugins' commands or bindings change: an editor
/// holding registries built at another builds them again.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The plugins' commands.
pub fn commands() -> Vec<Command> {
    state().commands.values().cloned().collect()
}

/// The plugins' key bindings, after the commands' default keys and before
/// the profile's and the user's.
pub fn bindings() -> Vec<Binding> {
    state().bindings.values().cloned().collect()
}

/// Adds a plugin's command; refused when its ID is a built-in command's
/// or another plugin's, or it is not of the convention.
pub fn add_command(command: Command) -> Result<(), String> {
    if !crate::command::valid_id(&command.id) {
        return Err(format!("Invalid command ID `{}`", command.id));
    }
    if command.scope.is_none() {
        return Err(format!("Command `{}` has no scope", command.id));
    }
    if builtin(&command.id) {
        return Err(format!("Command `{}` is Kalem's", command.id));
    }
    let mut s = state();
    if s.commands.contains_key(&command.id) {
        return Err(format!("Command `{}` is already registered", command.id));
    }
    s.commands.insert(command.id.clone(), command);
    drop(s);
    changed();
    Ok(())
}

/// Whether `id` is a built-in command's.
fn builtin(id: &str) -> bool {
    static IDS: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    IDS.get_or_init(|| {
        crate::builtin::commands()
            .into_iter()
            .map(|c| c.id)
            .collect()
    })
    .contains(id)
}

/// Whether command `id` exists, built in or a plugin's.
pub fn known(id: &str) -> bool {
    builtin(id) || state().commands.contains_key(id)
}

/// Removes a plugin's command.
pub fn remove_command(id: &str) {
    if state().commands.remove(id).is_some() {
        changed();
    }
}

/// Adds binding `id` of `keys` (as `keymap.json` writes them) to
/// `command`, when `when` holds.
pub fn add_binding(id: u64, keys: &str, command: &str, when: Option<&str>) -> Result<(), String> {
    let parsed = KeySequence::parse(keys).ok_or_else(|| format!("`{keys}` are not keys"))?;
    let when = when
        .map(WhenClause::parse)
        .transpose()
        .map_err(|e| format!("`{}`: {e:?}", when.unwrap_or_default()))?;
    state().bindings.insert(
        id,
        Binding {
            keys: parsed,
            command: command.to_string(),
            args: Value::Null,
            when,
            terminal_keys: None,
            origin: Origin::Default,
        },
    );
    changed();
    Ok(())
}

/// Removes binding `id`.
pub fn remove_binding(id: u64) {
    if state().bindings.remove(&id).is_some() {
        changed();
    }
}

/// The plugins' bindings as keymap entries.
pub fn entries() -> Vec<Entry> {
    bindings().into_iter().map(Entry::Add).collect()
}

/// Queues command `id` with `args` (a plugin's `kalem.run`).
pub fn queue_run(id: &str, args: Value) {
    state().runs.push((id.to_string(), args));
}

/// The commands queued, in order.
pub fn take_runs() -> Vec<(String, Value)> {
    std::mem::take(&mut state().runs)
}

/// Runs plugin command `id`.
pub fn run(id: &str, args: &Value) -> Result<(), CommandError> {
    let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(x) = installed.as_mut() else {
        return Err(CommandError::new(format!("No plugin runs `{id}`")));
    };
    x.run(id, &args.to_string()).map_err(CommandError::new)
}

/// Hands `event` to the plugins: a bus's subscriber.
pub fn event(event: &Event) -> Reply {
    let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    match installed.as_mut().and_then(|x| x.event(event)) {
        Some(why) => Reply::Veto(why),
        None => Reply::Continue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandHandler, CommandSource, Scope};

    fn command(id: &str) -> Command {
        Command {
            id: id.into(),
            title: "T".into(),
            category: String::new(),
            default_keys: Vec::new(),
            when: None,
            handler: CommandHandler::Plugin("p".into()),
            args_schema: None,
            source: CommandSource::Plugin("p".into()),
            scope: Some(Scope::all()),
        }
    }

    #[test]
    fn registrations_are_checked_and_counted() {
        let g = generation();
        assert!(add_command(command("org.todo.cycle")).is_err(), "Kalem's");
        assert!(add_command(command("Bad")).is_err());
        add_command(command("extests.one")).unwrap();
        assert!(add_command(command("extests.one")).is_err(), "taken");
        assert!(known("extests.one") && known("org.todo.cycle"));
        assert!(generation() > g);
        assert!(add_binding(9_001, "not a key ++", "extests.one", None).is_err());
        add_binding(9_001, "ctrl+alt+x", "extests.one", Some("inTable")).unwrap();
        assert!(bindings().iter().any(|b| b.command == "extests.one"));
        remove_binding(9_001);
        remove_command("extests.one");
        assert!(!known("extests.one"));
        assert!(!bindings().iter().any(|b| b.command == "extests.one"));
    }
}
