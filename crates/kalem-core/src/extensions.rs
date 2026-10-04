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
//!
//! What a plugin shows lands here too: its questions become the editors'
//! own requests ([`take_requests`]: a line asked for in the palette, a
//! choice offered as a list), answered through the command
//! `plugin.answer`; its status bar items ([`status_items`]) and its panels
//! ([`panels`]), a tree of widgets each editor draws with its own (D11),
//! the editors draw again when [`shown`] changes.

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

    /// Hands the user's answer to question `request` to the plugin that
    /// asked it.
    fn answer(&mut self, request: u64, answer: Answer);

    /// Tells the plugin of panel `panel` what the user did to its widget
    /// `key`.
    fn panel_event(&mut self, panel: &str, key: &str, event: &PanelEvent);
}

/// A question a plugin asks.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// A line of text.
    Prompt {
        /// What is asked.
        title: String,
        /// The text it starts with.
        value: Option<String>,
    },
    /// Yes or no.
    Confirm(String),
    /// One of a list: labels and second lines.
    Pick {
        /// The list's title.
        title: Option<String>,
        /// The entries.
        items: Vec<(String, Option<String>)>,
    },
}

/// The user's answer to a question.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The text; `None` when cancelled.
    Text(Option<String>),
    /// Yes or no.
    Confirmed(bool),
    /// The entries chosen, by index.
    Picked(Vec<u32>),
}

/// What the user did to a widget of a panel.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelEvent {
    /// A button or an entry clicked.
    Clicked,
    /// An input's text changed.
    Changed(String),
    /// An input's text entered.
    Submitted(String),
    /// A checkbox ticked or not.
    Toggled(bool),
    /// An entry's children shown or hidden.
    Expanded(bool),
}

/// A plugin's status bar item.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusItem {
    /// The plugin.
    pub plugin: String,
    /// Its ID in the plugin.
    pub id: String,
    /// What it shows.
    pub text: String,
    /// Shown on hovering.
    pub tooltip: Option<String>,
    /// The command a click runs.
    pub command: Option<String>,
    /// At the right end, else the left.
    pub right: bool,
    /// Higher stands nearer its end.
    pub priority: i32,
}

/// How a label's text shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    /// As text.
    Normal,
    /// Bold.
    Strong,
    /// Italic.
    Emphasis,
    /// Dimmer.
    Muted,
    /// Monospace.
    Code,
    /// In the error colour.
    Error,
    /// A heading.
    Heading,
}

/// What a widget is (the API's `widget-kind`).
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetKind {
    /// Its children, one under another.
    Column,
    /// Its children side by side.
    Row,
    /// A text.
    Label {
        /// The text.
        text: String,
        /// How it shows.
        style: TextStyle,
    },
    /// A button.
    Button {
        /// Its label.
        label: String,
        /// A command it runs; else the plugin hears the click.
        command: Option<String>,
    },
    /// A line of text the user edits.
    Input {
        /// The text.
        value: String,
        /// Shown while empty.
        placeholder: Option<String>,
    },
    /// A box to tick.
    Checkbox {
        /// Its label.
        label: String,
        /// Ticked.
        checked: bool,
    },
    /// An entry of a list or a tree.
    Item {
        /// Its label.
        label: String,
        /// A dimmer second text.
        detail: Option<String>,
        /// Whether its children show; `None` without children.
        expanded: Option<bool>,
        /// Marked as chosen.
        selected: bool,
    },
    /// A bar of progress.
    Progress {
        /// From 0 to 1; `None` while unknown.
        value: Option<f32>,
        /// A text with it.
        label: Option<String>,
    },
    /// A line between widgets.
    Separator,
}

/// A widget of a panel's tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    /// Names it in its events.
    pub key: String,
    /// What it is.
    pub kind: WidgetKind,
    /// Its children, by index, after it.
    pub children: Vec<u32>,
}

/// A plugin's panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// The plugin.
    pub plugin: String,
    /// `pluginId.name`.
    pub id: String,
    /// Its title.
    pub title: String,
    /// At the bottom, else at the side.
    pub bottom: bool,
    /// The tree, the root first (checked by the host); empty until set.
    pub widgets: Vec<Widget>,
}

impl Panel {
    /// The widgets in the order they show, with their depth, a collapsed
    /// entry's children left out; rows as one line each: the widgets of
    /// a row after it at its depth.
    pub fn lines(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if !self.widgets.is_empty() {
            self.walk(0, 0, &mut out);
        }
        out
    }

    fn walk(&self, i: usize, depth: usize, out: &mut Vec<(usize, usize)>) {
        let Some(w) = self.widgets.get(i) else {
            return;
        };
        let (inner, shown) = match &w.kind {
            // The root column and rows are not lines of their own.
            WidgetKind::Column if i == 0 => (depth, true),
            WidgetKind::Column => (depth, true),
            WidgetKind::Item { expanded, .. } => {
                out.push((i, depth));
                (depth + 1, expanded.unwrap_or(true))
            }
            _ => {
                out.push((i, depth));
                (depth, true)
            }
        };
        if shown && !matches!(w.kind, WidgetKind::Row) {
            for &c in &w.children {
                self.walk(c as usize, inner, out);
            }
        }
    }

    /// The widgets a user acts on, by index: buttons, inputs, checkboxes
    /// and entries, in the order they show.
    pub fn actions(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, _) in self.lines() {
            let w = &self.widgets[i];
            let row: Vec<usize> = if matches!(w.kind, WidgetKind::Row) {
                w.children.iter().map(|&c| c as usize).collect()
            } else {
                vec![i]
            };
            out.extend(row.into_iter().filter(|&j| {
                matches!(
                    self.widgets[j].kind,
                    WidgetKind::Button { .. }
                        | WidgetKind::Input { .. }
                        | WidgetKind::Checkbox { .. }
                        | WidgetKind::Item { .. }
                )
            }));
        }
        out
    }
}

static INSTALLED: Mutex<Option<Box<dyn Extensions>>> = Mutex::new(None);

/// What the plugins registered.
#[derive(Default)]
struct State {
    commands: BTreeMap<String, Command>,
    bindings: BTreeMap<u64, Binding>,
    runs: Vec<(String, Value)>,
    /// Questions not yet shown.
    questions: Vec<(u64, Question)>,
    /// Questions not yet answered.
    asking: std::collections::BTreeSet<u64>,
    status: BTreeMap<(String, String), StatusItem>,
    panels: BTreeMap<String, Panel>,
}

static STATE: Mutex<State> = Mutex::new(State {
    commands: BTreeMap::new(),
    bindings: BTreeMap::new(),
    runs: Vec::new(),
    questions: Vec::new(),
    asking: std::collections::BTreeSet::new(),
    status: BTreeMap::new(),
    panels: BTreeMap::new(),
});

static GENERATION: AtomicU64 = AtomicU64::new(0);
static SHOWN: AtomicU64 = AtomicU64::new(0);

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

fn shown_changed() {
    SHOWN.fetch_add(1, Ordering::Relaxed);
}

/// Changes each time the plugins' status bar items or panels change: the
/// editors draw them again.
pub fn shown() -> u64 {
    SHOWN.load(Ordering::Relaxed)
}

/// Queues `question`, asked as `request`, for the editors to show.
pub fn ask(request: u64, question: Question) {
    let mut s = state();
    s.asking.insert(request);
    s.questions.push((request, question));
}

/// Closes question `request` unanswered (its plugin went).
pub fn withdraw(request: u64) {
    let mut s = state();
    s.asking.remove(&request);
    s.questions.retain(|(r, _)| *r != request);
}

/// Whether a plugin's question waits for an answer (`pluginAsks`).
pub fn asking() -> bool {
    !state().asking.is_empty()
}

/// The questions to show, as the editors' requests: a line asked for in
/// the palette, a choice offered as a list, each answered through
/// `plugin.answer`.
pub fn take_requests() -> Vec<crate::command::Request> {
    use crate::command::Request;
    use crate::palette::{PaletteItem, invocation};
    let item = |title: String, category: String, args: Value| PaletteItem {
        id: invocation(ANSWER, &args),
        title,
        category,
        keys: String::new(),
        also: String::new(),
    };
    std::mem::take(&mut state().questions)
        .into_iter()
        .map(|(request, q)| match q {
            Question::Prompt { title, value } => {
                let mut args = serde_json::json!({ "request": request, "ask": title });
                if let Some(v) = value {
                    args[format!("{title}_default")] = Value::String(v);
                }
                Request::Ask {
                    command: ANSWER.into(),
                    args,
                    arg: title,
                }
            }
            Question::Confirm(message) => Request::Choose(
                [
                    (true, crate::tr!("choice-yes")),
                    (false, crate::tr!("choice-no")),
                ]
                .into_iter()
                .map(|(yes, label)| {
                    item(
                        label,
                        message.clone(),
                        serde_json::json!({ "request": request, "confirmed": yes }),
                    )
                })
                .collect(),
            ),
            Question::Pick { title, items } => Request::Choose(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(i, (label, detail))| {
                        item(
                            label,
                            detail.or_else(|| title.clone()).unwrap_or_default(),
                            serde_json::json!({ "request": request, "picked": i }),
                        )
                    })
                    .collect(),
            ),
        })
        .collect()
}

/// The command answering a plugin's question.
pub const ANSWER: &str = "plugin.answer";

/// Hands the answer to question `request` to its plugin, once.
pub fn answer(request: u64, answer: Answer) -> bool {
    if !state().asking.remove(&request) {
        return false;
    }
    let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = installed.as_mut() {
        x.answer(request, answer);
    }
    true
}

/// Shows a plugin's status bar item, or changes it.
pub fn set_status(item: StatusItem) {
    state()
        .status
        .insert((item.plugin.clone(), item.id.clone()), item);
    shown_changed();
}

/// Takes plugin `plugin`'s status bar item `id` away.
pub fn remove_status(plugin: &str, id: &str) {
    if state()
        .status
        .remove(&(plugin.to_string(), id.to_string()))
        .is_some()
    {
        shown_changed();
    }
}

/// The plugins' status bar items: the left ones, then the right ones,
/// each side in the order they stand from its end.
pub fn status_items() -> (Vec<StatusItem>, Vec<StatusItem>) {
    let mut all: Vec<StatusItem> = state().status.values().cloned().collect();
    all.sort_by_key(|i| std::cmp::Reverse(i.priority));
    all.into_iter().partition(|i| !i.right)
}

/// Adds a plugin's panel, empty.
pub fn add_panel(panel: Panel) {
    state().panels.insert(panel.id.clone(), panel);
    shown_changed();
}

/// Replaces panel `id`'s widgets.
pub fn set_panel(id: &str, widgets: Vec<Widget>) {
    if let Some(p) = state().panels.get_mut(id) {
        p.widgets = widgets;
    }
    shown_changed();
}

/// Removes panel `id`.
pub fn remove_panel(id: &str) {
    if state().panels.remove(id).is_some() {
        shown_changed();
    }
}

/// The plugins' panels, by ID.
pub fn panels() -> Vec<Panel> {
    state().panels.values().cloned().collect()
}

/// Panel `id`.
pub fn panel(id: &str) -> Option<Panel> {
    state().panels.get(id).cloned()
}

/// Whether a plugin has a panel (`pluginPanels`).
pub fn has_panels() -> bool {
    !state().panels.is_empty()
}

/// Tells the plugin of panel `panel` what the user did to widget `key`.
pub fn panel_event(panel: &str, key: &str, event: &PanelEvent) {
    let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = installed.as_mut() {
        x.panel_event(panel, key, event);
    }
}

/// What a click on widget `index` of `panel` does: the plugin hears it,
/// or a button's command is returned to run. An input's new text is
/// asked for: the request returned.
pub fn activate(panel: &Panel, index: usize) -> Option<crate::command::Request> {
    let w = panel.widgets.get(index)?;
    let event = match &w.kind {
        WidgetKind::Button {
            command: Some(c), ..
        } => {
            queue_run(c, Value::Null);
            return None;
        }
        WidgetKind::Button { .. } => PanelEvent::Clicked,
        WidgetKind::Checkbox { checked, .. } => PanelEvent::Toggled(!checked),
        WidgetKind::Item {
            expanded: Some(e), ..
        } => PanelEvent::Expanded(!e),
        WidgetKind::Item { .. } => PanelEvent::Clicked,
        WidgetKind::Input { value, .. } => {
            let name = crate::tr!("panel-input");
            return Some(crate::command::Request::Ask {
                command: PANEL_EVENT.into(),
                args: serde_json::json!({
                    "panel": panel.id, "key": w.key, "ask": name,
                    format!("{name}_default"): value,
                }),
                arg: name,
            });
        }
        _ => return None,
    };
    panel_event(&panel.id, &w.key, &event);
    None
}

/// The command acting on a panel's widget.
pub const PANEL_EVENT: &str = "plugin.panelEvent";

/// The text of a widget as a line: a label's text, a button's label in
/// brackets, a checkbox's box, an entry's arrow.
pub fn widget_text(w: &Widget) -> String {
    match &w.kind {
        WidgetKind::Label { text, .. } => text.clone(),
        WidgetKind::Button { label, .. } => format!("[ {label} ]"),
        WidgetKind::Input { value, placeholder } => {
            if value.is_empty() {
                format!("[{}]", placeholder.as_deref().unwrap_or("…"))
            } else {
                format!("[{value}]")
            }
        }
        WidgetKind::Checkbox { label, checked } => {
            format!("{} {label}", if *checked { "☑" } else { "☐" })
        }
        WidgetKind::Item {
            label,
            detail,
            expanded,
            ..
        } => {
            let arrow = match expanded {
                Some(true) => "▾ ",
                Some(false) => "▸ ",
                None => "",
            };
            match detail {
                Some(d) => format!("{arrow}{label}  {d}"),
                None => format!("{arrow}{label}"),
            }
        }
        WidgetKind::Progress { value, label } => {
            let filled = (value.unwrap_or(0.0).clamp(0.0, 1.0) * 10.0).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
            match label {
                Some(l) => format!("{bar} {l}"),
                None => bar,
            }
        }
        WidgetKind::Separator => "─".repeat(12),
        WidgetKind::Column | WidgetKind::Row => String::new(),
    }
}

/// The plugins' commands of the core: answering questions, acting on a
/// panel's widgets, showing panels.
pub(crate) fn core_commands() -> Vec<Command> {
    use crate::command::{CommandHandler, CommandSource, Request, Scope};
    let cmd = |id: &str, title: &str, category: &str, when: &str, handler| Command {
        id: id.into(),
        title: title.into(),
        category: category.into(),
        default_keys: Vec::new(),
        when: Some(WhenClause::parse(when).expect("valid when-clause")),
        handler: CommandHandler::Native(handler),
        args_schema: None,
        source: CommandSource::Builtin,
        scope: Some(Scope::all()),
    };
    vec![
        cmd(ANSWER, "Answer", "Plugins", "pluginAsks", |_, args| {
            let request = args["request"]
                .as_u64()
                .ok_or_else(|| CommandError::new("No question"))?;
            let a = if let Some(yes) = args["confirmed"].as_bool() {
                Answer::Confirmed(yes)
            } else if let Some(i) = args["picked"].as_u64() {
                Answer::Picked(vec![i as u32])
            } else {
                let key = args["ask"].as_str().unwrap_or_default();
                Answer::Text(args[key].as_str().map(str::to_string))
            };
            answer(request, a);
            Ok(())
        }),
        cmd(
            PANEL_EVENT,
            "Act on a Panel",
            "Plugins",
            "pluginPanels",
            |ctx, args| {
                let (Some(id), Some(key)) = (args["panel"].as_str(), args["key"].as_str()) else {
                    return Err(CommandError::new("No panel widget"));
                };
                // An input's text, asked for.
                if let Some(text) = args["ask"].as_str().and_then(|k| args[k].as_str()) {
                    panel_event(id, key, &PanelEvent::Submitted(text.to_string()));
                    return Ok(());
                }
                let p = panel(id).ok_or_else(|| CommandError::new("No panel"))?;
                let i = p
                    .widgets
                    .iter()
                    .position(|w| w.key == key)
                    .ok_or_else(|| CommandError::new("No panel widget"))?;
                if let Some(r) = activate(&p, i) {
                    ctx.requests.push(r);
                }
                Ok(())
            },
        ),
        cmd(
            "view.pluginPanel",
            "Plugin Panel",
            "View",
            "pluginPanels",
            |ctx, args| {
                if let Some(id) = args["id"].as_str() {
                    ctx.requests
                        .push(Request::PluginPanel(Some(id.to_string())));
                    return Ok(());
                }
                let all = panels();
                if all.len() == 1 {
                    ctx.requests
                        .push(Request::PluginPanel(Some(all[0].id.clone())));
                    return Ok(());
                }
                ctx.requests.push(Request::Choose(
                    all.into_iter()
                        .map(|p| crate::palette::PaletteItem {
                            id: crate::palette::invocation(
                                "view.pluginPanel",
                                &serde_json::json!({ "id": p.id }),
                            ),
                            title: p.title,
                            category: p.plugin,
                            keys: String::new(),
                            also: String::new(),
                        })
                        .collect(),
                ));
                Ok(())
            },
        ),
        cmd(
            "view.panelActions",
            "Panel Actions",
            "View",
            "pluginPanels",
            |ctx, _| {
                let mut items = Vec::new();
                for p in panels() {
                    for i in p.actions() {
                        let w = &p.widgets[i];
                        items.push(crate::palette::PaletteItem {
                            id: crate::palette::invocation(
                                PANEL_EVENT,
                                &serde_json::json!({ "panel": p.id, "key": w.key }),
                            ),
                            title: widget_text(w),
                            category: p.title.clone(),
                            keys: String::new(),
                            also: String::new(),
                        });
                    }
                }
                ctx.requests.push(Request::Choose(items));
                Ok(())
            },
        ),
    ]
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

    #[test]
    fn questions_become_the_editors_requests() {
        use crate::command::Request;
        ask(
            7_001,
            Question::Prompt {
                title: "Name?".into(),
                value: Some("Ada".into()),
            },
        );
        ask(7_002, Question::Confirm("Sure?".into()));
        ask(
            7_003,
            Question::Pick {
                title: None,
                items: vec![("a".into(), None), ("b".into(), Some("second".into()))],
            },
        );
        assert!(asking());
        let mut ours = take_requests().into_iter().filter(|r| match r {
            Request::Ask { args, .. } => args["request"] == 7_001,
            Request::Choose(items) => items[0].id.contains("700"),
            _ => false,
        });
        match ours.next() {
            Some(Request::Ask { command, args, arg }) => {
                assert_eq!((command.as_str(), arg.as_str()), (ANSWER, "Name?"));
                assert_eq!(args["Name?_default"], "Ada");
            }
            r => panic!("{r:?}"),
        }
        match ours.next() {
            Some(Request::Choose(items)) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].category, "Sure?");
                let (cmd, args) = crate::palette::split_invocation(&items[0].id);
                assert_eq!((cmd, args["confirmed"].as_bool()), (ANSWER, Some(true)));
            }
            r => panic!("{r:?}"),
        }
        match ours.next() {
            Some(Request::Choose(items)) => {
                assert_eq!(items[1].title, "b");
                assert_eq!(items[1].category, "second");
            }
            r => panic!("{r:?}"),
        }
        withdraw(7_001);
        withdraw(7_002);
        withdraw(7_003);
    }

    #[test]
    fn a_panel_shows_its_lines_and_actions() {
        let w = |key: &str, kind: WidgetKind, children: Vec<u32>| Widget {
            key: key.into(),
            kind,
            children,
        };
        let item = |label: &str, expanded| WidgetKind::Item {
            label: label.into(),
            detail: None,
            expanded,
            selected: false,
        };
        let label = |t: &str| WidgetKind::Label {
            text: t.into(),
            style: TextStyle::Normal,
        };
        let button = |l: &str| WidgetKind::Button {
            label: l.into(),
            command: None,
        };
        let mut p = Panel {
            plugin: "p".into(),
            id: "p.panel".into(),
            title: "P".into(),
            bottom: false,
            widgets: vec![
                w("", WidgetKind::Column, vec![1, 2, 5]),
                w("title", label("Title"), vec![]),
                w("list", item("List", Some(true)), vec![3]),
                w("one", item("One", None), vec![]),
                w("hidden", label("never"), vec![]),
                w("row", WidgetKind::Row, vec![6, 7]),
                w("ok", button("OK"), vec![]),
                w("no", button("No"), vec![]),
            ],
        };
        // Widget 4 is under none: the host refuses such a tree; the lines
        // walk from the root only.
        assert_eq!(p.lines(), [(1, 0), (2, 0), (3, 1), (5, 0)]);
        assert_eq!(p.actions(), [2, 3, 6, 7]);
        assert_eq!(widget_text(&p.widgets[2]), "▾ List");
        assert_eq!(widget_text(&p.widgets[6]), "[ OK ]");
        // Collapsed, its children do not show.
        p.widgets[2].kind = item("List", Some(false));
        assert_eq!(p.lines(), [(1, 0), (2, 0), (5, 0)]);
        assert_eq!(widget_text(&p.widgets[2]), "▸ List");
    }
}
