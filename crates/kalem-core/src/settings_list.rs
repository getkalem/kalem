//! Every setting as a list to browse and change, as the terminal
//! editor's settings panel shows it (lazygit's way: a framed list and a
//! key for each change). The settings are grouped by their table
//! (`editor`, `ui`, `projects`…), filtered by typed text, and changed in
//! place: a switch flipped, the next choice, a number up or down. Text
//! is typed; a list's or a table's items are shown and changed one by one
//! ([`items`]), so that no setting needs `settings.toml` opened.

use serde_json::Value;

use crate::l10n::tr;
use crate::settings::{Config, Kind, SPECS, Spec};

/// A line of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// A table's name, such as `editor`, before its settings.
    Section(&'static str),
    /// A setting: its index in [`SPECS`].
    Setting(usize),
}

/// How a setting is changed from the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Stepped in place ([`step`]): switches, choices, numbers.
    Step,
    /// Typed: text.
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

/// The settings whose key or description holds `filter` (in any case),
/// grouped by table, the tables in the order they first appear in
/// [`SPECS`], each after its name.
pub fn lines(filter: &str) -> Vec<Line> {
    let f = filter.trim().to_lowercase();
    let mut sections: Vec<(&'static str, Vec<usize>)> = Vec::new();
    for (i, s) in SPECS.iter().enumerate() {
        if !f.is_empty()
            && !s.key.to_lowercase().contains(&f)
            && !s.description.to_lowercase().contains(&f)
        {
            continue;
        }
        let table = section(s.key);
        match sections.iter_mut().find(|(n, _)| *n == table) {
            Some((_, items)) => items.push(i),
            None => sections.push((table, vec![i])),
        }
    }
    let mut out = Vec::new();
    for (table, items) in sections {
        out.push(Line::Section(table));
        out.extend(items.into_iter().map(Line::Setting));
    }
    out
}

/// How `spec` is changed from the list.
pub fn edit(spec: &Spec) -> Edit {
    match spec.kind {
        Kind::Bool | Kind::Enum(_) | Kind::Int(..) => Edit::Step,
        Kind::Str => Edit::Type,
        Kind::List(_) | Kind::Modes(_) => Edit::Items,
    }
}

/// How the items of a list or a table setting are changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Items {
    /// A list of some of known choices (`editor.vim.modes`): each choice
    /// is in or out.
    Choices(&'static [&'static str]),
    /// A list of texts (`org.todo_keywords`): added, edited, removed and
    /// moved.
    Texts,
    /// A table from paths to modes (`files.modes`): entries typed as
    /// `path = mode`, their modes stepped.
    Table(&'static [&'static str]),
}

/// An item as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The choice, the text, or the entry as `path = mode`.
    pub text: String,
    /// For a list of choices: whether the choice is in it.
    pub on: Option<bool>,
}

/// How `spec`'s items are changed, for a list or a table.
pub fn items_kind(spec: &Spec) -> Option<Items> {
    match spec.kind {
        Kind::List(Some(choices)) => Some(Items::Choices(choices)),
        Kind::List(None) => Some(Items::Texts),
        Kind::Modes(modes) => Some(Items::Table(modes)),
        _ => None,
    }
}

/// The strings of the list `spec` holds in `config`.
fn texts(config: &Config, spec: &Spec) -> Vec<String> {
    config
        .get(spec.key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The entries of the table `spec` holds in `config`, in its order.
fn entries(config: &Config, spec: &Spec) -> Vec<(String, String)> {
    config
        .get(spec.key)
        .and_then(Value::as_object)
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

/// The items of `spec` in `config`: every choice of a list of choices,
/// marked in or out; a list's texts; a table's entries.
pub fn items(config: &Config, spec: &Spec) -> Vec<Item> {
    match items_kind(spec) {
        Some(Items::Choices(choices)) => {
            let on = texts(config, spec);
            choices
                .iter()
                .map(|c| Item {
                    text: (*c).to_string(),
                    on: Some(on.iter().any(|o| o == c)),
                })
                .collect()
        }
        Some(Items::Texts) => texts(config, spec)
            .into_iter()
            .map(|text| Item { text, on: None })
            .collect(),
        Some(Items::Table(_)) => entries(config, spec)
            .into_iter()
            .map(|(k, v)| Item {
                text: format!("{k} = {v}"),
                on: None,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// The list of choices with choice `i` put in or taken out; the choices
/// in, in their order.
pub fn toggle(config: &Config, spec: &Spec, i: usize) -> Option<Value> {
    let Some(Items::Choices(choices)) = items_kind(spec) else {
        return None;
    };
    let mut on = texts(config, spec);
    let c = choices.get(i)?;
    match on.iter().position(|o| o == c) {
        Some(at) => {
            on.remove(at);
        }
        None => on.push((*c).to_string()),
    }
    Some(list(
        choices
            .iter()
            .filter(|c| on.iter().any(|o| o == *c))
            .map(|c| (*c).to_string()),
    ))
}

/// The list or the table without its item `i`.
pub fn remove(config: &Config, spec: &Spec, i: usize) -> Option<Value> {
    match items_kind(spec)? {
        Items::Texts => {
            let mut t = texts(config, spec);
            (i < t.len()).then(|| {
                t.remove(i);
                list(t)
            })
        }
        Items::Table(_) => {
            let mut e = entries(config, spec);
            (i < e.len()).then(|| {
                e.remove(i);
                table(e)
            })
        }
        Items::Choices(_) => None,
    }
}

/// The list with its text `i` moved a place up or down, and the place
/// it has then.
pub fn shift(config: &Config, spec: &Spec, i: usize, up: bool) -> Option<(Value, usize)> {
    let Some(Items::Texts) = items_kind(spec) else {
        return None;
    };
    let mut t = texts(config, spec);
    let to = if up { i.checked_sub(1)? } else { i + 1 };
    if to >= t.len() || i >= t.len() {
        return None;
    }
    t.swap(i, to);
    Some((list(t), to))
}

/// The table with the mode of its entry `i` the next or the previous
/// known mode (from a language, the first).
pub fn cycle(config: &Config, spec: &Spec, i: usize, forward: bool) -> Option<Value> {
    let Some(Items::Table(modes)) = items_kind(spec) else {
        return None;
    };
    let mut e = entries(config, spec);
    let (_, mode) = e.get_mut(i)?;
    let n = modes.len();
    let to = match modes.iter().position(|m| m == mode) {
        Some(at) if forward => (at + 1) % n,
        Some(at) => (at + n - 1) % n,
        None => 0,
    };
    *mode = modes.get(to)?.to_string();
    Some(table(e))
}

/// The list or the table with `text` in place of item `at`, or added at
/// the end; a table's entry is typed `path = mode`. The value is checked
/// as the settings file's would be.
pub fn put(config: &Config, spec: &Spec, at: Option<usize>, text: &str) -> Result<Value, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(tr("settings-item-empty"));
    }
    let value = match items_kind(spec) {
        Some(Items::Texts) => {
            let mut t = texts(config, spec);
            match at.filter(|&i| i < t.len()) {
                Some(i) => t[i] = text.to_string(),
                None => t.push(text.to_string()),
            }
            list(t)
        }
        Some(Items::Table(_)) => {
            let Some((path, mode)) = text
                .split_once('=')
                .map(|(p, m)| (p.trim(), m.trim()))
                .filter(|(p, m)| !p.is_empty() && !m.is_empty())
            else {
                return Err(tr("settings-item-table"));
            };
            let mut e = entries(config, spec);
            if let Some(i) = at.filter(|&i| i < e.len()) {
                e.remove(i);
            }
            e.retain(|(p, _)| p != path);
            e.push((path.to_string(), mode.to_string()));
            table(e)
        }
        _ => return Err(tr("settings-item-empty")),
    };
    crate::settings::check(spec.kind, &value).map_err(|e| format!("`{}` {e}", spec.key))?;
    Ok(value)
}

/// `spec`'s default value.
pub fn default(spec: &Spec) -> Option<Value> {
    serde_json::from_str(spec.default).ok()
}

/// Whether `spec`'s value in `config` differs from its default.
pub fn changed(config: &Config, spec: &Spec) -> bool {
    config.get(spec.key) != default(spec).as_ref()
}

/// `spec`'s value in `config` as the list shows it.
pub fn shown(config: &Config, spec: &Spec) -> String {
    config
        .get(spec.key)
        .map_or_else(String::new, |v| show(spec, v))
}

/// `spec`'s default as the list shows it.
pub fn shown_default(spec: &Spec) -> String {
    default(spec).map_or_else(String::new, |v| show(spec, &v))
}

/// `v`, a value of `spec`, for reading: a switch as on or off, nothing
/// as "empty", a list's items and a table's entries joined.
fn show(spec: &Spec, v: &Value) -> String {
    let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    match v {
        Value::Bool(b) if spec.kind == Kind::Bool => {
            tr(if *b { "settings-on" } else { "settings-off" })
        }
        Value::String(s) if s.is_empty() => tr("settings-empty"),
        Value::String(s) => s.clone(),
        Value::Array(a) if a.is_empty() => tr("settings-empty"),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        Value::Object(m) if m.is_empty() => tr("settings-empty"),
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| format!("{k} = {}", text(v)))
            .collect::<Vec<_>>()
            .join(", "),
        v => v.to_string(),
    }
}

/// The value after `spec`'s in `config`, a step forward or back: a
/// switch flipped, the next or previous choice (round), a number up or
/// down by a step that suits its range and kept in it. None for text,
/// lists and a number at the end of its range.
pub fn step(config: &Config, spec: &Spec, forward: bool) -> Option<Value> {
    let v = config.get(spec.key)?;
    match spec.kind {
        Kind::Bool => Some(Value::Bool(!v.as_bool()?)),
        Kind::Enum(options) if !options.is_empty() => {
            let n = options.len();
            let to = match options.iter().position(|o| Some(*o) == v.as_str()) {
                Some(i) if forward => (i + 1) % n,
                Some(i) => (i + n - 1) % n,
                None => 0,
            };
            Some(Value::from(options[to]))
        }
        Kind::Int(lo, hi) => {
            let span = hi - lo;
            let by = if span <= 100 {
                1
            } else if span <= 1000 {
                10
            } else {
                100
            };
            let n = v.as_i64()?;
            let to = if forward { n + by } else { n - by }.clamp(lo, hi);
            (to != n).then(|| Value::from(to))
        }
        _ => None,
    }
}

/// The list as browsed: what is typed to filter it and the setting
/// chosen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Browser {
    /// The filter.
    pub filter: String,
    /// Keys type into the filter (after `/`).
    pub filtering: bool,
    /// The chosen setting, among those shown.
    pub selected: usize,
    /// The chosen setting's items shown (a list or a table), and the item
    /// chosen among them.
    pub item: Option<usize>,
}

impl Browser {
    /// Shows the chosen setting's items, if it has some to show.
    pub fn open_items(&mut self) -> bool {
        let open = self.current().is_some_and(|s| items_kind(s).is_some());
        if open {
            self.item = Some(0);
        }
        open
    }

    /// Moves the chosen item by `by`, kept among `count` items.
    pub fn move_item(&mut self, by: isize, count: usize) {
        if let Some(i) = &mut self.item {
            *i = i.saturating_add_signed(by).min(count.saturating_sub(1));
        }
    }

    /// The lines shown.
    pub fn lines(&self) -> Vec<Line> {
        lines(&self.filter)
    }

    /// The settings shown, as indices in [`SPECS`].
    pub fn shown(&self) -> Vec<usize> {
        self.lines()
            .into_iter()
            .filter_map(|l| match l {
                Line::Setting(i) => Some(i),
                Line::Section(_) => None,
            })
            .collect()
    }

    /// The chosen setting.
    pub fn current(&self) -> Option<&'static Spec> {
        let shown = self.shown();
        shown
            .get(self.selected.min(shown.len().saturating_sub(1)))
            .map(|&i| &SPECS[i])
    }

    /// Moves the choice by `by` settings, kept in the list.
    pub fn move_by(&mut self, by: isize) {
        let last = self.shown().len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(by).min(last);
    }

    /// Chooses the last setting.
    pub fn last(&mut self) {
        self.selected = self.shown().len().saturating_sub(1);
    }

    /// Chooses the setting `key`, if shown.
    pub fn choose(&mut self, key: &str) {
        if let Some(i) = self.shown().iter().position(|&i| SPECS[i].key == key) {
            self.selected = i;
        }
    }

    /// Sets the filter; the choice goes back to the first setting.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(key: &str) -> &'static Spec {
        SPECS.iter().find(|s| s.key == key).unwrap()
    }

    #[test]
    fn every_setting_is_listed_once_under_its_table() {
        let all = lines("");
        let shown: Vec<usize> = all
            .iter()
            .filter_map(|l| match l {
                Line::Setting(i) => Some(*i),
                Line::Section(_) => None,
            })
            .collect();
        let mut sorted = shown.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..SPECS.len()).collect::<Vec<_>>());
        // Each table once, its settings right after it.
        let mut table = "";
        let mut seen = Vec::new();
        for l in &all {
            match l {
                Line::Section(s) => {
                    assert!(!seen.contains(s), "{s} twice");
                    seen.push(*s);
                    table = s;
                }
                Line::Setting(i) => assert_eq!(section(SPECS[*i].key), table),
            }
        }
        assert_eq!(seen[0], "editor");
    }

    #[test]
    fn the_filter_looks_at_keys_and_descriptions() {
        let found = |f: &str| -> Vec<&str> {
            lines(f)
                .into_iter()
                .filter_map(|l| match l {
                    Line::Setting(i) => Some(SPECS[i].key),
                    Line::Section(_) => None,
                })
                .collect()
        };
        assert_eq!(found("AUTO_ADD"), vec!["projects.auto_add"]);
        assert!(found("version control").contains(&"projects.auto_add"));
        assert!(lines("no such setting at all").is_empty());
    }

    #[test]
    fn values_step_in_place() {
        let config = Config::default();
        let auto = spec("projects.auto_add");
        assert_eq!(edit(auto), Edit::Step);
        assert_eq!(step(&config, auto, true), Some(Value::Bool(true)));
        assert_eq!(shown(&config, auto), tr("settings-off"));
        assert!(!changed(&config, auto));
        // Choices go round, both ways.
        let theme = spec("editor.theme");
        assert_eq!(step(&config, theme, true), Some(Value::from("light")));
        assert_eq!(step(&config, theme, false), Some(Value::from("dark")));
        // Numbers by a step suiting their range, kept in it.
        let size = spec("editor.font_size");
        assert_eq!(step(&config, size, true), Some(Value::from(17)));
        let delay = spec("keys.hints_delay");
        assert_eq!(step(&config, delay, false), Some(Value::from(300)));
        let width = spec("editor.line_width");
        assert_eq!(step(&config, width, false), None, "0 is its least");
        // Text is typed, lists edited in the file.
        assert_eq!(edit(spec("latex.engine")), Edit::Type);
        assert_eq!(step(&config, spec("latex.engine"), true), None);
        assert_eq!(edit(spec("plugins.sources")), Edit::Items);
        assert_eq!(
            shown(&config, spec("plugins.sources")),
            tr("settings-empty")
        );
        assert_eq!(shown(&config, spec("org.todo_keywords")), "TODO, |, DONE");
    }

    #[test]
    fn changed_values_are_marked() {
        let config = Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[projects]\nauto_add = true\n",
        )]);
        assert!(changed(&config, spec("projects.auto_add")));
        assert_eq!(shown(&config, spec("projects.auto_add")), tr("settings-on"));
        assert_eq!(shown_default(spec("projects.auto_add")), tr("settings-off"));
    }

    #[test]
    fn items_of_lists_and_tables_change_one_by_one() {
        let config = Config::default();
        // A list of choices: each one in or out, in the choices' order.
        let modes = spec("editor.vim.modes");
        assert!(matches!(items_kind(modes), Some(Items::Choices(_))));
        let all = items(&config, modes);
        assert!(all.iter().all(|i| i.on == Some(false)));
        let csv = all.iter().position(|i| i.text == "csv").unwrap();
        let org = all.iter().position(|i| i.text == "org").unwrap();
        let v = toggle(&config, modes, csv).unwrap();
        let config = with("editor.vim.modes", &v);
        let v = toggle(&config, modes, org).unwrap();
        assert_eq!(v, serde_json::json!(["org", "csv"]));
        let config = with("editor.vim.modes", &v);
        assert_eq!(
            toggle(&config, modes, org),
            Some(serde_json::json!(["csv"]))
        );
        // A list of texts: added, edited, moved, removed.
        let todo = spec("org.todo_keywords");
        assert_eq!(items(&config, todo).len(), 3);
        let v = put(&config, todo, None, " WAIT ").unwrap();
        let config = with("org.todo_keywords", &v);
        assert_eq!(texts(&config, todo), ["TODO", "|", "DONE", "WAIT"]);
        let (v, to) = shift(&config, todo, 3, true).unwrap();
        assert_eq!(to, 2);
        let config = with("org.todo_keywords", &v);
        assert_eq!(texts(&config, todo), ["TODO", "|", "WAIT", "DONE"]);
        assert!(shift(&config, todo, 0, true).is_none());
        let v = put(&config, todo, Some(2), "NEXT").unwrap();
        let config = with("org.todo_keywords", &v);
        let v = remove(&config, todo, 1).unwrap();
        assert_eq!(v, serde_json::json!(["TODO", "NEXT", "DONE"]));
        assert!(put(&config, todo, None, "  ").is_err());
        // A table: `path = mode` typed, its mode stepped, an entry gone.
        let files = spec("files.modes");
        assert!(put(&config, files, None, "notes.txt").is_err());
        assert!(put(&config, files, None, " = markdown").is_err());
        let v = put(&config, files, None, "notes.txt = markdown").unwrap();
        let config = with("files.modes", &v);
        assert_eq!(items(&config, files)[0].text, "notes.txt = markdown");
        let v = cycle(&config, files, 0, true).unwrap();
        assert_ne!(v["notes.txt"], "markdown");
        let v = put(&config, files, Some(0), "b.rs = rust").unwrap();
        assert_eq!(v, serde_json::json!({"b.rs": "rust"}));
        let config = with("files.modes", &v);
        assert_eq!(remove(&config, files, 0), Some(serde_json::json!({})));
        // A list of choices has no texts to type.
        assert!(put(&config, modes, None, "org").is_err());
    }

    /// The settings with only `key` set by the user, to `value`.
    fn with(key: &str, value: &Value) -> Config {
        let parts: Vec<&str> = key.split('.').collect();
        let text = crate::settings::set_in_toml("", &parts, value).unwrap();
        Config::from_layers(&[(crate::settings::Layer::User, None, &text)])
    }

    #[test]
    fn browsing() {
        let mut b = Browser::default();
        assert_eq!(b.current().map(|s| s.key), Some(SPECS[0].key));
        b.move_by(-3);
        assert_eq!(b.selected, 0);
        b.last();
        assert_eq!(b.selected, SPECS.len() - 1);
        b.move_by(5);
        assert_eq!(b.selected, SPECS.len() - 1);
        b.set_filter("projects");
        assert_eq!(b.selected, 0);
        b.choose("projects.auto_add");
        assert_eq!(b.current().map(|s| s.key), Some("projects.auto_add"));
        b.set_filter("nothing like this");
        assert!(b.current().is_none());
    }
}
