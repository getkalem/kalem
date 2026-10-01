//! Projects and open documents as the frontends show them (design §2.8,
//! D12): the project list with its files, the list of open files grouped
//! by project, and the items of the pickers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kalem_project::{FileIndex, Projects};

pub use kalem_project::list::normal;
pub use kalem_project::{Hit, Project, Query};

use crate::command::PickKind;
use crate::palette::PaletteItem;

/// The file the project list is kept in: `projects.toml` in the user's
/// configuration folder.
pub fn list_file() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("projects.toml"))
}

/// The name of the project holding `path`, from the saved list.
pub fn current_name(path: &Path) -> Option<String> {
    Projects::load(list_file())
        .containing(path)
        .map(|p| p.name.clone())
}

/// The project list and the files of the projects in use.
#[derive(Debug, Default)]
pub struct ProjectState {
    /// The list.
    pub list: Projects,
    indexes: HashMap<PathBuf, FileIndex>,
    trees: HashMap<PathBuf, FolderTree>,
}

impl ProjectState {
    /// The list kept in `file` (see [`list_file`]).
    pub fn load(file: Option<PathBuf>) -> ProjectState {
        ProjectState {
            list: Projects::load(file),
            indexes: HashMap::new(),
            trees: HashMap::new(),
        }
    }

    /// Saves the list; the error as a message.
    pub fn save(&self) -> Result<(), String> {
        self.list
            .save()
            .map_err(|e| crate::tr!("msg-cannot-save-projects", error = e.to_string()))
    }

    /// The project holding `path`.
    pub fn containing(&self, path: Option<&Path>) -> Option<&Project> {
        path.and_then(|p| self.list.containing(p))
    }

    /// The files of the project at `root` found so far (relative), and
    /// whether the walk is done. The walk starts on first use.
    pub fn files(&mut self, root: &Path) -> (Arc<Vec<PathBuf>>, bool) {
        let ignore = self
            .list
            .get(root)
            .map(|p| p.ignore.clone())
            .unwrap_or_default();
        self.indexes
            .entry(root.to_path_buf())
            .or_insert_with(|| FileIndex::new(root, &ignore))
            .files()
    }

    /// The lines of the folder tree of the project at `root`: its folders
    /// and files as its file index lists them (ignored files left out),
    /// open folders' contents below them.
    pub fn tree_rows(&mut self, root: &Path) -> Vec<TreeRow> {
        let (files, _) = self.files(root);
        let tree = self
            .trees
            .entry(root.to_path_buf())
            .or_insert_with(|| FolderTree::new(root));
        tree.update(&files);
        tree.rows()
    }

    /// Opens or closes folder `path` in the tree of the project at `root`.
    pub fn toggle_tree(&mut self, root: &Path, path: &Path) {
        self.trees
            .entry(root.to_path_buf())
            .or_insert_with(|| FolderTree::new(root))
            .toggle(path);
    }

    /// Opens the folders of the tree of the project at `root` down to
    /// `path`.
    pub fn reveal_in_tree(&mut self, root: &Path, path: &Path) {
        self.trees
            .entry(root.to_path_buf())
            .or_insert_with(|| FolderTree::new(root))
            .reveal(path);
    }

    /// Walks the project at `root` again.
    pub fn refresh(&mut self, root: &Path) {
        if let Some(i) = self.indexes.get(root) {
            i.refresh();
        }
    }

    /// The document at `path` became the active one: its project counts
    /// as switched to (first in Switch Project, its last file this one),
    /// and its files start being listed, so Find File and Search in
    /// Project are ready. With `auto_add`, a folder under version control
    /// (or with a `.projectile` file) holding a file outside every project
    /// becomes a project first, as Projectile does; the message says so.
    pub fn entered(&mut self, path: Option<&Path>, auto_add: bool) -> Option<String> {
        let path = path?;
        let mut message = None;
        if auto_add
            && self.list.containing(path).is_none()
            && let Some(root) = kalem_project::list::detect_root(path)
            && let Ok(m) = self.add(&root)
        {
            message = Some(m);
        }
        let root = self.list.containing(path)?.root.clone();
        let file = path.is_file().then(|| normal(path));
        self.list.used(&root);
        if let Some(f) = file
            && let Some(p) = self.list.list.iter_mut().find(|p| p.root == root)
        {
            p.last_file = Some(f);
        }
        let _ = self.files(&root);
        if let Err(e) = self.save() {
            tracing::warn!("{e}");
        }
        message
    }

    /// Notes that `path` was opened, and saves the list.
    pub fn opened(&mut self, path: &Path) {
        self.list.opened(path);
        if let Err(e) = self.save() {
            tracing::warn!("{e}");
        }
    }

    /// Adds `root` as a project, and saves the list; a message either way.
    pub fn add(&mut self, root: &Path) -> Result<String, String> {
        let i = self.list.add(root)?;
        self.save()?;
        Ok(crate::tr!(
            "msg-project-added",
            name = self.list.list[i].name.clone()
        ))
    }

    /// Removes the project at `root`, and saves the list.
    pub fn remove(&mut self, root: &Path) -> Result<String, String> {
        let name = self
            .list
            .get(root)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        if !self.list.remove(root) {
            return Err(crate::tr!("msg-no-project"));
        }
        self.indexes.remove(root);
        self.save()?;
        Ok(crate::tr!("msg-project-removed", name = name))
    }

    /// Renames the project at `root`, and saves the list.
    pub fn rename(&mut self, root: &Path, name: &str) -> Result<String, String> {
        if !self.list.rename(root, name) {
            return Err(crate::tr!("msg-no-project"));
        }
        self.save()?;
        Ok(crate::tr!("msg-project-renamed", name = name.trim()))
    }
}

/// An open document, as the list of open files and the pickers show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFile {
    /// Its file, if saved.
    pub path: Option<PathBuf>,
    /// Its title: the file name, or "Untitled".
    pub title: String,
    /// It has unsaved changes.
    pub modified: bool,
    /// It belongs to another workspace: left out of the list and the
    /// cycle (T2.7i.15).
    pub hidden: bool,
}

/// A line of a project's folder tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    /// The file or folder.
    pub path: PathBuf,
    /// Its name.
    pub name: String,
    /// How deep it is: 0 for the project's own entries.
    pub depth: usize,
    /// A folder.
    pub dir: bool,
    /// An open folder.
    pub open: bool,
}

/// A project's folders and files as a tree whose folders open and close,
/// built from the project's file index.
#[derive(Debug, Clone, Default)]
pub struct FolderTree {
    root: PathBuf,
    /// Open folders, relative to the root.
    open: std::collections::BTreeSet<PathBuf>,
    /// Each folder's folders and files, sorted, by relative path.
    children: HashMap<PathBuf, (Vec<String>, Vec<String>)>,
    /// The file list the children come from.
    source: Option<Arc<Vec<PathBuf>>>,
}

impl FolderTree {
    /// The tree of the project at `root`, all folders closed.
    pub fn new(root: &Path) -> FolderTree {
        FolderTree {
            root: root.to_path_buf(),
            ..FolderTree::default()
        }
    }

    /// Takes the project's files (relative paths) when they changed.
    pub fn update(&mut self, files: &Arc<Vec<PathBuf>>) {
        if self.source.as_ref().is_some_and(|s| Arc::ptr_eq(s, files)) {
            return;
        }
        let mut children: HashMap<PathBuf, (Vec<String>, Vec<String>)> = HashMap::new();
        for f in files.iter() {
            let rel = f.strip_prefix(&self.root).unwrap_or(f);
            let mut parent = PathBuf::new();
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            for (i, name) in parts.iter().enumerate() {
                let entry = children.entry(parent.clone()).or_default();
                let list = if i + 1 == parts.len() {
                    &mut entry.1
                } else {
                    &mut entry.0
                };
                if !list.contains(name) {
                    list.push(name.clone());
                }
                parent.push(name);
            }
        }
        for (dirs, files) in children.values_mut() {
            dirs.sort_by(|a, b| kalem_fs::natural(a, b));
            files.sort_by(|a, b| kalem_fs::natural(a, b));
        }
        self.children = children;
        self.source = Some(files.clone());
    }

    /// The visible lines: folders first, then files, open folders'
    /// contents under them.
    pub fn rows(&self) -> Vec<TreeRow> {
        let mut out = Vec::new();
        self.push_rows(Path::new(""), 0, &mut out);
        out
    }

    fn push_rows(&self, rel: &Path, depth: usize, out: &mut Vec<TreeRow>) {
        let Some((dirs, files)) = self.children.get(rel) else {
            return;
        };
        for d in dirs {
            let r = rel.join(d);
            let open = self.open.contains(&r);
            out.push(TreeRow {
                path: self.root.join(&r),
                name: d.clone(),
                depth,
                dir: true,
                open,
            });
            if open {
                self.push_rows(&r, depth + 1, out);
            }
        }
        for f in files {
            out.push(TreeRow {
                path: self.root.join(rel).join(f),
                name: f.clone(),
                depth,
                dir: false,
                open: false,
            });
        }
    }

    /// Opens folder `path` if it is closed, closes it if it is open.
    pub fn toggle(&mut self, path: &Path) {
        let rel = path.strip_prefix(&self.root).unwrap_or(path).to_path_buf();
        if !self.open.remove(&rel) {
            self.open.insert(rel);
        }
    }

    /// Opens the folders down to `path`.
    pub fn reveal(&mut self, path: &Path) {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return;
        };
        let mut p = PathBuf::new();
        let parts: Vec<_> = rel.components().collect();
        for c in parts.iter().take(parts.len().saturating_sub(1)) {
            p.push(c);
            self.open.insert(p.clone());
        }
    }
}

/// A line of the list of open files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A project with open files; its files follow.
    Project {
        /// Its name.
        name: String,
        /// Its folder.
        root: PathBuf,
    },
    /// An open document: its index among the open documents, and whether
    /// it belongs to the project above.
    File {
        /// The document's index.
        index: usize,
        /// Under a project.
        nested: bool,
    },
}

/// The list of open files: each project with open files, its files under
/// it (in the order they were opened), projects in the order their first
/// file was opened; then the files outside every project, one by one.
pub fn entries(files: &[OpenFile], projects: &Projects) -> Vec<Entry> {
    let mut groups: Vec<(PathBuf, String, Vec<usize>)> = Vec::new();
    let mut loose = Vec::new();
    for (i, f) in files.iter().enumerate() {
        if f.hidden {
            continue;
        }
        match f.path.as_deref().and_then(|p| projects.containing(p)) {
            Some(p) => match groups.iter_mut().find(|g| g.0 == p.root) {
                Some(g) => g.2.push(i),
                None => groups.push((p.root.clone(), p.name.clone(), vec![i])),
            },
            None => loose.push(i),
        }
    }
    let mut out = Vec::new();
    for (root, name, docs) in groups {
        out.push(Entry::Project { name, root });
        out.extend(docs.into_iter().map(|index| Entry::File {
            index,
            nested: true,
        }));
    }
    out.extend(loose.into_iter().map(|index| Entry::File {
        index,
        nested: false,
    }));
    out
}

/// The open documents in the order the list of open files shows them.
pub fn order(files: &[OpenFile], projects: &Projects) -> Vec<usize> {
    entries(files, projects)
        .into_iter()
        .filter_map(|e| match e {
            Entry::File { index, .. } => Some(index),
            Entry::Project { .. } => None,
        })
        .collect()
}

/// A path for display: `~` for the home folder.
pub fn tilde(path: &Path) -> String {
    let s = path.display().to_string();
    match std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        Some(h) => {
            let h = h.to_string_lossy().into_owned();
            match s.strip_prefix(&h) {
                Some(rest) if rest.is_empty() || rest.starts_with(std::path::MAIN_SEPARATOR) => {
                    format!("~{rest}")
                }
                _ => s,
            }
        }
        None => s,
    }
}

/// What choosing a project in the project list leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Its last file, or its file picker (Switch Project).
    Open,
    /// Another list about that project.
    Pick(PickKind),
    /// Searching its files.
    Search,
    /// Its folder in the file manager.
    Browse,
}

/// What a picker shows and what choosing an item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    /// For the project list: what choosing a project leads to.
    pub after: After,
    /// The list.
    pub kind: PickKind,
    /// The project it is about, for project lists.
    pub project: Option<PathBuf>,
    /// The prompt.
    pub prompt: String,
    /// The items: `id` is what choosing gives (a document's index, a
    /// path), `title` what is matched, `category` a detail, `keys` a mark.
    pub items: Vec<PaletteItem>,
    /// More items are coming (a project walk in progress).
    pub partial: bool,
}

/// The items of picker `kind`. `current` is the active document's file;
/// `project` the project a project list is about (else the current one).
/// `None` when the list needs a project and there is none: then the
/// frontend offers the projects first.
pub fn picker(
    kind: PickKind,
    files: &[OpenFile],
    current: Option<&Path>,
    project: Option<&Path>,
    state: &mut ProjectState,
) -> Option<Picker> {
    let project_root = project
        .map(Path::to_path_buf)
        .or_else(|| state.containing(current).map(|p| p.root.clone()));
    let item = |id: String, title: String, category: String, keys: String| PaletteItem {
        id,
        title,
        category,
        keys,
        also: String::new(),
    };
    let (prompt, items, partial, project) = match kind {
        PickKind::Documents | PickKind::ProjectDocuments => {
            let root = match kind {
                PickKind::ProjectDocuments => Some(project_root.clone()?),
                _ => None,
            };
            let items = order(files, &state.list)
                .into_iter()
                .filter(|&i| {
                    root.as_ref()
                        .is_none_or(|r| files[i].path.as_ref().is_some_and(|p| p.starts_with(r)))
                })
                .map(|i| {
                    let f = &files[i];
                    let detail = f
                        .path
                        .as_deref()
                        .and_then(Path::parent)
                        .map(tilde)
                        .unwrap_or_default();
                    item(
                        i.to_string(),
                        f.title.clone(),
                        detail,
                        if f.modified {
                            "●".into()
                        } else {
                            String::new()
                        },
                    )
                })
                .collect();
            (crate::l10n::tr("pick-documents"), items, false, root)
        }
        PickKind::RecentFiles => {
            let items = state
                .list
                .recent
                .iter()
                .filter(|p| current != Some(p.as_path()))
                .map(|p| {
                    let name = p
                        .file_name()
                        .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into());
                    let dir = p.parent().map(tilde).unwrap_or_default();
                    item(p.display().to_string(), name, dir, String::new())
                })
                .collect();
            (crate::l10n::tr("pick-recent"), items, false, None)
        }
        PickKind::Projects | PickKind::RemoveProject => {
            let items = state
                .list
                .by_use()
                .into_iter()
                .map(|p| {
                    let mark = if p.exists() {
                        String::new()
                    } else {
                        crate::l10n::tr("project-missing")
                    };
                    item(
                        p.root.display().to_string(),
                        p.name.clone(),
                        tilde(&p.root),
                        mark,
                    )
                })
                .collect();
            let prompt = if kind == PickKind::Projects {
                "pick-projects"
            } else {
                "pick-remove-project"
            };
            (crate::l10n::tr(prompt), items, false, None)
        }
        PickKind::ProjectFiles => {
            let root = project_root?;
            let (files, done) = state.files(&root);
            let p = state.list.get(&root)?;
            // Recently opened files first, then the rest in order.
            let recent: Vec<String> = p.recent.iter().map(|f| p.relative(f)).collect();
            let mut items: Vec<PaletteItem> = recent
                .iter()
                .filter(|r| root.join(r).is_file())
                .map(|r| {
                    item(
                        root.join(r).display().to_string(),
                        r.clone(),
                        String::new(),
                        String::new(),
                    )
                })
                .collect();
            for f in files.iter() {
                let rel = f
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if !recent.contains(&rel) {
                    items.push(item(
                        root.join(f).display().to_string(),
                        rel,
                        String::new(),
                        String::new(),
                    ));
                }
            }
            let prompt = crate::tr!("pick-project-files", project = p.name.clone());
            (prompt, items, !done, Some(root))
        }
        PickKind::ProjectRecentFiles => {
            let root = project_root?;
            let p = state.list.get(&root)?;
            let items = p
                .recent
                .iter()
                .filter(|f| current != Some(f.as_path()))
                .map(|f| {
                    item(
                        f.display().to_string(),
                        p.relative(f),
                        String::new(),
                        String::new(),
                    )
                })
                .collect();
            let prompt = crate::tr!("pick-project-recent", project = p.name.clone());
            (prompt, items, false, Some(root))
        }
    };
    Some(Picker {
        after: After::Open,
        kind,
        project,
        prompt,
        items,
        partial,
    })
}

/// A search through a project's files, restarted as the query changes.
#[derive(Debug)]
pub struct ProjectSearch {
    /// The project's folder.
    pub root: PathBuf,
    /// Its name.
    pub name: String,
    ignore: Vec<String>,
    /// The query and its switches.
    pub query: kalem_project::Query,
    running: Option<kalem_project::Search>,
    /// The matching lines so far.
    pub hits: Vec<kalem_project::Hit>,
    /// The search is done.
    pub done: bool,
    /// Why the query is not valid.
    pub error: Option<String>,
    changed: Option<std::time::Instant>,
}

/// Searches start this long after the last change of the query.
const SEARCH_DELAY: std::time::Duration = std::time::Duration::from_millis(120);

impl ProjectSearch {
    /// A search in the project at `root`, with `text` (the selection, say).
    pub fn new(project: &Project, text: &str) -> ProjectSearch {
        let mut s = ProjectSearch {
            root: project.root.clone(),
            name: project.name.clone(),
            ignore: project.ignore.clone(),
            query: kalem_project::Query::default(),
            running: None,
            hits: Vec::new(),
            done: true,
            error: None,
            changed: None,
        };
        s.set_text(text);
        s
    }

    /// Changes the query's text.
    pub fn set_text(&mut self, text: &str) {
        if self.query.text != text {
            self.query.text = text.to_string();
            self.changed = Some(std::time::Instant::now());
        }
    }

    /// Toggles a switch: `c` case, `w` whole words, `r` regular expression.
    pub fn toggle(&mut self, switch: char) {
        match switch {
            'c' => self.query.case_sensitive = !self.query.case_sensitive,
            'w' => self.query.whole_word = !self.query.whole_word,
            'r' => self.query.regex = !self.query.regex,
            _ => return,
        }
        self.changed = Some(std::time::Instant::now() - SEARCH_DELAY);
    }

    /// Starts the search once the query rests, and takes new results;
    /// whether anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(at) = self.changed
            && at.elapsed() >= SEARCH_DELAY
        {
            self.changed = None;
            self.hits.clear();
            self.error = None;
            self.running = (!self.query.text.is_empty()).then(|| {
                kalem_project::Search::start(&self.root, &self.ignore, self.query.clone())
            });
            self.done = self.running.is_none();
            changed = true;
        }
        if let Some(r) = &self.running {
            let (new, done, error) = r.hits(self.hits.len());
            changed |= !new.is_empty() || done != self.done;
            self.hits.extend(new);
            self.done = done;
            self.error = error;
            if done {
                self.hits
                    .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
                self.running = None;
            }
        }
        changed
    }

    /// A search is waiting to start or running.
    pub fn busy(&self) -> bool {
        self.changed.is_some() || !self.done
    }

    /// Waits for the search (for tests).
    pub fn wait(&mut self) {
        if let Some(at) = self.changed {
            self.changed = Some(at - SEARCH_DELAY);
        }
        self.poll();
        while !self.done {
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.poll();
        }
    }

    /// Stops the search.
    pub fn cancel(&mut self) {
        if let Some(r) = self.running.take() {
            r.cancel();
        }
    }

    /// A hit as a line: the file (relative) and line number, and the text.
    pub fn line(&self, hit: &kalem_project::Hit) -> (String, String) {
        let rel = hit.path.strip_prefix(&self.root).unwrap_or(&hit.path);
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        (format!("{rel}:{}", hit.line), hit.text.trim().to_string())
    }

    /// The status: the number of matches, searching, or the error.
    pub fn status(&self) -> String {
        if let Some(e) = &self.error {
            return crate::tr!("search-error", error = e.clone());
        }
        let n = crate::tr!(
            "search-results",
            count = crate::l10n::number(self.hits.len())
        );
        if self.done && self.changed.is_none() {
            n
        } else {
            format!("{n}, {}", crate::l10n::tr("pick-searching"))
        }
    }

    /// The switches as shown: `Aa W .*`, the active ones marked.
    pub fn switches(&self) -> Vec<(String, bool)> {
        vec![
            (crate::l10n::tr("search-case"), self.query.case_sensitive),
            (crate::l10n::tr("search-word"), self.query.whole_word),
            (crate::l10n::tr("search-regex"), self.query.regex),
        ]
    }
}

/// The items of `picker` matching `input`, best first. File lists match
/// paths (file names count most); the others match titles, then details.
pub fn matches<'a>(picker: &'a Picker, input: &str) -> Vec<&'a PaletteItem> {
    if input.trim().is_empty() {
        return picker.items.iter().collect();
    }
    let paths = matches!(
        picker.kind,
        PickKind::ProjectFiles | PickKind::ProjectRecentFiles
    );
    let mut scored: Vec<(i64, usize, &PaletteItem)> = picker
        .items
        .iter()
        .enumerate()
        .filter_map(|(n, it)| {
            let s = if paths {
                kalem_project::fuzzy_path(input, &it.title)
            } else {
                kalem_project::fuzzy(input, &it.title)
                    .or_else(|| kalem_project::fuzzy(input, &it.category).map(|s| s + 10))
            };
            s.map(|s| (s, n, it))
        })
        .collect();
    scored.sort_by_key(|a| (a.0, a.1));
    scored.into_iter().map(|(_, _, it)| it).collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn folder_tree() {
        let root = Path::new("/p");
        let files: Arc<Vec<PathBuf>> = Arc::new(
            [
                "b.org",
                "a10.txt",
                "a2.txt",
                "src/main.rs",
                "src/util/x.rs",
                "doc/manual.org",
            ]
            .iter()
            .map(PathBuf::from)
            .collect(),
        );
        let mut t = FolderTree::new(root);
        t.update(&files);
        let names = |t: &FolderTree| {
            t.rows()
                .iter()
                .map(|r| {
                    format!(
                        "{}{}{}",
                        "  ".repeat(r.depth),
                        r.name,
                        if r.dir { "/" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&t), ["doc/", "src/", "a2.txt", "a10.txt", "b.org"]);
        t.toggle(&root.join("src"));
        assert_eq!(
            names(&t),
            [
                "doc/",
                "src/",
                "  util/",
                "  main.rs",
                "a2.txt",
                "a10.txt",
                "b.org"
            ]
        );
        assert!(t.rows()[1].open && t.rows()[2].path == root.join("src/util"));
        t.toggle(&root.join("src"));
        assert_eq!(names(&t).len(), 5);
        t.reveal(&root.join("src/util/x.rs"));
        assert!(
            names(&t).contains(&"    x.rs".to_string()),
            "{:?}",
            names(&t)
        );
    }

    use super::*;

    #[test]
    fn open_files_by_project() {
        let base = std::env::temp_dir().join(format!("kalem-core-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["notes", "code/sub"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        let base = kalem_project::list::normal(&base);
        let mut projects = Projects::default();
        projects.add(&base.join("notes")).unwrap();
        projects.add(&base.join("code")).unwrap();
        let file = |p: Option<&str>| OpenFile {
            path: p.map(|p| base.join(p)),
            title: p.unwrap_or("Untitled").into(),
            modified: false,
            hidden: false,
        };
        let files = [
            file(Some("loose.org")),
            file(Some("code/sub/a.rs")),
            file(Some("notes/b.org")),
            file(None),
            file(Some("code/c.md")),
        ];
        let e = entries(&files, &projects);
        assert_eq!(
            e,
            vec![
                Entry::Project {
                    name: "code".into(),
                    root: base.join("code")
                },
                Entry::File {
                    index: 1,
                    nested: true
                },
                Entry::File {
                    index: 4,
                    nested: true
                },
                Entry::Project {
                    name: "notes".into(),
                    root: base.join("notes")
                },
                Entry::File {
                    index: 2,
                    nested: true
                },
                Entry::File {
                    index: 0,
                    nested: false
                },
                Entry::File {
                    index: 3,
                    nested: false
                },
            ]
        );
        assert_eq!(order(&files, &projects), vec![1, 4, 2, 0, 3]);
        let mut state = ProjectState {
            list: projects,
            ..ProjectState::default()
        };
        let current = base.join("notes/b.org");
        std::fs::write(&current, "x").unwrap();
        std::fs::write(base.join("notes/c.org"), "y").unwrap();
        let docs = picker(
            PickKind::ProjectDocuments,
            &files,
            Some(&current),
            None,
            &mut state,
        )
        .unwrap();
        assert_eq!(docs.items.len(), 1);
        // The project's files, once walked.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let files_picker = loop {
            let p = picker(
                PickKind::ProjectFiles,
                &files,
                Some(&current),
                None,
                &mut state,
            )
            .unwrap();
            if !p.partial || std::time::Instant::now() > deadline {
                break p;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let titles: Vec<&str> = files_picker
            .items
            .iter()
            .map(|i| i.title.as_str())
            .collect();
        assert_eq!(titles, ["b.org", "c.org"]);
        assert_eq!(matches(&files_picker, "co")[0].title, "c.org");
        // Outside every project, project lists need a project first.
        assert!(
            picker(
                PickKind::ProjectFiles,
                &files,
                Some(&base.join("loose.org")),
                None,
                &mut state
            )
            .is_none()
        );
        assert_eq!(tilde(Path::new("/nowhere")), "/nowhere");
    }
}
