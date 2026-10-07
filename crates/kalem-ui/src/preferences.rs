//! The settings panel (T1.5.19), lazygit's way as the terminal editor's:
//! every setting in a list grouped by its table, the chosen one's
//! description below, a key for each change (`j` and `k` choose, `h` and
//! `l` change in place, a text stepping through its usual values, Enter
//! edits or opens, `/` filters, `d` goes back to the default), a list's
//! or a table's items in the same panel, and the installed plugins with
//! each one's settings as pages of their own
//! ([`kalem_core::settings_list`]). A change is written to the user's
//! `settings.toml` (comments kept) and applies to every window at once.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, InteractiveElement, IntoElement, Keystroke, ParentElement, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use kalem_core::l10n::tr;
use kalem_core::line_edit;
use kalem_core::settings::{self, Config, Layer};
use kalem_core::settings_list::{self, Browser, Edit, Entry, Field, FieldKind, Row};
use serde_json::Value;

use crate::editor::{Editor, Shared};
use crate::theme::Theme;
use crate::workspace::Workspace;

/// How many lines of settings (or items) the panel shows at once.
const ROOM: usize = 16;

/// How many usual values the panel offers under a text being typed.
const OFFERED: usize = 8;

/// Text being typed in the settings panel.
#[derive(Debug, Clone)]
pub struct Typing {
    /// The text.
    pub text: String,
    /// The cursor, as characters after it ([`line_edit`]).
    pub back: usize,
    /// The setting it sets.
    pub field: Field,
    /// With its items shown: item `at` typed, or a new one; else its
    /// value.
    pub at: Option<Option<usize>>,
    /// The usual value offered chosen with the arrows.
    chosen: Option<usize>,
}

impl Typing {
    /// Whether item `n` of `field` is typed.
    fn types_item(&self, field: &Field, n: Option<usize>) -> bool {
        self.field.key == field.key && self.at == Some(n)
    }
}

/// The open settings panel.
#[derive(Debug, Clone, Default)]
pub struct SettingsPanel {
    /// The list as browsed.
    pub list: Browser,
    /// A value or an item being typed.
    pub typing: Option<Typing>,
}

impl SettingsPanel {
    /// The usual values of the text typed holding what is typed (in any
    /// case): the fonts installed, the TeX engines…
    fn offered(&self) -> Vec<String> {
        let Some(t) = self.typing.as_ref().filter(|t| t.at.is_none()) else {
            return Vec::new();
        };
        let typed = t.text.trim().to_lowercase();
        let mut out: Vec<String> = settings_list::candidates(&t.field)
            .into_iter()
            .filter(|c| c.to_lowercase().contains(&typed))
            .collect();
        out.truncate(OFFERED);
        out
    }
}

/// What a key does in a page.
enum Act {
    Move(isize),
    First,
    Last,
    Step(bool),
    Edit,
    Reset,
    File,
    Back,
    Close,
}

/// What a key does among a list's or a table's items.
enum ItemAct {
    Back,
    Close,
    Move(isize),
    First,
    Last,
    Toggle,
    Edit,
    Add,
    Remove,
    Shift(bool),
    Cycle(bool),
}

/// Settings, commands and keys for `config`, keeping what `old` was
/// given from outside (tests replace the clipboard reader and the
/// settings file; the keymap is read beside the latter).
pub fn rebuild(old: &Shared, config: Config) -> Shared {
    Shared {
        html_clipboard: old.html_clipboard,
        jobs: old.jobs.clone(),
        bus: old.bus.clone(),
        last: old.last.clone(),
        ..crate::shared_in(config, old.settings_path.clone())
    }
}

impl Editor {
    /// Opens the settings panel.
    pub fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) {
        self.palette = None;
        self.completion = None;
        let mut panel = SettingsPanel::default();
        // The plugins of the settings folder this editor uses.
        if let Some(dir) = self
            .shared
            .settings_path
            .as_deref()
            .and_then(std::path::Path::parent)
        {
            panel
                .list
                .set_plugins(kalem_core::plugin_settings::installed_in(dir));
        }
        kalem_core::fonts::prefetch();
        self.settings = Some(panel);
        cx.notify();
    }

    /// Typed text for the settings panel: the value or item typed, or the
    /// filter. Taken while the panel is open, so that none reaches the
    /// document.
    pub fn settings_input(&mut self, text: &str, cx: &mut Context<'_, Self>) -> bool {
        let Some(s) = &mut self.settings else {
            return false;
        };
        if let Some(t) = &mut s.typing {
            line_edit::insert(&mut t.text, t.back, text);
            t.chosen = None;
        } else if s.list.filtering {
            let f = format!("{}{text}", s.list.filter);
            s.list.set_filter(&f);
        }
        cx.notify();
        true
    }

    /// Keys for the settings panel; `true` if used.
    pub fn settings_key(
        &mut self,
        k: &Keystroke,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(s) = &mut self.settings else {
            return false;
        };
        let m = k.modifiers;
        let plain = !m.control && !m.alt && !m.platform && !m.function;
        // A letter's key as typed (`G` with Shift), for the panel's own keys.
        let typed = k.key_char.as_deref().filter(|_| plain);
        let special = matches!(
            k.key.as_str(),
            "escape"
                | "enter"
                | "backspace"
                | "delete"
                | "left"
                | "right"
                | "up"
                | "down"
                | "home"
                | "end"
                | "tab"
                | "pageup"
                | "pagedown"
        );
        if s.typing.is_some() {
            return self.settings_typing_key(k, plain, special, cx);
        }
        let shared = self.shared.clone();
        let config = &shared.config;
        if s.list.filtering {
            match k.key.as_str() {
                "escape" => {
                    s.list.set_filter("");
                    s.list.filtering = false;
                }
                "enter" => s.list.filtering = false,
                "backspace" => {
                    let mut f = s.list.filter.clone();
                    if f.pop().is_none() {
                        s.list.filtering = false;
                    }
                    s.list.set_filter(&f);
                }
                "up" => s.list.move_by(-1, config),
                "down" => s.list.move_by(1, config),
                // A viewer's file takes no typed text: the keys give it.
                _ if plain && !special && self.doc.viewer.is_some() => {
                    let Some(text) = k.key_char.clone() else {
                        return false;
                    };
                    return self.settings_input(&text, cx);
                }
                _ => return false,
            }
            cx.notify();
            return true;
        }
        if s.list.item.is_some() {
            return self.settings_item_key(k, typed, cx);
        }
        let ctrl = m.control;
        let act = match (k.key.as_str(), typed) {
            ("escape", _) if !s.list.filter.is_empty() => {
                s.list.set_filter("");
                cx.notify();
                return true;
            }
            (_, Some("/")) => {
                s.list.filtering = true;
                cx.notify();
                return true;
            }
            ("escape", _) => Act::Back,
            (_, Some("q")) => Act::Close,
            ("g", _) if ctrl => Act::Close,
            ("p", _) if ctrl => Act::Move(-1),
            ("n", _) if ctrl => Act::Move(1),
            ("u", _) if ctrl => Act::Move(-10),
            ("d", _) if ctrl => Act::Move(10),
            ("up", _) | (_, Some("k")) => Act::Move(-1),
            ("down", _) | (_, Some("j")) => Act::Move(1),
            ("pageup", _) => Act::Move(-10),
            ("pagedown", _) => Act::Move(10),
            ("home", _) | (_, Some("g")) => Act::First,
            ("end", _) | (_, Some("G")) => Act::Last,
            ("right", _) | (_, Some("l" | " ")) => Act::Step(true),
            ("left", _) | (_, Some("h")) => Act::Step(false),
            ("enter", _) => Act::Edit,
            (_, Some("d")) => Act::Reset,
            (_, Some("e")) => Act::File,
            // Other letters do nothing here (and type nothing).
            _ if plain && !special => return true,
            _ => return false,
        };
        match act {
            Act::Move(by) => s.list.move_by(by, config),
            Act::First => s.list.selected = 0,
            Act::Last => s.list.last(config),
            Act::Step(forward) => self.settings_step(forward, window, cx),
            Act::Edit => self.settings_edit(window, cx),
            Act::Reset => {
                if let Some(f) = s.list.current_field(config) {
                    self.set_field(&f, Value::Null, cx);
                }
            }
            Act::File => self.settings_file(cx),
            Act::Back => {
                if !s.list.back() {
                    self.settings = None;
                }
            }
            Act::Close => self.settings = None,
        }
        cx.notify();
        true
    }

    /// Keys while a value or an item is typed: Enter sets it, Escape
    /// leaves it, Tab completes to a usual value, the arrows choose among
    /// those offered; the rest edits the line.
    fn settings_typing_key(
        &mut self,
        k: &Keystroke,
        plain: bool,
        special: bool,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(s) = &mut self.settings else {
            return false;
        };
        let offered = s.offered();
        let Some(t) = &mut s.typing else {
            return false;
        };
        let m = k.modifiers;
        let word = if cfg!(target_os = "macos") {
            m.alt
        } else {
            m.control
        };
        let line = cfg!(target_os = "macos") && m.platform;
        match k.key.as_str() {
            "escape" => s.typing = None,
            "enter" => self.settings_commit(cx),
            "tab" if !offered.is_empty() => {
                let pick = t.chosen.unwrap_or(0).min(offered.len() - 1);
                t.text = offered[pick].clone();
                t.back = 0;
                t.chosen = None;
            }
            "down" if !offered.is_empty() => {
                t.chosen = Some(t.chosen.map_or(0, |c| (c + 1).min(offered.len() - 1)));
            }
            "up" if !offered.is_empty() => {
                t.chosen = t.chosen.and_then(|c| c.checked_sub(1));
            }
            name => match line_edit::from_key(name, word, line) {
                Some(key) => {
                    line_edit::apply(&mut t.text, &mut t.back, key);
                    t.chosen = None;
                }
                // A viewer's file takes no typed text: the keys give it.
                None if plain && !special && self.doc.viewer.is_some() => {
                    let Some(text) = k.key_char.clone() else {
                        return false;
                    };
                    return self.settings_input(&text, cx);
                }
                None => return false,
            },
        }
        cx.notify();
        true
    }

    /// Keys among a list's or a table's items, as the terminal editor's:
    /// `j` and `k` choose, Space puts a choice in or out, Enter edits an
    /// item and `a` adds one, `x` removes it, `J` and `K` move a text,
    /// `h` and `l` step an entry's mode, Escape goes back.
    fn settings_item_key(
        &mut self,
        k: &Keystroke,
        typed: Option<&str>,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let shared = self.shared.clone();
        let config = &shared.config;
        let Some(s) = &mut self.settings else {
            return false;
        };
        let Some(field) = s.list.current_field(config) else {
            s.list.item = None;
            return true;
        };
        let count = settings_list::items(config, &field).len();
        let i = s.list.item.unwrap_or(0).min(count.saturating_sub(1));
        let texts = matches!(field.kind, FieldKind::Texts | FieldKind::Table(_));
        let choices = matches!(field.kind, FieldKind::Choices(_));
        let table = matches!(field.kind, FieldKind::Table(_));
        let act = match (k.key.as_str(), typed) {
            ("escape" | "backspace", _) => ItemAct::Back,
            (_, Some("q")) => ItemAct::Close,
            ("up", _) | (_, Some("k")) => ItemAct::Move(-1),
            ("down", _) | (_, Some("j")) => ItemAct::Move(1),
            ("home", _) | (_, Some("g")) => ItemAct::First,
            ("end", _) | (_, Some("G")) => ItemAct::Last,
            ("enter" | "left" | "right", _) | (_, Some(" " | "h" | "l")) if choices => {
                ItemAct::Toggle
            }
            ("enter", _) if count > 0 => ItemAct::Edit,
            ("enter", _) | (_, Some("a" | "o")) if texts => ItemAct::Add,
            ("delete", _) | (_, Some("x" | "d")) if texts => ItemAct::Remove,
            (_, Some("K")) if field.kind == FieldKind::Texts => ItemAct::Shift(true),
            (_, Some("J")) if field.kind == FieldKind::Texts => ItemAct::Shift(false),
            ("right", _) | (_, Some(" " | "l")) if table => ItemAct::Cycle(true),
            ("left", _) | (_, Some("h")) if table => ItemAct::Cycle(false),
            (_, Some(_)) => return true,
            _ => return false,
        };
        let value = match act {
            ItemAct::Back => {
                s.list.item = None;
                None
            }
            ItemAct::Close => {
                self.settings = None;
                None
            }
            ItemAct::Move(by) => {
                s.list.move_item(by, count);
                None
            }
            ItemAct::First => {
                s.list.item = Some(0);
                None
            }
            ItemAct::Last => {
                s.list.item = Some(count.saturating_sub(1));
                None
            }
            ItemAct::Toggle => settings_list::toggle(config, &field, i),
            ItemAct::Edit => {
                let text = settings_list::items(config, &field)
                    .get(i)
                    .map(|it| it.text.clone())
                    .unwrap_or_default();
                s.typing = Some(Typing {
                    text,
                    back: 0,
                    field: field.clone(),
                    at: Some(Some(i)),
                    chosen: None,
                });
                None
            }
            ItemAct::Add => {
                s.typing = Some(Typing {
                    text: String::new(),
                    back: 0,
                    field: field.clone(),
                    at: Some(None),
                    chosen: None,
                });
                None
            }
            ItemAct::Remove => settings_list::remove(config, &field, i),
            ItemAct::Shift(up) => settings_list::shift(config, &field, i, up).map(|(v, to)| {
                s.list.item = Some(to);
                v
            }),
            ItemAct::Cycle(forward) => settings_list::cycle(config, &field, i, forward),
        };
        if let Some(v) = value {
            self.set_field(&field, v, cx);
        }
        cx.notify();
        true
    }

    /// The chosen entry a step forward or back: a switch flipped, the
    /// next choice or usual text, a number up or down; a page or a list's
    /// items opened instead, and a text without usual values typed.
    fn settings_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        let shared = self.shared.clone();
        let Some(s) = &self.settings else {
            return;
        };
        match s.list.current(&shared.config) {
            Some(Entry::Field(f)) => match settings_list::step(&shared.config, &f, forward) {
                Some(v) => self.set_field(&f, v, cx),
                None if forward && settings_list::edit(&f) != Edit::Step => {
                    self.settings_edit(window, cx);
                }
                None => {}
            },
            Some(Entry::Plugins(_) | Entry::Plugin(_)) if forward => {
                self.settings_edit(window, cx);
            }
            _ => {}
        }
    }

    /// Edits or opens the chosen entry: a setting stepped forward, its
    /// text typed (the current text offered) or its items shown; the
    /// installed plugins' or a plugin's page; an action run.
    fn settings_edit(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let shared = self.shared.clone();
        let config = &shared.config;
        let Some(s) = &mut self.settings else {
            return;
        };
        let Some(entry) = s.list.current(config) else {
            return;
        };
        match entry {
            Entry::Field(f) => match settings_list::edit(&f) {
                Edit::Step => self.settings_step(true, window, cx),
                Edit::Type => {
                    let text = match settings_list::value(config, &f) {
                        Value::String(t) => t,
                        Value::Null => String::new(),
                        v => v.to_string(),
                    };
                    s.typing = Some(Typing {
                        text,
                        back: 0,
                        field: f,
                        at: None,
                        chosen: None,
                    });
                }
                Edit::Items => {
                    s.list.open(config);
                }
            },
            Entry::Plugins(_) | Entry::Plugin(_) => {
                s.list.open(config);
            }
            Entry::Action { command, args, .. } => {
                self.settings = None;
                self.run_command(&command, args, window, cx);
            }
        }
        cx.notify();
    }

    /// Sets what was typed: a setting's text (a usual value chosen with
    /// the arrows taken instead), or a list's or a table's item.
    fn settings_commit(&mut self, cx: &mut Context<'_, Self>) {
        let Some(s) = &mut self.settings else {
            return;
        };
        let chosen = s
            .typing
            .as_ref()
            .and_then(|t| t.chosen)
            .and_then(|c| s.offered().get(c).cloned());
        let Some(t) = s.typing.take() else {
            return;
        };
        let value = match t.at {
            Some(at) => settings_list::put(&self.shared.config, &t.field, at, &t.text),
            None => settings_list::typed(&t.field, chosen.as_deref().unwrap_or(&t.text)),
        };
        match value {
            Ok(v) => self.set_field(&t.field, v, cx),
            Err(e) => {
                self.status = Some((e, true));
                if let Some(s) = &mut self.settings {
                    s.typing = Some(t);
                }
            }
        }
        cx.notify();
    }

    /// Closes the settings panel and opens the user's `settings.toml`.
    fn settings_file(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.shared.settings_path.clone() else {
            self.status = Some((tr("msg-no-settings-dir"), true));
            return;
        };
        self.settings = None;
        cx.emit(crate::editor::DocEvent::Open { path, at: None });
    }

    /// A click on entry `n` (among those shown): chosen, or edited or
    /// opened when chosen already.
    fn settings_click(&mut self, n: usize, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(s) = &mut self.settings else {
            return;
        };
        s.typing = None;
        if s.list.selected == n {
            self.settings_edit(window, cx);
        } else {
            s.list.selected = n;
        }
        cx.notify();
    }

    /// A click on item `n` of the setting shown: chosen, or changed as
    /// Enter changes it when chosen already.
    fn settings_item_click(&mut self, n: usize, cx: &mut Context<'_, Self>) {
        let shared = self.shared.clone();
        let Some(s) = &mut self.settings else {
            return;
        };
        // The item being typed stays so.
        if let (Some(t), Some(f)) = (&s.typing, s.list.current_field(&shared.config))
            && t.types_item(&f, Some(n))
        {
            return;
        }
        if s.list.item == Some(n) && s.typing.is_none() {
            if let Ok(enter) = Keystroke::parse("enter") {
                self.settings_item_key(&enter, None, cx);
            }
        } else {
            s.list.item = Some(n);
            s.typing = None;
        }
        cx.notify();
    }

    /// Saves `key` in the user's settings and applies the settings to
    /// every window.
    pub fn set_setting(&mut self, key: &str, value: Value, cx: &mut Context<'_, Self>) {
        match settings::spec(key) {
            Some(spec) => self.set_field(&Field::of(spec), value, cx),
            None => self.status = Some((format!("Unknown setting `{key}`"), true)),
        }
    }

    /// [`Editor::set_setting`] for a setting of the settings panel: one
    /// of Kalem's, or a plugin's under `[plugins."ID"]` (`null` takes it
    /// out, back to its default).
    pub fn set_field(&mut self, field: &Field, value: Value, cx: &mut Context<'_, Self>) {
        let Some(path) = self.shared.settings_path.clone() else {
            self.status = Some((tr("msg-no-settings-dir"), true));
            return;
        };
        if let Err(e) = settings_list::save(&path, field, &value) {
            self.status = Some((e, true));
            return;
        }
        let workspace = self
            .shared
            .config
            .sources()
            .iter()
            .find(|(l, _)| *l == Layer::Workspace)
            .and_then(|(_, p)| p.clone());
        let config = Config::load(Some(&path), workspace.as_deref());
        let key = field.key.as_str();
        let message = if !value.is_null() && settings_list::value(&config, field) != value {
            (kalem_core::tr!("msg-setting-overridden", key = key), true)
        } else if value.is_null() {
            (kalem_core::tr!("msg-setting-reset", key = key), false)
        } else {
            (kalem_core::tr!("msg-setting-saved", key = key), false)
        };
        let shared = Rc::new(rebuild(&self.shared, config));
        self.status = Some(message);
        // After this update: every window, this one too.
        cx.defer(move |cx| apply(shared, cx));
    }

    /// Reads the settings and the user's keymap again, in every window
    /// (`SPC h r r`).
    pub fn reload_settings(&mut self, cx: &mut Context<'_, Self>) {
        let workspace = self
            .shared
            .config
            .sources()
            .iter()
            .find(|(l, _)| *l == Layer::Workspace)
            .and_then(|(_, p)| p.clone());
        let path = self.shared.settings_path.clone();
        let config = Config::load(path.as_deref(), workspace.as_deref());
        let shared = Rc::new(rebuild(&self.shared, config));
        self.status = Some((tr("msg-reloaded-settings"), false));
        cx.defer(move |cx| apply(shared, cx));
    }

    /// The settings panel, when open: the page's entries (or the chosen
    /// setting's items) around the choice, the usual values offered while
    /// a text is typed, then the chosen entry's heading and description,
    /// and the keys.
    pub fn settings_view(&self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let s = self.settings.as_ref()?;
        let theme = &self.theme;
        let config = &self.shared.config;
        let list = &s.list;
        let rows = list.rows(config);
        let count = rows.iter().filter(|r| matches!(r, Row::Entry(_))).count();
        let selected = list.selected.min(count.saturating_sub(1));
        let current = list.current(config);
        let items_of = list
            .item
            .and(current.as_ref())
            .and_then(Entry::field)
            .filter(|f| settings_list::has_items(f));
        let size = theme.size.min(18.) * 0.85;
        let row = |id: SharedString, on: bool| {
            div()
                .id(id.clone())
                .debug_selector(move || id.to_string())
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .px(px(10.))
                .h(px(size * 1.7))
                .rounded(px(4.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme.selection))
        };
        let heading = |text: String| {
            div()
                .px(px(4.))
                .pt(px(4.))
                .text_color(theme.link)
                .font_weight(gpui::FontWeight::BOLD)
                .child(text)
        };
        let input = |t: &Typing| {
            let (before, after) = line_edit::split(&t.text, t.back);
            div()
                .debug_selector(|| "settings-input".into())
                .flex_1()
                .px(px(6.))
                .rounded(px(3.))
                .border_1()
                .border_color(theme.link)
                .bg(theme.background)
                .text_color(theme.foreground)
                .whitespace_nowrap()
                .child(SharedString::from(format!("{before}▏{after}")))
        };
        let empty = || {
            div()
                .px(px(10.))
                .text_color(theme.muted)
                .child(tr("settings-empty"))
                .into_any_element()
        };
        let mut body: Vec<gpui::AnyElement> = Vec::new();
        if let Some(field) = items_of {
            body.push(heading(field.key.clone()).into_any_element());
            let items = settings_list::items(config, field);
            let chosen = list.item.unwrap_or(0).min(items.len().saturating_sub(1));
            let first = (chosen + 1).saturating_sub(ROOM);
            let editing = s.typing.as_ref().filter(|t| t.at.is_some());
            if items.is_empty() && editing.is_none() {
                body.push(empty());
            }
            for (n, item) in items.iter().enumerate().skip(first).take(ROOM) {
                let id = SharedString::from(format!("settings-item-{n}"));
                let r = row(id, n == chosen);
                let r = match editing {
                    Some(t) if t.types_item(field, Some(n)) => r.child(input(t)),
                    _ => {
                        let text = match item.on {
                            Some(true) => format!("☑ {}", item.text),
                            Some(false) => format!("☐ {}", item.text),
                            None => item.text.clone(),
                        };
                        r.text_color(if item.on == Some(true) {
                            theme.link
                        } else {
                            theme.foreground
                        })
                        .child(div().truncate().child(SharedString::from(text)))
                    }
                };
                body.push(
                    r.on_click(cx.listener(move |this, _, _, cx| this.settings_item_click(n, cx)))
                        .into_any_element(),
                );
            }
            if let Some(t) = editing.filter(|t| t.types_item(field, None)) {
                body.push(
                    row("settings-item-new".into(), true)
                        .child(div().text_color(theme.muted).child(tr("settings-item-new")))
                        .child(input(t))
                        .into_any_element(),
                );
            }
        } else {
            if list.filtering || !list.filter.is_empty() {
                let caret = if list.filtering { "▏" } else { "" };
                body.push(
                    div()
                        .debug_selector(|| "settings-filter".into())
                        .px(px(4.))
                        .text_color(theme.link)
                        .child(SharedString::from(format!("/ {}{caret}", list.filter)))
                        .into_any_element(),
                );
            }
            if rows.is_empty() {
                body.push(empty());
            }
            // The line of the chosen entry, and the first line shown.
            let mut n = 0;
            let at = rows
                .iter()
                .position(|r| {
                    let entry = matches!(r, Row::Entry(_));
                    let found = entry && n == selected;
                    n += usize::from(entry);
                    found
                })
                .unwrap_or(0);
            let first = (at + 1).saturating_sub(ROOM);
            let mut n = rows[..first]
                .iter()
                .filter(|r| matches!(r, Row::Entry(_)))
                .count();
            for r in rows.iter().skip(first).take(ROOM) {
                let e = match r {
                    Row::Heading(name) => {
                        body.push(heading(name.clone()).into_any_element());
                        continue;
                    }
                    Row::Entry(e) => e,
                };
                let id = match e {
                    Entry::Field(f) => format!("settings-row-{}", f.key),
                    Entry::Plugins(_) => "settings-plugins".to_string(),
                    Entry::Plugin(p) => format!("settings-plugin-{}", p.id),
                    Entry::Action { command, .. } => format!("settings-action-{command}"),
                };
                let r = row(SharedString::from(id), n == selected).child(
                    div()
                        .w(px(size * 15.))
                        .flex_none()
                        .truncate()
                        .text_color(theme.foreground)
                        .child(SharedString::from(e.name())),
                );
                let typing = s
                    .typing
                    .as_ref()
                    .filter(|t| t.at.is_none() && e.field().is_some_and(|f| f.key == t.field.key));
                let r = match typing {
                    Some(t) => r.child(input(t)),
                    None => {
                        let changed = e.changed(config);
                        r.child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(if changed {
                                    theme.link
                                } else {
                                    theme.foreground
                                })
                                .when(changed, |d| d.font_weight(gpui::FontWeight::BOLD))
                                .child(SharedString::from(e.value(config))),
                        )
                    }
                };
                let at = n;
                body.push(
                    r.on_click(
                        cx.listener(move |this, _, window, cx| this.settings_click(at, window, cx)),
                    )
                    .into_any_element(),
                );
                n += 1;
            }
        }
        // The usual values offered while a text is typed.
        let chosen = s.typing.as_ref().and_then(|t| t.chosen);
        let offered = s.offered().into_iter().enumerate().map(|(i, v)| {
            let label = match (&s.typing, v.is_empty()) {
                (Some(t), true) if t.field.key.ends_with("font_family") => {
                    tr("settings-system-font")
                }
                (_, true) => tr("settings-empty"),
                _ => v.clone(),
            };
            div()
                .id(("settings-offered", i))
                .debug_selector(move || format!("settings-offered-{i}"))
                .px(px(10.))
                .rounded(px(3.))
                .cursor_pointer()
                .when(chosen == Some(i), |d| d.bg(theme.selection))
                .child(SharedString::from(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let Some(t) = this.settings.as_mut().and_then(|s| s.typing.take()) else {
                        return;
                    };
                    this.set_field(&t.field, Value::from(v.clone()), cx);
                }))
        });
        // The chosen entry: its heading and description.
        let about = current.as_ref().map(|e| {
            let (head, about) = e.about(config);
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pt(px(6.))
                .border_t_1()
                .border_color(theme.border)
                .child(div().text_color(theme.link).child(SharedString::from(head)))
                .child(
                    div()
                        .text_color(theme.foreground)
                        .child(SharedString::from(about)),
                )
        });
        let hints = match items_of.map(|f| &f.kind) {
            Some(FieldKind::Choices(_)) => tr("settings-items-choices"),
            Some(FieldKind::Texts) => tr("settings-items-texts"),
            Some(FieldKind::Table(_)) => tr("settings-items-table"),
            _ => tr("settings-hints"),
        };
        let count = format!("{}/{}", (selected + 1).min(count), count);
        let panel = div()
            .id("settings")
            .debug_selector(|| "settings".into())
            .occlude()
            .w(px(680.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.bar)
            .text_size(px(size))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(
                        div()
                            .text_color(theme.foreground)
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(tr("settings-title")),
                    )
                    .child(
                        div()
                            .text_color(theme.muted)
                            .child(SharedString::from(count)),
                    ),
            )
            .child(
                div()
                    .id("settings-list")
                    .flex()
                    .flex_col()
                    .min_h(px(size * 1.7 * ROOM as f32))
                    .children(body)
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                        let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                        if dy == 0. {
                            return;
                        }
                        let by = if dy < 0. { 1 } else { -1 };
                        let shared = this.shared.clone();
                        let Some(s) = &mut this.settings else {
                            return;
                        };
                        match (s.list.item, s.list.current_field(&shared.config)) {
                            (Some(_), Some(f)) => {
                                let n = settings_list::items(&shared.config, &f).len();
                                s.list.move_item(by, n);
                            }
                            _ => s.list.move_by(by, &shared.config),
                        }
                        cx.notify();
                        cx.stop_propagation();
                    })),
            )
            .child(div().flex().flex_col().children(offered))
            .children(about)
            .child(
                div()
                    .pt(px(4.))
                    .text_color(theme.muted)
                    .child(SharedString::from(hints)),
            );
        Some(
            div()
                .absolute()
                .top(px(8.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(panel)
                .into_any_element(),
        )
    }
}

/// Gives every window the settings of `shared`: the language, commands
/// and keys, and the theme; the menus follow.
pub fn apply(shared: Rc<Shared>, cx: &mut gpui::App) {
    shared.config.apply_process_settings();
    cx.set_menus(crate::workspace::menus());
    crate::workspace::refresh_menus();
    cx.clear_key_bindings();
    cx.bind_keys(crate::workspace::menu_bindings(&shared));
    for w in cx.windows() {
        let Some(w) = w.downcast::<Workspace>() else {
            continue;
        };
        let shared = shared.clone();
        let _ = w.update(cx, |ws, window, cx| {
            let theme = Theme::from_config(&shared.config, window.appearance());
            ws.shared = shared.clone();
            ws.files_at =
                crate::workspace::FilesAt::from_setting(shared.config.str("ui.open_files"));
            ws.files_shown = ws.files_at != crate::workspace::FilesAt::Hidden;
            for e in ws.editors.clone() {
                let (shared, theme) = (shared.clone(), theme.clone());
                e.update(cx, |e, cx| {
                    e.shared = shared;
                    e.vim = None;
                    e.refresh_vim();
                    e.theme = theme;
                    e.relayout();
                    cx.notify();
                });
            }
            cx.notify();
        });
    }
}
