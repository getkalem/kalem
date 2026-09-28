//! The command palette and the find and replace bar (T1.5.16).
//!
//! Both take typed text through the editor's input handler while they
//! are open (the find bar while it has the focus), so IME commits reach
//! them too; compositions show only once committed.

use std::ops::Range;
use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, InteractiveElement, IntoElement, Keystroke, MouseButton, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kalem_core::command::PickKind;
use kalem_core::find::{self, FindOptions};
use kalem_core::keys::KeySequence;
use kalem_core::palette::{self, PaletteItem};
use kalem_core::projects::{After, Picker, ProjectSearch};
use kalem_core::tr;
use org_edit::{ChangeKind, Selection, Transaction};
use serde_json::Value;

use crate::editor::{DocEvent, Editor};

/// A command waiting for an argument the palette asks for.
#[derive(Debug, Clone)]
pub struct ArgPrompt {
    command: String,
    args: Value,
    name: String,
    ty: String,
    /// What the palette shows.
    pub label: String,
}

/// The command palette.
#[derive(Debug)]
pub struct Palette {
    /// What was typed.
    pub input: String,
    /// The chosen line.
    pub selected: usize,
    items: Vec<PaletteItem>,
    /// Asking for an argument instead of choosing a command.
    pub arg: Option<ArgPrompt>,
    /// Choosing from a list instead: documents, files, projects.
    pub pick: Option<Picker>,
    /// Searching a project's files instead.
    pub search: Option<ProjectSearch>,
}

impl Palette {
    fn new(input: String) -> Palette {
        Palette {
            input,
            selected: 0,
            items: Vec::new(),
            arg: None,
            pick: None,
            search: None,
        }
    }

    /// The commands (or the list's items) matching the input, best first.
    pub fn matches(&self) -> Vec<&PaletteItem> {
        if self.arg.is_some() || self.search.is_some() {
            return Vec::new();
        }
        match &self.pick {
            Some(p) => kalem_core::projects::matches(p, &self.input),
            None => palette::matches(&self.items, &self.input),
        }
    }

    /// How many lines can be chosen.
    pub fn len(&self) -> usize {
        match &self.search {
            Some(s) => s.hits.len(),
            None => self.matches().len(),
        }
    }

    /// Whether nothing can be chosen.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The find bar.
#[derive(Debug, Clone, Default)]
pub struct FindBar {
    /// The search.
    pub query: String,
    /// The replacement, in find and replace.
    pub replacement: Option<String>,
    /// Typing goes to the replacement.
    pub on_replacement: bool,
    /// Keys and typing go to the bar (a click in the text takes them).
    pub focused: bool,
    /// Where the selection started when the search did.
    pub origin: usize,
    /// The matches.
    pub matches: Vec<Range<usize>>,
    /// The query is a regular expression.
    pub regex: bool,
    /// Why the regular expression does not compile.
    pub error: Option<String>,
}

impl FindBar {
    fn options(&self) -> FindOptions {
        FindOptions { regex: self.regex }
    }
}

/// A key sequence as the menus write it: Command for Control where they
/// trade places.
pub fn show_keys(k: &KeySequence, swap: bool) -> String {
    let s = k.to_string();
    if swap { s.replace("ctrl+", "cmd+") } else { s }
}

impl Editor {
    /// Opens the command palette.
    pub fn open_palette(&mut self, cx: &mut Context<'_, Self>) {
        let ctx = self.context();
        let swap = self.shared.swap_primary;
        let items = palette::items(&self.shared.registry, &self.shared.keymap, &ctx, |k| {
            show_keys(k, swap)
        });
        self.completion = None;
        let mut p = Palette::new(String::new());
        p.items = items;
        self.palette = Some(p);
        cx.notify();
    }

    /// Opens a list to choose from.
    pub fn open_picker(&mut self, picker: Picker, cx: &mut Context<'_, Self>) {
        self.completion = None;
        let mut p = Palette::new(String::new());
        p.pick = Some(picker);
        self.palette = Some(p);
        cx.notify();
    }

    /// Opens a search through a project's files, for the selected text.
    pub fn open_search(&mut self, root: &std::path::Path, cx: &mut Context<'_, Self>) {
        let project = self.shared.projects.borrow().list.get(root).cloned();
        let Some(project) = project else {
            self.message(tr!("msg-no-project"), true);
            return;
        };
        let text = self
            .doc
            .selected_text()
            .filter(|t| !t.contains('\n'))
            .unwrap_or("")
            .to_string();
        self.completion = None;
        let mut p = Palette::new(text.clone());
        p.search = Some(ProjectSearch::new(&project, &text));
        self.palette = Some(p);
        cx.notify();
    }

    /// Background work of an open list: search results, files found.
    pub fn tick_palette(&mut self, cx: &mut Context<'_, Self>) {
        let Some(p) = &mut self.palette else { return };
        if let Some(s) = &mut p.search {
            if s.poll() {
                cx.notify();
            }
            return;
        }
        if let Some(pick) = p.pick.as_mut().filter(|k| k.partial) {
            let fresh = kalem_core::projects::picker(
                pick.kind,
                &[],
                None,
                pick.project.as_deref(),
                &mut self.shared.projects.borrow_mut(),
            );
            if let Some(mut f) = fresh {
                f.after = pick.after;
                *pick = f;
                cx.notify();
            }
        }
    }

    /// Does what choosing `id` in `picker` means.
    fn picked(&mut self, picker: Picker, id: String, cx: &mut Context<'_, Self>) {
        match picker.kind {
            PickKind::Documents | PickKind::ProjectDocuments => {
                if let Ok(i) = id.parse() {
                    cx.emit(DocEvent::Activate(i));
                }
            }
            PickKind::RecentFiles | PickKind::ProjectFiles | PickKind::ProjectRecentFiles => {
                cx.emit(DocEvent::Open {
                    path: id.into(),
                    at: None,
                });
            }
            PickKind::Projects => {
                let root = std::path::PathBuf::from(id);
                let last = {
                    let mut projects = self.shared.projects.borrow_mut();
                    projects.list.used(&root);
                    if let Err(e) = projects.save() {
                        tracing::warn!("{e}");
                    }
                    projects.list.get(&root).map(|p| {
                        (
                            p.exists(),
                            p.name.clone(),
                            p.last_file.clone().filter(|f| f.is_file()),
                        )
                    })
                };
                let Some((exists, name, last)) = last else {
                    return;
                };
                if !exists {
                    self.message(tr!("msg-project-missing", name = name), true);
                    return;
                }
                match (picker.after, last) {
                    (After::Open, Some(f)) => cx.emit(DocEvent::Open { path: f, at: None }),
                    (After::Open, None) => {
                        cx.emit(DocEvent::Pick(
                            PickKind::ProjectFiles,
                            Some(root),
                            After::Open,
                        ));
                    }
                    (After::Pick(k), _) => cx.emit(DocEvent::Pick(k, Some(root), After::Open)),
                    (After::Search, _) => cx.emit(DocEvent::Search(Some(root))),
                }
            }
            PickKind::RemoveProject => {
                let r = self
                    .shared
                    .projects
                    .borrow_mut()
                    .remove(std::path::Path::new(&id));
                match r {
                    Ok(m) => self.message(m, false),
                    Err(m) => self.message(m, true),
                }
            }
        }
    }

    /// Asks for argument `name` of `command` in the palette.
    pub fn ask_argument(
        &mut self,
        command: &str,
        title: &str,
        args: Value,
        name: String,
        ty: String,
        cx: &mut Context<'_, Self>,
    ) {
        let input = kalem_core::command::argument_default(command, &name, &mut self.doc);
        let mut p = Palette::new(input);
        p.arg = Some(ArgPrompt {
            command: command.to_string(),
            args,
            label: format!("{title}: {name}"),
            name,
            ty,
        });
        self.palette = Some(p);
        cx.notify();
    }

    /// Runs the command on palette line `n`.
    fn run_palette_line(&mut self, n: usize, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(mut p) = self.palette.take() else {
            return;
        };
        if let Some(mut s) = p.search.take() {
            s.cancel();
            if let Some(h) = s.hits.get(n) {
                cx.emit(DocEvent::Open {
                    path: h.path.clone(),
                    at: Some((h.line, h.column)),
                });
            }
            cx.notify();
            return;
        }
        let Some(id) = p.matches().get(n).map(|it| it.id.clone()) else {
            return;
        };
        if let Some(picker) = p.pick.take() {
            self.picked(picker, id, cx);
            cx.notify();
            return;
        }
        self.run_command(&id, Value::Null, window, cx);
        self.last_command = Some(id);
        cx.notify();
    }

    /// Keys for the open palette; `true` if used. Typed text comes through
    /// [`Editor::panel_input`].
    pub fn palette_key(
        &mut self,
        k: &Keystroke,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(p) = &mut self.palette else {
            return false;
        };
        let n = p.len();
        // Alt+C, Alt+W, Alt+R: the search's switches.
        if let Some(s) = &mut p.search
            && k.modifiers.alt
            && let Some(c) = k.key.chars().next().filter(|c| "cwr".contains(*c))
        {
            s.toggle(c);
            cx.notify();
            return true;
        }
        match k.key.as_str() {
            "escape" => {
                if let Some(s) = &mut p.search {
                    s.cancel();
                }
                self.palette = None;
            }
            "enter" => match p.arg.take() {
                Some(a) => {
                    let input = std::mem::take(&mut p.input);
                    self.palette = None;
                    match kalem_core::command::parse_argument(&a.name, &a.ty, &input) {
                        Ok(v) => {
                            let args = kalem_core::command::with_argument(a.args, &a.name, v);
                            self.run_command(&a.command, args, window, cx);
                        }
                        Err(e) => self.status = Some((e, true)),
                    }
                }
                None => {
                    let s = p.selected;
                    self.run_palette_line(s, window, cx);
                }
            },
            "down" if n > 0 => p.selected = (p.selected + 1) % n,
            "up" if n > 0 => p.selected = (p.selected + n - 1) % n,
            "backspace" => {
                p.input.pop();
                p.selected = 0;
                if let Some(s) = &mut p.search {
                    s.set_text(&p.input);
                }
            }
            "pagedown" if n > 0 => p.selected = (p.selected + 10).min(n - 1),
            "pageup" => p.selected = p.selected.saturating_sub(10),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Typed text for an open panel; `true` if a panel took it.
    pub fn panel_input(&mut self, text: &str, cx: &mut Context<'_, Self>) -> bool {
        if self.date_input(text, cx) || self.settings_input(text, cx) {
            return true;
        }
        if let Some(p) = &mut self.palette {
            p.input.push_str(text);
            p.selected = 0;
            if let Some(s) = &mut p.search {
                s.set_text(&p.input);
            }
            cx.notify();
            return true;
        }
        let Some(f) = self.find.as_mut().filter(|f| f.focused) else {
            return false;
        };
        let one_line = text.replace(['\n', '\r'], " ");
        if f.on_replacement {
            f.replacement.get_or_insert_default().push_str(&one_line);
            cx.notify();
        } else {
            f.query.push_str(&one_line);
            self.search(false, true, cx);
        }
        true
    }

    /// Opens the find bar (with the replacement when `replace`), searching
    /// for the selected text or the last search.
    pub fn open_find(&mut self, replace: bool, cx: &mut Context<'_, Self>) {
        let selected = self
            .doc
            .selected_text()
            .filter(|t| !t.contains('\n') && !t.is_empty())
            .map(str::to_string);
        let s = self.doc.selection;
        let (query, regex) = match (&self.find, selected) {
            (_, Some(q)) => (q, self.find.as_ref().is_some_and(|f| f.regex)),
            (Some(f), None) => (f.query.clone(), f.regex),
            (None, None) => self.last_search.clone(),
        };
        let replacement = replace.then(|| {
            self.find
                .as_ref()
                .and_then(|f| f.replacement.clone())
                .unwrap_or_default()
        });
        self.find = Some(FindBar {
            query,
            replacement,
            focused: true,
            origin: s.anchor.min(s.head),
            regex,
            ..FindBar::default()
        });
        self.completion = None;
        self.search(false, true, cx);
    }

    /// Finds the matches again (after edits), without moving.
    pub fn refresh_matches(&mut self) {
        let Some(f) = &mut self.find else {
            self.highlights.clear();
            return;
        };
        match find::find_with(self.doc.text().as_str(), &f.query, f.options()) {
            Ok(m) => {
                f.matches = m;
                f.error = None;
            }
            Err(e) => {
                f.matches.clear();
                f.error = Some(e);
            }
        }
        self.highlights = f.matches.clone();
    }

    /// Selects the next (or previous) match; `from_origin`: from where the
    /// search started, while typing the query.
    pub fn search(&mut self, backward: bool, from_origin: bool, cx: &mut Context<'_, Self>) {
        self.refresh_matches();
        let Some(f) = &self.find else { return };
        let s = self.doc.selection;
        let from = if from_origin {
            f.origin
        } else if backward {
            s.anchor.min(s.head)
        } else {
            s.anchor.max(s.head)
        };
        if let Some(m) = find::next(&f.matches, from, backward) {
            self.doc.move_cursor(m.start, false);
            self.doc.move_cursor(m.end, true);
        }
        self.after_change(cx);
    }

    fn close_find(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(f) = self.find.take() {
            self.last_search = (f.query, f.regex);
        }
        self.highlights.clear();
        cx.notify();
    }

    /// Replaces the selected match and selects the next one.
    fn replace_one(&mut self, cx: &mut Context<'_, Self>) {
        let Some(f) = &self.find else { return };
        let s = self.doc.selection;
        let sel = s.anchor.min(s.head)..s.anchor.max(s.head);
        if f.matches.contains(&sel) {
            let with = f.replacement.as_deref().unwrap_or("");
            match find::replacement(
                self.doc.text().as_str(),
                sel.clone(),
                &f.query,
                with,
                f.options(),
            ) {
                Ok(r) => {
                    let mut tx = Transaction::new("Replace");
                    tx.replace(sel.clone(), r.clone()).expect("one edit");
                    let tx = tx.select(Selection::caret(sel.start + r.len()));
                    self.doc.apply(&tx, ChangeKind::Command, Instant::now());
                }
                Err(e) => self.status = Some((e, true)),
            }
        }
        self.search(false, false, cx);
    }

    /// Replaces every match, as one step.
    fn replace_all(&mut self, cx: &mut Context<'_, Self>) {
        let Some(f) = &self.find else { return };
        let with = f.replacement.clone().unwrap_or_default();
        match find::replace_all_with(self.doc.text().as_str(), &f.query, &with, f.options()) {
            Ok(Some(tx)) => {
                let n = tx.edits.len();
                self.doc.apply(&tx, ChangeKind::Command, Instant::now());
                self.status = Some((kalem_core::tr!("msg-replaced", count = n), false));
            }
            Ok(None) => self.status = Some((kalem_core::l10n::tr("msg-no-matches"), false)),
            Err(e) => self.status = Some((e, true)),
        }
        self.after_change(cx);
    }

    /// Keys for the focused find bar; `true` if used.
    pub fn find_key(&mut self, k: &Keystroke, cx: &mut Context<'_, Self>) -> bool {
        let Some(f) = self.find.as_mut().filter(|f| f.focused) else {
            return false;
        };
        let m = k.modifiers;
        match k.key.as_str() {
            "escape" => self.close_find(cx),
            "r" if m.alt => {
                f.regex = !f.regex;
                self.search(false, true, cx);
            }
            "enter" if m.alt => self.replace_all(cx),
            "enter" if f.on_replacement => self.replace_one(cx),
            "enter" => self.search(m.shift, false, cx),
            "down" => self.search(false, false, cx),
            "up" => self.search(true, false, cx),
            "tab" if f.replacement.is_some() => {
                f.on_replacement = !f.on_replacement;
                cx.notify();
            }
            "backspace" => {
                if f.on_replacement {
                    f.replacement.as_mut().map(String::pop);
                    cx.notify();
                } else {
                    f.query.pop();
                    self.search(false, true, cx);
                }
            }
            _ => return false,
        }
        true
    }

    /// The palette, when open.
    pub fn palette_view(&self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let p = self.palette.as_ref()?;
        let theme = &self.theme;
        let mut note = String::new();
        let prompt = match (&p.arg, &p.pick, &p.search) {
            (Some(a), _, _) => format!("{}  {}▏", a.label, p.input),
            (_, Some(k), _) => {
                if k.partial {
                    note = kalem_core::l10n::tr("pick-walking");
                } else if k.items.is_empty()
                    && matches!(k.kind, PickKind::Projects | PickKind::RemoveProject)
                {
                    note = kalem_core::l10n::tr("pick-no-projects");
                }
                format!("{}: {}▏", k.prompt, p.input)
            }
            (_, _, Some(s)) => {
                let switches: Vec<String> = s
                    .switches()
                    .into_iter()
                    .map(|(l, on)| if on { format!("[{l}]") } else { l })
                    .collect();
                note = format!("{}   {}", switches.join(" "), s.status());
                format!(
                    "{}: {}▏",
                    kalem_core::tr!("search-project", project = s.name.clone()),
                    p.input
                )
            }
            _ => format!("> {}▏", p.input),
        };
        // Lines: a title, a detail and keys (or a mark).
        let lines: Vec<(String, String, String)> = match &p.search {
            Some(s) => s
                .hits
                .iter()
                .map(|h| {
                    let (at, text) = s.line(h);
                    (text, at, String::new())
                })
                .collect(),
            None if p.pick.is_some() => p
                .matches()
                .iter()
                .map(|it| (it.title.clone(), it.category.clone(), it.keys.clone()))
                .collect(),
            None => p
                .matches()
                .iter()
                .map(|it| {
                    (
                        format!("{}: {}", it.category, it.title),
                        String::new(),
                        it.keys.clone(),
                    )
                })
                .collect(),
        };
        let first = p.selected.saturating_sub(11);
        let rows =
            lines
                .into_iter()
                .enumerate()
                .skip(first)
                .take(12)
                .map(|(n, (title, detail, keys))| {
                    let row = div()
                        .id(("palette-row", n))
                        .debug_selector(|| format!("palette-{n}"))
                        .flex()
                        .flex_row()
                        .justify_between()
                        .gap(px(16.))
                        .px(px(12.))
                        .py(px(3.))
                        .cursor_pointer()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(10.))
                                .overflow_hidden()
                                .child(SharedString::from(title))
                                .child(
                                    div()
                                        .text_color(theme.muted)
                                        .child(SharedString::from(detail)),
                                ),
                        )
                        .child(
                            div()
                                .text_color(theme.muted)
                                .child(SharedString::from(keys)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_palette_line(n, window, cx);
                        }));
                    if n == p.selected {
                        row.bg(theme.selection)
                    } else {
                        row
                    }
                });
        Some(
            div()
                .absolute()
                .top(px(8.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("palette")
                        .occlude()
                        .w(px(if p.search.is_some() || p.pick.is_some() {
                            720.
                        } else {
                            560.
                        }))
                        .flex()
                        .flex_col()
                        .py(px(4.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.bar)
                        .text_size(px(theme.size * 0.9))
                        .child(
                            div()
                                .px(px(12.))
                                .py(px(6.))
                                .border_b_1()
                                .border_color(theme.border)
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(SharedString::from(prompt))
                                .child(
                                    div()
                                        .text_color(theme.muted)
                                        .child(SharedString::from(note)),
                                ),
                        )
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    /// The find bar, when open.
    pub fn find_view(&self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let f = self.find.as_ref()?;
        let theme = &self.theme;
        let field = |text: &str, active: bool| {
            let shown = if active {
                format!("{text}▏")
            } else {
                text.to_string()
            };
            div()
                .min_w(px(220.))
                .px(px(6.))
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(if active { theme.caret } else { theme.border })
                .bg(theme.background)
                .child(SharedString::from(shown))
        };
        let current = {
            let s = self.doc.selection;
            let sel = s.anchor.min(s.head)..s.anchor.max(s.head);
            f.matches
                .iter()
                .position(|m| *m == sel)
                .map_or(0, |i| i + 1)
        };
        let status = match &f.error {
            Some(e) => e.clone(),
            None if f.query.is_empty() => String::new(),
            None => format!("{current}/{}", f.matches.len()),
        };
        let regex = div()
            .id("find-regex")
            .debug_selector(|| "find-regex".into())
            .px(px(4.))
            .rounded(px(3.))
            .cursor_pointer()
            .text_color(if f.regex {
                theme.foreground
            } else {
                theme.muted
            })
            .when(f.regex, |d| d.bg(theme.selection))
            .child(".*")
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(f) = &mut this.find {
                    f.regex = !f.regex;
                    f.focused = true;
                }
                this.search(false, true, cx);
            }));
        let mut bar = div()
            .id("find")
            .debug_selector(|| "find".into())
            .occlude()
            .absolute()
            .top(px(8.))
            .right(px(24.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.bar)
            .text_size(px(theme.size * 0.85))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(f) = &mut this.find {
                        f.focused = true;
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(field(&f.query, f.focused && !f.on_replacement))
                    .child(regex)
                    .child(
                        div()
                            .text_color(if f.error.is_some() {
                                theme.todo
                            } else {
                                theme.muted
                            })
                            .child(SharedString::from(status)),
                    ),
            );
        if let Some(r) = &f.replacement {
            bar = bar.child(field(r, f.focused && f.on_replacement));
        }
        Some(bar.into_any_element())
    }
}
