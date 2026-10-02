//! Documents of several files (T2.7h.4): the root document of a file, and
//! the model of the whole project, the included files read where they are
//! included, so that numbers continue across them and labels, references
//! and citations resolve between them.
//!
//! Paths resolve as LaTeX resolves them: `\input` and `\include` from the
//! folder LaTeX runs in (the root document's; inside a file brought in by
//! `\import`, `\subimport` or `\subfile`, that file's folder, as those
//! packages arrange), `\import{folder}{file}` from the root's folder and
//! `\subimport` and `\subfile` from the including file's. A name without
//! an extension is tried with `.tex` first. A file included by `\subfile`
//! counts only inside its `document` environment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use latex_syntax::Parse;

use crate::Model;
use crate::extract::{self, Item};

/// Where the files of a project come from: the disk, or the editor's
/// open documents over it.
pub trait Files {
    /// The text of `path`, if it is a readable text file.
    fn read(&self, path: &Path) -> Option<String>;
    /// The text of `path`, shared: a reader that keeps texts gives the
    /// same allocation for a file that did not change, so that the
    /// project's cache sees at once that it did not.
    fn read_shared(&self, path: &Path) -> Option<Arc<str>> {
        self.read(path).map(Arc::from)
    }
    /// The files in the folder `dir`.
    fn list(&self, dir: &Path) -> Vec<PathBuf>;
}

/// The files on disk.
#[derive(Debug, Clone, Copy, Default)]
pub struct Disk;

impl Files for Disk {
    fn read(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|r| {
                r.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

/// `% !TEX root = FILE` among the first lines of `text`.
fn magic_root(text: &str) -> Option<String> {
    text.lines().take(20).find_map(|l| {
        let l = l.trim_start().strip_prefix('%')?.trim_start();
        let l = l.strip_prefix('!').unwrap_or(l).trim_start();
        let (key, value) = l.split_once('=')?;
        let key: Vec<String> = key.split_whitespace().map(str::to_lowercase).collect();
        (key == ["tex", "root"]).then(|| value.trim().to_string())
    })
}

/// `\documentclass[MAIN]{subfiles}`: the main document of a subfile.
fn subfiles_main(text: &str) -> Option<String> {
    let at = text.find("\\documentclass[")?;
    let rest = &text[at + "\\documentclass[".len()..];
    let (main, after) = rest.split_once(']')?;
    after
        .trim_start()
        .starts_with("{subfiles}")
        .then(|| main.trim().to_string())
}

/// `path` with `.` and `..` taken out, without asking the disk.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn with_tex(dir: &Path, name: &str) -> Vec<PathBuf> {
    let dir = normalize(dir);
    let p = normalize(&dir.join(name));
    if p.extension().is_some_and(|e| e == "tex") {
        vec![p]
    } else {
        vec![normalize(&dir.join(format!("{name}.tex"))), p]
    }
}

fn has_class(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim_start().starts_with("\\documentclass") && !l.contains("{subfiles}"))
}

/// The root document of `file` (whose text is `text`): `% !TEX root`,
/// the main document of a subfile, a `NAME.tex.latexmain` marker in its
/// folder or above, a root the caller names (`setting`; no setting
/// supplies one yet, T2.7h.4), the file itself
/// when it has a `\documentclass`, or the first document with one in its
/// folder or above (up to `top`) that includes it.
pub fn find_root(
    file: &Path,
    text: &str,
    files: &dyn Files,
    setting: Option<&Path>,
    top: Option<&Path>,
) -> PathBuf {
    // A relative path is searched from the current folder upwards, and
    // what is found under it given back relative.
    // (`/q/x.tex` is rooted on Windows too, on the current drive.)
    if !file.has_root()
        && let Ok(cwd) = std::env::current_dir()
    {
        let root = find_root(&cwd.join(file), text, files, setting, top);
        return root
            .strip_prefix(&cwd)
            .map_or(root.clone(), Path::to_path_buf);
    }
    let dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
    for name in magic_root(text).into_iter().chain(subfiles_main(text)) {
        if let Some(p) = with_tex(&dir, &name)
            .into_iter()
            .find(|p| files.read(p).is_some())
        {
            return p;
        }
    }
    let ancestors: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|a| top.is_none_or(|t| a.starts_with(t)))
        .map(Path::to_path_buf)
        .collect();
    for a in &ancestors {
        for f in files.list(a) {
            if let Some(main) = f
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".latexmain"))
            {
                return a.join(main);
            }
        }
    }
    if let Some(s) = setting {
        return s.to_path_buf();
    }
    if has_class(text) {
        return file.to_path_buf();
    }
    let mut cache = ProjectCache::default();
    for a in &ancestors {
        for f in files.list(a) {
            if f.extension().is_none_or(|e| e != "tex") || f == file {
                continue;
            }
            if files.read(&f).is_some_and(|t| has_class(&t))
                && cache.load(&f, files).model.files.iter().any(|p| p == file)
            {
                return f;
            }
        }
    }
    file.to_path_buf()
}

/// A project: the root document and its model with the included files.
#[derive(Debug, Clone)]
pub struct Project {
    /// The root document.
    pub root: PathBuf,
    /// The model; [`Model::files`] lists the files.
    pub model: Arc<Model>,
}

/// The parses and the per-paragraph events of each file, kept between
/// loads.
#[derive(Debug, Default)]
pub struct ProjectCache {
    /// Each file's text, parse, events cache and last events.
    files: HashMap<PathBuf, CachedFile>,
    /// The last project loaded, with the events of each of its files.
    last: Option<Loaded>,
}

/// A project as last loaded.
#[derive(Debug)]
struct Loaded {
    root: PathBuf,
    model: Arc<Model>,
    /// The events of each file, by its index in the model's files.
    items: Vec<Option<Arc<Vec<Item>>>>,
    /// How the model came from the one before.
    change: Change,
    /// How many times it was loaded.
    generation: u64,
    /// The model as seen from a file ([`ProjectCache::seen_from`]), and
    /// the load it was made for.
    seen: Option<(usize, u64, Arc<Model>)>,
}

/// How a project's model came from the one before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// Numbered again.
    New,
    /// The same.
    Same,
    /// The positions of one file moved.
    Moved(usize, extract::Region),
}

/// A file of a project as the cache keeps it.
#[derive(Debug)]
struct CachedFile {
    text: Arc<str>,
    parse: Parse,
    events: extract::Cache,
    /// The events of `parse`, while it stays the same.
    items: Option<Arc<Vec<Item>>>,
    /// The parse and events before the last change, to see what moved.
    prev: Option<(Parse, Arc<Vec<Item>>)>,
}

impl CachedFile {
    fn new() -> CachedFile {
        CachedFile {
            text: Arc::from(""),
            parse: latex_syntax::parse(""),
            events: extract::Cache::default(),
            items: None,
            prev: None,
        }
    }

    /// A new parse: the events of the old one kept to compare.
    fn replace(&mut self, text: Arc<str>, parse: Parse) {
        if let Some(items) = self.items.take() {
            self.prev = Some((std::mem::replace(&mut self.parse, parse), items));
        } else {
            self.parse = parse;
        }
        self.text = text;
    }
}

impl ProjectCache {
    /// The events of `path` with text `text`: from the cache while the text
    /// is the same allocation or the same bytes.
    fn items(&mut self, path: &Path, text: Arc<str>) -> Arc<Vec<Item>> {
        let entry = self
            .files
            .entry(path.to_path_buf())
            .or_insert_with(CachedFile::new);
        if !Arc::ptr_eq(&entry.text, &text) {
            if *entry.text != *text {
                let parse = latex_syntax::parse(&text);
                entry.replace(text, parse);
            } else {
                entry.text = text;
            }
        }
        if let Some(items) = &entry.items {
            return items.clone();
        }
        let root = entry.parse.syntax();
        let items = Arc::new(entry.events.document(&root));
        entry.items = Some(items.clone());
        items
    }

    /// Updates the parse of an open document (after an edit, from the
    /// editor's incremental parse).
    pub fn set_parse(&mut self, path: &Path, text: Arc<str>, parse: Parse) {
        let entry = self
            .files
            .entry(path.to_path_buf())
            .or_insert_with(CachedFile::new);
        entry.replace(text, parse);
    }

    /// [`ProjectCache::set_parse`], with the events the document's own
    /// model read from the same parse (read once, not twice).
    pub fn set_parse_from(
        &mut self,
        path: &Path,
        text: Arc<str>,
        parse: Parse,
        own: &crate::Cache,
    ) {
        let items = own.items_of(&parse);
        self.set_parse(path, text, parse);
        if let Some(entry) = self.files.get_mut(path) {
            entry.items = items;
        }
    }

    /// The last project again, when its files' events are the same but
    /// for one file whose events only moved with an edit: its model with
    /// that file's positions moved, without numbering the project again.
    fn moved(&mut self, root: &Path, files: &dyn Files) -> Option<Arc<Model>> {
        let mut last = self.last.take()?;
        if last.root != root {
            return None;
        }
        // Which file's events changed.
        let mut changed = None;
        for i in 0..last.model.files.len() {
            let path = last.model.files[i].clone();
            let text = files.read_shared(&path)?;
            let items = self.items(&path, text);
            if last
                .items
                .get(i)?
                .as_ref()
                .is_some_and(|l| Arc::ptr_eq(l, &items))
            {
                continue;
            }
            if changed.is_some() {
                return None;
            }
            changed = Some((i, items));
        }
        last.generation += 1;
        let Some((i, items)) = changed else {
            last.change = Change::Same;
            let m = last.model.clone();
            self.last = Some(last);
            return Some(m);
        };
        let entry = self.files.get(&last.model.files[i])?;
        let (prev_parse, prev_items) = entry.prev.as_ref()?;
        let old = last.items[i].as_ref()?;
        if !Arc::ptr_eq(prev_items, old) {
            return None;
        }
        let region = extract::Region::between(prev_parse.green(), entry.parse.green());
        if !extract::same_moved(old, 0, &items, 0, &region) {
            return None;
        }
        // In place when nobody else holds the last model.
        let model = std::mem::take(&mut last.model);
        let mut m = Arc::try_unwrap(model).unwrap_or_else(|m| Model::clone(&m));
        if !m.move_positions(i, &region) {
            return None;
        }
        let model = Arc::new(m);
        last.model = model.clone();
        last.items[i] = Some(items);
        last.change = Change::Moved(i, region);
        self.last = Some(last);
        Some(model)
    }

    /// The last project's model as seen from its file `this` (see
    /// [`Model::seen_from`]), with that file's `preamble` and `body`: the
    /// one given last time, moved as the project's model moved, when it
    /// can be.
    pub fn seen_from(
        &mut self,
        this: usize,
        preamble: std::ops::Range<usize>,
        body: Option<std::ops::Range<usize>>,
    ) -> Option<Arc<Model>> {
        let last = self.last.as_mut()?;
        if this == 0 && last.model.preamble == preamble && last.model.body == body {
            return Some(last.model.clone());
        }
        let generation = last.generation;
        let cached = last
            .seen
            .take()
            .filter(|(t, g, _)| *t == this && (*g == generation || *g + 1 == generation));
        let fresh = |model: &Model| model.seen_from(this);
        let mut m = match cached {
            Some((_, g, m)) if g == generation || last.change == Change::Same => {
                if m.preamble == preamble && m.body == body {
                    last.seen = Some((this, generation, m.clone()));
                    return Some(m);
                }
                Arc::try_unwrap(m).unwrap_or_else(|m| Model::clone(&m))
            }
            Some((_, _, m)) => match last.change {
                Change::Moved(i, region) => {
                    // The files `this` and 0 trade places in the view.
                    let f = if i == this {
                        0
                    } else if i == 0 {
                        this
                    } else {
                        i
                    };
                    let mut m = Arc::try_unwrap(m).unwrap_or_else(|m| Model::clone(&m));
                    if m.move_positions(f, &region) {
                        m
                    } else {
                        fresh(&last.model)
                    }
                }
                _ => fresh(&last.model),
            },
            None => fresh(&last.model),
        };
        m.preamble = preamble;
        m.body = body;
        let m = Arc::new(m);
        last.seen = Some((this, generation, m.clone()));
        Some(m)
    }

    /// The project whose root document is `root`.
    pub fn load(&mut self, root: &Path, files: &dyn Files) -> Project {
        if let Some(model) = self.moved(root, files) {
            return Project {
                root: root.to_path_buf(),
                model,
            };
        }
        let text = files.read_shared(root).unwrap_or_else(|| Arc::from(""));
        let len = text.len();
        let items = self.items(root, text);
        let mut used: Vec<Option<Arc<Vec<Item>>>> = vec![Some(items.clone())];
        let root_dir = root.parent().unwrap_or(Path::new("")).to_path_buf();
        // The files, and the folder `\input` reads from in each.
        let mut paths: Vec<PathBuf> = vec![root.to_path_buf()];
        let mut bases: Vec<PathBuf> = vec![root_dir.clone()];
        let mut resolve =
            |from: usize, command: &str, args: &[String]| -> Option<(usize, Arc<Vec<Item>>)> {
                let here = paths[from].parent().unwrap_or(Path::new("")).to_path_buf();
                let name = args.last()?.as_str();
                let candidates = match command {
                    "include" => vec![bases[from].join(format!("{name}.tex"))],
                    // A package of the document's own, beside it.
                    "usepackage" => vec![root_dir.join(format!("{name}.sty"))],
                    "input" => with_tex(&bases[from], name),
                    "subfile" => {
                        let p = normalize(&here.join(name));
                        vec![p.clone(), normalize(&here.join(format!("{name}.tex")))]
                    }
                    "import" | "subimport" => {
                        let folder = args.first()?;
                        let base = if command == "import" && !Path::new(folder).is_absolute() {
                            root_dir.join(folder)
                        } else {
                            here.join(folder)
                        };
                        with_tex(&base, name)
                    }
                    _ => return None,
                };
                let (path, text) = candidates
                    .into_iter()
                    .find_map(|p| files.read_shared(&p).map(|t| (p, t)))?;
                let base = match command {
                    "input" | "include" => bases[from].clone(),
                    _ => path.parent().unwrap_or(Path::new("")).to_path_buf(),
                };
                let id = match paths.iter().position(|p| *p == path) {
                    Some(i) => i,
                    None => {
                        paths.push(path.clone());
                        bases.push(base);
                        used.push(None);
                        paths.len() - 1
                    }
                };
                let items = self.items(&path, text);
                used[id] = Some(items.clone());
                Some((id, items))
            };
        let mut model = crate::number(&items, len, Some(&mut resolve));
        model.files = paths;
        let model = Arc::new(model);
        let generation = self.last.as_ref().map_or(0, |l| l.generation + 1);
        self.last = Some(Loaded {
            root: root.to_path_buf(),
            model: model.clone(),
            items: used,
            change: Change::New,
            generation,
            seen: None,
        });
        Project {
            root: root.to_path_buf(),
            model,
        }
    }
}
