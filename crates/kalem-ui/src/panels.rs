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
    /// The cursor, as characters after it ([`kalem_core::line_edit`]).
    pub back: usize,
    /// The chosen line.
    pub selected: usize,
    items: Vec<PaletteItem>,
    /// The items' order means something (a context menu): kept while
    /// nothing is typed.
    pub ordered: bool,
    /// Asking for an argument instead of choosing a command.
    pub arg: Option<ArgPrompt>,
    /// Choosing from a list instead: documents, files, projects.
    pub pick: Option<Picker>,
    /// Searching a project's files instead.
    pub search: Option<ProjectSearch>,
    /// Opened by a request for a list ([`kalem_core::command::Request::is_picker`]),
    /// which `SPC '` opens again with what is typed in it.
    pub resumable: bool,
    /// Searching the lines of open documents instead (`SPC s b`).
    pub lines: Option<kalem_core::line_search::LineSearch>,
    /// Where the cursor was when the line search opened: its source in
    /// the search, and the selection's anchor and head, to go back to.
    pub origin: Option<(usize, usize, usize)>,
}

impl Palette {
    /// The typed text changed: the searches follow it, and the list
    /// starts at its top (a line search at the cursor's line).
    pub fn input_changed(&mut self) {
        self.selected = 0;
        if let Some(s) = &mut self.search {
            s.set_text(&self.input);
        }
        if let Some(l) = &mut self.lines {
            l.set_text(&self.input);
            if let Some((source, _, head)) = self.origin {
                self.selected = l.nearest(source, head);
            }
        }
    }

    fn new(input: String) -> Palette {
        Palette {
            input,
            back: 0,
            selected: 0,
            items: Vec::new(),
            ordered: false,
            arg: None,
            pick: None,
            search: None,
            resumable: false,
            lines: None,
            origin: None,
        }
    }

    /// The commands (or the list's items) matching the input, best first.
    pub fn matches(&self) -> Vec<&PaletteItem> {
        if self.arg.is_some() || self.search.is_some() || self.lines.is_some() {
            return Vec::new();
        }
        match &self.pick {
            Some(p) => kalem_core::projects::matches(p, &self.input),
            None if self.ordered => palette::matches_ordered(&self.items, &self.input),
            None => palette::matches(&self.items, &self.input),
        }
    }

    /// How many lines can be chosen.
    pub fn len(&self) -> usize {
        match (&self.search, &self.lines) {
            (Some(s), _) => s.hits.len(),
            (_, Some(l)) => l.hits.len(),
            _ => self.matches().len(),
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

    /// Opens a list of commands to choose from.
    pub fn open_choice(&mut self, items: Vec<PaletteItem>, cx: &mut Context<'_, Self>) {
        self.completion = None;
        let mut p = Palette::new(String::new());
        // The menu's separators are drawn by its popup at the mouse; the
        // list goes without them.
        p.items = kalem_core::palette::split_separators(items).0;
        p.ordered = true;
        self.palette = Some(p);
        cx.notify();
    }

    /// Opens the export dialog: the formats and the export settings.
    pub fn open_export_dialog(&mut self, cx: &mut Context<'_, Self>) {
        self.completion = None;
        let mut p = Palette::new(String::new());
        p.items = kalem_core::export_dialog_items(&self.shared.config);
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
        self.open_search_project(project, cx);
    }

    /// Opens the search of the files under `dir`: the project's when it is
    /// one (with its ignore rules), else the folder's.
    pub fn open_search_in(&mut self, dir: &std::path::Path, cx: &mut Context<'_, Self>) {
        let project = self.shared.projects.borrow().list.get(dir).cloned();
        let project =
            project.unwrap_or_else(|| kalem_core::projects::Project::new(dir.to_path_buf()));
        self.open_search_project(project, cx);
    }

    fn open_search_project(
        &mut self,
        project: kalem_core::projects::Project,
        cx: &mut Context<'_, Self>,
    ) {
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
                    (After::Browse, _) => cx.emit(DocEvent::FileManager {
                        place: kalem_core::dired::Place::Dir(root),
                        select: None,
                    }),
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
        let config = self.shared.config.clone();
        let input = kalem_core::command::argument_default_with(
            command,
            &name,
            &args,
            &mut self.doc,
            &config,
        );
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
    /// Opens the live search of lines ([`kalem_core::line_search`]);
    /// `here` is this document's source in it.
    pub fn open_line_search(
        &mut self,
        search: kalem_core::line_search::LineSearch,
        here: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let sel = self.doc.selection;
        let mut p = Palette::new(search.text().to_string());
        p.lines = Some(search);
        p.origin = Some((here, sel.anchor, sel.head));
        p.input_changed();
        self.completion = None;
        self.palette = Some(p);
        self.preview_line(cx);
        cx.notify();
    }

    /// The cursor follows the chosen line while it is in this document.
    fn preview_line(&mut self, cx: &mut Context<'_, Self>) {
        let Some(p) = &self.palette else { return };
        let (Some(l), Some((here, ..))) = (&p.lines, p.origin) else {
            return;
        };
        if let Some(h) = l.hits.get(p.selected).filter(|h| h.source == here) {
            let at = h.at;
            self.doc.move_cursor(at, false);
            self.after_change(cx);
        }
    }

    /// Ends the line search: at line `n` of the list (another document's
    /// through the workspace), or back where it began.
    fn end_line_search(&mut self, n: Option<usize>, cx: &mut Context<'_, Self>) {
        let Some(p) = self.palette.take() else { return };
        let (Some(l), Some((here, anchor, head))) = (p.lines, p.origin) else {
            return;
        };
        match n.and_then(|n| l.hits.get(n)) {
            Some(h) if h.source == here => self.doc.move_cursor(h.at, false),
            Some(h) => cx.emit(DocEvent::Jump {
                doc: l.sources[h.source].doc,
                at: h.at,
            }),
            None => {
                self.doc.move_cursor(anchor, false);
                self.doc.move_cursor(head, true);
            }
        }
        self.after_change(cx);
        cx.notify();
    }

    fn run_palette_line(&mut self, n: usize, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.palette.as_ref().is_some_and(|p| p.lines.is_some()) {
            self.end_line_search(Some(n), cx);
            return;
        }
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
        // A picker's item carries its command's arguments.
        let (command, args) = kalem_core::palette::split_invocation(&id);
        let command = command.to_string();
        self.run_command(&command, args, window, cx);
        self.last_command = Some(command);
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
        self.apply_resume();
        self.remember_picker();
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
        // The arrows, Home, End and the deletions edit the typed text.
        let mac = cfg!(target_os = "macos");
        let word = if mac {
            k.modifiers.alt
        } else {
            k.modifiers.control
        };
        if let Some(edit) =
            kalem_core::line_edit::from_key(&k.key, word, mac && k.modifiers.platform)
        {
            if kalem_core::line_edit::apply(&mut p.input, &mut p.back, edit) {
                p.input_changed();
            }
            self.preview_line(cx);
            cx.notify();
            return true;
        }
        match k.key.as_str() {
            "escape" if p.lines.is_some() => self.end_line_search(None, cx),
            "enter" if p.lines.is_some() => {
                let n = p.selected;
                self.end_line_search(Some(n), cx);
            }
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
            "pagedown" if n > 0 => p.selected = (p.selected + 10).min(n - 1),
            "pageup" => p.selected = p.selected.saturating_sub(10),
            _ => return false,
        }
        self.preview_line(cx);
        cx.notify();
        true
    }

    /// Typed text for an open panel; `true` if a panel took it.
    pub fn panel_input(&mut self, text: &str, cx: &mut Context<'_, Self>) -> bool {
        if self.date_input(text, cx) || self.settings_input(text, cx) {
            return true;
        }
        self.apply_resume();
        if let Some(p) = &mut self.palette {
            kalem_core::line_edit::insert(&mut p.input, p.back, text);
            p.input_changed();
            self.remember_picker();
            self.preview_line(cx);
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
        // A file a viewer shows: its units' text, searched on a thread.
        if let Some(v) = self.doc.viewer.as_deref_mut() {
            let Some(f) = &self.find else { return };
            if from_origin {
                v.search_start(&f.query);
            } else {
                v.search_next(backward);
            }
            cx.notify();
            return;
        }
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
        if let Some(v) = self.doc.viewer.as_deref_mut() {
            v.search_end();
        }
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
            // A file a viewer shows draws no text, so nothing takes typed
            // text as input: the bar takes it from the keys.
            _ if self.doc.viewer.is_some() && !(m.control || m.platform || m.function) => {
                let Some(text) = k.key_char.clone() else {
                    return false;
                };
                return self.panel_input(&text, cx);
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
        // The typed text with the cursor drawn where it is.
        let (before, after) = kalem_core::line_edit::split(&p.input, p.back);
        let typed = format!("{before}▏{after}");
        let prompt = match (&p.arg, &p.pick, &p.search) {
            _ if p.lines.is_some() => {
                let n = p.lines.as_ref().map_or(0, |l| l.hits.len());
                note = kalem_core::tr!("search-lines-count", count = n);
                format!("{}: {typed}", kalem_core::l10n::tr("search-lines"))
            }
            (Some(a), _, _) => format!("{}  {typed}", a.label),
            (_, Some(k), _) => {
                if k.partial {
                    note = kalem_core::l10n::tr("pick-walking");
                } else if k.items.is_empty()
                    && matches!(k.kind, PickKind::Projects | PickKind::RemoveProject)
                {
                    note = kalem_core::l10n::tr("pick-no-projects");
                }
                format!("{}: {typed}", k.prompt)
            }
            (_, _, Some(s)) => {
                let switches: Vec<String> = s
                    .switches()
                    .into_iter()
                    .map(|(l, on)| if on { format!("[{l}]") } else { l })
                    .collect();
                note = format!("{}   {}", switches.join(" "), s.status());
                format!(
                    "{}: {typed}",
                    kalem_core::tr!("search-project", project = s.name.clone()),
                )
            }
            _ => format!("> {typed}"),
        };
        // Lines: a title, a detail and keys (or a mark).
        let lines: Vec<(String, String, String)> = match &p.search {
            None if p.lines.is_some() => {
                let l = p.lines.as_ref().expect("a line search");
                l.hits
                    .iter()
                    .map(|h| {
                        let (text, place) = l.row(h);
                        (text, place, String::new())
                    })
                    .collect()
            }
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
        let status = match (&f.error, self.doc.viewer.as_deref()) {
            (Some(e), _) => e.clone(),
            (None, _) if f.query.is_empty() => String::new(),
            (None, Some(v)) => v.search_status(),
            (None, None) => format!("{current}/{}", f.matches.len()),
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
