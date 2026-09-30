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
    if file.is_relative()
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
}

/// A file of a project as the cache keeps it.
#[derive(Debug)]
struct CachedFile {
    text: Arc<str>,
    parse: Parse,
    events: extract::Cache,
    /// The events of `parse`, while it stays the same.
    items: Option<Arc<Vec<Item>>>,
}

impl CachedFile {
    fn new() -> CachedFile {
        CachedFile {
            text: Arc::from(""),
            parse: latex_syntax::parse(""),
            events: extract::Cache::default(),
            items: None,
        }
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
                entry.parse = latex_syntax::parse(&text);
                entry.items = None;
            }
            entry.text = text;
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
        entry.text = text;
        entry.parse = parse;
        entry.items = None;
    }

    /// The project whose root document is `root`.
    pub fn load(&mut self, root: &Path, files: &dyn Files) -> Project {
        let text = files.read_shared(root).unwrap_or_else(|| Arc::from(""));
        let len = text.len();
        let items = self.items(root, text);
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
                        paths.len() - 1
                    }
                };
                Some((id, self.items(&path, text)))
            };
        let mut model = crate::number(&items, len, Some(&mut resolve));
        model.files = paths;
        Project {
            root: root.to_path_buf(),
            model: Arc::new(model),
        }
    }
}
