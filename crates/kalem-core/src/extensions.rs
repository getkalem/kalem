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
//! The programs a plugin runs (`process`, API 0.2.4) run here too
//! ([`start_run`]): a thread each, their ends handed to the plugin
//! ([`process_done`]) as a fetch's response is.
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

    /// Tells the plugins watching them that settings `keys` (dotted, as
    /// `Config::changed_keys` gives them) changed.
    fn settings_changed(&mut self, keys: &[String]);

    /// Hands the response to request `id` to the plugin that sent it.
    fn respond(&mut self, id: u64, response: Result<HttpResponse, String>);

    /// Hands how run `run` ended to the plugin that started it.
    fn process_done(&mut self, run: u64, result: Result<ProcessExit, String>);
}

/// A document a plugin writes (`documents`, plugin API 0.2.5), as the
/// plugin last wrote it; the editors show it ([`crate::GeneratedDoc`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// Its number.
    pub number: u64,
    /// The plugin, by its short ID.
    pub plugin: String,
    /// The plugin's name for it (`git.status`).
    pub id: String,
    /// What tells documents of one ID apart (a repository's root).
    pub key: String,
    /// Its title.
    pub title: String,
    /// Its text type, for the scopes of commands (`git-status`).
    pub kind: String,
    /// Its highlighter (`diff`), if any.
    pub language: Option<String>,
    /// Its text.
    pub text: String,
    /// Where the cursor goes when the text is shown, if anywhere.
    pub cursor: Option<usize>,
    /// The styles of its text (`styled-documents`, API 0.2.6).
    pub styles: Vec<crate::StyleSpan>,
    /// Incremented by each write.
    pub version: u64,
}

impl Generated {
    /// The record the editors keep beside the document's text.
    pub fn doc(&self) -> crate::GeneratedDoc {
        crate::GeneratedDoc {
            number: self.number,
            plugin: self.plugin.clone(),
            kind: self.kind.clone(),
            title: self.title.clone(),
            version: self.version,
        }
    }
}

/// A program a plugin runs (`process`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRequest {
    /// The program, found already.
    pub program: std::path::PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its folder.
    pub cwd: std::path::PathBuf,
    /// What it reads on its standard input.
    pub stdin: Option<Vec<u8>>,
    /// Variables added to the environment.
    pub env: Vec<(String, String)>,
}

/// How a plugin's program ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessExit {
    /// Its exit status; none when a signal stopped it, or it was killed.
    pub status: Option<i32>,
    /// Its standard output, at most [`MAX_OUTPUT`] bytes.
    pub stdout: Vec<u8>,
    /// Its standard error, as much.
    pub stderr: Vec<u8>,
    /// Output past [`MAX_OUTPUT`] was dropped.
    pub truncated: bool,
}

/// The most of each output of a plugin's program kept (16 MB, as a file
/// read or a response).
pub const MAX_OUTPUT: usize = 16 << 20;

/// How long a plugin's program runs before it is killed.
pub const RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// An HTTP response for a plugin (`kalem.net`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The status code.
    pub status: u16,
    /// The headers, by name and value.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
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
    /// Requests for the editors besides the questions: a plugin's document
    /// to show or close.
    requests: Vec<crate::command::Request>,
    /// The menus plugins add to the menu bar.
    menus: Vec<PluginMenu>,
}

/// A menu a plugin adds to the menu bar, from its manifest's `menus`.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginMenu {
    /// The plugin, by its short ID.
    pub plugin: String,
    /// The menu's title (`Git`).
    pub title: String,
    /// Where it shows: a when-clause on the document (`vcs == git`).
    pub when: Option<WhenClause>,
    /// Its items: the plugin's commands, `-` a line between groups.
    pub items: Vec<String>,
}

/// Adds plugin menu `menu`, after the plugin's earlier ones.
pub fn add_menu(menu: PluginMenu) {
    state().menus.push(menu);
    changed();
}

/// Takes plugin `plugin`'s menus away.
pub fn remove_menus(plugin: &str) {
    state().menus.retain(|m| m.plugin != plugin);
    changed();
}

/// The menus plugins add, in the order they were added.
pub fn menus() -> Vec<PluginMenu> {
    state().menus.clone()
}

/// A plugin command's title in its menu: without its category's prefix
/// (`Git: Commit` is `Commit` in the Git menu).
pub fn menu_label(id: &str) -> String {
    let s = state();
    match s.commands.get(id) {
        Some(c) => c
            .title
            .strip_prefix(&format!("{}: ", c.category))
            .unwrap_or(&c.title)
            .to_string(),
        None => id.to_string(),
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    commands: BTreeMap::new(),
    bindings: BTreeMap::new(),
    runs: Vec::new(),
    questions: Vec::new(),
    asking: std::collections::BTreeSet::new(),
    status: BTreeMap::new(),
    panels: BTreeMap::new(),
    requests: Vec::new(),
    menus: Vec::new(),
});

fn queue_request(r: crate::command::Request) {
    state().requests.push(r);
}

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

static CONFIG: Mutex<Option<crate::settings::Config>> = Mutex::new(None);

/// The editors' settings, read by plugins (`kalem.settings`): given at
/// start and after each reload; the plugins watching a key that changed
/// are told.
pub fn set_config(config: &crate::settings::Config) {
    let changed = {
        let mut c = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let changed = c.as_ref().map(|old| config.changed_keys(old));
        *c = Some(config.clone());
        changed
    };
    if let Some(keys) = changed.filter(|k| !k.is_empty()) {
        let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(x) = installed.as_mut() {
            x.settings_changed(&keys);
        }
    }
}

/// Setting `parts` (a dotted key's parts) of the editors' settings.
pub fn setting(parts: &[&str]) -> Option<Value> {
    CONFIG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|c| c.get_path(parts).cloned())
}

/// Sets plugin `id`'s own setting `key` to `value` (`null` removes it) in
/// the user's `settings.toml`, under `[plugins."ID"]`, and has the
/// editors read the settings again.
pub fn set_own_setting(id: &str, key: &str, value: &Value) -> Result<(), String> {
    let path = crate::settings::config_dir()
        .map(|d| d.join("settings.toml"))
        .ok_or("No settings folder")?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let new = crate::settings::set_in_toml(&text, &["plugins", id, key], value)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::files::write(&path, new.as_bytes(), crate::files::SaveOptions::default())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    queue_run("help.reload", Value::Null);
    Ok(())
}

/// The folders of the projects Kalem knows, where a plugin's
/// `fs:*:workspace` permission reaches.
pub fn workspace() -> Vec<std::path::PathBuf> {
    kalem_project::Projects::load(crate::projects::list_file())
        .list
        .into_iter()
        .map(|p| p.root)
        .collect()
}

/// The runs going, by number: the flag that stops each.
static RUNS: Mutex<BTreeMap<u64, std::sync::Arc<std::sync::atomic::AtomicBool>>> =
    Mutex::new(BTreeMap::new());

/// Runs `request` as run `run` on a thread of its own; how it ends goes to
/// the plugin ([`process_done`]).
pub fn start_run(run: u64, request: ProcessRequest) {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run, stop.clone());
    let started = std::thread::Builder::new()
        .name("kalem-plugin-process".into())
        .spawn(move || {
            let result = run_program(&request, &stop, RUN_TIMEOUT);
            RUNS.lock().unwrap_or_else(|e| e.into_inner()).remove(&run);
            process_done(run, result);
        });
    if let Err(e) = started {
        RUNS.lock().unwrap_or_else(|e| e.into_inner()).remove(&run);
        fail_run(run, e.to_string());
    }
}

/// Tells the plugin that run `run` could not start, from a thread of its
/// own: the plugin's call that asked for it holds the plugins.
pub fn fail_run(run: u64, error: String) {
    let _ = std::thread::Builder::new()
        .name("kalem-plugin-process".into())
        .spawn(move || process_done(run, Err(error)));
}

/// Stops run `run`; its end is still handed over, without a status.
pub fn kill_run(run: u64) {
    if let Some(stop) = RUNS.lock().unwrap_or_else(|e| e.into_inner()).get(&run) {
        stop.store(true, Ordering::Relaxed);
    }
}

/// Runs a program and waits for it: its standard input written on a
/// thread of its own and each output read on one (so that a program
/// writing before it has read everything blocks neither side), each kept
/// up to [`MAX_OUTPUT`]; killed when `stop` is set or `timeout` passes.
/// Without a console window on Windows.
pub fn run_program(
    request: &ProcessRequest,
    stop: &std::sync::atomic::AtomicBool,
    timeout: std::time::Duration,
) -> Result<ProcessExit, String> {
    use std::io::{Read, Write};
    use std::process::Stdio;
    let mut command = std::process::Command::new(&request.program);
    command
        .args(&request.args)
        .current_dir(&request.cwd)
        .envs(request.env.iter().cloned())
        .stdin(if request.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", request.program.display()))?;
    if let (Some(mut pipe), Some(bytes)) = (child.stdin.take(), request.stdin.clone()) {
        std::thread::spawn(move || {
            let _ = pipe.write_all(&bytes);
        });
    }
    fn reader(
        pipe: Option<impl Read + Send + 'static>,
    ) -> std::thread::JoinHandle<(Vec<u8>, bool)> {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut cut = false;
            let Some(mut pipe) = pipe else {
                return (kept, cut);
            };
            let mut buf = [0u8; 64 * 1024];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let room = MAX_OUTPUT.saturating_sub(kept.len());
                        kept.extend_from_slice(&buf[..n.min(room)]);
                        cut |= n > room;
                    }
                }
            }
            (kept, cut)
        })
    }
    let out = reader(child.stdout.take());
    let err = reader(child.stderr.take());
    let started = std::time::Instant::now();
    let mut pause = std::time::Duration::from_millis(1);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if stop.load(Ordering::Relaxed) || started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(std::time::Duration::from_millis(20));
    };
    let (stdout, cut_out) = out.join().unwrap_or_default();
    let (stderr, cut_err) = err.join().unwrap_or_default();
    Ok(ProcessExit {
        status,
        stdout,
        stderr,
        truncated: cut_out || cut_err,
    })
}

/// The program `name` a plugin's manifest grants (`subprocess:NAME`), for
/// a run in `cwd`: where the user's setting `programs.NAME` in the
/// plugin's table says (`~` expanded), else found as the language server
/// client finds a server, on the `PATH`.
pub fn find_program(plugin: &str, name: &str, cwd: &std::path::Path) -> Option<std::path::PathBuf> {
    let set = setting(&["plugins", plugin, "programs", name])
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|p| !p.trim().is_empty());
    match set {
        Some(path) => kalem_lsp::find_program(path.trim(), Some(cwd), &[]),
        None => kalem_lsp::find_program(name, Some(cwd), &[]),
    }
}

/// The documents plugins write, by number.
static GENERATED: Mutex<BTreeMap<u64, Generated>> = Mutex::new(BTreeMap::new());
static GENERATED_NEXT: AtomicU64 = AtomicU64::new(0);
/// Changes with every write of a plugin's document: the editors show the
/// new text.
static GENERATED_WRITES: AtomicU64 = AtomicU64::new(0);

/// What a plugin asks to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSpec {
    /// Its name for the document (`git.status`).
    pub id: String,
    /// What tells documents of one ID apart.
    pub key: String,
    /// The title.
    pub title: String,
    /// The text type (`git-status`): not one of Kalem's.
    pub kind: String,
    /// The highlighter, if any.
    pub language: Option<String>,
}

/// Opens plugin `plugin`'s document `spec` with `text`, or writes again the
/// one of the same ID and key, and has the editors show it; its number.
pub fn open_generated(
    plugin: &str,
    spec: GeneratedSpec,
    text: String,
    cursor: Option<usize>,
    styles: Vec<crate::StyleSpan>,
) -> Result<u64, String> {
    // Its kind is the plugin's own, as its commands' IDs are: `git-…`.
    if !spec.kind.starts_with(&format!("{plugin}-")) || spec.kind.len() <= plugin.len() + 1 {
        return Err(format!(
            "A document's kind starts with the plugin's ID and a dash (`{plugin}-…`), not `{}`",
            spec.kind
        ));
    }
    let number = {
        let mut docs = GENERATED.lock().unwrap_or_else(|e| e.into_inner());
        let found = docs
            .values()
            .find(|g| g.plugin == plugin && g.id == spec.id && g.key == spec.key)
            .map(|g| g.number);
        let number = found.unwrap_or_else(|| GENERATED_NEXT.fetch_add(1, Ordering::Relaxed) + 1);
        let version = docs.get(&number).map_or(0, |g| g.version) + 1;
        docs.insert(
            number,
            Generated {
                number,
                plugin: plugin.to_string(),
                id: spec.id,
                key: spec.key,
                title: spec.title,
                kind: spec.kind,
                language: spec.language,
                text,
                cursor,
                styles,
                version,
            },
        );
        number
    };
    GENERATED_WRITES.fetch_add(1, Ordering::Relaxed);
    queue_request(crate::command::Request::ShowGenerated(number));
    Ok(number)
}

/// Writes plugin `plugin`'s document `number` again.
pub fn set_generated(
    plugin: &str,
    number: u64,
    text: String,
    cursor: Option<usize>,
    styles: Vec<crate::StyleSpan>,
) -> Result<(), String> {
    let mut docs = GENERATED.lock().unwrap_or_else(|e| e.into_inner());
    let g = docs
        .get_mut(&number)
        .filter(|g| g.plugin == plugin)
        .ok_or_else(|| format!("The plugin has no document {number}: closed"))?;
    g.text = text;
    g.cursor = cursor;
    g.styles = styles;
    g.version += 1;
    drop(docs);
    GENERATED_WRITES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Plugin `plugin` closes its document `number`.
pub fn close_generated(plugin: &str, number: u64) {
    let owned = GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&number)
        .is_some_and(|g| g.plugin == plugin);
    if owned {
        queue_request(crate::command::Request::CloseGenerated(number));
    }
}

/// A plugin's document as last written.
pub fn generated(number: u64) -> Option<Generated> {
    GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&number)
        .cloned()
}

/// Changes with every write of a plugin's document.
pub fn generated_writes() -> u64 {
    GENERATED_WRITES.load(Ordering::Relaxed)
}

/// The editors closed document `number`: forgotten, its plugin's next
/// write refused.
pub fn generated_closed(number: u64) {
    GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&number);
}

/// A plugin's documents are closed with it.
pub fn close_plugin_documents(plugin: &str) {
    let numbers: Vec<u64> = GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter(|g| g.plugin == plugin)
        .map(|g| g.number)
        .collect();
    for n in numbers {
        queue_request(crate::command::Request::CloseGenerated(n));
    }
}

/// The marks plugins set beside the lines of files (`decorations`), by
/// file: the version of its marks, and each plugin's.
#[allow(clippy::type_complexity)]
static GUTTERS: Mutex<
    BTreeMap<std::path::PathBuf, (u64, BTreeMap<String, Vec<(u32, crate::GutterMark)>>)>,
> = Mutex::new(BTreeMap::new());
/// Changes with every write of marks: the documents take theirs again.
static GUTTER_WRITES: AtomicU64 = AtomicU64::new(0);

/// Marks a plugin sets in a file at most.
pub const MAX_MARKS: usize = 10_000;

/// A file as the marks are kept by: its real path when it has one.
fn gutter_key(path: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Plugin `plugin` marks the lines of file `path` (lines from 1, what
/// changed there), in place of its earlier marks there.
pub fn set_gutter(
    plugin: &str,
    path: &std::path::Path,
    marks: Vec<(u32, crate::GutterMark)>,
) -> Result<(), String> {
    if marks.len() > MAX_MARKS {
        return Err(format!(
            "At most {MAX_MARKS} marks a file, not {}",
            marks.len()
        ));
    }
    let key = gutter_key(path);
    let mut g = GUTTERS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = g.entry(key).or_default();
    let had = entry.1.get(plugin);
    if had.is_none_or(Vec::is_empty) && marks.is_empty() || had == Some(&marks) {
        return Ok(());
    }
    if marks.is_empty() {
        entry.1.remove(plugin);
    } else {
        entry.1.insert(plugin.to_string(), marks);
    }
    entry.0 = GUTTER_WRITES.fetch_add(1, Ordering::Relaxed) + 1;
    Ok(())
}

/// Plugin `plugin` takes its marks away from file `path`, or from every
/// file.
pub fn clear_gutter(plugin: &str, path: Option<&std::path::Path>) {
    let key = path.map(gutter_key);
    let mut g = GUTTERS.lock().unwrap_or_else(|e| e.into_inner());
    for (file, entry) in g.iter_mut() {
        if key.as_ref().is_some_and(|k| k != file) {
            continue;
        }
        if entry.1.remove(plugin).is_some() {
            entry.0 = GUTTER_WRITES.fetch_add(1, Ordering::Relaxed) + 1;
        }
    }
}

/// The marks of file `path`, every plugin's, with the version of the
/// file's marks (0 when none was ever set).
pub fn gutter(path: &std::path::Path) -> (u64, Vec<(u32, crate::GutterMark)>) {
    let g = GUTTERS.lock().unwrap_or_else(|e| e.into_inner());
    let Some((version, by_plugin)) = g.get(&gutter_key(path)) else {
        return (0, Vec::new());
    };
    let mut v: Vec<(u32, crate::GutterMark)> = by_plugin.values().flatten().copied().collect();
    v.sort();
    (*version, v)
}

/// Changes with every write of marks.
pub fn gutter_writes() -> u64 {
    GUTTER_WRITES.load(Ordering::Relaxed)
}

/// Hands how run `run` ended to its plugin (from the thread that ran it).
pub fn process_done(run: u64, result: Result<ProcessExit, String>) {
    let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = installed.as_mut() {
        x.process_done(run, result);
    }
}

/// Hands the response to request `id` to its plugin (from the thread
/// that fetched it).
pub fn respond(id: u64, response: Result<HttpResponse, String>) {
    let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = installed.as_mut() {
        x.respond(id, response);
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
    // Both lists under one lock: a second `state()` in this statement
    // would wait for the first's guard.
    let (questions, requests) = {
        let mut s = state();
        (
            std::mem::take(&mut s.questions),
            std::mem::take(&mut s.requests),
        )
    };
    questions
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
        .chain(requests)
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
        when: Some(crate::builtin::literal_when(when)),
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

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    fn sh(script: &str, stdin: Option<&str>) -> ProcessRequest {
        ProcessRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            stdin: stdin.map(|s| s.as_bytes().to_vec()),
            env: vec![("KALEM_TEST_VAR".into(), "set".into())],
        }
    }

    #[test]
    fn a_program_reads_its_input_and_ends_with_its_status() {
        let stop = AtomicBool::new(false);
        let e = run_program(
            &sh(
                "cat; echo \"$KALEM_TEST_VAR\" >&2; pwd >&2; exit 3",
                Some("hello"),
            ),
            &stop,
            RUN_TIMEOUT,
        )
        .unwrap();
        assert_eq!(e.status, Some(3));
        assert_eq!(e.stdout, b"hello");
        let err = String::from_utf8(e.stderr).unwrap();
        let dir = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(err, format!("set\n{}\n", dir.display()));
        assert!(!e.truncated);
    }

    #[test]
    fn output_past_the_limit_is_cut_and_the_program_still_ends() {
        let stop = AtomicBool::new(false);
        let script = format!("head -c {} /dev/zero", MAX_OUTPUT + 1000);
        let e = run_program(&sh(&script, None), &stop, RUN_TIMEOUT).unwrap();
        assert_eq!(e.status, Some(0));
        assert_eq!(e.stdout.len(), MAX_OUTPUT);
        assert!(e.truncated);
    }

    #[test]
    fn a_program_is_killed_when_asked_or_late() {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            s.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let e = run_program(&sh("exec sleep 30", None), &stop, RUN_TIMEOUT).unwrap();
        assert_eq!(e.status, None);
        assert!(started.elapsed() < Duration::from_secs(10));
        let started = Instant::now();
        let never = AtomicBool::new(false);
        let e = run_program(
            &sh("exec sleep 30", None),
            &never,
            Duration::from_millis(100),
        )
        .unwrap();
        assert_eq!(e.status, None);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn plugins_documents_are_kept_until_closed() {
        let spec = |key: &str, kind: &str| GeneratedSpec {
            id: "gentests.status".into(),
            key: key.into(),
            title: "Status".into(),
            kind: kind.into(),
            language: Some("diff".into()),
        };
        let open = |key: &str, kind: &str, text: &str| {
            open_generated("gentests", spec(key, kind), text.into(), None, Vec::new())
        };
        // Its kind is the plugin's own.
        for kind in ["git-status", "gentests", "gentests-", "status"] {
            assert!(open("/a", kind, "").is_err(), "{kind}");
        }
        let writes = generated_writes();
        let n = open("/a", "gentests-status", "one").unwrap();
        assert!(generated_writes() > writes);
        let g = generated(n).unwrap();
        assert_eq!(
            (g.text.as_str(), g.version, g.plugin.as_str()),
            ("one", 1, "gentests")
        );
        assert_eq!(g.doc().kind, "gentests-status");
        // The same ID and key is that document, written anew.
        assert_eq!(open("/a", "gentests-status", "two"), Ok(n));
        assert_eq!(
            generated(n).map(|g| (g.text, g.version)),
            Some(("two".into(), 2))
        );
        let other = open("/b", "gentests-status", "").unwrap();
        assert_ne!(other, n);
        // Only its plugin writes it.
        assert!(set_generated("another", n, "x".into(), None, Vec::new()).is_err());
        set_generated("gentests", n, "three".into(), Some(2), Vec::new()).unwrap();
        let g = generated(n).unwrap();
        assert_eq!(
            (g.text.as_str(), g.cursor, g.version),
            ("three", Some(2), 3)
        );
        // Closed in the editor, it is forgotten; a write says so.
        generated_closed(n);
        assert_eq!(generated(n), None);
        let e = set_generated("gentests", n, "late".into(), None, Vec::new()).unwrap_err();
        assert!(e.contains("closed"), "{e}");
        generated_closed(other);
    }

    #[test]
    fn marks_are_kept_by_file_and_plugin() {
        use crate::GutterMark as M;
        let dir = std::env::temp_dir().join(format!("kalem-gutter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        let writes = gutter_writes();
        assert_eq!(gutter(&file), (0, Vec::new()));
        set_gutter("gutterone", &file, vec![(2, M::Changed)]).unwrap();
        set_gutter("guttertwo", &file, vec![(1, M::Added)]).unwrap();
        assert!(gutter_writes() >= writes + 2);
        let (v1, marks) = gutter(&file);
        assert_eq!(marks, [(1, M::Added), (2, M::Changed)]);
        // The same marks again change nothing; a path through `..` is the
        // same file.
        set_gutter("gutterone", &file, vec![(2, M::Changed)]).unwrap();
        let same = dir.join("..").join(dir.file_name().unwrap()).join("a.txt");
        assert_eq!(gutter(&same).0, v1);
        assert!(set_gutter("gutterone", &file, vec![(1, M::Added); MAX_MARKS + 1]).is_err());
        clear_gutter("guttertwo", None);
        let (v2, marks) = gutter(&file);
        assert!(v2 > v1);
        assert_eq!(marks, [(2, M::Changed)]);
        clear_gutter("gutterone", Some(&file));
        assert!(gutter(&file).1.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let stop = AtomicBool::new(false);
        let mut r = sh("", None);
        r.program = "/nonexistent/kalem-no-such-program".into();
        assert!(run_program(&r, &stop, RUN_TIMEOUT).is_err());
    }

    #[test]
    fn programs_are_found_on_the_path_or_where_the_setting_says() {
        let dir = std::env::temp_dir();
        assert!(find_program("org.example.none", "sh", &dir).is_some());
        assert_eq!(
            find_program("org.example.none", "kalem-no-such-program", &dir),
            None
        );
        let config = crate::settings::Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[plugins.\"org.example.find\".programs]\nkalemgit = \"/bin/sh\"\n",
        )]);
        set_config(&config);
        assert_eq!(
            find_program("org.example.find", "kalemgit", &dir),
            Some(std::path::PathBuf::from("/bin/sh"))
        );
    }
}
