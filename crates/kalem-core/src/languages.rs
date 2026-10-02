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
    /// The servers in order of preference (keys of the plugin's servers).
    pub servers: Vec<String>,
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
    /// `initializationOptions`.
    pub initialization_options: Value,
    /// Its settings, before the user's.
    pub settings: Value,
    /// How to install it, said when it is not found.
    pub install: Option<String>,
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
}

impl Plugin {
    /// The server with key `key`.
    pub fn server(&self, key: &str) -> Option<&ServerSpec> {
        self.servers.iter().find(|s| s.key == key)
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
                    initialization_options: s["initializationOptions"].clone(),
                    settings: s["settings"].clone(),
                    install: s["install"].as_str().map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default();
    let commands = m["commands"]
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), strings(v))).collect())
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
    }))
}

/// The plugins loaded, and the problems met loading them.
#[derive(Debug, Default)]
struct Loaded {
    plugins: Vec<Arc<Plugin>>,
    problems: Vec<String>,
    dirs: Vec<PathBuf>,
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
/// the folders change. Cheap after the first call.
pub fn load() {
    load_from(&plugin_dirs());
}

/// Loads the language plugins in `dirs` (replacing those loaded before).
pub fn load_from(dirs: &[PathBuf]) {
    let _guard = LOAD.lock().expect("load");
    if LOADED
        .read()
        .expect("loaded")
        .as_ref()
        .is_some_and(|l| l.dirs == dirs)
    {
        return;
    }
    let mut loaded = Loaded {
        dirs: dirs.to_vec(),
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
    if !sources.is_empty() {
        let cache = crate::logging::state_dir().map(|d| d.join("cache"));
        let r = kalem_highlight::register_cached(&sources, cache.as_deref());
        for (f, e) in &r.errors {
            loaded.problems.push(format!("{f}: {e}"));
        }
        for (p, files) in &mut plugins {
            p.syntaxes = r
                .names
                .iter()
                .filter(|(f, _)| files.contains(f))
                .map(|(_, n)| n.clone())
                .collect();
        }
    }
    loaded.plugins = plugins.into_iter().map(|(p, _)| Arc::new(p)).collect();
    *LOADED.write().expect("loaded") = Some(loaded);
}

/// The language plugins loaded.
pub fn plugins() -> Vec<Arc<Plugin>> {
    LOADED
        .read()
        .expect("loaded")
        .as_ref()
        .map(|l| l.plugins.clone())
        .unwrap_or_default()
}

/// The problems met loading plugins.
pub fn problems() -> Vec<String> {
    LOADED
        .read()
        .expect("loaded")
        .as_ref()
        .map(|l| l.problems.clone())
        .unwrap_or_default()
}

/// Sets the user's `plugins` settings table (from
/// [`crate::settings::Config::apply_process_settings`]).
pub fn set_user_settings(plugins: Option<&Value>) {
    *USER.write().expect("user") = plugins.cloned().unwrap_or(Value::Null);
}

/// The user's settings of plugin `id`.
pub fn user_settings(id: &str) -> Value {
    USER.read()
        .expect("user")
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

/// The language of a file: by name, then the longest extension, then the
/// interpreter of its `#!` line (`first_line`).
pub fn for_path(path: &Path, first_line: Option<&str>) -> Option<(Arc<Plugin>, LanguageSpec)> {
    let name = path.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();
    let plugins = plugins();
    // Later plugins win, as their syntaxes do.
    for p in plugins.iter().rev() {
        if let Some(l) = p
            .languages
            .iter()
            .find(|l| l.filenames.iter().any(|f| f == name))
        {
            return Some((p.clone(), l.clone()));
        }
    }
    let mut best: Option<(usize, Arc<Plugin>, LanguageSpec)> = None;
    for p in plugins.iter().rev() {
        for l in &p.languages {
            for e in &l.extensions {
                let suffix = format!(".{}", e.to_ascii_lowercase());
                if lower.ends_with(&suffix) && best.as_ref().is_none_or(|b| e.len() > b.0) {
                    best = Some((e.len(), p.clone(), l.clone()));
                }
            }
        }
    }
    if let Some((_, p, l)) = best {
        return Some((p, l));
    }
    let interp = first_line?.strip_prefix("#!")?;
    let mut words = interp.split_whitespace();
    let mut prog = words.next()?.rsplit('/').next()?;
    if prog == "env" {
        prog = words.find(|w| !w.starts_with('-'))?;
    }
    plugins.iter().rev().find_map(|p| {
        p.languages
            .iter()
            .find(|l| l.shebangs.iter().any(|s| s == prog))
            .map(|l| (p.clone(), l.clone()))
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
    Found(ServerSpec, PathBuf, Vec<String>),
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
        let Some(spec) = plugin.server(key) else {
            continue;
        };
        let mut spec = spec.clone();
        let over = &user["servers"][key.as_str()];
        let command = strings(&over["command"]);
        if !command.is_empty() {
            spec.command = command;
            spec.candidates.clear();
        }
        spec.env.extend(env_pairs(&over["env"]));
        let Some((program, args)) = spec.command.split_first() else {
            continue;
        };
        let candidates = std::iter::once(program.clone()).chain(spec.candidates.iter().cloned());
        let found = candidates
            .into_iter()
            .find_map(|c| kalem_lsp::find_program(&c, root, &spec.local_dirs));
        match found {
            Some(path) => {
                let args = args.to_vec();
                return Resolved::Found(spec, path, args);
            }
            None => missing.push(match &spec.install {
                Some(how) => format!("{} is not installed ({how})", spec.name),
                None => format!("{} is not installed", spec.name),
            }),
        }
    }
    if missing.is_empty() {
        Resolved::Missing(format!(
            "{} has no language server for {}",
            plugin.name, lang.name
        ))
    } else {
        Resolved::Missing(missing.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
      "id": "org.example.lang", "name": "Lang", "version": "1",
      "languages": [
        {"id": "lang", "name": "Lang", "extensions": ["lg", "html.lg"], "filenames": ["Langfile"],
         "shebangs": ["lang"], "comment": {"line": "--", "block": ["{-", "-}"]}, "servers": ["a", "b"]},
        {"id": "tmpl", "extensions": ["lg.tmpl"]}
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
        std::fs::write(dir.join("lang/plugin.json"), MANIFEST).unwrap();
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
        // `a` is missing, `b` (sh) is found.
        match resolve_server(&p, &l, None) {
            Resolved::Found(s, path, _) => {
                assert_eq!(s.key, "b");
                assert!(path.ends_with("sh"));
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
        set_user_settings(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
