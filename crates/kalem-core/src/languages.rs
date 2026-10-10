//! Language plugins (design §11.12, D57; T3.8.4): a plugin that serves a
//! programming language is a manifest, Sublime syntax files and settings,
//! no code. It needs no WebAssembly runtime: the core reads the manifest,
//! adds the syntaxes to the highlighter and starts the language servers
//! it names through the one client of the core (`kalem-lsp`).
//!
//! Plugins are folders holding a `plugin.json`, found in
//! `CONFIG/plugins/` and in the folders of `KALEM_PLUGIN_PATH` (a list
//! like `PATH`; each entry a plugin or a folder of plugins). The manifest's
//! `languages` and `servers` sections are described in the Book's chapter
//! "Language plugins"; `plugins/elixir` in `getkalem/plugins` is the
//! first.
//!
//! A user's settings for a plugin live under `plugins."ID"`:
//! `server` (a server's key, `auto` for the first one found, `off`),
//! `settings` (merged over the server's own), and per server
//! `servers.KEY.command` (the program and its arguments) and
//! `servers.KEY.env`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Map, Value};

/// A language a plugin serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSpec {
    /// The language id sent to servers: `elixir`.
    pub id: String,
    /// Its name: `Elixir`.
    pub name: String,
    /// The name of its syntax (the highlighter's language).
    pub syntax: Option<String>,
    /// File extensions, without the dot, longest matched first
    /// (`html.heex` before `heex`).
    pub extensions: Vec<String>,
    /// Whole file names: `mix.lock`.
    pub filenames: Vec<String>,
    /// Interpreters of a `#!` line.
    pub shebangs: Vec<String>,
    /// The line comment token.
    pub line_comment: Option<String>,
    /// The block comment tokens.
    pub block_comment: Option<(String, String)>,
    /// The servers in order of preference (keys of the plugin's servers):
    /// the first found serves the language's files.
    pub servers: Vec<String>,
    /// Servers that serve the files beside it (`alongside`): a linter
    /// beside the type server (ruff beside basedpyright), a server of one
    /// part of the file (crates-lsp's versions in a `Cargo.toml`, beside
    /// taplo). Their diagnostics, completions and code actions join the
    /// first's; a question one server answers goes to the first that
    /// has it, the language's own server first.
    pub alongside: Vec<String>,
}

/// A language server a plugin can start.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerSpec {
    /// Its key in the manifest: `elixir-ls`.
    pub key: String,
    /// Its name: `ElixirLS`.
    pub name: String,
    /// The program and its arguments.
    pub command: Vec<String>,
    /// Programs tried in order when the first word of `command` is not
    /// found (bare names on the PATH, or paths).
    pub candidates: Vec<String>,
    /// Folders under the root searched first (`node_modules/.bin`).
    pub local_dirs: Vec<String>,
    /// Variables added to its environment.
    pub env: Vec<(String, String)>,
    /// Files that mark the root.
    pub root_markers: Vec<String>,
    /// The outermost marked folder is the root (an umbrella project), not
    /// the nearest.
    pub root_outermost: bool,
    /// No server for a file outside a marked folder (a script beside no
    /// `mix.exs`), rather than one per folder.
    pub require_root: bool,
    /// `initializationOptions`.
    pub initialization_options: Value,
    /// Its settings, before the user's.
    pub settings: Value,
    /// How to install it, said when it is not found or does not start.
    pub install: Option<String>,
    /// Log lines that start and end work the server tells only in its log
    /// (`busyLog`: `{"start": [...], "done": [...]}`).
    pub busy_log: (Vec<String>, Vec<String>),
    /// The arguments that make its program print its version and end
    /// (`version`: `["--version"]`), for [`server_version`]; none when
    /// the manifest gives none, since a server's program may not end
    /// when it is asked something it does not know.
    pub version: Vec<String>,
    /// Client capabilities added to Kalem's (`capabilities`): the
    /// extensions the server sends only to a client that asks
    /// (`{"experimental": {"serverStatusNotification": true}}`).
    pub capabilities: Value,
    /// Its notification of its state (`status`), shown in the status bar.
    pub status: Option<kalem_lsp::StatusSpec>,
}

/// A request of a server's own that one of Kalem's commands sends (the
/// manifest's `requests`, keyed by the command): rust-analyzer's
/// `rust-analyzer/expandMacro` for `code.expandMacro`. Kalem knows the
/// shapes of the questions and of the answers, not the methods, so the
/// same command serves every server that has such a request.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestSpec {
    /// Kalem's command: `code.expandMacro`.
    pub command: String,
    /// The server it is sent to (a key of the plugin's servers); any of
    /// the plugin's when none is named.
    pub server: Option<String>,
    /// The method: `rust-analyzer/expandMacro`.
    pub method: String,
    /// What is asked about (`params`): `position` (the document and the
    /// cursor), `document`, `range` (the selection), `ranges` (the
    /// selection in a list) or `none`.
    pub params: String,
    /// Fields added to the question (`extra`): `{"direction": "Up"}`.
    pub extra: Value,
    /// What the answer is (`shape`): `text` (shown as documentation is),
    /// `url` (opened in the browser), `location` (gone to), `edits`
    /// (applied to the document) or `none`.
    pub shape: String,
    /// Where the answer's text or URL is (`answer`): JSON pointers tried
    /// in order, `""` for the whole answer.
    pub answer: Vec<String>,
    /// Where its title is (`title`), a JSON pointer.
    pub title: Option<String>,
    /// The language a text answer is highlighted as (`language`).
    pub language: Option<String>,
}

/// A loaded language plugin.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Its id: `org.kalem.elixir`.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// Its folder.
    pub dir: PathBuf,
    /// Its languages.
    pub languages: Vec<LanguageSpec>,
    /// Its servers by key.
    pub servers: Vec<ServerSpec>,
    /// Its commands by name (`format`, `test`, `testAtPoint`, `run`):
    /// programs and arguments with `{file}` and `{line}` filled in.
    pub commands: HashMap<String, Vec<String>>,
    /// The syntaxes it added.
    pub syntaxes: Vec<String>,
    /// Its servers' own requests, by Kalem's command.
    pub requests: Vec<RequestSpec>,
    /// When it serves a file ([`crate::applies::Applies::of`] of its
    /// manifest): what the index says of it too.
    pub applies: crate::applies::Applies,
}

impl Plugin {
    /// Its language for the file named `name` (and `lower`), whose first
    /// line is `first_line`: by the whole name, then the longest
    /// extension, then the interpreter of its `#!` line.
    fn language(&self, name: &str, lower: &str, first_line: Option<&str>) -> Option<&LanguageSpec> {
        if let Some(l) = self
            .languages
            .iter()
            .find(|l| l.filenames.iter().any(|f| f == name))
        {
            return Some(l);
        }
        let mut best: Option<(usize, &LanguageSpec)> = None;
        for l in &self.languages {
            for e in &l.extensions {
                let suffix = format!(".{}", e.to_ascii_lowercase());
                if lower.ends_with(&suffix) && best.is_none_or(|b| e.len() > b.0) {
                    best = Some((e.len(), l));
                }
            }
        }
        if let Some((_, l)) = best {
            return Some(l);
        }
        let interp = first_line?.strip_prefix("#!")?;
        let mut words = interp.split_whitespace();
        let mut prog = words.next()?.rsplit('/').next()?;
        if prog == "env" {
            prog = words.find(|w| !w.starts_with('-'))?;
        }
        self.languages
            .iter()
            .find(|l| l.shebangs.iter().any(|s| s == prog))
    }

    /// The server with key `key`.
    pub fn server(&self, key: &str) -> Option<&ServerSpec> {
        self.servers.iter().find(|s| s.key == key)
    }

    /// The request command `command` sends to its server `server`.
    pub fn request(&self, command: &str, server: &str) -> Option<&RequestSpec> {
        self.requests
            .iter()
            .find(|r| r.command == command && r.server.as_deref().is_none_or(|s| s == server))
    }
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn env_pairs(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// A server's notification of its state (`status`): `method`, and JSON
/// pointers into it for its `text`, its `level` (with the server's words
/// for a `warning` and an `error`), and the values that say it is `idle`.
fn status_spec(v: &Value) -> Option<kalem_lsp::StatusSpec> {
    Some(kalem_lsp::StatusSpec {
        method: v["method"].as_str()?.to_string(),
        text: strings(&v["text"]),
        level: v["level"].as_str().map(str::to_string),
        warning: strings(&v["warning"]),
        error: strings(&v["error"]),
        idle: v["idle"]
            .as_object()
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
    })
}

/// Reads a manifest; `Ok(None)` when it has no `languages`.
pub fn parse_manifest(dir: &Path, text: &str) -> Result<Option<Plugin>, String> {
    let m: Value = serde_json::from_str(text).map_err(|e| format!("plugin.json: {e}"))?;
    let Some(langs) = m.get("languages").and_then(Value::as_array) else {
        return Ok(None);
    };
    let id = m["id"].as_str().ok_or("plugin.json: no `id`")?.to_string();
    let languages = langs
        .iter()
        .map(|l| {
            let lid = l["id"]
                .as_str()
                .ok_or("a language without `id`")?
                .to_string();
            let mut extensions = strings(&l["extensions"]);
            extensions.sort_by_key(|e| std::cmp::Reverse(e.len()));
            Ok(LanguageSpec {
                name: l["name"].as_str().unwrap_or(&lid).to_string(),
                syntax: l["syntax"].as_str().map(str::to_string),
                extensions,
                filenames: strings(&l["filenames"]),
                shebangs: strings(&l["shebangs"]),
                line_comment: l["comment"]["line"].as_str().map(str::to_string),
                block_comment: match strings(&l["comment"]["block"]).as_slice() {
                    [a, b] => Some((a.clone(), b.clone())),
                    _ => None,
                },
                servers: strings(&l["servers"]),
                alongside: strings(&l["alongside"]),
                id: lid,
            })
        })
        .collect::<Result<Vec<_>, &str>>()
        .map_err(|e| format!("plugin.json: {e}"))?;
    let servers = m["servers"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(key, s)| ServerSpec {
                    key: key.clone(),
                    name: s["name"].as_str().unwrap_or(key).to_string(),
                    command: strings(&s["command"]),
                    candidates: strings(&s["candidates"]),
                    local_dirs: strings(&s["localDirs"]),
                    env: env_pairs(&s["env"]),
                    root_markers: strings(&s["rootMarkers"]),
                    root_outermost: s["rootOutermost"].as_bool().unwrap_or(false),
                    require_root: s["requireRoot"].as_bool().unwrap_or(false),
                    initialization_options: s["initializationOptions"].clone(),
                    settings: s["settings"].clone(),
                    install: s["install"].as_str().map(str::to_string),
                    version: strings(&s["version"]),
                    capabilities: s["capabilities"].clone(),
                    status: status_spec(&s["status"]),
                    busy_log: (
                        strings(&s["busyLog"]["start"]),
                        strings(&s["busyLog"]["done"]),
                    ),
                })
                .collect()
        })
        .unwrap_or_default();
    let commands = m["commands"]
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), strings(v))).collect())
        .unwrap_or_default();
    let requests = m["requests"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(command, r)| {
                    Some(RequestSpec {
                        command: command.clone(),
                        server: r["server"].as_str().map(str::to_string),
                        method: r["method"].as_str()?.to_string(),
                        params: r["params"].as_str().unwrap_or("position").to_string(),
                        extra: r["extra"].clone(),
                        shape: r["shape"].as_str().unwrap_or("text").to_string(),
                        answer: match strings(&r["answer"]) {
                            a if a.is_empty() => vec![String::new()],
                            a => a,
                        },
                        title: r["title"].as_str().map(str::to_string),
                        language: r["language"].as_str().map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(Plugin {
        name: m["name"].as_str().unwrap_or(&id).to_string(),
        version: m["version"].as_str().unwrap_or("").to_string(),
        id,
        dir: dir.to_path_buf(),
        languages,
        servers,
        commands,
        syntaxes: Vec::new(),
        requests,
        applies: crate::applies::Applies::of(&m),
    }))
}

/// The plugins loaded, and the problems met loading them.
#[derive(Debug, Default)]
struct Loaded {
    plugins: Vec<Arc<Plugin>>,
    problems: Vec<String>,
    dirs: Vec<PathBuf>,
    /// Which load this is: the syntaxes built on a thread
    /// ([`load`]) are told to this one only.
    load: u64,
}

impl Loaded {
    /// What registering the plugins' syntaxes found: each plugin's
    /// syntaxes (`files`: its files of syntaxes, by plugin), the files
    /// that could not be loaded.
    fn registered(&mut self, files: &[Vec<String>], r: &kalem_highlight::Registered) {
        for (f, e) in &r.errors {
            self.problems.push(format!("{f}: {e}"));
        }
        for (p, files) in self.plugins.iter_mut().zip(files) {
            let syntaxes = r
                .names
                .iter()
                .filter(|(f, _)| files.contains(f))
                .map(|(_, n)| n.clone())
                .collect();
            Arc::make_mut(p).syntaxes = syntaxes;
        }
    }
}

static LOADED: RwLock<Option<Loaded>> = RwLock::new(None);
static USER: RwLock<Value> = RwLock::new(Value::Null);
static LOAD: Mutex<()> = Mutex::new(());

/// The folders plugins are looked for in.
pub fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(p) = std::env::var_os("KALEM_PLUGIN_PATH") {
        dirs.extend(std::env::split_paths(&p).filter(|d| !d.as_os_str().is_empty()));
    }
    if let Some(c) = crate::settings::config_dir() {
        dirs.push(c.join("plugins"));
    }
    dirs
}

/// The plugin folders under `dirs`: a folder with a `plugin.json`, or the
/// folders in it that have one.
fn plugin_folders(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in dirs {
        if d.join("plugin.json").is_file() {
            out.push(d.clone());
            continue;
        }
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        let mut sub: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("plugin.json").is_file())
            .collect();
        sub.sort();
        out.extend(sub);
    }
    out
}

/// A plugin and its syntax files.
fn load_plugin(dir: &Path) -> Result<Option<(Plugin, Vec<kalem_highlight::SyntaxSource>)>, String> {
    let text = std::fs::read_to_string(dir.join("plugin.json")).map_err(|e| e.to_string())?;
    let Some(plugin) = parse_manifest(dir, &text)? else {
        return Ok(None);
    };
    let m: Value = serde_json::from_str(&text).unwrap_or_default();
    let mut sources = Vec::new();
    for (key, base_only) in [("syntaxes", false), ("syntaxBases", true)] {
        for rel in strings(&m[key]) {
            let path = dir.join(&rel);
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            sources.push(kalem_highlight::SyntaxSource {
                file: rel.rsplit('/').next().unwrap_or(&rel).to_string(),
                text,
                base_only,
            });
        }
    }
    Ok(Some((plugin, sources)))
}

/// Loads the language plugins of [`plugin_dirs`], once; again only when
/// the folders change. Cheap after the first call. Their syntaxes, when
/// not cached (a start after an update), are built on a thread of their
/// own: the first screen does not wait for them, a file in a plugin's
/// language does ([`kalem_highlight::Language::find`]).
pub fn load() {
    load_dirs(&plugin_dirs(), true);
}

/// Loads the language plugins of [`plugin_dirs`] again, after one was
/// installed or removed.
pub fn reload() {
    let dirs = LOADED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map_or_else(plugin_dirs, |l| l.dirs.clone());
    *LOADED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    load_from(&dirs);
}

/// Loads the language plugins in `dirs` (replacing those loaded before),
/// their syntaxes in place on return.
pub fn load_from(dirs: &[PathBuf]) {
    load_dirs(dirs, false);
}

/// Waits for the plugins' syntaxes being built ([`load`]).
pub fn wait() {
    kalem_highlight::wait();
}

fn load_dirs(dirs: &[PathBuf], later: bool) {
    static LOADS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let _guard = LOAD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if LOADED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|l| l.dirs == dirs)
    {
        return;
    }
    let mut loaded = Loaded {
        dirs: dirs.to_vec(),
        load: LOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ..Loaded::default()
    };
    // Every plugin's syntaxes are added at once: building the set takes
    // most of a second, so it is built once and cached by its sources.
    let mut plugins = Vec::new();
    let mut sources = Vec::new();
    for folder in plugin_folders(dirs) {
        match load_plugin(&folder) {
            Ok(Some((p, s))) => {
                tracing::info!(plugin = %p.id, dir = %folder.display(), "language plugin loaded");
                let files: Vec<String> = s
                    .iter()
                    .filter(|s| !s.base_only)
                    .map(|s| s.file.clone())
                    .collect();
                sources.extend(s);
                plugins.push((p, files));
            }
            Ok(None) => {}
            Err(e) => loaded.problems.push(format!("{}: {e}", folder.display())),
        }
    }
    // Each language's names and extensions name its syntax.
    let aliases: Vec<(String, String)> = plugins
        .iter()
        .flat_map(|(p, _)| p.languages.iter())
        .filter_map(|l| Some((l, l.syntax.clone()?)))
        .flat_map(|(l, syntax)| {
            std::iter::once(l.id.clone())
                .chain(l.extensions.iter().cloned())
                .map(move |n| (n.to_ascii_lowercase(), syntax.clone()))
        })
        .collect();
    kalem_highlight::set_aliases(aliases);
    let (plugins, files): (Vec<_>, Vec<_>) = plugins.into_iter().unzip();
    loaded.plugins = plugins.into_iter().map(Arc::new).collect();
    let cache = crate::logging::state_dir().map(|d| d.join("cache"));
    let later = later && !sources.is_empty();
    if sources.is_empty() {
        kalem_highlight::reset();
    } else if !later {
        let r = kalem_highlight::register_cached(&sources, cache.as_deref());
        loaded.registered(&files, &r);
    }
    let load = loaded.load;
    *LOADED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(loaded);
    if !later {
        return;
    }
    // The syntaxes told to this load, if it is still the one loaded.
    let tell = move |files: &[Vec<String>], r: &kalem_highlight::Registered| {
        if let Some(l) = LOADED
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .filter(|l| l.load == load)
        {
            l.registered(files, r);
        }
    };
    let files = Arc::new(files);
    let f = files.clone();
    let started = std::time::Instant::now();
    let done = move |r: kalem_highlight::Registered| {
        tracing::info!(ms = started.elapsed().as_millis(), "plugin syntaxes built");
        tell(&f, &r);
    };
    if let Some(r) = kalem_highlight::register_cached_later(sources, cache, done) {
        tell(&files, &r);
    }
}

/// The language plugins loaded.
pub fn plugins() -> Vec<Arc<Plugin>> {
    LOADED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|l| l.plugins.clone())
        .unwrap_or_default()
}

/// The problems met loading plugins, their syntaxes' with them.
pub fn problems() -> Vec<String> {
    wait();
    LOADED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|l| l.problems.clone())
        .unwrap_or_default()
}

/// Sets the user's `plugins` settings table (from
/// [`crate::settings::Config::apply_process_settings`]).
pub fn set_user_settings(plugins: Option<&Value>) {
    *USER
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        plugins.cloned().unwrap_or(Value::Null);
}

/// The user's settings of plugin `id`.
pub fn user_settings(id: &str) -> Value {
    USER.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .cloned()
        .unwrap_or(Value::Null)
}

/// `over` merged into `base`, objects key by key.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, o) if !o.is_null() => *b = o.clone(),
        _ => {}
    }
}

/// The language of a file whose first line is `first_line`: the plugin
/// whose declaration serves it ([`crate::applies`]), the surest match
/// first and the later plugin among equals, as its syntaxes are; then its
/// language for the file (by the whole name, the longest extension, the
/// `#!` line), or its first when the plugin serves the file by what it
/// adds to its languages (a marker).
pub fn for_path(path: &Path, first_line: Option<&str>) -> Option<(Arc<Plugin>, LanguageSpec)> {
    let name = path.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();
    let head = first_line.map(str::as_bytes);
    let plugins = plugins();
    let mut best: Option<(crate::applies::Strength, &Arc<Plugin>)> = None;
    for p in plugins.iter().rev() {
        let Some(why) = p.applies.serves(path, head) else {
            continue;
        };
        let s = why.strength();
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, p));
        }
    }
    let (_, p) = best?;
    let l = p
        .language(name, &lower, first_line)
        .or_else(|| p.languages.first())?
        .clone();
    Some((p.clone(), l))
}

/// A plugin's comment marker as the `'static` text the comment commands
/// take: each distinct marker kept once for the run (a few bytes).
fn intern(marker: &str) -> &'static str {
    static MARKERS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let Ok(mut all) = MARKERS.lock() else {
        return "#";
    };
    if let Some(m) = all.iter().find(|m| **m == marker) {
        return m;
    }
    let m: &'static str = Box::leak(marker.to_string().into_boxed_str());
    all.push(m);
    m
}

/// How the language plugins say a language writes comments: by the
/// language's id or name, or one of its extensions (`heex`, `ex`).
pub fn comment_style(language: &str) -> Option<crate::code::CommentStyle> {
    let l = language.to_ascii_lowercase();
    plugins().iter().rev().find_map(|p| {
        let lang = p.languages.iter().find(|x| {
            x.id.eq_ignore_ascii_case(&l)
                || x.name.eq_ignore_ascii_case(&l)
                || x.extensions.iter().any(|e| e.eq_ignore_ascii_case(&l))
        })?;
        match (&lang.line_comment, &lang.block_comment) {
            (Some(line), _) => Some(crate::code::CommentStyle::Line(intern(line))),
            (None, Some((a, b))) => Some(crate::code::CommentStyle::Block(intern(a), intern(b))),
            (None, None) => None,
        }
    })
}

/// The settings a server starts with: its own, then the user's
/// `plugins."ID".settings`.
pub fn server_settings(plugin: &Plugin, server: &ServerSpec) -> Value {
    let mut s = if server.settings.is_null() {
        Value::Object(Map::new())
    } else {
        server.settings.clone()
    };
    merge(&mut s, &user_settings(&plugin.id)["settings"]);
    s
}

/// Why no server runs for a language, or which program runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// The server, its program and arguments.
    Found(Box<ServerSpec>, PathBuf, Vec<String>),
    /// The user turned servers off for the plugin.
    Off,
    /// None of the servers is installed: what to do.
    Missing(String),
}

/// The server to start for `lang` of `plugin` in `root`: the user's
/// choice, else the first of the language's servers whose program is
/// found.
pub fn resolve_server(plugin: &Plugin, lang: &LanguageSpec, root: Option<&Path>) -> Resolved {
    let user = user_settings(&plugin.id);
    let choice = user["server"].as_str().unwrap_or("auto");
    if choice == "off" {
        return Resolved::Off;
    }
    let keys: Vec<&String> = if choice == "auto" {
        lang.servers.iter().collect()
    } else {
        lang.servers.iter().filter(|k| *k == choice).collect()
    };
    let mut missing = Vec::new();
    for key in keys {
        match resolve_key(plugin, key, root) {
            Some(Ok((spec, path, args))) => return Resolved::Found(spec, path, args),
            Some(Err(why)) => missing.push(why),
            None => {}
        }
    }
    if missing.is_empty() {
        Resolved::Missing(crate::tr!(
            "lsp-plugin-no-server",
            plugin = plugin.name.as_str(),
            language = lang.name.as_str()
        ))
    } else {
        Resolved::Missing(missing.join("; "))
    }
}

/// Server `key` of `plugin` with the user's `servers.KEY` settings
/// (`command`, `env`) over the manifest's: its program found, with its
/// arguments, or what to do to install it; `None` when the plugin has no
/// such server or no command for it.
#[allow(clippy::type_complexity)]
pub fn resolve_key(
    plugin: &Plugin,
    key: &str,
    root: Option<&Path>,
) -> Option<Result<(Box<ServerSpec>, PathBuf, Vec<String>), String>> {
    let mut spec = plugin.server(key)?.clone();
    let user = user_settings(&plugin.id);
    let over = &user["servers"][key];
    let command = strings(&over["command"]);
    if !command.is_empty() {
        spec.command = command;
        spec.candidates.clear();
    }
    spec.env.extend(env_pairs(&over["env"]));
    let (program, args) = spec.command.split_first()?;
    let args = args.to_vec();
    let candidates = std::iter::once(program.clone()).chain(spec.candidates.iter().cloned());
    let found = candidates
        .into_iter()
        .find_map(|c| kalem_lsp::find_program(&c, root, &spec.local_dirs));
    Some(match found {
        Some(path) => Ok((Box::new(spec), path, args)),
        None => Err(match &spec.install {
            Some(how) => crate::tr!(
                "lsp-not-installed-how",
                server = spec.name.as_str(),
                how = how.as_str()
            ),
            None => crate::tr!("lsp-not-installed", server = spec.name.as_str()),
        }),
    })
}

/// The servers that serve `lang`'s files beside its own (`alongside`),
/// by key: none when the user turned the plugin's servers off, none of
/// those the user turned off (`servers.KEY.enabled = false`).
pub fn alongside(plugin: &Plugin, lang: &LanguageSpec) -> Vec<String> {
    let user = user_settings(&plugin.id);
    if user["server"].as_str() == Some("off") {
        return Vec::new();
    }
    lang.alongside
        .iter()
        .filter(|k| plugin.server(k).is_some())
        .filter(|k| user["servers"][k.as_str()]["enabled"].as_bool() != Some(false))
        .cloned()
        .collect()
}

/// How long [`server_version`] waits for the program to end, in seconds.
const VERSION_WAIT: u32 = 5;

/// What the program found for `spec` says its version is: the manifest's
/// `version` arguments run in `root`, with the server's environment (so a
/// toolchain's proxy, rustup's or pyenv's, picks the project's toolchain),
/// and the first line it prints. `Err` says why it does not run, in its
/// own words when it wrote any on its standard error: a program found
/// is not always one that runs (rustup's proxy for a component not
/// installed is on the `PATH` all the same). `None` when the manifest
/// gives no `version`.
pub fn server_version(
    spec: &ServerSpec,
    program: &Path,
    root: &Path,
) -> Option<Result<String, String>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    if spec.version.is_empty() {
        return None;
    }
    let mut cmd = Command::new(program);
    cmd.args(&spec.version)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if root.is_dir() {
        cmd.current_dir(root);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Some(Err(e.to_string())),
    };
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut p) = pipe {
                let mut bytes = Vec::new();
                let _ = p.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed().as_secs() < u64::from(VERSION_WAIT) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let Some(status) = status else {
        // Its output is not waited for: a process it started may hold
        // the pipes.
        return Some(Err(crate::tr!(
            "lsp-version-no-answer",
            seconds = VERSION_WAIT
        )));
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    let first = |t: &str| {
        t.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    };
    Some(if status.success() {
        first(&out)
            .or_else(|| first(&err))
            .ok_or_else(|| exit_text(status.code()))
    } else {
        let lines: Vec<&str> = err
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(3)
            .collect();
        Err(if lines.is_empty() {
            exit_text(status.code())
        } else {
            lines.join(" ")
        })
    })
}

/// A process's end in words: its exit code, or the signal that ended it.
pub fn exit_text(code: Option<i32>) -> String {
    match code {
        Some(code) => crate::tr!("lsp-exit-code", code = code),
        None => crate::tr!("lsp-exit-signal"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server's own requests (`requests`): keyed by Kalem's command,
    /// asked about the cursor and answered as text unless said otherwise,
    /// the whole answer read unless pointers say where; one naming a
    /// server only for it; one without a method left out.
    #[test]
    fn requests_read() {
        let p = parse_manifest(
            Path::new("/p"),
            r#"{"id": "x", "languages": [], "requests": {
                "code.expandMacro": {"method": "s/expand", "answer": ["/expansion"],
                                     "title": "/name", "language": "rust"},
                "code.reloadProject": {"server": "a", "method": "s/reload", "params": "none",
                                       "shape": "none", "extra": {"all": true}},
                "code.nothing": {"shape": "text"}
            }}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.requests.len(), 2);
        let r = p.request("code.expandMacro", "any").unwrap();
        assert_eq!((r.params.as_str(), r.shape.as_str()), ("position", "text"));
        assert_eq!(r.answer, ["/expansion"]);
        assert_eq!(
            (r.title.as_deref(), r.language.as_deref()),
            (Some("/name"), Some("rust"))
        );
        assert!(p.request("code.reloadProject", "b").is_none());
        let r = p.request("code.reloadProject", "a").unwrap();
        assert_eq!(r.answer, [""]);
        assert_eq!(r.extra, serde_json::json!({"all": true}));
    }

    /// A server's notification of its state (`status`) and the
    /// capabilities that make it send one: rust-analyzer's health read as
    /// Kalem's, busy until quiescent; a server without one has none.
    #[test]
    fn server_status_read() {
        let p = parse_manifest(
            Path::new("/p"),
            r#"{"id": "x", "languages": [], "servers": {
                "ra": {"command": ["ra"],
                       "capabilities": {"experimental": {"serverStatusNotification": true}},
                       "status": {"method": "experimental/serverStatus", "text": "/message",
                                  "level": "/health", "warning": "warning", "error": ["error"],
                                  "idle": {"/quiescent": true}}},
                "plain": {"command": ["p"], "status": {"text": "/message"}}}}"#,
        )
        .unwrap()
        .unwrap();
        let ra = p.server("ra").unwrap();
        assert_eq!(
            ra.capabilities,
            serde_json::json!({"experimental": {"serverStatusNotification": true}})
        );
        let spec = ra.status.as_ref().unwrap();
        let read = |v: serde_json::Value| spec.read(&v);
        let s = read(serde_json::json!({"health": "warning", "quiescent": true,
                                       "message": "cargo check failed to start\n"}));
        assert_eq!(
            (s.health, s.text.as_str(), s.busy),
            (
                kalem_lsp::Health::Warning,
                "cargo check failed to start",
                false
            )
        );
        let s = read(serde_json::json!({"health": "ok", "quiescent": false}));
        assert_eq!(
            (s.health, s.text.as_str(), s.busy),
            (kalem_lsp::Health::Ok, "", true)
        );
        assert_eq!(
            read(serde_json::json!({"health": "error", "quiescent": true})).health,
            kalem_lsp::Health::Error
        );
        // No method, no notification.
        assert!(p.server("plain").unwrap().status.is_none());
    }

    /// A server's `version`: read from the manifest; its program's first
    /// line when it runs, its standard error's lines when it does not, its
    /// exit code when it says nothing; none without `version`.
    #[cfg(unix)]
    #[test]
    fn server_version_runs_or_says_why() {
        let p = parse_manifest(
            Path::new("/p"),
            r#"{"id": "x", "languages": [], "servers": {"s": {"command": ["sh"],
                "version": ["-c", "echo 'fake 1.2' && echo more"]}}}"#,
        )
        .unwrap()
        .unwrap();
        let mut s = p.server("s").unwrap().clone();
        assert_eq!(s.version, ["-c", "echo 'fake 1.2' && echo more"]);
        let (sh, root) = (Path::new("/bin/sh"), std::env::temp_dir());
        assert_eq!(server_version(&s, sh, &root), Some(Ok("fake 1.2".into())));
        s.version[1] = "echo 'error: not installed' >&2; echo 'help: get it' >&2; exit 1".into();
        assert_eq!(
            server_version(&s, sh, &root),
            Some(Err("error: not installed help: get it".into()))
        );
        s.version[1] = "exit 3".into();
        assert_eq!(server_version(&s, sh, &root), Some(Err(exit_text(Some(3)))));
        s.version.clear();
        assert_eq!(server_version(&s, sh, &root), None);
    }

    const MANIFEST: &str = r#"{
      "id": "org.example.lang", "name": "Lang", "version": "1",
      "languages": [
        {"id": "lang", "name": "Lang", "extensions": ["lg", "html.lg"], "filenames": ["Langfile"],
         "shebangs": ["lang"], "comment": {"line": "--", "block": ["{-", "-}"]}, "servers": ["a", "b"]},
        {"id": "tmpl", "syntax": "Rust", "extensions": ["lg.tmpl"]}
      ],
      "servers": {
        "a": {"name": "A", "command": ["kalem-no-such-program-a", "--stdio"], "install": "get A"},
        "b": {"name": "B", "command": ["sh"], "settings": {"b": {"x": 1, "y": 2}}}
      }
    }"#;

    #[test]
    fn manifest_read() {
        let p = parse_manifest(Path::new("/p"), MANIFEST).unwrap().unwrap();
        assert_eq!(p.languages[0].extensions, ["html.lg", "lg"]);
        assert_eq!(
            p.languages[0].block_comment,
            Some(("{-".into(), "-}".into()))
        );
        assert_eq!(p.server("a").unwrap().command[1], "--stdio");
        assert!(
            parse_manifest(Path::new("/p"), r#"{"id": "x"}"#)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn languages_and_servers() {
        let dir = std::env::temp_dir().join(format!("kalem-lang-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lang")).unwrap();
        // Server `b` is this test's own program: found on every system.
        let exe = std::env::current_exe().unwrap();
        let exe_json = serde_json::to_string(&exe).unwrap();
        let manifest = MANIFEST.replace(r#"["sh"]"#, &format!("[{exe_json}]"));
        std::fs::write(dir.join("lang/plugin.json"), manifest).unwrap();
        load_from(std::slice::from_ref(&dir));
        let (p, l) = for_path(Path::new("/x/a.html.lg"), None).unwrap();
        assert_eq!(l.id, "lang");
        assert_eq!(
            for_path(Path::new("/x/b.lg.tmpl"), None).unwrap().1.id,
            "tmpl"
        );
        assert_eq!(
            for_path(Path::new("/x/Langfile"), None).unwrap().1.id,
            "lang"
        );
        assert_eq!(
            for_path(Path::new("/x/run"), Some("#!/usr/bin/env lang -q"))
                .unwrap()
                .1
                .id,
            "lang"
        );
        assert!(for_path(Path::new("/x/a.txt"), None).is_none());
        // `a` is missing, `b` is found.
        match resolve_server(&p, &l, None) {
            Resolved::Found(s, path, _) => {
                assert_eq!(s.key, "b");
                assert_eq!(path, exe);
            }
            r => panic!("{r:?}"),
        }
        set_user_settings(Some(&serde_json::json!({
            "org.example.lang": {"server": "a", "settings": {"b": {"y": 3}}}
        })));
        match resolve_server(&p, &l, None) {
            Resolved::Missing(m) => assert!(m.contains("get A"), "{m}"),
            r => panic!("{r:?}"),
        }
        let s = server_settings(&p, p.server("b").unwrap());
        assert_eq!(s, serde_json::json!({"b": {"x": 1, "y": 3}}));
        // A language's syntax by its name, whatever the syntax's own
        // extensions are.
        assert_eq!(
            kalem_highlight::Language::find("tmpl").map(|l| l.name()),
            Some("Rust")
        );
        // Comment markers by language or extension; a block for a
        // language without a line marker.
        assert_eq!(
            comment_style("lg"),
            Some(crate::code::CommentStyle::Line("--"))
        );
        assert_eq!(
            crate::code::comment_style("Lang"),
            Some(crate::code::CommentStyle::Line("--"))
        );
        set_user_settings(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
