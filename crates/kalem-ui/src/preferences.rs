//! The settings panel (T1.5.19), lazygit's way as the terminal editor's:
//! every setting in a list grouped by its table, the chosen one's
//! description below, a key for each change (`j` and `k` choose, `h` and
//! `l` change in place, Enter edits, `/` filters, `d` goes back to the
//! default), a list's or a table's items in the same panel
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
use kalem_core::settings::{self, Config, Layer, SPECS};
use kalem_core::settings_list::{self, Browser, Edit, Items, Line};
use serde_json::Value;

use crate::editor::{Editor, Shared};
use crate::theme::Theme;
use crate::workspace::Workspace;

/// How many lines of settings (or items) the panel shows at once.
const ROOM: usize = 16;

/// What text typed in the settings panel sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The text of setting `key`.
    Value(&'static str),
    /// An item of the list or table `key`: in place of item `at`, else a
    /// new one.
    Item(&'static str, Option<usize>),
}

/// Text being typed in the settings panel.
#[derive(Debug, Clone)]
pub struct Typing {
    /// The text.
    pub text: String,
    /// The cursor, as characters after it ([`line_edit`]).
    pub back: usize,
    /// What it sets.
    pub target: Target,
    /// The font offered chosen with the arrows (`editor.font_family`).
    chosen: Option<usize>,
}

/// The open settings panel.
#[derive(Debug, Clone, Default)]
pub struct SettingsPanel {
    /// The list as browsed.
    pub list: Browser,
    /// A value or an item being typed.
    pub typing: Option<Typing>,
    fonts: Vec<String>,
}

impl SettingsPanel {
    /// The fonts holding `typed` (in any case), the system font first,
    /// for `editor.font_family`.
    pub fn fonts(&self, typed: &str) -> Vec<&str> {
        let f = typed.trim().to_lowercase();
        let mut out = vec![""];
        out.extend(
            self.fonts
                .iter()
                .map(String::as_str)
                .filter(|n| n.to_lowercase().contains(&f)),
        );
        out.truncate(9);
        out
    }

    /// The fonts offered while `editor.font_family` is typed.
    fn offered(&self) -> Vec<&str> {
        match &self.typing {
            Some(t) if t.target == Target::Value("editor.font_family") => self.fonts(&t.text),
            _ => Vec::new(),
        }
    }
}

/// What a key does in the list of settings.
enum Act {
    Move(isize),
    First,
    Last,
    Step(bool),
    Edit,
    Reset,
    File,
    Close,
}

/// What a key does among a list's or a table's items.
enum ItemAct {
    Back,
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
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let mut fonts = window.text_system().all_font_names();
        fonts.sort();
        fonts.dedup();
        fonts.retain(|f| !f.starts_with('.'));
        self.palette = None;
        self.completion = None;
        self.settings = Some(SettingsPanel {
            fonts,
            ..SettingsPanel::default()
        });
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
        _window: &mut Window,
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
                "up" => s.list.move_by(-1),
                "down" => s.list.move_by(1),
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
            ("escape", _) | (_, Some("q")) if !s.list.filter.is_empty() => {
                s.list.set_filter("");
                cx.notify();
                return true;
            }
            (_, Some("/")) => {
                s.list.filtering = true;
                cx.notify();
                return true;
            }
            ("escape", _) | (_, Some("q")) => Act::Close,
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
            Act::Move(by) => s.list.move_by(by),
            Act::First => s.list.selected = 0,
            Act::Last => s.list.last(),
            Act::Step(forward) => self.settings_step(forward, cx),
            Act::Edit => self.settings_edit(cx),
            Act::Reset => {
                if let Some(spec) = s.list.current() {
                    self.set_setting(spec.key, Value::Null, cx);
                }
            }
            Act::File => self.settings_file(cx),
            Act::Close => self.settings = None,
        }
        cx.notify();
        true
    }

    /// Keys while a value or an item is typed: Enter sets it, Escape
    /// leaves it, Tab completes a font, the arrows choose among the fonts
    /// offered; the rest edits the line.
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
        let offered: Vec<String> = s.offered().into_iter().map(str::to_string).collect();
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
                let pick = t.chosen.unwrap_or(1).min(offered.len() - 1);
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
        let Some(s) = &mut self.settings else {
            return false;
        };
        let Some((spec, kind)) = s
            .list
            .current()
            .and_then(|spec| Some((spec, settings_list::items_kind(spec)?)))
        else {
            s.list.item = None;
            return true;
        };
        let shared = self.shared.clone();
        let config = &shared.config;
        let count = settings_list::items(config, spec).len();
        let i = s.list.item.unwrap_or(0).min(count.saturating_sub(1));
        let texts = matches!(kind, Items::Texts | Items::Table(_));
        let act = match (k.key.as_str(), typed, kind) {
            ("escape" | "backspace", ..) | (_, Some("q"), _) => ItemAct::Back,
            ("up", ..) | (_, Some("k"), _) => ItemAct::Move(-1),
            ("down", ..) | (_, Some("j"), _) => ItemAct::Move(1),
            ("home", ..) | (_, Some("g"), _) => ItemAct::First,
            ("end", ..) | (_, Some("G"), _) => ItemAct::Last,
            ("enter" | "left" | "right", _, Items::Choices(_))
            | (_, Some(" " | "h" | "l"), Items::Choices(_)) => ItemAct::Toggle,
            ("enter", ..) if count > 0 => ItemAct::Edit,
            ("enter", ..) | (_, Some("a" | "o"), _) if texts => ItemAct::Add,
            ("delete", ..) | (_, Some("x" | "d"), _) if texts => ItemAct::Remove,
            (_, Some("K"), Items::Texts) => ItemAct::Shift(true),
            (_, Some("J"), Items::Texts) => ItemAct::Shift(false),
            ("right", _, Items::Table(_)) | (_, Some(" " | "l"), Items::Table(_)) => {
                ItemAct::Cycle(true)
            }
            ("left", _, Items::Table(_)) | (_, Some("h"), Items::Table(_)) => ItemAct::Cycle(false),
            (_, Some(_), _) => return true,
            _ => return false,
        };
        let value = match act {
            ItemAct::Back => {
                s.list.item = None;
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
            ItemAct::Toggle => settings_list::toggle(config, spec, i),
            ItemAct::Edit => {
                let text = settings_list::items(config, spec)
                    .get(i)
                    .map(|it| it.text.clone())
                    .unwrap_or_default();
                s.typing = Some(Typing {
                    text,
                    back: 0,
                    target: Target::Item(spec.key, Some(i)),
                    chosen: None,
                });
                None
            }
            ItemAct::Add => {
                s.typing = Some(Typing {
                    text: String::new(),
                    back: 0,
                    target: Target::Item(spec.key, None),
                    chosen: None,
                });
                None
            }
            ItemAct::Remove => settings_list::remove(config, spec, i),
            ItemAct::Shift(up) => settings_list::shift(config, spec, i, up).map(|(v, to)| {
                s.list.item = Some(to);
                v
            }),
            ItemAct::Cycle(forward) => settings_list::cycle(config, spec, i, forward),
        };
        if let Some(v) = value {
            self.set_setting(spec.key, v, cx);
        }
        cx.notify();
        true
    }

    /// The chosen setting a step forward or back: a switch flipped, the
    /// next choice, a number up or down; text is typed and a list's items
    /// shown instead.
    fn settings_step(&mut self, forward: bool, cx: &mut Context<'_, Self>) {
        let Some(spec) = self.settings.as_ref().and_then(|s| s.list.current()) else {
            return;
        };
        match settings_list::edit(spec) {
            Edit::Step => {
                if let Some(v) = settings_list::step(&self.shared.config, spec, forward) {
                    self.set_setting(spec.key, v, cx);
                }
            }
            Edit::Type | Edit::Items if forward => self.settings_edit(cx),
            _ => {}
        }
    }

    /// Edits the chosen setting: stepped forward, its text typed (the
    /// current text offered), or its items shown.
    fn settings_edit(&mut self, cx: &mut Context<'_, Self>) {
        let shared = self.shared.clone();
        let Some(s) = &mut self.settings else {
            return;
        };
        let Some(spec) = s.list.current() else {
            return;
        };
        match settings_list::edit(spec) {
            Edit::Step => self.settings_step(true, cx),
            Edit::Type => {
                let text = shared.config.str(spec.key).to_string();
                s.typing = Some(Typing {
                    text,
                    back: 0,
                    target: Target::Value(spec.key),
                    chosen: None,
                });
            }
            Edit::Items => {
                s.list.open_items();
            }
        }
        cx.notify();
    }

    /// Sets what was typed: the text of a setting (a font offered chosen
    /// with the arrows taken instead), or a list's or a table's item.
    fn settings_commit(&mut self, cx: &mut Context<'_, Self>) {
        let Some(s) = &mut self.settings else {
            return;
        };
        let chosen = s
            .typing
            .as_ref()
            .and_then(|t| t.chosen)
            .and_then(|c| s.offered().get(c).map(|f| f.to_string()));
        let Some(t) = s.typing.take() else {
            return;
        };
        match t.target {
            Target::Value(key) => {
                let text = chosen.unwrap_or(t.text);
                self.set_setting(key, Value::String(text), cx);
            }
            Target::Item(key, at) => {
                let put = settings::spec(key)
                    .ok_or_else(|| format!("Unknown setting `{key}`"))
                    .and_then(|spec| settings_list::put(&self.shared.config, spec, at, &t.text));
                match put {
                    Ok(v) => self.set_setting(key, v, cx),
                    Err(e) => {
                        self.status = Some((e, true));
                        if let Some(s) = &mut self.settings {
                            s.typing = Some(t);
                        }
                    }
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

    /// A click on setting `n` (among those shown): chosen, or edited when
    /// chosen already.
    fn settings_click(&mut self, n: usize, cx: &mut Context<'_, Self>) {
        let Some(s) = &mut self.settings else {
            return;
        };
        s.typing = None;
        if s.list.selected == n {
            self.settings_edit(cx);
        } else {
            s.list.selected = n;
        }
        cx.notify();
    }

    /// A click on item `n` of the setting shown: chosen, or changed as
    /// Enter changes it when chosen already.
    fn settings_item_click(&mut self, n: usize, cx: &mut Context<'_, Self>) {
        let Some(s) = &mut self.settings else {
            return;
        };
        let key = s.list.current().map(|spec| spec.key);
        // The item being typed stays so.
        if let (Some(t), Some(key)) = (&s.typing, key)
            && t.target == Target::Item(key, Some(n))
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
        let Some(path) = self.shared.settings_path.clone() else {
            self.status = Some((tr("msg-no-settings-dir"), true));
            return;
        };
        if let Err(e) = settings::save_setting(&path, key, &value) {
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
        let shared = Rc::new(rebuild(&self.shared, config));
        self.status = Some((kalem_core::tr!("msg-setting-saved", key = key), false));
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

    /// The settings panel, when open: the list of settings (or the chosen
    /// one's items) around the choice, then the chosen setting's key,
    /// default and description, and the keys.
    pub fn settings_view(&self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let s = self.settings.as_ref()?;
        let theme = &self.theme;
        let config = &self.shared.config;
        let list = &s.list;
        let lines = list.lines();
        let shown = list.shown();
        let selected = list.selected.min(shown.len().saturating_sub(1));
        let current = shown.get(selected).map(|&i| &SPECS[i]);
        let items_of = list
            .item
            .and(current)
            .and_then(|spec| Some((spec, settings_list::items_kind(spec)?)));
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
        let mut body: Vec<gpui::AnyElement> = Vec::new();
        if let Some((spec, kind)) = items_of {
            body.push(heading(spec.key.to_string()).into_any_element());
            let items = settings_list::items(config, spec);
            let chosen = list.item.unwrap_or(0).min(items.len().saturating_sub(1));
            let first = (chosen + 1).saturating_sub(ROOM);
            let editing = s
                .typing
                .as_ref()
                .filter(|t| matches!(t.target, Target::Item(..)));
            if items.is_empty() && editing.is_none() {
                body.push(
                    div()
                        .px(px(10.))
                        .text_color(theme.muted)
                        .child(tr("settings-empty"))
                        .into_any_element(),
                );
            }
            for (n, item) in items.iter().enumerate().skip(first).take(ROOM) {
                let id = SharedString::from(format!("settings-item-{n}"));
                let r = row(id, n == chosen);
                let r = match editing {
                    Some(t) if t.target == Target::Item(spec.key, Some(n)) => r.child(input(t)),
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
            if let Some(t) = editing.filter(|t| t.target == Target::Item(spec.key, None)) {
                body.push(
                    row("settings-item-new".into(), true)
                        .child(div().text_color(theme.muted).child(tr("settings-item-new")))
                        .child(input(t))
                        .into_any_element(),
                );
            }
            let _ = kind;
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
            if lines.is_empty() {
                body.push(
                    div()
                        .px(px(10.))
                        .text_color(theme.muted)
                        .child(tr("settings-empty"))
                        .into_any_element(),
                );
            }
            let at = lines
                .iter()
                .position(|l| matches!(l, Line::Setting(i) if shown.get(selected) == Some(i)))
                .unwrap_or(0);
            let first = (at + 1).saturating_sub(ROOM);
            // The settings above the first line shown.
            let mut n = lines[..first]
                .iter()
                .filter(|l| matches!(l, Line::Setting(_)))
                .count();
            for line in lines.iter().skip(first).take(ROOM) {
                match line {
                    Line::Section(name) => {
                        body.push(heading((*name).to_string()).into_any_element());
                    }
                    Line::Setting(i) => {
                        let spec = &SPECS[*i];
                        let id = SharedString::from(format!("settings-row-{}", spec.key));
                        let r = row(id, n == selected).child(
                            div()
                                .w(px(size * 15.))
                                .flex_none()
                                .truncate()
                                .text_color(theme.foreground)
                                .child(settings_list::name(spec.key)),
                        );
                        let r = match &s.typing {
                            Some(t) if t.target == Target::Value(spec.key) => r.child(input(t)),
                            _ => {
                                let changed = settings_list::changed(config, spec);
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
                                        .child(SharedString::from(settings_list::shown(
                                            config, spec,
                                        ))),
                                )
                            }
                        };
                        let at = n;
                        body.push(
                            r.on_click(
                                cx.listener(move |this, _, _, cx| this.settings_click(at, cx)),
                            )
                            .into_any_element(),
                        );
                        n += 1;
                    }
                }
            }
        }
        // The fonts offered while the font is typed.
        let fonts = s.offered().into_iter().enumerate().map(|(i, f)| {
            let name = f.to_string();
            let label = if name.is_empty() {
                tr("settings-system-font")
            } else {
                name.clone()
            };
            let chosen = s.typing.as_ref().and_then(|t| t.chosen) == Some(i);
            div()
                .id(("settings-font", i))
                .debug_selector(move || format!("settings-font-{i}"))
                .px(px(10.))
                .rounded(px(3.))
                .cursor_pointer()
                .when(chosen, |d| d.bg(theme.selection))
                .child(SharedString::from(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(s) = &mut this.settings {
                        s.typing = None;
                    }
                    this.set_setting("editor.font_family", Value::from(name.clone()), cx);
                }))
        });
        // The chosen setting: its key, default (or items) and description.
        let about = current.map(|spec| {
            let note = match settings_list::edit(spec) {
                Edit::Items => kalem_core::tr!(
                    "settings-items",
                    count = settings_list::items(config, spec).len()
                ),
                _ => kalem_core::tr!(
                    "settings-default",
                    value = settings_list::shown_default(spec)
                ),
            };
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pt(px(6.))
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_color(theme.link)
                        .child(SharedString::from(format!("{}  {note}", spec.key))),
                )
                .child(
                    div()
                        .text_color(theme.foreground)
                        .child(SharedString::from(spec.description)),
                )
        });
        let hints = match items_of.map(|(_, k)| k) {
            Some(Items::Choices(_)) => tr("settings-items-choices"),
            Some(Items::Texts) => tr("settings-items-texts"),
            Some(Items::Table(_)) => tr("settings-items-table"),
            None => tr("settings-hints"),
        };
        let count = format!("{}/{}", (selected + 1).min(shown.len()), shown.len());
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
                        let by = if dy < 0. { 1 } else { -1 };
                        let shared = this.shared.clone();
                        let Some(s) = &mut this.settings else {
                            return;
                        };
                        if dy == 0. {
                            return;
                        }
                        match (s.list.item, s.list.current()) {
                            (Some(_), Some(spec)) => {
                                let n = settings_list::items(&shared.config, spec).len();
                                s.list.move_item(by, n);
                            }
                            _ => s.list.move_by(by),
                        }
                        cx.notify();
                        cx.stop_propagation();
                    })),
            )
            .child(div().flex().flex_col().children(fonts))
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
