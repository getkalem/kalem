//! Every setting as a list to browse and change, as the terminal
//! editor's settings panel shows it (lazygit's way: a framed list and a
//! key for each change). The settings are grouped by their table
//! (`editor`, `ui`, `projects`…), filtered by typed text, and changed in
//! place: a switch flipped, the next choice, a number up or down. Text
//! is typed; lists and tables are edited in `settings.toml`.

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
    /// In the settings file: lists and tables.
    File,
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
        Kind::List(_) | Kind::Modes(_) => Edit::File,
    }
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
}

impl Browser {
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
        assert_eq!(edit(spec("plugins.sources")), Edit::File);
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
