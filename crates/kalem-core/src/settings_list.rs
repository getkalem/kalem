//! Every setting as pages to browse and change, as both editors' settings
//! panel shows them (lazygit's way: a framed list and a key for each
//! change). Kalem's settings are grouped by their table (`editor`, `ui`,
//! `projects`…) and filtered by typed text; the installed plugins are a
//! page of their own and each plugin's settings another
//! ([`crate::plugin_settings`]). A value changes in place: a switch
//! flipped, the next choice, a number up or down, the next of a text's
//! usual values (the fonts installed, the TeX engines…), so that little
//! is typed; a value of one's own is typed, and a list's or a table's
//! items are shown and changed one by one ([`items`]). No setting needs
//! `settings.toml` opened.

use std::cell::OnceCell;

use serde_json::Value;

use crate::l10n::tr;
use crate::plugin_settings::PluginInfo;
use crate::settings::{Config, Kind, SPECS, Spec};

/// The kind of a setting, as the panel changes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// On or off.
    Bool,
    /// An integer in a range.
    Int(i64, i64),
    /// One of these texts.
    Enum(Vec<String>),
    /// Any text; usual values beside those of [`candidates`].
    Text(Vec<String>),
    /// A list of some of these choices (`editor.vim.modes`): each choice
    /// in or out.
    Choices(Vec<String>),
    /// A list of texts (`org.todo_keywords`): added, edited, removed and
    /// moved.
    Texts,
    /// A table from paths to modes (`files.modes`): entries typed as
    /// `path = mode`, their modes stepped among these.
    Table(Vec<String>),
    /// Any value, typed as JSON: a plugin's setting its manifest does not
    /// describe.
    Json,
}

/// A setting as the panel shows and changes it: one of Kalem's, or a
/// plugin's.
#[derive(Debug, Clone)]
pub struct Field {
    /// Where its value is: `["editor", "theme"]`, `["plugins", ID, "goal"]`.
    pub path: Vec<String>,
    /// How messages name it: `editor.theme`, `plugins."ID".goal`.
    pub key: String,
    /// The name the list shows: `theme`, `goal`.
    pub name: String,
    /// Its kind.
    pub kind: FieldKind,
    /// Its default (null for none).
    pub default: Value,
    /// What it does.
    pub description: String,
    /// One of Kalem's settings: checked and saved by its key.
    pub spec: Option<&'static Spec>,
}

impl PartialEq for Field {
    fn eq(&self, other: &Field) -> bool {
        // The spec is the key's.
        self.path == other.path
            && self.key == other.key
            && self.name == other.name
            && self.kind == other.kind
            && self.default == other.default
            && self.description == other.description
    }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

impl Field {
    /// Kalem's setting `spec`.
    pub fn of(spec: &'static Spec) -> Field {
        let kind = match spec.kind {
            Kind::Bool => FieldKind::Bool,
            Kind::Int(lo, hi) => FieldKind::Int(lo, hi),
            Kind::Str => FieldKind::Text(Vec::new()),
            Kind::Enum(options) => FieldKind::Enum(strings(options)),
            Kind::List(Some(choices)) => FieldKind::Choices(strings(choices)),
            Kind::List(None) => FieldKind::Texts,
            Kind::Modes(modes) => FieldKind::Table(strings(modes)),
        };
        Field {
            path: spec.key.split('.').map(str::to_string).collect(),
            key: spec.key.to_string(),
            name: name(spec.key).to_string(),
            kind,
            default: serde_json::from_str(spec.default).unwrap_or(Value::Null),
            description: spec.description.to_string(),
            spec: Some(spec),
        }
    }

    /// Plugin `id`'s setting `key`, under `[plugins."ID"]`.
    pub fn plugin(
        id: &str,
        key: &str,
        kind: FieldKind,
        default: Value,
        description: String,
    ) -> Field {
        Field {
            path: vec!["plugins".into(), id.into(), key.into()],
            key: format!("plugins.\"{id}\".{key}"),
            name: key.to_string(),
            kind,
            default,
            description,
            spec: None,
        }
    }
}

/// How a setting is changed from the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Stepped in place ([`step`]): switches, choices, numbers.
    Step,
    /// Typed: text (stepped too among its usual values), JSON.
    Type,
    /// Item by item ([`items`]): lists and tables.
    Items,
}

/// The table of `key`: `editor` for `editor.vim.leader`.
pub fn section(key: &str) -> &str {
    key.split_once('.').map_or(key, |(s, _)| s)
}

/// `key` within its table: `vim.leader` for `editor.vim.leader`.
pub fn name(key: &str) -> &str {
    key.split_once('.').map_or(key, |(_, n)| n)
}

/// How `field` is changed from the list.
pub fn edit(field: &Field) -> Edit {
    match field.kind {
        FieldKind::Bool | FieldKind::Enum(_) | FieldKind::Int(..) => Edit::Step,
        FieldKind::Text(_) | FieldKind::Json => Edit::Type,
        FieldKind::Choices(_) | FieldKind::Texts | FieldKind::Table(_) => Edit::Items,
    }
}

/// `field`'s value in `config`: the user's (or the workspace's), else
/// its default.
pub fn value(config: &Config, field: &Field) -> Value {
    let parts: Vec<&str> = field.path.iter().map(String::as_str).collect();
    config
        .get_path(&parts)
        .cloned()
        .unwrap_or_else(|| field.default.clone())
}

/// Whether `field`'s value in `config` differs from its default.
pub fn changed(config: &Config, field: &Field) -> bool {
    value(config, field) != field.default
}

/// `field`'s value in `config` as the list shows it.
pub fn shown(config: &Config, field: &Field) -> String {
    show(field, &value(config, field))
}

/// `field`'s default as the list shows it.
pub fn shown_default(field: &Field) -> String {
    show(field, &field.default)
}

/// `v`, a value of `field`, for reading: a switch as on or off, the
/// system's font for no font, nothing as "empty", a list's items and a
/// table's entries joined.
fn show(field: &Field, v: &Value) -> String {
    let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    match v {
        Value::Bool(b) if field.kind == FieldKind::Bool => {
            tr(if *b { "settings-on" } else { "settings-off" })
        }
        Value::String(s) if s.is_empty() && field.key.ends_with("font_family") => {
            tr("settings-system-font")
        }
        Value::String(s) if s.is_empty() => tr("settings-empty"),
        Value::String(s) => s.clone(),
        Value::Null => tr("settings-empty"),
        Value::Array(a) if a.is_empty() => tr("settings-empty"),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        Value::Object(m) if m.is_empty() => tr("settings-empty"),
        Value::Object(m) if field.kind == FieldKind::Json => v.to_string(),
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| format!("{k} = {}", text(v)))
            .collect::<Vec<_>>()
            .join(", "),
        v => v.to_string(),
    }
}

/// The usual values of a text setting, stepped through with `h` and `l`
/// so that they need no typing: the fonts installed for a font, the TeX
/// engines, the search sites… and its default.
pub fn candidates(field: &Field) -> Vec<String> {
    let FieldKind::Text(own) = &field.kind else {
        return Vec::new();
    };
    let known: &[&str] = match field.key.as_str() {
        "editor.vim.leader" => &["space", ",", "\\", ";"],
        "search.online_url" => &[
            "https://duckduckgo.com/?q=%s",
            "https://www.google.com/search?q=%s",
            "https://www.bing.com/search?q=%s",
            "https://www.startpage.com/do/search?q=%s",
            "https://www.ecosia.org/search?q=%s",
            "https://kagi.com/search?q=%s",
        ],
        "latex.engine" => &["auto", "pdflatex", "xelatex", "lualatex", "tectonic"],
        "latex.output_directory" => &["", "build", "out"],
        "org.footnote_section" => &["Footnotes", ""],
        "org.log_into_drawer" => &["", "LOGBOOK"],
        "org.assets_dir" => &["{name}_assets", "{name}", "assets", "images"],
        "markdown.assets_dir" => &["images", "assets", "{name}_assets", "{name}"],
        "notes.directory" => &["~/org", "~/notes", "~/Documents/notes"],
        _ => &[],
    };
    let mut out: Vec<String> = field
        .default
        .as_str()
        .map(str::to_string)
        .into_iter()
        .collect();
    out.extend(known.iter().map(|s| (*s).to_string()));
    out.extend(own.iter().cloned());
    match field.key.as_str() {
        "editor.font_family" => out.extend(crate::fonts::families(false).iter().cloned()),
        "editor.code_font_family" => out.extend(crate::fonts::families(true).iter().cloned()),
        "export.pandoc_path" => out.extend(
            [
                "/opt/homebrew/bin/pandoc",
                "/usr/local/bin/pandoc",
                "/usr/bin/pandoc",
            ]
            .iter()
            .filter(|p| std::path::Path::new(p).exists())
            .map(|p| (*p).to_string()),
        ),
        _ => {}
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(c.clone()));
    out
}

/// The value after `field`'s in `config`, a step forward or back: a
/// switch flipped, the next or previous choice or usual text (round), a
/// number up or down by a step that suits its range and kept in it. None
/// for lists, for a text without usual values and a number at the end of
/// its range.
pub fn step(config: &Config, field: &Field, forward: bool) -> Option<Value> {
    let v = value(config, field);
    let round = |options: &[String], current: Option<&str>| -> Option<Value> {
        let n = options.len();
        if n == 0 {
            return None;
        }
        let to = match options.iter().position(|o| Some(o.as_str()) == current) {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None if forward => 0,
            None => n - 1,
        };
        (Some(options[to].as_str()) != current).then(|| Value::from(options[to].clone()))
    };
    match &field.kind {
        FieldKind::Bool => Some(Value::Bool(!v.as_bool()?)),
        FieldKind::Enum(options) => round(options, v.as_str()),
        FieldKind::Text(_) => round(&candidates(field), v.as_str()),
        FieldKind::Int(lo, hi) => {
            let span = hi.saturating_sub(*lo);
            let by = if span <= 100 {
                1
            } else if span <= 1000 {
                10
            } else {
                100
            };
            let n = v.as_i64()?;
            let to = if forward {
                n.saturating_add(by)
            } else {
                n.saturating_sub(by)
            }
            .clamp(*lo, *hi);
            (to != n).then(|| Value::from(to))
        }
        _ => None,
    }
}

/// An item as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The choice, the text, or the entry as `path = mode`.
    pub text: String,
    /// For a list of choices: whether the choice is in it.
    pub on: Option<bool>,
}

/// The strings of the list `field` holds in `config`.
fn texts(config: &Config, field: &Field) -> Vec<String> {
    value(config, field)
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The entries of the table `field` holds in `config`, in its order.
fn entries(config: &Config, field: &Field) -> Vec<(String, String)> {
    value(config, field)
        .as_object()
        .map(|t| {
            t.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn list(items: impl IntoIterator<Item = String>) -> Value {
    Value::Array(items.into_iter().map(Value::String).collect())
}

fn table(entries: impl IntoIterator<Item = (String, String)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect(),
    )
}

/// Whether `field` has items to show: a list or a table.
pub fn has_items(field: &Field) -> bool {
    edit(field) == Edit::Items
}

/// The items of `field` in `config`: every choice of a list of choices,
/// marked in or out; a list's texts; a table's entries.
pub fn items(config: &Config, field: &Field) -> Vec<Item> {
    match &field.kind {
        FieldKind::Choices(choices) => {
            let on = texts(config, field);
            choices
                .iter()
                .map(|c| Item {
                    text: c.clone(),
                    on: Some(on.contains(c)),
                })
                .collect()
        }
        FieldKind::Texts => texts(config, field)
            .into_iter()
            .map(|text| Item { text, on: None })
            .collect(),
        FieldKind::Table(_) => entries(config, field)
            .into_iter()
            .map(|(k, v)| Item {
                text: format!("{k} = {v}"),
                on: None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The list of choices with choice `i` put in or taken out; the choices
/// in, in their order.
pub fn toggle(config: &Config, field: &Field, i: usize) -> Option<Value> {
    let FieldKind::Choices(choices) = &field.kind else {
        return None;
    };
    let mut on = texts(config, field);
    let c = choices.get(i)?;
    match on.iter().position(|o| o == c) {
        Some(at) => {
            on.remove(at);
        }
        None => on.push(c.clone()),
    }
    Some(list(choices.iter().filter(|c| on.contains(c)).cloned()))
}

/// The list or the table without its item `i`.
pub fn remove(config: &Config, field: &Field, i: usize) -> Option<Value> {
    match &field.kind {
        FieldKind::Texts => {
            let mut t = texts(config, field);
            (i < t.len()).then(|| {
                t.remove(i);
                list(t)
            })
        }
        FieldKind::Table(_) => {
            let mut e = entries(config, field);
            (i < e.len()).then(|| {
                e.remove(i);
                table(e)
            })
        }
        _ => None,
    }
}

/// The list with its text `i` moved a place up or down, and the place
/// it has then.
pub fn shift(config: &Config, field: &Field, i: usize, up: bool) -> Option<(Value, usize)> {
    if field.kind != FieldKind::Texts {
        return None;
    }
    let mut t = texts(config, field);
    let to = if up { i.checked_sub(1)? } else { i + 1 };
    if to >= t.len() || i >= t.len() {
        return None;
    }
    t.swap(i, to);
    Some((list(t), to))
}

/// The table with the mode of its entry `i` the next or the previous
/// known mode (from a language, the first).
pub fn cycle(config: &Config, field: &Field, i: usize, forward: bool) -> Option<Value> {
    let FieldKind::Table(modes) = &field.kind else {
        return None;
    };
    let mut e = entries(config, field);
    let (_, mode) = e.get_mut(i)?;
    let n = modes.len();
    let to = match modes.iter().position(|m| m == mode) {
        Some(at) if forward => (at + 1) % n,
        Some(at) => (at + n - 1) % n,
        None => 0,
    };
    *mode = modes.get(to)?.clone();
    Some(table(e))
}

/// The list or the table with `text` in place of item `at`, or added at
/// the end; a table's entry is typed `path = mode`. The value is checked
/// as the settings file's would be.
pub fn put(config: &Config, field: &Field, at: Option<usize>, text: &str) -> Result<Value, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(tr("settings-item-empty"));
    }
    let value = match &field.kind {
        FieldKind::Texts => {
            let mut t = texts(config, field);
            match at.filter(|&i| i < t.len()) {
                Some(i) => t[i] = text.to_string(),
                None => t.push(text.to_string()),
            }
            list(t)
        }
        FieldKind::Table(_) => {
            let Some((path, mode)) = text
                .split_once('=')
                .map(|(p, m)| (p.trim(), m.trim()))
                .filter(|(p, m)| !p.is_empty() && !m.is_empty())
            else {
                return Err(tr("settings-item-table"));
            };
            let mut e = entries(config, field);
            if let Some(i) = at.filter(|&i| i < e.len()) {
                e.remove(i);
            }
            e.retain(|(p, _)| p != path);
            e.push((path.to_string(), mode.to_string()));
            table(e)
        }
        _ => return Err(tr("settings-item-empty")),
    };
    check(field, &value)?;
    Ok(value)
}

/// The value of `field` typed as `text`: the text itself, or for a
/// value of any kind its JSON (a bare word taken as text).
pub fn typed(field: &Field, text: &str) -> Result<Value, String> {
    let value = match field.kind {
        FieldKind::Json => {
            serde_json::from_str(text.trim()).unwrap_or_else(|_| Value::from(text.trim()))
        }
        _ => Value::from(text),
    };
    check(field, &value)?;
    Ok(value)
}

/// Whether `value` suits `field`, and why not.
pub fn check(field: &Field, value: &Value) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    if let Some(spec) = field.spec {
        return crate::settings::check(spec.kind, value)
            .map_err(|e| format!("`{}` {e}", field.key));
    }
    let ok = match &field.kind {
        FieldKind::Bool => value.is_boolean(),
        FieldKind::Int(lo, hi) => value.as_i64().is_some_and(|n| (*lo..=*hi).contains(&n)),
        FieldKind::Enum(options) => value
            .as_str()
            .is_some_and(|s| options.iter().any(|o| o == s)),
        FieldKind::Text(_) => value.is_string(),
        FieldKind::Choices(choices) => value.as_array().is_some_and(|a| {
            a.iter()
                .all(|v| v.as_str().is_some_and(|s| choices.iter().any(|c| c == s)))
        }),
        FieldKind::Texts => value
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string)),
        FieldKind::Table(_) => value.is_object(),
        FieldKind::Json => true,
    };
    if ok {
        Ok(())
    } else {
        Err(crate::tr!("settings-wrong-value", key = field.key.as_str()))
    }
}

/// Saves `value` as `field`'s in the settings file `file` (removes it
/// for `null`): Kalem's settings checked by their key, a plugin's by its
/// field.
pub fn save(file: &std::path::Path, field: &Field, value: &Value) -> Result<(), String> {
    match field.spec {
        Some(spec) => crate::settings::save_setting(file, spec.key, value),
        None => {
            check(field, value)?;
            let parts: Vec<&str> = field.path.iter().map(String::as_str).collect();
            crate::settings::save_at(file, &parts, value)
        }
    }
}

/// What the panel shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Page {
    /// Kalem's settings, by table.
    #[default]
    Settings,
    /// The installed plugins.
    Plugins,
    /// A plugin's settings and what can be done with it, by its ID.
    Plugin(String),
}

/// Something chosen in the list.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A setting.
    Field(Field),
    /// The installed plugins (how many): their page.
    Plugins(usize),
    /// A plugin: its page.
    Plugin(PluginInfo),
    /// Something done with a plugin: a command run.
    Action {
        /// What the list shows.
        label: String,
        /// What it does, shown below.
        about: String,
        /// The command.
        command: String,
        /// Its arguments.
        args: Value,
    },
}

impl Entry {
    /// The name the list shows.
    pub fn name(&self) -> String {
        match self {
            Entry::Field(f) => f.name.clone(),
            Entry::Plugins(_) => tr("settings-plugins"),
            Entry::Plugin(p) => p.name.clone(),
            Entry::Action { label, .. } => label.clone(),
        }
    }

    /// The value the list shows beside the name.
    pub fn value(&self, config: &Config) -> String {
        match self {
            Entry::Field(f) => shown(config, f),
            Entry::Plugins(n) => format!("{n} ›"),
            Entry::Plugin(p) => format!("{} ›", p.version),
            Entry::Action { .. } => String::new(),
        }
    }

    /// Whether the value stands out: a setting not at its default, a
    /// plugin turned off.
    pub fn changed(&self, config: &Config) -> bool {
        match self {
            Entry::Field(f) => changed(config, f),
            Entry::Plugin(p) => p.turned_off,
            _ => false,
        }
    }

    /// What is said below the list: a heading line and a description.
    pub fn about(&self, config: &Config) -> (String, String) {
        match self {
            Entry::Field(f) => {
                let note = match edit(f) {
                    Edit::Items => crate::tr!("settings-items", count = items(config, f).len()),
                    _ => crate::tr!("settings-default", value = shown_default(f)),
                };
                (format!("{}  {note}", f.key), f.description.clone())
            }
            Entry::Plugins(n) => (
                crate::tr!("settings-plugins-count", count = *n),
                tr("settings-plugins-about"),
            ),
            Entry::Plugin(p) => (
                format!("{}  {}", p.id, p.version),
                if p.turned_off {
                    format!("{} {}", tr("settings-plugin-off"), p.description)
                } else {
                    p.description.clone()
                },
            ),
            Entry::Action { label, about, .. } => (label.clone(), about.clone()),
        }
    }

    /// The setting, if it is one.
    pub fn field(&self) -> Option<&Field> {
        match self {
            Entry::Field(f) => Some(f),
            _ => None,
        }
    }
}

/// A line of a page.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// A heading: a table's name, a plugin's.
    Heading(String),
    /// Something to choose.
    Entry(Entry),
}

/// Whether `entry` holds `filter` (lower case) in its name, key or
/// description.
fn matches(entry: &Entry, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let has = |s: &str| s.to_lowercase().contains(filter);
    match entry {
        Entry::Field(f) => has(&f.key) || has(&f.description),
        Entry::Plugins(_) => has(&tr("settings-plugins")) || has("plugins"),
        Entry::Plugin(p) => has(&p.name) || has(&p.id) || has(&p.description),
        Entry::Action { label, about, .. } => has(label) || has(about),
    }
}

/// Kalem's settings matching `filter`, grouped by table in the order the
/// tables first appear in [`SPECS`], each after its name; the installed
/// plugins' page first under `plugins`.
fn settings_rows(filter: &str, plugins: usize) -> Vec<Row> {
    let mut sections: Vec<(&'static str, Vec<Entry>)> = Vec::new();
    for s in SPECS {
        let table = section(s.key);
        let i = match sections.iter().position(|(n, _)| *n == table) {
            Some(i) => i,
            None => {
                let link = (table == "plugins").then_some(Entry::Plugins(plugins));
                sections.push((
                    table,
                    link.into_iter().filter(|l| matches(l, filter)).collect(),
                ));
                sections.len() - 1
            }
        };
        let entry = Entry::Field(Field::of(s));
        if matches(&entry, filter) {
            sections[i].1.push(entry);
        }
    }
    let mut out = Vec::new();
    for (table, entries) in sections {
        if entries.is_empty() {
            continue;
        }
        out.push(Row::Heading(table.to_string()));
        out.extend(entries.into_iter().map(Row::Entry));
    }
    out
}

/// The list as browsed: the page, what is typed to filter it and what is
/// chosen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Browser {
    /// The page shown.
    pub page: Page,
    /// The filter.
    pub filter: String,
    /// Keys type into the filter (after `/`).
    pub filtering: bool,
    /// The chosen entry, among those shown.
    pub selected: usize,
    /// The chosen setting's items shown (a list or a table), and the item
    /// chosen among them.
    pub item: Option<usize>,
    /// The pages to go back to, with their choice and filter.
    back: Vec<(Page, usize, String)>,
    /// The installed plugins, read when first needed.
    plugins: OnceCell<Vec<PluginInfo>>,
}

impl Browser {
    /// The installed plugins.
    pub fn plugins(&self) -> &[PluginInfo] {
        self.plugins.get_or_init(crate::plugin_settings::installed)
    }

    /// Sets the installed plugins (read again after one changed; tests).
    pub fn set_plugins(&mut self, plugins: Vec<PluginInfo>) {
        self.plugins = OnceCell::from(plugins);
    }

    /// The lines of the page shown.
    pub fn rows(&self, config: &Config) -> Vec<Row> {
        let f = self.filter.trim().to_lowercase();
        match &self.page {
            Page::Settings => settings_rows(&f, self.plugins().len()),
            Page::Plugins => {
                let mut out = vec![Row::Heading(tr("settings-plugins"))];
                out.extend(
                    self.plugins()
                        .iter()
                        .map(|p| Entry::Plugin(p.clone()))
                        .filter(|e| matches(e, &f))
                        .map(Row::Entry),
                );
                out
            }
            Page::Plugin(id) => match self.plugins().iter().find(|p| &p.id == id) {
                Some(p) => crate::plugin_settings::rows(p, config)
                    .into_iter()
                    .filter(|r| match r {
                        Row::Entry(e) => matches(e, &f),
                        Row::Heading(_) => true,
                    })
                    .collect(),
                None => Vec::new(),
            },
        }
    }

    /// The entries of the page shown.
    pub fn entries(&self, config: &Config) -> Vec<Entry> {
        self.rows(config)
            .into_iter()
            .filter_map(|r| match r {
                Row::Entry(e) => Some(e),
                Row::Heading(_) => None,
            })
            .collect()
    }

    /// The chosen entry.
    pub fn current(&self, config: &Config) -> Option<Entry> {
        let mut e = self.entries(config);
        if e.is_empty() {
            return None;
        }
        let i = self.selected.min(e.len() - 1);
        Some(e.swap_remove(i))
    }

    /// The chosen setting, if a setting is chosen.
    pub fn current_field(&self, config: &Config) -> Option<Field> {
        match self.current(config)? {
            Entry::Field(f) => Some(f),
            _ => None,
        }
    }

    /// Moves the choice by `by` entries, kept in the list.
    pub fn move_by(&mut self, by: isize, config: &Config) {
        let last = self.entries(config).len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(by).min(last);
    }

    /// Chooses the last entry.
    pub fn last(&mut self, config: &Config) {
        self.selected = self.entries(config).len().saturating_sub(1);
    }

    /// Chooses the setting `key`, if shown.
    pub fn choose(&mut self, key: &str, config: &Config) {
        if let Some(i) = self
            .entries(config)
            .iter()
            .position(|e| e.field().is_some_and(|f| f.key == key))
        {
            self.selected = i;
        }
    }

    /// Sets the filter; the choice goes back to the first entry.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.selected = 0;
    }

    /// Goes into the chosen entry when it is a page (the plugins, a
    /// plugin) or a list's or a table's items: whether it did.
    pub fn open(&mut self, config: &Config) -> bool {
        let to = match self.current(config) {
            Some(Entry::Plugins(_)) => Page::Plugins,
            Some(Entry::Plugin(p)) => Page::Plugin(p.id),
            Some(Entry::Field(f)) if has_items(&f) => {
                self.item = Some(0);
                return true;
            }
            _ => return false,
        };
        let from = std::mem::replace(&mut self.page, to);
        self.back
            .push((from, self.selected, std::mem::take(&mut self.filter)));
        self.selected = 0;
        self.filtering = false;
        true
    }

    /// Back from items to the list, else to the page before: whether
    /// there was somewhere to go back to.
    pub fn back(&mut self) -> bool {
        if self.item.take().is_some() {
            return true;
        }
        match self.back.pop() {
            Some((page, selected, filter)) => {
                self.page = page;
                self.selected = selected;
                self.filter = filter;
                true
            }
            None => false,
        }
    }

    /// Moves the chosen item by `by`, kept among `count` items.
    pub fn move_item(&mut self, by: isize, count: usize) {
        if let Some(i) = &mut self.item {
            *i = i.saturating_add_signed(by).min(count.saturating_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str) -> Field {
        Field::of(SPECS.iter().find(|s| s.key == key).unwrap())
    }

    /// The settings with only `key` set by the user, to `value`.
    fn with(key: &str, value: &Value) -> Config {
        let parts: Vec<&str> = key.split('.').collect();
        let text = crate::settings::set_in_toml("", &parts, value).unwrap();
        Config::from_layers(&[(crate::settings::Layer::User, None, &text)])
    }

    fn keys(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .filter_map(|r| match r {
                Row::Entry(Entry::Field(f)) => Some(f.key.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_setting_is_listed_once_under_its_table() {
        let all = settings_rows("", 2);
        let mut found = keys(&all);
        found.sort();
        let mut want: Vec<String> = SPECS.iter().map(|s| s.key.to_string()).collect();
        want.sort();
        assert_eq!(found, want);
        // Each table once, its settings right after it; the plugins'
        // page under `plugins`.
        let mut table = String::new();
        let mut seen = Vec::new();
        for r in &all {
            match r {
                Row::Heading(s) => {
                    assert!(!seen.contains(s), "{s} twice");
                    seen.push(s.clone());
                    table = s.clone();
                }
                Row::Entry(Entry::Field(f)) => assert_eq!(section(&f.key), table),
                Row::Entry(Entry::Plugins(n)) => {
                    assert_eq!((table.as_str(), *n), ("plugins", 2));
                }
                Row::Entry(_) => panic!("{r:?}"),
            }
        }
        assert_eq!(seen[0], "editor");
    }

    #[test]
    fn the_filter_looks_at_keys_and_descriptions() {
        let found = |f: &str| keys(&settings_rows(f, 0));
        assert_eq!(found("auto_add"), vec!["projects.auto_add"]);
        assert!(found("version control").contains(&"projects.auto_add".to_string()));
        assert!(settings_rows("no such setting at all", 0).is_empty());
    }

    #[test]
    fn values_step_in_place() {
        let config = Config::default();
        let auto = field("projects.auto_add");
        assert_eq!(edit(&auto), Edit::Step);
        assert_eq!(step(&config, &auto, true), Some(Value::Bool(true)));
        assert_eq!(shown(&config, &auto), tr("settings-off"));
        assert!(!changed(&config, &auto));
        // Choices go round, both ways.
        let theme = field("editor.theme");
        assert_eq!(step(&config, &theme, true), Some(Value::from("light")));
        assert_eq!(step(&config, &theme, false), Some(Value::from("dark")));
        // Numbers by a step suiting their range, kept in it.
        let size = field("editor.font_size");
        assert_eq!(step(&config, &size, true), Some(Value::from(17)));
        let delay = field("keys.hints_delay");
        assert_eq!(step(&config, &delay, false), Some(Value::from(300)));
        let width = field("editor.line_width");
        assert_eq!(step(&config, &width, false), None, "0 is its least");
        // A text steps through its usual values, and is typed too.
        let engine = field("latex.engine");
        assert_eq!(edit(&engine), Edit::Type);
        assert_eq!(step(&config, &engine, true), Some(Value::from("pdflatex")));
        assert_eq!(step(&config, &engine, false), Some(Value::from("tectonic")));
        let other = with("latex.engine", &Value::from("/opt/tex/bin/xelatex"));
        assert_eq!(step(&other, &engine, true), Some(Value::from("auto")));
        assert_eq!(
            step(&config, &field("latex.root"), true),
            None,
            "no other value"
        );
        // Lists are edited item by item.
        assert_eq!(edit(&field("plugins.sources")), Edit::Items);
        assert_eq!(
            shown(&config, &field("plugins.sources")),
            tr("settings-empty")
        );
        assert_eq!(shown(&config, &field("org.todo_keywords")), "TODO, |, DONE");
        assert_eq!(
            shown(&config, &field("editor.font_family")),
            tr("settings-system-font")
        );
    }

    #[test]
    fn changed_values_are_marked() {
        let config = with("projects.auto_add", &Value::Bool(true));
        let auto = field("projects.auto_add");
        assert!(changed(&config, &auto));
        assert_eq!(shown(&config, &auto), tr("settings-on"));
        assert_eq!(shown_default(&auto), tr("settings-off"));
    }

    #[test]
    fn items_of_lists_and_tables_change_one_by_one() {
        let config = Config::default();
        // A list of choices: each one in or out, in the choices' order.
        let modes = field("editor.vim.modes");
        let all = items(&config, &modes);
        assert!(all.iter().all(|i| i.on == Some(false)));
        let csv = all.iter().position(|i| i.text == "csv").unwrap();
        let org = all.iter().position(|i| i.text == "org").unwrap();
        let v = toggle(&config, &modes, csv).unwrap();
        let config = with("editor.vim.modes", &v);
        let v = toggle(&config, &modes, org).unwrap();
        assert_eq!(v, serde_json::json!(["org", "csv"]));
        let config = with("editor.vim.modes", &v);
        assert_eq!(
            toggle(&config, &modes, org),
            Some(serde_json::json!(["csv"]))
        );
        // A list of texts: added, edited, moved, removed.
        let todo = field("org.todo_keywords");
        assert_eq!(items(&config, &todo).len(), 3);
        let v = put(&config, &todo, None, " WAIT ").unwrap();
        let config = with("org.todo_keywords", &v);
        assert_eq!(texts(&config, &todo), ["TODO", "|", "DONE", "WAIT"]);
        let (v, to) = shift(&config, &todo, 3, true).unwrap();
        assert_eq!(to, 2);
        let config = with("org.todo_keywords", &v);
        assert_eq!(texts(&config, &todo), ["TODO", "|", "WAIT", "DONE"]);
        assert!(shift(&config, &todo, 0, true).is_none());
        let v = put(&config, &todo, Some(2), "NEXT").unwrap();
        let config = with("org.todo_keywords", &v);
        let v = remove(&config, &todo, 1).unwrap();
        assert_eq!(v, serde_json::json!(["TODO", "NEXT", "DONE"]));
        assert!(put(&config, &todo, None, "  ").is_err());
        // A table: `path = mode` typed, its mode stepped, an entry gone.
        let files = field("files.modes");
        assert!(put(&config, &files, None, "notes.txt").is_err());
        assert!(put(&config, &files, None, " = markdown").is_err());
        let v = put(&config, &files, None, "notes.txt = markdown").unwrap();
        let config = with("files.modes", &v);
        assert_eq!(items(&config, &files)[0].text, "notes.txt = markdown");
        let v = cycle(&config, &files, 0, true).unwrap();
        assert_ne!(v["notes.txt"], "markdown");
        let v = put(&config, &files, Some(0), "b.rs = rust").unwrap();
        assert_eq!(v, serde_json::json!({"b.rs": "rust"}));
        let config = with("files.modes", &v);
        assert_eq!(remove(&config, &files, 0), Some(serde_json::json!({})));
        // A list of choices has no texts to type.
        assert!(put(&config, &modes, None, "org").is_err());
    }

    #[test]
    fn plugin_settings_are_checked_by_their_field() {
        let goal = Field::plugin(
            "org.example.count",
            "goal",
            FieldKind::Int(0, 100),
            Value::from(10),
            String::new(),
        );
        assert_eq!(goal.key, "plugins.\"org.example.count\".goal");
        assert!(check(&goal, &Value::from(50)).is_ok());
        assert!(check(&goal, &Value::from(500)).is_err());
        assert!(check(&goal, &Value::from("x")).is_err());
        let config = Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[plugins.\"org.example.count\"]\ngoal = 20\n",
        )]);
        assert_eq!(value(&config, &goal), Value::from(20));
        assert!(changed(&config, &goal));
        assert_eq!(step(&config, &goal, true), Some(Value::from(21)));
        let any = Field::plugin("p", "x", FieldKind::Json, Value::Null, String::new());
        assert_eq!(typed(&any, "[1, 2]").unwrap(), serde_json::json!([1, 2]));
        assert_eq!(typed(&any, "word").unwrap(), Value::from("word"));
    }

    #[test]
    fn pages_open_and_go_back() {
        let config = Config::default();
        let mut b = Browser::default();
        b.set_plugins(vec![crate::plugin_settings::tests::example()]);
        assert!(matches!(b.current(&config), Some(Entry::Field(_))));
        b.move_by(-3, &config);
        assert_eq!(b.selected, 0);
        b.last(&config);
        let n = b.entries(&config).len();
        assert_eq!(b.selected, n - 1);
        // The plugins' page, then one plugin's.
        b.set_filter("plugins");
        let at = b
            .entries(&config)
            .iter()
            .position(|e| matches!(e, Entry::Plugins(1)))
            .expect("the plugins' page");
        b.selected = at;
        assert!(b.open(&config));
        assert_eq!(b.page, Page::Plugins);
        assert!(b.filter.is_empty());
        assert!(matches!(b.current(&config), Some(Entry::Plugin(_))));
        assert!(b.open(&config));
        assert_eq!(b.page, Page::Plugin("org.example.count".into()));
        let names: Vec<String> = b.entries(&config).iter().map(Entry::name).collect();
        assert!(names.contains(&"goal".to_string()), "{names:?}");
        // Back to the plugins, then to the settings with their filter.
        assert!(b.back());
        assert_eq!(b.page, Page::Plugins);
        assert!(b.back());
        assert_eq!(
            (b.page.clone(), b.filter.as_str(), b.selected),
            (Page::Settings, "plugins", at)
        );
        assert!(!b.back());
        // A list's items, and back.
        b.set_filter("todo_keywords");
        assert!(b.open(&config));
        assert_eq!(b.item, Some(0));
        assert!(b.back());
        assert_eq!(b.item, None);
        b.choose("org.todo_keywords", &config);
        assert_eq!(
            b.current_field(&config).map(|f| f.key),
            Some("org.todo_keywords".into())
        );
    }
}
