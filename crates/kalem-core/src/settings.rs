//! Settings (design §14, D9): TOML files in layers, later ones overriding
//! earlier ones.
//!
//! 1. Built-in defaults ([`SPECS`]).
//! 2. The user's `settings.toml` in [`config_dir`].
//! 3. The workspace's `.kalem/settings.toml` ([`find_workspace_settings`]).
//! 4. The document's `#+` keywords, for document behavior: the document's
//!    `#+TODO` replaces `org.todo_keywords` and `#+STARTUP` the logging
//!    settings, through [`Config::parse_base`] and
//!    [`Config::todo_settings`].
//!
//! Each file is checked against the known settings: a value of the wrong
//! type or out of range is reported and the layer below applies; unknown
//! keys are reported and kept, and tables under `plugins.<id>` belong to
//! plugins and are not checked. [`set_in_toml`] changes a value in a
//! file's text and keeps its comments and layout.

use std::path::{Path, PathBuf};

use org_edit::todo::{LogKind, TagTrigger, TodoSettings};
use org_syntax::{ParseContext, TodoSequence, TodoSequenceKind};
use serde_json::{Map, Value};

/// The type of a setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// `true` or `false`.
    Bool,
    /// An integer in a range.
    Int(i64, i64),
    /// Any string.
    Str,
    /// One of these strings.
    Enum(&'static [&'static str]),
    /// A list of strings, each one of these if given.
    List(Option<&'static [&'static str]>),
    /// A table from names to one of these strings.
    Map(&'static [&'static str]),
}

/// A known setting.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// The dotted key, such as `editor.font_size`.
    pub key: &'static str,
    /// Its type.
    pub kind: Kind,
    /// The default, as JSON.
    pub default: &'static str,
    /// What it does.
    pub description: &'static str,
}

const MODES: &[&str] = &["org", "markdown", "csv", "plain"];

/// The built-in settings.
pub const SPECS: &[Spec] = &[
    Spec {
        key: "editor.font_family",
        kind: Kind::Str,
        default: r#""""#,
        description: "Font of body text; empty for the system font",
    },
    Spec {
        key: "editor.code_font_family",
        kind: Kind::Str,
        default: r#""""#,
        description: "Font of code, tables and the source view; empty for the system's monospace font",
    },
    Spec {
        key: "editor.font_size",
        kind: Kind::Int(6, 72),
        default: "16",
        description: "Font size of body text in points",
    },
    Spec {
        key: "editor.theme",
        kind: Kind::Enum(&["system", "light", "dark"]),
        default: r#""system""#,
        description: "Light or dark colors, or those of the system",
    },
    Spec {
        key: "ui.language",
        kind: Kind::Enum(&["auto", "en", "tr"]),
        default: r#""auto""#,
        description: "Language of the interface: that of the system, English or Turkish",
    },
    Spec {
        key: "editor.line_width",
        kind: Kind::Int(0, 400),
        default: "80",
        description: "Width of the text column in characters; 0 for the whole window",
    },
    Spec {
        key: "editor.center_text",
        kind: Kind::Bool,
        default: "false",
        description: "Center the text column in the window, like a page; off, the text starts at the left edge",
    },
    Spec {
        key: "editor.outline_indent",
        kind: Kind::Bool,
        default: "true",
        description: "Text under a heading starts where the heading's title does, as Org's org-indent-mode (#+STARTUP: indent or noindent decides for one file)",
    },
    Spec {
        key: "editor.soft_wrap",
        kind: Kind::Bool,
        default: "true",
        description: "Wrap long lines at the window's edge (Alt+Z toggles it for a window)",
    },
    Spec {
        key: "editor.trim_trailing_whitespace",
        kind: Kind::Bool,
        default: "false",
        description: "Remove the blanks at the ends of lines when saving",
    },
    Spec {
        key: "editor.line_numbers",
        kind: Kind::Bool,
        default: "true",
        description: "Line numbers in plain text files and in the source view",
    },
    Spec {
        key: "editor.keymap_profile",
        kind: Kind::Enum(&["word", "vim"]),
        default: r#""word""#,
        description: "Word-like keys, or Vim's modal keys (with the Word-like keys in insert mode)",
    },
    Spec {
        key: "editor.show_source_markers",
        kind: Kind::Enum(&["cursor", "always", "never"]),
        default: r#""cursor""#,
        description: "When Org markup such as `*` around bold text is shown",
    },
    Spec {
        key: "editor.vim.modes",
        kind: Kind::List(Some(MODES)),
        default: "[]",
        description: "Document modes where the Vim profile applies (org, markdown, csv, plain); empty for all",
    },
    Spec {
        key: "editor.vim.leader",
        kind: Kind::Str,
        default: r#""space""#,
        description: "The leader key of the Vim profile's Doom Emacs style bindings (`leader p p` switches project)",
    },
    Spec {
        key: "ui.open_files",
        kind: Kind::Enum(&["left", "top", "hidden"]),
        default: r#""left""#,
        description: "Where the list of open files shows: a sidebar on the left, tabs at the top, or not at all",
    },
    Spec {
        key: "ui.folder_tree",
        kind: Kind::Bool,
        default: "true",
        description: "The sidebar on the left shows the current project's folders and files below the open files",
    },
    Spec {
        key: "format.recent_colors",
        kind: Kind::List(None),
        default: "[]",
        description: "Text colors used lately, newest first (the color menus offer them)",
    },
    Spec {
        key: "format.recent_highlights",
        kind: Kind::List(None),
        default: "[]",
        description: "Highlight colors used lately, newest first",
    },
    Spec {
        key: "org.allow_kalem_markup",
        kind: Kind::Bool,
        default: "false",
        description: "Kalem's formatting may be written into .org files too (a workspace's own setting, for a folder shared with no Emacs user); without it .org stays strict Org and .klm files hold Kalem documents",
    },
    Spec {
        key: "org.table_auto_recalc",
        kind: Kind::Bool,
        default: "false",
        description: "Recalculate a table's formulas when Tab, Shift+Tab or Enter leaves a field, as F9 does; a document's `#+KALEM: recalc=auto` or `recalc=manual` wins",
    },
    Spec {
        key: "export.body_only",
        kind: Kind::Bool,
        default: "false",
        description: "Exports write the document's body only, without the page around it",
    },
    Spec {
        key: "export.open_after",
        kind: Kind::Bool,
        default: "false",
        description: "Open an exported file with the system's application",
    },
    Spec {
        key: "org.footnote_section",
        kind: Kind::Str,
        default: r#""Footnotes""#,
        description: "The heading footnote definitions go under (org-footnote-section); empty: at the end of each section",
    },
    Spec {
        key: "export.text_charset",
        kind: Kind::Enum(&["ascii", "utf-8"]),
        default: r#""ascii""#,
        description: "Characters of plain text exports: ASCII, or UTF-8 lines, bullets and quotes",
    },
    Spec {
        key: "export.math",
        kind: Kind::Enum(&["mathjax", "svg"]),
        default: r#""mathjax""#,
        description: "Formulas in HTML exports: MathJax in the browser, or SVG images drawn by Kalem (a document's own `#+OPTIONS: tex:` wins)",
    },
    Spec {
        key: "projects.auto_add",
        kind: Kind::Bool,
        default: "true",
        description: "A folder under version control (Git, Mercurial, Subversion) or with a .projectile file becomes a project when one of its files is opened, as in Projectile",
    },
    Spec {
        key: "files.details",
        kind: Kind::Bool,
        default: "true",
        description: "Permissions, sizes and times in the file manager (the ( key toggles them)",
    },
    Spec {
        key: "files.show_hidden",
        kind: Kind::Bool,
        default: "false",
        description: "Dot files in the file manager (the . key toggles them)",
    },
    Spec {
        key: "files.sort",
        kind: Kind::Enum(&["name", "time", "size", "extension"]),
        default: r#""name""#,
        description: "How the file manager sorts a folder (the s key cycles through the orders)",
    },
    Spec {
        key: "files.directories_first",
        kind: Kind::Bool,
        default: "true",
        description: "Folders before files in the file manager",
    },
    Spec {
        key: "org.todo_keywords",
        kind: Kind::List(None),
        default: r#"["TODO", "|", "DONE"]"#,
        description: "TODO keywords of documents without #+TODO; `|` separates the done states",
    },
    Spec {
        key: "org.log_done",
        kind: Kind::Enum(&["none", "time", "note"]),
        default: r#""none""#,
        description: "What marking a task done records (org-log-done)",
    },
    Spec {
        key: "org.log_into_drawer",
        kind: Kind::Str,
        default: r#""""#,
        description: "Drawer for state change notes, such as LOGBOOK; empty for none",
    },
    Spec {
        key: "org.adapt_indentation",
        kind: Kind::Bool,
        default: "false",
        description: "Indent planning lines and notes to the headline text",
    },
    Spec {
        key: "org.enforce_todo_dependencies",
        kind: Kind::Bool,
        default: "false",
        description: "A task cannot be marked done while a task below it is open, or an earlier sibling under a parent with ORDERED (org-enforce-todo-dependencies); a NOBLOCKING property turns it off for an entry",
    },
    Spec {
        key: "org.enforce_todo_checkbox_dependencies",
        kind: Kind::Bool,
        default: "false",
        description: "A task cannot be marked done while a checkbox in it is unchecked (org-enforce-todo-checkbox-dependencies)",
    },
    Spec {
        key: "org.todo_state_tags_triggers",
        kind: Kind::List(None),
        default: "[]",
        description: "Tags changed on entering a state (org-todo-state-tags-triggers): \"STATE: +tag -tag\", where STATE is a keyword, todo or done for any open or done state, or empty for no keyword",
    },
    Spec {
        key: "org.assets_dir",
        kind: Kind::Str,
        default: r#""{name}_assets""#,
        description: "Where pasted images go; {name} is the document's name",
    },
    Spec {
        key: "files.backup",
        kind: Kind::Bool,
        default: "false",
        description: "Keep the previous version as NAME.bak when saving",
    },
    Spec {
        key: "files.modes",
        kind: Kind::Map(&["org", "markdown", "csv", "text"]),
        default: "{}",
        description: "Document modes chosen with Set Document Mode, by path relative to the workspace",
    },
    Spec {
        key: "log.level",
        kind: Kind::Enum(&["error", "warn", "info", "debug", "trace"]),
        default: r#""info""#,
        description: "How much goes to the log file (KALEM_LOG overrides it)",
    },
    Spec {
        key: "export.pdf_engine",
        kind: Kind::Enum(&["auto", "latexmk", "tectonic"]),
        default: r#""auto""#,
        description: "LaTeX engine for PDF export",
    },
    Spec {
        key: "export.pandoc_path",
        kind: Kind::Str,
        default: r#""""#,
        description: "The pandoc program; empty to search PATH",
    },
    Spec {
        key: "plugins.enabled",
        kind: Kind::List(None),
        default: "[]",
        description: "Enabled plugins, by ID",
    },
];

/// A settings layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// Built-in defaults.
    Default,
    /// The user's `settings.toml`.
    User,
    /// The workspace's `.kalem/settings.toml`.
    Workspace,
}

/// A problem in a settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsIssue {
    /// The file.
    pub path: Option<PathBuf>,
    /// The key, if the problem is one value.
    pub key: Option<String>,
    /// What is wrong.
    pub message: String,
    /// `false` for problems that change nothing (unknown keys).
    pub error: bool,
}

/// The directory of the user's configuration: `$KALEM_CONFIG_DIR`, or
/// `kalem` in `$XDG_CONFIG_HOME`, `%APPDATA%` on Windows, or `~/.config`.
pub fn config_dir() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(d) = var("KALEM_CONFIG_DIR") {
        return Some(d);
    }
    let base = var("XDG_CONFIG_HOME")
        .or_else(|| cfg!(windows).then(|| var("APPDATA")).flatten())
        .or_else(|| var("HOME").map(|h| h.join(".config")))?;
    Some(base.join("kalem"))
}

/// `path` with a leading `~` for the home folder.
pub fn expand_home(path: &str) -> String {
    let path = path.trim();
    match (path.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            format!("{}{rest}", home.to_string_lossy())
        }
        _ => path.to_string(),
    }
}

/// The workspace settings for a document in `dir`: `.kalem/settings.toml`
/// in `dir` or the nearest ancestor that has one.
pub fn find_workspace_settings(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .map(|d| d.join(".kalem").join("settings.toml"))
        .find(|p| p.is_file())
}

fn spec(key: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.key == key)
}

fn check(kind: Kind, v: &Value) -> Result<(), String> {
    let list = |allowed: Option<&[&str]>| -> Result<(), String> {
        let Some(items) = v.as_array() else {
            return Err("must be a list of strings".into());
        };
        for i in items {
            match (i.as_str(), allowed) {
                (None, _) => return Err("must be a list of strings".into()),
                (Some(s), Some(a)) if !a.contains(&s) => {
                    return Err(format!("`{s}` is not one of {}", a.join(", ")));
                }
                _ => {}
            }
        }
        Ok(())
    };
    match kind {
        Kind::Bool if v.is_boolean() => Ok(()),
        Kind::Bool => Err("must be true or false".into()),
        Kind::Int(lo, hi) => match v.as_i64() {
            Some(n) if (lo..=hi).contains(&n) => Ok(()),
            _ => Err(format!("must be an integer from {lo} to {hi}")),
        },
        Kind::Str if v.is_string() => Ok(()),
        Kind::Str => Err("must be a string".into()),
        Kind::Enum(options) => match v.as_str() {
            Some(s) if options.contains(&s) => Ok(()),
            _ => Err(format!("must be one of {}", options.join(", "))),
        },
        Kind::List(allowed) => list(allowed),
        Kind::Map(options) => {
            let Some(t) = v.as_object() else {
                return Err("must be a table".into());
            };
            match t
                .values()
                .find(|x| x.as_str().is_none_or(|s| !options.contains(&s)))
            {
                Some(bad) => Err(format!("has {bad}, not one of {}", options.join(", "))),
                None => Ok(()),
            }
        }
    }
}

fn to_json(v: &toml_edit::Value) -> Value {
    use toml_edit::Value as T;
    match v {
        T::String(s) => Value::String(s.value().clone()),
        T::Integer(n) => Value::from(*n.value()),
        T::Float(f) => serde_json::Number::from_f64(*f.value()).map_or(Value::Null, Value::Number),
        T::Boolean(b) => Value::Bool(*b.value()),
        T::Datetime(d) => Value::String(d.value().to_string()),
        T::Array(a) => Value::Array(a.iter().map(to_json).collect()),
        T::InlineTable(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.to_string(), to_json(v))).collect())
        }
    }
}

fn item_to_json(item: &toml_edit::Item) -> Value {
    match item {
        toml_edit::Item::Value(v) => to_json(v),
        toml_edit::Item::Table(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.to_string(), item_to_json(v)))
                .collect(),
        ),
        toml_edit::Item::ArrayOfTables(a) => Value::Array(
            a.iter()
                .map(|t| {
                    Value::Object(
                        t.iter()
                            .map(|(k, v)| (k.to_string(), item_to_json(v)))
                            .collect(),
                    )
                })
                .collect(),
        ),
        toml_edit::Item::None => Value::Null,
    }
}

/// Checks a layer's values: invalid ones are removed and reported, unknown
/// keys reported.
fn validate(
    tree: &mut Map<String, Value>,
    prefix: &str,
    path: Option<&Path>,
    issues: &mut Vec<SettingsIssue>,
) {
    let keys: Vec<String> = tree.keys().cloned().collect();
    for k in keys {
        let full = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        let issue = |message: String, error: bool| SettingsIssue {
            path: path.map(Path::to_path_buf),
            key: Some(full.clone()),
            message,
            error,
        };
        if let Some(s) = spec(&full) {
            if let Err(e) = check(s.kind, &tree[&k]) {
                issues.push(issue(format!("`{full}` {e}"), true));
                tree.remove(&k);
            }
            continue;
        }
        if prefix == "plugins" && tree[&k].is_object() {
            continue;
        }
        let is_section = SPECS.iter().any(|s| s.key.starts_with(&format!("{full}.")));
        match tree.get_mut(&k) {
            Some(Value::Object(sub)) if is_section => validate(sub, &full, path, issues),
            _ if is_section => {
                issues.push(issue(format!("`{full}` must be a table"), true));
                tree.remove(&k);
            }
            _ => issues.push(issue(format!("Unknown setting `{full}`"), false)),
        }
    }
}

fn merge(into: &mut Map<String, Value>, from: &Map<String, Value>) {
    for (k, v) in from {
        match (into.get_mut(k), v) {
            (Some(Value::Object(a)), Value::Object(b)) => merge(a, b),
            _ => {
                into.insert(k.clone(), v.clone());
            }
        }
    }
}

fn insert_dotted(tree: &mut Map<String, Value>, key: &str, value: Value) {
    let mut t = tree;
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop().expect("a key");
    for p in parts {
        t = t
            .entry(p.to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("defaults are tables");
    }
    t.insert(last.to_string(), value);
}

/// The merged settings.
#[derive(Debug, Clone)]
pub struct Config {
    merged: Map<String, Value>,
    sources: Vec<(Layer, Option<PathBuf>)>,
    issues: Vec<SettingsIssue>,
}

impl Default for Config {
    fn default() -> Self {
        Config::from_layers(&[])
    }
}

impl Config {
    /// Settings from the texts of settings files, in layer order.
    pub fn from_layers(layers: &[(Layer, Option<&Path>, &str)]) -> Config {
        let mut merged = Map::new();
        for s in SPECS {
            let v = serde_json::from_str(s.default).expect("valid default");
            insert_dotted(&mut merged, s.key, v);
        }
        let mut issues = Vec::new();
        let mut sources = vec![(Layer::Default, None)];
        for (layer, path, text) in layers {
            let doc = match text.parse::<toml_edit::DocumentMut>() {
                Ok(d) => d,
                Err(e) => {
                    issues.push(SettingsIssue {
                        path: path.map(Path::to_path_buf),
                        key: None,
                        message: format!("Not valid TOML: {}", e.to_string().trim_end()),
                        error: true,
                    });
                    continue;
                }
            };
            let Value::Object(mut tree) = item_to_json(doc.as_item()) else {
                continue;
            };
            validate(&mut tree, "", *path, &mut issues);
            merge(&mut merged, &tree);
            sources.push((*layer, path.map(Path::to_path_buf)));
        }
        Config {
            merged,
            sources,
            issues,
        }
    }

    /// Reads the user's and the workspace's files; missing files are
    /// skipped, unreadable ones reported.
    pub fn load(user: Option<&Path>, workspace: Option<&Path>) -> Config {
        let mut texts = Vec::new();
        let mut issues = Vec::new();
        for (layer, path) in [(Layer::User, user), (Layer::Workspace, workspace)] {
            let Some(p) = path else { continue };
            match std::fs::read_to_string(p) {
                Ok(t) => texts.push((layer, p, t)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => issues.push(SettingsIssue {
                    path: Some(p.to_path_buf()),
                    key: None,
                    message: format!("Cannot read the settings: {e}"),
                    error: true,
                }),
            }
        }
        let layers: Vec<(Layer, Option<&Path>, &str)> = texts
            .iter()
            .map(|(l, p, t)| (*l, Some(*p), t.as_str()))
            .collect();
        let mut c = Config::from_layers(&layers);
        issues.append(&mut c.issues);
        for i in &issues {
            let path = i
                .path
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            if i.error {
                tracing::warn!(path, "{}", i.message);
            } else {
                tracing::info!(path, "{}", i.message);
            }
        }
        c.issues = issues;
        c
    }

    /// Problems found while loading.
    pub fn issues(&self) -> &[SettingsIssue] {
        &self.issues
    }

    /// The layers that were read.
    pub fn sources(&self) -> &[(Layer, Option<PathBuf>)] {
        &self.sources
    }

    /// A value by dotted key (`editor.font_size`); keys of plugins with
    /// dots in their IDs go through [`Config::get_path`].
    pub fn get(&self, key: &str) -> Option<&Value> {
        let parts: Vec<&str> = key.split('.').collect();
        self.get_path(&parts)
    }

    /// A value by the parts of its key.
    pub fn get_path(&self, parts: &[&str]) -> Option<&Value> {
        let (first, rest) = parts.split_first()?;
        let mut v = self.merged.get(*first)?;
        for p in rest {
            v = v.get(*p)?;
        }
        Some(v)
    }

    /// A string setting (empty if missing or not a string).
    pub fn str(&self, key: &str) -> &str {
        self.get(key).and_then(Value::as_str).unwrap_or("")
    }

    /// An integer setting (0 if missing or not an integer).
    pub fn int(&self, key: &str) -> i64 {
        self.get(key).and_then(Value::as_i64).unwrap_or(0)
    }

    /// A boolean setting (`false` if missing or not a boolean).
    pub fn bool(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    /// A list of strings (empty if missing).
    pub fn strings(&self, key: &str) -> Vec<&str> {
        self.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// The keys whose values differ from `other`, for change notifications
    /// after a reload.
    pub fn changed_keys(&self, other: &Config) -> Vec<String> {
        fn walk(
            a: &Map<String, Value>,
            b: &Map<String, Value>,
            prefix: &str,
            out: &mut Vec<String>,
        ) {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let full = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                match (a.get(k), b.get(k)) {
                    (Some(Value::Object(x)), Some(Value::Object(y))) => walk(x, y, &full, out),
                    (x, y) if x != y => out.push(full),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.merged, &other.merged, "", &mut out);
        out
    }

    /// How documents are saved.
    pub fn save_options(&self) -> crate::files::SaveOptions {
        crate::files::SaveOptions {
            backup: self.bool("files.backup"),
        }
    }

    /// The keymap profile.
    pub fn keymap_profile(&self) -> crate::keymap::Profile {
        crate::keymap::Profile::from_name(self.str("editor.keymap_profile"))
            .unwrap_or(crate::keymap::Profile::Word)
    }

    /// The Vim leader key (`editor.vim.leader`), Space if it is not a key.
    pub fn vim_leader(&self) -> String {
        let l = self.str("editor.vim.leader").trim();
        if crate::keys::KeySequence::parse(l).is_some() {
            l.to_string()
        } else {
            crate::keymap::DEFAULT_LEADER.to_string()
        }
    }

    /// The parse configuration documents start from: `org.todo_keywords`,
    /// which a document's `#+TODO` replaces.
    pub fn parse_base(&self) -> ParseContext {
        let mut ctx = ParseContext::default();
        let words = self.strings("org.todo_keywords");
        if !words.is_empty() {
            let seq = TodoSequence::parse(TodoSequenceKind::Sequence, &words.join(" "));
            ctx.set_todo_sequences(vec![seq]);
        }
        ctx
    }

    /// The TODO settings documents start from; a document's `#+STARTUP`
    /// applies on top ([`TodoSettings::for_document`]).
    pub fn todo_settings(&self) -> TodoSettings {
        let drawer = self.str("org.log_into_drawer");
        TodoSettings {
            log_done: match self.str("org.log_done") {
                "time" => Some(LogKind::Time),
                "note" => Some(LogKind::Note),
                _ => None,
            },
            log_into_drawer: (!drawer.is_empty()).then(|| drawer.to_string()),
            adapt_indentation: self.bool("org.adapt_indentation"),
            enforce_todo_dependencies: self.bool("org.enforce_todo_dependencies"),
            enforce_todo_checkbox_dependencies: self.bool("org.enforce_todo_checkbox_dependencies"),
            todo_state_tags_triggers: self
                .strings("org.todo_state_tags_triggers")
                .into_iter()
                .filter_map(tag_trigger)
                .collect(),
            ..TodoSettings::default()
        }
    }
}

/// An entry of `org.todo_state_tags_triggers`: `STATE: +tag -tag`.
fn tag_trigger(s: &str) -> Option<(TagTrigger, Vec<(String, bool)>)> {
    let (state, tags) = s.split_once(':')?;
    let trigger = match state.trim() {
        "" => TagTrigger::NoKeyword,
        "todo" => TagTrigger::Todo,
        "done" => TagTrigger::Done,
        k => TagTrigger::Keyword(k.to_string()),
    };
    let changes = tags
        .split_whitespace()
        .filter_map(|t| match t.as_bytes().first() {
            Some(b'+') => Some((t[1..].to_string(), true)),
            Some(b'-') => Some((t[1..].to_string(), false)),
            _ => None,
        })
        .filter(|(t, _)| !t.is_empty())
        .collect();
    Some((trigger, changes))
}

fn to_toml(v: &Value) -> Option<toml_edit::Value> {
    Some(match v {
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64()?.into(),
        },
        Value::String(s) => s.as_str().into(),
        Value::Array(a) => {
            let items: Option<Vec<toml_edit::Value>> = a.iter().map(to_toml).collect();
            toml_edit::Value::Array(items?.into_iter().collect())
        }
        Value::Object(o) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, v) in o {
                t.insert(k, to_toml(v)?);
            }
            toml_edit::Value::InlineTable(t)
        }
        Value::Null => return None,
    })
}

/// The workspace root of `config`: the directory holding the `.kalem`
/// directory of its workspace settings.
fn workspace_root(config: &Config) -> Option<PathBuf> {
    let (_, p) = config
        .sources()
        .iter()
        .find(|(l, p)| *l == Layer::Workspace && p.is_some())?;
    Some(p.as_ref()?.parent()?.parent()?.to_path_buf())
}

/// `path` relative to `root`, with `/` between names (the key of
/// `files.modes`).
fn relative_key(root: &Path, path: &Path) -> Option<String> {
    let abs = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| std::path::absolute(p).unwrap_or(p.to_path_buf()))
    };
    let rel = abs(path).strip_prefix(abs(root)).ok()?.to_path_buf();
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The document mode chosen for the file at `path` in the workspace
/// settings, if any (§2.6: an explicit choice comes first).
pub fn remembered_mode(config: &Config, path: &Path) -> Option<crate::DocumentMode> {
    let root = workspace_root(config)?;
    let key = relative_key(&root, path)?;
    let name = config.get_path(&["files", "modes", &key])?.as_str()?;
    crate::DocumentMode::from_name(name)
}

/// Remembers `mode` for the file at `path` in the workspace settings: the
/// nearest `.kalem/settings.toml` above it, or a new one beside it.
/// Returns the settings file.
pub fn remember_mode(path: &Path, mode: &str) -> Result<PathBuf, String> {
    let dir = std::path::absolute(path)
        .map_err(|e| e.to_string())?
        .parent()
        .map(Path::to_path_buf)
        .ok_or("No directory")?;
    let file =
        find_workspace_settings(&dir).unwrap_or_else(|| dir.join(".kalem").join("settings.toml"));
    let root = file
        .parent()
        .and_then(Path::parent)
        .ok_or("No workspace")?
        .to_path_buf();
    let key = relative_key(&root, path).ok_or("The file is not in the workspace")?;
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Cannot read {}: {e}", file.display())),
    };
    let new = set_in_toml(&text, &["files", "modes", &key], &Value::from(mode))?;
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("Cannot create {}: {e}", d.display()))?;
    }
    crate::files::write(&file, new.as_bytes(), crate::files::SaveOptions::default())
        .map_err(|e| format!("Cannot save {}: {e}", file.display()))?;
    Ok(file)
}

/// Sets `key` (dotted, a known setting) to `value` in the settings file
/// at `path`, keeping its comments; the file and its directory are made
/// when missing.
pub fn save_setting(path: &Path, key: &str, value: &Value) -> Result<(), String> {
    let s = spec(key).ok_or_else(|| format!("Unknown setting `{key}`"))?;
    check(s.kind, value).map_err(|e| format!("`{key}` {e}"))?;
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };
    let parts: Vec<&str> = key.split('.').collect();
    let new = set_in_toml(&text, &parts, value)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    }
    crate::files::write(path, new.as_bytes(), crate::files::SaveOptions::default())
        .map(|_| ())
        .map_err(|e| format!("Cannot save {}: {e}", path.display()))
}

/// `text` (a settings file) with `key` set to `value`, or removed for
/// `null`, keeping comments and layout. Missing tables are added.
pub fn set_in_toml(text: &str, key: &[&str], value: &Value) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e| format!("Not valid TOML: {e}"))?;
    let (last, tables) = key.split_last().ok_or("Empty key")?;
    let mut t: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for (i, p) in tables.iter().enumerate() {
        if t.get(p).is_none() {
            let mut new = toml_edit::Table::new();
            // Only the innermost table gets a header.
            new.set_implicit(i + 1 < tables.len());
            t.insert(p, toml_edit::Item::Table(new));
        }
        t = t
            .get_mut(p)
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| format!("`{}` is not a table", tables[..=i].join(".")))?;
    }
    match to_toml(value) {
        Some(v) => {
            match t.get_mut(last) {
                // Keep the comment after the old value.
                Some(toml_edit::Item::Value(old)) => {
                    let decor = old.decor().clone();
                    *old = v;
                    *old.decor_mut() = decor;
                }
                _ => {
                    t.insert(last, toml_edit::Item::Value(v));
                }
            }
        }
        None => {
            t.remove(last);
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_modes() {
        let dir = std::env::temp_dir().join(format!("kalem-modes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let file = dir.join("sub").join("notes.txt");
        std::fs::write(&file, "* x\n").unwrap();
        let settings = remember_mode(&file, "org").unwrap();
        assert_eq!(
            settings,
            dir.join("sub").join(".kalem").join("settings.toml")
        );
        let ws = find_workspace_settings(&dir.join("sub")).unwrap();
        let c = Config::load(None, Some(&ws));
        assert!(c.issues().is_empty(), "{:?}", c.issues());
        assert_eq!(remembered_mode(&c, &file), Some(crate::DocumentMode::Org));
        assert_eq!(
            remembered_mode(&c, &dir.join("sub").join("other.txt")),
            None
        );
        let bad = Config::from_layers(&[(Layer::User, None, "[files.modes]\n\"a\" = \"word\"\n")]);
        assert_eq!(bad.issues().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saving_settings() {
        let dir = std::env::temp_dir().join(format!("kalem-save-setting-{}", std::process::id()));
        let path = dir.join("sub").join("settings.toml");
        save_setting(&path, "editor.theme", &Value::from("dark")).unwrap();
        save_setting(&path, "editor.font_size", &Value::from(20)).unwrap();
        let c = Config::load(Some(&path), None);
        assert_eq!(
            (c.str("editor.theme"), c.int("editor.font_size")),
            ("dark", 20)
        );
        assert!(save_setting(&path, "editor.theme", &Value::from("blue")).is_err());
        assert!(save_setting(&path, "editor.nothing", &Value::from(1)).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn layers() {
        let user = r#"
[editor]
font_size = 18
keymap_profile = "vim"
line_width = "wide"   # wrong type

[org]
todo_keywords = ["TODO", "NEXT", "|", "DONE", "CANCELLED"]
log_done = "time"
colour = "red"

[plugins]
enabled = ["com.example.wordcount"]
[plugins."com.example.wordcount"]
per_page = 250
"#;
        let workspace = "editor.font_size = 12\neditor.vim.modes = [\"plain\", \"emacs\"]\n";
        let c = Config::from_layers(&[
            (Layer::User, Some(Path::new("user.toml")), user),
            (Layer::Workspace, Some(Path::new("ws.toml")), workspace),
        ]);
        assert_eq!(c.int("editor.font_size"), 12);
        assert_eq!(c.int("editor.line_width"), 80);
        assert_eq!(c.keymap_profile(), crate::keymap::Profile::Vim);
        assert_eq!(c.str("editor.show_source_markers"), "cursor");
        assert!(c.strings("editor.vim.modes").is_empty());
        assert_eq!(
            c.get_path(&["plugins", "com.example.wordcount", "per_page"]),
            Some(&Value::from(250))
        );
        let msgs: Vec<(&str, bool)> = c
            .issues()
            .iter()
            .map(|i| (i.key.as_deref().unwrap(), i.error))
            .collect();
        assert_eq!(
            msgs,
            [
                ("editor.line_width", true),
                ("org.colour", false),
                ("editor.vim.modes", true)
            ]
        );
        // The document layer.
        let base = c.parse_base();
        assert_eq!(base.todo_keywords, ["TODO", "NEXT"]);
        let parse = org_syntax::parse_with_base("* NEXT a\n", &base, None);
        assert!(parse.syntax().to_string().contains("NEXT"));
        let own = org_syntax::parse_with_base("#+TODO: A | B\n* NEXT a\n", &base, None);
        assert_eq!(own.context().todo_keywords, ["A"]);
        assert_eq!(c.todo_settings().log_done, Some(LogKind::Time));
    }

    #[test]
    fn bad_files() {
        let c = Config::from_layers(&[(Layer::User, None, "[editor\nx=1")]);
        assert!(c.issues()[0].message.starts_with("Not valid TOML"));
        assert_eq!(c.int("editor.font_size"), 16);
        let c = Config::from_layers(&[(Layer::User, None, "editor = 3\n")]);
        assert_eq!(c.issues()[0].message, "`editor` must be a table");
        assert_eq!(c.int("editor.font_size"), 16);
    }

    #[test]
    fn changes() {
        let a = Config::default();
        let b = Config::from_layers(&[(Layer::User, None, "editor.font_size = 20\n[x]\ny = 1\n")]);
        assert_eq!(a.changed_keys(&b), ["editor.font_size", "x"]);
        assert!(a.changed_keys(&a).is_empty());
    }

    #[test]
    fn writing() {
        let text = "# My settings\n[editor]\nfont_size = 16 # big\n";
        let t = set_in_toml(text, &["editor", "font_size"], &Value::from(18)).unwrap();
        assert_eq!(t, "# My settings\n[editor]\nfont_size = 18 # big\n");
        let t = set_in_toml(&t, &["org", "log_done"], &Value::from("time")).unwrap();
        assert_eq!(
            t,
            "# My settings\n[editor]\nfont_size = 18 # big\n\n[org]\nlog_done = \"time\"\n"
        );
        let t = set_in_toml(
            "",
            &["editor", "vim", "modes"],
            &serde_json::json!(["plain"]),
        )
        .unwrap();
        assert_eq!(t, "[editor.vim]\nmodes = [\"plain\"]\n");
        let t = set_in_toml(text, &["editor", "font_size"], &Value::Null).unwrap();
        assert_eq!(t, "# My settings\n[editor]\n");
        assert!(set_in_toml("a = 1\n", &["a", "b"], &Value::from(1)).is_err());
    }

    #[test]
    fn directories() {
        let dir = std::env::temp_dir().join(format!("kalem-settings-{}", std::process::id()));
        let deep = dir.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(dir.join(".kalem")).unwrap();
        std::fs::write(dir.join(".kalem/settings.toml"), "editor.font_size = 30\n").unwrap();
        let ws = find_workspace_settings(&deep).unwrap();
        assert_eq!(ws, dir.join(".kalem/settings.toml"));
        let c = Config::load(Some(&dir.join("missing.toml")), Some(&ws));
        assert!(c.issues().is_empty());
        assert_eq!(c.int("editor.font_size"), 30);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
