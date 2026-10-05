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
    /// The open menu of the window's own menu bar (Linux and Windows,
    /// where gpui draws no menus), by its place in the bar.
    pub menubar: Option<usize>,
    /// The menu of the bar a click outside just closed (its title's click
    /// must not open it again).
    menubar_closed: Option<(usize, std::time::Instant)>,
    subscriptions: Vec<Subscription>,
    /// The last document shown that is not a file manager, to go back to.
    last_text: Option<Entity<Editor>>,
    /// The text document (not a listing, not a viewer's file) shown last,
    /// where a viewer's Insert Link at Point inserts.
    last_document: Option<Entity<Editor>>,
    /// The document shown before the active one (`SPC b l`).
    previous: Option<Entity<Editor>>,
    /// The window's panes (`SPC w`, T2.7i.5); the focused one shows the
    /// active editor.
    pub layout: kalem_core::layout::Layout,
    /// What the other panes show (and closed ones, for an undo).
    pub panes: std::collections::HashMap<kalem_core::layout::PaneId, Entity<Editor>>,
    /// The window's workspaces (`SPC TAB`, T2.7i.15), each document by its
    /// editor's entity id.
    pub spaces: kalem_core::workspaces::Workspaces,
    /// The panes of the workspaces not shown, by workspace.
    stashed: std::collections::HashMap<u64, PaneStash>,
}

/// A workspace's panes while another one shows.
type PaneStash = (
    kalem_core::layout::Layout,
    std::collections::HashMap<kalem_core::layout::PaneId, Entity<Editor>>,
);

/// The number a workspace knows a document by.
fn doc_key(e: &Entity<Editor>) -> u64 {
    e.entity_id().as_u64()
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
            menubar: None,
            menubar_closed: None,
            subscriptions: Vec::new(),
            last_text: None,
            last_document: None,
            previous: None,
            layout: kalem_core::layout::Layout::new(),
            panes: std::collections::HashMap::new(),
            spaces: kalem_core::workspaces::Workspaces::new(),
            stashed: std::collections::HashMap::new(),
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

    /// A link to `file` in the text document shown last, which is shown.
    fn insert_link(
        &mut self,
        file: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let text = |e: &Entity<Editor>, cx: &App| {
            let d = &e.read(cx).doc;
            d.dired.is_none() && d.viewer.is_none()
        };
        let target = self
            .last_document
            .clone()
            .filter(|e| self.editors.contains(e) && text(e, cx))
            .or_else(|| self.editors.iter().rev().find(|e| text(e, cx)).cloned());
        let Some(target) = target else {
            self.editor.update(cx, |e, cx| {
                e.status = Some((kalem_core::l10n::tr("msg-viewer-no-document"), true));
                cx.notify();
            });
            return;
        };
        let file = file.to_path_buf();
        target.update(cx, |e, cx| {
            match e
                .doc
                .drop_pictures(std::slice::from_ref(&file), std::time::Instant::now())
            {
                Ok(()) => e.after_change(cx),
                Err(m) => e.status = Some((m, true)),
            }
            cx.notify();
        });
        self.activate(target, window, cx);
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
            .map(|e| projects::OpenFile {
                hidden: !self.spaces.shows(doc_key(e)),
                ..e.read(cx).open_file()
            })
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
        let doc = &self.editor.read(cx).doc;
        if doc.dired.is_none() {
            self.last_text = Some(self.editor.clone());
        }
        if doc.dired.is_none() && doc.viewer.is_none() {
            self.last_document = Some(self.editor.clone());
        }
        if self.editor != editor {
            self.previous = Some(self.editor.clone());
            // A pane that showed it shows the one the focused pane left.
            let focus = self.layout.focus();
            let live = self.layout.panes();
            if let Some(p) = self
                .panes
                .iter()
                .find(|(p, e)| **e == editor && **p != focus && live.contains(p))
                .map(|(p, _)| *p)
            {
                self.panes.insert(p, self.editor.clone());
            }
        }
        self.editor = editor.clone();
        self.spaces.showing(doc_key(&editor));
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
                    // A PDF with a password: asked for, then opened again.
                    Err(err) if err == tr!("msg-needs-password") => {
                        let title = tr!("cmd-file-openWithPassword");
                        let args = serde_json::json!({ "path": target.display().to_string() });
                        self.editor.update(cx, |e, cx| {
                            e.ask_argument(
                                "file.openWithPassword",
                                &title,
                                args,
                                "password".into(),
                                "string".into(),
                                cx,
                            );
                        });
                        return;
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
                // A PDF or another paged file: the line is the page.
                if let Some(v) = e.doc.viewer.as_deref_mut() {
                    v.go_to(line.max(1) as usize - 1);
                    if column > 0 {
                        v.show_height(column as f32);
                    }
                    cx.notify();
                    return;
                }
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
    /// closing the last one leaves an empty document, not an empty or
    /// closed window (quitting is Quit's).
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
        self.spaces.leave(doc_key(editor));
        if self.editors.is_empty() {
            self.new_document(window, cx);
            cx.notify();
            return;
        }
        // The other panes showing it close, or show the active document.
        let focus = self.layout.focus();
        let showing: Vec<_> = self
            .panes
            .iter()
            .filter(|(p, e)| *e == editor && **p != focus)
            .map(|(p, _)| *p)
            .collect();
        for p in showing {
            self.panes.remove(&p);
            self.layout.close(p);
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

    /// Shows the workspace `change` makes current: its panes and the
    /// document it showed last, else one of its documents, else a new
    /// empty one.
    fn show_space(
        &mut self,
        change: impl FnOnce(&mut kalem_core::workspaces::Workspaces) -> bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let old = self.spaces.current().id;
        self.spaces.showing(doc_key(&self.editor));
        if !change(&mut self.spaces) {
            return;
        }
        let stash = (
            std::mem::take(&mut self.layout),
            std::mem::take(&mut self.panes),
        );
        if self.spaces.list().iter().any(|w| w.id == old) {
            self.stashed.insert(old, stash);
        }
        let ws = self.spaces.current().clone();
        let (layout, panes) = self.stashed.remove(&ws.id).unwrap_or_default();
        self.layout = layout;
        self.panes = panes;
        let target = ws
            .active
            .and_then(|k| self.editors.iter().find(|e| doc_key(e) == k).cloned())
            .filter(|e| self.spaces.shows(doc_key(e)))
            .or_else(|| {
                self.editors
                    .iter()
                    .find(|e| self.spaces.shows(doc_key(e)))
                    .cloned()
            });
        match target {
            Some(e) => self.activate(e, window, cx),
            None => self.new_document(window, cx),
        }
        let msg = tr!("msg-workspace", name = ws.name);
        self.editor.update(cx, |e, cx| {
            e.message(msg, false);
            cx.notify();
        });
        cx.notify();
    }

    /// A workspace request (`SPC TAB`).
    fn workspace_op(
        &mut self,
        op: kalem_core::workspaces::WorkspaceOp,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::workspaces::{WorkspaceOp as W, Workspaces};
        let say = |ws: &mut Self, msg: String, error: bool, cx: &mut Context<'_, Self>| {
            ws.editor.update(cx, |e, cx| {
                e.message(msg, error);
                cx.notify();
            });
        };
        match op {
            W::List => {
                let items = self.spaces.items();
                self.editor.update(cx, |e, cx| e.open_choice(items, cx));
            }
            W::New(name) => self.show_space(
                |w| {
                    w.add(name.as_deref());
                    true
                },
                window,
                cx,
            ),
            W::Delete => {
                if !self.spaces.several() {
                    say(self, tr!("msg-last-workspace"), false, cx);
                    return;
                }
                let gone = self.spaces.current().id;
                self.show_space(|w| w.delete().is_some(), window, cx);
                self.stashed.remove(&gone);
            }
            W::Rename(name) => {
                self.spaces.rename(&name);
                cx.notify();
            }
            W::Cycle(back) => self.show_space(|w| w.cycle(back), window, cx),
            W::Switch(i) => self.show_space(|w| w.switch(i), window, cx),
            W::Last => self.show_space(|w| w.switch_last(), window, cx),
            W::Save => {
                let mut s = self.session(cx);
                let shown: Vec<std::path::PathBuf> = self
                    .editors
                    .iter()
                    .filter(|e| self.spaces.shows(doc_key(e)))
                    .filter_map(|e| e.read(cx).doc.meta.path.clone())
                    .collect();
                let active = s.documents.get(s.active).map(|d| d.path.clone());
                s.documents.retain(|d| shown.contains(&d.path));
                s.active = active
                    .and_then(|a| s.documents.iter().position(|d| d.path == a))
                    .unwrap_or(0);
                let name = Workspaces::session_name(&self.spaces.current().name);
                let (msg, error) = match kalem_core::sessions::save(&name, &s) {
                    Ok(p) => (
                        tr!("msg-session-saved", path = p.display().to_string()),
                        false,
                    ),
                    Err(e) => (e, true),
                };
                say(self, msg, error, cx);
            }
            W::Load(None) | W::DeleteSaved(None) => {
                let command = if matches!(op, W::DeleteSaved(_)) {
                    "workspace.deleteSaved"
                } else {
                    "workspace.load"
                };
                let names = Workspaces::saved();
                if names.is_empty() {
                    say(self, tr!("msg-no-saved-workspaces"), false, cx);
                    return;
                }
                let items: Vec<_> = names
                    .into_iter()
                    .map(|n| kalem_core::palette::PaletteItem {
                        id: kalem_core::palette::invocation(
                            command,
                            &serde_json::json!({ "name": n }),
                        ),
                        title: n,
                        category: tr!("category-workspaces"),
                        keys: String::new(),
                        also: String::new(),
                    })
                    .collect();
                self.editor.update(cx, |e, cx| e.open_choice(items, cx));
            }
            W::Load(Some(name)) => {
                match kalem_core::sessions::load(&Workspaces::session_name(&name)) {
                    Ok(s) => {
                        self.show_space(
                            |w| {
                                w.add(Some(&name));
                                true
                            },
                            window,
                            cx,
                        );
                        self.restore_session(s, window, cx);
                    }
                    Err(e) => say(self, e, true, cx),
                }
            }
            W::DeleteSaved(Some(name)) => {
                let (msg, error) =
                    match kalem_core::sessions::delete(&Workspaces::session_name(&name)) {
                        Ok(()) => (tr!("msg-workspace-deleted", name = name), false),
                        Err(e) => (e, true),
                    };
                say(self, msg, error, cx);
            }
        }
    }

    /// Focuses pane `p`: its editor becomes the active one.
    pub fn focus_pane(
        &mut self,
        p: kalem_core::layout::PaneId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let old = self.layout.focus();
        if p == old || !self.layout.set_focus(p) {
            return;
        }
        self.show_focused(old, window, cx);
    }

    /// After the focus moved from pane `old`: that pane keeps the active
    /// editor, and the focused pane's editor becomes active.
    fn show_focused(
        &mut self,
        old: kalem_core::layout::PaneId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let focus = self.layout.focus();
        self.panes.insert(old, self.editor.clone());
        let e = self
            .panes
            .remove(&focus)
            .filter(|e| self.editors.contains(e))
            .unwrap_or_else(|| self.editor.clone());
        // Set first so that activating does not hand `old` a pane twice.
        if e != self.editor {
            self.previous = Some(self.editor.clone());
            self.editor = e.clone();
            let f = gpui::Focusable::focus_handle(e.read(cx), cx);
            window.focus(&f, cx);
            self.set_title(window, cx);
        }
        cx.notify();
    }

    /// A change of the panes (`SPC w`).
    pub fn pane_op(
        &mut self,
        op: &kalem_core::layout::PaneOp,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::layout::PaneOp;
        if *op == PaneOp::CloseOrQuit {
            if self.layout.is_split() {
                return self.pane_op(&PaneOp::Close(false), window, cx);
            }
            // The split view of one document closes first.
            if self.editor.read(cx).other.is_some() {
                self.editor.update(cx, |e, cx| e.toggle_split(cx));
                return;
            }
            return self.quit(window, cx);
        }
        let old = self.layout.focus();
        // A split of the only document: the document beside itself, as
        // the split view shows it.
        if matches!(op, PaneOp::Split(_)) && self.editors.len() == 1 {
            self.editor.update(cx, |e, cx| e.toggle_split(cx));
            return;
        }
        let (changed, new) = self.layout.apply(op);
        if let PaneOp::Close(with_doc) = op {
            if !changed {
                self.editor.update(cx, |e, cx| {
                    e.message(tr!("msg-last-pane"), false);
                    cx.notify();
                });
                return;
            }
            let closing = self.editor.clone();
            self.show_focused(old, window, cx);
            if *with_doc {
                closing.update(cx, |e, cx| {
                    e.run_command("file.close", serde_json::Value::Null, window, cx)
                });
            }
            return;
        }
        if new.is_some() {
            // The pane left shows the document; the new one, focused, the
            // document shown before it (or a new empty one).
            self.panes.insert(old, self.editor.clone());
            if *op == PaneOp::New {
                self.new_document(window, cx);
            } else {
                let other = self
                    .previous
                    .clone()
                    .filter(|p| *p != self.editor && self.editors.contains(p))
                    .or_else(|| self.editors.iter().find(|e| **e != self.editor).cloned());
                if let Some(e) = other {
                    // The pane left keeps showing the document the user was
                    // in; the new pane shows the other one.
                    let here = self.editor.clone();
                    self.editor = e.clone();
                    self.panes.insert(old, here);
                    let f = gpui::Focusable::focus_handle(e.read(cx), cx);
                    window.focus(&f, cx);
                    self.set_title(window, cx);
                }
            }
            cx.notify();
            return;
        }
        if self.layout.focus() != old {
            self.show_focused(old, window, cx);
        }
        cx.notify();
    }

    /// The element of layout node `node`: a pane's editor, or a split's
    /// panes side by side or one above another with a rule between.
    fn pane_element(
        &self,
        node: &kalem_core::layout::Node,
        theme: &Theme,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        use kalem_core::layout::{Axis, Node};
        match node {
            Node::Leaf(p) => {
                let focus = self.layout.focus();
                let editor = if *p == focus {
                    Some(self.editor.clone())
                } else {
                    self.panes
                        .get(p)
                        .filter(|e| self.editors.contains(e))
                        .cloned()
                };
                let p = *p;
                let mut d = div().size_full().min_w(px(0.)).min_h(px(0.));
                if p != focus {
                    d = d.capture_any_mouse_down(cx.listener(move |ws, _, window, cx| {
                        ws.focus_pane(p, window, cx);
                    }));
                }
                d.children(editor).into_any_element()
            }
            Node::Split { axis, children } => {
                let total: f32 = children
                    .iter()
                    .map(|(_, w)| *w)
                    .sum::<f32>()
                    .max(f32::EPSILON);
                let n = children.len();
                let mut d = div().size_full().flex().min_w(px(0.)).min_h(px(0.));
                d = match axis {
                    Axis::Row => d.flex_row(),
                    Axis::Column => d.flex_col(),
                };
                for (i, (c, w)) in children.iter().enumerate() {
                    let share = gpui::relative(w / total);
                    let mut cell = div().min_w(px(0.)).min_h(px(0.)).overflow_hidden();
                    cell = match axis {
                        Axis::Row => cell.h_full().w(share),
                        Axis::Column => cell.w_full().h(share),
                    };
                    if i + 1 < n {
                        cell = match axis {
                            Axis::Row => cell.border_r_1(),
                            Axis::Column => cell.border_b_1(),
                        }
                        .border_color(theme.border);
                    }
                    d = d.child(cell.child(self.pane_element(c, theme, cx)));
                }
                d.into_any_element()
            }
        }
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

    /// Open documents follow their files after an operation: to where
    /// they were moved; closed when trashed or deleted, unless they have
    /// unsaved changes.
    fn follow_files(
        &mut self,
        kind: kalem_core::kalem_fs::OpKind,
        out: &kalem_core::kalem_fs::Outcome,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::dired::{Followed, follow};
        for e in self.editors.clone() {
            let path = e
                .read(cx)
                .doc
                .meta
                .path
                .as_deref()
                .and_then(|p| std::path::absolute(p).ok());
            let Some(path) = path else { continue };
            match follow(kind, out, &path) {
                Some(Followed::Moved(to)) => e.update(cx, |e, cx| {
                    e.doc.meta.path = Some(to);
                    cx.notify();
                }),
                Some(Followed::Removed) if !e.read(cx).doc.has_unsaved_edits() => {
                    e.update(cx, |e, _| e.doc.accept_removal());
                    if self.editors.len() == 1 {
                        self.new_document(window, cx);
                    }
                    self.close(&e, window, cx);
                }
                _ => {}
            }
        }
        self.set_title(window, cx);
    }

    /// Opens the live search of lines in the active editor: its
    /// document's, or every open one's (T2.7i.4).
    fn search_lines(&mut self, all: bool, headings: bool, text: &str, cx: &mut Context<'_, Self>) {
        use kalem_core::line_search::{LineSearch, Lines, Source};
        let files = self.open_files(cx);
        let mut sources = Vec::new();
        let mut here = 0;
        for (i, e) in self.editors.iter().enumerate() {
            let active = *e == self.editor;
            let doc = &e.read(cx).doc;
            if doc.dired.is_some() || (!all && !active) {
                continue;
            }
            if active {
                here = sources.len();
            }
            sources.push(Source {
                doc: i,
                name: files.get(i).map(|f| f.title.clone()).unwrap_or_default(),
                text: doc.text().as_str().into(),
            });
        }
        if sources.is_empty() {
            return;
        }
        let lines = if headings {
            Lines::Headings
        } else {
            Lines::All
        };
        let search = LineSearch::new(sources, lines, text);
        self.editor
            .update(cx, |e, cx| e.open_line_search(search, here, cx));
    }

    /// Doom's `SPC b` commands on the open documents (T2.7i.2).
    fn documents(
        &mut self,
        r: kalem_core::command::DocumentsRequest,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        use kalem_core::command::DocumentsRequest as D;
        let status = |ws: &Self, msg: String, error: bool, cx: &mut Context<'_, Self>| {
            ws.editor.update(cx, |e, cx| {
                e.status = Some((msg, error));
                cx.notify();
            });
        };
        match r {
            D::SaveAll => {
                let mut n = 0;
                for e in self.editors.clone() {
                    let saved = e.update(cx, |e, cx| {
                        let was = e.doc.is_modified() && e.doc.meta.path.is_some();
                        let ok = was && e.save_quietly();
                        cx.notify();
                        ok
                    });
                    n += usize::from(saved);
                }
                status(
                    self,
                    tr!("msg-saved-count", count = n.to_string()),
                    false,
                    cx,
                );
            }
            D::CloseOthers | D::CloseAll => {
                let keep = self.editor.clone();
                for e in self.editors.clone() {
                    if e != keep && !e.read(cx).doc.is_modified() {
                        self.close(&e, window, cx);
                    }
                }
                self.activate(keep.clone(), window, cx);
                // The last one gives way to an empty document.
                if r == D::CloseAll && !keep.read(cx).doc.is_modified() {
                    self.new_document(window, cx);
                    self.close(&keep, window, cx);
                }
            }
            D::Last => match self.previous.clone().filter(|p| self.editors.contains(p)) {
                Some(p) => self.activate(p, window, cx),
                None => status(self, tr!("msg-no-last-document"), true, cx),
            },
            D::Bury => {
                if self.editors.len() < 2 {
                    return;
                }
                let buried = self.editor.clone();
                self.cycle(false, window, cx);
                self.editors.retain(|e| *e != buried);
                self.editors.push(buried);
                cx.notify();
            }
            D::Scratch { project } => {
                let root = self.editor.read(cx).project();
                if project && root.is_none() {
                    status(self, tr!("msg-no-project"), true, cx);
                    return;
                }
                let root = if project { root } else { None };
                let Some(path) = kalem_core::command::scratch_path(root.as_deref()) else {
                    status(self, tr!("msg-no-state-dir"), true, cx);
                    return;
                };
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                self.open(&path, None, window, cx);
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
    /// The window's documents with a file, as a session.
    pub fn session(&self, cx: &App) -> kalem_core::sessions::Session {
        let mut s = kalem_core::sessions::Session::default();
        for e in &self.editors {
            let ed = e.read(cx);
            let doc = &ed.doc;
            let Some(path) = doc.meta.path.clone().filter(|_| doc.dired.is_none()) else {
                continue;
            };
            if *e == self.editor {
                s.active = s.documents.len();
                s.project = ed.project();
            }
            let t = doc.text();
            let line = t.line_of(doc.selection.head);
            s.documents.push(kalem_core::sessions::SessionDoc {
                path,
                line: line as u64 + 1,
                column: doc.selection.head - t.line_range(line).start,
            });
        }
        s
    }

    /// Opens the documents of `session`, showing its active one.
    pub fn restore_session(
        &mut self,
        session: kalem_core::sessions::Session,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let s = session.existing();
        for d in &s.documents {
            self.open(&d.path, Some((d.line, d.column)), window, cx);
        }
        if let Some(d) = s.documents.get(s.active) {
            self.open(&d.path, None, window, cx);
        }
        let msg = tr!("msg-session-restored", count = s.documents.len());
        self.editor.update(cx, |e, cx| {
            e.message(msg, false);
            cx.notify();
        });
    }

    /// Saves the session `SPC q l` restores.
    pub fn save_last_session(&self, cx: &App) {
        let session = self.session(cx);
        if !session.documents.is_empty() {
            let _ = kalem_core::sessions::save(kalem_core::sessions::LAST, &session);
        }
    }

    /// The documents with unsaved changes: this window's, and with `all`
    /// the other windows' too.
    fn modified(&self, all: bool, window: &Window, cx: &App) -> Vec<Entity<Editor>> {
        let mut modified: Vec<Entity<Editor>> = self
            .editors
            .iter()
            .filter(|e| e.read(cx).doc.is_modified())
            .cloned()
            .collect();
        let me = window.window_handle();
        for w in cx.windows() {
            if w == me || !all {
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
        modified
    }

    /// Quits after a confirmation, losing the unsaved changes (`SPC q Q`).
    pub fn quit_without_saving(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let n = self.modified(true, window, cx).len();
        if n == 0 {
            cx.quit();
            return;
        }
        let (quit, cancel) = (tr!("dialog-quit"), tr!("dialog-cancel"));
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &tr!("dialog-quit-discard", count = n.to_string()),
            None,
            &[quit.as_str(), cancel.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(0) = answer.await {
                let _ = cx.update(|_, cx| cx.quit());
            }
        })
        .detach();
    }

    /// Closes the window, asking about its unsaved documents (`SPC q f`).
    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let modified = self.modified(false, window, cx);
        if modified.is_empty() {
            window.remove_window();
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
            let _ = this.update_in(cx, |ws, window, cx| match a {
                Ok(0) => {
                    let all = modified
                        .iter()
                        .all(|e| e.update(cx, |e, _| e.save_quietly()));
                    if all {
                        window.remove_window();
                    } else {
                        ws.editor.update(cx, |e, cx| {
                            e.status = Some((tr!("msg-not-saved", reason = tr!("untitled")), true));
                            cx.notify();
                        });
                    }
                }
                Ok(1) => window.remove_window(),
                _ => {}
            });
        })
        .detach();
    }

    /// Starts Kalem again, with the open documents when `restore`
    /// (`SPC q r`, `SPC q R`); refused while documents are unsaved.
    pub fn restart(&mut self, restore: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        let error = if !self.modified(true, window, cx).is_empty() {
            Some(tr!("msg-restart-unsaved"))
        } else if restore {
            kalem_core::sessions::restore_on_next_start().err()
        } else {
            None
        };
        if let Some(e) = error {
            self.editor.update(cx, |ed, cx| {
                ed.message(e, true);
                cx.notify();
            });
            return;
        }
        self.save_last_session(cx);
        cx.restart();
    }

    pub fn quit(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let modified = self.modified(true, window, cx);
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
            DocEvent::InsertLink(path) => self.insert_link(&path, window, cx),
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
            DocEvent::Documents(r) => self.documents(r, window, cx),
            DocEvent::SearchLines {
                all,
                headings,
                text,
            } => self.search_lines(all, headings, &text, cx),
            DocEvent::Jump { doc, at } => {
                if let Some(e) = self.editors.get(doc).cloned() {
                    self.activate(e.clone(), window, cx);
                    e.update(cx, |e, cx| {
                        e.doc.move_cursor(at, false);
                        e.after_change(cx);
                    });
                }
            }
            DocEvent::Quit => self.quit(window, cx),
            DocEvent::Pane(op) => self.pane_op(&op, window, cx),
            DocEvent::Workspace(op) => self.workspace_op(op, window, cx),
            DocEvent::QuitWithoutSaving => self.quit_without_saving(window, cx),
            DocEvent::CloseWindow => self.close_window(window, cx),
            DocEvent::Choose(items) => self.editor.update(cx, |e, cx| e.open_choice(items, cx)),
            DocEvent::Notice(text, error) => self.editor.update(cx, |e, cx| {
                e.message(text, error);
                cx.notify();
            }),
            DocEvent::Restart(restore) => self.restart(restore, window, cx),
            DocEvent::SaveSession(name) => {
                let msg = match kalem_core::sessions::save(&name, &self.session(cx)) {
                    Ok(p) => (
                        tr!("msg-session-saved", path = p.display().to_string()),
                        false,
                    ),
                    Err(e) => (e, true),
                };
                self.editor.update(cx, |e, cx| {
                    e.message(msg.0, msg.1);
                    cx.notify();
                });
            }
            DocEvent::RestoreSession(name) => match kalem_core::sessions::load(&name) {
                Ok(s) => self.restore_session(s, window, cx),
                Err(e) => self.editor.update(cx, |ed, cx| {
                    ed.message(e, true);
                    cx.notify();
                }),
            },
            DocEvent::FileManager { place, select } => self.file_manager(place, select, window, cx),
            DocEvent::LeaveFileManager => self.leave_file_manager(window, cx),
            DocEvent::FilesChanged {
                message,
                error,
                outcome,
            } => {
                if let Some((kind, out)) = &outcome {
                    self.follow_files(*kind, out, window, cx);
                }
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
        // The current project's folders and files; not beside the file
        // manager (or the projects), which lists them itself.
        let lists_files = self.editor.read(cx).doc.dired.is_some();
        if !top && !lists_files && self.shared.config.bool("ui.folder_tree") {
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
        // The file manager and the projects, one click away; the button of
        // the view shown is pressed, and pressing it again keeps it.
        let place = self
            .editor
            .read(cx)
            .doc
            .dired
            .as_deref()
            .map(|d| d.place == kalem_core::dired::Place::Projects);
        bar = bar
            .child(self.command_button(
                "tool-files",
                kalem_core::l10n::tr("menu-file-manager").into(),
                "dired.jump",
                r#"{"show":true}"#,
                place == Some(false),
                cx,
            ))
            .child(self.command_button(
                "tool-projects",
                kalem_core::l10n::tr("menu-projects-view").into(),
                "dired.projects",
                r#"{"show":true}"#,
                place == Some(true),
                cx,
            ))
            .child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(theme.border));
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

    /// Runs `id` with `args` in the active editor, then gives it the focus.
    fn run_active(
        &mut self,
        id: &str,
        args: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let editor = self.editor.clone();
        editor.update(cx, |e, cx| e.run_command(id, args, window, cx));
        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// A toolbar button that runs `id` with `args`, drawn pressed when
    /// `pressed`.
    fn command_button(
        &self,
        name: &'static str,
        label: SharedString,
        id: &'static str,
        args: &'static str,
        pressed: bool,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(name)
            .debug_selector(move || name.to_string())
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .cursor_pointer()
            .when(pressed, |d| d.bg(gpui::hsla(0., 0., 0.5, 0.25)))
            .hover(|s| s.bg(gpui::hsla(0., 0., 0.5, 0.15)))
            .child(label)
            .on_click(cx.listener(move |ws, _, window, cx| {
                let args = serde_json::from_str(args).unwrap_or(serde_json::Value::Null);
                ws.run_active(id, args, window, cx);
            }))
    }

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
        let kind = match kalem_core::kinds::file_kind(&e.doc) {
            Some(_) => format!("{}   ", kalem_core::l10n::tr("kind-org")),
            None => String::new(),
        };
        let encoding = kalem_core::files::encoding_label(&e.doc.meta)
            .map(|n| format!("{n}   "))
            .unwrap_or_default();
        let space = if self.spaces.several() {
            format!("[{}]   ", self.spaces.current().name)
        } else {
            String::new()
        };
        let left = format!(
            "{mode}{space}{project}{name}   {kind}{encoding}{state}   {position}{words}{formula}{table}"
        );
        let (msg, error) = match self.shared.jobs.borrow().first() {
            // A file operation running: its progress.
            Some(j) => (j.status(), false),
            None => e.status.clone().unwrap_or_default(),
        };
        // The plugins' items, a click running an item's command.
        let (left_items, right_items) = kalem_core::extensions::status_items();
        let accent = theme.caret;
        let item =
            |n: usize, it: kalem_core::extensions::StatusItem, cx: &mut Context<'_, Self>| {
                let el = div()
                    .id(("plugin-status", n))
                    .text_color(accent)
                    .child(SharedString::from(it.text));
                match it.command {
                    Some(c) => el
                        .cursor_pointer()
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            let e = ws.editor.clone();
                            let c = c.clone();
                            e.update(cx, |e, cx| {
                                e.run_command(&c, serde_json::Value::Null, window, cx);
                            });
                        }))
                        .into_any_element(),
                    None => el.into_any_element(),
                }
            };
        let n = left_items.len();
        let lefts: Vec<_> = left_items
            .into_iter()
            .enumerate()
            .map(|(i, it)| item(i, it, cx))
            .collect();
        let rights: Vec<_> = right_items
            .into_iter()
            .rev()
            .enumerate()
            .map(|(i, it)| item(n + i, it, cx))
            .collect();
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
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(16.))
                    .child(left)
                    .children(lefts),
            )
            .child(
                div().flex().flex_row().gap(px(16.)).children(rights).child(
                    div()
                        .text_color(if error { theme.todo } else { theme.muted })
                        .child(msg),
                ),
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
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .child(if self.layout.is_split() {
                                        self.pane_element(&self.layout.root().clone(), &theme, cx)
                                    } else {
                                        self.editor.clone().into_any_element()
                                    }),
                            ),
                    ),
            )
            .child(self.status_bar(&theme, cx))
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
                #[expect(
                    clippy::expect_used,
                    reason = "an empty document reads no file and cannot fail"
                )]
                let e = crate::editor::open(None, shared.clone(), theme, cx)
                    .expect("an empty document");
                // A file with a password (a PDF): asked for in the palette.
                if err == tr!("msg-needs-password")
                    && let Some(p) = path.as_deref()
                {
                    let title = tr!("cmd-file-openWithPassword");
                    let args = serde_json::json!({ "path": p.display().to_string() });
                    e.update(cx, |e, cx| {
                        e.ask_argument(
                            "file.openWithPassword",
                            &title,
                            args,
                            "password".into(),
                            "string".into(),
                            cx,
                        );
                    });
                } else {
                    e.update(cx, |e, _| e.status = Some((err, true)));
                }
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

/// The plugins' commands or keys changed: commands and keys built again
/// for every window, as after the settings change.
pub fn plugins_changed(cx: &mut App) {
    let Some(old) = cx
        .windows()
        .into_iter()
        .find_map(|w| w.downcast::<Workspace>())
        .and_then(|w| w.read(cx).ok().map(|ws| ws.shared.clone()))
    else {
        return;
    };
    let new = crate::preferences::rebuild(&old, old.config.clone());
    crate::preferences::apply(Rc::new(new), cx);
}

/// Runs the commands plugins asked for outside their own (from an
/// event's handler) in the active window's editor.
pub fn run_queued(runs: Vec<(String, serde_json::Value)>, cx: &mut App) {
    let Some(w) = cx
        .active_window()
        .and_then(|w| w.downcast::<Workspace>())
        .or_else(|| cx.windows().iter().find_map(|w| w.downcast::<Workspace>()))
    else {
        return;
    };
    let _ = w.update(cx, |ws, window, cx| {
        let editor = ws.editor.clone();
        editor.update(cx, |e, cx| {
            for (id, args) in runs {
                e.run_command(&id, args, window, cx);
            }
        });
    });
}

/// Asks the plugins' questions in the active window's editor.
pub fn ask_queued(asked: Vec<kalem_core::Request>, cx: &mut App) {
    let Some(w) = cx
        .active_window()
        .and_then(|w| w.downcast::<Workspace>())
        .or_else(|| cx.windows().iter().find_map(|w| w.downcast::<Workspace>()))
    else {
        return;
    };
    let _ = w.update(cx, |ws, window, cx| {
        let editor = ws.editor.clone();
        editor.update(cx, |e, cx| {
            for r in asked {
                e.request(r, window, cx);
            }
        });
    });
}

/// Saves the active window's session as the last one, on quitting.
pub fn save_last_session(cx: &mut App) {
    let mut windows: Vec<gpui::AnyWindowHandle> = cx.active_window().into_iter().collect();
    windows.extend(cx.windows());
    for w in windows {
        if let Some(w) = w.downcast::<Workspace>()
            && let Ok(ws) = w.read(cx)
        {
            ws.save_last_session(cx);
            return;
        }
    }
}

/// Opens the last session in the first window, when the settings say so
/// (`editor.restore_session`).
pub fn restore_last_session(shared: &Shared, cx: &mut App) {
    let asked = kalem_core::sessions::take_restore_request();
    if !asked && !shared.config.bool("editor.restore_session") {
        return;
    }
    let Ok(session) = kalem_core::sessions::load(kalem_core::sessions::LAST) else {
        return;
    };
    let Some(w) = cx
        .windows()
        .into_iter()
        .find_map(|w| w.downcast::<Workspace>())
    else {
        return;
    };
    let _ = w.update(cx, |ws, window, cx| ws.restore_session(session, window, cx));
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
