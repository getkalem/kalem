//! A window: the toolbar, the open documents (a list on the left or
//! tabs at the top, grouped by project), the active editor and the status
//! bar; the menus.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, KeyBinding, Menu, MenuItem,
    ParentElement, PathPromptOptions, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, WindowBounds, WindowOptions, div, prelude::FluentBuilder as _, px, size,
};
use kalem_core::command::PickKind;
use kalem_core::projects::{self, After, Entry};
use kalem_core::tr;
use serde_json::json;

use crate::editor::{DocEvent, Editor, RunCommand, Shared};
use crate::theme::Theme;

/// Where the list of open files shows (`ui.open_files`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesAt {
    /// A sidebar on the left.
    Left,
    /// Tabs at the top.
    Top,
    /// Not at all.
    Hidden,
}

impl FilesAt {
    /// The place a setting names.
    pub fn from_setting(s: &str) -> FilesAt {
        match s {
            "top" => FilesAt::Top,
            "hidden" => FilesAt::Hidden,
            _ => FilesAt::Left,
        }
    }
}

/// A menu of the toolbar, when open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMenu {
    /// Font families.
    Font,
    /// Font sizes.
    Size,
    /// Text colors.
    Color,
    /// Highlight colors.
    Highlight,
    /// Line spacings of the document.
    Spacing,
}

/// A window's content.
pub struct Workspace {
    /// The active document's editor.
    pub editor: Entity<Editor>,
    /// Every open document's editor, in the order they were opened.
    pub editors: Vec<Entity<Editor>>,
    /// Settings, commands, keys and projects.
    pub shared: Rc<Shared>,
    /// Where the list of open files shows.
    pub files_at: FilesAt,
    /// The list of open files is shown (View > Open Files toggles it).
    pub files_shown: bool,
    /// The open menu of the toolbar.
    pub menu: Option<ToolMenu>,
    /// The open menu of the window's own menu bar (Linux and Windows,
    /// where gpui draws no menus), by its place in the bar.
    pub menubar: Option<usize>,
    /// The menu of the bar a click outside just closed (its title's click
    /// must not open it again).
    menubar_closed: Option<(usize, std::time::Instant)>,
    /// The menu a click outside just closed (its button's click must not
    /// open it again).
    menu_closed: Option<(ToolMenu, std::time::Instant)>,
    /// What is typed while the font menu is open: fonts whose names
    /// contain it are listed.
    pub font_filter: String,
    subscriptions: Vec<Subscription>,
    /// The last document shown that is not a file manager, to go back to.
    last_text: Option<Entity<Editor>>,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("documents", &self.editors.len())
            .field("files_at", &self.files_at)
            .finish_non_exhaustive()
    }
}

gpui::actions!(kalem, [AddProjectFolder]);

impl Workspace {
    /// A window's content with `editor` open.
    pub fn new(
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Workspace {
        let shared = editor.read(cx).shared.clone();
        let files_at = FilesAt::from_setting(shared.config.str("ui.open_files"));
        let mut ws = Workspace {
            editor: editor.clone(),
            editors: Vec::new(),
            shared,
            files_at,
            files_shown: files_at != FilesAt::Hidden,
            menu: None,
            menubar: None,
            menubar_closed: None,
            menu_closed: None,
            font_filter: String::new(),
            subscriptions: Vec::new(),
            last_text: None,
        };
        ws.adopt(editor.clone(), window, cx);
        // A window coming to the front brings its document's menus.
        ws.subscriptions
            .push(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        if let Some(p) = editor.read(cx).doc.meta.path.clone()
            && p.is_file()
        {
            ws.shared.projects.borrow_mut().opened(&p);
        }
        ws.set_title(window, cx);
        ws.enter_project(cx);
        ws
    }

    /// The active document's project counts as switched to, and its files
    /// start being listed (a folder under version control becomes a
    /// project first, with `projects.auto_add`).
    fn enter_project(&self, cx: &mut Context<'_, Self>) {
        let path = self.editor.read(cx).doc.meta.path.clone();
        let auto = self.shared.config.bool("projects.auto_add");
        let msg = self
            .shared
            .projects
            .borrow_mut()
            .entered(path.as_deref(), auto);
        if let Some(m) = msg {
            self.editor.update(cx, |e, cx| {
                e.status = Some((m, false));
                cx.notify();
            });
        }
    }

    /// Back from the file manager to the document shown before it.
    fn leave_file_manager(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let listing = |e: &Entity<Editor>, cx: &App| e.read(cx).doc.dired.is_some();
        let target = self
            .last_text
            .clone()
            .filter(|e| self.editors.contains(e))
            .or_else(|| self.editors.iter().find(|e| !listing(e, cx)).cloned());
        match target {
            Some(e) => self.activate(e, window, cx),
            None => self.editor.update(cx, |e, cx| {
                e.status = Some((kalem_core::l10n::tr("msg-no-other-document"), false));
                cx.notify();
            }),
        }
    }

    /// Takes `editor` in: its events, its changes, its background work.
    fn adopt(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.subscriptions
            .push(cx.subscribe_in(&editor, window, Self::on_event));
        self.subscriptions
            .push(cx.observe(&editor, |_, _, cx| cx.notify()));
        spawn_tick(&editor, cx);
        self.editors.push(editor);
    }

    /// The open documents as the list of open files shows them.
    pub fn open_files(&self, cx: &App) -> Vec<projects::OpenFile> {
        self.editors
            .iter()
            .map(|e| e.read(cx).open_file())
            .collect()
    }

    fn index_of(&self, editor: &Entity<Editor>) -> Option<usize> {
        self.editors.iter().position(|e| e == editor)
    }

    /// Makes `editor` the active one.
    pub fn activate(
        &mut self,
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.editor.read(cx).doc.dired.is_none() {
            self.last_text = Some(self.editor.clone());
        }
        self.editor = editor.clone();
        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
        window.focus(&focus, cx);
        self.set_title(window, cx);
        self.enter_project(cx);
        cx.notify();
    }

    fn set_title(&self, window: &mut Window, cx: &App) {
        let title = self.editor.read(cx).title();
        window.set_window_title(&title);
    }

    /// Opens `path` (or shows it if it is open), the cursor at `at` (a
    /// line from 1, a byte column).
    pub fn open(
        &mut self,
        path: &Path,
        at: Option<(u64, usize)>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if path.is_dir() {
            let dir = projects::normal(path);
            self.file_manager(kalem_core::dired::Place::Dir(dir), None, window, cx);
            return;
        }
        let target = projects::normal(path);
        let found = self.editors.iter().find(|e| {
            e.read(cx)
                .doc
                .meta
                .path
                .as_deref()
                .is_some_and(|p| projects::normal(p) == target)
        });
        let editor = match found {
            Some(e) => e.clone(),
            None => {
                let theme = self.editor.read(cx).theme.clone();
                match crate::editor::open(Some(&target), self.shared.clone(), theme, cx) {
                    Ok(e) => {
                        // An untouched empty document gives way to the file.
                        let old = self.editor.clone();
                        let replace = {
                            let o = old.read(cx);
                            o.doc.meta.path.is_none()
                                && !o.doc.is_modified()
                                && o.doc.text().is_empty()
                        };
                        self.adopt(e.clone(), window, cx);
                        if replace {
                            self.editors.retain(|x| *x != old);
                        }
                        e
                    }
                    Err(err) => {
                        let msg = tr!(
                            "msg-cannot-open-file",
                            path = target.display().to_string(),
                            error = err
                        );
                        self.editor.update(cx, |e, cx| {
                            e.status = Some((msg, true));
                            cx.notify();
                        });
                        return;
                    }
                }
            }
        };
        if target.is_file() {
            self.shared.projects.borrow_mut().opened(&target);
        }
        if let Some((line, column)) = at {
            editor.update(cx, |e, cx| {
                let text = e.doc.text();
                let l = (line.max(1) as usize - 1).min(text.line_count().saturating_sub(1));
                let r = text.line_range(l);
                let mut pos = (r.start + column).min(r.end);
                while !text.as_str().is_char_boundary(pos) {
                    pos -= 1;
                }
                e.doc.move_cursor(pos, false);
                e.after_change(cx);
            });
        }
        self.activate(editor, window, cx);
    }

    /// A new, empty document.
    pub fn new_document(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let theme = self.editor.read(cx).theme.clone();
        if let Ok(e) = crate::editor::open(None, self.shared.clone(), theme, cx) {
            self.adopt(e.clone(), window, cx);
            self.activate(e, window, cx);
        }
    }

    /// Closes `editor`'s document (its unsaved changes were dealt with);
    /// the last one closes the window.
    pub fn close(
        &mut self,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let order = projects::order(&self.open_files(cx), &self.shared.projects.borrow().list);
        let Some(i) = self.index_of(editor) else {
            return;
        };
        let at = order.iter().position(|&x| x == i).unwrap_or(0);
        self.editors.remove(i);
        if self.editors.is_empty() {
            window.remove_window();
            return;
        }
        if *editor == self.editor {
            // The next one in the list, else the one before.
            let next = order
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|p| order.get(p)))
                .copied()
                .unwrap_or(0);
            let next = if next > i { next - 1 } else { next };
            let e = self.editors[next.min(self.editors.len() - 1)].clone();
            self.activate(e, window, cx);
        }
        cx.notify();
    }

    /// Shows the next open document (in the list's order), or the previous.
    pub fn cycle(&mut self, back: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        let order = projects::order(&self.open_files(cx), &self.shared.projects.borrow().list);
        let Some(i) = self.index_of(&self.editor.clone()) else {
            return;
        };
        let at = order.iter().position(|&x| x == i).unwrap_or(0);
        let n = order.len();
        let next = if back { (at + n - 1) % n } else { (at + 1) % n };
        let e = self.editors[order[next]].clone();
        self.activate(e, window, cx);
    }

    /// Opens picker `kind` in the active editor; lists about a project
    /// offer the projects first when there is none.
    pub fn pick(
        &mut self,
        kind: PickKind,
        project: Option<PathBuf>,
        after: After,
        cx: &mut Context<'_, Self>,
    ) {
        let files = self.open_files(cx);
        let current = self.editor.read(cx).doc.meta.path.clone();
        let picker = {
            let mut state = self.shared.projects.borrow_mut();
            match projects::picker(
                kind,
                &files,
                current.as_deref(),
                project.as_deref(),
                &mut state,
            ) {
                Some(mut p) => {
                    p.after = after;
                    Some(p)
                }
                None => projects::picker(PickKind::Projects, &files, None, None, &mut state).map(
                    |mut p| {
                        p.after = After::Pick(kind);
                        p
                    },
                ),
            }
        };
        if let Some(p) = picker {
            self.editor.update(cx, |e, cx| e.open_picker(p, cx));
        }
    }

    /// Searches the project at `root` (else the current one, after
    /// choosing a project when there is none).
    pub fn search(&mut self, root: Option<PathBuf>, cx: &mut Context<'_, Self>) {
        let root = root.or_else(|| self.editor.read(cx).project());
        match root {
            Some(r) => self.editor.update(cx, |e, cx| e.open_search(&r, cx)),
            None => {
                let files = self.open_files(cx);
                let p = projects::picker(
                    PickKind::Projects,
                    &files,
                    None,
                    None,
                    &mut self.shared.projects.borrow_mut(),
                );
                if let Some(mut p) = p {
                    p.after = After::Search;
                    self.editor.update(cx, |e, cx| e.open_picker(p, cx));
                }
            }
        }
    }

    /// The editors of the documents in the project at `root`.
    fn in_project(&self, root: &Path, cx: &App) -> Vec<Entity<Editor>> {
        self.editors
            .iter()
            .filter(|e| {
                e.read(cx)
                    .doc
                    .meta
                    .path
                    .as_deref()
                    .is_some_and(|p| projects::normal(p).starts_with(root))
            })
            .cloned()
            .collect()
    }

    /// Quits, asking first when documents (in any window) have unsaved
    /// changes.
    pub fn quit(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let mut modified: Vec<Entity<Editor>> = self
            .editors
            .iter()
            .filter(|e| e.read(cx).doc.is_modified())
            .cloned()
            .collect();
        let me = window.window_handle();
        for w in cx.windows() {
            if w == me {
                continue;
            }
            if let Some(w) = w.downcast::<Workspace>()
                && let Ok(ws) = w.read(cx)
            {
                modified.extend(
                    ws.editors
                        .iter()
                        .filter(|e| e.read(cx).doc.is_modified())
                        .cloned(),
                );
            }
        }
        if modified.is_empty() {
            cx.quit();
            return;
        }
        let (save, discard, cancel) = (
            tr!("dialog-save"),
            tr!("dialog-dont-save"),
            tr!("dialog-cancel"),
        );
        let question = if modified.len() == 1 {
            tr!("dialog-save-before-quit")
        } else {
            tr!(
                "dialog-save-all-before-quit",
                count = modified.len().to_string()
            )
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &question,
            None,
            &[save.as_str(), discard.as_str(), cancel.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let a = answer.await;
            let _ = this.update_in(cx, |ws, _window, cx| match a {
                Ok(0) => {
                    let mut all = true;
                    for e in &modified {
                        all &= e.update(cx, |e, cx| {
                            let ok = e.save_quietly();
                            cx.notify();
                            ok
                        });
                    }
                    if all {
                        cx.quit();
                    } else {
                        // Untitled documents need a name: Save As.
                        ws.editor.update(cx, |e, cx| {
                            e.status = Some((tr!("msg-not-saved", reason = tr!("untitled")), true));
                            cx.notify();
                        });
                    }
                }
                Ok(1) => cx.quit(),
                _ => {}
            });
        })
        .detach();
    }

    fn on_event(
        &mut self,
        editor: &Entity<Editor>,
        ev: &DocEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match ev.clone() {
            DocEvent::Open { path, at } => self.open(&path, at, window, cx),
            DocEvent::New => self.new_document(window, cx),
            DocEvent::Close => self.close(editor, window, cx),
            DocEvent::Cycle(back) => self.cycle(back, window, cx),
            DocEvent::Activate(i) => {
                if let Some(e) = self.editors.get(i).cloned() {
                    self.activate(e, window, cx);
                }
            }
            DocEvent::Pick(kind, project, after) => self.pick(kind, project, after, cx),
            DocEvent::Search(root) => self.search(root, cx),
            DocEvent::ToggleFiles => {
                if self.files_at == FilesAt::Hidden {
                    self.files_at = FilesAt::Left;
                    self.files_shown = true;
                } else {
                    self.files_shown = !self.files_shown;
                }
                cx.notify();
            }
            DocEvent::SaveProject(root) => {
                let mut n = 0;
                for e in self.in_project(&root, cx) {
                    let saved = e.update(cx, |e, cx| {
                        let was = e.doc.is_modified();
                        let ok = e.save_quietly();
                        cx.notify();
                        was && ok
                    });
                    n += usize::from(saved);
                }
                let msg = tr!("msg-saved-count", count = n.to_string());
                self.editor.update(cx, |e, cx| {
                    e.status = Some((msg, false));
                    cx.notify();
                });
            }
            DocEvent::CloseProject(root) => {
                for e in self.in_project(&root, cx) {
                    if e.read(cx).doc.is_modified() {
                        continue;
                    }
                    self.close(&e, window, cx);
                    if self.editors.is_empty() {
                        return;
                    }
                }
            }
            DocEvent::Quit => self.quit(window, cx),
            DocEvent::FileManager { place, select } => self.file_manager(place, select, window, cx),
            DocEvent::LeaveFileManager => self.leave_file_manager(window, cx),
            DocEvent::FilesChanged { message, error } => {
                for e in self.editors.clone() {
                    e.update(cx, |e, cx| {
                        if e.doc.dired.is_some() {
                            e.doc.refresh_listing();
                            e.after_change(cx);
                        }
                    });
                }
                self.editor.update(cx, |e, cx| {
                    e.status = Some((message, error));
                    cx.notify();
                });
            }
        }
    }

    /// Shows `place` in the window's file manager: the active document if
    /// it is one, else another open one, else a new one; the cursor on
    /// `select`.
    pub fn file_manager(
        &mut self,
        place: kalem_core::dired::Place,
        select: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::dired::Place;
        let is_listing = |e: &Entity<Editor>, cx: &App| e.read(cx).doc.dired.is_some();
        let editor = if is_listing(&self.editor, cx) {
            self.editor.clone()
        } else if let Some(e) = self.editors.iter().find(|e| is_listing(e, cx)).cloned() {
            e
        } else {
            let theme = self.editor.read(cx).theme.clone();
            let e = crate::editor::open_listing(place.clone(), self.shared.clone(), theme, cx);
            // An untouched empty document gives way.
            let old = self.editor.clone();
            let replace = {
                let o = old.read(cx);
                o.doc.meta.path.is_none()
                    && o.doc.dired.is_none()
                    && !o.doc.is_modified()
                    && o.doc.text().is_empty()
            };
            self.adopt(e.clone(), window, cx);
            if replace {
                self.editors.retain(|x| *x != old);
            }
            e
        };
        let rows = kalem_core::dired::project_rows(&self.shared.projects.borrow().list);
        editor.update(cx, |e, cx| {
            match place {
                Place::Projects => {
                    kalem_core::dired::show_projects(&mut e.doc, rows, select.as_deref())
                }
                place => e.doc.visit(place, select.as_deref()),
            }
            e.after_change(cx);
        });
        self.activate(editor, window, cx);
    }

    /// Asks for a folder and adds it as a project.
    fn add_project_folder(&mut self, cx: &mut Context<'_, Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(p) = paths.into_iter().next()
            {
                let _ = this.update(cx, |ws, cx| {
                    let r = ws.shared.projects.borrow_mut().add(&p);
                    ws.editor.update(cx, |e, cx| {
                        e.status = Some(match r {
                            Ok(m) => (m, false),
                            Err(m) => (m, true),
                        });
                        cx.notify();
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Runs `id` in `editor`, then gives it the focus back.
    fn run_in(editor: &Entity<Editor>, id: &str, window: &mut Window, cx: &mut App) {
        editor.update(cx, |e, cx| {
            e.run_command(id, serde_json::Value::Null, window, cx)
        });
    }

    /// The folder tree of the active document's project, added to the
    /// sidebar `list`: a click opens or closes a folder, or opens a file.
    fn folder_tree(
        &self,
        mut list: gpui::Stateful<gpui::Div>,
        theme: &Theme,
        cx: &mut Context<'_, Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let current = self.editor.read(cx).doc.meta.path.clone();
        let root = {
            let projects = self.shared.projects.borrow();
            projects
                .containing(current.as_deref())
                .map(|p| p.root.clone())
        };
        let Some(root) = root else { return list };
        let rows = self.shared.projects.borrow_mut().tree_rows(&root);
        if rows.is_empty() {
            return list;
        }
        list = list.child(
            div()
                .px(px(10.))
                .pt(px(12.))
                .pb(px(4.))
                .text_color(theme.muted)
                .child(kalem_core::l10n::tr("folder-tree")),
        );
        for (i, r) in rows.into_iter().enumerate() {
            let glyph = match (r.dir, r.open) {
                (true, true) => "▾ ",
                (true, false) => "▸ ",
                _ => "  ",
            };
            let path = r.path.clone();
            let tree_root = root.clone();
            let mut row = div()
                .id(("tree", i))
                .debug_selector(move || format!("tree-{i}"))
                .pl(px(10. + 14. * r.depth as f32))
                .pr(px(6.))
                .mx(px(4.))
                .py(px(1.))
                .rounded(px(4.))
                .cursor_pointer()
                .overflow_hidden()
                .whitespace_nowrap()
                .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.12)))
                .child(format!("{glyph}{}", r.name))
                .on_click(cx.listener(move |ws, _, window, cx| {
                    if r.dir {
                        ws.shared
                            .projects
                            .borrow_mut()
                            .toggle_tree(&tree_root, &path);
                        cx.notify();
                    } else {
                        ws.open(&path, None, window, cx);
                    }
                }));
            if r.dir {
                row = row.text_color(theme.link);
            }
            if current.as_deref() == Some(r.path.as_path()) {
                row = row.bg(theme.selection);
            }
            list = list.child(row);
        }
        list
    }

    /// The list of open files: projects with their files, then the files
    /// outside every project.
    fn files_view(&self, theme: &Theme, top: bool, cx: &mut Context<'_, Self>) -> gpui::AnyElement {
        let files = self.open_files(cx);
        let entries = projects::entries(&files, &self.shared.projects.borrow().list);
        let active = self.index_of(&self.editor);
        let mut list = if top {
            div()
                .id("open-files")
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.))
                .px(px(6.))
                .py(px(3.))
                .bg(theme.bar)
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(theme.size * 0.8))
                .overflow_x_scroll()
        } else {
            div()
                .id("open-files")
                .w(px(210.))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .py(px(6.))
                .bg(theme.bar)
                .border_r_1()
                .border_color(theme.border)
                .text_size(px(theme.size * 0.8))
                .overflow_y_scroll()
                .child(
                    div()
                        .px(px(10.))
                        .pb(px(4.))
                        .text_color(theme.muted)
                        .child(kalem_core::l10n::tr("open-files")),
                )
        };
        for entry in entries {
            match entry {
                Entry::Project { name, root } => {
                    let label = if top {
                        format!("{name}:")
                    } else {
                        format!("▾ {name}")
                    };
                    let head = div()
                        .text_color(theme.muted)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(label);
                    let _ = root;
                    list = list.child(if top {
                        head.pl(px(8.)).pr(px(2.))
                    } else {
                        head.px(px(10.)).pt(px(6.)).pb(px(2.))
                    });
                }
                Entry::File { index, nested } => {
                    let f = &files[index];
                    let editor = self.editors[index].clone();
                    let close_editor = editor.clone();
                    let mut row = div()
                        .id(("open-file", index))
                        .debug_selector(|| format!("open-file-{index}"))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.12)))
                        .child(div().overflow_hidden().child(f.title.clone()))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(4.))
                                .text_color(theme.muted)
                                .child(if f.modified { "●" } else { "" })
                                .child(
                                    div()
                                        .id(("close-file", index))
                                        .debug_selector(|| format!("close-file-{index}"))
                                        .px(px(3.))
                                        .rounded(px(3.))
                                        .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.25)))
                                        .child("×")
                                        .on_click(cx.listener(move |_, _, window, cx| {
                                            cx.stop_propagation();
                                            Self::run_in(&close_editor, "file.close", window, cx);
                                        })),
                                ),
                        )
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            ws.activate(editor.clone(), window, cx);
                        }));
                    if let Some(p) = &f.path {
                        let tip = projects::tilde(p);
                        row = row
                            .tooltip(move |_, cx| cx.new(|_| Tooltip(tip.clone().into())).into());
                    }
                    row = if top {
                        row.px(px(8.))
                    } else {
                        row.pl(px(if nested { 24. } else { 10. }))
                            .pr(px(6.))
                            .mx(px(4.))
                    };
                    if Some(index) == active {
                        row = row.bg(theme.selection);
                    }
                    list = list.child(row);
                }
            }
        }
        // The current project's folders and files.
        if !top && self.shared.config.bool("ui.folder_tree") {
            list = self.folder_tree(list, theme, cx);
        }
        // The file manager and the projects, at the end of the list.
        if !top {
            list = list.child(div().flex_1());
        }
        for (name, label, id) in [
            ("files-file-manager", "menu-file-manager", "dired.jump"),
            ("files-projects", "menu-projects-view", "dired.projects"),
        ] {
            let row = div()
                .id(name)
                .debug_selector(move || name.to_string())
                .py(px(2.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(theme.link)
                .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.12)))
                .child(if top {
                    kalem_core::l10n::tr(label)
                } else {
                    format!("▸ {}", kalem_core::l10n::tr(label))
                })
                .on_click(cx.listener(move |ws, _, window, cx| {
                    ws.run_active(id, serde_json::Value::Null, window, cx);
                }));
            list = list.child(if top {
                row.px(px(8.))
            } else {
                row.px(px(10.)).mx(px(4.))
            });
        }
        list.into_any_element()
    }
}

/// A tooltip's text.
struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .bg(gpui::hsla(0., 0., 0.15, 0.95))
            .text_color(gpui::white())
            .text_size(px(12.))
            .child(self.0.clone())
    }
}

/// Background parses and file changes of `editor`, until it is gone.
fn spawn_tick(editor: &Entity<Editor>, cx: &mut App) {
    let weak = editor.downgrade();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(50))
                .await;
            if weak.update(cx, |e, cx| e.tick(cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Toolbar buttons: label, tooltip, command, arguments, and a mode they
/// are not shown in (besides the documents their command is not offered in).
const TOOLBAR: &[(&str, &str, &str, &str, &str)] = &[
    ("B", "Bold", "org.emphasis.bold", "", ""),
    ("I", "Italic", "org.emphasis.italic", "", ""),
    ("U", "Underline", "org.emphasis.underline", "", ""),
    ("S", "Strike Through", "org.emphasis.strikeThrough", "", ""),
    ("</>", "Code", "org.emphasis.code", "", ""),
    ("•", "List", "list.cycleBullet", "", ""),
    (
        "☐",
        "Checkbox",
        "list.toggleCheckbox",
        r#"{"presence":true}"#,
        "",
    ),
    ("TODO", "Cycle TODO State", "org.todo.cycle", "", ""),
    (
        "⊞",
        "Insert Table",
        "table.create",
        r#"{"columns":3,"rows":2}"#,
        "",
    ),
    // LaTeX.
    ("B", "Bold", "latex.format.bold", "", ""),
    ("I", "Emphasis", "latex.format.italic", "", ""),
    ("U", "Underline", "latex.format.underline", "", ""),
    ("</>", "Typewriter", "latex.format.code", "", ""),
    ("∑", "Insert Equation", "latex.insert.equation", "", ""),
    ("▣", "Insert Figure", "latex.insert.figure", "", ""),
    (
        "⊞",
        "Insert Table",
        "latex.insert.table",
        r#"{"columns":3,"rows":2}"#,
        "",
    ),
    ("PDF", "Build PDF", "latex.build", "", ""),
    // CSV.
    ("+↓", "Insert Row", "csv.insertRow", "", ""),
    ("−↓", "Delete Row", "csv.deleteRow", "", ""),
    ("+→", "Insert Column", "csv.insertColumn", "", ""),
    ("−→", "Delete Column", "csv.deleteColumn", "", ""),
    ("⇅", "Sort File by Column", "csv.sortFile", "", ""),
    ("⌕", "Filter Rows", "csv.filter", "", ""),
    // Code.
    ("//", "Toggle Comment", "edit.toggleComment", "", "org"),
];

impl Workspace {
    fn toolbar(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let mut bar = div()
            .flex()
            .flex_row()
            .gap(px(2.))
            .px(px(8.))
            .py(px(4.))
            .bg(theme.bar)
            .border_b_1()
            .border_color(theme.border);
        // The file manager and the projects, one click away.
        bar = bar
            .child(self.command_button(
                "tool-files",
                kalem_core::l10n::tr("menu-file-manager").into(),
                "dired.jump",
                "",
                cx,
            ))
            .child(self.command_button(
                "tool-projects",
                kalem_core::l10n::tr("menu-projects-view").into(),
                "dired.projects",
                "",
                cx,
            ))
            .child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(theme.border));
        // Kalem's formatting: font, size, colors, alignment (§9.5).
        let org = self.editor.read(cx).doc.meta.mode == kalem_core::DocumentMode::Org;
        if org {
            let (f, doc) = {
                let e = self.editor.read(cx);
                (e.format_at_cursor(), e.doc_defaults())
            };
            let font = f
                .font
                .or(doc.font)
                .map(|f| f.family().to_string())
                .unwrap_or_else(|| kalem_core::l10n::tr("toolbar-font"));
            let size = f
                .size
                .or(doc.size)
                .map(kalem_core::rich::size_text)
                .unwrap_or_else(|| (theme.size as u16).to_string());
            bar = bar
                .child(self.menu_button("tool-font", font, ToolMenu::Font, 130., theme, cx))
                .child(self.menu_button("tool-size", size, ToolMenu::Size, 44., theme, cx))
                .child(self.command_button("tool-grow", "A+".into(), "format.grow", "", cx))
                .child(self.command_button("tool-shrink", "A−".into(), "format.shrink", "", cx))
                .child(self.swatch_button("tool-color", ToolMenu::Color, f.color, theme, cx))
                .child(self.swatch_button(
                    "tool-highlight",
                    ToolMenu::Highlight,
                    f.highlight,
                    theme,
                    cx,
                ))
                .child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(theme.border));
            for (i, (align, id)) in [
                (kalem_core::rich::Align::Left, "format.alignLeft"),
                (kalem_core::rich::Align::Center, "format.alignCenter"),
                (kalem_core::rich::Align::Right, "format.alignRight"),
                (kalem_core::rich::Align::Justify, "format.justify"),
            ]
            .into_iter()
            .enumerate()
            {
                bar = bar.child(self.align_button(i, align, id, theme, cx));
            }
            bar = bar
                .child(self.menu_button(
                    "tool-spacing",
                    "↕".into(),
                    ToolMenu::Spacing,
                    34.,
                    theme,
                    cx,
                ))
                .child(self.command_button("tool-clear", "T̸".into(), "format.clear", "", cx))
                .child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(theme.border));
        }
        let doc = self.editor.read(cx).doc.document_context();
        let mode = self.editor.read(cx).doc.meta.mode.name();
        for (i, (label, _tip, id, args, not_in)) in TOOLBAR.iter().enumerate() {
            // Only the buttons whose command the document offers.
            if *not_in == mode || !self.shared.registry.offered(id, &doc) {
                continue;
            }
            let (id, args) = (id.to_string(), args.to_string());
            let editor = self.editor.clone();
            bar = bar.child(
                div()
                    .id(("tool", i))
                    .debug_selector(|| format!("tool-{i}"))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.15)))
                    .child(label.to_string())
                    .on_click(cx.listener(move |_, _, window, cx| {
                        let a = RunCommand {
                            id: id.clone().into(),
                            args: args.clone().into(),
                        };
                        editor.update(cx, |e, cx| e.run_command(&a.id, a.args(), window, cx));
                        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
                        window.focus(&focus, cx);
                    })),
            );
        }
        bar.flex_wrap().items_center()
    }

    fn toggle_menu(&mut self, menu: ToolMenu, cx: &mut Context<'_, Self>) {
        let just_closed = self.menu_closed.take().is_some_and(|(m, at)| {
            m == menu && at.elapsed() < std::time::Duration::from_millis(400)
        });
        self.menu = if self.menu == Some(menu) || just_closed {
            None
        } else {
            Some(menu)
        };
        self.font_filter.clear();
        cx.notify();
    }

    /// Typing while the font menu is open searches the fonts: letters
    /// narrow the list, Backspace widens it, Enter takes the first font,
    /// Escape closes the menu.
    fn font_menu_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.menu != Some(ToolMenu::Font) {
            return;
        }
        let k = &ev.keystroke;
        if k.modifiers.control || k.modifiers.platform || k.modifiers.alt {
            return;
        }
        match k.key.as_str() {
            "escape" => {
                self.menu = None;
                self.font_filter.clear();
            }
            "backspace" => {
                self.font_filter.pop();
            }
            "enter" => {
                if let Some(family) = self.font_names(window).into_iter().next() {
                    self.font_filter.clear();
                    self.run_active("format.font", json!({ "family": family }), window, cx);
                }
            }
            _ => match &k.key_char {
                Some(c) if !c.chars().any(char::is_control) => self.font_filter.push_str(c),
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The system's fonts whose names contain the typed filter (case
    /// ignored), sorted.
    fn font_names(&self, window: &mut Window) -> Vec<String> {
        let filter = self.font_filter.to_lowercase();
        let mut names: Vec<String> = window
            .text_system()
            .all_font_names()
            .into_iter()
            .filter(|n| !n.starts_with('.'))
            .filter(|n| n.to_lowercase().contains(&filter))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Runs `id` with `args` in the active editor, then gives it the focus.
    fn run_active(
        &mut self,
        id: &str,
        args: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.menu = None;
        let editor = self.editor.clone();
        editor.update(cx, |e, cx| e.run_command(id, args, window, cx));
        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn command_button(
        &self,
        name: &'static str,
        label: SharedString,
        id: &'static str,
        args: &'static str,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(name)
            .debug_selector(move || name.to_string())
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.15)))
            .child(label)
            .on_click(cx.listener(move |ws, _, window, cx| {
                let args = serde_json::from_str(args).unwrap_or(serde_json::Value::Null);
                ws.run_active(id, args, window, cx);
            }))
    }

    fn menu_button(
        &self,
        name: &'static str,
        label: String,
        menu: ToolMenu,
        width: f32,
        theme: &Theme,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(name)
            .debug_selector(move || name.to_string())
            .w(px(width))
            .flex()
            .flex_row()
            .justify_between()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .cursor_pointer()
            .overflow_hidden()
            .child(div().overflow_hidden().child(label))
            .child(div().text_color(theme.muted).child("▾"))
            .on_click(cx.listener(move |ws, _, _, cx| ws.toggle_menu(menu, cx)))
    }

    fn swatch_button(
        &self,
        name: &'static str,
        menu: ToolMenu,
        current: Option<kalem_core::theme::Color>,
        theme: &Theme,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let (label, bar_color) = match menu {
            ToolMenu::Color => ("A", current.map_or(theme.foreground, crate::theme::color)),
            _ => (
                "ab",
                current.map_or(gpui::hsla(0.15, 1., 0.7, 1.), crate::theme::color),
            ),
        };
        div()
            .id(name)
            .debug_selector(move || name.to_string())
            .flex()
            .flex_col()
            .items_center()
            .px(px(6.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.15)))
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(label))
            .child(div().w(px(16.)).h(px(3.)).rounded(px(1.)).bg(bar_color))
            .on_click(cx.listener(move |ws, _, _, cx| ws.toggle_menu(menu, cx)))
    }

    fn align_button(
        &self,
        i: usize,
        align: kalem_core::rich::Align,
        id: &'static str,
        theme: &Theme,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        use kalem_core::rich::Align;
        // Four lines drawn like the icons of word processors.
        let widths: [f32; 4] = match align {
            Align::Justify => [14., 14., 14., 14.],
            _ => [14., 9., 12., 8.],
        };
        let mut icon = div().w(px(14.)).flex().flex_col().gap(px(2.));
        icon = match align {
            Align::Left | Align::Justify => icon.items_start(),
            Align::Center => icon.items_center(),
            Align::Right => icon.items_end(),
        };
        for w in widths {
            icon = icon.child(div().w(px(w)).h(px(1.5)).bg(theme.foreground));
        }
        div()
            .id(("tool-align", i))
            .debug_selector(move || format!("tool-align-{i}"))
            .px(px(5.))
            .py(px(5.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.15)))
            .child(icon)
            .on_click(cx.listener(move |ws, _, window, cx| {
                ws.run_active(id, serde_json::Value::Null, window, cx);
            }))
    }

    /// The open menu of the toolbar: fonts, sizes or colors.
    /// The window's own menu bar where the system draws none (Linux,
    /// Windows): the menus of `menus_for`, each opening a list of its
    /// items with their keys; `ui.menu_bar = false` hides it.
    fn menu_bar(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        if cfg!(target_os = "macos") || !self.shared.config.bool("ui.menu_bar") {
            return None;
        }
        let doc = self.editor.read(cx).doc.document_context();
        let menus = menus_for(&self.shared.registry, &doc);
        let hover = gpui::hsla(0., 0., 0.5, 0.15);
        let mut bar = div()
            .id("menu-bar")
            .flex()
            .flex_row()
            .px(px(4.))
            .bg(theme.bar)
            .border_b_1()
            .border_color(theme.border)
            .text_size(px(theme.size * 0.85));
        for (i, m) in menus.into_iter().enumerate() {
            let open = self.menubar == Some(i);
            let name = m.name.to_string();
            let mut title = div()
                .id(("menu-title", i))
                .debug_selector(move || format!("menu-{name}"))
                .relative()
                .px(px(8.))
                .py(px(3.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .when(open, move |d| d.bg(hover))
                .child(m.name.to_string())
                .on_click(cx.listener(move |ws, _, _, cx| {
                    let just_closed = ws
                        .menubar_closed
                        .is_some_and(|(j, t)| j == i && t.elapsed().as_millis() < 300);
                    ws.menubar = (!just_closed && ws.menubar != Some(i)).then_some(i);
                    cx.notify();
                }))
                .on_hover(cx.listener(move |ws, hovered: &bool, _, cx| {
                    // With a menu open, pointing at another title opens it.
                    if *hovered && ws.menubar.is_some_and(|j| j != i) {
                        ws.menubar = Some(i);
                        cx.notify();
                    }
                }));
            if open {
                let mut list = div()
                    .id("menu-list")
                    .occlude()
                    .absolute()
                    .top(px(theme.size * 0.85 + 10.))
                    .left(px(0.))
                    .min_w(px(240.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.bar)
                    .on_mouse_down_out(cx.listener(move |ws, _, _, cx| {
                        ws.menubar_closed = Some((i, std::time::Instant::now()));
                        ws.menubar = None;
                        cx.notify();
                    }));
                for (n, it) in m.items.into_iter().enumerate() {
                    match it {
                        MenuItem::Separator => {
                            list = list.child(div().h(px(1.)).my(px(3.)).bg(theme.border));
                        }
                        MenuItem::Action { name, action, .. } => {
                            let rc = action.as_any().downcast_ref::<RunCommand>();
                            let id = rc.map_or(String::new(), |rc| rc.id.to_string());
                            let keys = rc
                                .and_then(|rc| {
                                    self.shared.keymap.keys_for(&rc.id).first().map(|k| {
                                        crate::panels::show_keys(k, self.shared.swap_primary)
                                    })
                                })
                                .unwrap_or_default();
                            list = list.child(
                                div()
                                    .id(("menu-item", n))
                                    .debug_selector(move || format!("menu-item-{id}"))
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .gap(px(24.))
                                    .px(px(10.))
                                    .py(px(3.))
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(hover))
                                    .child(name.to_string())
                                    .child(div().text_color(theme.muted).child(keys))
                                    .on_click(cx.listener(move |ws, _, window, cx| {
                                        ws.menubar = None;
                                        window.dispatch_action(action.boxed_clone(), cx);
                                        cx.notify();
                                    })),
                            );
                        }
                        _ => {}
                    }
                }
                title = title.child(gpui::deferred(list).with_priority(1));
            }
            bar = bar.child(title);
        }
        Some(bar.into_any_element())
    }

    fn menu_view(
        &self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Option<gpui::AnyElement> {
        let menu = self.menu?;
        let item = |id: gpui::ElementId, label: String| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(3.))
                .cursor_pointer()
                .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.15)))
                .child(label)
        };
        let panel = div()
            .id("tool-menu")
            .occlude()
            .on_mouse_down_out(cx.listener(move |ws, _, _, cx| {
                ws.menu_closed = ws.menu.map(|m| (m, std::time::Instant::now()));
                ws.menu = None;
                cx.notify();
            }))
            .absolute()
            .top(px(34.))
            .left(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.bar)
            .text_size(px(theme.size * 0.85));
        let panel = match menu {
            ToolMenu::Font => {
                let names = self.font_names(window);
                let search = if self.font_filter.is_empty() {
                    kalem_core::l10n::tr("toolbar-font-search")
                } else {
                    format!("{}▏", self.font_filter)
                };
                let mut list = panel
                    .w(px(240.))
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .debug_selector(|| "font-search".to_string())
                            .px(px(8.))
                            .py(px(3.))
                            .text_color(theme.muted)
                            .child(search),
                    )
                    .child(
                        item(
                            "font-default".into(),
                            kalem_core::l10n::tr("toolbar-automatic"),
                        )
                        .on_click(cx.listener(|ws, _, window, cx| {
                            ws.run_active(
                                "format.font",
                                json!({ "family": "default" }),
                                window,
                                cx,
                            );
                        })),
                    );
                for (n, name) in names.into_iter().enumerate() {
                    let family = name.clone();
                    list = list.child(
                        item(("font", n).into(), name.clone())
                            .font_family(SharedString::from(name))
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.run_active(
                                    "format.font",
                                    json!({ "family": family }),
                                    window,
                                    cx,
                                );
                            })),
                    );
                }
                list.left(px(8.))
            }
            ToolMenu::Size => {
                let mut list = panel
                    .w(px(70.))
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .left(px(146.));
                for &sz in kalem_core::rich::SIZES {
                    list = list.child(
                        item(("size", sz as usize).into(), sz.to_string())
                            .debug_selector(move || format!("size-{sz}"))
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.run_active(
                                    "format.size",
                                    json!({ "size": sz.to_string() }),
                                    window,
                                    cx,
                                );
                            })),
                    );
                }
                list
            }
            ToolMenu::Spacing => {
                let mut list = panel.w(px(150.)).left(px(470.));
                for &sp in kalem_core::rich::SPACINGS {
                    let label = kalem_core::rich::size_text(sp);
                    let v = label.clone();
                    list = list.child(
                        item(("spacing", sp as usize).into(), label)
                            .debug_selector(move || format!("spacing-{sp}"))
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.run_active(
                                    "format.lineSpacing",
                                    json!({ "spacing": v }),
                                    window,
                                    cx,
                                );
                            })),
                    );
                }
                // The paragraph's space before and after it.
                for (id, key, points) in [
                    ("format.spaceBefore", "toolbar-space-before", "0"),
                    ("format.spaceBefore", "toolbar-space-before", "6"),
                    ("format.spaceBefore", "toolbar-space-before", "12"),
                    ("format.spaceAfter", "toolbar-space-after", "0"),
                    ("format.spaceAfter", "toolbar-space-after", "6"),
                    ("format.spaceAfter", "toolbar-space-after", "12"),
                ] {
                    let label = kalem_core::tr!(key, points = points);
                    let name = format!("{id}-{points}");
                    list = list.child(
                        item(SharedString::from(name.clone()).into(), label)
                            .debug_selector(move || name.clone())
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.run_active(id, json!({ "points": points }), window, cx);
                            })),
                    );
                }
                list
            }
            ToolMenu::Color | ToolMenu::Highlight => {
                let (colors, id, none) = if menu == ToolMenu::Color {
                    (
                        kalem_core::rich::COLORS,
                        "format.color",
                        "toolbar-automatic",
                    )
                } else {
                    (
                        kalem_core::rich::HIGHLIGHTS,
                        "format.highlight",
                        "toolbar-none",
                    )
                };
                let mut grid = div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(4.))
                    .w(px(150.))
                    .px(px(8.))
                    .py(px(4.));
                for (n, (name, hex)) in colors.iter().enumerate() {
                    let c = kalem_core::theme::Color::parse(hex)
                        .map_or(theme.foreground, crate::theme::color);
                    let name = name.to_string();
                    grid = grid.child(
                        div()
                            .id(("swatch", n))
                            .debug_selector(move || format!("swatch-{n}"))
                            .size(px(22.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(c)
                            .cursor_pointer()
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.run_active(id, json!({ "color": name }), window, cx);
                            })),
                    );
                }
                // The colors used lately, above the named ones.
                let key = if menu == ToolMenu::Color {
                    "format.recent_colors"
                } else {
                    "format.recent_highlights"
                };
                let recent: Vec<String> = self
                    .shared
                    .config
                    .strings(key)
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                let mut recent_row = div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(4.))
                    .w(px(150.))
                    .px(px(8.))
                    .py(px(4.));
                for (n, hex) in recent.iter().enumerate() {
                    let c = kalem_core::theme::Color::parse(hex)
                        .map_or(theme.foreground, crate::theme::color);
                    let value = hex.clone();
                    recent_row = recent_row.child(
                        div()
                            .id(("recent-swatch", n))
                            .debug_selector(move || format!("recent-swatch-{n}"))
                            .size(px(22.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(c)
                            .cursor_pointer()
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.run_active(id, json!({ "color": value }), window, cx);
                            })),
                    );
                }
                let mut panel = panel
                    .left(px(if menu == ToolMenu::Color { 250. } else { 280. }))
                    .child(
                        item("swatch-none".into(), kalem_core::l10n::tr(none)).on_click(
                            cx.listener(move |ws, _, window, cx| {
                                ws.run_active(id, json!({ "color": "none" }), window, cx);
                            }),
                        ),
                    );
                if !recent.is_empty() {
                    panel = panel
                        .child(
                            div()
                                .px(px(8.))
                                .pt(px(4.))
                                .text_color(theme.muted)
                                .child(kalem_core::l10n::tr("toolbar-recent")),
                        )
                        .child(recent_row);
                }
                panel.child(grid)
            }
        };
        Some(panel.into_any_element())
    }

    fn status_bar(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let e = self.editor.read(cx);
        let name = e.title();
        let (line, col) = e.doc.text().line_col(e.doc.selection.head);
        let words = match e.doc.dired.as_deref() {
            // The file manager: how many entries, and how many marked.
            Some(d) => {
                let mut s = format!(
                    "   {}",
                    kalem_core::tr!("fm-items", count = d.entries.len().max(d.projects.len()))
                );
                if !d.marks.is_empty() {
                    s.push_str(&format!(
                        "   {}",
                        kalem_core::tr!("fm-marked", count = d.marks.len())
                    ));
                }
                s
            }
            None => {
                let mut w = e.words.borrow_mut();
                w.get(&e.doc)
                    .map(|(d, s)| format!("   {}", kalem_core::stats::describe(d, s, w.targets())))
                    .unwrap_or_default()
            }
        };
        let state = kalem_core::l10n::tr(if e.doc.is_modified() {
            "status-modified"
        } else {
            "status-saved"
        });
        let position = kalem_core::tr!("status-position", line = line + 1, column = col + 1);
        let mode = e
            .vim
            .as_ref()
            .map(|v| format!("{}   ", v.status()))
            .unwrap_or_default();
        let formula = e
            .formula_status
            .as_ref()
            .map(|f| format!("   {f}"))
            .unwrap_or_default();
        let project = self
            .shared
            .projects
            .borrow()
            .containing(e.doc.meta.path.as_deref())
            .map(|p| format!("{} ▸ ", p.name))
            .unwrap_or_default();
        let table = kalem_core::formulas::selection_stats(&e.doc)
            .map(|t| format!("   {t}"))
            .unwrap_or_default();
        // The file kind: strict Org, or a Kalem document (§3.7).
        let kind = match kalem_core::kinds::file_kind(&e.doc) {
            Some("klm") => format!("{}   ", kalem_core::l10n::tr("kind-klm")),
            Some(_) => format!("{}   ", kalem_core::l10n::tr("kind-org")),
            None => String::new(),
        };
        let encoding = kalem_core::files::encoding_label(&e.doc.meta)
            .map(|n| format!("{n}   "))
            .unwrap_or_default();
        let left = format!(
            "{mode}{project}{name}   {kind}{encoding}{state}   {position}{words}{formula}{table}"
        );
        let (msg, error) = match self.shared.jobs.borrow().first() {
            // A file operation running: its progress.
            Some(j) => (j.status(), false),
            None => e.status.clone().unwrap_or_default(),
        };
        div()
            .flex()
            .flex_row()
            .justify_between()
            .px(px(12.))
            .py(px(3.))
            .text_size(px(12.))
            .bg(theme.bar)
            .border_t_1()
            .border_color(theme.border)
            .text_color(theme.muted)
            .child(left)
            .child(
                div()
                    .text_color(if error { theme.todo } else { theme.muted })
                    .child(msg),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = self.editor.read(cx).theme.clone();
        // The menus of the active window's document: only the commands
        // that serve it.
        if window.is_window_active() {
            let d = &self.editor.read(cx).doc;
            let (doc, key) = (
                d.document_context(),
                format!("{} {}", d.meta.mode.name(), d.document_type()),
            );
            if MENUS_FOR.with(|m| m.borrow().as_deref() != Some(key.as_str())) {
                cx.set_menus(menus_for(&self.shared.registry, &doc));
                MENUS_FOR.with(|m| *m.borrow_mut() = Some(key));
            }
        }
        let menu = self.menu_view(&theme, window, cx);
        let shown = self.files_shown && self.files_at != FilesAt::Hidden;
        let left =
            (shown && self.files_at == FilesAt::Left).then(|| self.files_view(&theme, false, cx));
        let top =
            (shown && self.files_at == FilesAt::Top).then(|| self.files_view(&theme, true, cx));
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(SharedString::from(theme.font.clone()))
            .on_action(cx.listener(|ws, _: &AddProjectFolder, _, cx| ws.add_project_folder(cx)))
            .capture_key_down(cx.listener(Self::font_menu_key))
            .relative()
            .children(self.menu_bar(&theme, cx))
            .child(self.toolbar(&theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_row()
                    .children(left)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .flex()
                            .flex_col()
                            .children(top)
                            .child(div().flex_1().min_h(px(0.)).child(self.editor.clone())),
                    ),
            )
            .child(self.status_bar(&theme, cx))
            .children(menu)
    }
}

/// The menus for a document with context `doc`
/// (`DocumentState::document_context`): items whose command is not offered
/// there are left out, and so are the separators and menus that leaves
/// empty.
pub fn menus_for(
    registry: &kalem_core::CommandRegistry,
    doc: &kalem_core::when::Context,
) -> Vec<Menu> {
    menus()
        .into_iter()
        .filter_map(|mut m| {
            let items = std::mem::take(&mut m.items);
            let mut kept: Vec<MenuItem> = Vec::new();
            for it in items {
                match &it {
                    MenuItem::Separator => {
                        if kept
                            .last()
                            .is_some_and(|l| !matches!(l, MenuItem::Separator))
                        {
                            kept.push(it);
                        }
                    }
                    MenuItem::Action { action, .. } => {
                        let serves = action
                            .as_any()
                            .downcast_ref::<RunCommand>()
                            .is_none_or(|rc| registry.offered(&rc.id, doc));
                        if serves {
                            kept.push(it);
                        }
                    }
                    _ => kept.push(it),
                }
            }
            if matches!(kept.last(), Some(MenuItem::Separator)) {
                kept.pop();
            }
            m.items = kept;
            (!m.items.is_empty()).then_some(m)
        })
        .collect()
}

thread_local! {
    /// The document context the application's menus were last made for.
    static MENUS_FOR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Makes the menus again on the next render of the active window (after a
/// change of language, say).
pub fn refresh_menus() {
    MENUS_FOR.with(|m| m.borrow_mut().take());
}

/// Every menu item, with the command of each, in the interface language
/// (`kalem_core::menus`).
pub fn menus() -> Vec<Menu> {
    use kalem_core::menus::MenuEntry;
    kalem_core::menus::menus()
        .into_iter()
        .map(|m| Menu {
            name: m.name.into(),
            disabled: false,
            items: m
                .entries
                .into_iter()
                .map(|e| match e {
                    MenuEntry::Separator => MenuItem::separator(),
                    MenuEntry::Command {
                        label,
                        id,
                        args: None,
                    } => MenuItem::action(label, RunCommand::new(id)),
                    MenuEntry::Command {
                        label,
                        id,
                        args: Some(args),
                    } => MenuItem::action(label, RunCommand::with(id, args)),
                    MenuEntry::Open(label) => MenuItem::action(label, OpenFile),
                    MenuEntry::AddProjectFolder(label) => MenuItem::action(label, AddProjectFolder),
                })
                .collect(),
        })
        .collect()
}

gpui::actions!(kalem, [OpenFile]);

/// Key bindings shown in the menus. Their context never applies, so keys
/// go through Kalem's keymap only.
pub fn menu_bindings(shared: &Shared) -> Vec<KeyBinding> {
    let mut out = Vec::new();
    for m in menus() {
        for it in m.items {
            let MenuItem::Action { action, .. } = it else {
                continue;
            };
            let Some(rc) = action.as_any().downcast_ref::<RunCommand>() else {
                continue;
            };
            let keys = shared.keymap.keys_for(&rc.id);
            if let Some(k) = keys.iter().find(|k| k.0.len() == 1) {
                let s = crate::keys::gpui_keys(&k.0[0], shared.swap_primary);
                out.push(KeyBinding::new(&s, rc.clone(), Some("KalemMenu")));
            }
        }
    }
    out
}

/// Opens a window for `path` (or an empty document).
pub fn open_window(path: Option<PathBuf>, shared: Rc<Shared>, cx: &mut App) {
    let bounds = gpui::Bounds::centered(None, size(px(980.), px(820.)), cx);
    let title: SharedString = path
        .as_ref()
        .and_then(|p| p.file_name())
        .map_or("Kalem".into(), |n| n.to_string_lossy().into_owned())
        .into();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(title),
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = cx.open_window(options, |window, cx| {
        let theme = Theme::from_config(&shared.config, window.appearance());
        let editor = match crate::editor::open(path.as_deref(), shared.clone(), theme.clone(), cx) {
            Ok(e) => e,
            Err(err) => {
                tracing::error!("{err}");
                // An empty document that says why (a binary file, say).
                let e = crate::editor::open(None, shared.clone(), theme, cx)
                    .expect("an empty document");
                e.update(cx, |e, _| e.status = Some((err, true)));
                e
            }
        };
        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
        window.focus(&focus, cx);
        // For start-up measurements (book/part-5/performance.org): quit after the
        // first frame.
        if std::env::var_os("KALEM_EXIT_AFTER_START").is_some() {
            window.on_next_frame(|_, cx| cx.quit());
        }
        cx.new(|cx| {
            // Follow the system between light and dark.
            cx.observe_window_appearance(window, |ws: &mut Workspace, window, cx| {
                let theme = Theme::from_config(&ws.shared.config, window.appearance());
                for e in ws.editors.clone() {
                    let theme = theme.clone();
                    e.update(cx, |e, cx| {
                        e.theme = theme;
                        e.relayout();
                        cx.notify();
                    });
                }
            })
            .detach();
            Workspace::new(editor, window, cx)
        })
    });
    if let Err(e) = result {
        tracing::error!("cannot open a window: {e}");
    }
}

/// Asks for files and opens them in the active window, or a new one.
pub fn open_file(shared: Rc<Shared>, cx: &mut App) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = paths.await {
            cx.update(|cx| {
                for p in paths {
                    open_path(p, shared.clone(), cx);
                }
            });
        }
    })
    .detach();
}

/// Opens `path` in the active window (or the first), or in a new window
/// when there is none.
pub fn open_path(path: PathBuf, shared: Rc<Shared>, cx: &mut App) {
    let target = cx
        .active_window()
        .and_then(|w| w.downcast::<Workspace>())
        .or_else(|| cx.windows().iter().find_map(|w| w.downcast::<Workspace>()));
    match target {
        Some(w) => {
            let opened = w.update(cx, |ws, window, cx| {
                ws.open(&path, None, window, cx);
                window.activate_window();
            });
            if opened.is_err() {
                open_window(Some(path), shared, cx);
            }
        }
        None => open_window(Some(path), shared, cx),
    }
}

/// Whether `path` names an existing file.
pub fn exists(path: &Path) -> bool {
    path.exists()
}
