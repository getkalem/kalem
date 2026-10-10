//! The language servers of open documents (D57; T3.8.1, T3.8.2): which
//! server serves a file (from the language plugins, [`crate::languages`]),
//! one server per plugin, server and root, documents kept in step with
//! it, crashes restarted with backoff, and the answers the frontends show.
//!
//! The frontends call [`sync`] after a document changes (and when it is
//! opened), [`saved`] and [`closed`], and [`tick`] on their timer; the
//! commands start requests ([`request`]) whose answers come back as
//! [`Outcome`]s from [`take_outcomes`]. Nothing here waits for a server
//! on the frontend's thread except completion, which is a slow completer
//! on its own thread with its budget.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use kalem_lsp::features::Severity;
use kalem_lsp::features::{self, Diagnostic};
use kalem_lsp::{Client, Edit, Encoding, Event, Pending, Position, ServerConfig, Wake};
use serde_json::{Value, json};

use crate::DocumentState;
use crate::languages::{self, LanguageSpec, Plugin, Resolved};

/// A server: the plugin, the server's key, the root.
type Key = (String, String, PathBuf);

/// A document's diagnostics in byte ranges and lines, with what they were
/// read for.
struct DiagCache {
    /// The client (its address), its publication count, the text's
    /// change count.
    key: (usize, u64, u64),
    list: Vec<Diagnostic>,
    /// The line of each diagnostic's start.
    lines: Vec<usize>,
    /// Made for an older version of the text: places may have moved.
    stale: bool,
    /// The most serious diagnostic starting on each line, for the gutter.
    by_line: HashMap<usize, Severity>,
}

impl Doc {
    /// The line starts of the text.
    fn line_starts(&mut self) -> &[usize] {
        if self.lines.0 != self.text_rev {
            let starts = std::iter::once(0)
                .chain(self.text.match_indices('\n').map(|(i, _)| i + 1))
                .collect();
            self.lines = (self.text_rev, starts);
        }
        &self.lines.1
    }

    /// The line of byte `at`.
    fn line_of(&mut self, at: usize) -> usize {
        self.line_starts()
            .partition_point(|&s| s <= at)
            .saturating_sub(1)
    }

    /// The diagnostics, read again only when the text or the server's
    /// diagnostics changed (the status bar asks on every frame).
    fn diagnostics(&mut self) -> Option<&DiagCache> {
        let c = self.opened_in.clone()?;
        let key = (Arc::as_ptr(&c) as usize, c.published(), self.text_rev);
        if self.diags.as_ref().is_none_or(|d| d.key != key) {
            let list = features::diagnostics(&self.text, &c.diagnostics(&self.uri), c.encoding());
            let lines: Vec<usize> = list.iter().map(|d| self.line_of(d.range.start)).collect();
            let mut by_line: HashMap<usize, Severity> = HashMap::new();
            for (d, &l) in list.iter().zip(&lines) {
                let worst = by_line.entry(l).or_insert(d.severity);
                // `Severity` orders Error first.
                *worst = (*worst).min(d.severity);
            }
            self.diags = Some(DiagCache {
                key,
                list,
                lines,
                stale: c.diagnostics_stale(&self.uri),
                by_line,
            });
        }
        self.diags.as_ref()
    }
}

struct Slot {
    client: Option<Arc<Client>>,
    name: String,
    /// Starts after a crash, for the backoff.
    crashes: u32,
    retry_at: Option<Instant>,
    /// Why it is not running, when it is not.
    failed: Option<String>,
    /// When its process last started: a server that ran a while before
    /// it stopped has its crashes forgotten.
    started: Instant,
    /// The root watched for files changed outside the editor, told to the
    /// server in batches.
    watch: Option<RootWatch>,
}

/// A server's root watched for changes made outside the editor (a
/// checkout, `mix deps.get`, another editor).
struct RootWatch {
    _watcher: notify::RecommendedWatcher,
    events: std::sync::mpsc::Receiver<(PathBuf, u8)>,
    /// Changes gathered, by file: created 1, changed 2, deleted 3.
    pending: HashMap<PathBuf, u8>,
    /// When the first gathered change came.
    since: Option<Instant>,
}

/// Folders of build output, dependencies and tools: their changes are not
/// the project's sources.
const UNWATCHED: &[&str] = &[
    "_build",
    "deps",
    ".git",
    ".elixir_ls",
    ".expert",
    "node_modules",
    "target",
    ".hg",
];

/// Changes are sent this long after the first of a batch.
const WATCH_BATCH: Duration = Duration::from_millis(250);

impl RootWatch {
    fn start(root: &Path) -> Option<RootWatch> {
        use notify::Watcher;
        let (tx, events) = std::sync::mpsc::channel();
        let root_owned = root.to_path_buf();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            let kind = match event.kind {
                notify::EventKind::Create(_) => 1,
                notify::EventKind::Modify(_) => 2,
                notify::EventKind::Remove(_) => 3,
                _ => return,
            };
            for p in event.paths {
                let skip = p.strip_prefix(&root_owned).ok().is_some_and(|rel| {
                    rel.components().any(|c| {
                        UNWATCHED
                            .iter()
                            .any(|u| c.as_os_str() == std::ffi::OsStr::new(u))
                    })
                });
                if !skip {
                    let _ = tx.send((p, kind));
                }
            }
        })
        .ok()?;
        watcher.watch(root, notify::RecursiveMode::Recursive).ok()?;
        Some(RootWatch {
            _watcher: watcher,
            events,
            pending: HashMap::new(),
            since: None,
        })
    }

    /// The batch due, if any: the files and what happened to each.
    fn due(&mut self, now: Instant) -> Vec<(PathBuf, u8)> {
        while let Ok((p, kind)) = self.events.try_recv() {
            self.since.get_or_insert(now);
            let e = self.pending.entry(p).or_insert(kind);
            // A file created then changed is created; deleted wins.
            if kind == 3 || *e != 1 {
                *e = kind;
            }
        }
        if self
            .since
            .is_some_and(|t| now.duration_since(t) >= WATCH_BATCH)
        {
            self.since = None;
            return self.pending.drain().collect();
        }
        Vec::new()
    }
}

/// How long a server may take to answer `initialize`.
const INIT_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a server runs before its earlier crashes are forgotten.
const CRASHES_FORGOTTEN: Duration = Duration::from_secs(10 * 60);

struct Doc {
    uri: String,
    /// The file with links resolved, as servers name it (`/tmp` is
    /// `/private/tmp` on macOS): the URI is made from it, and places in
    /// answers are mapped back to the path the editor opened.
    real: PathBuf,
    language: LanguageSpec,
    plugin: Arc<Plugin>,
    /// `None` when no server serves it (the reason is in `missing`).
    key: Option<Key>,
    missing: Option<String>,
    /// The text the server has.
    text: String,
    /// The document's version when it was last compared.
    version: u64,
    /// The editor's document ([`DocumentState::serial`]): a rename or a
    /// Save As gives it another path, and the old one is closed.
    serial: u64,
    /// Changes of `text`, for the caches below.
    text_rev: u64,
    /// The text's line starts, for `text_rev`.
    lines: (u64, Vec<usize>),
    /// The diagnostics read against the text, until the text or the
    /// server's diagnostics change.
    diags: Option<DiagCache>,
    opened_in: Option<Arc<Client>>,
}

/// What a request was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The documentation at the cursor.
    Hover,
    /// Where the thing at the cursor is defined.
    Definition,
    /// Where it is declared.
    Declaration,
    /// Where its type is defined.
    TypeDefinition,
    /// Its implementations.
    Implementation,
    /// Its uses.
    References,
    /// The document formatted.
    Format,
    /// The document's symbols, as a list to jump to.
    Symbols,
    /// The signature of the call the cursor is in (asked as `(` or `,` is
    /// typed).
    Signature,
    /// The edits renaming the symbol at the cursor.
    Rename,
    /// The code actions at the cursor or the selection.
    CodeAction,
    /// A chosen code action's edit, fetched before it is applied.
    ResolveAction,
}

impl Kind {
    fn method(self) -> &'static str {
        match self {
            Kind::Hover => "textDocument/hover",
            Kind::Definition => "textDocument/definition",
            Kind::Declaration => "textDocument/declaration",
            Kind::TypeDefinition => "textDocument/typeDefinition",
            Kind::Implementation => "textDocument/implementation",
            Kind::References => "textDocument/references",
            Kind::Format => "textDocument/formatting",
            Kind::Symbols => "textDocument/documentSymbol",
            Kind::Signature => "textDocument/signatureHelp",
            Kind::Rename => "textDocument/rename",
            Kind::CodeAction => "textDocument/codeAction",
            Kind::ResolveAction => "codeAction/resolve",
        }
    }

    fn provider(self) -> &'static str {
        match self {
            Kind::Hover => "hoverProvider",
            Kind::Definition => "definitionProvider",
            Kind::Declaration => "declarationProvider",
            Kind::TypeDefinition => "typeDefinitionProvider",
            Kind::Implementation => "implementationProvider",
            Kind::References => "referencesProvider",
            Kind::Format => "documentFormattingProvider",
            Kind::Symbols => "documentSymbolProvider",
            Kind::Signature => "signatureHelpProvider",
            Kind::Rename => "renameProvider",
            Kind::CodeAction | Kind::ResolveAction => "codeActionProvider",
        }
    }

    /// What it asks for, in the interface language.
    fn what(self) -> String {
        crate::l10n::tr(match self {
            Kind::Hover => "lsp-what-documentation",
            Kind::Definition => "lsp-what-definitions",
            Kind::Declaration => "lsp-what-declarations",
            Kind::TypeDefinition => "lsp-what-type-definitions",
            Kind::Implementation => "lsp-what-implementations",
            Kind::References => "lsp-what-references",
            Kind::Format => "lsp-what-formatting",
            Kind::Symbols => "lsp-what-symbols",
            Kind::Signature => "lsp-what-signature",
            Kind::Rename => "lsp-what-rename",
            Kind::CodeAction | Kind::ResolveAction => "lsp-what-code-actions",
        })
    }
}

/// A place to go to, with the line it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The file.
    pub path: PathBuf,
    /// The line, from 1.
    pub line: u64,
    /// The byte in the line.
    pub column: usize,
    /// The line's text, trimmed (for the list).
    pub preview: String,
}

/// An answer for the frontend.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A line for the status bar.
    Message {
        /// The text.
        text: String,
        /// An error.
        error: bool,
    },
    /// Documentation to show beside the cursor of `path`.
    Hover {
        /// The document.
        path: PathBuf,
        /// Markdown.
        text: String,
    },
    /// Go to a place.
    Jump(Place),
    /// Places to choose from.
    Places {
        /// What they are: `References`.
        title: String,
        /// The places.
        places: Vec<Place>,
    },
    /// A list to choose from (a rename to confirm, the code actions).
    Choose(Vec<crate::palette::PaletteItem>),
    /// The signature of the call being typed, beside the cursor of
    /// `path` while the call is open; `None` closes it.
    Signature {
        /// The document.
        path: PathBuf,
        /// The signature, its active parameter's documentation after it.
        text: Option<String>,
    },
    /// Edits to apply to `path` if it is still at `version`.
    Edits {
        /// The document.
        path: PathBuf,
        /// Its version when asked.
        version: u64,
        /// Sorted, non-overlapping, in the text of that version.
        edits: Vec<(Range<usize>, String)>,
        /// The undo label.
        label: String,
    },
}

struct Action {
    kind: Kind,
    path: PathBuf,
    version: u64,
    text: String,
    enc: Encoding,
    pending: Pending,
    client: Arc<Client>,
    started: Instant,
    /// What was asked, sent again when the server answers that its state
    /// changed under it ([`kalem_lsp::CONTENT_MODIFIED`]).
    params: Value,
    /// How many times it was asked again.
    retries: u32,
    /// When to ask again.
    retry_at: Option<Instant>,
}

/// How many times a request the server cancelled is asked again (after
/// half a second, one, two): rust-analyzer cancels what it is asked while
/// it loads the project.
const RETRIES: u32 = 3;

#[derive(Default)]
struct Service {
    docs: HashMap<PathBuf, Doc>,
    servers: HashMap<Key, Slot>,
    actions: Vec<Action>,
    /// Answers waiting for their document's editor to take them.
    outcomes: Vec<Answer>,
    /// Files no language plugin serves, not looked up again until the
    /// plugins or the settings change.
    unserved: std::collections::HashSet<PathBuf>,
    /// Files a server named in its answers outside its root (a
    /// definition in the standard library, a dependency's source), by
    /// their real paths: the server that named each, which serves it
    /// when it opens ([`Service::named_by`]).
    named: HashMap<PathBuf, Key>,
}

/// Files [`Service::named`] keeps at most: past it, it starts again.
const NAMED_MAX: usize = 20_000;

/// An answer for a document, until its editor takes it or it expires.
struct Answer {
    path: PathBuf,
    /// The document's version when it was asked.
    version: u64,
    at: Instant,
    outcome: Outcome,
}

/// How long an answer waits for its editor (a document closed or hidden
/// meanwhile never takes it).
const ANSWER_TTL: Duration = Duration::from_secs(15);

/// What a server says on its own (a crash, a warning), for the status bar
/// of the active editor whatever its document.
fn notice(text: String, error: bool) {
    crate::jobs::notice(text, error);
}

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Service) -> R) -> R {
    let mut g = SERVICE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(g.get_or_insert_with(Service::default))
}

/// The frontends look at the servers on their timer ([`tick`]), so the
/// client's wake-up does nothing.
fn wake() -> Wake {
    Arc::new(|| {})
}

/// Servers are running: the terminal editor then looks at them sooner
/// than at its idle pace.
pub fn active() -> bool {
    with(|s| s.servers.values().any(|slot| slot.client.is_some()))
}

/// How long a request may take before it is given up.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Restarts after crashes before giving up.
const MAX_CRASHES: u32 = 5;

fn code_file(doc: &DocumentState) -> Option<&Path> {
    match doc.meta.mode {
        crate::mode::DocumentMode::Text { .. } => doc.meta.path.as_deref(),
        _ => None,
    }
}

fn start_client(
    plugin: &Plugin,
    lang: &LanguageSpec,
    root: &Path,
) -> Result<(String, Client), String> {
    match languages::resolve_server(plugin, lang, Some(root)) {
        Resolved::Off => Err(crate::tr!("lsp-off", plugin = plugin.name.as_str())),
        Resolved::Missing(why) => Err(why),
        Resolved::Found(spec, program, args) => {
            let mut folders = Vec::new();
            // An umbrella project's applications are folders of its own.
            if spec.root_outermost
                && let Ok(rd) = std::fs::read_dir(root.join("apps"))
            {
                let mut apps: Vec<PathBuf> = rd
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| spec.root_markers.iter().any(|m| p.join(m).exists()))
                    .collect();
                apps.sort();
                if !apps.is_empty() {
                    folders.push(root.to_path_buf());
                    folders.extend(apps);
                }
            }
            let config = ServerConfig {
                name: spec.name.clone(),
                command: program,
                args,
                env: spec.env.clone(),
                root: root.to_path_buf(),
                folders,
                initialization_options: spec.initialization_options.clone(),
                settings: languages::server_settings(plugin, &spec),
                busy_start: spec.busy_log.0.clone(),
                busy_done: spec.busy_log.1.clone(),
            };
            tracing::info!(server = %spec.name, root = %root.display(), "starting language server");
            Client::start(config, wake())
                .map(|c| (spec.name.clone(), c))
                .map_err(|e| {
                    crate::tr!(
                        "lsp-did-not-start",
                        server = spec.name.as_str(),
                        reason = e.to_string()
                    )
                })
        }
    }
}

/// Why `path` gets no server when its language's server needs a root
/// (`requireRoot`) and no folder up from it has a marker.
fn outside_root(path: &Path, plugin: &Plugin, lang: &LanguageSpec) -> Option<String> {
    let spec = lang.servers.iter().find_map(|k| plugin.server(k))?;
    if !spec.require_root || kalem_lsp::find_root(path, &spec.root_markers, false).is_some() {
        return None;
    }
    Some(crate::tr!(
        "lsp-outside-root",
        markers = spec.root_markers.join(", ")
    ))
}

fn root_of(path: &Path, plugin: &Plugin, lang: &LanguageSpec) -> PathBuf {
    let spec = lang.servers.iter().find_map(|k| plugin.server(k));
    let (markers, outer) = spec.map_or((Vec::new(), false), |s| {
        (s.root_markers.clone(), s.root_outermost)
    });
    kalem_lsp::find_root(path, &markers, outer)
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

impl Service {
    fn client_for(
        &mut self,
        key: &Key,
        plugin: &Plugin,
        lang: &LanguageSpec,
    ) -> Result<Arc<Client>, String> {
        if let Some(slot) = self.servers.get(key) {
            if let Some(c) = &slot.client {
                return Ok(c.clone());
            }
            return Err(slot
                .failed
                .clone()
                .unwrap_or_else(|| crate::tr!("lsp-restarting", server = slot.name.as_str())));
        }
        match start_client(plugin, lang, &key.2) {
            Ok((name, c)) => {
                let c = Arc::new(c);
                self.servers.insert(
                    key.clone(),
                    Slot {
                        client: Some(c.clone()),
                        name,
                        crashes: 0,
                        retry_at: None,
                        failed: None,
                        started: Instant::now(),
                        watch: RootWatch::start(&key.2),
                    },
                );
                Ok(c)
            }
            Err(e) => {
                self.servers.insert(
                    key.clone(),
                    Slot {
                        client: None,
                        name: key.1.clone(),
                        crashes: MAX_CRASHES,
                        retry_at: None,
                        failed: Some(e.clone()),
                        started: Instant::now(),
                        watch: None,
                    },
                );
                Err(e)
            }
        }
    }

    fn open(&mut self, path: &Path, text: &str) {
        let first = text.lines().next();
        let Some((plugin, language)) = languages::for_path(path, first) else {
            self.unserved.insert(path.to_path_buf());
            return;
        };
        let real = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let root = root_of(&real, &plugin, &language);
        let (key, missing) = match self.named_by(&real, &plugin, &language, &root) {
            Some(key) => (Some(key), None),
            None => {
                let outside = outside_root(&real, &plugin, &language);
                let resolved = match &outside {
                    Some(why) => Resolved::Missing(why.clone()),
                    None => languages::resolve_server(&plugin, &language, Some(&root)),
                };
                match resolved {
                    Resolved::Found(spec, ..) => (Some((plugin.id.clone(), spec.key, root)), None),
                    Resolved::Off => (None, None),
                    Resolved::Missing(m) => (None, Some(m)),
                }
            }
        };
        let mut doc = Doc {
            uri: kalem_lsp::uri::from_path(&real),
            real,
            language,
            plugin,
            key,
            missing,
            text: text.to_string(),
            version: 0,
            serial: 0,
            text_rev: 0,
            lines: (u64::MAX, Vec::new()),
            diags: None,
            opened_in: None,
        };
        if let Some(key) = doc.key.clone() {
            match self.client_for(&key, &doc.plugin, &doc.language) {
                Ok(c) => {
                    c.did_open(&doc.uri, &doc.language.id, text);
                    doc.opened_in = Some(c);
                }
                Err(e) => {
                    doc.missing = Some(e.clone());
                    notice(e, true);
                }
            }
        }
        self.docs.insert(path.to_path_buf(), doc);
    }
}

/// Keeps the server of `doc` in step: opens it the first time, sends the
/// change since the last call after that. Cheap for documents no server
/// serves.
pub fn sync(doc: &DocumentState) {
    let Some(path) = code_file(doc) else { return };
    let text = doc.text().as_str();
    let version = doc.version();
    with(|s| {
        if s.unserved.contains(path) {
            return;
        }
        let Some(d) = s.docs.get_mut(path) else {
            s.open(path, text);
            if let Some(d) = s.docs.get_mut(path) {
                d.version = version;
                d.serial = doc.serial();
            }
            // The same document under its old path (renamed, moved, saved
            // as): closed there, after the new one opened so a shared
            // server keeps running.
            let old: Vec<PathBuf> = s
                .docs
                .iter()
                .filter(|(p, d)| d.serial == doc.serial() && p.as_path() != path)
                .map(|(p, _)| p.clone())
                .collect();
            for p in old {
                s.outcomes.retain(|a| a.path != p);
                if let Some(d) = s.docs.remove(&p)
                    && let Some(c) = d.opened_in
                {
                    c.did_close(&d.uri);
                    if c.open_documents() == 0
                        && let Some(k) = d.key
                    {
                        s.servers.remove(&k);
                        std::thread::spawn(move || c.shutdown());
                    }
                }
            }
            return;
        };
        if d.version == version {
            return;
        }
        d.version = version;
        let mut typed: Option<(String, usize)> = None;
        if let Some(c) = &d.opened_in {
            let (a, b) = (d.text.as_bytes(), text.as_bytes());
            let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
            while !d.text.is_char_boundary(pre) || !text.is_char_boundary(pre) {
                pre -= 1;
            }
            let max = a.len().min(b.len()) - pre;
            let mut suf = a
                .iter()
                .rev()
                .zip(b.iter().rev())
                .take(max)
                .take_while(|(x, y)| x == y)
                .count();
            while !d.text.is_char_boundary(a.len() - suf) || !text.is_char_boundary(b.len() - suf) {
                suf -= 1;
            }
            if pre == a.len() && pre == b.len() {
                // An undo back to the same text, a change and its revert.
                return;
            }
            let edit = Edit {
                range: pre..a.len() - suf,
                text: text[pre..b.len() - suf].to_string(),
            };
            c.did_change(&d.uri, &d.text, std::slice::from_ref(&edit), text);
            // Typing a call's `(` or `,` asks for its signature; its `)`
            // closes it.
            if edit.range.is_empty() {
                typed = Some((edit.text, edit.range.start));
            }
        } else if d.text == text {
            return;
        }
        d.text = text.to_string();
        d.text_rev += 1;
        match typed {
            Some((t, at)) if t == "(" || t == "," => {
                let can = d
                    .opened_in
                    .as_ref()
                    .is_some_and(|c| c.is_ready() && c.provides(Kind::Signature.provider()));
                if can {
                    let _ = s.ask(path, Kind::Signature, at + t.len(), version, Some(&t));
                }
            }
            Some((t, _)) if t == ")" => s.outcomes.push(Answer {
                path: path.to_path_buf(),
                version,
                at: Instant::now(),
                outcome: Outcome::Signature {
                    path: path.to_path_buf(),
                    text: None,
                },
            }),
            _ => {}
        }
    });
}

/// `doc` was saved.
pub fn saved(doc: &DocumentState) {
    sync(doc);
    let Some(path) = code_file(doc) else { return };
    with(|s| {
        if let Some(d) = s.docs.get(path)
            && let Some(c) = &d.opened_in
        {
            c.did_save(&d.uri, &d.text);
        }
    });
}

/// The document at `path` was closed; its server stops with its last
/// document.
pub fn closed(path: &Path) {
    let stop = with(|s| {
        s.outcomes.retain(|a| a.path != path);
        let d = s.docs.remove(path)?;
        let c = d.opened_in?;
        c.did_close(&d.uri);
        if c.open_documents() == 0 {
            let key = d.key?;
            s.servers.remove(&key);
            return Some(c);
        }
        None
    });
    if let Some(c) = stop {
        // `shutdown` waits a moment for the server; not on the caller's
        // thread.
        std::thread::spawn(move || c.shutdown());
    }
}

/// Moves the servers on: their events into notices, crashed servers
/// restarted, answers read. True when there is something to redraw.
pub fn tick() -> bool {
    let (changed, finished, aliases, open) = with(|s| {
        let changed = s.watch_servers(Instant::now());
        s.outcomes.retain(|a| a.at.elapsed() < ANSWER_TTL);
        let finished = s.finished_actions();
        if finished.is_empty() {
            return (changed, finished, HashMap::new(), HashMap::new());
        }
        s.remember_named(&finished);
        let (aliases, open) = s.answer_context(&finished);
        (true, finished, aliases, open)
    });
    if finished.is_empty() {
        return changed;
    }
    // Read without the lock: places in files not open come from the disk.
    let now = Instant::now();
    let answers: Vec<Answer> = finished
        .into_iter()
        .map(|(a, r)| Answer {
            path: a.path.clone(),
            version: a.version,
            at: now,
            outcome: outcome(&a, r, &aliases, &open),
        })
        .collect();
    with(|s| s.outcomes.extend(answers));
    true
}

/// A request's answer, or why there is none.
type Answered = Result<Value, kalem_lsp::RpcError>;

impl Service {
    /// The servers' events: what they say, their exits (restarted with
    /// backoff, given up after [`MAX_CRASHES`]), and restarts due. True
    /// when something changed.
    fn watch_servers(&mut self, now: Instant) -> bool {
        let mut changed = false;
        // Servers given up on: their documents say why.
        let mut gave_up: Vec<(Key, String)> = Vec::new();
        let mut server_edits: Vec<(Arc<Client>, Value, Value)> = Vec::new();
        let keys: Vec<Key> = self.servers.keys().cloned().collect();
        for key in keys {
            let Some(slot) = self.servers.get_mut(&key) else {
                continue;
            };
            let Some(c) = slot.client.clone() else {
                if slot.retry_at.is_some_and(|t| t <= now) {
                    slot.retry_at = None;
                    self.restart_slot(&key);
                    changed = true;
                }
                continue;
            };
            // A server that never answers `initialize` is ended, and
            // treated as one that stopped.
            if !c.is_ready() && !c.has_exited() && slot.started.elapsed() > INIT_TIMEOUT {
                c.kill();
            }
            // Files changed outside the editor, in a batch.
            if let Some(w) = &mut slot.watch {
                let changes = w.due(now);
                if !changes.is_empty() {
                    let list: Vec<Value> = changes
                        .iter()
                        .map(|(p, kind)| json!({ "uri": kalem_lsp::uri::from_path(p), "type": kind }))
                        .collect();
                    c.notify(
                        "workspace/didChangeWatchedFiles",
                        json!({ "changes": list }),
                    );
                }
            }
            for e in c.take_events() {
                changed = true;
                match e {
                    Event::Message { level, text } if level <= 2 => {
                        notice(
                            crate::tr!("lsp-says", server = c.name(), text = text),
                            level == 1,
                        );
                    }
                    Event::Exited { code } => {
                        let why = if c.started() {
                            slot_exited(slot, code, now)
                        } else {
                            // Ended by Kalem for its silence, or by itself.
                            let reason = if slot.started.elapsed() >= INIT_TIMEOUT {
                                Some(crate::tr!(
                                    "lsp-no-initialize",
                                    seconds =
                                        u32::try_from(INIT_TIMEOUT.as_secs()).unwrap_or(u32::MAX)
                                ))
                            } else {
                                c.why_not_started()
                            };
                            Some(slot_not_started(slot, &key, code, reason))
                        };
                        if let Some(why) = why {
                            gave_up.push((key.clone(), why));
                        }
                    }
                    // The server's own edit (after a code action's
                    // command): applied, and answered.
                    Event::ApplyEdit { id, edit } => server_edits.push((c.clone(), id, edit)),
                    _ => {}
                }
            }
        }
        for (key, why) in gave_up {
            for d in self
                .docs
                .values_mut()
                .filter(|d| d.key.as_ref() == Some(&key))
            {
                d.opened_in = None;
                d.missing = Some(why.clone());
            }
        }
        // Applied on another thread: applying takes the service's lock.
        if !server_edits.is_empty() {
            std::thread::spawn(move || {
                for (c, id, edit) in server_edits {
                    match features::workspace_edit(&edit) {
                        Some(files) => match apply_files(&files, &c) {
                            Ok(m) => {
                                c.answer_apply(id, true, None);
                                notice(m, false);
                            }
                            Err(e) => c.answer_apply(id, false, Some(&e)),
                        },
                        None => {
                            c.answer_apply(id, false, Some("file operations are not supported"))
                        }
                    }
                }
            });
        }
        changed
    }

    /// Starts the server of `key` again and opens its documents in it.
    fn restart_slot(&mut self, key: &Key) {
        let docs: Vec<PathBuf> = self
            .docs
            .iter()
            .filter(|(_, d)| d.key.as_ref() == Some(key))
            .map(|(p, _)| p.clone())
            .collect();
        let Some(first) = docs.first().and_then(|p| self.docs.get(p)) else {
            self.servers.remove(key);
            return;
        };
        let (plugin, lang) = (first.plugin.clone(), first.language.clone());
        match start_client(&plugin, &lang, &key.2) {
            Ok((_, c)) => {
                let c = Arc::new(c);
                if let Some(slot) = self.servers.get_mut(key) {
                    slot.client = Some(c.clone());
                    slot.started = Instant::now();
                }
                for p in docs {
                    if let Some(d) = self.docs.get_mut(&p) {
                        c.did_open(&d.uri, &d.language.id, &d.text);
                        d.opened_in = Some(c.clone());
                    }
                }
            }
            Err(e) => {
                if let Some(slot) = self.servers.get_mut(key) {
                    slot.failed = Some(e.clone());
                }
                notice(e, true);
            }
        }
    }

    /// The requests answered, or past their time.
    fn finished_actions(&mut self) -> Vec<(Action, Answered)> {
        let mut finished = Vec::new();
        let mut i = 0;
        while i < self.actions.len() {
            let a = &mut self.actions[i];
            if let Some(at) = a.retry_at {
                if at <= Instant::now() {
                    a.retry_at = None;
                    a.pending = a.client.request(a.kind.method(), a.params.clone());
                }
                i += 1;
                continue;
            }
            let answer = match a.pending.poll() {
                Some(r) => Some(r),
                None if a.started.elapsed() > TIMEOUT => {
                    a.client.cancel(&a.pending);
                    Some(Err(kalem_lsp::RpcError {
                        code: 0,
                        message: crate::tr!("lsp-no-answer"),
                    }))
                }
                None => None,
            };
            let cancelled = |r: &Answered| {
                matches!(r, Err(e) if e.code == kalem_lsp::CONTENT_MODIFIED
                    || e.code == kalem_lsp::SERVER_CANCELLED)
            };
            match answer {
                None => i += 1,
                // Asked again while the document is as it was asked
                // about: the server was busy (loading the project).
                Some(r)
                    if cancelled(&r)
                        && a.retries < RETRIES
                        && self.docs.get(&a.path).is_some_and(|d| d.text == a.text) =>
                {
                    a.retry_at = Some(Instant::now() + Duration::from_millis(500 << a.retries));
                    a.retries += 1;
                    i += 1;
                }
                Some(r) if cancelled(&r) => {
                    let busy = Err(kalem_lsp::RpcError {
                        code: kalem_lsp::CONTENT_MODIFIED,
                        message: crate::tr!("lsp-busy"),
                    });
                    finished.push((self.actions.remove(i), busy));
                }
                Some(r) => finished.push((self.actions.remove(i), r)),
            }
        }
        finished
    }

    /// What reading `finished` needs from the open documents: the paths
    /// servers name mapped to the editor's, and the texts of the open
    /// documents the answers point into (copied, so the answers are read
    /// without the lock).
    ///
    /// Before it, [`Service::remember_named`] keeps the files the answers
    /// name outside their servers' roots.
    fn answer_context(
        &self,
        finished: &[(Action, Answered)],
    ) -> (HashMap<PathBuf, PathBuf>, HashMap<PathBuf, String>) {
        let aliases: HashMap<PathBuf, PathBuf> = self
            .docs
            .iter()
            .map(|(p, d)| (d.real.clone(), p.clone()))
            .collect();
        let mut open: HashMap<PathBuf, String> = HashMap::new();
        for (_, r) in finished {
            let Ok(v) = r else { continue };
            for l in features::locations(v) {
                let p = aliases.get(&l.path).cloned().unwrap_or(l.path);
                if !open.contains_key(&p)
                    && let Some(d) = self.docs.get(&p)
                {
                    open.insert(p, d.text.clone());
                }
            }
        }
        (aliases, open)
    }

    /// Keeps the files the answers name outside the root of the server
    /// that gave them, with that server's key ([`Service::named`]).
    fn remember_named(&mut self, finished: &[(Action, Answered)]) {
        for (a, r) in finished {
            let Ok(v) = r else { continue };
            let Some(key) = self.docs.get(&a.path).and_then(|d| d.key.clone()) else {
                continue;
            };
            for l in features::locations(v) {
                if l.path.starts_with(&key.2) {
                    continue;
                }
                if self.named.len() >= NAMED_MAX {
                    self.named.clear();
                }
                let real = dunce::canonicalize(&l.path).unwrap_or(l.path);
                self.named.insert(real, key.clone());
            }
        }
    }

    /// The server that serves `real`, a file of `language` whose own root
    /// is `root`, because it named it in an answer: a server of the same
    /// plugin, running and serving the language, and none running in the
    /// file's own root. Its own root would start another server where it
    /// knows less: a crate's sources from crates.io have a `Cargo.lock`,
    /// and so does the standard library's folder, which rust-analyzer
    /// started there cannot load; the server that named the file knows
    /// it as part of its project. The same holds for Go's module cache,
    /// Python's `site-packages` and a C compiler's headers.
    fn named_by(
        &self,
        real: &Path,
        plugin: &Plugin,
        language: &LanguageSpec,
        root: &Path,
    ) -> Option<Key> {
        let running = |k: &Key| self.servers.get(k).is_some_and(|s| s.client.is_some());
        let own = self
            .servers
            .keys()
            .any(|k| k.0 == plugin.id && k.2 == root && running(k));
        if own {
            return None;
        }
        self.named
            .get(real)
            .filter(|k| k.0 == plugin.id && language.servers.contains(&k.1) && running(k))
            .cloned()
    }
}

/// A server's process ended before it answered `initialize`: it is not
/// started again, since it would end the same way (a toolchain's proxy
/// for a component not installed, a version manager's shim with no
/// version chosen, an argument the program does not know, `initialize`
/// refused), and the reason returned is the server's own, with the
/// plugin's `install` text. `code.restartServer` tries again.
fn slot_not_started(
    slot: &mut Slot,
    key: &Key,
    code: Option<i32>,
    reason: Option<String>,
) -> String {
    slot.client = None;
    slot.retry_at = None;
    let reason = reason.unwrap_or_else(|| languages::exit_text(code));
    let install = languages::plugins()
        .into_iter()
        .find(|p| p.id == key.0)
        .and_then(|p| p.server(&key.1).and_then(|s| s.install.clone()));
    let why = match install {
        Some(how) => crate::tr!(
            "lsp-did-not-start-how",
            server = slot.name.as_str(),
            reason = reason,
            how = how
        ),
        None => crate::tr!(
            "lsp-did-not-start",
            server = slot.name.as_str(),
            reason = reason
        ),
    };
    slot.failed = Some(why.clone());
    notice(why.clone(), true);
    why
}

/// A server's process ended: a restart is planned with backoff, or, past
/// [`MAX_CRASHES`], the server is given up on and the reason returned.
fn slot_exited(slot: &mut Slot, code: Option<i32>, now: Instant) -> Option<String> {
    slot.client = None;
    if slot.started.elapsed() >= CRASHES_FORGOTTEN {
        slot.crashes = 0;
    }
    slot.crashes += 1;
    let code = languages::exit_text(code);
    if slot.crashes > MAX_CRASHES {
        let why = crate::tr!(
            "lsp-gave-up",
            server = slot.name.as_str(),
            code = code,
            times = MAX_CRASHES
        );
        slot.failed = Some(why.clone());
        notice(why.clone(), true);
        return Some(why);
    }
    slot.retry_at = Some(now + Duration::from_millis(500 << slot.crashes.min(6)));
    notice(
        crate::tr!(
            "lsp-stopped-restarting",
            server = slot.name.as_str(),
            code = code
        ),
        true,
    );
    None
}

/// The answers for the document at `path`, now at `version`: what the
/// editor showing it does with them. Documentation asked for an older
/// version is dropped (the text under the cursor changed); what servers
/// say on their own comes as notices ([`crate::jobs::take_notices`]).
pub fn take_outcomes(path: &Path, version: u64) -> Vec<Outcome> {
    with(|s| {
        if !s.outcomes.iter().any(|a| a.path == path) {
            return Vec::new();
        }
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut s.outcomes)
            .into_iter()
            .partition(|a| a.path == path);
        s.outcomes = rest;
        mine.into_iter()
            .filter(|a| !(matches!(a.outcome, Outcome::Hover { .. }) && a.version != version))
            .map(|a| a.outcome)
            .collect()
    })
}

/// Hover text as lines of at most `width` characters, at most `max`
/// of them: code fences dropped, paragraphs wrapped at spaces.
pub fn hover_lines(text: &str, width: usize, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            continue;
        }
        let mut cur = String::new();
        for word in line.split(' ') {
            if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        out.push(cur);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    if out.len() > max {
        out.truncate(max);
        if let Some(l) = out.last_mut() {
            l.push_str(" …");
        }
    }
    out
}

/// Requests are waiting for answers (the terminal frontend polls sooner).
pub fn busy() -> bool {
    with(|s| !s.actions.is_empty() || s.servers.values().any(|v| v.retry_at.is_some()))
}

/// The place of `loc` in `text`, its file's text.
fn place(loc: &features::Location, text: &str, enc: Encoding) -> Place {
    let at = kalem_lsp::position::offset(text, loc.start, enc);
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    Place {
        path: loc.path.clone(),
        line: u64::from(loc.start.line) + 1,
        column: at - line_start,
        preview: text[line_start..line_end].trim().to_string(),
    }
}

/// What a request's answer shows; `aliases` maps the files servers name
/// to the paths the editor opened them under, `open` has the texts of the
/// open documents it points into (other files are read from the disk).
fn outcome(
    a: &Action,
    r: Result<Value, kalem_lsp::RpcError>,
    aliases: &HashMap<PathBuf, PathBuf>,
    open: &HashMap<PathBuf, String>,
) -> Outcome {
    let v = match r {
        Ok(v) => v,
        // A signature not found is no news: the card stays closed.
        Err(_) if a.kind == Kind::Signature => {
            return Outcome::Signature {
                path: a.path.clone(),
                text: None,
            };
        }
        Err(e) => {
            return Outcome::Message {
                text: crate::tr!(
                    "lsp-says",
                    server = a.client.name(),
                    text = e.message.as_str()
                ),
                error: true,
            };
        }
    };
    let nothing = || Outcome::Message {
        text: crate::tr!(
            "lsp-nothing",
            server = a.client.name(),
            what = a.kind.what()
        ),
        error: false,
    };
    match a.kind {
        Kind::Rename => match features::workspace_edit(&v) {
            Some(files) if files.iter().any(|(_, e)| !e.is_empty()) => {
                let n: usize = files.iter().map(|(_, e)| e.len()).sum();
                let m = files.iter().filter(|(_, e)| !e.is_empty()).count();
                let id = keep_plan(Plan {
                    files,
                    client: a.client.clone(),
                    answer: None,
                });
                let file = a
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Outcome::Choose(vec![
                    choice(
                        crate::palette::invocation("code.applyEdit", &json!({ "id": id })),
                        crate::tr!("lsp-rename-apply", changes = n, files = m),
                        file,
                    ),
                    choice(
                        crate::palette::invocation("code.dropEdit", &json!({ "id": id })),
                        crate::tr!("plugin-cancel"),
                        String::new(),
                    ),
                ])
            }
            Some(_) => nothing(),
            None => Outcome::Message {
                text: crate::tr!("lsp-edit-unsupported", server = a.client.name()),
                error: true,
            },
        },
        Kind::CodeAction => {
            let list = v.as_array().cloned().unwrap_or_default();
            let items: Vec<crate::palette::PaletteItem> = list
                .into_iter()
                .filter(|x| x.get("disabled").is_none())
                .filter_map(|x| {
                    let title = x["title"].as_str()?.to_string();
                    let kind = x["kind"].as_str().unwrap_or("").to_string();
                    let id = keep_offer(a.client.clone(), a.path.clone(), x);
                    Some(choice(
                        crate::palette::invocation("code.runAction", &json!({ "id": id })),
                        title,
                        kind,
                    ))
                })
                .collect();
            if items.is_empty() {
                nothing()
            } else {
                Outcome::Choose(items)
            }
        }
        Kind::ResolveAction => run_action(&a.client, &v),
        Kind::Signature => Outcome::Signature {
            path: a.path.clone(),
            text: features::signature_text(&v),
        },
        Kind::Hover => match features::hover_text(&v) {
            Some(text) => Outcome::Hover {
                path: a.path.clone(),
                text,
            },
            None => nothing(),
        },
        Kind::Format => match features::text_edits(&a.text, &v, a.enc) {
            Some(edits) if !edits.is_empty() => Outcome::Edits {
                path: a.path.clone(),
                version: a.version,
                edits,
                label: crate::l10n::tr(&crate::l10n::command_key("edit.formatDocument")),
            },
            Some(_) => Outcome::Message {
                text: crate::tr!("lsp-formatted"),
                error: false,
            },
            None => Outcome::Message {
                text: crate::tr!("lsp-overlapping", server = a.client.name()),
                error: true,
            },
        },
        Kind::Symbols => {
            let places: Vec<Place> = features::symbols(&v)
                .into_iter()
                .map(|(depth, name, _, start)| {
                    let mut p = place(
                        &features::Location {
                            path: a.path.clone(),
                            start,
                            end: start,
                        },
                        &a.text,
                        a.enc,
                    );
                    p.preview = format!("{}{name}", "  ".repeat(depth));
                    p
                })
                .collect();
            if places.is_empty() {
                nothing()
            } else {
                Outcome::Places {
                    title: crate::tr!("lsp-title-symbols"),
                    places,
                }
            }
        }
        _ => {
            let mut locs = features::locations(&v);
            for l in &mut locs {
                if let Some(p) = aliases.get(&l.path) {
                    l.path = p.clone();
                }
            }
            // The asking document's text as asked, the other open
            // documents' as the editor has them, the rest from the disk.
            let mut read: HashMap<PathBuf, String> = HashMap::new();
            let places: Vec<Place> = locs
                .iter()
                .map(|l| {
                    if l.path == a.path {
                        return place(l, &a.text, a.enc);
                    }
                    if let Some(t) = open.get(&l.path) {
                        return place(l, t, a.enc);
                    }
                    let t = read
                        .entry(l.path.clone())
                        .or_insert_with(|| std::fs::read_to_string(&l.path).unwrap_or_default());
                    place(l, t, a.enc)
                })
                .collect();
            match places.len() {
                0 => nothing(),
                1 if a.kind != Kind::References => places
                    .into_iter()
                    .next()
                    .map_or_else(nothing, Outcome::Jump),
                _ => Outcome::Places {
                    title: match a.kind {
                        Kind::References => crate::tr!("lsp-title-references"),
                        Kind::Implementation => crate::tr!("lsp-title-implementations"),
                        _ => crate::tr!("lsp-title-definitions"),
                    },
                    places,
                },
            }
        }
    }
}

/// A workspace edit waiting for the user's yes (a rename), or applied
/// at once (a code action's, a server's own).
struct Plan {
    /// The text edits by document URI, as the server sent them.
    files: Vec<(String, Vec<Value>)>,
    client: Arc<Client>,
    /// The server's `workspace/applyEdit` request to answer, if it asked.
    answer: Option<Value>,
}

/// A code action offered in a list, until it is chosen.
type Offer = (u64, Arc<Client>, PathBuf, Value);

static PLANS: Mutex<Vec<(u64, Plan)>> = Mutex::new(Vec::new());
static OFFERS: Mutex<Vec<Offer>> = Mutex::new(Vec::new());
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Keeps a plan for [`apply_plan`]; the ten latest are kept.
fn keep_plan(plan: Plan) -> u64 {
    let id = next_id();
    let mut p = PLANS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    p.push((id, plan));
    let excess = p.len().saturating_sub(10);
    p.drain(..excess);
    id
}

/// Keeps an offered code action for [`run_offer`].
fn keep_offer(client: Arc<Client>, path: PathBuf, action: Value) -> u64 {
    let id = next_id();
    let mut o = OFFERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if o.len() > 64 {
        o.clear();
    }
    o.push((id, client, path, action));
    id
}

fn choice(id: String, title: String, category: String) -> crate::palette::PaletteItem {
    crate::palette::PaletteItem {
        also: title.clone(),
        id,
        title,
        category,
        keys: String::new(),
    }
}

/// Applies plan `id`: edits of documents open in the editor go to their
/// editors (applied there if the document has not changed since), files
/// not open are written. A message for the status bar.
pub fn apply_plan(id: u64) -> Result<String, String> {
    let plan = {
        let mut p = PLANS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let i = p
            .iter()
            .position(|(x, _)| *x == id)
            .ok_or_else(|| crate::tr!("lsp-edit-gone"))?;
        p.remove(i).1
    };
    let r = apply_files(&plan.files, &plan.client);
    if let Some(req) = plan.answer {
        match &r {
            Ok(_) => plan.client.answer_apply(req, true, None),
            Err(e) => plan.client.answer_apply(req, false, Some(e)),
        }
    }
    r
}

/// Drops plan `id` (the user said no).
pub fn drop_plan(id: u64) {
    let mut p = PLANS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    p.retain(|(x, plan)| {
        if *x == id
            && let Some(req) = &plan.answer
        {
            plan.client
                .answer_apply(req.clone(), false, Some("declined"));
        }
        *x != id
    });
}

/// Writes the edits: to the editors of open documents, as answers they
/// take on their timer; to the disk for the rest.
fn apply_files(files: &[(String, Vec<Value>)], client: &Client) -> Result<String, String> {
    let enc = client.encoding();
    let mut changes = 0;
    let mut written = 0;
    for (uri, edits) in files.iter().filter(|(_, e)| !e.is_empty()) {
        let Some(real) = kalem_lsp::uri::to_path(uri) else {
            continue;
        };
        let norm = kalem_lsp::uri::normalize(uri);
        // An open document: its editor applies the edits.
        let open = with(|s| {
            let (p, d) = s
                .docs
                .iter()
                .find(|(_, d)| d.real == real || kalem_lsp::uri::normalize(&d.uri) == norm)?;
            let edits = features::text_edits(&d.text, &Value::Array(edits.clone()), enc)?;
            Some((p.clone(), d.version, edits))
        });
        if let Some((path, version, edits)) = open {
            changes += edits.len();
            let label = crate::tr!("lsp-edit-label");
            with(|s| {
                s.outcomes.push(Answer {
                    path: path.clone(),
                    version,
                    at: Instant::now(),
                    outcome: Outcome::Edits {
                        path,
                        version,
                        edits,
                        label,
                    },
                });
            });
            continue;
        }
        let text =
            std::fs::read_to_string(&real).map_err(|e| format!("{}: {e}", real.display()))?;
        let edits = features::text_edits(&text, &Value::Array(edits.clone()), enc)
            .ok_or_else(|| crate::tr!("lsp-overlapping", server = client.name()))?;
        changes += edits.len();
        let new = features::apply(&text, &edits);
        let tmp = real.with_extension("kalem-tmp");
        std::fs::write(&tmp, new).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &real).map_err(|e| format!("{}: {e}", real.display()))?;
        written += 1;
    }
    Ok(crate::tr!(
        "lsp-edit-applied",
        changes = changes,
        written = written
    ))
}

/// Runs offered code action `id`: its edit applied and its command run,
/// or, with neither, its edit fetched first (`codeAction/resolve`).
pub fn run_offer(id: u64) -> Result<String, String> {
    let (client, path, action) = {
        let mut o = OFFERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let i = o
            .iter()
            .position(|(x, ..)| *x == id)
            .ok_or_else(|| crate::tr!("lsp-edit-gone"))?;
        let (_, c, p, a) = o.remove(i);
        (c, p, a)
    };
    let resolvable = client.capabilities()["codeActionProvider"]["resolveProvider"]
        .as_bool()
        .unwrap_or(false);
    if action.get("edit").is_none() && action.get("command").is_none() && resolvable {
        let version = with(|s| s.docs.get(&path).map(|d| d.version)).unwrap_or(0);
        with(|s| {
            s.ask_with(
                &path,
                Kind::ResolveAction,
                0..0,
                version,
                None,
                action.clone(),
            )
        })
        .map_err(Option::unwrap_or_default)?;
        return Ok(String::new());
    }
    match run_action(&client, &action) {
        Outcome::Message { text, error: true } => Err(text),
        Outcome::Message { text, .. } => Ok(text),
        _ => Ok(String::new()),
    }
}

/// A code action or command run: its edit applied, its command sent to
/// the server (whose edits come back as `workspace/applyEdit`).
fn run_action(client: &Arc<Client>, action: &Value) -> Outcome {
    let mut said = String::new();
    if let Some(edit) = action.get("edit") {
        match features::workspace_edit(edit) {
            Some(files) => match apply_files(&files, client) {
                Ok(m) => said = m,
                Err(e) => {
                    return Outcome::Message {
                        text: e,
                        error: true,
                    };
                }
            },
            None => {
                return Outcome::Message {
                    text: crate::tr!("lsp-edit-unsupported", server = client.name()),
                    error: true,
                };
            }
        }
    }
    // A `Command` is itself the command; a `CodeAction` may carry one.
    let command = match &action["command"] {
        Value::String(_) => Some(action.clone()),
        c @ Value::Object(_) => Some(c.clone()),
        _ => None,
    };
    if let Some(c) = command {
        let _ = client.request(
            "workspace/executeCommand",
            json!({
                "command": c["command"],
                "arguments": c.get("arguments").cloned().unwrap_or(json!([])),
            }),
        );
    }
    Outcome::Message {
        text: said,
        error: false,
    }
}

/// Asks the server of `doc` for the edits renaming the symbol at its
/// cursor to `new_name`; they come as a list to confirm.
pub fn rename(doc: &DocumentState, new_name: &str) -> Result<(), String> {
    sync(doc);
    let path = code_file(doc).ok_or_else(|| no_server(doc))?.to_path_buf();
    let (at, version) = (doc.selection.head, doc.version());
    let extra = json!({ "newName": new_name });
    with(
        |s| match s.ask_with(&path, Kind::Rename, at..at, version, None, extra) {
            Err(None) => Err(no_server(doc)),
            Err(Some(why)) => Err(why),
            Ok(()) => Ok(()),
        },
    )
}

/// Asks the server of `doc` for the code actions at its cursor or
/// selection (with the problems there); they come as a list.
pub fn code_actions(doc: &DocumentState) -> Result<(), String> {
    sync(doc);
    let path = code_file(doc).ok_or_else(|| no_server(doc))?.to_path_buf();
    let (sel, version) = (doc.selection, doc.version());
    let range = sel.anchor..sel.head;
    with(
        |s| match s.ask_with(&path, Kind::CodeAction, range, version, None, Value::Null) {
            Err(None) => Err(no_server(doc)),
            Err(Some(why)) => Err(why),
            Ok(()) => Ok(()),
        },
    )
}

/// Asks the server of `doc` for `kind` at its cursor; the answer comes as
/// an [`Outcome`]. An error says why not at once (no server, the server
/// lacks the feature, it is starting).
pub fn request(doc: &DocumentState, kind: Kind) -> Result<(), String> {
    sync(doc);
    let path = match code_file(doc) {
        Some(p) => p.to_path_buf(),
        None => return Err(no_server(doc)),
    };
    let (at, version) = (doc.selection.head, doc.version());
    with(|s| match s.ask(&path, kind, at, version, None) {
        Err(None) => Err(no_server(doc)),
        Err(Some(why)) => Err(why),
        Ok(()) => Ok(()),
    })
}

impl Service {
    /// Sends request `kind` for byte `at` of the document at `path`
    /// (at `version`), its answer to come as an [`Outcome`]; `trigger` is
    /// the character that asked for a signature. `Err(None)` when no
    /// plugin serves the file, else why the server cannot answer.
    fn ask(
        &mut self,
        path: &Path,
        kind: Kind,
        at: usize,
        version: u64,
        trigger: Option<&str>,
    ) -> Result<(), Option<String>> {
        self.ask_with(path, kind, at..at, version, trigger, Value::Null)
    }

    /// [`Service::ask`] for bytes `range` (a code action's selection),
    /// with `extra` parameters (a rename's new name, an action to resolve).
    fn ask_with(
        &mut self,
        path: &Path,
        kind: Kind,
        range: Range<usize>,
        version: u64,
        trigger: Option<&str>,
        extra: Value,
    ) -> Result<(), Option<String>> {
        let at = range.end;
        let Some(d) = self.docs.get(path) else {
            return Err(None);
        };
        let c = match &d.opened_in {
            Some(c) => c.clone(),
            None => {
                return Err(Some(d.missing.clone().unwrap_or_else(|| {
                    crate::tr!("lsp-no-server-see", language = d.language.name.as_str())
                })));
            }
        };
        if c.has_exited() {
            return Err(Some(crate::tr!("lsp-restarting", server = c.name())));
        }
        if !c.is_ready() {
            return Err(Some(crate::tr!("lsp-starting", server = c.name())));
        }
        if !c.provides(kind.provider()) {
            return Err(Some(crate::tr!(
                "lsp-not-provided",
                server = c.name(),
                what = kind.what()
            )));
        }
        let enc = c.encoding();
        let td = json!({ "uri": d.uri });
        let position = || kalem_lsp::position::position(&d.text, at, enc).to_json();
        let params = match kind {
            Kind::Format => {
                // The document's own indentation.
                let (tab, spaces) = match crate::text::detect_indent(&d.text) {
                    Some(crate::text::Indent::Spaces(n)) => (n, true),
                    Some(_) => (4, false),
                    None => (2, true),
                };
                json!({ "textDocument": td, "options": { "tabSize": tab, "insertSpaces": spaces } })
            }
            Kind::Symbols => json!({ "textDocument": td }),
            Kind::References => json!({
                "textDocument": td,
                "position": position(),
                "context": { "includeDeclaration": true },
            }),
            Kind::Signature => json!({
                "textDocument": td,
                "position": position(),
                "context": match trigger {
                    Some(t) => json!({ "triggerKind": 2, "triggerCharacter": t, "isRetrigger": false }),
                    None => json!({ "triggerKind": 1, "isRetrigger": false }),
                },
            }),
            Kind::Rename => json!({
                "textDocument": td,
                "position": position(),
                "newName": extra["newName"],
            }),
            Kind::CodeAction => {
                // The server's diagnostics under the range, for its fixes.
                let r = range.start.min(range.end)..range.start.max(range.end);
                let raw = c.diagnostics(&d.uri);
                let under: Vec<Value> = raw
                    .iter()
                    .filter(|x| {
                        kalem_lsp::position::byte_range(&d.text, &x["range"], enc)
                            .is_some_and(|b| b.start <= r.end && r.start <= b.end)
                    })
                    .cloned()
                    .collect();
                json!({
                    "textDocument": td,
                    "range": kalem_lsp::position::range_json(&d.text, r, enc),
                    "context": { "diagnostics": under },
                })
            }
            Kind::ResolveAction => extra.clone(),
            _ => json!({ "textDocument": td, "position": position() }),
        };
        let pending = c.request(kind.method(), params.clone());
        let text = d.text.clone();
        self.actions.push(Action {
            kind,
            path: path.to_path_buf(),
            version,
            text,
            enc,
            pending,
            client: c,
            started: Instant::now(),
            params,
            retries: 0,
            retry_at: None,
        });
        Ok(())
    }
}

/// Why no server answers for `doc`: no plugin for its kind of file.
fn no_server(doc: &DocumentState) -> String {
    let kind = match &doc.meta.mode {
        crate::mode::DocumentMode::Text { language: Some(l) } => {
            crate::tr!("lsp-kind-files", ext = l.as_str())
        }
        crate::mode::DocumentMode::Text { language: None } => crate::tr!("lsp-kind-plain"),
        _ => crate::tr!("lsp-kind-documents", kind = doc.document_type()),
    };
    crate::tr!("lsp-no-server", kind = kind)
}

/// The server of `doc` can format it (the `hasFormatter` of when-clauses).
pub fn can(doc: &DocumentState, kind: Kind) -> bool {
    let Some(path) = code_file(doc) else {
        return false;
    };
    with(|s| {
        s.docs
            .get(path)
            .and_then(|d| d.opened_in.as_ref())
            .is_some_and(|c| c.is_ready() && c.provides(kind.provider()))
    })
}

/// A language server serves `doc` (or is starting for it).
pub fn serves(doc: &DocumentState) -> bool {
    let Some(path) = code_file(doc) else {
        return false;
    };
    with(|s| s.docs.get(path).is_some_and(|d| d.opened_in.is_some()))
}

/// The server of the document at `path` is starting, restarting or
/// reports work in progress (indexing, compiling).
pub fn working(path: &Path) -> bool {
    with(|s| {
        s.docs
            .get(path)
            .and_then(|d| d.opened_in.as_ref())
            .is_some_and(|c| !c.is_ready() || c.progress().is_some())
            || s.docs
                .get(path)
                .and_then(|d| d.key.as_ref())
                .and_then(|k| s.servers.get(k))
                .is_some_and(|v| v.retry_at.is_some())
    })
}

/// The diagnostics of the document at `path`, against the text the
/// server has (the document's, when it is in step).
pub fn diagnostics(path: &Path) -> Vec<Diagnostic> {
    with(|s| {
        // Made for an older version of the text: not drawn where they no
        // longer belong; the server sends new ones.
        s.docs
            .get_mut(path)
            .and_then(Doc::diagnostics)
            .filter(|c| !c.stale)
            .map(|c| c.list.clone())
            .unwrap_or_default()
    })
}

/// The language plugin of `doc` has a formatter command (`commands.format`).
/// Asked at every key press (the `hasFormatter` context): it looks for no
/// root folder, unlike [`format_command`].
pub fn has_format_command(doc: &DocumentState) -> bool {
    let Some(path) = code_file(doc) else {
        return false;
    };
    let first = doc.text().as_str().lines().next();
    languages::for_path(path, first)
        .is_some_and(|(plugin, _)| plugin.commands.get("format").is_some_and(|c| !c.is_empty()))
}

/// The formatter command of `doc`'s language plugin with `{file}` filled
/// in, and the folder it runs in (the server's root, else the file's).
fn format_command(doc: &DocumentState) -> Option<(Vec<String>, PathBuf)> {
    let path = code_file(doc)?;
    let first = doc.text().as_str().lines().next();
    let (plugin, lang) = languages::for_path(path, first)?;
    let cmd = plugin.commands.get("format").filter(|c| !c.is_empty())?;
    let file = path.to_string_lossy();
    let cmd = cmd.iter().map(|a| a.replace("{file}", &file)).collect();
    let real = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Some((cmd, root_of(&real, &plugin, &lang)))
}

/// The formatter command of `doc`'s language plugin (`commands.format`)
/// with `{file}` filled in, as a formatter to run.
pub fn plugin_formatter(doc: &DocumentState) -> Option<crate::formatters::Formatter> {
    let (command, dir) = format_command(doc)?;
    Some(crate::formatters::Formatter { command, dir })
}

/// Formats `doc` with its language plugin's formatter command
/// ([`format_with`]).
pub fn format_with_command(doc: &DocumentState) -> Result<(), String> {
    let (command, dir) = format_command(doc).ok_or_else(|| no_server(doc))?;
    format_with(doc, crate::formatters::Formatter { command, dir })
}

/// Formats `doc` with formatter `f` on another thread: the text on its
/// standard input, the formatted text on its output; the edits come back
/// as an [`Outcome::Edits`] for this version (applied only if the
/// document has not changed), a failure as a message with the command's
/// first line of errors.
pub fn format_with(doc: &DocumentState, f: crate::formatters::Formatter) -> Result<(), String> {
    let path = doc
        .meta
        .path
        .clone()
        .ok_or_else(|| crate::l10n::tr("msg-no-file"))?;
    let (program, args) = f.command.split_first().ok_or_else(|| no_server(doc))?;
    let program = kalem_lsp::find_program(program, Some(&f.dir), &[])
        .ok_or_else(|| crate::tr!("lsp-formatter-missing", program = program.as_str()))?;
    let args = args.to_vec();
    let dir = f.dir.clone();
    let text = doc.text().as_str().to_string();
    let version = doc.version();
    let name = f.command.join(" ");
    std::thread::spawn(move || {
        let outcome = crate::formatters::run(&program, &args, &dir, &text)
            .map(|formatted| {
                let edits = whole_text_edit(&text, &formatted);
                if edits.is_empty() {
                    Outcome::Message {
                        text: crate::tr!("lsp-formatted"),
                        error: false,
                    }
                } else {
                    Outcome::Edits {
                        path: path.clone(),
                        version,
                        edits,
                        label: crate::l10n::tr(&crate::l10n::command_key("edit.formatDocument")),
                    }
                }
            })
            .unwrap_or_else(|why| Outcome::Message {
                text: crate::tr!(
                    "lsp-formatter-failed",
                    command = name.as_str(),
                    reason = why
                ),
                error: true,
            });
        with(|s| {
            s.outcomes.push(Answer {
                path,
                version,
                at: Instant::now(),
                outcome,
            });
        });
    });
    Ok(())
}

/// The one edit turning `old` into `new`: the part between their common
/// start and end (none when they are the same).
fn whole_text_edit(old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(pre) || !new.is_char_boundary(pre) {
        pre -= 1;
    }
    if pre == a.len() && pre == b.len() {
        return Vec::new();
    }
    let max = a.len().min(b.len()) - pre;
    let mut suf = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max)
        .take_while(|(x, y)| x == y)
        .count();
    while !old.is_char_boundary(a.len() - suf) || !new.is_char_boundary(b.len() - suf) {
        suf -= 1;
    }
    vec![(pre..a.len() - suf, new[pre..b.len() - suf].to_string())]
}

/// Flags the runs of line view `v` under the language server's problems
/// starting on its line, as LaTeX's checks are flagged: errors and
/// warnings as probably wrong, information and hints as style; nothing
/// for diagnostics made for older text.
pub fn flag_diagnostics(doc: &DocumentState, v: &mut crate::view::LineView) {
    let Some(path) = code_file(doc) else { return };
    let line = v.range.clone();
    let here: Vec<(Range<usize>, bool)> = with(|s| {
        let Some(cache) = s.docs.get_mut(path).and_then(Doc::diagnostics) else {
            return Vec::new();
        };
        if cache.stale {
            return Vec::new();
        }
        let first = cache.list.partition_point(|d| d.range.start < line.start);
        cache.list[first..]
            .iter()
            .take_while(|d| d.range.start <= line.end)
            .map(|d| (d.range.clone(), d.severity <= Severity::Warning))
            .collect()
    });
    if !here.is_empty() {
        crate::latex_view::flag_ranges(v, &here);
    }
}

/// The most serious problem starting on line `line` (from 0) of `doc`,
/// for a mark in the gutter.
pub fn line_mark(doc: &DocumentState, line: usize) -> Option<Severity> {
    let path = code_file(doc)?;
    with(|s| {
        let cache = s.docs.get_mut(path).and_then(Doc::diagnostics)?;
        if cache.stale {
            return None;
        }
        cache.by_line.get(&line).copied()
    })
}

/// The messages of the problems at byte `at` of `doc`, for a hover
/// card: each with its source, joined by blank lines.
pub fn diagnostic_at(doc: &DocumentState, at: usize) -> Option<String> {
    let path = code_file(doc)?;
    with(|s| {
        let d = s.docs.get_mut(path)?;
        let name = d
            .opened_in
            .as_ref()
            .map(|c| c.name().to_string())
            .unwrap_or_default();
        let cache = d.diagnostics()?;
        if cache.stale {
            return None;
        }
        let texts: Vec<String> = cache
            .list
            .iter()
            .filter(|d| d.range.start <= at && at < d.range.end.max(d.range.start + 1))
            .map(|d| {
                let src = d.source.as_deref().unwrap_or(&name);
                format!("{src}: {}", d.message)
            })
            .collect();
        (!texts.is_empty()).then(|| texts.join("\n\n"))
    })
}

/// The status bar's word on the document at `path`, whose cursor is at
/// `at`: the diagnostic there, else the server's progress, else the
/// counts.
pub fn status(path: &Path, at: usize) -> Option<String> {
    with(|s| {
        let d = s.docs.get_mut(path)?;
        let Some(c) = d.opened_in.clone() else {
            return d.missing.clone();
        };
        let name = c.name().to_string();
        let line = d.line_of(at);
        let cache = d.diagnostics()?;
        // The problem on the cursor's line, unless the text moved since.
        if !cache.stale
            && let Some(i) = cache.lines.iter().position(|&l| l == line)
        {
            let diag = &cache.list[i];
            let first = diag.message.lines().next().unwrap_or("");
            let src = diag.source.as_deref().unwrap_or(&name);
            return Some(format!("{src}: {first}"));
        }
        let errors = cache
            .list
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = cache
            .list
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
        let progress = if c.has_exited() {
            Some(crate::tr!("lsp-status-restarting", server = name.as_str()))
        } else if !c.is_ready() {
            Some(crate::tr!("lsp-status-starting", server = name.as_str()))
        } else {
            c.progress()
                .map(|p| format!("{name}: {p}").replace(&format!("{name}: {name}"), &name))
        };
        if progress.is_some() {
            return progress;
        }
        match (errors, warnings) {
            (0, 0) => None,
            (e, w) => Some(crate::tr!(
                "lsp-status-counts",
                server = name.as_str(),
                errors = e,
                warnings = w
            )),
        }
    })
}

/// The diagnostics of every open document, for the problems list.
pub fn all_problems() -> Vec<Place> {
    let mut out = problems_of(None);
    // The project's other files the servers report, read from the disk
    // (outside the lock).
    let others: Vec<(PathBuf, Vec<Value>, Encoding)> = with(|s| {
        let open: std::collections::HashSet<String> = s
            .docs
            .values()
            .map(|d| kalem_lsp::uri::normalize(&d.uri))
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut v = Vec::new();
        for slot in s.servers.values() {
            let Some(c) = &slot.client else { continue };
            for (uri, list) in c.all_published() {
                if open.contains(&uri) || !seen.insert(uri.clone()) {
                    continue;
                }
                if let Some(path) = kalem_lsp::uri::to_path(&uri) {
                    v.push((path, list, c.encoding()));
                }
            }
        }
        v
    });
    let mut more: Vec<Place> = Vec::new();
    for (path, list, enc) in others {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for diag in features::diagnostics(&text, &list, enc) {
            let at = diag.range.start;
            let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
            more.push(Place {
                path: path.clone(),
                line: text[..at].matches('\n').count() as u64 + 1,
                column: at - line_start,
                preview: format!(
                    "{}: {}",
                    severity_word(diag.severity),
                    diag.message.lines().next().unwrap_or("")
                ),
            });
        }
    }
    more.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    out.extend(more);
    out
}

/// A severity in the interface language.
fn severity_word(s: Severity) -> String {
    match s {
        Severity::Error => crate::tr!("lsp-severity-error"),
        Severity::Warning => crate::tr!("lsp-severity-warning"),
        Severity::Information => crate::tr!("lsp-severity-info"),
        Severity::Hint => crate::tr!("lsp-severity-hint"),
    }
}

/// The problems of the document at `path`, or of every open document.
pub fn problems_of(only: Option<&Path>) -> Vec<Place> {
    with(|s| {
        let mut out = Vec::new();
        let mut paths: Vec<PathBuf> = match only {
            Some(p) => vec![p.to_path_buf()],
            None => s.docs.keys().cloned().collect(),
        };
        paths.sort();
        for p in paths {
            let Some(d) = s.docs.get_mut(&p) else {
                continue;
            };
            let Some(cache) = d.diagnostics() else {
                continue;
            };
            let found: Vec<(usize, usize, String)> = cache
                .list
                .iter()
                .zip(&cache.lines)
                .map(|(diag, &line)| {
                    let severity = severity_word(diag.severity);
                    let first = diag.message.lines().next().unwrap_or("");
                    (diag.range.start, line, format!("{severity}: {first}"))
                })
                .collect();
            for (at, line, preview) in found {
                let start = d.line_starts().get(line).copied().unwrap_or(0);
                out.push(Place {
                    path: p.clone(),
                    line: line as u64 + 1,
                    column: at.saturating_sub(start),
                    preview,
                });
            }
        }
        out
    })
}

/// A line per running server, for `kalem lsp status` and the log view.
pub fn report() -> Vec<String> {
    with(|s| {
        let mut out = Vec::new();
        for ((plugin, key, root), slot) in &s.servers {
            let state = match &slot.client {
                Some(c) if c.has_exited() => crate::tr!("lsp-report-exited"),
                Some(c) if c.is_ready() => {
                    crate::tr!("lsp-report-ready", count = c.open_documents())
                }
                Some(_) => crate::tr!("lsp-report-starting"),
                None => slot
                    .failed
                    .clone()
                    .unwrap_or_else(|| crate::tr!("lsp-report-restarting")),
            };
            out.push(crate::tr!(
                "lsp-report-line",
                server = slot.name.as_str(),
                plugin = plugin.as_str(),
                key = key.as_str(),
                root = root.display().to_string(),
                state = state
            ));
        }
        out.sort();
        out
    })
}

/// The log of the server of the document at `path`.
pub fn log(path: &Path) -> Vec<String> {
    with(|s| {
        s.docs
            .get(path)
            .and_then(|d| d.opened_in.as_ref())
            .map(|c| c.log())
            .unwrap_or_default()
    })
}

/// A plugin was installed, updated or removed: its servers stop, and the
/// documents they served open again on the next [`sync`] with the
/// plugin as it is now (new settings, a new server, or none).
pub fn plugin_changed(id: &str) {
    let stopped: Vec<Arc<Client>> = with(|s| {
        let paths: Vec<PathBuf> = s
            .docs
            .iter()
            .filter(|(_, d)| d.plugin.id == id)
            .map(|(p, _)| p.clone())
            .collect();
        let mut clients = Vec::new();
        for p in paths {
            if let Some(d) = s.docs.remove(&p)
                && let Some(k) = d.key
                && let Some(slot) = s.servers.remove(&k)
            {
                clients.extend(slot.client);
            }
        }
        // Files no plugin served before are tried again too.
        s.docs
            .retain(|_, d| d.opened_in.is_some() || d.key.is_some());
        s.unserved.clear();
        clients
    });
    for c in stopped {
        std::thread::spawn(move || c.shutdown());
    }
}

/// The settings changed: running servers get theirs (when they differ),
/// and documents whose server would now be another one, or none, or one
/// newly found, are opened again on the next [`sync`]; failed starts are
/// tried again.
pub fn settings_changed() {
    let stopped: Vec<Arc<Client>> = with(|s| {
        // Failed starts: forgotten, to be tried again.
        s.servers.retain(|_, slot| slot.failed.is_none());
        // Files no plugin served are looked up again.
        s.unserved.clear();
        let mut reopen = Vec::new();
        for (path, d) in &s.docs {
            let root = d
                .key
                .as_ref()
                .map_or_else(|| root_of(&d.real, &d.plugin, &d.language), |k| k.2.clone());
            let now = match languages::resolve_server(&d.plugin, &d.language, Some(&root)) {
                Resolved::Found(spec, ..) => Some((d.plugin.id.clone(), spec.key, root)),
                _ => None,
            };
            let running = d.key.as_ref().is_some_and(|k| s.servers.contains_key(k));
            if now != d.key || (d.key.is_some() && !running) {
                reopen.push(path.clone());
            } else if let (Some(spec_key), Some(c)) = (&d.key, &d.opened_in)
                && let Some(spec) = d.plugin.server(&spec_key.1)
            {
                c.set_settings(languages::server_settings(&d.plugin, spec));
            }
        }
        let mut clients = Vec::new();
        for p in reopen {
            if let Some(d) = s.docs.remove(&p)
                && let Some(c) = d.opened_in
            {
                c.did_close(&d.uri);
                if c.open_documents() == 0
                    && let Some(k) = d.key
                {
                    s.servers.remove(&k);
                    clients.push(c);
                }
            }
        }
        clients
    });
    for c in stopped {
        std::thread::spawn(move || c.shutdown());
    }
}

/// Stops every server (on quit).
pub fn shutdown_all() {
    let clients: Vec<Arc<Client>> = with(|s| {
        s.docs.clear();
        s.servers.drain().filter_map(|(_, v)| v.client).collect()
    });
    // Together: each waits a moment for its server, and quitting should
    // not wait for them one after another.
    let threads: Vec<_> = clients
        .into_iter()
        .map(|c| std::thread::spawn(move || c.shutdown()))
        .collect();
    for t in threads {
        let _ = t.join();
    }
}

/// The completer of language servers, on the completer contract (§11.12).
#[derive(Debug)]
pub struct LspCompleter;

impl crate::completers::Completer for LspCompleter {
    fn id(&self) -> &'static str {
        "lsp"
    }
    fn priority(&self) -> i32 {
        10
    }
    fn applies(&self, ctx: &crate::completers::Context) -> bool {
        let Some(path) = &ctx.path else { return false };
        with(|s| {
            s.docs
                .get(path)
                .and_then(|d| d.opened_in.as_ref())
                .is_some_and(|c| c.is_ready() && c.provides("completionProvider"))
        })
    }
    fn trigger(&self) -> crate::completers::Trigger {
        crate::completers::Trigger::WordOrAfter(1, &[".", ":", "@", "<", "/", "%", "&", "->", "::"])
    }
    fn slow(&self) -> bool {
        true
    }
    fn budget(&self) -> Duration {
        Duration::from_millis(1500)
    }
    fn resolve(&self, item: &crate::completers::Item) -> Option<String> {
        let data: Value = serde_json::from_str(item.data.as_deref()?).ok()?;
        let path = PathBuf::from(data["path"].as_str()?);
        let client = with(|s| s.docs.get(&path).and_then(|d| d.opened_in.clone()))?;
        let answer = client
            .request("completionItem/resolve", data["item"].clone())
            .wait(Duration::from_secs(3))
            .ok()?;
        features::item_documentation(&answer)
            .or_else(|| answer["detail"].as_str().map(str::to_string))
    }
    fn complete(
        &self,
        ctx: &crate::completers::Context,
        _doc: Option<&DocumentState>,
        cancel: &crate::completers::Cancel,
    ) -> Vec<crate::completers::Item> {
        let Some(path) = &ctx.path else {
            return Vec::new();
        };
        let started = with(|s| {
            let d = s.docs.get(path)?;
            let c = d.opened_in.clone()?;
            let enc = c.encoding();
            let point = ctx.point.min(d.text.len());
            let pending = c.request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": d.uri },
                    "position": kalem_lsp::position::position(&d.text, point, enc).to_json(),
                    // Invoked: the editor asks at a word or after a trigger string.
                    "context": { "triggerKind": 1 },
                }),
            );
            Some((c, pending, d.text.clone(), enc, point))
        });
        let Some((client, pending, text, enc, point)) = started else {
            return Vec::new();
        };
        let deadline = Instant::now() + self.budget();
        let answer = loop {
            if cancel.cancelled() || Instant::now() > deadline {
                client.cancel(&pending);
                return Vec::new();
            }
            // Waits on the answer, a cancel noticed within 20 ms.
            if let Some(a) = pending.wait_for(Duration::from_millis(20)) {
                break a;
            }
        };
        let Ok(v) = answer else { return Vec::new() };
        let resolves = client.capabilities()["completionProvider"]["resolveProvider"]
            .as_bool()
            .unwrap_or(false);
        // Each item's JSON is copied only to fetch its documentation later.
        let (items, _) = features::completion_items(&v, resolves);
        let word_start = {
            let before = &text[..point];
            before
                .char_indices()
                .rev()
                .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '?' || *c == '!')
                .last()
                .map_or(point, |(i, _)| i)
        };
        let mut items: Vec<_> = items
            .into_iter()
            .map(|i| {
                let fallback = (word_start..point, i.insert_text.clone(), i.cursor);
                let (range, insert, cursor) = match &i.edit {
                    Some((r, t, at)) => match kalem_lsp::position::byte_range(&text, r, enc) {
                        Some(r) if r.end == point || r.contains(&point) => {
                            (r.start..point, t.clone(), *at)
                        }
                        _ => fallback,
                    },
                    None => fallback,
                };
                let mut item =
                    crate::completers::Item::new(i.label.clone(), insert, range, kind_of(i.kind));
                item.cursor = cursor.unwrap_or(item.insert.len()).min(item.insert.len());
                item.detail = i.detail.clone().unwrap_or_default();
                item.source = "lsp";
                item.documentation = i.documentation.clone().filter(|d| !d.trim().is_empty());
                item.extra = i
                    .additional
                    .iter()
                    .filter_map(|(r, t)| {
                        Some((kalem_lsp::position::byte_range(&text, r, enc)?, t.clone()))
                    })
                    .collect();
                // Fetched when chosen, from servers that give it so.
                if item.documentation.is_none() && resolves {
                    item.data = Some(json!({ "path": path, "item": i.raw }).to_string());
                }
                (i.sort_text.clone(), item)
            })
            .collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        items.into_iter().map(|(_, i)| i).collect()
    }
}

fn kind_of(k: Option<u8>) -> crate::completers::Kind {
    use crate::completers::Kind as K;
    match k {
        Some(2..=10 | 13 | 20..=25) => K::Symbol,
        Some(15) => K::Snippet,
        Some(14) => K::Keyword,
        _ => K::Word,
    }
}

/// The position of byte `at` of the document at `path` as the server
/// counts it (for tests and the command line).
pub fn position_of(path: &Path, at: usize) -> Option<Position> {
    with(|s| {
        let d = s.docs.get(path)?;
        let c = d.opened_in.as_ref()?;
        Some(kalem_lsp::position::position(&d.text, at, c.encoding()))
    })
}

/// Places as items to choose from: each runs `code.goto` with its place.
pub fn place_items(places: &[Place]) -> Vec<crate::palette::PaletteItem> {
    places
        .iter()
        .map(|p| {
            let file = p
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            crate::palette::PaletteItem {
                id: crate::palette::invocation(
                    "code.goto",
                    &json!({ "path": p.path, "line": p.line, "column": p.column }),
                ),
                title: p.preview.clone(),
                category: format!("{file}:{}", p.line),
                keys: String::new(),
                also: p.path.to_string_lossy().into_owned(),
            }
        })
        .collect()
}

/// A transaction applying an [`Outcome::Edits`]'s edits.
pub fn transaction(edits: &[(Range<usize>, String)], label: &str) -> Option<org_edit::Transaction> {
    let mut tx = org_edit::Transaction::new(label);
    for (r, t) in edits {
        tx.replace(r.clone(), t.clone()).ok()?;
    }
    Some(tx)
}

/// Stops the server of `doc` and starts it again with its documents.
pub fn restart(doc: &DocumentState) -> Result<String, String> {
    let path = code_file(doc)
        .ok_or_else(|| crate::tr!("lsp-not-code"))?
        .to_path_buf();
    // A file with no server yet (none was installed, a start failed):
    // looked up again, and opened anew.
    let fresh = with(|s| {
        let d = s.docs.get(&path)?;
        let has_client = d
            .key
            .as_ref()
            .and_then(|k| s.servers.get(k))
            .is_some_and(|slot| slot.failed.is_none());
        if has_client {
            return None;
        }
        if let Some(k) = d.key.clone() {
            s.servers.remove(&k);
        }
        s.docs.remove(&path);
        Some(())
    });
    if fresh.is_some() {
        sync(doc);
        return describe(doc).ok_or_else(|| crate::tr!("lsp-no-plugin"));
    }
    let (old, name) = with(|s| {
        let d = s
            .docs
            .get_mut(&path)
            .ok_or_else(|| crate::tr!("lsp-no-plugin"))?;
        let key = d.key.clone().ok_or_else(|| {
            d.missing
                .clone()
                .unwrap_or_else(|| crate::tr!("lsp-no-server-short"))
        })?;
        let slot = s
            .servers
            .get_mut(&key)
            .ok_or_else(|| crate::tr!("lsp-not-running"))?;
        let old = slot.client.take();
        slot.crashes = 0;
        slot.failed = None;
        slot.retry_at = Some(Instant::now());
        Ok::<_, String>((old, slot.name.clone()))
    })?;
    if let Some(c) = old {
        // Its exit is not a crash: the slot has no client any more.
        std::thread::spawn(move || c.shutdown());
    }
    Ok(crate::tr!("lsp-restarting-now", server = name))
}

/// What serves `doc`: the plugin, the language and the server, or why
/// none does.
pub fn describe(doc: &DocumentState) -> Option<String> {
    let path = code_file(doc)?;
    with(|s| {
        let d = s.docs.get(path)?;
        Some(match (&d.opened_in, &d.missing) {
            (Some(c), _) => crate::tr!(
                "lsp-describe",
                language = d.language.name.as_str(),
                plugin = d.plugin.id.as_str(),
                server = c.name(),
                command = c.config().command.display().to_string(),
                root = c.config().root.display().to_string()
            ),
            (None, Some(m)) => format!("{} ({}): {m}", d.language.name, d.plugin.id),
            (None, None) => crate::tr!(
                "lsp-describe-off",
                language = d.language.name.as_str(),
                plugin = d.plugin.id.as_str()
            ),
        })
    })
}
