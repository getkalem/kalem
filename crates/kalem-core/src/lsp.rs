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
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use kalem_lsp::features::{self, Diagnostic, Severity};
use kalem_lsp::{Client, Edit, Encoding, Event, Pending, Position, ServerConfig, Wake};
use serde_json::{Value, json};

use crate::DocumentState;
use crate::languages::{self, LanguageSpec, Plugin, Resolved};

/// A server: the plugin, the server's key, the root.
type Key = (String, String, PathBuf);

struct Slot {
    client: Option<Arc<Client>>,
    name: String,
    /// Starts after a crash, for the backoff.
    crashes: u32,
    retry_at: Option<Instant>,
    /// Why it is not running, when it is not.
    failed: Option<String>,
}

struct Doc {
    uri: String,
    language: LanguageSpec,
    plugin: Arc<Plugin>,
    /// `None` when no server serves it (the reason is in `missing`).
    key: Option<Key>,
    missing: Option<String>,
    /// The text the server has.
    text: String,
    /// The document's version when it was last compared.
    version: u64,
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
        }
    }

    fn what(self) -> &'static str {
        match self {
            Kind::Hover => "documentation",
            Kind::Definition => "definitions",
            Kind::Declaration => "declarations",
            Kind::TypeDefinition => "type definitions",
            Kind::Implementation => "implementations",
            Kind::References => "references",
            Kind::Format => "formatting",
            Kind::Symbols => "symbols",
        }
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
}

#[derive(Default)]
struct Service {
    docs: HashMap<PathBuf, Doc>,
    servers: HashMap<Key, Slot>,
    actions: Vec<Action>,
    /// Answers, with the document whose request they answer (none for
    /// what a server says on its own).
    outcomes: Vec<(Option<PathBuf>, Outcome)>,
}

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);
static WAKE: RwLock<Option<Wake>> = RwLock::new(None);

fn with<R>(f: impl FnOnce(&mut Service) -> R) -> R {
    let mut g = SERVICE.lock().expect("lsp");
    f(g.get_or_insert_with(Service::default))
}

/// Sets what wakes the frontend when a server says something.
pub fn set_wake(wake: Wake) {
    *WAKE.write().expect("wake") = Some(wake);
}

fn wake() -> Wake {
    WAKE.read()
        .expect("wake")
        .clone()
        .unwrap_or_else(|| Arc::new(|| {}))
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
        Resolved::Off => Err(format!("language servers are off for {}", plugin.name)),
        Resolved::Missing(why) => Err(why),
        Resolved::Found(spec, program, args) => {
            let mut folders = Vec::new();
            // An umbrella project's applications are folders of its own.
            if spec.root_outermost {
                if let Ok(rd) = std::fs::read_dir(root.join("apps")) {
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
            };
            tracing::info!(server = %spec.name, root = %root.display(), "starting language server");
            Client::start(config, wake())
                .map(|c| (spec.name.clone(), c))
                .map_err(|e| format!("{} did not start: {e}", spec.name))
        }
    }
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
                .unwrap_or_else(|| format!("{} is restarting", slot.name)));
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
                    },
                );
                Err(e)
            }
        }
    }

    fn open(&mut self, path: &Path, text: &str) {
        let first = text.lines().next();
        let Some((plugin, language)) = languages::for_path(path, first) else {
            return;
        };
        let root = root_of(path, &plugin, &language);
        let (key, missing) = match languages::resolve_server(&plugin, &language, Some(&root)) {
            Resolved::Found(spec, ..) => (Some((plugin.id.clone(), spec.key, root)), None),
            Resolved::Off => (None, None),
            Resolved::Missing(m) => (None, Some(m)),
        };
        let mut doc = Doc {
            uri: kalem_lsp::uri::from_path(path),
            language,
            plugin,
            key,
            missing,
            text: text.to_string(),
            version: 0,
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
                    self.outcomes.push((
                        None,
                        Outcome::Message {
                            text: e,
                            error: true,
                        },
                    ));
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
        let Some(d) = s.docs.get_mut(path) else {
            s.open(path, text);
            if let Some(d) = s.docs.get_mut(path) {
                d.version = version;
            }
            return;
        };
        if d.version == version {
            return;
        }
        d.version = version;
        if d.text == text {
            return;
        }
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
            let edit = Edit {
                range: pre..a.len() - suf,
                text: text[pre..b.len() - suf].to_string(),
            };
            c.did_change(&d.uri, &d.text, &[edit], text);
        }
        d.text = text.to_string();
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

/// Moves the servers on: their events into outcomes, crashed servers
/// restarted, answers read. True when there is something to redraw.
pub fn tick() -> bool {
    with(|s| {
        let mut changed = false;
        let now = Instant::now();
        let keys: Vec<Key> = s.servers.keys().cloned().collect();
        for key in keys {
            let Some(slot) = s.servers.get_mut(&key) else {
                continue;
            };
            if let Some(c) = slot.client.clone() {
                for e in c.take_events() {
                    changed = true;
                    match e {
                        Event::Message { level, text } if level <= 2 => {
                            s.outcomes.push((
                                None,
                                Outcome::Message {
                                    text: format!("{}: {text}", c.name()),
                                    error: level == 1,
                                },
                            ));
                        }
                        Event::Exited { code } => {
                            slot.client = None;
                            slot.crashes += 1;
                            if slot.crashes > MAX_CRASHES {
                                let m = format!(
                                    "{} stopped ({code:?}) and was restarted {MAX_CRASHES} times; see `kalem lsp log`",
                                    slot.name
                                );
                                slot.failed = Some(m.clone());
                                s.outcomes.push((
                                    None,
                                    Outcome::Message {
                                        text: m,
                                        error: true,
                                    },
                                ));
                            } else {
                                let wait = Duration::from_millis(500 << slot.crashes.min(6));
                                slot.retry_at = Some(now + wait);
                                s.outcomes.push((
                                    None,
                                    Outcome::Message {
                                        text: format!(
                                            "{} stopped ({code:?}); restarting",
                                            slot.name
                                        ),
                                        error: true,
                                    },
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            } else if slot.retry_at.is_some_and(|t| t <= now) {
                slot.retry_at = None;
                let docs: Vec<PathBuf> = s
                    .docs
                    .iter()
                    .filter(|(_, d)| d.key.as_ref() == Some(&key))
                    .map(|(p, _)| p.clone())
                    .collect();
                let Some(first) = docs.first().and_then(|p| s.docs.get(p)) else {
                    s.servers.remove(&key);
                    continue;
                };
                let (plugin, lang) = (first.plugin.clone(), first.language.clone());
                match start_client(&plugin, &lang, &key.2) {
                    Ok((_, c)) => {
                        let c = Arc::new(c);
                        if let Some(slot) = s.servers.get_mut(&key) {
                            slot.client = Some(c.clone());
                        }
                        for p in docs {
                            if let Some(d) = s.docs.get_mut(&p) {
                                c.did_open(&d.uri, &d.language.id, &d.text);
                                d.opened_in = Some(c.clone());
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(slot) = s.servers.get_mut(&key) {
                            slot.failed = Some(e.clone());
                        }
                        s.outcomes.push((
                            None,
                            Outcome::Message {
                                text: e,
                                error: true,
                            },
                        ));
                    }
                }
                changed = true;
            }
        }
        let mut i = 0;
        while i < s.actions.len() {
            let a = &s.actions[i];
            let answer = match a.pending.poll() {
                Some(r) => Some(r),
                None if a.started.elapsed() > TIMEOUT => {
                    a.client.cancel(&a.pending);
                    Some(Err(kalem_lsp::RpcError {
                        code: 0,
                        message: "no answer in time".into(),
                    }))
                }
                None => None,
            };
            match answer {
                None => i += 1,
                Some(r) => {
                    let a = s.actions.remove(i);
                    let out = outcome(&a, r);
                    s.outcomes.push((Some(a.path.clone()), out));
                    changed = true;
                }
            }
        }
        changed || !s.outcomes.is_empty()
    })
}

/// The answers for the document at `path` (the one a frontend shows),
/// and what servers said on their own.
pub fn take_outcomes(path: Option<&Path>) -> Vec<Outcome> {
    with(|s| {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut s.outcomes)
            .into_iter()
            .partition(|(p, _)| p.is_none() || p.as_deref() == path);
        s.outcomes = rest;
        mine.into_iter().map(|(_, o)| o).collect()
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

fn place(loc: &features::Location, texts: &HashMap<PathBuf, String>, enc: Encoding) -> Place {
    let text = texts
        .get(&loc.path)
        .cloned()
        .or_else(|| std::fs::read_to_string(&loc.path).ok())
        .unwrap_or_default();
    let at = kalem_lsp::position::offset(&text, loc.start, enc);
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    Place {
        path: loc.path.clone(),
        line: u64::from(loc.start.line) + 1,
        column: at - line_start,
        preview: text[line_start..line_end].trim().to_string(),
    }
}

fn outcome(a: &Action, r: Result<Value, kalem_lsp::RpcError>) -> Outcome {
    let v = match r {
        Ok(v) => v,
        Err(e) => {
            return Outcome::Message {
                text: format!("{}: {}", a.client.name(), e.message),
                error: true,
            };
        }
    };
    let nothing = || Outcome::Message {
        text: format!("{}: no {}", a.client.name(), a.kind.what()),
        error: false,
    };
    match a.kind {
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
                label: "Format Document".into(),
            },
            Some(_) => Outcome::Message {
                text: "Already formatted".into(),
                error: false,
            },
            None => Outcome::Message {
                text: format!("{}: overlapping edits", a.client.name()),
                error: true,
            },
        },
        Kind::Symbols => {
            let texts = HashMap::from([(a.path.clone(), a.text.clone())]);
            let places: Vec<Place> = features::symbols(&v)
                .into_iter()
                .map(|(depth, name, _, start)| {
                    let mut p = place(
                        &features::Location {
                            path: a.path.clone(),
                            start,
                            end: start,
                        },
                        &texts,
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
                    title: "Symbols".into(),
                    places,
                }
            }
        }
        _ => {
            let locs = features::locations(&v);
            let mut texts: HashMap<PathBuf, String> =
                HashMap::from([(a.path.clone(), a.text.clone())]);
            let places: Vec<Place> = locs
                .iter()
                .map(|l| {
                    if !texts.contains_key(&l.path)
                        && let Ok(t) = std::fs::read_to_string(&l.path)
                    {
                        texts.insert(l.path.clone(), t);
                    }
                    place(l, &texts, a.enc)
                })
                .collect();
            match places.len() {
                0 => nothing(),
                1 if a.kind != Kind::References => {
                    Outcome::Jump(places.into_iter().next().expect("one"))
                }
                _ => Outcome::Places {
                    title: match a.kind {
                        Kind::References => "References",
                        Kind::Implementation => "Implementations",
                        _ => "Definitions",
                    }
                    .into(),
                    places,
                },
            }
        }
    }
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
    let at = doc.selection.head;
    let version = doc.version();
    with(|s| {
        let Some(d) = s.docs.get(&path) else {
            return Err(no_server(doc));
        };
        let c = match &d.opened_in {
            Some(c) => c.clone(),
            None => {
                return Err(d.missing.clone().unwrap_or_else(|| {
                    format!(
                        "No language server for {} (see `kalem lsp status`)",
                        d.language.name
                    )
                }));
            }
        };
        if c.has_exited() {
            return Err(format!("{} is restarting", c.name()));
        }
        if !c.is_ready() {
            return Err(format!("{} is starting", c.name()));
        }
        if !c.provides(kind.provider()) {
            return Err(format!("{} does not provide {}", c.name(), kind.what()));
        }
        let enc = c.encoding();
        let td = json!({ "uri": d.uri });
        let params = match kind {
            Kind::Format => {
                let tab = 2;
                json!({ "textDocument": td, "options": { "tabSize": tab, "insertSpaces": true } })
            }
            Kind::Symbols => json!({ "textDocument": td }),
            Kind::References => json!({
                "textDocument": td,
                "position": kalem_lsp::position::position(&d.text, at, enc).to_json(),
                "context": { "includeDeclaration": true },
            }),
            _ => json!({
                "textDocument": td,
                "position": kalem_lsp::position::position(&d.text, at, enc).to_json(),
            }),
        };
        let pending = c.request(kind.method(), params);
        let text = d.text.clone();
        s.actions.push(Action {
            kind,
            path,
            version,
            text,
            enc,
            pending,
            client: c,
            started: Instant::now(),
        });
        Ok(())
    })
}

/// Why no server answers for `doc`: no plugin for its kind of file.
fn no_server(doc: &DocumentState) -> String {
    let kind = match &doc.meta.mode {
        crate::mode::DocumentMode::Text { language: Some(l) } => format!(".{l} files"),
        crate::mode::DocumentMode::Text { language: None } => "plain text".into(),
        _ => format!("{} documents", doc.document_type()),
    };
    format!("No language server for {kind}: a language plugin from getkalem/plugins provides one")
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
        let Some(d) = s.docs.get(path) else {
            return Vec::new();
        };
        let Some(c) = &d.opened_in else {
            return Vec::new();
        };
        features::diagnostics(&d.text, &c.diagnostics(&d.uri), c.encoding())
    })
}

/// The status bar's word on the document at `path`, whose cursor is at
/// `at`: the diagnostic there, else the server's progress, else the
/// counts.
pub fn status(path: &Path, at: usize) -> Option<String> {
    let (diags, progress, name, missing) = with(|s| {
        let d = s.docs.get(path)?;
        let Some(c) = &d.opened_in else {
            return Some((Vec::new(), None, String::new(), d.missing.clone()));
        };
        let diags = features::diagnostics(&d.text, &c.diagnostics(&d.uri), c.encoding());
        let progress = if c.has_exited() {
            Some(format!("{} restarting", c.name()))
        } else if !c.is_ready() {
            Some(format!("{} starting", c.name()))
        } else {
            c.progress()
        };
        Some((diags, progress, c.name().to_string(), None))
    })?;
    if let Some(m) = missing {
        return Some(m);
    }
    let text = with(|s| s.docs.get(path).map(|d| d.text.clone())).unwrap_or_default();
    let line_of = |b: usize| text[..b.min(text.len())].matches('\n').count();
    let line = line_of(at);
    if let Some(d) = diags.iter().find(|d| line_of(d.range.start) == line) {
        let first = d.message.lines().next().unwrap_or("");
        let src = d.source.as_deref().unwrap_or(&name);
        return Some(format!("{src}: {first}"));
    }
    if let Some(p) = progress {
        return Some(format!("{name}: {p}").replace(&format!("{name}: {name}"), &name));
    }
    let errors = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = diags
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    match (errors, warnings) {
        (0, 0) => None,
        (e, w) => Some(format!("{name}: {e} errors, {w} warnings")),
    }
}

/// The diagnostics of every open document, for the problems list.
pub fn all_problems() -> Vec<Place> {
    with(|s| {
        let mut out = Vec::new();
        let mut paths: Vec<&PathBuf> = s.docs.keys().collect();
        paths.sort();
        for p in paths {
            let d = &s.docs[p];
            let Some(c) = &d.opened_in else { continue };
            for diag in features::diagnostics(&d.text, &c.diagnostics(&d.uri), c.encoding()) {
                let at = diag.range.start;
                let line_start = d.text[..at].rfind('\n').map_or(0, |i| i + 1);
                out.push(Place {
                    path: p.clone(),
                    line: d.text[..at].matches('\n').count() as u64 + 1,
                    column: at - line_start,
                    preview: format!(
                        "{}: {}",
                        match diag.severity {
                            Severity::Error => "error",
                            Severity::Warning => "warning",
                            Severity::Information => "info",
                            Severity::Hint => "hint",
                        },
                        diag.message.lines().next().unwrap_or("")
                    ),
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
                Some(c) if c.has_exited() => "exited".to_string(),
                Some(c) if c.is_ready() => format!("ready, {} documents", c.open_documents()),
                Some(_) => "starting".to_string(),
                None => slot.failed.clone().unwrap_or_else(|| "restarting".into()),
            };
            out.push(format!(
                "{} ({plugin} {key}) in {}: {state}",
                slot.name,
                root.display()
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

/// Stops every server (on quit).
pub fn shutdown_all() {
    let clients: Vec<Arc<Client>> = with(|s| {
        s.docs.clear();
        s.servers.drain().filter_map(|(_, v)| v.client).collect()
    });
    for c in clients {
        c.shutdown();
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
                    "context": { "triggerKind": if ctx.requested { 1 } else { 1 } },
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
            if let Some(a) = pending.poll() {
                break a;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let Ok(v) = answer else { return Vec::new() };
        let (items, _) = features::completion_items(&v);
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
                let (range, insert) = match &i.edit {
                    Some((r, t)) => match kalem_lsp::position::byte_range(&text, r, enc) {
                        Some(r) if r.end == point || r.contains(&point) => {
                            (r.start..point, t.clone())
                        }
                        _ => (word_start..point, i.insert_text.clone()),
                    },
                    None => (word_start..point, i.insert_text.clone()),
                };
                let mut item =
                    crate::completers::Item::new(i.label.clone(), insert, range, kind_of(i.kind));
                item.detail = i.detail.clone().unwrap_or_default();
                item.source = "lsp";
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
    let path = code_file(doc).ok_or("Not a code file")?.to_path_buf();
    let (old, name) = with(|s| {
        let d = s
            .docs
            .get_mut(&path)
            .ok_or("No language plugin serves this file")?;
        let key = d.key.clone().ok_or_else(|| {
            d.missing
                .clone()
                .unwrap_or_else(|| "No language server".into())
        })?;
        let slot = s.servers.get_mut(&key).ok_or("The server is not running")?;
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
    Ok(format!("Restarting {name}"))
}

/// What serves `doc`: the plugin, the language and the server, or why
/// none does.
pub fn describe(doc: &DocumentState) -> Option<String> {
    let path = code_file(doc)?;
    with(|s| {
        let d = s.docs.get(path)?;
        Some(match (&d.opened_in, &d.missing) {
            (Some(c), _) => format!(
                "{} ({}) by {} {} in {}",
                d.language.name,
                d.plugin.id,
                c.name(),
                c.config().command.display(),
                c.config().root.display()
            ),
            (None, Some(m)) => format!("{} ({}): {m}", d.language.name, d.plugin.id),
            (None, None) => format!(
                "{} ({}): language servers off",
                d.language.name, d.plugin.id
            ),
        })
    })
}
