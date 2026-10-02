//! The LaTeX editor's view (design §9.5, T2.7h.5): each source line of a
//! `.tex` file drawn as the document reads, the file left as it is.
//!
//! Sectioning commands are headings with the numbers LaTeX gives them;
//! `\emph`, `\textbf` and their kin show their text styled, with the
//! command and its braces hidden away from the cursor (as Org's emphasis
//! markers are); quotes and dashes (``` `` ```, `''`, `--`, `---`) and
//! `~`, `\&`, `\%`, `\,`, `\\` show as the characters they typeset;
//! `\maketitle` shows the title, the authors and the date; `center`,
//! `flushleft` and `flushright` align their lines; comments are dimmed.
//! Anything else stays as its source, never hidden or guessed.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use latex_syntax::{SyntaxKind as K, SyntaxNode, SyntaxToken};

use crate::view::{LineView, Run, Style};

/// `\title`, `\author` and `\date`.
type Titles = Arc<[Option<String>; 3]>;

/// The parse of a LaTeX document, kept up to date with its edits, and
/// its model.
#[derive(Debug)]
pub struct LatexState {
    parse: latex_syntax::Parse,
    /// The text of the parse, shared with the project's cache (building it
    /// from the tree would walk every token).
    text: Arc<str>,
    models: RefCell<latex_model::Cache>,
    titles: RefCell<Option<(latex_syntax::GreenNode, Titles)>>,
    /// The text between `\iffalse` and its `\fi`, for the parse.
    skipped: RefCell<Option<(latex_syntax::GreenNode, Spans)>>,
    /// The labels of the items of each list, by its green node.
    lists: RefCell<std::collections::HashMap<usize, (latex_syntax::GreenNode, Arc<Items>)>>,
    /// The diagnostics, worked out in the background.
    pub diagnostics: crate::latex_check::Live,
    /// The project the document belongs to, once its root is found.
    project: RefCell<Option<ProjectView>>,
    /// The root document, being found on a thread.
    root: Option<(
        std::path::PathBuf,
        std::sync::mpsc::Receiver<std::path::PathBuf>,
    )>,
    /// The file, and its `% !TEX root` line when the root was looked for.
    file: Option<(std::path::PathBuf, Option<String>)>,
    /// The paragraphs over several lines, for the text they were found in.
    paragraphs: RefCell<Option<(Arc<str>, Paragraphs)>>,
    /// The pictures TeX had finished when the view last looked.
    pub(crate) pictures_seen: std::cell::Cell<u64>,
}

/// The paragraphs of a LaTeX document over several source lines.
pub type Paragraphs = Arc<Vec<Range<usize>>>;

/// The root document `latex.root` names, relative to the project's
/// folder (or the file's without a project); empty for none.
static ROOT_SETTING: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Sets `latex.root` (from the settings, for the whole process).
pub fn set_root_setting(value: &str) {
    if let Ok(mut s) = ROOT_SETTING.write() {
        *s = value.trim().to_string();
    }
}

/// The root document of the LaTeX file `file` with text `text`, as every
/// part of Kalem finds it (T2.7h.4): `% !TEX root`, a subfile's main
/// document, a `.latexmain` marker, the root `latex.root` names, the file
/// itself with a `\documentclass`, a document with one in its folder or
/// above, within its project, that includes it; and last, any document of
/// the project that includes it.
pub fn find_root(file: &std::path::Path, text: &str) -> std::path::PathBuf {
    use latex_model::project::{self, Disk};
    let dir = file.parent().unwrap_or(std::path::Path::new(""));
    let top = kalem_project::list::detect_root(dir);
    let setting = ROOT_SETTING
        .read()
        .ok()
        .map(|s| s.clone())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let p = std::path::PathBuf::from(&s);
            if p.is_absolute() {
                p
            } else {
                top.clone().unwrap_or_else(|| dir.to_path_buf()).join(p)
            }
        })
        .filter(|p| p.exists());
    let root = project::find_root(file, text, &Disk, setting.as_deref(), top.as_deref());
    if root != file || text.contains("\\documentclass") {
        return root;
    }
    // Any document of the project that includes it (a main file in a
    // sibling folder).
    let Some(top) = top else { return root };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut candidates = Vec::new();
    kalem_project::files::walk(&top, &[], &cancel, |p| {
        if p.extension().is_some_and(|e| e == "tex") && candidates.len() < 2000 {
            candidates.push(if p.is_relative() { top.join(p) } else { p });
        }
    });
    candidates.sort();
    let mut cache = project::ProjectCache::default();
    let canon = |p: &std::path::Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let this = canon(file);
    for c in candidates {
        if canon(&c) == this {
            continue;
        }
        let has_class = std::fs::read_to_string(&c).is_ok_and(|t| t.contains("\\documentclass"));
        if has_class
            && cache
                .load(&c, &Disk)
                .model
                .files
                .iter()
                .any(|f| canon(f) == this)
        {
            return c;
        }
    }
    root
}

/// The `% !TEX root = …` line among the first lines of `text`, as written.
fn magic_root_line(text: &str) -> Option<String> {
    text.lines()
        .take(20)
        .find(|l| {
            let l = l.trim_start();
            l.starts_with('%') && l.to_ascii_lowercase().contains("tex root")
        })
        .map(|l| l.trim().to_string())
}

/// A document's project: the other files from the disk (read again when
/// they change), the document itself as edited, and the model seen from
/// it (T2.7h.4).
#[derive(Debug)]
struct ProjectView {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    cache: latex_model::project::ProjectCache,
    disk: DiskCache,
    last: Option<(latex_syntax::GreenNode, Arc<latex_model::Model>)>,
}

/// Files read from the disk, kept while their modification time stays.
#[derive(Debug, Default)]
struct DiskCache {
    /// Each file's time of change, when that was last asked, and its text.
    files: RefCell<std::collections::HashMap<std::path::PathBuf, DiskFile>>,
}

/// A file of [`DiskCache`].
#[derive(Debug)]
struct DiskFile {
    modified: std::time::SystemTime,
    checked: std::time::Instant,
    text: Arc<str>,
}

/// How long a file read from disk is trusted without asking the disk
/// again: a keystroke reads every file of a project, and a hundred
/// `stat` calls cost more than the rest of it.
const DISK_TRUSTED: std::time::Duration = std::time::Duration::from_secs(1);

/// The disk, with one file's text as edited.
struct Overlay<'a> {
    disk: &'a DiskCache,
    path: &'a std::path::Path,
    text: &'a Arc<str>,
}

impl latex_model::project::Files for Overlay<'_> {
    fn read(&self, path: &std::path::Path) -> Option<String> {
        self.read_shared(path).map(|t| t.to_string())
    }

    fn read_shared(&self, path: &std::path::Path) -> Option<Arc<str>> {
        if path == self.path {
            return Some(self.text.clone());
        }
        let mut files = self.disk.files.borrow_mut();
        if let Some(f) = files.get(path)
            && f.checked.elapsed() < DISK_TRUSTED
        {
            return Some(f.text.clone());
        }
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        if let Some(f) = files.get_mut(path)
            && f.modified == modified
        {
            f.checked = std::time::Instant::now();
            return Some(f.text.clone());
        }
        let text: Arc<str> = Arc::from(std::fs::read_to_string(path).ok()?);
        files.insert(
            path.to_path_buf(),
            DiskFile {
                modified,
                checked: std::time::Instant::now(),
                text: text.clone(),
            },
        );
        Some(text)
    }

    fn list(&self, dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        latex_model::project::Disk.list(dir)
    }
}

impl ProjectView {
    /// The project's model seen from the document, whose parse is `parse`
    /// and own model `own`; `None` when the document is alone.
    fn model(
        &mut self,
        parse: &latex_syntax::Parse,
        text: &Arc<str>,
        own: &latex_model::Model,
        cache: &latex_model::Cache,
    ) -> Option<Arc<latex_model::Model>> {
        // The root document that includes nothing needs no project; a
        // package of its own beside it (`\\usepackage{macros}`) is
        // included.
        let dir = self.root.parent().unwrap_or(std::path::Path::new(""));
        if self.root == self.path
            && own.includes.is_empty()
            && !own
                .packages
                .iter()
                .any(|p| dir.join(format!("{}.sty", p.name)).is_file())
        {
            return None;
        }
        if let Some((g, m)) = &self.last
            && same(g, parse.green())
        {
            return Some(m.clone());
        }
        // The last model let go, so that it can be moved in place.
        self.last = None;
        let text = text.clone();
        self.cache
            .set_parse_from(&self.path, text.clone(), parse.clone(), cache);
        let files = Overlay {
            disk: &self.disk,
            path: &self.path,
            text: &text,
        };
        let project = self.cache.load(&self.root, &files);
        let this = project.model.files.iter().position(|f| *f == self.path)?;
        drop(project);
        // Seen from this file, with its own preamble and body (the root
        // document's: the project's model as it is).
        let m = self
            .cache
            .seen_from(this, own.preamble.clone(), own.body.clone())?;
        self.last = Some((parse.green().clone(), m.clone()));
        Some(m)
    }
}

/// Ranges of the source.
type Spans = Arc<Vec<Range<usize>>>;

/// Where each `\item` of a list starts and what it shows.
type Items = Vec<(usize, String)>;

impl LatexState {
    /// The state of `text`.
    pub fn new(text: &str) -> LatexState {
        LatexState {
            parse: latex_syntax::parse(text),
            text: Arc::from(text),
            models: RefCell::new(latex_model::Cache::default()),
            titles: RefCell::new(None),
            skipped: RefCell::new(None),
            lists: RefCell::new(std::collections::HashMap::new()),
            diagnostics: crate::latex_check::Live::default(),
            project: RefCell::new(None),
            root: None,
            file: None,
            paragraphs: RefCell::new(None),
            pictures_seen: std::cell::Cell::new(0),
        }
    }

    /// After an edit of the text (now `text`): parsed again, only around
    /// the edit where that gives the same tree.
    pub(crate) fn edit(&mut self, text: &str, edit: Option<&org_syntax::TextEdit>) {
        self.parse = match edit {
            Some(e) => {
                let edit = latex_syntax::TextEdit {
                    range: usize::from(e.range.start())..usize::from(e.range.end()),
                    insert: e.insert.clone(),
                };
                self.parse.reparse(text, &edit)
            }
            None => latex_syntax::parse(text),
        };
        self.text = Arc::from(text);
    }

    /// The parse.
    pub fn parse(&self) -> &latex_syntax::Parse {
        &self.parse
    }

    /// The bibliography files the project names, found from the root's
    /// folder (or the document's, `path`, before the root is known).
    pub fn bibliography_files(&self, path: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
        let root = self.root_dir();
        let base = root
            .as_deref()
            .or_else(|| path.and_then(std::path::Path::parent));
        self.model()
            .bibliography
            .iter()
            .flat_map(|b| b.files.iter())
            .map(|f| base.map_or_else(|| std::path::PathBuf::from(f), |d| d.join(f)))
            .collect()
    }

    /// The project's root document, once it is found.
    pub fn root_path(&self) -> Option<std::path::PathBuf> {
        self.project.borrow().as_ref().map(|p| p.root.clone())
    }

    /// The folder of the project's root document, once it is found: where
    /// LaTeX runs, so where bibliography files are looked for.
    pub fn root_dir(&self) -> Option<std::path::PathBuf> {
        self.project
            .borrow()
            .as_ref()
            .and_then(|p| p.root.parent().map(std::path::Path::to_path_buf))
    }

    /// The document model (numbers, labels, citations, definitions): in
    /// a project of several files, the project's, seen from this file
    /// (its numbers continue the files before it, and labels in the other
    /// files resolve).
    pub fn model(&self) -> Arc<latex_model::Model> {
        let mut models = self.models.borrow_mut();
        let own = models.model(&self.parse);
        let mut project = self.project.borrow_mut();
        match project
            .as_mut()
            .and_then(|p| p.model(&self.parse, &self.text, &own, &models))
        {
            Some(m) => m,
            None => own,
        }
    }

    /// Starts finding the root document of the file at `path`, whose text
    /// is `text`, on a thread (it reads the folders around).
    pub(crate) fn find_project(&mut self, path: &std::path::Path, text: &str) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (p, t) = (path.to_path_buf(), text.to_string());
        std::thread::spawn(move || {
            let root = find_root(&p, &t);
            let _ = tx.send(root);
        });
        self.root = Some((path.to_path_buf(), rx));
        self.file = Some((path.to_path_buf(), magic_root_line(text)));
    }

    /// After an edit of the text (now `text`) that started at `at`: the
    /// root looked for again when the `% !TEX root` line changed.
    pub(crate) fn check_root(&mut self, text: &str, at: usize) {
        // Only an edit among the first lines can change it.
        if at > 4096 {
            return;
        }
        let Some((path, magic)) = &self.file else {
            return;
        };
        if magic_root_line(text) != *magic {
            let path = path.clone();
            *self.project.borrow_mut() = None;
            self.find_project(&path, text);
        }
    }

    /// Takes the root document when it is found; `true` then.
    pub(crate) fn poll_project(&mut self) -> bool {
        let Some((path, rx)) = &self.root else {
            return false;
        };
        match rx.try_recv() {
            Ok(root) => {
                let path = dunce::canonicalize(path).unwrap_or_else(|_| path.clone());
                let root = dunce::canonicalize(&root).unwrap_or(root);
                *self.project.borrow_mut() = Some(ProjectView {
                    root,
                    path,
                    cache: latex_model::project::ProjectCache::default(),
                    disk: DiskCache::default(),
                    last: None,
                });
                self.root = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.root = None;
                false
            }
        }
    }

    /// Waits for the root document (tests).
    pub fn wait_for_project(&mut self) {
        while self.root.is_some() {
            if !self.poll_project() {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    /// The labels of the items of list environment `env`.
    fn items(&self, env: &SyntaxNode) -> Arc<Items> {
        let green = env.green();
        let key = std::ptr::from_ref(green).cast::<()>() as usize;
        let mut lists = self.lists.borrow_mut();
        if let Some((_, v)) = lists.get(&key) {
            return v.clone();
        }
        let v = Arc::new(list_items(env));
        if lists.len() > 4096 {
            lists.clear();
        }
        lists.insert(key, (green.to_owned(), v.clone()));
        v
    }

    /// The text between each `\iffalse` and its `\fi` (other `\if…`
    /// commands between them nest).
    fn skipped(&self, text: &str) -> Spans {
        let mut sk = self.skipped.borrow_mut();
        if let Some((g, v)) = &*sk
            && same(g, self.parse.green())
        {
            return v.clone();
        }
        let mut out = Vec::new();
        // Most documents have none: no walk over the tree for them.
        if !text.contains("\\iffalse") {
            let v = Arc::new(out);
            *sk = Some((self.parse.green().clone(), v.clone()));
            return v;
        }
        let mut open: Option<(usize, usize)> = None;
        for t in self
            .parse
            .syntax()
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == K::CONTROL_WORD)
        {
            let name = &t.text()[1..];
            match (&mut open, name) {
                (None, "iffalse") => open = Some((usize::from(t.text_range().start()), 0)),
                (Some((_, depth)), n) if n.starts_with("if") => *depth += 1,
                (Some((_, depth)), "fi") if *depth > 0 => *depth -= 1,
                (Some((start, _)), "fi") => {
                    out.push(*start..usize::from(t.text_range().end()));
                    open = None;
                }
                _ => {}
            }
        }
        let v = Arc::new(out);
        *sk = Some((self.parse.green().clone(), v.clone()));
        v
    }

    /// `\title`, `\author` and `\date`, as written.
    fn titles(&self) -> Titles {
        let mut t = self.titles.borrow_mut();
        if let Some((g, v)) = &*t
            && same(g, self.parse.green())
        {
            return v.clone();
        }
        let mut v: [Option<String>; 3] = [None, None, None];
        for n in self.parse.syntax().descendants() {
            if n.kind() != K::COMMAND {
                continue;
            }
            let i = match latex_syntax::name(&n).as_deref() {
                Some("title") => 0,
                Some("author") => 1,
                Some("date") => 2,
                _ => continue,
            };
            // Written in the body, the command shows itself (acmart and
            // RevTeX put the front matter after `\begin{document}`).
            if n.ancestors().any(|a| {
                a.kind() == K::ENVIRONMENT && latex_syntax::name(&a).as_deref() == Some("document")
            }) {
                continue;
            }
            if let Some(g) = n.children().find(|c| c.kind() == K::GROUP) {
                let parts: Vec<String> = front_text(&g)
                    .split('\u{0}')
                    .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|p| !p.is_empty())
                    .collect();
                if parts.is_empty() {
                    continue;
                }
                let joined = parts.join(", ");
                // Several `\author`s (llncs, acmart, RevTeX) are listed.
                v[i] = Some(match (i, v[i].take()) {
                    (1, Some(before)) => format!("{before}, {joined}"),
                    _ => joined,
                });
            }
        }
        let v = Arc::new(v);
        *t = Some((self.parse.green().clone(), v.clone()));
        v
    }
}

pub(crate) fn is_list(name: &str) -> bool {
    matches!(name, "itemize" | "enumerate" | "description")
}

/// The list environments around `n`, innermost first.
fn lists_around(n: &SyntaxNode) -> Vec<(SyntaxNode, String)> {
    n.ancestors()
        .filter(|a| a.kind() == K::ENVIRONMENT)
        .filter_map(|a| {
            let name = latex_syntax::name(&a)?;
            is_list(&name).then_some((a, name))
        })
        .collect()
}

/// The code of `\verb` or `\lstinline` node `verb`: between its
/// delimiters (any character, or braces for `\lstinline`), after its
/// options.
fn verb_code(text: &str, verb: &SyntaxNode) -> Option<Range<usize>> {
    let v = verb
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == K::VERBATIM)?;
    let r = span(&v);
    let src = &text[r.clone()];
    let mut start = 0;
    // `\verb*|…|`: the star is not the delimiter.
    if src.starts_with('*') {
        start = 1;
    }
    if src[start..].starts_with('[') {
        start += src[start..].find(']')? + 1;
    }
    let open = src[start..].chars().next()?;
    let close = if open == '{' { '}' } else { open };
    let body = start + open.len_utf8();
    let end = body + src[body..].rfind(close).filter(|_| src.ends_with(close))?;
    Some(r.start + body..r.start + end)
}

/// Applies font declaration `name` (`bf`, `itshape`, `normalfont`…) to
/// `style`; whether it is one.
fn declaration(name: &str, style: &mut Style) -> bool {
    match name {
        "bf" | "bfseries" => style.bold = true,
        "it" | "itshape" | "sl" | "slshape" | "em" => style.italic = true,
        "tt" | "ttfamily" => style.code = true,
        "upshape" => style.italic = false,
        "mdseries" => style.bold = false,
        "rmfamily" | "sffamily" => style.code = false,
        "normalfont" | "rm" => {
            style.bold = false;
            style.italic = false;
            style.code = false;
        }
        "sc" | "scshape" | "sf" => {}
        // Sizes, as the standard classes set them at 10pt, shown at the
        // same ratio to a 12-point text.
        "normalsize" => style.rich.size = None,
        size => {
            let points = match size {
                "tiny" => 5.0,
                "scriptsize" => 7.0,
                "footnotesize" => 8.0,
                "small" => 9.0,
                "large" => 12.0,
                "Large" => 14.4,
                "LARGE" => 17.28,
                "huge" => 20.74,
                "Huge" => 24.88,
                _ => return false,
            };
            style.rich.size = Some((points * 12.0) as u16);
        }
    }
    true
}

/// The declarations among `node`'s children before `at` (and, in an
/// environment's body, those of its earlier paragraphs), as a style.
fn declared_before(node: &SyntaxNode, at: usize) -> Option<Style> {
    let list = declarations(node);
    let i = list.partition_point(|(start, _)| *start < at);
    i.checked_sub(1).map(|i| list[i].1)
}

/// The declarations among the children of `node` (of each paragraph of an
/// environment's body): where each starts and the style from there on.
/// Kept per node: drawing a line asks for those before it, and a long
/// group or body would be read from its start for every line.
fn declarations(node: &SyntaxNode) -> std::rc::Rc<Vec<(usize, Style)>> {
    type Memo = std::collections::HashMap<
        (usize, usize, usize, u16),
        (latex_syntax::GreenNode, std::rc::Rc<Vec<(usize, Style)>>),
    >;
    thread_local! {
        static MEMO: std::cell::RefCell<Memo> = std::cell::RefCell::new(Memo::new());
    }
    let green = node.green().to_owned();
    let r = node.text_range();
    // The green node, kept alive with the entry, so its address names it.
    let key = (
        std::ptr::from_ref(&*green).cast::<()>() as usize,
        usize::from(r.start()),
        usize::from(r.end()),
        node.kind() as u16,
    );
    if let Some(v) = MEMO.with(|m| m.borrow().get(&key).map(|e| e.1.clone())) {
        return v;
    }
    let mut declared = Style::default();
    let mut out = Vec::new();
    let mut scan = |n: &SyntaxNode| {
        for e in n.children() {
            if e.kind() != K::COMMAND {
                continue;
            }
            let Some(name) = latex_syntax::name(&e) else {
                continue;
            };
            // `\color{red}`: what follows in red (its name is the group
            // after it).
            if name == "color"
                && let Some(g) = e
                    .children()
                    .find(|x| x.kind() == K::GROUP)
                    .or_else(|| e.next_sibling().filter(|x| x.kind() == K::GROUP))
            {
                declared.rich.color = latex_color(&group_text(&g));
                out.push((usize::from(e.text_range().start()), declared));
            } else if declaration(&name, &mut declared) {
                out.push((usize::from(e.text_range().start()), declared));
            }
        }
    };
    if node.kind() == K::BODY {
        for p in node.children() {
            scan(&p);
        }
    } else {
        scan(node);
    }
    let v = std::rc::Rc::new(out);
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 512 {
            m.clear();
        }
        m.insert(key, (green, v.clone()));
    });
    v
}

/// Whether token `t` is inside math.
fn c_math(t: &SyntaxToken) -> bool {
    math_node(t).is_some()
}

/// The lines from which an environment the view does not render folds.
const LONG_UNKNOWN: usize = 8;

/// The environments around `n` that indent their text, as lists do:
/// `quote`, `quotation`, `verse` and `abstract`.
fn quotes_around(n: &SyntaxNode) -> usize {
    n.ancestors()
        .filter(|a| a.kind() == K::BODY)
        .filter_map(|a| a.parent())
        .filter(|e| {
            latex_syntax::name(e)
                .is_some_and(|n| matches!(n.as_str(), "quote" | "quotation" | "verse" | "abstract"))
        })
        .count()
}

/// An `enumerate` label in LaTeX's default styles by depth (`1.`, `(a)`,
/// `i.`, `A.`), or as `label=` of enumitem gives it.
fn enum_label(n: i64, depth: usize, pattern: Option<&str>) -> String {
    let arabic = n.to_string();
    let alph = |upper: bool| {
        if (1..=26).contains(&n) {
            let c = (b'a' + (n - 1) as u8) as char;
            if upper { c.to_ascii_uppercase() } else { c }.to_string()
        } else {
            arabic.clone()
        }
    };
    let roman = |upper: bool| {
        let mut s = String::new();
        let mut m = n;
        for (v, r) in [
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ] {
            while m >= v {
                s.push_str(r);
                m -= v;
            }
        }
        if upper { s.to_uppercase() } else { s }
    };
    if let Some(p) = pattern {
        return p
            .replace("\\arabic*", &arabic)
            .replace("\\alph*", &alph(false))
            .replace("\\Alph*", &alph(true))
            .replace("\\roman*", &roman(false))
            .replace("\\Roman*", &roman(true));
    }
    match depth {
        1 => format!("{arabic}."),
        2 => format!("({})", alph(false)),
        3 => format!("{}.", roman(false)),
        _ => format!("{}.", alph(true)),
    }
}

/// `key=value` among a list's options (enumitem).
fn list_option(env: &SyntaxNode, key: &str) -> Option<String> {
    let begin = env.children().find(|c| c.kind() == K::BEGIN)?;
    let opt = begin.children().find(|c| c.kind() == K::OPT_ARG)?;
    let s = opt.text().to_string();
    let s = s.strip_prefix('[')?.strip_suffix(']')?;
    // Split at top-level commas.
    let mut depth = 0;
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts.iter().find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k.trim() == key).then(|| {
            let v = v.trim();
            v.strip_prefix('{')
                .and_then(|v| v.strip_suffix('}'))
                .unwrap_or(v)
                .to_string()
        })
    })
}

/// The items of a list and what each shows: a bullet by the depth of
/// `itemize` lists, a number in the style of the depth of `enumerate`
/// lists, or the item's own label.
fn list_items(env: &SyntaxNode) -> Items {
    let around = lists_around(env);
    let name = around.first().map(|(_, n)| n.clone()).unwrap_or_default();
    let depth_of = |kind: &str| around.iter().filter(|(_, n)| n == kind).count();
    let label = list_option(env, "label");
    let mut n: i64 = list_option(env, "start")
        .and_then(|s| s.parse().ok())
        .map_or(0, |s: i64| s - 1);
    let mut out = Vec::new();
    // This level's counter, which `\setcounter` and `\addtocounter` set.
    let counter =
        ["enumi", "enumii", "enumiii", "enumiv"][(depth_of("enumerate").max(1) - 1).min(3)];
    for cmd in env.descendants().filter(|c| c.kind() == K::COMMAND) {
        let cname = latex_syntax::name(&cmd).unwrap_or_default();
        if !matches!(cname.as_str(), "item" | "setcounter" | "addtocounter") {
            continue;
        }
        // Items of this list, not of one inside it.
        if cmd
            .ancestors()
            .find(|a| {
                a.kind() == K::ENVIRONMENT && latex_syntax::name(a).is_some_and(|n| is_list(&n))
            })
            .as_ref()
            != Some(env)
        {
            continue;
        }
        if cname != "item" {
            let (_, m) = arguments(&cmd);
            if name == "enumerate"
                && m.first().map(|c| c.trim()) == Some(counter)
                && let Some(v) = m.get(1).and_then(|v| v.trim().parse::<i64>().ok())
            {
                n = if cname == "setcounter" { v } else { n + v };
            }
            continue;
        }
        let start = usize::from(cmd.text_range().start());
        let own = cmd.children().find(|c| c.kind() == K::OPT_ARG).map(|o| {
            let t = o.text().to_string();
            t[1..t.len() - usize::from(t.ends_with(']'))].to_string()
        });
        let shown = match (own, name.as_str()) {
            (Some(l), _) => l,
            (None, "enumerate") => {
                n += 1;
                enum_label(n, depth_of("enumerate"), label.as_deref())
            }
            (None, "description") => String::new(),
            (None, _) => label.clone().unwrap_or_else(|| {
                ["\u{2022}", "\u{2013}", "\u{2217}", "\u{b7}"]
                    [(depth_of("itemize").max(1) - 1).min(3)]
                .to_string()
            }),
        };
        out.push((start, shown));
    }
    out
}

/// Whether two green trees are the same node (a pointer comparison; the
/// derived equality compares the whole trees).
fn same(a: &latex_syntax::GreenNode, b: &latex_syntax::GreenNode) -> bool {
    let (a, b): (&latex_syntax::GreenNode, &latex_syntax::GreenNode) = (a, b);
    std::ptr::eq(
        std::ptr::from_ref(&**a).cast::<()>(),
        std::ptr::from_ref(&**b).cast::<()>(),
    )
}

/// The words of a title or an author list: the commands' names, braces
/// and the notes, affiliations and addresses inside left out; `\and` and
/// `\\` as NUL, the separator between names.
fn front_text(g: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut skip: Option<Range<usize>> = None;
    for e in g.descendants_with_tokens() {
        let r = match (e.as_node(), e.as_token()) {
            (Some(n), _) => node_span(n),
            (_, Some(t)) => span(t),
            _ => continue,
        };
        if skip.as_ref().is_some_and(|k| r.start < k.end) {
            continue;
        }
        match (e.as_node(), e.as_token()) {
            (Some(n), _) => {
                if n.kind() == K::COMMAND
                    && latex_syntax::name(n).is_some_and(|name| {
                        matches!(
                            name.as_str(),
                            "thanks"
                                | "inst"
                                | "footnote"
                                | "footnotemark"
                                | "IEEEauthorblockA"
                                | "IEEEmembership"
                                | "affiliation"
                                | "institute"
                                | "address"
                                | "email"
                                | "orcid"
                                | "textsuperscript"
                        )
                    })
                {
                    // Arguments the parser does not know the command
                    // takes follow it as groups of their own.
                    let mut end = r.end;
                    let mut next = n.next_sibling();
                    while let Some(g) = next.filter(|g| g.kind() == K::GROUP)
                        && node_span(&g).start == end
                    {
                        end = node_span(&g).end;
                        next = g.next_sibling();
                    }
                    skip = Some(r.start..end);
                }
            }
            (_, Some(t)) => match t.kind() {
                K::TEXT | K::WHITESPACE | K::NEWLINE => out.push_str(t.text()),
                K::TILDE => out.push(' '),
                K::CONTROL_SYMBOL if t.text() == "\\\\" => out.push('\u{0}'),
                K::CONTROL_SYMBOL => out.push_str(symbol(t.text()).unwrap_or("")),
                K::CONTROL_WORD if t.text() == "\\and" => out.push('\u{0}'),
                K::CONTROL_WORD => out.push_str(word(&t.text()[1..]).unwrap_or("")),
                _ => {}
            },
            _ => {}
        }
    }
    out
}

/// The text of an optional argument without its brackets.
fn group_text_brackets(o: &SyntaxNode) -> String {
    let s = o.text().to_string();
    let s = s.strip_prefix('[').unwrap_or(&s);
    s.strip_suffix(']').unwrap_or(s).to_string()
}

/// The text of a group without its braces.
fn group_text(g: &SyntaxNode) -> String {
    let s = g.text().to_string();
    let s = s.strip_prefix('{').unwrap_or(&s);
    s.strip_suffix('}').unwrap_or(s).to_string()
}

fn span(t: &SyntaxToken) -> Range<usize> {
    usize::from(t.text_range().start())..usize::from(t.text_range().end())
}

fn node_span(n: &SyntaxNode) -> Range<usize> {
    usize::from(n.text_range().start())..usize::from(n.text_range().end())
}

/// The style a formatting command gives its argument.
fn format_style(name: &str) -> Option<Style> {
    let mut s = Style::default();
    match name {
        "emph" | "textit" | "textsl" => s.italic = true,
        "textbf" => s.bold = true,
        "texttt" => s.code = true,
        "underline" | "uline" => s.underline = true,
        "sout" => s.strike = true,
        "textsuperscript" => s.superscript = true,
        "textsubscript" => s.subscript = true,
        "textsc" | "textsf" | "textrm" | "textup" | "textmd" | "textnormal" => {}
        _ => return None,
    }
    Some(s)
}

/// The front matter commands of the common classes (IEEEtran, acmart,
/// elsarticle, llncs, RevTeX, amsart) written in the body: the style
/// their text takes and the words shown before it.
fn front_style(name: &str) -> Option<(Style, &'static str)> {
    let mut s = Style::default();
    let prefix = match name {
        "title" => {
            s.title = true;
            ""
        }
        "author" | "affiliation" | "affil" | "institute" | "address" | "email" | "ead"
        | "IEEEauthorblockN" | "IEEEauthorblockA" | "institution" | "department" | "city"
        | "state" | "country" | "streetaddress" | "postcode" | "orcid" | "subtitle" => {
            s.byline = true;
            ""
        }
        "keywords" => {
            s.byline = true;
            "Keywords: "
        }
        "ccsdesc" => {
            s.byline = true;
            "CCS: "
        }
        "pacs" => {
            s.byline = true;
            "PACS: "
        }
        "thanks" => {
            s.dim = true;
            ""
        }
        "inst" => {
            s.superscript = true;
            ""
        }
        "IEEEPARstart" => "",
        _ => return None,
    };
    Some((s, prefix))
}

/// Front matter parts run together in one argument (acmart's
/// `\affiliation{\institution{…}\city{…}}`), separated when shown.
fn front_part(name: &str) -> bool {
    matches!(
        name,
        "institution" | "department" | "city" | "state" | "country" | "streetaddress" | "postcode"
    )
}

fn merge(a: &mut Style, b: &Style) {
    a.title |= b.title;
    a.byline |= b.byline;
    a.dim |= b.dim;
    a.bold |= b.bold;
    a.italic |= b.italic;
    a.code |= b.code;
    a.underline |= b.underline;
    a.strike |= b.strike;
    a.superscript |= b.superscript;
    a.subscript |= b.subscript;
    // The innermost size and color win.
    if a.rich.size.is_none() {
        a.rich.size = b.rich.size;
    }
    if a.rich.color.is_none() {
        a.rich.color = b.rich.color;
    }
}

/// An xcolor color by name (a mix, `red!50`, as its first color):
/// xcolor's base colors.
fn latex_color(spec: &str) -> Option<crate::theme::Color> {
    let name = spec.split('!').next()?.trim();
    Some(crate::theme::Color(match name {
        "red" => 0xff0000,
        "green" => 0x00ff00,
        "blue" => 0x0000ff,
        "cyan" => 0x00ffff,
        "magenta" => 0xff00ff,
        "yellow" => 0xffff00,
        "black" => 0x000000,
        "white" => 0xffffff,
        "gray" => 0x808080,
        "darkgray" => 0x404040,
        "lightgray" => 0xbfbfbf,
        "brown" => 0xbf8040,
        "lime" => 0xbfff00,
        "olive" => 0x808000,
        "orange" => 0xff8000,
        "pink" => 0xffbfbf,
        "purple" => 0xbf0040,
        "teal" => 0x008080,
        "violet" => 0x800080,
        _ => return None,
    }))
}

/// Whether token `t` is in a table of the text the grid does not draw
/// (a simple one is drawn as the grid).
fn in_text_table(text: &str, t: &SyntaxToken) -> bool {
    t.parent_ancestors()
        .find(|a| a.kind() == K::ENVIRONMENT)
        .is_some_and(|e| {
            latex_syntax::name(&e).is_some_and(|n| crate::latex_table::is_table(&n))
                && crate::latex_table::simple(text, &e).is_none()
        })
}

/// Whether command `cmd` is in an algorithm's statements.
fn in_algorithm(cmd: &SyntaxNode) -> bool {
    cmd.ancestors().any(|a| {
        a.kind() == K::ENVIRONMENT && latex_syntax::name(&a).is_some_and(|n| n == "algorithmic")
    })
}

/// What a statement of algpseudocode or algorithmic prints: before its
/// first argument, between two, after the last, and whether that is bold
/// (keywords are).
fn algorithm_words(name: &str) -> Option<(&'static str, &'static str, &'static str, bool)> {
    Some(match name {
        "State" | "Statex" | "STATE" | "STATEX" => ("", "", "", false),
        "If" | "IF" => ("if ", "", " then", true),
        "ElsIf" | "ELSIF" => ("else if ", "", " then", true),
        "Else" | "ELSE" => ("else", "", "", true),
        "EndIf" | "ENDIF" => ("end if", "", "", true),
        "For" | "FOR" => ("for ", "", " do", true),
        "ForAll" | "FORALL" => ("for all ", "", " do", true),
        "EndFor" | "ENDFOR" => ("end for", "", "", true),
        "While" | "WHILE" => ("while ", "", " do", true),
        "EndWhile" | "ENDWHILE" => ("end while", "", "", true),
        "Repeat" | "REPEAT" => ("repeat", "", "", true),
        "Until" | "UNTIL" => ("until ", "", "", true),
        "Loop" | "LOOP" => ("loop", "", "", true),
        "EndLoop" | "ENDLOOP" => ("end loop", "", "", true),
        "Require" | "REQUIRE" => ("Require:", "", "", true),
        "Ensure" | "ENSURE" => ("Ensure:", "", "", true),
        "Return" | "RETURN" => ("return", "", "", true),
        "PRINT" => ("print", "", "", true),
        "Procedure" => ("procedure ", "(", ")", true),
        "EndProcedure" => ("end procedure", "", "", true),
        "Function" => ("function ", "(", ")", true),
        "EndFunction" => ("end function", "", "", true),
        "Call" => ("", "(", ")", false),
        // algpseudocode: ▷ and the comment; algorithmic: {the comment}.
        "Comment" => ("\u{25b7} ", "", "", false),
        "COMMENT" => ("{", "", "}", false),
        _ => return None,
    })
}

/// Commands that print nothing where they are (a definition, a setting):
/// their arguments, as [`args_end`] reads them.
pub(crate) fn silent(name: &str) -> Option<&'static str> {
    Some(match name {
        "newcommand" | "renewcommand" | "providecommand" | "DeclareRobustCommand" => "smoom",
        "DeclareMathOperator" => "smm",
        "newenvironment" | "renewenvironment" => "smoomm",
        "newtheorem" => "smomo",
        "theoremstyle"
        | "thispagestyle"
        | "pagestyle"
        | "pagenumbering"
        | "date"
        | "hypersetup"
        | "graphicspath"
        | "linespread"
        | "DeclareGraphicsExtensions" => "m",
        "newcounter" => "mo",
        "setcounter" | "addtocounter" | "setlength" | "addtolength" => "mm",
        "definecolor" => "ommm",
        "colorlet" => "omm",
        "captionsetup" => "om",
        "numberwithin" => "omm",
        "counterwithin" | "counterwithout" => "smm",
        "allowdisplaybreaks" => "o",
        "makeatletter" | "makeatother" | "raggedbottom" | "flushbottom" | "sloppy" | "fussy"
        | "hline" | "tabularnewline" | "BibitemOpen" | "EOS" | "ProcessOptions" => "",
        // A package's or a class's own commands, written in a document.
        "DeclareOption" => "smm",
        "ProvidesPackage" | "ProvidesClass" | "NeedsTeXFormat" => "mo",
        // REVTeX's end of a bibliography entry.
        "BibitemShut" => "m",
        // Space as wide as its argument: nothing to read.
        "phantom" | "hphantom" | "vphantom" => "m",
        // A table's rules: markup.
        "toprule" | "midrule" | "bottomrule" | "addlinespace" => "o",
        "cline" => "m",
        "cmidrule" => "pm",
        "def" | "gdef" | "edef" | "xdef" => "d",
        "let" | "global" => "l",
        "twocolumn" => "o",
        "setstretch" | "authorrunning" | "titlerunning" | "pagerange" | "preprint"
        | "IEEEmembership" | "JournalTitle" | "corref" | "fnref" | "tnoteref" | "pubyear"
        | "volume" | "issue" | "jyear" | "jvol" | "jnum" | "received" | "revised" | "accepted"
        | "published" | "articletype" | "copyrightyear" | "acmDOI" | "acmISBN"
        | "acmConference" | "acmBooktitle" | "acmYear" | "setcopyright" | "ccsdesc"
        | "shorttitle" | "shortauthors" | "runningauthor" | "runningtitle" => "m",
        "markboth" | "markright" | "fontsize" => "mm",
        "cortext" | "fntext" | "tnotetext" => "om",
        "rowcolor" | "cellcolor" | "columncolor" => "om",
        "addcontentsline" => "mmm",
        "newcolumntype" => "mom",
        _ => return None,
    })
}

/// Commands that box their text: the arguments before the text (sizes,
/// angles, positions), as [`args_end`] reads them.
pub(crate) fn box_args(name: &str) -> Option<&'static str> {
    Some(match name {
        "resizebox" => "smm",
        "scalebox" => "mo",
        "rotatebox" => "om",
        "raisebox" => "moo",
        "adjustbox" => "m",
        "parbox" => "ooom",
        "makebox" | "framebox" => "oo",
        // A spanning cell: its count and alignment hidden, its text shown.
        "multicolumn" => "mm",
        // REVTeX's bibliography fields (`\\bibinfo{author}{…}`): the text.
        "bibinfo" | "bibfield" => "m",
        "multirow" => "omom",
        "makecell" | "thead" => "o",
        // A caption outside a float, a link to a label, a colored box:
        // their text.
        "captionof" => "sm",
        "hyperref" => "o",
        "colorbox" => "om",
        "fcolorbox" => "omm",
        // REVTeX's bibliography: a link with nothing to link, its text.
        "href@noop" => "m",
        _ => return None,
    })
}

/// Where the arguments after `at` end (before `limit`), read as `spec`
/// says: `s` an optional star, `o` an optional `[…]`, `p` an optional
/// `(…)`, `m` a group or one token, `d` what `\def` takes (a name, its
/// parameters and the body), `l` what `\let` takes (two names, `=` between
/// them or not).
pub(crate) fn args_end(text: &str, at: usize, limit: usize, spec: &str) -> Option<usize> {
    let b = text.as_bytes();
    let mut p = at;
    let blanks = |p: &mut usize| {
        while *p < limit && matches!(b[*p], b' ' | b'\t') {
            *p += 1;
        }
    };
    // One token: a control sequence, a group or a character.
    let token = |p: &mut usize| -> Option<()> {
        if *p >= limit {
            return None;
        }
        match b[*p] {
            b'{' => *p = group_end(text, *p, limit)?,
            b'\\' => {
                *p += 1;
                // `@` a letter: the names defined and let are a package's
                // (`\let\auto@bib@innerbib\@empty`).
                let n = text[*p..limit]
                    .bytes()
                    .take_while(|c| c.is_ascii_alphabetic() || *c == b'@')
                    .count()
                    .max(1);
                *p = (*p + n).min(limit);
            }
            _ => *p += text[*p..].chars().next()?.len_utf8(),
        }
        Some(())
    };
    for c in spec.chars() {
        let before = p;
        blanks(&mut p);
        match c {
            's' => {
                if text[p..limit].starts_with('*') {
                    p += 1;
                } else {
                    p = before;
                }
            }
            'o' => {
                if text[p..limit].starts_with('[') {
                    let close = text[p..limit].find(']')?;
                    p += close + 1;
                } else {
                    p = before;
                }
            }
            // booktabs' `(lr)`.
            'p' => {
                if text[p..limit].starts_with('(') {
                    let close = text[p..limit].find(')')?;
                    p += close + 1;
                } else {
                    p = before;
                }
            }
            'm' => token(&mut p)?,
            'd' => {
                token(&mut p)?;
                let open = text[p..limit].find('{')?;
                p = group_end(text, p + open, limit)?;
            }
            'l' => {
                token(&mut p)?;
                blanks(&mut p);
                if text[p..limit].starts_with('=') {
                    p += 1;
                    blanks(&mut p);
                }
                token(&mut p)?;
            }
            _ => return None,
        }
    }
    Some(p)
}

/// Commands whose arguments are text a reader reads (typography applies).
pub(crate) fn prose(name: &str) -> bool {
    format_style(name).is_some()
        || front_style(name).is_some()
        || accent_mark(name).is_some()
        || transparent(name)
        || latex_syntax::signatures::is_sectioning(name)
        || matches!(
            name,
            "footnote"
                | "caption"
                | "item"
                | "title"
                | "author"
                | "date"
                | "text"
                | "mbox"
                | "textcolor"
                | "footnotetext"
                | "thanks"
                | "enquote"
                | "MakeUppercase"
                | "MakeLowercase"
                | "uppercase"
                | "lowercase"
                | "MakeTextUppercase"
                | "MakeTextLowercase"
                | "textsuperscript"
                | "textsubscript"
                | "underline"
                | "uline"
                | "\\"
        )
}

/// What the view makes of a token.
#[derive(Debug, Default)]
struct Context {
    style: Style,
    math: bool,
    typography: bool,
    /// The command whose marker the token is (hidden away from it).
    marker_of: Option<SyntaxNode>,
    /// The token is the title's opening brace of this sectioning command.
    title_open: Option<SyntaxNode>,
    /// Inside a sectioning command.
    heading: bool,
}

fn context(t: &SyntaxToken) -> Context {
    let mut c = Context {
        typography: true,
        ..Context::default()
    };
    let mut child: Option<SyntaxNode> = None;
    for a in t.parent_ancestors() {
        match a.kind() {
            K::COMMAND => {
                let name = latex_syntax::name(&a).unwrap_or_default();
                let section = latex_syntax::signatures::is_sectioning(&name);
                let front = front_style(&name);
                let format = format_style(&name).or(front.map(|f| f.0));
                if let Some(s) = &format {
                    merge(&mut c.style, s);
                }
                if section {
                    c.heading = true;
                }
                if name == "footnote" || name == "footnotetext" {
                    c.style.dim = true;
                }
                // `\textcolor{red}{…}`: the text in red.
                if name == "textcolor"
                    && let Some(g) = &child
                    && g.kind() == K::GROUP
                {
                    let groups: Vec<SyntaxNode> =
                        a.children().filter(|x| x.kind() == K::GROUP).collect();
                    if groups.len() == 2 && groups[1] == *g && c.style.rich.color.is_none() {
                        c.style.rich.color = latex_color(&group_text(&groups[0]));
                    }
                }
                if !prose(&name) {
                    c.typography = false;
                }
                if (format.is_some() || section) && c.marker_of.is_none() {
                    let marker = match &child {
                        // The name, a star, blanks between the arguments.
                        None => {
                            matches!(
                                t.kind(),
                                K::CONTROL_WORD | K::STAR | K::WHITESPACE | K::NEWLINE
                            ) && (t.kind() != K::CONTROL_WORD
                                || t.prev_sibling_or_token().is_none())
                        }
                        // The braces of an argument, a short title.
                        Some(g) if g.kind() == K::GROUP => {
                            let first = g.first_token().is_some_and(|f| f == *t);
                            let last = g
                                .last_token()
                                .is_some_and(|l| l == *t && l.kind() == K::R_BRACE);
                            if first && section {
                                c.title_open = Some(a.clone());
                            }
                            (first || last) && t.parent().as_ref() == Some(g)
                        }
                        Some(o) if o.kind() == K::OPT_ARG => section || front.is_some(),
                        _ => false,
                    };
                    if marker {
                        c.marker_of = Some(a.clone());
                    }
                }
            }
            K::INLINE_MATH | K::DISPLAY_MATH => {
                c.math = true;
                c.typography = false;
            }
            // `{\\bf …}`, `{\\itshape …}`: the declarations before the token
            // in its group style it to the group's end.
            // So do those earlier in a paragraph, and in an environment's
            // body (the environment is a group); not the whole document's.
            K::GROUP | K::PARAGRAPH => {
                let at = child.as_ref().map_or_else(
                    || usize::from(t.text_range().start()),
                    |c| usize::from(c.text_range().start()),
                );
                if let Some(declared) = declared_before(&a, at) {
                    // The innermost group's declarations come first; an
                    // outer group only adds.
                    merge(&mut c.style, &declared);
                }
            }
            K::BODY
                if a.parent()
                    .and_then(|e| latex_syntax::name(&e))
                    .is_some_and(|n| n != "document") =>
            {
                let at = child.as_ref().map_or_else(
                    || usize::from(t.text_range().start()),
                    |c| usize::from(c.text_range().start()),
                );
                if let Some(declared) = declared_before(&a, at) {
                    merge(&mut c.style, &declared);
                }
            }
            K::ENVIRONMENT => {
                let name = latex_syntax::name(&a).unwrap_or_default();
                if latex_syntax::signatures::is_math(&name) {
                    c.math = true;
                    c.typography = false;
                }
                if latex_syntax::signatures::is_verbatim(&name) {
                    c.typography = false;
                }
            }
            K::VERB | K::BEGIN | K::END => c.typography = false,
            _ => {}
        }
        child = Some(a);
    }
    // A typewriter font has no ligatures: `--` and ``` `` ``` stay.
    if c.style.code {
        c.typography = false;
    }
    c
}

/// Typographic replacements in text: `` ` `` `'` quotes and dashes.
fn typography(s: &str) -> Vec<(Range<usize>, &'static str)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let rep: Option<(usize, &str)> = match b[i] {
            b'`' if b.get(i + 1) == Some(&b'`') => Some((2, "\u{201c}")),
            b'`' => Some((1, "\u{2018}")),
            b'\'' if b.get(i + 1) == Some(&b'\'') => Some((2, "\u{201d}")),
            b'\'' => Some((1, "\u{2019}")),
            b'-' if b.get(i + 1) == Some(&b'-') && b.get(i + 2) == Some(&b'-') => {
                Some((3, "\u{2014}"))
            }
            b'-' if b.get(i + 1) == Some(&b'-') => Some((2, "\u{2013}")),
            // The Spanish ligatures.
            b'!' if b.get(i + 1) == Some(&b'`') => Some((2, "\u{a1}")),
            b'?' if b.get(i + 1) == Some(&b'`') => Some((2, "\u{bf}")),
            _ => None,
        };
        match rep {
            Some((n, r)) => {
                out.push((i..i + n, r));
                i += n;
            }
            None => i += 1,
        }
    }
    out
}

/// The combining mark of a text accent command (`"` for `\"`).
pub(crate) fn accent_mark(name: &str) -> Option<char> {
    Some(match name {
        "\"" => '\u{308}',
        "'" => '\u{301}',
        "`" => '\u{300}',
        "^" => '\u{302}',
        "~" => '\u{303}',
        "=" => '\u{304}',
        "." => '\u{307}',
        "u" => '\u{306}',
        "v" => '\u{30c}',
        "H" => '\u{30b}',
        "c" => '\u{327}',
        "k" => '\u{328}',
        "r" => '\u{30a}',
        "d" => '\u{323}',
        "b" => '\u{331}',
        _ => return None,
    })
}

/// The accent command `name` (ending at `at`) applied to the letter after
/// it: `\"o`, `\"{o}`, `\c c`, `\u{g}`, `\'{\i}`. Gives where the letter
/// ends and the character, composed where Unicode has one.
fn accented(text: &str, name: &str, at: usize, limit: usize) -> Option<(usize, String)> {
    use unicode_normalization::UnicodeNormalization;
    let mark = accent_mark(name)?;
    let word = name.chars().all(|c| c.is_ascii_alphabetic());
    let mut p = at;
    let rest = &text[p..limit.max(p)];
    // The letter is an argument: TeX skips the blanks before it (after a
    // word accent, at least one ends its name).
    let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if word && blanks == 0 && !rest.starts_with('{') {
        return None;
    }
    p += blanks;
    let rest = &text[p..limit.max(p)];
    let (letter, end) = if let Some(inner) = rest.strip_prefix('{') {
        let close = inner.find('}')?;
        let body = inner[..close].trim();
        let l = match body {
            "\\i" => 'ı',
            "\\j" => 'ȷ',
            b if b.chars().count() == 1 => b.chars().next()?,
            // `\"{}`: nothing to accent.
            _ => return None,
        };
        (l, p + 1 + close + 1)
    } else if let Some(r) = rest
        .strip_prefix("\\i")
        .filter(|r| !r.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        // With the blanks the control word `\\i` eats.
        ('ı', limit - r.trim_start_matches([' ', '\t']).len())
    } else {
        let l = rest.chars().next().filter(|c| c.is_alphabetic())?;
        (l, p + l.len_utf8())
    };
    // `\i` and `\j` are dotless to carry the accent: `\'{\i}` is í.
    let letter = match letter {
        'ı' => 'i',
        'ȷ' => 'j',
        l => l,
    };
    let composed: String = [letter, mark].iter().collect::<String>().nfc().collect();
    Some((end, composed))
}

/// Commands that typeset their argument as it is (in a box): the name
/// and the braces are markup.
fn transparent(name: &str) -> bool {
    matches!(
        name,
        "captionof"
            | "hyperref"
            | "colorbox"
            | "fcolorbox"
            | "href@noop"
            // Springer Nature's and others' parts of an author's name and
            // affiliation, and a class's `\abstract{…}`.
            | "fnm"
            | "sur"
            | "orgname"
            | "orgdiv"
            | "orgaddress"
            | "street"
            | "city"
            | "postcode"
            | "state"
            | "country"
            | "abstract"
            | "mbox"
            | "hbox"
            | "makebox"
            | "fbox"
            | "framebox"
            | "text"
            | "textnormal"
            | "nolinkurl"
            | "resizebox"
            | "scalebox"
            | "rotatebox"
            | "raisebox"
            | "adjustbox"
            | "parbox"
            | "centerline"
            | "textcolor"
            | "multicolumn"
            | "multirow"
            | "makecell"
            | "thead"
            | "footnotetext"
            | "bibinfo"
            | "bibfield"
            | "bibnamefont"
            | "bibfnamefont"
            | "bibsnamefont"
            | "bibeditornamefont"
            | "natexlab"
    )
}

/// Whether brace token `t` only groups: a plain group's (`{\bf x}`), an
/// empty group's (`\LaTeX{}`), or the argument's of a command that
/// typesets it as it is (`\mbox{x}`).
fn brace_is_markup(t: &SyntaxToken) -> bool {
    let Some(g) = t.parent().filter(|g| g.kind() == K::GROUP) else {
        return false;
    };
    if g.children_with_tokens().count() == 2 {
        return true;
    }
    match g.parent() {
        Some(p) if p.kind() == K::COMMAND => {
            latex_syntax::name(&p).is_some_and(|n| transparent(&n))
        }
        Some(p) => matches!(p.kind(), K::PARAGRAPH | K::GROUP | K::BODY),
        None => false,
    }
}

/// `s` upper or lower cased as LaTeX's `\\MakeUppercase` does: not the
/// micro sign, the ohm, kelvin and ångström signs, symbols (not Greek or
/// Latin letters) to LaTeX.
fn change_case(s: &str, upper: bool) -> String {
    s.chars()
        .map(|c| match c {
            '\u{b5}' | '\u{2126}' | '\u{212a}' | '\u{212b}' => c.to_string(),
            _ if upper => c.to_uppercase().collect(),
            _ => c.to_lowercase().collect(),
        })
        .collect()
}

/// Where the group at `at` ends, when it does before `limit`: the
/// argument of `\MakeUppercase{x {y}}`.
fn group_end(text: &str, at: usize, limit: usize) -> Option<usize> {
    let rest = text.get(at..limit)?;
    if !rest.starts_with('{') {
        return None;
    }
    let mut depth = 0;
    let mut escaped = false;
    for (i, ch) in rest.char_indices() {
        match ch {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + i + 1);
                }
            }
            '%' | '$' => return None,
            _ => {}
        }
    }
    None
}

/// What a control symbol typesets in text.
fn symbol(s: &str) -> Option<&'static str> {
    Some(match s {
        "\\&" => "&",
        "\\%" => "%",
        "\\$" => "$",
        "\\#" => "#",
        "\\_" => "_",
        "\\{" => "{",
        "\\}" => "}",
        "\\," => "\u{2009}",
        // LaTeX's thick and medium spaces, in text as in math.
        "\\;" => "\u{2005}",
        "\\:" | "\\>" => "\u{205f}",
        "\\!" => "",
        // A control space: also a backslash before a line's end or a tab.
        "\\ " | "\\\t" | "\\\n" | "\\\r\n" | "\\\r" => " ",
        "\\@" => "",
        "\\/" => "",
        "\\-" => "",
        _ => return None,
    })
}

/// What a command without arguments typesets.
/// What a document's own macro prints in text, when its definition is
/// text the view can show without the renderer (`\newcommand{\ie}{i.e.\xspace}`,
/// `\newcommand{\method}{\textsc{Foo}}`): the text, its style, whether it
/// ends in `\xspace` (TeX keeps the space after it), and the arguments it
/// takes (shown only when it prints nothing, as `\todo`'s `{}`).
pub(crate) struct OwnMacro {
    pub text: String,
    pub style: Style,
    pub xspace: bool,
    pub args: usize,
    pub default: bool,
}

pub(crate) fn own_macro(model: &latex_model::Model, name: &str) -> Option<OwnMacro> {
    own_macro_depth(model, name, 0)
}

fn own_macro_depth(model: &latex_model::Model, name: &str, depth: usize) -> Option<OwnMacro> {
    if depth > 8 {
        return None;
    }
    let m = model
        .macros
        .iter()
        .rev()
        .find(|m| m.name.strip_prefix('\\') == Some(name))?;
    let body = m.body.trim();
    if m.args > 0 {
        // A note to self (`\newcommand{\todo}[1]{}`): nothing printed.
        return body.is_empty().then(|| OwnMacro {
            text: String::new(),
            style: Style::default(),
            xspace: false,
            args: m.args,
            default: m.default.is_some(),
        });
    }
    let mut style = Style::default();
    let mut inner = body;
    // One wrapper around the whole: its style.
    for (cmd, set) in [
        ("\\textbf{", 0),
        ("\\textit{", 1),
        ("\\emph{", 1),
        ("\\texttt{", 2),
        ("\\textsc{", 3),
        ("\\textrm{", 3),
        ("\\textsf{", 3),
        ("\\mbox{", 3),
        ("\\text{", 3),
        ("\\textnormal{", 3),
        ("\\textup{", 3),
    ] {
        if let Some(r) = inner.strip_prefix(cmd)
            && let Some(close) = matching_brace(r)
            && r[close + 1..].trim().is_empty()
        {
            inner = &r[..close];
            match set {
                0 => style.bold = true,
                1 => style.italic = true,
                2 => style.code = true,
                _ => {}
            }
            break;
        }
    }
    let (out, xspace) = plain_text(model, inner, depth)?;
    Some(OwnMacro {
        text: out,
        style,
        xspace,
        args: 0,
        default: false,
    })
}

/// The text LaTeX prints for `inner` when it is only text, letters the
/// view knows, spaces and the document's own text macros; whether it
/// ends in `\xspace`.
fn plain_text(model: &latex_model::Model, inner: &str, depth: usize) -> Option<(String, bool)> {
    let mut out = String::new();
    let mut xspace = false;
    let b = inner.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' | b'}' => i += 1,
            b'~' => {
                out.push('\u{a0}');
                i += 1;
            }
            b'$' | b'^' | b'_' | b'&' | b'#' | b'%' => return None,
            b'\\' => {
                let rest = &inner[i + 1..];
                let n = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
                if n == 0 {
                    let c = rest.chars().next()?;
                    match c {
                        ' ' => out.push(' '),
                        '@' | '/' | ',' => {}
                        '&' | '%' | '#' | '_' | '$' => out.push(c),
                        _ => return None,
                    }
                    i += 1 + c.len_utf8();
                    continue;
                }
                let cname = &rest[..n];
                i += 1 + n;
                match cname {
                    "xspace" => xspace = true,
                    "textbf" | "textit" | "emph" | "textsc" | "textrm" | "textsf" | "texttt"
                    | "mbox" | "text" | "textnormal" | "textup" | "relax" | "protect" => {}
                    _ => {
                        if let Some(w) = word(cname) {
                            out.push_str(w);
                        } else if let Some(o) = own_macro_depth(model, cname, depth + 1)
                            && o.args == 0
                        {
                            out.push_str(&o.text);
                        } else {
                            return None;
                        }
                        // A control word takes the spaces after it.
                        while i < b.len() && b[i] == b' ' {
                            i += 1;
                        }
                    }
                }
            }
            _ => {
                let c = inner[i..].chars().next()?;
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    Some((out, xspace))
}

/// What the glossary command `cmd` (`gls`, `acp`, `acrfull`, …) prints
/// for entry `key` used at `start` of the document, as LaTeX: an
/// acronym's long form and short one at its first use, its short form
/// after; plurals and capitals as the command asks.
pub(crate) fn glossary_use(
    model: &latex_model::Model,
    cmd: &str,
    key: &str,
    start: usize,
) -> Option<String> {
    let e = model.glossary_entry(key)?;
    let lower = cmd.to_lowercase();
    let plural =
        lower.ends_with("pl") || matches!(lower.as_str(), "acp" | "acsp" | "aclp" | "acfp");
    let short = if plural {
        e.plural.clone().unwrap_or_else(|| format!("{}s", e.name))
    } else {
        e.name.clone()
    };
    let long = e
        .long
        .as_ref()
        .map(|l| if plural { format!("{l}s") } else { l.clone() });
    let full = || match &long {
        Some(l) => format!("{l} ({short})"),
        None => short.clone(),
    };
    let text = match lower.as_str() {
        "gls" | "glspl" | "ac" | "acp" => {
            if long.is_some() && model.first_use(key, 0, start) {
                full()
            } else {
                short.clone()
            }
        }
        "acs" | "acsp" | "acrshort" | "acrshortpl" | "glsxtrshort" | "glsentryshort" => short,
        "acl" | "aclp" | "acrlong" | "acrlongpl" | "glsxtrlong" | "glsentrylong" => {
            long.clone().unwrap_or(short)
        }
        "acf" | "acfp" | "acrfull" | "acrfullpl" | "glsxtrfull" => full(),
        "glssymbol" => e.symbol.clone().unwrap_or(short),
        "glsentryname" | "glsentrytext" => e.name.clone(),
        _ => return None,
    };
    Some(
        if cmd.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase()) {
            text.to_uppercase()
        } else if cmd.starts_with(|c: char| c.is_ascii_uppercase()) {
            let mut c = text.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        } else {
            text
        },
    )
}

/// A definition with the glossary entries it uses
/// (`\\newcommand{\\fee}{{\\gls[hyper=false]{fee}}}`) as what they print: the
/// renderer has no glossary.
fn glossary_in_definition(model: &latex_model::Model, def: &str) -> String {
    if !def.contains("\\gls") && !def.contains("\\ac") {
        return def.to_string();
    }
    let mut out = String::with_capacity(def.len());
    let mut rest = def;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let n = after.bytes().take_while(u8::is_ascii_alphabetic).count();
        let name = &after[..n];
        let mut tail = after[n..].trim_start();
        if tail.starts_with('*') {
            tail = &tail[1..];
        }
        if tail.starts_with('[')
            && let Some(k) = tail.find(']')
        {
            tail = &tail[k + 1..];
        }
        let found = tail.strip_prefix('{').and_then(|r| {
            let k = r.find('}')?;
            let key = r[..k].trim();
            // An entry the model has not read (in a glossary file of its
            // own): its key, as the closest to its name.
            let shown = glossary_use(model, name, key, usize::MAX).or_else(|| {
                matches!(name, "gls" | "Gls" | "glspl" | "ac" | "acs" | "acl" | "acf")
                    .then(|| key.to_string())
            })?;
            Some((shown, &r[k + 1..]))
        });
        match found {
            Some((shown, r)) => {
                match entry_math(&shown) {
                    Some(m) => out.push_str(&format!("{{{m}}}")),
                    None => out.push_str(&format!("\\text{{{shown}}}")),
                }
                rest = r;
            }
            None => {
                out.push('\\');
                out.push_str(name);
                rest = &after[n..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A glossary command's key: its first group.
fn glossary_key(cmd: &SyntaxNode) -> Option<String> {
    cmd.children()
        .find(|c| c.kind() == K::GROUP)
        .map(|g| group_text(&g).trim().to_string())
}

/// What a glossary command prints, for coverage: `None` when the view
/// does not show it (no such entry).
pub(crate) fn glossary_shown(model: &latex_model::Model, cmd: &SyntaxNode) -> Option<String> {
    let name = latex_syntax::name(cmd)?;
    let key = glossary_key(cmd)?;
    glossary_use(model, &name, &key, node_span(cmd).start)
}

/// Whether the view draws a glossary command: as text, or as a formula.
pub(crate) fn glossary_drawn(model: &latex_model::Model, cmd: &SyntaxNode) -> bool {
    glossary_shown(model, cmd)
        .is_some_and(|s| entry_math(&s).is_some() || plain_text(model, &s, 0).is_some())
}

/// The math of an entry's text (`$\gamma$`, `\ensuremath{\gamma}`), if it
/// is a formula.
fn entry_math(text: &str) -> Option<&str> {
    let t = text.trim();
    t.strip_prefix('$')
        .and_then(|r| r.strip_suffix('$'))
        .or_else(|| {
            t.strip_prefix("\\ensuremath{")
                .and_then(|r| r.strip_suffix('}'))
        })
}

/// The `}` that closes the group whose `{` is just before `s`.
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn word(name: &str) -> Option<&'static str> {
    Some(match name {
        "ldots" | "dots" | "textellipsis" => "\u{2026}",
        // Vertical space and a discretionary hyphen typeset nothing here.
        "medskip" | "smallskip" | "bigskip" | "vfill" | "par" | "noindent" | "indent" => "",
        // A page break: a mark, dimmed like the line break's.
        "newpage" | "clearpage" | "cleardoublepage" | "pagebreak" => "\u{21a1}",
        "slash" => "/",
        "textquotesingle" => "'",
        "textvisiblespace" => "\u{2423}",
        "guillemotleft" | "guillemetleft" => "«",
        "guillemotright" | "guillemetright" => "»",
        "guilsinglleft" => "‹",
        "guilsinglright" => "›",
        // The headings LaTeX prints for these lists.
        "listoffigures" => "List of Figures",
        "listoftables" => "List of Tables",
        "printbibliography" => "References",
        // Special letters.
        "ss" => "ß",
        "ae" => "æ",
        "AE" => "Æ",
        "oe" => "œ",
        "OE" => "Œ",
        "o" => "ø",
        "O" => "Ø",
        "aa" => "å",
        "AA" => "Å",
        "l" => "ł",
        "L" => "Ł",
        "i" => "ı",
        "j" => "ȷ",
        "th" => "þ",
        "TH" => "Þ",
        "dh" => "ð",
        "DH" => "Ð",
        "ng" => "ŋ",
        "NG" => "Ŋ",
        "dj" => "đ",
        "DJ" => "Đ",
        "LaTeX" => "LaTeX",
        "TeX" => "TeX",
        "textendash" => "\u{2013}",
        "textemdash" => "\u{2014}",
        "S" => "\u{a7}",
        "P" => "\u{b6}",
        "copyright" => "\u{a9}",
        "quad" => "\u{2003}",
        "qquad" => "\u{2003}\u{2003}",
        "newline" | "linebreak" => "\u{21b5}",
        // textcomp's characters, and spacing shown as a space.
        "texteuro" | "euro" => "\u{20ac}",
        "textcopyright" => "\u{a9}",
        "textregistered" => "\u{ae}",
        "texttrademark" => "\u{2122}",
        "textdegree" => "\u{b0}",
        "textonehalf" => "\u{bd}",
        "textonequarter" => "\u{bc}",
        "textthreequarters" => "\u{be}",
        "textasciitilde" => "~",
        "textasciicircum" => "^",
        "textbackslash" => "\\",
        "textbar" => "|",
        "textless" => "<",
        "textgreater" => ">",
        "textbullet" => "\u{2022}",
        "textdagger" | "dag" => "\u{2020}",
        "textdaggerdbl" | "ddag" => "\u{2021}",
        "textpilcrow" => "\u{b6}",
        "textsection" => "\u{a7}",
        "textperiodcentered" => "\u{b7}",
        "textquoteleft" => "\u{2018}",
        "textquoteright" => "\u{2019}",
        "textquotedblleft" => "\u{201c}",
        "textquotedblright" => "\u{201d}",
        "pounds" | "textsterling" => "\u{a3}",
        "textyen" => "\u{a5}",
        "textcent" => "\u{a2}",
        "textmu" => "\u{b5}",
        "textpm" => "\u{b1}",
        "texttimes" => "\u{d7}",
        "textdiv" => "\u{f7}",
        "hfill" | "hfil" | "enspace" | "enskip" => "\u{2002}",
        "thinspace" => "\u{2009}",
        "LaTeXe" => "LaTeX2\u{3b5}",
        "METAFONT" => "METAFONT",
        "textexclamdown" => "\u{a1}",
        "textquestiondown" => "\u{bf}",
        "textparagraph" => "\u{b6}",
        "textordfeminine" => "\u{aa}",
        "textordmasculine" => "\u{ba}",
        "textbrokenbar" => "\u{a6}",
        "textasteriskcentered" => "\u{2217}",
        "textquotedbl" => "\"",
        "textasciigrave" => "`",
        "textunderscore" => "_",
        "textbraceleft" => "{",
        "textbraceright" => "}",
        "textnumero" => "\u{2116}",
        "textcelsius" => "\u{2103}",
        "textohm" => "\u{2126}",
        "textleftarrow" => "\u{2190}",
        "textrightarrow" => "\u{2192}",
        "textuparrow" => "\u{2191}",
        "textdownarrow" => "\u{2193}",
        "textestimated" => "\u{212e}",
        "textreferencemark" => "\u{203b}",
        "textmusicalnote" => "\u{266a}",
        "textlira" => "\u{20a4}",
        "textwon" => "\u{20a9}",
        "textnaira" => "\u{20a6}",
        "textpeso" => "\u{20b1}",
        "textflorin" => "\u{192}",
        "textcurrency" => "\u{a4}",
        "textonesuperior" => "\u{b9}",
        "texttwosuperior" => "\u{b2}",
        "textthreesuperior" => "\u{b3}",
        "textlnot" => "\u{ac}",
        "textminus" => "\u{2212}",
        "textfractionsolidus" => "\u{2044}",
        "textdblhyphen" => "\u{2e40}",
        "textinterrobang" => "\u{203d}",
        "textopenbullet" => "\u{25e6}",
        "textbigcircle" => "\u{25ef}",
        "textdied" => "\u{2020}",
        "textborn" => "\u{2605}",
        "textmarried" => "\u{26ad}",
        "textdivorced" => "\u{26ae}",
        "textleaf" => "\u{1f343}",
        "textrecipe" => "\u{211e}",
        "textservicemark" => "\u{2120}",
        "textdiscount" => "\u{2052}",
        "textpertenthousand" => "\u{2031}",
        "textperthousand" => "\u{2030}",
        "textsurd" => "\u{221a}",
        "textbaht" => "\u{e3f}",
        "textdong" => "\u{20ab}",
        "textguarani" => "\u{20b2}",
        "textcolonmonetary" => "\u{20a1}",
        "textsci" => "\u{29c}",
        "textquotestraightbase" => "\u{201a}",
        "textquotestraightdblbase" | "quotedblbase" => "\u{201e}",
        "quotesinglbase" => "\u{201a}",
        "space" => " ",
        "nobreakspace" => "\u{a0}",
        "relax" | "leavevmode" | "nolinebreak" | "nopagebreak" | "allowbreak" | "newblock" => "",
        // Layout, spacing and page settings: nothing printed where they are.
        "onecolumn"
        | "FloatBarrier"
        | "raggedright"
        | "raggedleft"
        | "onehalfspacing"
        | "doublespacing"
        | "singlespacing"
        | "IEEEpeerreviewmaketitle"
        | "endfirsthead"
        | "endhead"
        | "endfoot"
        | "endlastfoot"
        | "selectfont"
        | "begingroup"
        | "endgroup"
        | "protect"
        | "expandafter"
        | "balance"
        | "IEEEoverridecommandlockouts"
        | "nolinenumbers"
        | "linenumbers" => "",
        _ => return None,
    })
}

struct Builder<'a> {
    text: &'a str,
    line: Range<usize>,
    runs: Vec<Run>,
}

impl Builder<'_> {
    fn verbatim(&mut self, src: Range<usize>, style: Style) {
        let src = src.start.max(self.line.start)..src.end.min(self.line.end);
        if src.is_empty() {
            return;
        }
        if let Some(last) = self.runs.last_mut()
            && last.verbatim
            && last.widget.is_none()
            && last.src.end == src.start
            && last.style == style
        {
            last.text.push_str(&self.text[src.clone()]);
            last.src.end = src.end;
            return;
        }
        self.runs.push(Run {
            text: self.text[src.clone()].to_string(),
            src,
            verbatim: true,
            style,
            widget: None,
        });
    }

    fn replace(&mut self, src: Range<usize>, text: &str, style: Style) {
        self.runs.push(Run {
            src,
            text: text.to_string(),
            verbatim: false,
            style,
            widget: None,
        });
    }
}

/// The view of source line `line` (without its line ending) of the LaTeX
/// document `doc`, with the cursor at `cursor`; the text under the
/// document's diagnostics flagged (T2.7h.20).
pub fn line_view(
    doc: &crate::DocumentState,
    line: Range<usize>,
    cursor: Option<usize>,
) -> LineView {
    let mut v = unflagged_line_view(doc, line.clone(), cursor);
    if let Some(diags) = doc.latex_diagnostics() {
        flag(&mut v, diags);
    }
    // A paragraph's first line indented as TeX indents it (`\parindent`,
    // 1.5 em in the standard classes), away from the cursor.
    if !cursor.is_some_and(|c| line.start <= c && c <= line.end) && indented(doc, line.clone()) {
        v.runs.insert(
            0,
            Run {
                src: line.start..line.start,
                text: "\u{2003}\u{2002}".into(),
                verbatim: false,
                // Space, not text: as markup.
                style: Style {
                    dim: true,
                    ..Style::default()
                },
                widget: None,
            },
        );
    }
    v
}

/// Whether TeX indents source line `line` as a paragraph's first line:
/// running text of the document after a blank line, not after a heading,
/// and not `\noindent` or an item.
fn indented(doc: &crate::DocumentState, line: Range<usize>) -> bool {
    let Some(state) = doc.latex() else {
        return false;
    };
    let text = doc.text().as_str();
    let t = text[line.clone()].trim_start();
    if t.starts_with("\\noindent") || t.starts_with("\\item") || line.start == 0 {
        return false;
    }
    let root = state.parse().syntax();
    if paragraph_line(state, &root, text, line.clone()).is_none() {
        return false;
    }
    // Directly in the document (lists, centered text, boxes: no indent).
    let first = line.start + (text[line.clone()].len() - t.len());
    let in_document = latex_syntax::token_at(&root, first).is_some_and(|tok| {
        tok.parent_ancestors()
            .filter(|a| a.kind() == K::ENVIRONMENT)
            .all(|a| latex_syntax::name(&a).as_deref() == Some("document"))
    });
    if !in_document {
        return false;
    }
    // A blank line before it, and before that no heading.
    let mut lines = text[..line.start - 1].rsplit('\n');
    if !lines.next().is_some_and(|l| l.trim().is_empty()) {
        return false;
    }
    let before = lines.find(|l| !l.trim().is_empty()).unwrap_or("");
    let b = before.trim_start();
    let heading = [
        "\\part",
        "\\chapter",
        "\\section",
        "\\subsection",
        "\\subsubsection",
        "\\paragraph",
    ]
    .iter()
    .any(|h| b.starts_with(h));
    !heading
}

/// Whether source line `line` (its line feed out) is running text of a
/// paragraph, and if so whether the next line follows it in the same
/// one (its line break a space); `None` for a line TeX does not set as
/// running text (blank, a comment, a command line of its own, in a table,
/// a formula, verbatim...).
fn paragraph_line(
    state: &LatexState,
    root: &SyntaxNode,
    text: &str,
    line: Range<usize>,
) -> Option<bool> {
    let src = &text[line.clone()];
    let t = src.trim();
    if t.is_empty() || t.starts_with('%') {
        return None;
    }
    // A line of its own: an environment's edge, a heading, a display.
    for p in [
        "\\begin",
        "\\end",
        "\\[",
        "\\]",
        "$$",
        "\\caption",
        "\\centering",
        "\\label",
        "\\maketitle",
        "\\documentclass",
        "\\usepackage",
        "\\input",
        "\\include",
        "\\bibliography",
        "\\newcommand",
        "\\renewcommand",
        "\\def",
        "\\clearpage",
        "\\newpage",
        "\\noindent",
        "\\vspace",
        "\\hline",
        "\\toprule",
        "\\midrule",
        "\\bottomrule",
        "\\includegraphics",
        "\\appendix",
        "\\tableofcontents",
    ] {
        if t.starts_with(p) && !(p == "\\noindent" && t.len() > p.len()) {
            return None;
        }
    }
    let first = line.start + (src.len() - src.trim_start().len());
    let tok = latex_syntax::token_at(root, first)?;
    // A heading, or a command whose argument is the whole line.
    if let Some(cmd) = tok.parent().filter(|p| p.kind() == K::COMMAND)
        && let Some(name) = latex_syntax::name(&cmd)
        && (latex_syntax::signatures::is_sectioning(&name)
            || matches!(name.as_str(), "title" | "author" | "date" | "paragraph"))
    {
        return None;
    }
    // In running text: the document, a list, a quote, a theorem, a proof,
    // a box of text; nothing else (tables, formulas, verbatim, pictures).
    let model = state.model();
    for a in tok.parent_ancestors() {
        if matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH) && node_span(&a).start < line.start
        {
            return None;
        }
        if a.kind() == K::ENVIRONMENT {
            let name = latex_syntax::name(&a).unwrap_or_default();
            let ok = matches!(
                name.as_str(),
                "document"
                    | "abstract"
                    | "quote"
                    | "quotation"
                    | "itemize"
                    | "enumerate"
                    | "description"
                    | "proof"
                    | "minipage"
                    | "center"
                    | "flushleft"
                    | "flushright"
            ) || front_environment(&name).is_some()
                || model.theorem_kinds.iter().any(|k| k.env == name);
            if !ok {
                return None;
            }
        }
    }
    // The line break is a space unless the line forces one (`\\`,
    // `\\newline`, `\\par`) or ends in a comment, which takes the break.
    let code = t.split('%').next().unwrap_or(t).trim_end();
    let breaks = code.ends_with("\\\\")
        || code.ends_with("\\newline")
        || code.ends_with("\\par")
        || t.contains('%');
    Some(!breaks)
}

/// The runs of source lines TeX sets as one paragraph (two lines or more,
/// one line break a space between them), as byte ranges from the first
/// line's start to the last line's end (its line feed out).
pub fn joined_paragraphs(doc: &crate::DocumentState) -> Paragraphs {
    let Some(state) = doc.latex() else {
        return Arc::new(Vec::new());
    };
    if let Some((t, p)) = state.paragraphs.borrow().as_ref()
        && Arc::ptr_eq(t, &state.text)
    {
        return p.clone();
    }
    let found = Arc::new(find_paragraphs(doc, state));
    *state.paragraphs.borrow_mut() = Some((state.text.clone(), found.clone()));
    found
}

fn find_paragraphs(doc: &crate::DocumentState, state: &LatexState) -> Vec<Range<usize>> {
    let text = doc.text().as_str();
    let root = state.parse().syntax();
    let body = state.model().body.clone().unwrap_or(0..text.len());
    let mut out = Vec::new();
    let mut run: Option<(usize, usize, bool)> = None;
    let mut at = body.start;
    for raw in text[body.start..body.end.min(text.len())].split_inclusive('\n') {
        let line = at..at + raw.trim_end_matches(['\n', '\r']).len();
        at += raw.len();
        let p = paragraph_line(state, &root, text, line.clone());
        // A line starting with `\\item` begins a run of its own.
        let item = text[line.clone()].trim_start().starts_with("\\item");
        match (run, p) {
            (Some((s, _, true)), Some(joins)) if !item => run = Some((s, line.end, joins)),
            (prev, p) => {
                if let Some((s, e, _)) = prev
                    && text[s..e].contains('\n')
                {
                    out.push(s..e);
                }
                run = p.map(|joins| (line.start, line.end, joins));
            }
        }
    }
    if let Some((s, e, _)) = run
        && text[s..e].contains('\n')
    {
        out.push(s..e);
    }
    out
}

/// The paragraph `range` (one of [`joined_paragraphs`]) as one line: its
/// lines' views one after the other, each line break shown as a space.
pub fn paragraph_view(
    doc: &crate::DocumentState,
    range: Range<usize>,
    cursor: Option<usize>,
) -> LineView {
    let text = doc.text().as_str();
    let mut out: Option<LineView> = None;
    let mut at = range.start;
    for raw in text[range.clone()].split_inclusive('\n') {
        let content = raw.trim_end_matches(['\n', '\r']).len();
        let line = at..at + content;
        let v = line_view(doc, line.clone(), cursor);
        match &mut out {
            None => out = Some(v),
            Some(o) => {
                // TeX's space for the line break, after what the line before
                // ends with (blanks at a line's end are TeX's one space too).
                let prev_end = o.runs.last().map_or(line.start, |r| r.src.end);
                let gap = prev_end..line.start;
                let ends_blank = o.runs.last().is_some_and(|r| r.text.ends_with(' '));
                o.runs.push(Run {
                    src: gap,
                    text: if ends_blank {
                        String::new()
                    } else {
                        " ".into()
                    },
                    verbatim: false,
                    style: Style::default(),
                    widget: None,
                });
                // The next line's leading blanks are not more space.
                let mut runs = v.runs;
                if let Some(r) = runs.first_mut()
                    && r.widget.is_none()
                {
                    let trimmed = r.text.trim_start().to_string();
                    r.text = trimmed;
                }
                o.runs.extend(runs);
            }
        }
        at += raw.len();
    }
    let mut v = out.unwrap_or_default();
    v.range = range;
    v
}

/// Flags the text of line view `v` (the source view's, say) under the
/// diagnostics of the LaTeX document `doc`.
pub fn flag_diagnostics(doc: &crate::DocumentState, v: &mut LineView) {
    if let Some(diags) = doc.latex_diagnostics() {
        flag(v, diags);
    }
}

/// Flags the runs of `v` under `diags`: a run of source text split where a
/// diagnostic starts or ends, a run standing for other text flagged whole.
fn flag(v: &mut LineView, diags: &[crate::latex_check::Diagnostic]) {
    let line = v.range.clone();
    // Diagnostics are in order of their start, and one over many lines
    // is flagged on its first: those starting on this line, up to its end.
    let first = diags.partition_point(|d| d.range.start < line.start);
    let here: Vec<(Range<usize>, bool)> = diags[first..]
        .iter()
        .take_while(|d| d.range.start <= line.end)
        .map(|d| {
            // At least one character, so a point shows.
            let end = d.range.end.min(line.end).max(d.range.start + 1);
            let warning = d.severity == crate::latex_check::Severity::Warning;
            (d.range.start..end, warning)
        })
        .collect();
    if here.is_empty() {
        return;
    }
    let flag_of = |r: &Range<usize>| -> Option<bool> {
        let hits = here
            .iter()
            .filter(|(d, _)| d.start < r.end.max(r.start + 1) && d.end > r.start);
        hits.map(|(_, w)| *w).reduce(|a, b| a || b)
    };
    let mut out = Vec::with_capacity(v.runs.len());
    for run in std::mem::take(&mut v.runs) {
        if !run.verbatim || run.widget.is_some() || run.src.len() != run.text.len() {
            let mut run = run;
            if run.style.flagged.is_none() {
                run.style.flagged = flag_of(&run.src);
            }
            out.push(run);
            continue;
        }
        // Cut where diagnostics start and end.
        let mut cuts: Vec<usize> = here
            .iter()
            .flat_map(|(d, _)| [d.start, d.end])
            .filter(|c| {
                run.src.start < *c
                    && *c < run.src.end
                    && run.text.is_char_boundary(c - run.src.start)
            })
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        let mut start = run.src.start;
        for c in cuts.into_iter().chain(std::iter::once(run.src.end)) {
            let src = start..c;
            let mut piece = run.clone();
            piece.text = run.text[start - run.src.start..c - run.src.start].to_string();
            piece.style.flagged = flag_of(&src);
            piece.src = src;
            out.push(piece);
            start = c;
        }
    }
    v.runs = out;
}

fn unflagged_line_view(
    doc: &crate::DocumentState,
    line: Range<usize>,
    cursor: Option<usize>,
) -> LineView {
    let text = doc.text().as_str();
    let Some(state) = doc.latex() else {
        return crate::view::plain_line_view(text, line, cursor);
    };
    let mut v = LineView {
        range: line.clone(),
        ..LineView::default()
    };
    if line.is_empty() {
        return v;
    }
    let root = state.parse().syntax();
    let near = |r: &Range<usize>| cursor.is_some_and(|c| r.start <= c && c <= r.end);
    let mut b = Builder {
        text,
        line: line.clone(),
        runs: Vec::new(),
    };
    let dim = Style {
        dim: true,
        ..Style::default()
    };
    let mut tok = latex_syntax::token_at(&root, line.start);
    // An abstract's title line, centered.
    let mut title_line = false;
    // In a list: the line's indentation by its depth, the source's own
    // blanks hidden (unless the cursor is in them).
    let mut first = tok.clone();
    while let Some(t) = &first
        && matches!(t.kind(), K::WHITESPACE)
        && span(t).end < line.end
    {
        first = t.next_token();
    }
    if let Some(f) = first.clone().filter(|f| span(f).start < line.end)
        && let Some(parent) = f.parent()
    {
        let fs = span(&f).start;
        let depth = lists_around(&parent).len() + quotes_around(&parent);
        let delimiter = f
            .parent_ancestors()
            .find(|a| matches!(a.kind(), K::BEGIN | K::END));
        if let Some(d) = &delimiter
            && let Some(env) = d.parent()
            && latex_syntax::name(&env).is_some_and(|n| {
                is_list(&n)
                    || float_name(&n, false).is_some()
                    || matches!(
                        n.as_str(),
                        "center"
                            | "flushleft"
                            | "flushright"
                            | "quote"
                            | "quotation"
                            | "verse"
                            | "abstract"
                            | "subequations"
                    )
                    || front_environment(&n).is_some()
            })
            && node_span(d).end >= line.start + text[line.clone()].trim_end().len()
        {
            // An abstract's title, as the article class prints it.
            let ds = node_span(d);
            let env_name = d.parent().and_then(|e| latex_syntax::name(&e));
            let front_head = env_name.as_deref().and_then(front_environment);
            if d.kind() == K::BEGIN
                && let Some(head) = front_head.filter(|h| !h.is_empty())
                && !near(&ds)
            {
                let bold = Style {
                    bold: true,
                    ..Style::default()
                };
                b.replace(ds.clone(), head, bold);
                while let Some(n) = &tok
                    && span(n).start < ds.end
                {
                    tok = n.next_token();
                }
            } else if d.kind() == K::BEGIN && env_name.as_deref() == Some("abstract") && !near(&ds)
            {
                let model = state.model();
                let turkish = model
                    .packages
                    .iter()
                    .any(|p| p.name == "babel" && p.options.iter().any(|o| o == "turkish"))
                    || model
                        .class
                        .as_ref()
                        .is_some_and(|c| c.options.iter().any(|o| o == "turkish"));
                let bold = Style {
                    bold: true,
                    ..Style::default()
                };
                b.replace(ds.clone(), if turkish { "Özet" } else { "Abstract" }, bold);
                title_line = true;
                while let Some(n) = &tok
                    && span(n).start < ds.end
                {
                    tok = n.next_token();
                }
            } else {
                v.role = crate::view::LineRole::Delimiter;
            }
        } else if depth > 0 && !near(&(line.start..fs)) {
            let item = f.kind() == K::CONTROL_WORD && &text[span(&f)] == "\\item";
            tok = first.clone();
            if !item {
                b.replace(fs..fs, &"\u{2003}\u{2003}".repeat(depth), Style::default());
            }
        }
    }
    // Code: the lines of a verbatim environment's body, monospace; its
    // `\begin` and `\end` lines delimiters; a `comment` dimmed.
    let mut dimmed = false;
    if let Some(env) = verbatim_env(&root, line.start) {
        let body = env
            .children()
            .find(|c| c.kind() == K::BODY)
            .map(|b| node_span(&b));
        let name = latex_syntax::name(&env).unwrap_or_default();
        match body {
            Some(bs) if bs.start <= line.start && line.end <= bs.end => {
                v.mono = true;
                dimmed = name == "comment";
            }
            _ => v.role = crate::view::LineRole::Delimiter,
        }
    }
    let skipped = state.skipped(text);
    let mut heading_command: Option<SyntaxNode> = None;
    // Source ranges not shown (a caption's closing brace).
    let mut hidden: Vec<Range<usize>> = Vec::new();
    // Source ranges shown as other text (the break between a
    // sub-caption and its body).
    let mut replaced: Vec<(Range<usize>, &'static str, Style)> = Vec::new();
    // The arguments of case changes: whether to upper case, and whether
    // by TeX's primitive (which changes characters, not `\\ss` or `\\i`).
    let mut cases: Vec<(Range<usize>, bool, bool)> = Vec::new();
    while let Some(t) = tok {
        let r = span(&t);
        if r.start >= line.end {
            break;
        }
        tok = t.next_token();
        // A bibliography's `\begin`: its heading, as LaTeX prints it
        // (`\section*{References}`, `\chapter*{Bibliography}`).
        if t.kind() == K::CONTROL_WORD
            && t.text() == "\\begin"
            && let Some(begin) = t.parent().filter(|p| p.kind() == K::BEGIN)
            && begin
                .parent()
                .and_then(|e| latex_syntax::name(&e))
                .as_deref()
                == Some("thebibliography")
            && !near(&node_span(&begin))
        {
            let bs = node_span(&begin);
            let chapters = state
                .model()
                .class
                .as_ref()
                .is_some_and(|c| latex_model::has_chapters(&c.name));
            let title = if chapters {
                "Bibliography"
            } else {
                "References"
            };
            b.replace(bs.start..bs.end.min(line.end), title, Style::default());
            v.heading = 1;
            v.role = crate::view::LineRole::Content;
            while let Some(n) = &tok
                && span(n).start < bs.end
            {
                tok = n.next_token();
            }
            continue;
        }
        // `\verb|…|` and `\lstinline{…}` away from the cursor: the code,
        // monospace, the command and delimiters hidden.
        if t.kind() == K::CONTROL_WORD
            && let Some(verb) = t.parent().filter(|p| p.kind() == K::VERB)
            && !near(&node_span(&verb))
            && let Some(code) = verb_code(text, &verb)
        {
            let vs = node_span(&verb);
            if vs.end <= line.end {
                let code_style = Style {
                    code: true,
                    ..Style::default()
                };
                b.replace(vs.start..code.start, "", Style::default());
                // `\verb*` shows its spaces as ␣.
                let starred = text[span(&t).end..code.start].starts_with('*');
                if starred && text[code.clone()].contains(' ') {
                    let mut at = code.start;
                    for (i, _) in text[code.clone()].match_indices(' ') {
                        let sp = code.start + i;
                        b.verbatim(at..sp, code_style);
                        b.replace(sp..sp + 1, "\u{2423}", code_style);
                        at = sp + 1;
                    }
                    b.verbatim(at..code.end, code_style);
                } else {
                    b.verbatim(code.clone(), code_style);
                }
                b.replace(code.end..vs.end, "", Style::default());
                while let Some(n) = &tok
                    && span(n).start < vs.end
                {
                    tok = n.next_token();
                }
                continue;
            }
        }
        // `\item`: its bullet, number or label, indented by its depth.
        if t.kind() == K::CONTROL_WORD
            && &text[r.clone()] == "\\item"
            && let Some(cmd) = t.parent().filter(|p| p.kind() == K::COMMAND)
            && let Some((env, name)) = lists_around(&cmd).into_iter().next()
            && !near(&node_span(&cmd))
        {
            let cs = node_span(&cmd);
            let depth = lists_around(&cmd).len() + quotes_around(&cmd);
            let items = state.items(&env);
            let label = items
                .iter()
                .find(|(p, _)| *p == cs.start)
                .map(|(_, l)| l.clone())
                .unwrap_or_default();
            let indent = "\u{2003}\u{2003}".repeat(depth - 1);
            let style = Style {
                bold: name == "description",
                ..Style::default()
            };
            b.replace(cs.start..cs.start, &indent, Style::default());
            b.replace(cs.clone(), &label, style);
            while let Some(n) = &tok
                && span(n).start < cs.end
            {
                tok = n.next_token();
            }
            continue;
        }
        if let Some(skip) = hidden.iter().find(|h| h.start <= r.start && r.end <= h.end) {
            let _ = skip;
            continue;
        }
        if let Some((range, with, style)) = replaced
            .iter()
            .find(|(h, _, _)| h.start <= r.start && r.end <= h.end)
            .cloned()
        {
            if r.start == range.start {
                b.replace(range.clone(), with, style);
            }
            continue;
        }
        // A theorem's or a proof's `\begin` and `\end`: its name, number
        // and note; the end of a proof as ∎.
        if t.kind() == K::CONTROL_WORD
            && let Some(edge) = t.parent().filter(|p| {
                matches!(p.kind(), K::BEGIN | K::END) && p.first_token().as_ref() == Some(&t)
            })
            && let Some(env) = edge.parent()
            && let Some(name) = latex_syntax::name(&env)
            && !near(&node_span(&edge))
        {
            let model = state.model();
            let es = node_span(&edge);
            let theorem = model
                .theorems
                .iter()
                .find(|th| th.range.start == node_span(&env).start && th.file == 0);
            let proof = name == "proof";
            if theorem.is_some() || proof {
                let skip_to = if edge.kind() == K::BEGIN {
                    let bold = Style {
                        bold: true,
                        italic: proof,
                        ..Style::default()
                    };
                    let (title, number, note) = match theorem {
                        Some(th) => (th.title.clone(), th.number.clone(), th.note.clone()),
                        None => (
                            "Proof".to_string(),
                            None,
                            edge.children()
                                .find(|c| c.kind() == K::OPT_ARG)
                                .map(|o| group_text_brackets(&o)),
                        ),
                    };
                    // amsthm's proof with a note prints the note instead
                    // of "Proof" (`\begin{proof}[Sketch]` is "Sketch.").
                    let (title, note) = match (proof, note) {
                        (true, Some(n)) => (n, None),
                        (_, note) => (title, note),
                    };
                    let head = match number {
                        Some(n) => format!("{title} {n}"),
                        None => title,
                    };
                    // amsthm (loaded by the AMS classes) ends a head with
                    // a period; LaTeX's own `\newtheorem` does not.
                    let amsthm = proof
                        || model.packages.iter().any(|p| p.name == "amsthm")
                        || model.class.as_ref().is_some_and(|c| {
                            matches!(c.name.as_str(), "amsart" | "amsbook" | "amsproc")
                        });
                    b.replace(es.start..es.start, &head, bold);
                    // The note: an optional argument, or `[…]` right after.
                    let mut end = es.end;
                    if edge.children().all(|c| c.kind() != K::OPT_ARG)
                        && note.is_some()
                        && text[es.end..].trim_start().starts_with('[')
                        && let Some(close) = text[es.end..line.end].find(']')
                    {
                        end = es.end + close + 1;
                    }
                    let stop = if amsthm { "." } else { "" };
                    let tail = match &note {
                        Some(n) => format!(" ({n}){stop} "),
                        None => format!("{stop} "),
                    };
                    let plain = Style {
                        italic: proof,
                        ..Style::default()
                    };
                    b.replace(es.start..end, &tail, plain);
                    end
                } else {
                    if proof {
                        b.replace(es.clone(), "\u{220e}", Style::default());
                    } else {
                        v.role = crate::view::LineRole::Delimiter;
                    }
                    es.end
                };
                while let Some(n) = &tok
                    && span(n).start < skip_to
                {
                    tok = n.next_token();
                }
                continue;
            }
        }
        // A TikZ picture away from the cursor: drawn by TeX, on its
        // `\begin` line (the rest folded).
        if t.kind() == K::CONTROL_WORD
            && t.text() == "\\begin"
            && let Some(begin) = t.parent().filter(|p| p.kind() == K::BEGIN)
            && let Some(env) = begin.parent()
            && latex_syntax::name(&env).is_some_and(|n| tex_picture(&n))
            && !near(&node_span(&env))
            && let Some(path) = picture_by_tex(doc, state, &text[node_span(&env)])
        {
            let end = line.end;
            b.runs.push(Run {
                src: r.start..end,
                text: crate::view::PLACEHOLDER.to_string(),
                verbatim: false,
                style: Style::default(),
                widget: Some(crate::view::Widget::Image { path, width: None }),
            });
            while let Some(n) = &tok
                && span(n).start < end
            {
                tok = n.next_token();
            }
            continue;
        }
        if t.kind() == K::CONTROL_WORD
            && let Some(cmd) = t
                .parent()
                .filter(|p| p.kind() == K::COMMAND && p.first_token().as_ref() == Some(&t))
            && !near(&node_span(&cmd))
        {
            let cs = node_span(&cmd);
            match &text[r.clone()] {
                // A picture: drawn, at the width its options ask for.
                "\\includegraphics" => {
                    if let Some(path) = picture_path(doc, &state.model(), &cmd) {
                        b.runs.push(Run {
                            src: cs.clone(),
                            text: crate::view::PLACEHOLDER.to_string(),
                            verbatim: false,
                            style: Style::default(),
                            widget: Some(crate::view::Widget::Image {
                                path,
                                width: picture_width(&cmd),
                            }),
                        });
                        while let Some(n) = &tok
                            && span(n).start < cs.end
                        {
                            tok = n.next_token();
                        }
                        continue;
                    }
                }
                name if chip_command(&name[1..]) => {
                    let model = state.model();
                    let (shown, resolved) = chip(doc, &model, &name[1..], &cmd);
                    let style = Style {
                        link: resolved,
                        todo: (!resolved).then_some(true),
                        ..Style::default()
                    };
                    b.replace(cs.clone(), &shown, style);
                    while let Some(n) = &tok
                        && span(n).start < cs.end
                    {
                        tok = n.next_token();
                    }
                    continue;
                }
                // A footnote: its mark raised, its text dimmed.
                "\\footnote" => {
                    let model = state.model();
                    if let (Some(f), Some(g)) = (
                        model
                            .footnotes
                            .iter()
                            .find(|f| f.range.start == cs.start && f.file == 0),
                        cmd.children().find(|c| c.kind() == K::GROUP),
                    ) {
                        let gs = node_span(&g);
                        let mark = Style {
                            superscript: true,
                            link: true,
                            ..Style::default()
                        };
                        b.replace(cs.start..gs.start + 1, &f.number, mark);
                        b.replace(gs.start + 1..gs.start + 1, " ", Style::default());
                        if text[..gs.end].ends_with('}') {
                            hidden.push(gs.end - 1..gs.end);
                        }
                        while let Some(n) = &tok
                            && span(n).start < gs.start + 1
                        {
                            tok = n.next_token();
                        }
                        continue;
                    }
                }
                // Index entries typeset nothing where they are: dimmed
                // with their arguments.
                "\\index" | "\\indexsee" | "\\glossary" | "\\nomenclature" => {
                    b.verbatim(cs.clone(), dim);
                    while let Some(n) = &tok
                        && span(n).start < cs.end
                    {
                        tok = n.next_token();
                    }
                    continue;
                }
                // `\\ensuremath{…}`: the formula.
                "\\ensuremath" if !c_math(&t) => {
                    if let Some(g) = cmd.children().find(|c| c.kind() == K::GROUP) {
                        b.runs.push(Run {
                            src: cs.clone(),
                            text: crate::view::PLACEHOLDER.to_string(),
                            verbatim: false,
                            style: Style::default(),
                            widget: Some(crate::view::Widget::Math {
                                source: group_text(&g),
                                display: false,
                            }),
                        });
                        while let Some(n) = &tok
                            && span(n).start < cs.end
                        {
                            tok = n.next_token();
                        }
                        continue;
                    }
                }
                // `\\texorpdfstring{TeX}{PDF}`: the TeX text.
                "\\texorpdfstring" => {
                    let groups: Vec<SyntaxNode> =
                        cmd.children().filter(|c| c.kind() == K::GROUP).collect();
                    if let [first, second] = groups.as_slice() {
                        let (f, sc) = (node_span(first), node_span(second));
                        hidden.push(r.clone());
                        hidden.push(f.start..f.start + 1);
                        hidden.push(f.end - 1..f.end);
                        hidden.push(sc);
                        continue;
                    }
                }
                "\\centering" => {
                    while let Some(n) = &tok
                        && span(n).start < cs.end
                    {
                        tok = n.next_token();
                    }
                    continue;
                }
                // A statement of an algorithm (algorithmicx's algpseudocode,
                // algorithmic): its keywords, bold, as the package prints
                // them; `\\State` nothing.
                name if in_algorithm(&cmd)
                    && let Some((before, between, after, bold_words)) =
                        algorithm_words(&name[1..]) =>
                {
                    let bold = Style {
                        bold: bold_words,
                        ..Style::default()
                    };
                    let groups: Vec<SyntaxNode> =
                        cmd.children().filter(|c| c.kind() == K::GROUP).collect();
                    match groups.as_slice() {
                        // `\\State x`: the blanks after it eaten, as TeX
                        // eats them after a command's name.
                        [] if before.is_empty() => {
                            let rest = &text[cs.end..line.end];
                            let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                            b.replace(cs.start..cs.end + blanks, "", bold);
                        }
                        [] => b.replace(cs.clone(), before, bold),
                        [g] => {
                            let gs = node_span(g);
                            b.replace(cs.start..gs.start + 1, before, bold);
                            if text[..gs.end].ends_with('}') {
                                replaced.push((gs.end - 1..gs.end, after, bold));
                            }
                        }
                        [g, h, ..] => {
                            let (gs, hs) = (node_span(g), node_span(h));
                            b.replace(cs.start..gs.start + 1, before, bold);
                            replaced.push((gs.end - 1..hs.start + 1, between, Style::default()));
                            if text[..hs.end].ends_with('}') {
                                replaced.push((hs.end - 1..hs.end, after, Style::default()));
                            }
                        }
                    }
                    let skip = match groups.first() {
                        Some(g) => node_span(g).start + 1,
                        None if before.is_empty() => {
                            let rest = &text[cs.end..line.end];
                            cs.end + rest.len() - rest.trim_start_matches([' ', '\t']).len()
                        }
                        None => cs.end,
                    };
                    while let Some(n) = &tok
                        && span(n).start < skip
                    {
                        tok = n.next_token();
                    }
                    continue;
                }
                // csquotes' `\\enquote{…}`: “…”, ‘…’ inside another and for
                // `\\enquote*`.
                "\\enquote" => {
                    if let Some(g) = cmd.children().find(|c| c.kind() == K::GROUP) {
                        let gs = node_span(&g);
                        let starred = text[cs.start..gs.start].contains('*');
                        let depth = cmd
                            .ancestors()
                            .skip(1)
                            .filter(|a| {
                                a.kind() == K::COMMAND
                                    && latex_syntax::name(a).as_deref() == Some("enquote")
                            })
                            .count();
                        let (open, close) = if (depth % 2 == 1) != starred {
                            ("\u{2018}", "\u{2019}")
                        } else {
                            ("\u{201c}", "\u{201d}")
                        };
                        b.replace(cs.start..gs.start + 1, open, Style::default());
                        if text[..gs.end].ends_with('}') {
                            replaced.push((gs.end - 1..gs.end, close, Style::default()));
                        }
                        while let Some(n) = &tok
                            && span(n).start < gs.start + 1
                        {
                            tok = n.next_token();
                        }
                        continue;
                    }
                }
                // subfig's `\\subfloat[list][caption]{body}`: `(a) `, the
                // caption, then the body; no caption without an optional
                // argument, the label alone with an empty one.
                "\\subfloat" | "\\subfigure" | "\\subtable" => {
                    let opts: Vec<SyntaxNode> =
                        cmd.children().filter(|c| c.kind() == K::OPT_ARG).collect();
                    if let Some(g) = cmd.children().find(|c| c.kind() == K::GROUP) {
                        let gs = node_span(&g);
                        let number = state.model().floats.iter().find_map(|f| {
                            f.captions
                                .iter()
                                .find(|c| c.range.start == cs.start && c.file == 0)
                                .and_then(|c| c.number.clone())
                        });
                        let bold = Style {
                            bold: true,
                            ..Style::default()
                        };
                        match (opts.last(), number) {
                            (Some(o), Some(n)) => {
                                let os = node_span(o);
                                b.replace(cs.start..os.start + 1, &format!("({n}) "), bold);
                                let empty = text[os.start + 1..os.end - 1].trim().is_empty();
                                replaced.push((
                                    os.end - 1..gs.start + 1,
                                    if empty { "" } else { " " },
                                    Style::default(),
                                ));
                                while let Some(n) = &tok
                                    && span(n).start < os.start + 1
                                {
                                    tok = n.next_token();
                                }
                            }
                            _ => {
                                b.replace(cs.start..gs.start + 1, "", bold);
                                while let Some(n) = &tok
                                    && span(n).start < gs.start + 1
                                {
                                    tok = n.next_token();
                                }
                            }
                        }
                        if text[..gs.end].ends_with('}') {
                            hidden.push(gs.end - 1..gs.end);
                        }
                        continue;
                    }
                }
                // A caption: `Figure 1: ` for the command and its brace.
                "\\caption" => {
                    let model = state.model();
                    let found = model.floats.iter().find_map(|f| {
                        let c = f
                            .captions
                            .iter()
                            .find(|c| c.range.start == cs.start && c.file == 0)?;
                        Some((f.kind.clone(), c.number.clone()))
                    });
                    if let (Some((kind, number)), Some(g)) =
                        (found, cmd.children().find(|c| c.kind() == K::GROUP))
                    {
                        let gs = node_span(&g);
                        let turkish =
                            model.packages.iter().any(|p| {
                                p.name == "babel" && p.options.iter().any(|o| o == "turkish")
                            }) || model
                                .class
                                .as_ref()
                                .is_some_and(|c| c.options.iter().any(|o| o == "turkish"));
                        let name = float_name(&kind, turkish).unwrap_or("");
                        let sub = cmd.ancestors().any(|a| {
                            a.kind() == K::ENVIRONMENT
                                && latex_syntax::name(&a).is_some_and(|n| n.starts_with("sub"))
                        });
                        // The algorithm package's ruled style: no colon
                        // (algorithm2e has one).
                        let colon = kind != "algorithm"
                            || model.packages.iter().any(|p| p.name == "algorithm2e");
                        let label = match number {
                            // In `subfigure`: `(a) `.
                            Some(n) if sub => format!("({n}) "),
                            Some(n) if !colon => format!("{name} {n} "),
                            Some(n) => format!("{name} {n}: "),
                            None => format!("{name}: "),
                        };
                        let bold = Style {
                            bold: true,
                            ..Style::default()
                        };
                        b.replace(cs.start..gs.start + 1, &label, bold);
                        if text[..gs.end].ends_with('}') {
                            hidden.push(gs.end - 1..gs.end);
                        }
                        while let Some(n) = &tok
                            && span(n).start < gs.start + 1
                        {
                            tok = n.next_token();
                        }
                        continue;
                    }
                }
                _ => {}
            }
        }
        // A formula on this line, away from the cursor: drawn.
        if let Some(m) = math_node(&t)
            && let ms = node_span(&m)
            && ms.start >= line.start
            && ms.end <= line.end
            && !near(&ms)
            && let Some(source) = math_source(doc, ms.clone())
        {
            let display = m.kind() != K::INLINE_MATH;
            b.runs.push(Run {
                src: ms.clone(),
                text: crate::view::PLACEHOLDER.to_string(),
                verbatim: false,
                style: Style::default(),
                widget: Some(crate::view::Widget::Math { source, display }),
            });
            while let Some(n) = &tok
                && span(n).start < ms.end
            {
                tok = n.next_token();
            }
            continue;
        }
        let c = context(&t);
        if let Some(cmd) = &c.marker_of {
            if latex_syntax::signatures::is_sectioning(&latex_syntax::name(cmd).unwrap_or_default())
            {
                heading_command = Some(cmd.clone());
            }
            if !near(&node_span(cmd)) {
                // A front matter command's words, or the separator between
                // the parts of an address.
                let name = latex_syntax::name(cmd).unwrap_or_default();
                if t.kind() == K::CONTROL_WORD
                    && let Some((st, prefix)) = front_style(&name)
                {
                    let words = if front_part(&name) && cmd.prev_sibling().is_some() {
                        ", "
                    } else {
                        prefix
                    };
                    if !words.is_empty() {
                        b.replace(r, words, Style { bold: true, ..st });
                        continue;
                    }
                }
                // The title's number where its brace was.
                if let Some(sec) = &c.title_open {
                    let model = state.model();
                    let start = node_span(sec).start;
                    if let Some(n) = model
                        .sections
                        .iter()
                        .find(|s| s.range.start == start && s.file == 0)
                        .and_then(|s| s.number.clone())
                    {
                        b.replace(r.end..r.end, &format!("{n}\u{2003}"), c.style);
                    }
                }
                continue;
            }
            b.verbatim(r, dim);
            continue;
        }
        if c.heading && heading_command.is_none() {
            heading_command = t.parent_ancestors().find(|a| {
                a.kind() == K::COMMAND
                    && latex_syntax::signatures::is_sectioning(
                        &latex_syntax::name(a).unwrap_or_default(),
                    )
            });
        }
        let s = &text[r.clone()];
        match t.kind() {
            K::TEXT if c.typography => {
                let mut at = r.start;
                for (rr, rep) in typography(s) {
                    let src = r.start + rr.start..r.start + rr.end;
                    if near(&src) && cursor != Some(src.start) && cursor != Some(src.end) {
                        continue;
                    }
                    b.verbatim(at..src.start, c.style);
                    b.replace(src.clone(), rep, c.style);
                    at = src.end;
                }
                b.verbatim(at..r.end, c.style);
            }
            // An accent and its letter as one character: `\"o` ö, `\c{c}` ç.
            K::CONTROL_SYMBOL | K::CONTROL_WORD
                if !c.math
                    && !near(&r)
                    && let Some((end, ch)) = accented(text, &s[1..], r.end, line.end) =>
            {
                b.replace(r.start..end, &ch, c.style);
                while let Some(n) = tok.clone()
                    && span(&n).start < end
                {
                    let ns = span(&n);
                    if ns.end > end {
                        // The rest of a word the letter began.
                        let rest = end..ns.end.min(line.end);
                        let mut at = rest.start;
                        if n.kind() == K::TEXT && c.typography {
                            for (rr, rep) in typography(&text[rest.clone()]) {
                                let src = rest.start + rr.start..rest.start + rr.end;
                                b.verbatim(at..src.start, c.style);
                                b.replace(src.clone(), rep, c.style);
                                at = src.end;
                            }
                        }
                        b.verbatim(at..rest.end, c.style);
                    }
                    tok = n.next_token();
                }
            }
            K::CONTROL_SYMBOL if !c.math && !near(&r) => {
                let is_break = s == "\\\\";
                match symbol(s) {
                    // (A control space at the line's end: its newline is not
                    // the line's.)
                    Some(rep) => b.replace(r.start..r.end.min(line.end), rep, c.style),
                    None if is_break => {
                        // `\\` and its star and spacing argument.
                        let cmd = t.parent().filter(|p| p.kind() == K::COMMAND);
                        let end = cmd.map_or(r.end, |p| node_span(&p).end);
                        if near(&(r.start..end)) {
                            b.verbatim(r, c.style);
                        } else {
                            b.replace(r.start..end, "\u{21b5}", dim);
                            // Skip what the replacement covers.
                            while let Some(n) = &tok
                                && span(n).start < end
                            {
                                tok = n.next_token();
                            }
                        }
                    }
                    None => b.verbatim(r, c.style),
                }
            }
            K::CONTROL_WORD if !c.math && !near(&r) => {
                let name = &s[1..];
                let untitled = name == "maketitle" && state.titles().iter().all(Option::is_none);
                match (name, word(name)) {
                    // A glossary entry or an acronym: what LaTeX prints, as
                    // text, or as a formula when it is one.
                    (_, None)
                        if let Some(cmd) = t.parent().filter(|p| p.kind() == K::COMMAND)
                            && node_span(&cmd).end <= line.end
                            && !near(&node_span(&cmd))
                            && let Some(shown) = glossary_shown(&state.model(), &cmd)
                            && (entry_math(&shown).is_some()
                                || plain_text(&state.model(), &shown, 0).is_some()) =>
                    {
                        let cs = node_span(&cmd);
                        match plain_text(&state.model(), &shown, 0) {
                            Some((plain, _)) => b.replace(r.start..cs.end, &plain, c.style),
                            None => b.runs.push(Run {
                                src: r.start..cs.end,
                                text: crate::view::PLACEHOLDER.to_string(),
                                verbatim: false,
                                style: Style::default(),
                                widget: Some(crate::view::Widget::Math {
                                    source: format!("${}$", entry_math(&shown).unwrap_or("")),
                                    display: false,
                                }),
                            }),
                        }
                        while let Some(n) = &tok
                            && span(n).start < cs.end
                        {
                            tok = n.next_token();
                        }
                    }
                    // A document's own macro whose definition is text: the
                    // text, as LaTeX prints it.
                    (n, None)
                        if !renders_command(n)
                            && let Some(own) = own_macro(&state.model(), n) =>
                    {
                        let spec = if own.default { "o" } else { "m" };
                        let spec: String = std::iter::once(spec)
                            .chain(std::iter::repeat_n("m", own.args.saturating_sub(1)))
                            .take(own.args)
                            .collect();
                        let mut end = args_end(text, r.end, line.end, &spec).unwrap_or(r.end);
                        // `\method{}`: the group that ends the name, nothing.
                        if text[end..line.end].starts_with("{}") {
                            end += 2;
                        } else if own.args == 0 && !own.xspace {
                            // TeX takes the spaces after a control word.
                            while text[end..line.end].starts_with(' ') {
                                end += 1;
                            }
                        }
                        if near(&(r.start..end)) {
                            b.verbatim(r, c.style);
                        } else {
                            let mut st = c.style;
                            st.bold |= own.style.bold;
                            st.italic |= own.style.italic;
                            st.code |= own.style.code;
                            b.replace(r.start..end, &own.text, st);
                            while let Some(n) = &tok
                                && span(n).start < end
                            {
                                tok = n.next_token();
                            }
                        }
                    }
                    // siunitx's numbers and quantities, typeset.
                    (
                        "num" | "si" | "unit" | "SI" | "qty" | "ang" | "numrange" | "SIrange"
                        | "qtyrange" | "numlist",
                        _,
                    ) if let Some(cmd) = t.parent().filter(|p| p.kind() == K::COMMAND)
                        && node_span(&cmd).end <= line.end
                        && !near(&node_span(&cmd))
                        && state.model().packages.iter().any(|p| p.name == "siunitx")
                        && let Some(shown) = crate::siunitx::render(
                            name,
                            &cmd.children()
                                .filter(|c| c.kind() == K::GROUP)
                                .map(|g| {
                                    let t = g.text().to_string();
                                    t[1..t.len() - usize::from(t.ends_with('}'))].to_string()
                                })
                                .collect::<Vec<_>>(),
                        ) =>
                    {
                        let end = node_span(&cmd).end;
                        b.replace(r.start..end, &shown, c.style);
                        while let Some(n) = &tok
                            && span(n).start < end
                        {
                            tok = n.next_token();
                        }
                    }
                    // What prints nothing here (a definition, a setting):
                    // markup, dimmed.
                    (n, _)
                        if let Some(spec) = silent(n)
                            && let Some(end) = args_end(text, r.end, line.end, spec)
                            && !near(&(r.start..end)) =>
                    {
                        b.verbatim(
                            r.start..end,
                            Style {
                                dim: true,
                                ..c.style
                            },
                        );
                        while let Some(n) = &tok
                            && span(n).start < end
                        {
                            tok = n.next_token();
                        }
                    }
                    // A box around text, and a color: the sizes, angles and
                    // color hidden as a format's markup is, the text shown
                    // (`\textcolor`'s in its color, `\color`'s after it).
                    (n, _)
                        if let Some(spec) = box_args(n).or(match n {
                            "textcolor" | "color" => Some("om"),
                            _ => None,
                        }) && let Some(end) = args_end(text, r.end, line.end, spec)
                            && !near(&(r.start..end)) =>
                    {
                        // xcolor's `\color` ignores the blanks after it.
                        let end = if n == "color" {
                            let rest = &text[end..line.end];
                            end + rest.len() - rest.trim_start_matches([' ', '\t']).len()
                        } else {
                            end
                        };
                        b.replace(r.start..end, "", c.style);
                        while let Some(n) = &tok
                            && span(n).start < end
                        {
                            tok = n.next_token();
                        }
                    }
                    // An entry of the document's own bibliography: its label,
                    // as LaTeX prints it ([1], [LL90]).
                    ("bibitem", _)
                        if let Some(cmd) = t.parent().filter(|p| p.kind() == K::COMMAND)
                            && node_span(&cmd).end <= line.end
                            && !near(&node_span(&cmd))
                            && let Some(item) = state.model().bib_items.iter().find(|i| {
                                i.file == 0 && i.range.start == node_span(&cmd).start
                            }) =>
                    {
                        let end = node_span(&cmd).end;
                        // A blank between the label and the entry, as the
                        // label's box keeps them apart.
                        let gap = if text[end..].starts_with(char::is_whitespace) {
                            ""
                        } else {
                            " "
                        };
                        b.replace(r.start..end, &format!("[{}]{gap}", item.label), c.style);
                        while let Some(n) = &tok
                            && span(n).start < end
                        {
                            tok = n.next_token();
                        }
                    }
                    // `\vspace{…}` typesets nothing here, `\hspace{…}` a
                    // space: the command and its argument hidden.
                    ("vspace" | "hspace" | "addvspace" | "vskip" | "hskip", _)
                        if let Some(cmd) = t.parent().filter(|p| p.kind() == K::COMMAND)
                            && node_span(&cmd).end <= line.end
                            && !near(&node_span(&cmd)) =>
                    {
                        let end = node_span(&cmd).end;
                        let rep = if name.starts_with('h') { " " } else { "" };
                        b.replace(r.start..end, rep, c.style);
                        while let Some(n) = &tok
                            && span(n).start < end
                        {
                            tok = n.next_token();
                        }
                    }
                    // `\today`: the date LaTeX prints, in English.
                    ("today", _) => {
                        let today = jiff::Zoned::now().date();
                        let month = [
                            "January",
                            "February",
                            "March",
                            "April",
                            "May",
                            "June",
                            "July",
                            "August",
                            "September",
                            "October",
                            "November",
                            "December",
                        ][usize::from(today.month().unsigned_abs()) - 1];
                        b.replace(
                            r,
                            &format!("{month} {}, {}", today.day(), today.year()),
                            c.style,
                        );
                    }
                    ("maketitle", _) if !untitled => {
                        let [title, author, date] = &*state.titles();
                        let title_style = Style {
                            title: true,
                            ..Style::default()
                        };
                        let by = Style {
                            byline: true,
                            ..Style::default()
                        };
                        b.replace(
                            r.start..r.start,
                            title.as_deref().unwrap_or(""),
                            title_style,
                        );
                        let rest: Vec<&str> =
                            [author, date].iter().filter_map(|x| x.as_deref()).collect();
                        let rest = if rest.is_empty() {
                            String::new()
                        } else {
                            format!("\u{2003}{}", rest.join(" \u{b7} "))
                        };
                        b.replace(r, &rest, by);
                    }
                    (_, Some(rep)) => {
                        // TeX eats the blanks after a control word; an
                        // empty `{}` after it ends it and typesets nothing.
                        let mut end = r.end;
                        if let Some(n) = tok.clone() {
                            let close = n.next_token().filter(|m| m.kind() == K::R_BRACE);
                            match (n.kind(), close) {
                                (K::L_BRACE, Some(m)) if span(&m).end <= line.end => {
                                    end = span(&m).end;
                                    tok = m.next_token();
                                }
                                (K::WHITESPACE, _) if span(&n).end <= line.end => {
                                    end = span(&n).end;
                                    tok = n.next_token();
                                }
                                _ => {}
                            }
                        }
                        b.replace(r.start..end, rep, c.style);
                    }
                    // A case change of plain text: the text changed.
                    (
                        "MakeUppercase" | "uppercase" | "MakeTextUppercase" | "MakeLowercase"
                        | "lowercase" | "MakeTextLowercase",
                        _,
                    ) if let Some(end) = group_end(text, r.end, line.end)
                        && !near(&(r.start..end)) =>
                    {
                        cases.push((r.end..end, name.contains("pper"), !name.starts_with("Make")));
                        b.verbatim(
                            r,
                            Style {
                                dim: true,
                                ..c.style
                            },
                        );
                    }
                    _ => {
                        let known = format_style(name).is_some()
                            || !latex_syntax::signatures::command(name).is_empty();
                        let mut st = c.style;
                        st.dim = !known || transparent(name);
                        // TeX eats the blanks after a control word: markup
                        // too, after a name shown as markup.
                        let mut end = r.end;
                        if st.dim
                            && let Some(n) = tok.clone()
                            && n.kind() == K::WHITESPACE
                            && span(&n).end <= line.end
                        {
                            end = span(&n).end;
                            tok = n.next_token();
                        }
                        b.verbatim(r.start..end, st);
                    }
                }
            }
            K::TILDE if !c.math && !near(&r) => b.replace(r, "\u{a0}", c.style),
            // A table the grid does not draw: its cells apart, a rule
            // between them.
            K::AMPERSAND if !c.math && !near(&r) && in_text_table(text, &t) => {
                b.replace(r, " \u{2502} ", dim);
            }
            // A group's braces typeset nothing: markup, dimmed.
            K::L_BRACE | K::R_BRACE if !c.math && brace_is_markup(&t) => {
                b.verbatim(
                    r,
                    Style {
                        dim: true,
                        ..c.style
                    },
                );
            }
            K::COMMENT => b.verbatim(r, dim),
            _ => b.verbatim(r, c.style),
        }
    }
    v.runs = b.runs;
    for (range, upper, primitive) in &cases {
        for r in &mut v.runs {
            // A letter a command typesets, under the primitive: unchanged.
            let letter = || {
                let src = &text[r.src.clone()];
                let name: String = src
                    .trim_start_matches('\\')
                    .chars()
                    .take_while(char::is_ascii_alphabetic)
                    .collect();
                src.contains("\\i") || src.contains("\\j") || word(&name).is_some()
            };
            if range.start <= r.src.start
                && r.src.end <= range.end
                && !r.style.dim
                && !(*primitive && !r.verbatim && letter())
            {
                r.text = change_case(&r.text, *upper);
                r.verbatim &= r.text.len() == r.src.len();
            }
        }
    }
    for r in &mut v.runs {
        if dimmed
            || skipped
                .iter()
                .any(|k| k.start <= r.src.start && r.src.end <= k.end && !r.src.is_empty())
        {
            r.style.dim = true;
        }
    }
    // A heading: its level among the document's sectioning levels.
    if let Some(cmd) = heading_command
        && node_span(&cmd).start >= line.start
    {
        let name = latex_syntax::name(&cmd).unwrap_or_default();
        let level = latex_model_level(&name);
        if level >= 4 {
            // `\paragraph` and `\subparagraph` are run-in: bold, in the text.
            for r in &mut v.runs {
                if r.src.start < node_span(&cmd).end {
                    r.style.bold = true;
                }
            }
            v.align = alignment(&root, line.start);
            return v;
        }
        let top = state
            .model()
            .sections
            .iter()
            .map(|s| s.level)
            .min()
            .unwrap_or(level);
        v.heading = (level - top + 1).clamp(1, 6) as u8;
    }
    // A displayed formula alone on its line is centered, as LaTeX
    // centers it (`fleqn` sets it flush left).
    let displayed = v.runs.iter().any(|r| {
        matches!(
            r.widget,
            Some(crate::view::Widget::Math { display: true, .. })
        )
    }) && v
        .runs
        .iter()
        .all(|r| r.widget.is_some() || r.style.dim || r.text.trim().is_empty());
    v.align = if title_line {
        crate::rich::Align::Center
    } else if displayed {
        display_align(doc)
    } else {
        alignment(&root, line.start)
    };
    v
}

fn latex_model_level(name: &str) -> i8 {
    match name {
        "part" => -1,
        "chapter" => 0,
        "section" => 1,
        "subsection" => 2,
        "subsubsection" => 3,
        "paragraph" => 4,
        _ => 5,
    }
}

/// Where a LaTeX document's displayed formulas stand: centered, unless
/// the class or a package takes `fleqn` (flush left).
pub fn display_align(doc: &crate::DocumentState) -> crate::rich::Align {
    let Some(state) = doc.latex() else {
        return crate::rich::Align::default();
    };
    let model = state.model();
    let fleqn = model
        .class
        .as_ref()
        .is_some_and(|c| c.options.iter().any(|o| o == "fleqn"))
        || model
            .packages
            .iter()
            .any(|p| p.options.iter().any(|o| o == "fleqn"));
    if fleqn {
        crate::rich::Align::Left
    } else {
        crate::rich::Align::Center
    }
}

/// The alignment of the environment around `pos`: `center`, `flushleft`,
/// `flushright`.
fn alignment(root: &SyntaxNode, pos: usize) -> crate::rich::Align {
    let Some(t) = latex_syntax::token_at(root, pos) else {
        return crate::rich::Align::default();
    };
    for a in t.parent_ancestors() {
        if a.kind() == K::BODY
            && let Some(env) = a.parent()
        {
            let name = latex_syntax::name(&env).unwrap_or_default();
            if float_name(&name, false).is_some()
                && a.descendants().any(|c| {
                    c.kind() == K::COMMAND
                        && latex_syntax::name(&c).as_deref() == Some("centering")
                        && c.ancestors().find(|x| x.kind() == K::BODY).as_ref() == Some(&a)
                })
            {
                return crate::rich::Align::Center;
            }
            match Some(name.as_str()) {
                Some("center") => return crate::rich::Align::Center,
                Some("flushright") => return crate::rich::Align::Right,
                Some("flushleft") => return crate::rich::Align::Left,
                _ => {}
            }
        }
    }
    crate::rich::Align::default()
}

/// Commands shown as what they resolve to.
fn chip_command(name: &str) -> bool {
    matches!(
        name,
        "ref"
            | "eqref"
            | "pageref"
            | "autoref"
            | "cref"
            | "Cref"
            | "nameref"
            | "vref"
            | "Vref"
            | "url"
            | "href"
    ) || latex_syntax::signatures::command(name) == "*oom"
}

/// The optional and mandatory arguments of a command, as written.
fn arguments(cmd: &SyntaxNode) -> (Vec<String>, Vec<String>) {
    let mut opts = Vec::new();
    let mut mands = Vec::new();
    for c in cmd.children() {
        match c.kind() {
            K::OPT_ARG => {
                let t = c.text().to_string();
                opts.push(t[1..t.len() - usize::from(t.ends_with(']'))].to_string());
            }
            K::GROUP => mands.push(group_text(&c)),
            _ => {}
        }
    }
    (opts, mands)
}

/// What a reference names its target by: hyperref's `\autoref` names
/// ("section 1", "Equation 3", "Appendix A"; a theorem by its counter, a
/// counter without a name by its number alone), cleveref's `\cref` and
/// `\Cref` names ("eq. (3)", "appendix A.1", a theorem by its title).
/// The document's own `\<counter>autorefname` wins for `\autoref`.
fn target_name(
    model: &latex_model::Model,
    target: &latex_model::Target,
    number: &str,
    command: &str,
) -> String {
    use latex_model::Target;
    let appendix = number
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase());
    if command == "autoref" {
        let counter: Option<String> = match target {
            Target::Section(-1) => Some("part".into()),
            Target::Section(0) if appendix => Some("appendix".into()),
            Target::Section(l) => {
                let names = [
                    "chapter",
                    "section",
                    "subsection",
                    "subsubsection",
                    "paragraph",
                    "subparagraph",
                ];
                names.get((*l).max(0) as usize).map(|s| s.to_string())
            }
            Target::Equation => Some("equation".into()),
            Target::Float(k) => Some(k.clone()),
            Target::Footnote => Some("footnote".into()),
            Target::Item => Some("item".into()),
            Target::Theorem(env) => model
                .theorem_kinds
                .iter()
                .find(|k| k.env == *env)
                .map(|k| k.counter.clone()),
            Target::Counter(c) => Some(c.clone()),
            Target::None => None,
        };
        let Some(counter) = counter else {
            return String::new();
        };
        if let Some(m) = model
            .macros
            .iter()
            .rev()
            .find(|m| m.name == format!("\\{counter}autorefname"))
        {
            return m.body.trim().to_string();
        }
        return match counter.as_str() {
            "part" => "Part",
            "appendix" => "Appendix",
            "chapter" => "chapter",
            "section" => "section",
            "subsection" => "subsection",
            "subsubsection" => "subsubsection",
            "paragraph" => "paragraph",
            "subparagraph" => "subparagraph",
            "equation" => "Equation",
            "figure" => "Figure",
            "table" => "Table",
            "footnote" => "footnote",
            "item" => "item",
            "theorem" => "Theorem",
            _ => "",
        }
        .to_string();
    }
    let short = match target {
        Target::Section(-1) => "part".to_string(),
        Target::Section(_) if appendix => "appendix".to_string(),
        Target::Section(0) => "chapter".to_string(),
        Target::Section(_) => "section".to_string(),
        Target::Equation => "eq.".to_string(),
        Target::Float(k) if k == "table" => "table".to_string(),
        Target::Float(_) => "fig.".to_string(),
        Target::Footnote => "footnote".to_string(),
        Target::Item => "item".to_string(),
        Target::Theorem(env) => model
            .theorem_kinds
            .iter()
            .find(|k| k.env == *env)
            .map_or_else(|| env.clone(), |k| k.title.to_lowercase()),
        Target::Counter(c) => c.clone(),
        Target::None => String::new(),
    };
    if command == "Cref" {
        capital(&match short.as_str() {
            "eq." => "equation".to_string(),
            "fig." => "figure".to_string(),
            _ => short,
        })
    } else {
        short
    }
}

/// `s` with a capital first letter.
fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// cleveref's plural of a name: "eqs.", "figs.", "appendices".
fn plural(name: &str) -> String {
    match name {
        "eq." => "eqs.".into(),
        "fig." => "figs.".into(),
        "appendix" => "appendices".into(),
        "Appendix" => "Appendices".into(),
        n => format!("{n}s"),
    }
}

/// `\cref{a,b,c}` as cleveref prints it: the keys grouped by kind in the
/// order they come, each group sorted, runs of three or more consecutive
/// numbers compressed to "1 to 3", "and" between two, commas and a final
/// "and" in a group, ", and" before the last of three or more groups.
fn cref_list(model: &latex_model::Model, keys: &[&str], command: &str) -> (String, bool) {
    let mut all = true;
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for k in keys {
        let Some(l) = model.label(k) else {
            all = false;
            groups.push((String::new(), vec!["??".into()]));
            continue;
        };
        let n = l.number.clone().unwrap_or_default();
        let name = target_name(model, &l.target, &n, command);
        let n = if l.target == latex_model::Target::Equation {
            format!("({n})")
        } else {
            n
        };
        match groups.iter_mut().find(|g| g.0 == name && !name.is_empty()) {
            Some(g) => g.1.push(n),
            None => groups.push((name, vec![n])),
        }
    }
    // The number's parts: a prefix and a last integer, for sorting and
    // runs.
    let parts = |n: &str| -> Option<(String, i64)> {
        let t = n.trim_start_matches('(').trim_end_matches(')');
        let cut = t.rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
        t[cut..].parse().ok().map(|v| (t[..cut].to_string(), v))
    };
    let texts: Vec<String> = groups
        .into_iter()
        .map(|(name, mut ns)| {
            if ns.iter().all(|n| parts(n).is_some()) {
                ns.sort_by_key(|n| parts(n));
                ns.dedup();
            }
            // Runs of consecutive numbers.
            let mut items: Vec<String> = Vec::new();
            let mut i = 0;
            while i < ns.len() {
                let mut j = i;
                while j + 1 < ns.len()
                    && matches!((parts(&ns[j]), parts(&ns[j + 1])),
                        (Some((p, a)), Some((q, b))) if p == q && b == a + 1)
                {
                    j += 1;
                }
                if j >= i + 2 {
                    items.push(format!("{} to {}", ns[i], ns[j]));
                    i = j + 1;
                } else {
                    items.push(ns[i].clone());
                    i += 1;
                }
            }
            let many = ns.len() > 1;
            let list = match items.len() {
                1 => items[0].clone(),
                n => format!("{} and {}", items[..n - 1].join(", "), items[n - 1]),
            };
            match (name.is_empty(), many) {
                (true, _) => list,
                (false, true) => format!("{}\u{a0}{list}", plural(&name)),
                (false, false) => format!("{name}\u{a0}{list}"),
            }
        })
        .collect();
    let text = match texts.len() {
        0 => String::new(),
        1 => texts[0].clone(),
        2 => format!("{} and {}", texts[0], texts[1]),
        n => format!("{}, and {}", texts[..n - 1].join(", "), texts[n - 1]),
    };
    (text, all)
}

/// What `\nameref` prints: a float's caption, a theorem's note, else the
/// title of the section the label is in.
fn nameref(model: &latex_model::Model, l: &latex_model::Label) -> String {
    use latex_model::Target;
    let around = |r: &std::ops::Range<usize>, file: usize| {
        file == l.file && r.start <= l.range.start && l.range.end <= r.end
    };
    match &l.target {
        Target::Float(_) => model
            .floats
            .iter()
            .rev()
            .find(|f| around(&f.range, f.file))
            .and_then(|f| {
                f.captions
                    .iter()
                    .rev()
                    .find(|c| c.range.start <= l.range.start)
                    .or(f.captions.first())
            })
            .map(|c| c.text.clone())
            .unwrap_or_default(),
        Target::Theorem(_) => model
            .theorems
            .iter()
            .rev()
            .find(|t| around(&t.range, t.file))
            .and_then(|t| t.note.clone())
            .unwrap_or_default(),
        _ => model
            .sections
            .iter()
            .rev()
            .find(|s| s.range.start <= l.range.start && s.file == l.file)
            .map_or_else(|| l.number.clone().unwrap_or_default(), |s| s.title.clone()),
    }
}

/// A citation of the document's own bibliography (`thebibliography`): the
/// entries' labels as LaTeX prints them, `[1, LL90]`, with natbib's
/// author-year labels (`\bibitem[Knuth(1984)]{knuth}`) as natbib prints
/// them, a key it does not have as `?`; `None` without one.
fn manual_citation(
    model: &latex_model::Model,
    command: &str,
    opts: &[String],
    keys: &str,
) -> Option<(String, bool)> {
    if model.bib_items.is_empty() {
        return None;
    }
    if command.starts_with("nocite") {
        return Some((String::new(), true));
    }
    // A key the bibliography does not have prints `?`, as LaTeX does.
    let mut found = true;
    let labels: Vec<&str> = keys
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| {
            model.bib_items.iter().find(|i| i.key == k).map_or_else(
                || {
                    found = false;
                    "?"
                },
                |i| i.label.as_str(),
            )
        })
        .collect();
    let tilde = |s: &str| s.replace('~', "\u{a0}");
    let (pre, post) = match opts {
        [post] => (None, Some(tilde(post))),
        [pre, post, ..] => (Some(tilde(pre)), Some(tilde(post))),
        [] => (None, None),
    };
    let pre = pre.filter(|p| !p.trim().is_empty());
    let post = post.filter(|p| !p.trim().is_empty());
    let natbib = model.packages.iter().any(|p| p.name == "natbib");
    // natbib's `Author(Year)` labels.
    let author_year: Option<Vec<(&str, &str)>> = natbib
        .then(|| {
            labels
                .iter()
                .map(|l| {
                    let (who, rest) = l.split_once('(')?;
                    let (year, _) = rest.split_once(')')?;
                    Some((who.trim(), year.trim()))
                })
                .collect()
        })
        .flatten();
    let wrap = |body: String| {
        let body = match &pre {
            Some(p) => format!("{p} {body}"),
            None => body,
        };
        match &post {
            Some(p) => format!("{body}, {p}"),
            None => body,
        }
    };
    let shown = match author_year {
        Some(ay) => match command {
            "citep" | "Citep" => format!(
                "({})",
                wrap(
                    ay.iter()
                        .map(|(w, y)| format!("{w}, {y}"))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            ),
            "citeauthor" | "Citeauthor" => {
                ay.iter().map(|(w, _)| *w).collect::<Vec<_>>().join(", ")
            }
            "citeyear" => ay.iter().map(|(_, y)| *y).collect::<Vec<_>>().join(", "),
            _ => ay
                .iter()
                .map(|(w, y)| format!("{w} ({y})"))
                .collect::<Vec<_>>()
                .join("; "),
        },
        None => match command {
            "citenum" => labels.join(", "),
            _ => format!("[{}]", wrap(labels.join(", "))),
        },
    };
    Some((shown, found))
}

/// The keys a document cites, in the order of their first citation
/// (`\nocite` too; `\nocite{*}` adds the rest of the bibliography there).
fn cited_keys(model: &latex_model::Model, bib: &org_cite::Bibliography) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for c in &model.citations {
        for k in &c.keys {
            if k == "*" {
                for e in bib.entries() {
                    if seen.insert(e.key.clone()) {
                        out.push(e.key.clone());
                    }
                }
            } else if seen.insert(k.clone()) {
                out.push(k.clone());
            }
        }
    }
    out
}

/// A citation as the document's `\bibliographystyle` prints it (see
/// [`crate::bibstyle`]), with natbib's commands when natbib is loaded; `None`
/// for a style Kalem does not label.
fn styled_citation(
    model: &latex_model::Model,
    bib: &org_cite::Bibliography,
    command: &str,
    opts: &[String],
    keys: &str,
) -> Option<(String, bool)> {
    use crate::bibstyle::Kind;
    let kind = crate::bibstyle::kind(model.bibliography_style.as_deref()?)?;
    let natbib = model.packages.iter().any(|p| p.name == "natbib");
    if command == "nocite" {
        return Some((String::new(), true));
    }
    let labels = crate::bibstyle::labels(kind, &cited_keys(model, bib), bib);
    let keys: Vec<&str> = keys
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect();
    let all = keys.iter().all(|k| labels.contains_key(*k));
    let tilde = |s: &str| s.replace('~', "\u{a0}");
    // natbib: `[pre][post]`; one optional argument is the post note.
    let (pre, post) = match opts {
        [post] => (None, Some(tilde(post))),
        [pre, post, ..] => (Some(tilde(pre)), Some(tilde(post))),
        [] => (None, None),
    };
    let pre = pre.filter(|p| !p.trim().is_empty());
    let post = post.filter(|p| !p.trim().is_empty());
    let wrap = |body: String| {
        let body = match &pre {
            Some(p) => format!("{p} {body}"),
            None => body,
        };
        match &post {
            Some(p) => format!("{body}, {p}"),
            None => body,
        }
    };
    let shown = match kind {
        Kind::Numeric { .. } | Kind::Alpha => {
            let marks: Vec<String> = keys
                .iter()
                .map(|k| {
                    labels
                        .get(*k)
                        .map_or_else(|| "?".to_string(), |l| l.mark.clone())
                })
                .collect();
            match command {
                "citet" | "Citet" if natbib => keys
                    .iter()
                    .zip(&marks)
                    .map(|(k, m)| {
                        let who = labels.get(*k).map_or("?", |l| l.names.as_str());
                        format!("{who} [{m}]")
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                "citeauthor" | "Citeauthor" if natbib => keys
                    .iter()
                    .map(|k| labels.get(*k).map_or("?".into(), |l| l.names.clone()))
                    .collect::<Vec<_>>()
                    .join(", "),
                "citealt" | "citealp" if natbib => wrap(marks.join(", ")),
                "citenum" => marks.join(", "),
                _ => format!("[{}]", wrap(marks.join(", "))),
            }
        }
        Kind::AuthorYear => {
            let entry = |k: &str| {
                labels
                    .get(k)
                    .map_or(("?".to_string(), "?".to_string()), |l| {
                        (l.names.clone(), l.year.clone())
                    })
            };
            // Consecutive keys of one author name them once: "Knuth,
            // 1984a,b", "Knuth, 1984, 1990".
            let grouped = |sep: &str| -> String {
                let mut out: Vec<(String, Vec<String>)> = Vec::new();
                for k in &keys {
                    let (who, year) = entry(k);
                    match out.last_mut() {
                        Some((w, years)) if *w == who && who != "?" => {
                            let prev = years.last().cloned().unwrap_or_default();
                            let base = |y: &str| {
                                y.trim_end_matches(|c: char| c.is_ascii_lowercase())
                                    .to_string()
                            };
                            if base(&prev) == base(&year) && year.len() > base(&year).len() {
                                years.push(year[base(&year).len()..].to_string());
                            } else {
                                years.push(year);
                            }
                        }
                        _ => out.push((who, vec![year])),
                    }
                }
                out.into_iter()
                    .map(|(w, ys)| format!("{w}{sep}{}", ys.join(",")))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            match command {
                "citep" | "Citep" => format!("[{}]", wrap(grouped(", "))),
                "citealp" => wrap(grouped(", ")),
                "citealt" => wrap(grouped(" ")),
                "citeauthor" | "Citeauthor" => keys
                    .iter()
                    .map(|k| entry(k).0)
                    .collect::<Vec<_>>()
                    .join(", "),
                "citeyear" => keys
                    .iter()
                    .map(|k| entry(k).1)
                    .collect::<Vec<_>>()
                    .join(", "),
                "citeyearpar" => format!(
                    "[{}]",
                    wrap(
                        keys.iter()
                            .map(|k| entry(k).1)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                ),
                // `\cite` is `\citet` in author-year mode.
                _ => {
                    let items: Vec<String> = keys
                        .iter()
                        .map(|k| entry(k))
                        .map(|(who, year)| {
                            let year = match (&pre, &post, keys.len()) {
                                (_, _, 1) => wrap(year),
                                _ => year,
                            };
                            format!("{who} [{year}]")
                        })
                        .collect();
                    items.join(", ")
                }
            }
        }
    };
    Some((shown, all))
}

/// What a reference, a citation or a link shows, and whether it resolved.
fn chip(
    doc: &crate::DocumentState,
    model: &latex_model::Model,
    name: &str,
    cmd: &SyntaxNode,
) -> (String, bool) {
    let (opts, mands) = arguments(cmd);
    let first = mands.first().cloned().unwrap_or_default();
    match name {
        "url" => (first, true),
        "href" => (mands.get(1).cloned().unwrap_or(first), true),
        // cleveref takes a list of keys.
        "cref" | "Cref" => {
            let keys: Vec<&str> = first
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .collect();
            cref_list(model, &keys, name)
        }
        // The others take one key: `\ref{a,b}` is the label `a,b`.
        "ref" | "eqref" | "pageref" | "autoref" | "nameref" | "vref" | "Vref" => {
            let k = first.trim();
            let Some(l) = model.label(k) else {
                return ("??".to_string(), false);
            };
            let n = l.number.clone().unwrap_or_default();
            let shown = match name {
                "eqref" => format!("({n})"),
                "pageref" => k.to_string(),
                "nameref" => nameref(model, l),
                "autoref" => {
                    let what = target_name(model, &l.target, &n, name);
                    if what.is_empty() {
                        n
                    } else {
                        format!("{what}\u{a0}{n}")
                    }
                }
                _ => n,
            };
            (shown, true)
        }
        _ => {
            // A citation: author and year from the bibliography, whose
            // files are found from the root's folder, as LaTeX finds them.
            let files = doc
                .latex()
                .map(|l| l.bibliography_files(doc.meta.path.as_deref()))
                .unwrap_or_default();
            let bib = crate::cite::load(&files);
            if let Some(shown) = styled_citation(model, &bib, name, &opts, &first) {
                return shown;
            }
            if let Some(shown) = manual_citation(model, name, &opts, &first) {
                return shown;
            }
            let mut all = true;
            let (pre, post) = match opts.as_slice() {
                [post] => (None, Some(post.clone())),
                [pre, post, ..] => (Some(pre.clone()), Some(post.clone())),
                [] => (None, None),
            };
            let entries: Vec<(String, Option<String>, Option<String>)> = first
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(|k| match bib.get(k) {
                    Some(e) => (
                        k.to_string(),
                        crate::cite::short_authors(e),
                        crate::cite::year(e),
                    ),
                    None => {
                        all = false;
                        (k.to_string(), None, None)
                    }
                })
                .collect();
            let one = |(key, who, year): &(String, Option<String>, Option<String>)| -> String {
                match (name, who, year) {
                    ("citeauthor" | "Citeauthor", Some(w), _) => w.clone(),
                    ("citeyear", _, Some(y)) => y.clone(),
                    ("citet" | "Citet" | "textcite" | "Textcite", Some(w), Some(y)) => {
                        format!("{w} ({y})")
                    }
                    (_, Some(w), Some(y)) => format!("{w} {y}"),
                    (_, Some(w), None) => w.clone(),
                    _ => key.clone(),
                }
            };
            let mut body = entries.iter().map(one).collect::<Vec<_>>().join("; ");
            if let Some(p) = pre.filter(|p| !p.trim().is_empty()) {
                body = format!("{} {body}", p.replace('~', "\u{a0}"));
            }
            if let Some(p) = post.filter(|p| !p.trim().is_empty()) {
                body = format!("{body}, {}", p.replace('~', "\u{a0}"));
            }
            let shown = match name {
                "citet" | "Citet" | "textcite" | "Textcite" | "citeauthor" | "Citeauthor"
                | "citeyear" => body,
                "citep" | "Citep" | "parencite" | "Parencite" | "autocite" | "Autocite"
                | "footcite" | "citealp" => {
                    format!("({body})")
                }
                _ => format!("[{body}]"),
            };
            (shown, all)
        }
    }
}

/// The message of the diagnostic at `pos` of a LaTeX document, with `⚠`
/// for a warning and `ⓘ` for style, and a word on Quick Fix when it has
/// a fix.
pub fn diagnostic_at(doc: &crate::DocumentState, pos: usize) -> Option<String> {
    let d = crate::latex_check::at(doc.latex_diagnostics()?, pos)?;
    let mark = if d.severity == crate::latex_check::Severity::Warning {
        "⚠"
    } else {
        "ⓘ"
    };
    Some(if d.fix.is_some() {
        crate::tr!(
            "latex-diagnostic-fixable",
            mark = mark,
            message = d.message.as_str()
        )
    } else {
        format!("{mark} {}", d.message)
    })
}

/// What the status bar and a tooltip say about the citation, reference
/// or footnote at `pos` of a LaTeX document: the entries cited, what a
/// label numbers, the footnote's text.
pub fn note_at(doc: &crate::DocumentState, pos: usize) -> Option<String> {
    let state = doc.latex()?;
    let root = state.parse().syntax();
    let t = latex_syntax::token_at(&root, pos)?;
    let cmd = t.parent_ancestors().find(|a| {
        a.kind() == K::COMMAND
            && latex_syntax::name(a)
                .is_some_and(|n| n == "footnote" || (chip_command(&n) && n != "url" && n != "href"))
    })?;
    let name = latex_syntax::name(&cmd)?;
    let model = state.model();
    let (_, mands) = arguments(&cmd);
    let keys: Vec<&str> = mands.first().map_or(Vec::new(), |k| {
        k.split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .collect()
    });
    if name == "footnote" {
        let number = model
            .footnotes
            .iter()
            .find(|f| f.range.start == node_span(&cmd).start)
            .map(|f| f.number.clone());
        return Some(crate::tr!(
            "footnote-preview",
            label = number.unwrap_or_default().as_str(),
            text = mands.first().map_or("", |s| s.trim())
        ));
    }
    if latex_syntax::signatures::command(&name) == "*oom" {
        let files = doc
            .latex()
            .map(|l| l.bibliography_files(doc.meta.path.as_deref()))
            .unwrap_or_default();
        let bib = crate::cite::load(&files);
        // The card in the style the document asks for: biblatex's
        // `style=`, else `\bibliographystyle`.
        let style = model
            .packages
            .iter()
            .find(|p| p.name == "biblatex")
            .and_then(|p| {
                p.options
                    .iter()
                    .find_map(|o| o.trim().strip_prefix("style="))
            })
            .or(model.bibliography_style.as_deref());
        let style = crate::cite::csl_style(style);
        let notes: Vec<String> = keys
            .iter()
            .map(|k| match bib.get(k) {
                Some(e) => format!(
                    "@{k}: {}",
                    crate::cite::card(&files, k, style).unwrap_or_else(|| crate::cite::describe(e))
                ),
                None => crate::tr!("cite-unknown-key", key = *k),
            })
            .collect();
        return Some(notes.join("  "));
    }
    let notes: Vec<String> = keys
        .iter()
        .map(|k| match model.label(k) {
            Some(l) => {
                let n = l.number.clone().unwrap_or_default();
                let what = target_name(&model, &l.target, &n, "Cref");
                let title = model
                    .sections
                    .iter()
                    .find(|s| {
                        l.target != latex_model::Target::Equation
                            && s.number.as_deref() == Some(n.as_str())
                            && s.range.start <= l.range.start
                    })
                    .map(|s| s.title.clone())
                    .or_else(|| {
                        model
                            .floats
                            .iter()
                            .flat_map(|f| f.captions.iter())
                            .find(|c| {
                                c.number.as_deref() == Some(n.as_str())
                                    && c.range.start <= l.range.start
                                    && matches!(l.target, latex_model::Target::Float(_))
                            })
                            .map(|c| c.text.clone())
                    });
                let head = if what.is_empty() {
                    n
                } else {
                    format!("{what} {n}")
                };
                match title {
                    Some(t) => format!("{head}: {t}"),
                    None => head,
                }
            }
            None => crate::tr!("latex-unknown-label", key = *k),
        })
        .collect();
    Some(notes.join("  "))
}

/// Where the command at `pos` leads (`org-open-at-point` for LaTeX): a
/// reference to its `\label`, `\url` and `\href` to their address,
/// `\input`, `\include` and `\subfile` to their file, `\includegraphics`
/// to its picture, a citation to the bibliography file.
pub fn link_at(doc: &crate::DocumentState, pos: usize) -> Option<crate::input::LinkAction> {
    use crate::input::LinkAction;
    let state = doc.latex()?;
    let root = state.parse().syntax();
    let t =
        latex_syntax::token_at(&root, pos).or_else(|| latex_syntax::token_before(&root, pos))?;
    let cmd = t.parent_ancestors().find(|a| {
        a.kind() == K::COMMAND
            && latex_syntax::name(a).is_some_and(|n| {
                chip_command(&n)
                    || matches!(
                        n.as_str(),
                        "input" | "include" | "subfile" | "includegraphics" | "import"
                    )
            })
    })?;
    let name = latex_syntax::name(&cmd)?;
    let model = state.model();
    let (_, mands) = arguments(&cmd);
    let first = mands
        .first()
        .map(|m| m.trim().to_string())
        .unwrap_or_default();
    let base = doc.meta.path.as_deref().and_then(std::path::Path::parent);
    let file = |p: std::path::PathBuf| LinkAction::File {
        path: p.display().to_string(),
        search: None,
    };
    match name.as_str() {
        "url" | "href" => Some(LinkAction::Url(first)),
        "includegraphics" => picture_path(doc, &model, &cmd).map(|p| LinkAction::File {
            path: p,
            search: None,
        }),
        "input" | "include" | "subfile" | "import" => {
            let target = if name == "import" {
                format!("{}{}", first, mands.get(1).map_or("", |m| m.trim()))
            } else {
                first
            };
            let p = base.map_or_else(|| std::path::PathBuf::from(&target), |b| b.join(&target));
            let p = if p.exists() || p.extension().is_some() {
                p
            } else {
                p.with_extension("tex")
            };
            Some(file(p))
        }
        _ if latex_syntax::signatures::command(&name) == "*oom" => {
            let bib = model.bibliography.first()?.files.first()?.clone();
            Some(file(base.map_or_else(
                || std::path::PathBuf::from(&bib),
                |b| b.join(&bib),
            )))
        }
        _ => {
            let key = first.split(',').next()?.trim().to_string();
            match model.label(&key) {
                Some(l) if l.file == 0 => Some(LinkAction::Jump(l.range.start)),
                Some(l) => model.files.get(l.file).cloned().map(file),
                None => Some(LinkAction::Missing(key)),
            }
        }
    }
}

/// The sectioning commands of a LaTeX document for the outline panel:
/// levels from 1 (the document's top level), titles with their numbers.
pub fn outline_items(doc: &crate::DocumentState) -> Option<Vec<crate::view::OutlineItem>> {
    let model = doc.latex()?.model();
    let top = model.sections.iter().map(|s| s.level).min().unwrap_or(1);
    // The project's sections in reading order, those of the files it
    // includes with their file (design §9.5).
    Some(
        model
            .sections
            .iter()
            .filter(|s| s.file == 0 || model.files.get(s.file).is_some())
            .map(|s| crate::view::OutlineItem {
                level: (s.level - top + 1).max(1) as usize,
                todo: None,
                title: match &s.number {
                    Some(n) => format!("{n}\u{2003}{}", s.title),
                    None => s.title.clone(),
                },
                start: s.range.start,
                file: (s.file != 0).then(|| model.files[s.file].clone()),
            })
            .collect(),
    )
}

/// Whether the view renders command `name` (for the report of what a
/// document leaves as source).
pub fn renders_command(name: &str) -> bool {
    algorithm_words(name).is_some()
        || silent(name).is_some()
        || box_args(name).is_some()
        || format_style(name).is_some()
        || front_style(name).is_some()
        || declaration(name, &mut Style::default())
        || accent_mark(name).is_some()
        || transparent(name)
        || latex_syntax::signatures::is_sectioning(name)
        || chip_command(name)
        || word(name).is_some()
        || matches!(
            name,
            "item"
                | "caption"
                | "includegraphics"
                | "index"
                | "bf"
                | "bfseries"
                | "it"
                | "itshape"
                | "sl"
                | "slshape"
                | "em"
                | "tt"
                | "ttfamily"
                | "upshape"
                | "mdseries"
                | "rmfamily"
                | "sffamily"
                | "normalfont"
                | "rm"
                | "sc"
                | "scshape"
                | "sf"
                | "indexsee"
                | "glossary"
                | "nomenclature"
                | "ensuremath"
                | "texorpdfstring"
                | "centering"
                | "footnote"
                | "maketitle"
                | "label"
                | "begin"
                | "end"
                | "iffalse"
                | "fi"
                | "verb"
                | "lstinline"
                | "\\"
                | "appendix"
                | "frontmatter"
                | "mainmatter"
                | "backmatter"
                | "tableofcontents"
                | "bibliography"
                | "bibliographystyle"
                | "noindent"
                | "par"
                | "vspace"
                | "hspace"
                | "addvspace"
                | "vskip"
                | "hskip"
                | "today"
                | "MakeUppercase"
                | "MakeLowercase"
                | "uppercase"
                | "lowercase"
                | "MakeTextUppercase"
                | "MakeTextLowercase"
                | "bibitem"
                | "newblock"
                | "subfloat"
                | "subfigure"
                | "subtable"
                | "enquote"
                | "textcolor"
                | "color"
                | "footnotetext"
                | "minipage"
                | "input"
                | "include"
                | "subfile"
                | "num"
                | "si"
                | "SI"
                | "qty"
                | "unit"
                | "ang"
        )
}

/// Whether the view shows the arguments of command `name` as text (a
/// format's, a caption's) rather than drawing or hiding them (a
/// picture's options, a label's key).
pub fn shows_arguments(name: &str) -> bool {
    prose(name)
}

/// Whether the view renders environment `name` (theorems are the ones
/// the model declares).
pub fn renders_environment(name: &str, model: &latex_model::Model) -> bool {
    // Tables: the grid, or text with their cells apart.
    crate::latex_table::is_table(name)
        || tex_picture(name)
        || is_list(name)
        || float_name(name, false).is_some()
        || is_display_math(name)
        || latex_syntax::signatures::is_math(name)
        || latex_syntax::signatures::is_verbatim(name)
        || matches!(
            name,
            "document"
                | "center"
                | "flushleft"
                | "flushright"
                | "proof"
                | "quote"
                | "quotation"
                | "verse"
                | "abstract"
                | "thebibliography"
                | "minipage"
                | "subequations"
                | "algorithmic"
        )
        || front_environment(name).is_some()
        || model.theorem_kinds.iter().any(|k| k.env == name)
}

/// The front matter environments of the common classes and the heading
/// each shows on its `\begin` line (none for `frontmatter`, a container).
fn front_environment(name: &str) -> Option<&'static str> {
    Some(match name {
        "IEEEkeywords" => "Index Terms\u{2014}",
        "keyword" | "keywords" => "Keywords: ",
        "acknowledgments" | "acknowledgements" | "acknowledgment" | "acknowledgement" => {
            "Acknowledgments. "
        }
        // Containers: sizes, spacing, page turns, REVTeX's wide text,
        // table notes, boxes and appendices around text the view shows.
        "frontmatter"
        | "small"
        | "footnotesize"
        | "scriptsize"
        | "tiny"
        | "large"
        | "Large"
        | "normalsize"
        | "centering"
        | "landscape"
        | "spacing"
        | "singlespace"
        | "onehalfspace"
        | "doublespace"
        | "widetext"
        | "threeparttable"
        | "tablenotes"
        | "adjustbox"
        | "appendices"
        | "appendix"
        | "sloppypar"
        | "IEEEbiography"
        | "IEEEbiographynophoto"
        | "biography"
        | "restatable"
        | "mdframed"
        | "tcolorbox"
        | "framed"
        | "fullwidth" => "",
        _ => return None,
    })
}

/// What a float is called in its caption.
fn float_name(kind: &str, turkish: bool) -> Option<&'static str> {
    Some(match (kind.trim_end_matches('*'), turkish) {
        ("figure" | "wrapfigure" | "subfigure", false) => "Figure",
        ("figure" | "wrapfigure" | "subfigure", true) => "\u{15e}ekil",
        ("table" | "wraptable" | "subtable", false) => "Table",
        ("table" | "wraptable" | "subtable", true) => "Tablo",
        ("algorithm", false) => "Algorithm",
        ("algorithm", true) => "Algoritma",
        _ => return None,
    })
}

/// Environments TeX draws as pictures (TikZ's, pgf's, circuitikz's).
pub(crate) fn tex_picture(name: &str) -> bool {
    matches!(name, "tikzpicture" | "pgfpicture" | "circuitikz")
}

/// The PDF of picture `source` of `doc`, compiled by TeX with the
/// document's preamble (see [`crate::tex_pictures`]).
fn picture_by_tex(doc: &crate::DocumentState, state: &LatexState, source: &str) -> Option<String> {
    let root = state.root_path();
    let preamble = crate::tex_pictures::preamble_of(doc.text().as_str(), root.as_deref())?;
    let dir = state.root_dir().or_else(|| {
        doc.meta
            .path
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf)
    });
    crate::tex_pictures::picture(&preamble, source, dir.as_deref())
        .map(|p| p.to_string_lossy().into_owned())
}

/// The file an `\includegraphics` shows, relative to the document: its
/// name as written, in the document's folder or a `\graphicspath` folder,
/// with the extensions LaTeX tries when it has none.
fn picture_path(
    doc: &crate::DocumentState,
    model: &latex_model::Model,
    cmd: &SyntaxNode,
) -> Option<String> {
    let name = cmd
        .children()
        .find(|c| c.kind() == K::GROUP)
        .map(|g| group_text(&g))?;
    let base = doc.meta.path.as_deref().and_then(std::path::Path::parent);
    find_picture(base, &model.graphics_paths, name.trim())
}

/// The file of picture `name`, relative to the folder `base`: in it or a
/// `\graphicspath` folder, with LaTeX's extensions when it has none.
pub(crate) fn find_picture(
    base: Option<&std::path::Path>,
    graphics_paths: &[String],
    name: &str,
) -> Option<String> {
    let dirs = std::iter::once("").chain(graphics_paths.iter().map(String::as_str));
    let has_ext = std::path::Path::new(name).extension().is_some();
    for d in dirs {
        let stem = format!("{d}{name}");
        let tries: Vec<String> = if has_ext {
            vec![stem]
        } else {
            ["png", "jpg", "jpeg", "pdf", "svg", "eps"]
                .iter()
                .map(|e| format!("{stem}.{e}"))
                .collect()
        };
        for t in tries {
            let full = base.map_or_else(|| std::path::PathBuf::from(&t), |b| b.join(&t));
            if full.is_file() {
                return Some(t);
            }
        }
    }
    None
}

/// The size `width=`, `height=` or `scale=` asks for, in that order: a
/// share of `\textwidth`, `\linewidth` or `\columnwidth`, a length in
/// pixels at 96 dpi, or a share of the picture's own size.
fn picture_width(cmd: &SyntaxNode) -> Option<crate::view::ImageWidth> {
    use crate::view::ImageWidth;
    let opt = cmd
        .children()
        .find(|c| c.kind() == K::OPT_ARG)?
        .text()
        .to_string();
    let opt = opt.trim_start_matches('[').trim_end_matches(']');
    let get = |key: &str| {
        opt.split(',').find_map(|p| {
            let (k, v) = p.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let Some(value) = get("width") else {
        if let Some(h) = get("height").as_deref().and_then(length_px) {
            return Some(ImageWidth::Height(h));
        }
        let s: f64 = get("scale")?.parse().ok()?;
        return (s > 0.0).then(|| ImageWidth::Scale((s * 100.0).round().min(1000.0) as u32));
    };
    for w in ["\\textwidth", "\\linewidth", "\\columnwidth", "\\hsize"] {
        if let Some(f) = value.strip_suffix(w) {
            let f = f.trim();
            let f: f64 = if f.is_empty() { 1.0 } else { f.parse().ok()? };
            return Some(ImageWidth::Percent(
                (f * 100.0).round().clamp(1.0, 1000.0) as u32
            ));
        }
    }
    length_px(&value).map(ImageWidth::Pixels)
}

/// A length in pixels at 96 dpi (`5cm`, `30mm`, `2in`, `144pt`, `200px`).
fn length_px(value: &str) -> Option<u32> {
    let unit = |u: &str, px: f64| {
        value
            .strip_suffix(u)
            .and_then(|n| n.trim().parse::<f64>().ok())
            .map(|n| n * px)
    };
    let px = unit("cm", 37.8)
        .or_else(|| unit("mm", 3.78))
        .or_else(|| unit("in", 96.0))
        .or_else(|| unit("pt", 96.0 / 72.27))
        .or_else(|| unit("px", 1.0))?;
    (px >= 1.0).then(|| px.round() as u32)
}

/// The verbatim environment (`verbatim`, `lstlisting`, `minted`,
/// `comment`) whose source holds `pos`.
fn verbatim_env(root: &SyntaxNode, pos: usize) -> Option<SyntaxNode> {
    let t = latex_syntax::token_at(root, pos)?;
    t.parent_ancestors().find(|a| {
        a.kind() == K::ENVIRONMENT
            && latex_syntax::name(a).is_some_and(|n| latex_syntax::signatures::is_verbatim(&n))
    })
}

/// The language of a code environment: `lstlisting`'s `language=`,
/// `minted`'s argument.
fn code_language(env: &SyntaxNode) -> Option<String> {
    let name = latex_syntax::name(env)?;
    let begin = env.children().find(|c| c.kind() == K::BEGIN)?;
    let lang = match name.as_str() {
        "minted" => begin
            .children()
            .filter(|c| c.kind() == K::GROUP)
            .nth(1)
            .map(|g| group_text(&g)),
        "lstlisting" => begin
            .children()
            .find(|c| c.kind() == K::OPT_ARG)
            .and_then(|o| {
                group_text_brackets(&o).split(',').find_map(|p| {
                    let (k, v) = p.split_once('=')?;
                    (k.trim() == "language").then(|| v.trim().trim_matches(['{', '}']).to_string())
                })
            }),
        _ => None,
    }
    .or_else(|| {
        (name == "lstlisting")
            .then(|| lstset_language(env))
            .flatten()
    })?;
    Some(
        lang.trim_start_matches('[')
            .split(']')
            .next_back()
            .unwrap_or(&lang)
            .to_lowercase(),
    )
}

/// The `language=` of options `opts` (`[language=Python, …]`).
fn language_option(opts: &str) -> Option<String> {
    opts.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .find_map(|p| {
            let (k, v) = p.split_once('=')?;
            (k.trim() == "language").then(|| v.trim().trim_matches(['{', '}']).to_string())
        })
}

/// The listings language `\lstset{language=…}` sets for the document
/// holding `n`.
fn lstset_language(n: &SyntaxNode) -> Option<String> {
    let root = n.ancestors().last()?;
    let text = root.text().to_string();
    let mut found = None;
    let mut at = 0;
    while let Some(i) = text[at..].find("\\lstset{") {
        let start = at + i + "\\lstset{".len();
        let end = text[start..].find('}').map_or(text.len(), |e| start + e);
        if let Some(l) = language_option(&text[start..end]) {
            found = Some(l);
        }
        at = start;
    }
    found
}

/// The inline code on `line` with a known language: `\lstinline` with
/// `language=` in its options or from `\lstset`, as the source range of
/// its code and the language's name.
pub fn inline_code(doc: &crate::DocumentState, line: Range<usize>) -> Vec<(Range<usize>, String)> {
    let Some(state) = doc.latex() else {
        return Vec::new();
    };
    let text = doc.text().as_str();
    if !text[line.clone()].contains("\\lstinline") {
        return Vec::new();
    }
    let root = state.parse().syntax();
    let mut out = Vec::new();
    let mut tok = latex_syntax::token_at(&root, line.start);
    while let Some(t) = tok {
        if span(&t).start >= line.end {
            break;
        }
        tok = t.next_token();
        if t.kind() != K::CONTROL_WORD || t.text() != "\\lstinline" {
            continue;
        }
        let Some(verb) = t.parent().filter(|p| p.kind() == K::VERB) else {
            continue;
        };
        let Some(code) = verb_code(text, &verb) else {
            continue;
        };
        let after = &text[span(&t).end..code.start];
        let lang = if after.starts_with('[') {
            language_option(after.split(']').next().unwrap_or(""))
        } else {
            None
        }
        .or_else(|| lstset_language(&verb));
        if let Some(l) = lang {
            out.push((code, l.to_lowercase()));
        }
    }
    out
}

/// The outermost math around `t`: `$…$`, `\(…\)`, `\[…\]`, `$$…$$` or a
/// math environment.
fn math_node(t: &SyntaxToken) -> Option<SyntaxNode> {
    t.parent_ancestors()
        .filter(|a| match a.kind() {
            K::INLINE_MATH | K::DISPLAY_MATH => true,
            K::ENVIRONMENT => latex_syntax::name(a).is_some_and(|n| is_display_math(&n)),
            _ => false,
        })
        .last()
}

/// The formulas of the document's body the math renderer cannot read
/// (shown as their source in a red frame): each one's range and the
/// renderer's message, without the position it gives.
pub fn formula_failures(doc: &crate::DocumentState) -> Vec<(Range<usize>, String)> {
    let Some(state) = doc.latex() else {
        return Vec::new();
    };
    let text = doc.text().as_str();
    let root = state.parse().syntax();
    let body = state.model().body.clone().unwrap_or(0..text.len());
    let macros = org_math::source::macros(&math_definitions(doc));
    let mut out = Vec::new();
    let mut after = 0;
    for n in root.descendants() {
        let r = node_span(&n);
        if r.start < after || !body.contains(&r.start) {
            continue;
        }
        let math = match n.kind() {
            K::INLINE_MATH | K::DISPLAY_MATH => true,
            K::ENVIRONMENT => latex_syntax::name(&n).is_some_and(|x| is_display_math(&x)),
            _ => false,
        };
        if !math {
            continue;
        }
        after = r.end;
        let src = math_source(doc, r.clone()).unwrap_or_else(|| text[r.clone()].to_string());
        let (inner, _) = org_math::source::body(&src);
        if inner.trim().is_empty() {
            continue;
        }
        if let Err(e) = org_math::check(&org_math::source::prepare(inner, &macros)) {
            out.push((r, error_kind(&e.message)));
        }
    }
    out
}

/// A renderer's message without what changes from formula to formula
/// (positions, the text around): `Undefined control sequence: \foo`.
fn error_kind(message: &str) -> String {
    let m = message.lines().next().unwrap_or("").trim();
    // `ParseError at position 12: Undefined control sequence: \foo`.
    let m = match m.split_once(": ") {
        Some((head, rest)) if head.starts_with("ParseError") => rest,
        _ => m,
    };
    m.chars().take(80).collect()
}

/// Environments that are displayed formulas of their own (not `split`,
/// `aligned` and the others that live inside one).
fn is_display_math(name: &str) -> bool {
    matches!(
        name.trim_end_matches('*'),
        "equation"
            | "align"
            | "gather"
            | "multline"
            | "eqnarray"
            | "alignat"
            | "flalign"
            | "displaymath"
            | "math"
            | "xalignat"
            | "xxalignat"
            | "IEEEeqnarray"
            | "dmath"
            | "dseries"
            | "dgroup"
            | "darray"
            | "empheq"
            | "tikzcd"
    )
}

/// Definitions of the math commands of `physics`, `siunitx` and `bm`,
/// for the formula renderer, which does not know them (T2.7h.7).
const PACKAGE_MACROS: &[(&str, &[&str])] = &[
    ("bm", &["\\newcommand{\\bm}[1]{\\boldsymbol{#1}}"]),
    // delimset's delimiters (its sizes and kinds of `\\brk` aside).
    (
        "delimset",
        &[
            "\\newcommand{\\brk}[1]{\\left(#1\\right)}",
            "\\newcommand{\\abs}[1]{\\left|#1\\right|}",
            "\\newcommand{\\norm}[1]{\\left\\|#1\\right\\|}",
            "\\newcommand{\\set}[1]{\\left\\{#1\\right\\}}",
        ],
    ),
    (
        "physics",
        &[
            "\\newcommand{\\abs}[1]{\\left|#1\\right|}",
            "\\newcommand{\\norm}[1]{\\left\\|#1\\right\\|}",
            "\\newcommand{\\qty}[1]{\\left(#1\\right)}",
            "\\newcommand{\\dv}[2]{\\frac{\\mathrm{d}#1}{\\mathrm{d}#2}}",
            "\\newcommand{\\pdv}[2]{\\frac{\\partial #1}{\\partial #2}}",
            "\\newcommand{\\vb}[1]{\\mathbf{#1}}",
            "\\newcommand{\\va}[1]{\\vec{#1}}",
            "\\newcommand{\\vu}[1]{\\hat{\\mathbf{#1}}}",
            "\\newcommand{\\bra}[1]{\\left\\langle #1\\right|}",
            "\\newcommand{\\ket}[1]{\\left|#1\\right\\rangle}",
            "\\newcommand{\\braket}[2]{\\left\\langle #1\\middle|#2\\right\\rangle}",
            "\\newcommand{\\expval}[1]{\\left\\langle #1\\right\\rangle}",
            "\\newcommand{\\order}[1]{\\mathcal{O}\\left(#1\\right)}",
            "\\newcommand{\\tr}{\\operatorname{tr}}",
            "\\newcommand{\\Tr}{\\operatorname{Tr}}",
        ],
    ),
    (
        "siunitx",
        &[
            "\\newcommand{\\SI}[2]{#1\\,\\mathrm{#2}}",
            "\\newcommand{\\si}[1]{\\mathrm{#1}}",
            "\\newcommand{\\num}[1]{#1}",
            // Version 3's names.
            "\\newcommand{\\qty}[2]{#1\\,\\mathrm{#2}}",
            "\\newcommand{\\unit}[1]{\\mathrm{#1}}",
            "\\newcommand{\\ang}[1]{#1^\\circ}",
            "\\newcommand{\\percent}{\\%}",
            "\\newcommand{\\degree}{^\\circ}",
            "\\newcommand{\\ohm}{\\Omega}",
            "\\newcommand{\\litre}{L}",
            "\\newcommand{\\hour}{h}",
            "\\newcommand{\\giga}{G}",
            "\\newcommand{\\nano}{n}",
            "\\newcommand{\\coulomb}{C}",
            "\\newcommand{\\tesla}{T}",
            "\\newcommand{\\electronvolt}{eV}",
            "\\newcommand{\\square}{}",
            "\\newcommand{\\metre}{m}",
            "\\newcommand{\\meter}{m}",
            "\\newcommand{\\second}{s}",
            "\\newcommand{\\kilogram}{kg}",
            "\\newcommand{\\gram}{g}",
            "\\newcommand{\\kelvin}{K}",
            "\\newcommand{\\ampere}{A}",
            "\\newcommand{\\mole}{mol}",
            "\\newcommand{\\newton}{N}",
            "\\newcommand{\\joule}{J}",
            "\\newcommand{\\watt}{W}",
            "\\newcommand{\\volt}{V}",
            "\\newcommand{\\hertz}{Hz}",
            "\\newcommand{\\pascal}{Pa}",
            "\\newcommand{\\kilo}{k}",
            "\\newcommand{\\milli}{m}",
            "\\newcommand{\\micro}{\\mu}",
            "\\newcommand{\\centi}{c}",
            "\\newcommand{\\mega}{M}",
            "\\newcommand{\\per}{/}",
            "\\newcommand{\\squared}{^2}",
            "\\newcommand{\\cubed}{^3}",
        ],
    ),
];

/// The definitions the formula renderer takes for a LaTeX document: those
/// of the packages it loads that the renderer lacks, then the document's
/// own `\newcommand`s (which win).
pub fn math_definitions(doc: &crate::DocumentState) -> Vec<String> {
    let Some(state) = doc.latex() else {
        return Vec::new();
    };
    let model = state.model();
    let loaded = |p: &str| model.packages.iter().any(|m| m.name == p);
    let mut out: Vec<String> = PACKAGE_MACROS
        .iter()
        .filter(|(p, _)| loaded(p))
        .flat_map(|(_, defs)| defs.iter().map(|d| d.to_string()))
        // With siunitx, `\qty` is its quantity, not physics' parentheses.
        .filter(|d| !(loaded("siunitx") && d.starts_with("\\newcommand{\\qty}[1]")))
        .collect();
    out.extend(COMMON_MACROS.iter().map(|d| d.to_string()));
    let own: Vec<(String, usize, String)> = model
        .macros
        .iter()
        .zip(model.macro_definitions())
        .map(|(m, d)| (m.name.clone(), m.args, glossary_in_definition(&model, &d)))
        .collect();
    out.extend(accepted_definitions(&out, &own));
    out
}

/// Why a formula of `doc` finds `name` undefined, for the coverage
/// report's trace: the definition the model read, and what the renderer
/// said to it.
pub fn explain_macro(doc: &crate::DocumentState, name: &str) -> String {
    let Some(state) = doc.latex() else {
        return "not LaTeX".into();
    };
    let model = state.model();
    let defs = model.macro_definitions();
    let Some(i) = model.macros.iter().position(|m| m.name == name) else {
        return format!(
            "not in the model ({} macros, {} files; packages {})",
            model.macros.len(),
            model.files.len(),
            model
                .packages
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
    };
    let m = &model.macros[i];
    let mut before: Vec<String> = PACKAGE_MACROS
        .iter()
        .filter(|(p, _)| model.packages.iter().any(|x| x.name == *p))
        .flat_map(|(_, d)| d.iter().map(|d| d.to_string()))
        .collect();
    before.extend(COMMON_MACROS.iter().map(|d| d.to_string()));
    let own: Vec<(String, usize, String)> = model
        .macros
        .iter()
        .zip(defs.iter().cloned())
        .map(|(m, d)| (m.name.clone(), m.args, d))
        .take(i)
        .collect();
    let kept = org_math::source::macros(&{
        let mut b = before.clone();
        b.extend(accepted_definitions(&before, &own));
        b
    });
    let with = org_math::source::macros(std::slice::from_ref(&defs[i]));
    let trial = format!("{kept}{with}");
    let x = org_math::check(&org_math::source::prepare(
        "\\def\\kalemprobe{x}\\kalemprobe",
        &trial,
    ))
    .err();
    let use_it = format!("{name}{}", "{x}".repeat(m.args.max(3)));
    let u = org_math::check(&org_math::source::prepare(&use_it, &trial)).err();
    // The prepared text around where the renderer stopped.
    let near = |msg: &str, src: &str| -> String {
        let p = org_math::source::prepare(src, &trial);
        let at = msg
            .split("position ")
            .nth(1)
            .and_then(|r| r.split(':').next())
            .and_then(|n| n.trim().parse::<usize>().ok());
        match at {
            Some(a) => {
                let b = p
                    .char_indices()
                    .map(|(i, _)| i)
                    .find(|&i| i + 200 >= a)
                    .unwrap_or(0);
                let e = p
                    .char_indices()
                    .map(|(i, _)| i)
                    .find(|&i| i > a + 60)
                    .unwrap_or(p.len());
                format!(" near «{}»", &p[b.min(e)..e])
            }
            None => String::new(),
        }
    };
    let xs = x.map(|e| {
        format!(
            "{}{}",
            e.message,
            near(&e.message, "\\def\\kalemprobe{x}\\kalemprobe")
        )
    });
    let us = u.map(|e| format!("{}{}", e.message, near(&e.message, &use_it)));
    format!("{} (file {}): x -> {xs:?}; use -> {us:?}", defs[i], m.file)
}

/// The document's own definitions `own` (name, arguments, definition)
/// the renderer takes after `before` and with a use of each: one it
/// cannot read would stop every formula after it. Remembered, since the
/// definitions change far less often than the text.
fn accepted_definitions(before: &[String], own: &[(String, usize, String)]) -> Vec<String> {
    use std::hash::{Hash, Hasher};
    type Memo = std::collections::HashMap<u64, Vec<String>>;
    static MEMO: std::sync::Mutex<Option<Memo>> = std::sync::Mutex::new(None);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    before.hash(&mut h);
    own.hash(&mut h);
    let key = h.finish();
    if let Some(v) = MEMO
        .lock()
        .ok()
        .and_then(|m| m.as_ref()?.get(&key).cloned())
    {
        return v;
    }
    let mut kept = org_math::source::macros(before);
    let mut out = Vec::new();
    for (name, args, def) in own {
        // TeX's own definition commands are not redefined (a package's
        // `\def\def` read wrongly would end every definition after it).
        if matches!(
            name.as_str(),
            "\\def"
                | "\\gdef"
                | "\\edef"
                | "\\xdef"
                | "\\let"
                | "\\newcommand"
                | "\\renewcommand"
                | "\\providecommand"
                | "\\begin"
                | "\\end"
                | "\\relax"
                | "\\left"
                | "\\right"
                | "\\over"
                | "\\\\"
        ) {
            continue;
        }
        let with = org_math::source::macros(std::slice::from_ref(def));
        let trial = format!("{kept}{with}");
        // The definition read, a formula and a definition after it still
        // read.
        if org_math::check(&org_math::source::prepare(
            "\\def\\kalemprobe{x}\\kalemprobe",
            &trial,
        ))
        .is_err()
        {
            continue;
        }
        // A use of it: dropped only for a command the renderer lacks in
        // its body (`\\todo`, `\\marginpar`); a body that wants what
        // follows it (`\\def\\f{\\frac}`, `\\def\\lb{\\left(}`) is kept.
        let use_it = format!("{name}{}", "{x}".repeat((*args).max(3)));
        if let Err(e) = org_math::check(&org_math::source::prepare(&use_it, &trial))
            && (e.message.contains("Undefined control sequence")
                || e.message.contains("Too many expansions")
                || e.message.contains("Recursion limit"))
        {
            continue;
        }
        kept = trial;
        out.push(def.clone());
    }
    if let Ok(mut m) = MEMO.lock() {
        let m = m.get_or_insert_with(Memo::new);
        if m.len() > 256 {
            m.clear();
        }
        m.insert(key, out.clone());
    }
    out
}

/// Commands of packages that papers use in formulas and the renderer
/// lacks, as it can draw them (the definitions of `dsfont`, `bbm`,
/// `nicefrac`, amsmath's capital accents and others).
const COMMON_MACROS: &[&str] = &[
    "\\newcommand{\\mathds}[1]{\\mathbb{#1}}",
    "\\newcommand{\\mathbbm}[1]{\\mathbb{#1}}",
    "\\newcommand{\\mathbbold}[1]{\\mathbb{#1}}",
    "\\newcommand{\\Tilde}[1]{\\tilde{#1}}",
    "\\newcommand{\\Bar}[1]{\\bar{#1}}",
    "\\newcommand{\\Hat}[1]{\\hat{#1}}",
    "\\newcommand{\\Vec}[1]{\\vec{#1}}",
    "\\newcommand{\\Dot}[1]{\\dot{#1}}",
    "\\newcommand{\\Ddot}[1]{\\ddot{#1}}",
    "\\newcommand{\\Check}[1]{\\check{#1}}",
    "\\newcommand{\\Breve}[1]{\\breve{#1}}",
    "\\newcommand{\\Acute}[1]{\\acute{#1}}",
    "\\newcommand{\\Grave}[1]{\\grave{#1}}",
    "\\newcommand{\\nicefrac}[2]{{}^{#1}\\!/_{#2}}",
    "\\newcommand{\\sfrac}[2]{{}^{#1}\\!/_{#2}}",
    "\\newcommand{\\hdots}{\\dots}",
    "\\newcommand{\\ensuremath}[1]{#1}",
    "\\newcommand{\\scalebox}[2]{#2}",
    "\\newcommand{\\resizebox}[3]{#3}",
    "\\newcommand{\\raisebox}[2]{#2}",
    "\\newcommand{\\mbox}[1]{\\text{#1}}",
    "\\newcommand{\\hbox}[1]{\\text{#1}}",
    "\\newcommand{\\parbox}[2]{\\text{#2}}",
    "\\newcommand{\\textup}[1]{\\text{#1}}",
    "\\newcommand{\\textsc}[1]{\\text{#1}}",
    "\\newcommand{\\textsl}[1]{\\textit{#1}}",
    "\\newcommand{\\textmd}[1]{\\text{#1}}",
    "\\newcommand{\\textcolor}[2]{\\color{#1}{#2}}",
    "\\newcommand{\\protect}{}",
    "\\newcommand{\\nolimits}{}",
    "\\newcommand{\\displaylimits}{}",
    "\\newcommand{\\allowbreak}{}",
    "\\newcommand{\\nobreak}{}",
    "\\newcommand{\\vphantom}[1]{}",
    "\\newcommand{\\smash}[1]{#1}",
    "\\newcommand{\\mathlarger}[1]{#1}",
    "\\newcommand{\\mathsmaller}[1]{#1}",
    "\\newcommand{\\upmu}{\\mu}",
    "\\newcommand{\\updelta}{\\delta}",
    "\\newcommand{\\uppi}{\\pi}",
    "\\newcommand{\\coloneqq}{\\mathrel{:}=}",
    "\\newcommand{\\eqqcolon}{=\\mathrel{:}}",
    "\\newcommand{\\mathclap}[1]{#1}",
    "\\newcommand{\\mathllap}[1]{#1}",
    "\\newcommand{\\mathrlap}[1]{#1}",
    "\\newcommand{\\cancel}[1]{#1}",
    "\\newcommand{\\xmapsto}[1]{\\overset{#1}{\\longmapsto}}",
    "\\newcommand{\\bigast}{\\mathop{\\Large *}}",
    "\\newcommand{\\iddots}{\\cdot^{\\cdot^{\\cdot}}}",
    "\\newcommand{\\ul}[1]{\\underline{#1}}",
    "\\newcommand{\\uline}[1]{\\underline{#1}}",
    "\\newcommand{\\lefteqn}[1]{#1}",
    "\\newcommand{\\bm}[1]{\\boldsymbol{#1}}",
    "\\newcommand{\\boldmath}{}",
    "\\newcommand{\\unboldmath}{}",
    "\\newcommand{\\qedhere}{}",
    "\\newcommand{\\numberthis}{}",
    "\\newcommand{\\cite}[1]{\\text{[#1]}}",
    "\\newcommand{\\citep}[1]{\\text{[#1]}}",
    "\\newcommand{\\Tr}{\\operatorname{Tr}}",
    "\\newcommand{\\tr}{\\operatorname{tr}}",
    "\\newcommand{\\dd}{\\mathrm{d}}",
    "\\newcommand{\\eval}[1]{\\left.#1\\right|}",
    "\\newcommand{\\pqty}[1]{\\left(#1\\right)}",
    "\\newcommand{\\bqty}[1]{\\left[#1\\right]}",
    "\\newcommand{\\Bqty}[1]{\\left\\{#1\\right\\}}",
    "\\newcommand{\\vqty}[1]{\\left|#1\\right|}",
    "\\newcommand{\\mathpzc}[1]{\\mathcal{#1}}",
    "\\newcommand{\\bigints}{\\int}",
    "\\newcommand{\\bigintss}{\\int}",
    "\\newcommand{\\bigintsss}{\\int}",
    "\\newcommand{\\bigintssss}{\\int}",
    "\\newcommand{\\makebox}[1]{\\text{#1}}",
    "\\newcommand{\\framebox}[1]{\\boxed{\\text{#1}}}",
    "\\newcommand{\\uppsi}{\\psi}",
    "\\newcommand{\\upphi}{\\phi}",
    "\\newcommand{\\upgamma}{\\gamma}",
    "\\newcommand{\\upeta}{\\eta}",
    "\\newcommand{\\uplambda}{\\lambda}",
    "\\newcommand{\\upepsilon}{\\epsilon}",
    "\\newcommand{\\upomega}{\\omega}",
    "\\newcommand{\\sideset}[2]{}",
    "\\newcommand{\\slashed}[1]{\\not{#1}}",
    "\\newcommand{\\numprint}[1]{#1}",
    "\\newcommand{\\multicolumn}[3]{#3}",
    "\\newcommand{\\multirow}[3]{#3}",
    "\\newcommand{\\vspace}[1]{}",
    "\\newcommand{\\hspace}[1]{\\quad}",
    "\\newcommand{\\normalfont}{}",
    "\\newcommand{\\upvarphi}{\\varphi}",
    "\\newcommand{\\upalpha}{\\alpha}",
    "\\newcommand{\\upbeta}{\\beta}",
    "\\newcommand{\\upsigma}{\\sigma}",
    "\\newcommand{\\uptau}{\\tau}",
    "\\newcommand{\\fullmoon}{\\circ}",
    "\\newcommand{\\newmoon}{\\bullet}",
    "\\newcommand{\\label}[1]{}",
    "\\newcommand{\\nonumber}{}",
    "\\newcommand{\\notag}{}",
    "\\newcommand{\\tabularnewline}{\\\\}",
    "\\newcommand{\\arraybackslash}{}",
    "\\newcommand{\\centering}{}",
    "\\newcommand{\\noindent}{}",
    "\\newcommand{\\displaybreak}{}",
    "\\newcommand{\\allowdisplaybreaks}{}",
    "\\newcommand{\\intertext}[1]{\\text{#1}}",
    "\\newcommand{\\shortintertext}[1]{\\text{#1}}",
    "\\newcommand{\\mit}{\\mathit}",
    "\\newcommand{\\openone}{\\mathbb{1}}",
    "\\newcommand{\\slash}{/}",
    "\\newcommand{\\medmath}[1]{#1}",
    "\\newcommand{\\widebar}[1]{\\overline{#1}}",
    "\\newcommand{\\smashoperator}[2][]{#2}",
    "\\newcommand{\\footnote}[1]{}",
    "\\newcommand{\\ubar}[1]{\\underline{#1}}",
    "\\newcommand{\\sun}{\\odot}",
    "\\newcommand{\\fontsize}[2]{}",
    "\\newcommand{\\selectfont}{}",
    "\\newcommand{\\uppercase}[1]{#1}",
    "\\newcommand{\\lowercase}[1]{#1}",
    "\\newcommand{\\MakeUppercase}[1]{#1}",
    "\\newcommand{\\MakeLowercase}[1]{#1}",
    "\\newcommand{\\scr}[1]{\\mathscr{#1}}",
    "\\newcommand{\\none}{}",
    "\\newcommand{\\ifthenelse}[3]{#2}",
    "\\newcommand{\\joinrel}{\\mathrel{\\mkern-3mu}}",
    "\\newcommand{\\IEEEyesnumber}{}",
    "\\newcommand{\\IEEEnonumber}{}",
    "\\newcommand{\\IEEEyessubnumber}{}",
    "\\newcommand{\\IEEEnosubnumber}{}",
];

/// The formula the cursor at `pos` is in, as the renderer takes it (the
/// preview under the cursor, T2.7h.16).
pub fn formula_at(doc: &crate::DocumentState, pos: usize) -> Option<String> {
    let root = doc.latex()?.parse().syntax();
    let node = latex_syntax::token_before(&root, pos).and_then(|t| math_node(&t))?;
    let r = node_span(&node);
    if !(r.start < pos && pos < r.end) {
        return None;
    }
    math_source(doc, r)
}

/// The formula of a math node as the renderer takes it: `\label`,
/// `\nonumber` and `\notag` taken out, and the numbers LaTeX gives the
/// equations as `\tag`s of a starred environment.
pub fn math_source(doc: &crate::DocumentState, range: Range<usize>) -> Option<String> {
    let state = doc.latex()?;
    let text = doc.text().as_str();
    let root = state.parse().syntax();
    let node = latex_syntax::token_at(&root, range.start)
        .and_then(|t| math_node(&t))
        .filter(|n| node_span(n).start >= range.start)?;
    let r = node_span(&node);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let model = state.model();
    for c in node.descendants().filter(|c| c.kind() == K::COMMAND) {
        let cname = latex_syntax::name(&c);
        // A glossary entry in a formula: its math, or its text.
        if let Some(shown) = glossary_shown(&model, &c) {
            let with = match entry_math(&shown) {
                Some(m) => format!("{{{m}}}"),
                None => format!("\\text{{{shown}}}"),
            };
            edits.push((node_span(&c), with));
            continue;
        }
        if matches!(cname.as_deref(), Some("label" | "nonumber" | "notag")) {
            edits.push((node_span(&c), String::new()));
        }
        // A reference in a formula: the number LaTeX prints, as text.
        if let Some(n @ ("ref" | "eqref" | "autoref" | "cref" | "Cref" | "pageref")) =
            cname.as_deref()
            && let Some(g) = c.children().find(|x| x.kind() == K::GROUP)
        {
            let key = group_text(&g);
            let number = model
                .label(key.trim())
                .and_then(|l| l.number.clone())
                .unwrap_or_else(|| "??".into());
            let shown = if n == "eqref" {
                format!("({number})")
            } else {
                number
            };
            edits.push((node_span(&c), format!("\\text{{{shown}}}")));
        }
    }
    // `\be … \ee`: the environment their definitions open and close,
    // starred (only the tags number it).
    if node.kind() == K::DISPLAY_MATH {
        let words: Vec<_> = node
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == K::CONTROL_WORD)
            .collect();
        if let (Some(open), Some(close)) = (words.first(), words.last())
            && open.text_range().start() == node.text_range().start()
            && close.text_range().end() == node.text_range().end()
        {
            let env = model
                .macros
                .iter()
                .rev()
                .find(|m| m.name == open.text())
                .and_then(|m| {
                    let b = m.body.trim().strip_prefix("\\begin{")?;
                    Some(b.split('}').next()?.trim_end_matches('*').to_string())
                })
                .unwrap_or_else(|| "equation".into());
            let span = |t: &latex_syntax::SyntaxToken| {
                usize::from(t.text_range().start())..usize::from(t.text_range().end())
            };
            edits.push((span(open), format!("\\begin{{{env}*}}")));
            edits.push((span(close), format!("\\end{{{env}*}}")));
        }
    }
    let name = (node.kind() == K::ENVIRONMENT)
        .then(|| latex_syntax::name(&node))
        .flatten();
    let alias = node.kind() == K::DISPLAY_MATH
        && node
            .first_token()
            .is_some_and(|t| t.kind() == K::CONTROL_WORD);
    if name.is_some() || alias {
        for e in model
            .equations
            .iter()
            .filter(|e| e.file == 0 && r.start <= e.range.start && e.range.end <= r.end && !e.tag)
        {
            if let Some(n) = &e.number {
                edits.push((e.range.end..e.range.end, format!("\\tag{{{n}}}")));
            }
        }
    }
    if let Some(name) = &name {
        // Starred, so that only the tags number it (not a diagram, which
        // has no starred form).
        let numbers = !name.ends_with('*') && name != "tikzcd";
        if numbers {
            for pat in [format!("\\begin{{{name}}}"), format!("\\end{{{name}}}")] {
                for (i, _) in text[r.clone()].match_indices(&pat) {
                    let at = r.start + i + pat.len() - 1;
                    edits.push((at..at, "*".into()));
                }
            }
        }
    }
    edits.sort_by_key(|(e, _)| (e.start, e.end));
    let mut out = String::new();
    let mut at = r.start;
    for (e, ins) in edits {
        if e.start < at {
            continue;
        }
        out.push_str(&text[at..e.start]);
        out.push_str(&ins);
        at = e.end;
    }
    out.push_str(&text[at..r.end]);
    Some(optional_argument_macros(
        &own_environments(&out, &model.environments),
        &model,
    ))
}

/// The document's macros with an optional argument used in a formula
/// (`\\expec[x]{B}`, `\\expec{B}`) as their definitions, arguments put in:
/// the renderer's macros have no optional arguments.
fn optional_argument_macros(src: &str, model: &latex_model::Model) -> String {
    let mut s = src.to_string();
    for m in model
        .macros
        .iter()
        .rev()
        .filter(|m| m.default.is_some() && m.args > 0)
    {
        let name = m.name.as_str();
        let mut out = String::new();
        let mut rest = s.as_str();
        let mut changed = false;
        while let Some(i) = rest.find(name) {
            let after = &rest[i + name.len()..];
            if after.starts_with(|c: char| c.is_ascii_alphabetic()) {
                out.push_str(&rest[..i + name.len()]);
                rest = after;
                continue;
            }
            out.push_str(&rest[..i]);
            let mut args: Vec<String> = Vec::new();
            let mut tail = after;
            let t = tail.trim_start();
            if t.starts_with('[')
                && let Some(k) = t.find(']')
            {
                args.push(t[1..k].to_string());
                tail = &t[k + 1..];
            } else {
                args.push(m.default.clone().unwrap_or_default());
            }
            while args.len() < m.args {
                let t = tail.trim_start();
                if let Some(r) = t.strip_prefix('{')
                    && let Some(k) = matching_brace(r)
                {
                    args.push(r[..k].to_string());
                    tail = &r[k + 1..];
                } else if let Some(c) = t.chars().next() {
                    args.push(c.to_string());
                    tail = &t[c.len_utf8()..];
                } else {
                    break;
                }
            }
            let mut body = m.body.clone();
            for (k, a) in args.iter().enumerate() {
                body = body.replace(&format!("#{}", k + 1), a);
            }
            out.push_str(&format!("{{{body}}}"));
            rest = tail;
            changed = true;
        }
        out.push_str(rest);
        if changed {
            s = out;
        }
    }
    s
}

/// The document's own environments inside a formula
/// (`\newenvironment{smallmat}{\left(\begin{smallmatrix}}{\end{smallmatrix}\right)}`)
/// as their definitions, arguments put in.
fn own_environments(src: &str, envs: &[latex_model::NewEnvironment]) -> String {
    let mut s = src.to_string();
    for _ in 0..4 {
        let mut changed = false;
        for e in envs.iter().rev() {
            let begin = format!("\\begin{{{}}}", e.name);
            let end = format!("\\end{{{}}}", e.name);
            if !s.contains(&begin) {
                continue;
            }
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(i) = rest.find(&begin) {
                out.push_str(&rest[..i]);
                let mut after = &rest[i + begin.len()..];
                let mut args: Vec<String> = Vec::new();
                if e.args > 0 {
                    let t = after.trim_start();
                    if let Some(d) = e.default.as_ref() {
                        if t.starts_with('[')
                            && let Some(k) = t.find(']')
                        {
                            args.push(t[1..k].to_string());
                            after = &t[k + 1..];
                        } else {
                            args.push(d.clone());
                        }
                    }
                    while args.len() < e.args {
                        let t = after.trim_start();
                        match t
                            .strip_prefix('{')
                            .and_then(|r| matching_brace(r).map(|k| (r, k)))
                        {
                            Some((r, k)) => {
                                args.push(r[..k].to_string());
                                after = &r[k + 1..];
                            }
                            None => break,
                        }
                    }
                }
                let mut code = e.begin.clone();
                for (k, a) in args.iter().enumerate() {
                    code = code.replace(&format!("#{}", k + 1), a);
                }
                out.push_str(&code);
                rest = after;
            }
            out.push_str(rest);
            s = out.replace(&end, &e.end);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    s
}

/// The blocks of a LaTeX document for the editors' line layout: its
/// displayed formulas on lines of their own as math blocks (shown as one
/// formula away from the cursor), the text between them as paragraphs.
pub fn blocks(doc: &crate::DocumentState) -> Vec<crate::view::Block> {
    use crate::view::{Block, BlockKind};
    let Some(state) = doc.latex() else {
        return Vec::new();
    };
    let text = doc.text().as_str();
    let len = text.len();
    let block = |kind, range: Range<usize>, content_end| Block {
        kind,
        range,
        content_end,
        depth: 0,
        headline: None,
    };
    let mut out = Vec::new();
    let mut at = 0;
    // The preamble, up to the line of `\\begin{document}`: folded to its
    // first line away from the cursor, as Org folds drawers.
    if let Some(body) = state.model().body.clone() {
        let begin = text[..body.start.min(len)]
            .rfind("\\begin{document}")
            .map(|b| text[..b].rfind('\n').map_or(0, |n| n + 1));
        if let Some(end) = begin
            && text[..end].matches('\n').count() >= 2
        {
            out.push(block(BlockKind::Drawer, 0..end, end));
            at = end;
        }
    }
    let root = state.parse().syntax();
    let model = state.model();
    // Where a displayed formula or an environment can start: found in the
    // text, not by walking the whole tree.
    let mut starts: Vec<usize> = text
        .match_indices("\\begin")
        .chain(text.match_indices("\\["))
        .chain(text.match_indices("$$"))
        .map(|(i, _)| i)
        .collect();
    starts.sort_unstable();
    starts.dedup();
    let nodes = starts.into_iter().filter_map(|p| {
        latex_syntax::token_at(&root, p)?
            .parent_ancestors()
            .find(|a| {
                matches!(a.kind(), K::ENVIRONMENT | K::DISPLAY_MATH) && node_span(a).start == p
            })
    });
    // The text between `\iffalse` and its `\fi`, folded as a comment is.
    let mut items: Vec<(usize, Option<SyntaxNode>)> =
        nodes.map(|n| (node_span(&n).start, Some(n))).collect();
    let skipped = state.skipped(text);
    items.extend(skipped.iter().map(|r| (r.start, None)));
    items.sort_by_key(|(p, _)| *p);
    for (p, n) in items {
        let Some(n) = n else {
            let Some(r) = skipped.iter().find(|r| r.start == p).cloned() else {
                continue;
            };
            let line_start = text[..r.start].rfind('\n').map_or(0, |i| i + 1);
            let line_end = text[r.end..].find('\n').map_or(len, |i| r.end + i + 1);
            let alone = text[line_start..r.start].trim().is_empty()
                && text[r.end..line_end].trim().is_empty();
            if !alone || text[r.clone()].matches('\n').count() < 2 || line_start < at {
                continue;
            }
            if line_start > at {
                out.push(block(BlockKind::Paragraph, at..line_start, line_start));
            }
            out.push(block(BlockKind::Drawer, line_start..line_end, r.end));
            at = line_end;
            continue;
        };
        // A simple table's rows: the grid (T2.7h.9).
        if n.kind() == K::ENVIRONMENT
            && let Some(t) = crate::latex_table::simple(text, &n)
        {
            if t.body.start < at {
                continue;
            }
            if t.body.start > at {
                out.push(block(BlockKind::Paragraph, at..t.body.start, t.body.start));
            }
            let content_end = t.body.end - usize::from(text[..t.body.end].ends_with('\n'));
            out.push(block(BlockKind::Table, t.body.clone(), content_end));
            at = t.body.end;
            continue;
        }
        let kind = match n.kind() {
            K::DISPLAY_MATH => BlockKind::Math,
            K::ENVIRONMENT => match latex_syntax::name(&n) {
                Some(x) if is_display_math(&x) => BlockKind::Math,
                Some(x) if latex_syntax::signatures::is_verbatim(&x) && x != "comment" => {
                    BlockKind::Code {
                        language: code_language(&n),
                    }
                }
                // A picture TeX draws: folded to its `\\begin` line, where
                // the picture is.
                Some(x) if tex_picture(&x) => BlockKind::Drawer,
                // A `comment` environment, dimmed: folded as a drawer.
                Some(x) if x == "comment" && text[node_span(&n)].matches('\n').count() >= 2 => {
                    BlockKind::Drawer
                }
                // A long environment the view does not render (T2.7h.13):
                // folded to its `\\begin` line away from the cursor.
                Some(x)
                    if !renders_environment(&x, &model)
                        && text[node_span(&n)].matches('\n').count() >= LONG_UNKNOWN =>
                {
                    BlockKind::Drawer
                }
                _ => continue,
            },
            _ => continue,
        };
        let r = node_span(&n);
        let line_start = text[..r.start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[r.end..].find('\n').map_or(len, |i| r.end + i + 1);
        let alone =
            text[line_start..r.start].trim().is_empty() && text[r.end..line_end].trim().is_empty();
        if !alone || !text[r.clone()].contains('\n') || line_start < at {
            continue;
        }
        if line_start > at {
            out.push(block(BlockKind::Paragraph, at..line_start, line_start));
        }
        out.push(block(kind, line_start..line_end, r.end));
        at = line_end;
    }
    if at < len || out.is_empty() {
        out.push(block(BlockKind::Paragraph, at..len, len));
    }
    with_sections(out, text, &model)
}

/// `blocks` with each sectioning command's line a heading block, as Org's
/// headlines are, and the blocks after it under it (their depth and
/// headline), so that sections fold as Org's subtrees do (T2.7h.14).
fn with_sections(
    blocks: Vec<crate::view::Block>,
    text: &str,
    model: &latex_model::Model,
) -> Vec<crate::view::Block> {
    use crate::view::{Block, BlockKind};
    // Each section's line, and its level from 1.
    let mut heads: Vec<(usize, usize, i8)> = model
        .sections
        .iter()
        .filter(|s| s.file == 0 && s.range.start <= text.len())
        .filter_map(|s| {
            let ls = text[..s.range.start].rfind('\n').map_or(0, |i| i + 1);
            text[ls..s.range.start].trim().is_empty().then(|| {
                let le = text[s.range.start..]
                    .find('\n')
                    .map_or(text.len(), |i| s.range.start + i + 1);
                (ls, le, s.level)
            })
        })
        .collect();
    if heads.is_empty() {
        return blocks;
    }
    heads.sort_unstable();
    heads.dedup_by_key(|h| h.0);
    let top = heads.iter().map(|h| h.2).min().unwrap_or(1);
    let level = |l: i8| (l - top + 1).max(1) as usize;
    let mut out = Vec::with_capacity(blocks.len() + heads.len() * 2);
    let mut under: Option<(usize, usize)> = None;
    let push = |out: &mut Vec<Block>, mut b: Block, under: Option<(usize, usize)>| {
        if let Some((d, h)) = under {
            b.depth = d;
            b.headline = Some(h);
        }
        out.push(b);
    };
    let mut hi = 0;
    for b in blocks {
        if b.kind != BlockKind::Paragraph {
            // Sections do not start inside formulas or code.
            while hi < heads.len() && heads[hi].0 < b.range.start {
                hi += 1;
            }
            push(&mut out, b, under);
            continue;
        }
        let mut at = b.range.start;
        while hi < heads.len() && heads[hi].0 < b.range.end {
            let (ls, le, l) = heads[hi];
            hi += 1;
            if ls < at {
                continue;
            }
            if ls > at {
                let piece = Block {
                    kind: BlockKind::Paragraph,
                    range: at..ls,
                    content_end: ls,
                    depth: 0,
                    headline: None,
                };
                push(&mut out, piece, under);
            }
            let d = level(l);
            let le = le.min(b.range.end.max(le));
            out.push(Block {
                kind: BlockKind::Heading { level: d },
                range: ls..le,
                content_end: le,
                depth: d,
                headline: Some(ls),
            });
            under = Some((d, ls));
            at = le;
        }
        if at < b.range.end {
            let piece = Block {
                range: at..b.range.end,
                content_end: b.content_end.max(at),
                ..b
            };
            push(&mut out, piece, under);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> crate::DocumentState {
        let meta = crate::Metadata {
            path: None,
            mode: crate::DocumentMode::Latex,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        crate::DocumentState::new(text, meta, Arc::new(org_model::Settings::default()))
    }

    fn shown(d: &crate::DocumentState, line: usize, cursor: Option<usize>) -> LineView {
        let r = d.text().line_range(line);
        let r = r.start..r.end - usize::from(d.text().as_str()[r.clone()].ends_with('\n'));
        line_view(d, r, cursor)
    }

    #[test]
    fn tikz_pictures_drawn_by_tex() {
        let search = crate::pdf::tex_search_path();
        if crate::pdf::find("pdflatex", &search).is_none() {
            return;
        }
        let text = "\\documentclass{article}\n\\usepackage{tikz}\n\\begin{document}\n\\begin{tikzpicture}\n\\draw (0,0) -- (1,1);\n\\end{tikzpicture}\n\\end{document}\n";
        let d = doc(text);
        // Away from the cursor: the picture on its `\\begin` line.
        let v = shown(&d, 3, None);
        let path = v.runs.iter().find_map(|r| match &r.widget {
            Some(crate::view::Widget::Image { path, .. }) => Some(path.clone()),
            _ => None,
        });
        let path = path.expect("a picture");
        assert!(path.ends_with(".pdf"), "{path}");
        // At the cursor: the source.
        let at = text.find("\\draw").unwrap();
        let v = shown(&d, 3, Some(at));
        assert!(v.runs.iter().all(|r| r.widget.is_none()));
    }

    #[test]
    fn glossaries_and_acronyms() {
        let text = "\\documentclass{article}\n\\usepackage{glossaries}\n\\newacronym{cnn}{CNN}{convolutional network}\n\\newglossaryentry{disc}{name={\\ensuremath{\\gamma}},description={discount}}\n\\newglossaryentry{fee}{name=fee,description={a fee}}\n\\begin{document}\nA \\gls{cnn}, then \\gls{cnn} and \\acrlong{cnn}; \\Glspl{fee}.\n$\\gls{disc} = 1$\n\\end{document}\n";
        let d = doc(text);
        let v = shown(&d, 6, None);
        let shown_text: String = v
            .runs
            .iter()
            .filter(|r| !r.style.dim)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(
            shown_text,
            "A convolutional network (CNN), then CNN and convolutional network; Fees."
        );
        // In a formula: the entry's math.
        assert_eq!(formula_failures(&d), Vec::new());
        let at = text.find("$\\gls").unwrap();
        assert_eq!(math_source(&d, at..at + 1).unwrap(), "${\\gamma} = 1$");
        assert_eq!(crate::latex_check::coverage_report(text, None).source, 0);
    }

    #[test]
    fn own_text_macros() {
        let text = "\\newcommand{\\ie}{i.e.\\xspace}\n\\newcommand{\\method}{\\textsc{Foo}}\n\\newcommand{\\todo}[1]{}\n\\newcommand{\\R}{\\mathbb{R}}\nWe use \\method{} and \\method is good, \\ie fast\\todo{fix}. In \\R.\n";
        let d = doc(text);
        let v = shown(&d, 4, None);
        let shown_text: String = v
            .runs
            .iter()
            .filter(|r| !r.style.dim)
            .map(|r| r.text.as_str())
            .collect();
        // TeX takes the space after `\\method`, not after `\\xspace`; a
        // macro that is math stays as written (dimmed source).
        assert_eq!(shown_text, "We use Foo and Foois good, i.e. fast. In .");
        let c = crate::latex_check::coverage_report(text, None);
        assert_eq!(c.source, "\\R".len(), "{:?}", c.source_by_name);
    }

    #[test]
    fn links() {
        use crate::input::LinkAction;
        let text = "\\section{A}\\label{s}\nSee \\ref{s}, \\ref{no}, \\url{https://x.org} and \\input{chap}.\n";
        let mut d = doc(text);
        d.meta.path = Some(std::path::PathBuf::from("/w/p.tex"));
        let at = |s: &str| text.find(s).unwrap() + 2;
        assert_eq!(
            link_at(&d, at("\\ref{s}")),
            Some(LinkAction::Jump(text.find("\\label").unwrap()))
        );
        assert_eq!(
            link_at(&d, at("\\ref{no}")),
            Some(LinkAction::Missing("no".into()))
        );
        assert_eq!(
            link_at(&d, at("\\url")),
            Some(LinkAction::Url("https://x.org".into()))
        );
        assert_eq!(
            link_at(&d, at("\\input")),
            Some(LinkAction::File {
                path: std::path::Path::new("/w")
                    .join("chap.tex")
                    .display()
                    .to_string(),
                search: None
            })
        );
        assert_eq!(link_at(&d, 1), None);
    }

    #[test]
    fn lists() {
        let text = "\\begin{itemize}\n  \\item One\n  more\n  \\begin{enumerate}\n  \\item A\n  \\item[x)] B\n  \\item C\n  \\end{enumerate}\n\\item[Term] Two\n\\end{itemize}\n\\begin{enumerate}[label=(\\roman*), start=3]\n\\item Z\n\\end{enumerate}\n\\begin{description}\n\\item[Key] value\n\\end{description}\n";
        let d = doc(text);
        let lines: Vec<String> = (0..16)
            .map(|l| shown(&d, l, Some(text.len())).display())
            .collect();
        let em = "\u{2003}\u{2003}";
        assert_eq!(lines[1], "\u{2022} One");
        assert_eq!(lines[2], format!("{em}more"));
        assert_eq!(lines[4], format!("{em}1. A"));
        assert_eq!(lines[5], format!("{em}x) B"));
        assert_eq!(lines[6], format!("{em}2. C"));
        assert_eq!(lines[8], "Term Two");
        assert_eq!(lines[11], "(iii) Z");
        assert_eq!(lines[14], "Key value");
        assert_eq!(
            shown(&d, 0, Some(text.len())).role,
            crate::view::LineRole::Delimiter
        );
        assert!(
            shown(&d, 14, Some(text.len()))
                .runs
                .iter()
                .any(|r| r.text == "Key" && r.style.bold)
        );
    }

    #[test]
    fn rendered_lines() {
        let text = "\\title{On Things}\\author{Ada \\and Bob}\n\\begin{document}\n\\maketitle\n\\section{Intro}\\label{s}\nThis is \\emph{very} ``good''---really -- ok~now \\& more\\\\\n\\subsection*{Aside}\n% a comment\n\\begin{center}\nMiddle \\unknown{x} \\ldots\n\\end{center}\n\\end{document}\n";
        let d = doc(text);
        assert_eq!(shown(&d, 2, None).display(), "On Things\u{2003}Ada, Bob");
        let h = shown(&d, 3, None);
        assert_eq!(h.display(), "1\u{2003}Intro\\label{s}");
        assert_eq!(h.heading, 1);
        let body = shown(&d, 4, None);
        assert_eq!(
            body.display(),
            "This is very \u{201c}good\u{201d}\u{2014}really \u{2013} ok\u{a0}now & more\u{21b5}"
        );
        assert!(body.runs.iter().any(|r| r.text == "very" && r.style.italic));
        // The cursor in `\emph{…}` shows its markers.
        let at = text.find("very").unwrap();
        assert!(shown(&d, 4, Some(at)).display().contains("\\emph{very}"));
        let sub = shown(&d, 5, None);
        assert_eq!((sub.display().as_str(), sub.heading), ("Aside", 2));
        assert!(shown(&d, 6, None).runs[0].style.dim);
        let mid = shown(&d, 8, None);
        assert_eq!(mid.align, crate::rich::Align::Center);
        assert_eq!(mid.display(), "Middle \\unknown{x} \u{2026}");
    }

    #[test]
    fn math() {
        let text = "Inline $a^2$ and \\(b\\).\n\\begin{equation}\\label{e}\n  E = mc^2\n\\end{equation}\n\\begin{align}\n  x &= 1 \\\\\n  y &= 2 \\nonumber\n\\end{align}\n\\[ z \\]\n";
        let d = doc(text);
        let v = shown(&d, 0, Some(text.len()));
        let maths: Vec<&crate::view::Widget> =
            v.runs.iter().filter_map(|r| r.widget.as_ref()).collect();
        assert_eq!(maths.len(), 2);
        assert!(
            matches!(maths[0], crate::view::Widget::Math { source, display: false } if source == "$a^2$")
        );
        let eq = text.find("\\begin{equation}").unwrap();
        assert_eq!(
            math_source(&d, eq..eq).unwrap(),
            "\\begin{equation*}\n  E = mc^2\n\\tag{1}\\end{equation*}"
        );
        let al = text.find("\\begin{align}").unwrap();
        assert_eq!(
            math_source(&d, al..al).unwrap(),
            "\\begin{align*}\n  x &= 1 \\tag{2}\\\\\n  y &= 2 \n\\end{align*}"
        );
        let b = blocks(&d);
        let kinds: Vec<_> = b.iter().map(|b| b.kind.clone()).collect();
        use crate::view::BlockKind;
        assert_eq!(
            kinds,
            [
                BlockKind::Paragraph,
                BlockKind::Math,
                BlockKind::Math,
                BlockKind::Paragraph
            ]
        );
        assert_eq!(b[1].range.start, eq);
        assert_eq!(b.last().unwrap().range.end, text.len());
        // Single-line display math is drawn in its line.
        let last = shown(&d, 8, Some(0));
        assert!(matches!(
            &last.runs[0].widget,
            Some(crate::view::Widget::Math { display: true, .. })
        ));
    }

    #[test]
    fn text_symbols() {
        let text = "5\\texteuro{} \\textcopyright\\ 2026, \\LaTeX{} a\\hfill b \\textonehalf\n";
        let d = doc(text);
        assert_eq!(
            shown(&d, 0, Some(text.len())).display(),
            "5€ © 2026, LaTeX a\u{2002}b ½"
        );
    }

    #[test]
    fn formula_under_the_cursor() {
        let text = "A $x^2$ and \\begin{equation}\\frac{a}{b}\\label{e}\\end{equation}\n";
        let d = doc(text);
        let f = formula_at(&d, text.find("x^2").unwrap() + 1).unwrap();
        assert!(f.contains("x^2"), "{f}");
        let f = formula_at(&d, text.find("frac").unwrap()).unwrap();
        assert!(f.contains("\\frac{a}{b}") && !f.contains("label"), "{f}");
        assert_eq!(formula_at(&d, 1), None);
    }

    #[test]
    fn package_math_macros() {
        let text = "\\usepackage{physics,siunitx}\n\\newcommand{\\abs}[1]{|#1|}\n$\\abs{x}$\n";
        let d = doc(text);
        let defs = math_definitions(&d);
        assert!(defs.iter().any(|x| x.contains("\\pdv")));
        assert!(defs.iter().any(|x| x.contains("\\SI")));
        // bm's `\\bm` is defined with or without the package.
        assert!(defs.iter().any(|x| x.contains("\\bm")));
        // The document's own definition comes last and wins.
        assert_eq!(
            defs.last().map(String::as_str),
            Some("\\newcommand{\\abs}[1]{|#1|}")
        );
        let m = org_math::source::macros(&defs);
        assert!(m.contains("\\pdv"));
        // The renderer takes them.
        use org_math::MathEngine;
        for f in [
            "\\pdv{f}{x} + \\dv{g}{t}",
            "\\abs{x} \\norm{v} \\ket{\\psi}",
            "\\SI{3}{\\metre\\per\\second}",
        ] {
            let r = org_math::Request {
                latex: org_math::source::prepare(f, &m),
                display: false,
                size: 20.,
                scale: 1.,
                color: [0, 0, 0, 255],
            };
            assert!(org_math::Ratex.render(&r).is_ok(), "{f}");
        }
    }

    #[test]
    fn inline_code() {
        let text = "A \\verb|x_y| and \\lstinline[language=C]{a+b}.\n";
        let d = doc(text);
        let v = shown(&d, 0, Some(text.len()));
        assert_eq!(v.display(), "A x_y and a+b.");
        let code: String = v
            .runs
            .iter()
            .filter(|r| r.style.code)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(code, "x_ya+b");
        // At the cursor, the source.
        assert_eq!(shown(&d, 0, Some(4)).display(), "A \\verb|x_y| and a+b.");
    }

    #[test]
    fn quotes_and_abstract() {
        let text = "\\begin{abstract}\nWe show.\n\\end{abstract}\n\\begin{quote}\nSaid.\n\\begin{itemize}\n\\item One\n\\end{itemize}\n\\end{quote}\n";
        let d = doc(text);
        let end = Some(text.len());
        let title = shown(&d, 0, end);
        assert_eq!(title.display(), "Abstract");
        assert_eq!(title.align, crate::rich::Align::Center);
        assert!(title.runs[0].style.bold);
        // The abstract's and the quote's text indented as a list's is.
        assert_eq!(shown(&d, 1, end).display(), "\u{2003}\u{2003}We show.");
        assert_eq!(shown(&d, 2, end).role, crate::view::LineRole::Delimiter);
        assert_eq!(shown(&d, 4, end).display(), "\u{2003}\u{2003}Said.");
        // An item in a list in a quote: one level for each.
        assert!(shown(&d, 6, end).display().starts_with("\u{2003}\u{2003}•"));
    }

    #[test]
    fn inline_code_languages() {
        let text = "\\lstset{language=Rust}\nSee \\lstinline[language=Python]{x = 1} and \\lstinline|let y|.\n\\begin{lstlisting}\nfn f() {}\n\\end{lstlisting}\n";
        let d = doc(text);
        let line = d.text().line_range(1);
        let found = super::inline_code(&d, line);
        assert_eq!(found.len(), 2);
        assert_eq!(&text[found[0].0.clone()], "x = 1");
        assert_eq!(found[0].1, "python");
        assert_eq!(&text[found[1].0.clone()], "let y");
        assert_eq!(found[1].1, "rust");
        // `\lstset`'s language for a listing without options.
        let blocks = blocks(&d);
        assert!(
            blocks
                .iter()
                .any(|b| b.kind.highlight_language() == Some("rust"))
        );
    }

    #[test]
    fn index_entries_and_pdf_strings() {
        let text = "A word\\index{word!sub} here.\n\\section{\\texorpdfstring{$x^2$}{x squared} and more}\nAlways \\ensuremath{\\alpha} math.\n";
        let d = doc(text);
        let end = Some(text.len());
        let l0 = shown(&d, 0, end);
        assert_eq!(l0.display(), "A word\\index{word!sub} here.");
        assert!(
            l0.runs
                .iter()
                .any(|r| r.text.contains("\\index{word!sub}") && r.style.dim)
        );
        let l1 = shown(&d, 1, end).display();
        assert!(!l1.contains("x squared") && l1.contains("and more"), "{l1}");
        let l2 = shown(&d, 2, end);
        assert!(l2.runs.iter().any(|r| matches!(&r.widget, Some(crate::view::Widget::Math { source, .. }) if source == "\\alpha")));
    }

    #[test]
    fn typeset_as_pdflatex() {
        // Found by `tools/latex-typeset-fuzz.py`: each line shows (its
        // markup dimmed aside) what pdflatex typesets.
        let cases = [
            ("a \\textexclamdown{} \\textparagraph b", "a ¡ ¶b"),
            ("\\textnumero\\ \\textcelsius{} \\textohm", "№ ℃ \u{2126}"),
            ("a {\\' e} \\^ o \\`{\\i} b", "a é ô ì b"),
            ("\\^\\i {x}", "îx"),
            ("a\\;b\\:c\\!d", "a\u{2005}b\u{205f}cd"),
            ("a {\\bfseries x y} b {} c", "a x y b  c"),
            ("{\\small /}", "/"),
            ("a \\mbox{x y} \\fbox{z}", "a x y z"),
            ("\\MakeUppercase{x \\ss{} \\aa\\ ---}", "X SS Å —"),
            ("\\uppercase{x \\ss{} \\'e}", "X ß É"),
            ("\\MakeUppercase{\\textmu}", "µ"),
            ("\\MakeLowercase{\\textohm {\\={E}}}", "\u{2126}ē"),
            ("\\c C---x", "Ç—x"),
            ("x \\S\\", "x § "),
        ];
        let mut wrong = Vec::new();
        for (src, want) in cases {
            let text = format!("{src}\n");
            let d = doc(&text);
            let v = shown(&d, 0, None);
            let got: String = v
                .runs
                .iter()
                .filter(|r| !r.style.dim)
                .map(|r| r.text.as_str())
                .collect();
            if got != want {
                wrong.push(format!("{src}: want {want:?}, got {got:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn colors_boxes_and_settings() {
        let text = "A \\textcolor{red}{warm} and {\\color{blue} cool} \\resizebox{2cm}{!}{boxed}.\n\\setlength{\\tabcolsep}{3pt}\\def\\foo#1{bar}\n";
        let d = doc(text);
        let v = shown(&d, 0, None);
        let run = |w: &str| v.runs.iter().find(|r| r.text.contains(w)).unwrap();
        assert_eq!(
            run("warm").style.rich.color,
            Some(crate::theme::Color(0xff0000))
        );
        assert_eq!(
            run("cool").style.rich.color,
            Some(crate::theme::Color(0x0000ff))
        );
        assert_eq!(run(" and ").style.rich.color, None);
        let shown_text: String = v
            .runs
            .iter()
            .filter(|r| !r.style.dim)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(shown_text, "A warm and cool boxed.");
        // A setting and a definition print nothing: markup, dimmed.
        let v = shown(&d, 1, None);
        assert!(v.runs.iter().all(|r| r.style.dim), "{:?}", v.runs);
    }

    #[test]
    fn formulas_the_renderer_cannot_read() {
        let text = "\\documentclass{article}\n\\newcommand{\\R}{\\mathbb{R}}\n\\begin{document}\n$x \\in \\R$ and $\\nosuch{x}$ and $$\\frac{a}{b}$$\n\\end{document}\n";
        let d = doc(text);
        let f = formula_failures(&d);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(&text[f[0].0.clone()], "$\\nosuch{x}$");
        assert_eq!(f[0].1, "Undefined control sequence: \\nosuch");
        // A definition the renderer cannot read is left out, not the end
        // of every formula; eqnarray, amsmath's capital accents and
        // dsfont are read.
        let text = "\\documentclass{article}\n\\newcommand{\\bad}{\\begin{nosuch}}\n\\newcommand{\\good}{y}\n\\begin{document}\n$x + \\good$ $\\Tilde{O}(\\mathds{1})$\n\\begin{eqnarray}a &=& b\\end{eqnarray}\n$\\bad$\n\\end{document}\n";
        let d = doc(text);
        let f = formula_failures(&d);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(&text[f[0].0.clone()], "$\\bad$");
        // A reference in a formula, mathtools' paired delimiters, an
        // array's `@{}`.
        let text = "\\documentclass{article}\n\\usepackage{mathtools}\n\\DeclarePairedDelimiter{\\abs}{\\lvert}{\\rvert}\n\\begin{document}\n\\begin{equation}a\\label{e}\\end{equation}\n$\\eqref{e} + \\abs{x}$ $\\begin{array}{@{}c@{\\quad}c@{}}1&2\\end{array}$\n\\end{document}\n";
        let d = doc(text);
        assert_eq!(formula_failures(&d), Vec::new());
        let at = text.find("$\\eqref").unwrap();
        let src = math_source(&d, at..at + 1).unwrap();
        assert!(src.contains("\\text{(1)}"), "{src}");
        // `\be … \ee`: a displayed formula, as the environment their
        // definitions name.
        let text = "\\documentclass{article}\n\\def\\bea{\\begin{eqnarray}}\n\\def\\eea{\\end{eqnarray}}\n\\begin{document}\n\\bea\na &=& \\frac{1}{2}\n\\eea\nand \\be x \\ee\n\\end{document}\n";
        let d = doc(text);
        assert_eq!(formula_failures(&d), Vec::new());
        let at = text.find("\\bea\n").unwrap();
        let src = math_source(&d, at..at + 1).unwrap();
        assert!(src.starts_with("\\begin{eqnarray*}"), "{src}");
        assert!(src.contains("\\tag{1}"), "{src}");
        // An environment of the document's own inside a formula.
        let text = "\\documentclass{article}\n\\newenvironment{smallmat}{\\left(\\begin{smallmatrix}}{\\end{smallmatrix}\\right)}\n\\begin{document}\n$A = \\begin{smallmat}1 & 0\\end{smallmat}$\n\\end{document}\n";
        assert_eq!(formula_failures(&doc(text)), Vec::new());
        // Definitions as papers write them: `\\let`, a font size, an italic
        // correction, a glossary entry in the body.
        let text = "\\documentclass{article}\n\\usepackage{glossaries}\n\\newglossaryentry{fee}{name={\\ensuremath{f}},description={x}}\n\\let\\ov\\overline\n\\newcommand{\\oL}{\\ov{L}}\n\\newcommand{\\sM}{{\\mbox{\\fontsize{5}{5}\\selectfont{$M$}}}}\n\\newcommand{\\E}{\\mathop{\\bf E\\/}}\n\\newcommand{\\fee}{{\\gls[hyper=false]{fee}}}\n\\begin{document}\n$\\oL + D_\\sM + \\E[\\fee]$\n\\end{document}\n";
        assert_eq!(formula_failures(&doc(text)), Vec::new());
        // A package's internals read wrongly (`\\def\\def{@}`) do not end
        // the definitions after them.
        let text = "\\documentclass{article}\n\\def\\def{@}\n\\def\\fancy@head{x}\n\\newcommand{\\E}{\\mathbb{E}}\n\\begin{document}\n$\\E[x]$\n\\end{document}\n";
        assert_eq!(formula_failures(&doc(text)), Vec::new());
        // A macro with an optional argument, `\\ensuremath` in text, a
        // diagram on its own.
        let text = "\\documentclass{article}\n\\newcommand{\\expec}[2][]{\\mathbb{E}_{#1}\\left[#2\\right]}\n\\begin{document}\n$\\frac{\\expec{A}}{\\expec[x]{B}}$\n\\[ v=\\text{ \\ensuremath{\\left(s-\\frac{1}{2}\\right)}} \\]\n\\begin{tikzcd} A \\arrow[r] & B \\end{tikzcd}\n\\end{document}\n";
        assert_eq!(formula_failures(&doc(text)), Vec::new());
        // One the document defines as an equation: a numbered formula.
        let text = "\\documentclass{article}\n\\newenvironment{eqn}{\\begin{equation}}{\\end{equation}}\n\\begin{document}\n\\begin{eqn}\na = \\frac{1}{2}\n\\end{eqn}\n\\end{document}\n";
        let d = doc(text);
        assert_eq!(formula_failures(&d), Vec::new());
        let at = text.find("\\begin{eqn}\n").unwrap();
        let src = math_source(&d, at..at + 1).unwrap();
        assert!(src.contains("\\tag{1}"), "{src}");
        assert_eq!(crate::latex_check::coverage_report(text, None).source, 0);
        assert_eq!(crate::latex_check::coverage_report(text, None).source, 0);
    }

    #[test]
    fn quotes_and_algorithms() {
        let text = "\\usepackage{csquotes,algorithm,algpseudocode}\nSay \\enquote{a \\enquote{b}} and \\enquote*{c}.\n\\begin{algorithm}\n\\caption{Search}\n\\begin{algorithmic}\n\\Procedure{Find}{$x$}\n\\State y\n\\For{all}\n\\Return z \\Comment{done}\n\\EndFor\n\\EndProcedure\n\\end{algorithmic}\n\\end{algorithm}\n";
        let d = doc(text);
        let text_of = |line: usize| -> String {
            shown(&d, line, None)
                .runs
                .iter()
                .filter(|r| !r.style.dim)
                .map(|r| r.text.as_str())
                .collect()
        };
        assert_eq!(
            text_of(1),
            "Say \u{201c}a \u{2018}b\u{2019}\u{201d} and \u{2018}c\u{2019}."
        );
        assert_eq!(text_of(3), "Algorithm 1 Search");
        assert_eq!(
            text_of(5),
            format!("procedure Find({})", crate::view::PLACEHOLDER)
        );
        assert_eq!(text_of(6), "y");
        assert_eq!(text_of(7), "for all do");
        assert_eq!(text_of(8), "return z \u{25b7} done");
        assert_eq!(text_of(9), "end for");
        assert_eq!(text_of(10), "end procedure");
        // Keywords bold, as the package sets them.
        assert!(
            shown(&d, 9, None)
                .runs
                .iter()
                .any(|r| r.text == "end for" && r.style.bold)
        );
    }

    #[test]
    fn paragraphs_as_tex_sets_them() {
        // The lines of a paragraph are one paragraph, each line break a
        // space; a blank line, a display, `\\\\` or a comment at a line's
        // end ends the run.
        let text = "\\documentclass{article}\n\\begin{document}\n\nThe well known theorem $x^2+y^2=z^2$ was\nproved to be invalid for other exponents.\nMeaning the next equation has no integer solutions:\n\\[x^n + y^n = z^n\\]\nOne line.\n\nForced \\\\\nbreak.\nNo % space\nhere.\n\\section{S}\nA\nB\n\\end{document}\n";
        let d = doc(text);
        let runs = joined_paragraphs(&d);
        let shown: Vec<&str> = runs.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(
            shown,
            [
                "The well known theorem $x^2+y^2=z^2$ was\nproved to be invalid for other exponents.\nMeaning the next equation has no integer solutions:",
                "break.\nNo % space",
                "A\nB",
            ]
        );
        let v = paragraph_view(&d, runs[0].clone(), None);
        let t: String = v
            .runs
            .iter()
            .filter(|r| !r.style.dim)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(
            t,
            format!(
                "The well known theorem {} was proved to be invalid for other exponents. Meaning the next equation has no integer solutions:",
                crate::view::PLACEHOLDER
            )
        );
        // A position in the second line maps back to it.
        let at = text.find("proved").unwrap();
        assert_eq!(v.source_offset(v.display_offset(at)), at);
        // Indented as TeX indents a paragraph: after a blank line, not
        // after a heading; not at the cursor.
        assert_eq!(v.runs[0].text, "\u{2003}\u{2002}");
        let after_heading = paragraph_view(&d, runs[2].clone(), None);
        assert_ne!(after_heading.runs[0].text, "\u{2003}\u{2002}");
        let at_cursor = paragraph_view(&d, runs[0].clone(), Some(runs[0].start + 1));
        assert_ne!(at_cursor.runs[0].text, "\u{2003}\u{2002}");
    }

    #[test]
    fn displayed_formulas_are_centered() {
        // As LaTeX sets them: centered on their line; flush left with
        // `fleqn`; a formula in the text stays where it is.
        let text = "\\documentclass{article}\n\\begin{document}\nText $x$ here.\n\\[x^n + y^n = z^n\\]\n$$a$$\n\\end{document}\n";
        let d = doc(text);
        assert_eq!(shown(&d, 2, None).align, crate::rich::Align::default());
        assert_eq!(shown(&d, 3, None).align, crate::rich::Align::Center);
        assert_eq!(shown(&d, 4, None).align, crate::rich::Align::Center);
        assert_eq!(display_align(&d), crate::rich::Align::Center);
        let d = doc(&text.replace("{article}", "[fleqn]{article}"));
        assert_eq!(shown(&d, 3, None).align, crate::rich::Align::Left);
        assert_eq!(display_align(&d), crate::rich::Align::Left);
    }

    #[test]
    fn subfloats_show_their_letter_and_caption() {
        let text = "\\usepackage{subfig}\n\\begin{figure}\n\\subfloat[Left $x$]{X}\n\\subfloat{Y}\n\\subfloat[][]{Z}\n\\subfloat[L][Shown]{W}\n\\caption{C}\n\\end{figure}\n";
        let d = doc(text);
        let text_of = |line: usize| -> String {
            shown(&d, line, None)
                .runs
                .iter()
                .filter(|r| !r.style.dim)
                .map(|r| r.text.as_str())
                .collect()
        };
        assert_eq!(
            text_of(2),
            format!("(a) Left {} X", crate::view::PLACEHOLDER)
        );
        assert_eq!(text_of(3), "Y");
        assert_eq!(text_of(4), "(c) Z");
        assert_eq!(text_of(5), "(d) Shown W");
        // The letter bold, as a caption's label.
        assert!(
            shown(&d, 2, None)
                .runs
                .iter()
                .any(|r| r.text == "(a) " && r.style.bold)
        );
    }

    #[test]
    fn tables_the_grid_does_not_draw() {
        // A row over two lines: text, its cells apart, rules and spans'
        // arguments markup.
        let text = "\\begin{tabular}{lcc}\n\\toprule\n\\multirow{2}{*}{Model} & \\multicolumn{2}{c}{Score} \\\\\n\\cmidrule(lr){2-3}\n & A &\n B \\\\ \\bottomrule\n\\end{tabular}\n";
        let d = doc(text);
        let text_of = |line: usize| -> String {
            shown(&d, line, None)
                .runs
                .iter()
                .filter(|r| !r.style.dim)
                .map(|r| r.text.as_str())
                .collect()
        };
        assert_eq!(text_of(1), "");
        assert_eq!(
            text_of(2).split_whitespace().collect::<Vec<_>>(),
            ["Model", "Score"]
        );
        assert_eq!(text_of(3), "");
        let v = shown(&d, 4, None);
        assert!(
            v.runs
                .iter()
                .any(|r| r.text.contains('\u{2502}') && r.style.dim)
        );
    }

    #[test]
    fn font_declarations() {
        let text = "Plain {\\bf bold {\\it both}} and {\\em it \\normalfont up}.\n";
        let d = doc(text);
        let v = shown(&d, 0, Some(text.len()));
        let style_of = |w: &str| {
            v.runs
                .iter()
                .find(|r| r.text.contains(w) && !r.style.dim)
                .map(|r| r.style)
                .unwrap()
        };
        assert!(style_of("bold").bold && !style_of("bold").italic);
        assert!(style_of("both").bold && style_of("both").italic);
        // (The blank after a declaration is eaten, with it.)
        assert!(style_of("it ").italic);
        assert!(!style_of("up").italic);
        assert!(!style_of("Plain").bold);
    }

    #[test]
    fn dimmed_parts_fold() {
        use crate::view::BlockKind;
        let text = "Text.\n\\iffalse\nold\nolder\n\\fi\nMore.\n\\begin{comment}\na\nb\n\\end{comment}\nEnd.\n";
        let d = doc(text);
        let drawers: Vec<_> = blocks(&d)
            .into_iter()
            .filter(|b| b.kind == BlockKind::Drawer)
            .map(|b| &text[b.range])
            .collect();
        assert_eq!(
            drawers,
            [
                "\\iffalse\nold\nolder\n\\fi\n",
                "\\begin{comment}\na\nb\n\\end{comment}\n"
            ]
        );
    }

    #[test]
    fn class_front_matter() {
        // IEEEtran: author blocks and the affiliation left out of the byline.
        let text = "\\documentclass{IEEEtran}\n\\title{A Study}\n\\author{\\IEEEauthorblockN{Ada Lovelace}\n\\IEEEauthorblockA{Analytical Engines\\\\London}\n\\and\n\\IEEEauthorblockN{Bob Byron\\thanks{Funded.}}}\n\\begin{document}\n\\maketitle\n\\begin{IEEEkeywords}\nengines, looms\n\\end{IEEEkeywords}\n\\IEEEPARstart{T}{his} paper.\n\\end{document}\n";
        let d = doc(text);
        let end = Some(text.len());
        let title = shown(&d, 7, end);
        assert_eq!(title.display(), "A Study\u{2003}Ada Lovelace, Bob Byron");
        assert_eq!(shown(&d, 8, end).display(), "Index Terms\u{2014}");
        assert_eq!(shown(&d, 10, end).role, crate::view::LineRole::Delimiter);
        assert_eq!(shown(&d, 11, end).display(), "This paper.");
        // acmart: the front matter in the body shows where it is written.
        let text = "\\documentclass{acmart}\n\\begin{document}\n\\title{Deep Things}\n\\author{Ada}\n\\affiliation{\\institution{Uni}\\city{Paris}\\country{France}}\n\\email{ada@uni.fr}\n\\keywords{a, b}\n\\maketitle\n\\end{document}\n";
        let d = doc(text);
        let end = Some(text.len());
        let t = shown(&d, 2, end);
        assert_eq!(t.display(), "Deep Things");
        assert!(t.runs.iter().all(|r| r.style.title));
        assert_eq!(shown(&d, 3, end).display(), "Ada");
        assert_eq!(shown(&d, 4, end).display(), "Uni, Paris, France");
        assert!(shown(&d, 4, end).runs.iter().all(|r| r.style.byline));
        assert_eq!(shown(&d, 5, end).display(), "ada@uni.fr");
        assert_eq!(shown(&d, 6, end).display(), "Keywords: a, b");
        // No title in the preamble: `\maketitle` stays as written.
        assert_eq!(shown(&d, 7, end).display(), "\\maketitle");
        // llncs: several authors, `\inst` left out.
        let text = "\\documentclass{llncs}\n\\title{T}\n\\author{Ada\\inst{1} \\and Bob\\inst{2}}\n\\institute{Uni \\email{a@b}}\n\\begin{document}\n\\maketitle\n\\end{document}\n";
        let d = doc(text);
        assert_eq!(
            shown(&d, 5, Some(text.len())).display(),
            "T\u{2003}Ada, Bob"
        );
        // elsarticle: `frontmatter` a container, `keyword` a heading.
        let text = "\\documentclass{elsarticle}\n\\begin{document}\n\\begin{frontmatter}\n\\title{E}\n\\begin{keyword}\nx \\sep y\n\\end{keyword}\n\\end{frontmatter}\n\\end{document}\n";
        let d = doc(text);
        let end = Some(text.len());
        assert_eq!(shown(&d, 2, end).role, crate::view::LineRole::Delimiter);
        assert_eq!(shown(&d, 4, end).display(), "Keywords: ");
        assert_eq!(shown(&d, 7, end).role, crate::view::LineRole::Delimiter);
    }

    #[test]
    fn picture_sizes() {
        use crate::view::ImageWidth;
        let size = |opts: &str| {
            let p = latex_syntax::parse(&format!("\\includegraphics[{opts}]{{a}}"));
            let cmd = p
                .syntax()
                .descendants()
                .find(|n| n.kind() == K::COMMAND)
                .unwrap();
            picture_width(&cmd)
        };
        assert_eq!(size("width=0.5\\linewidth"), Some(ImageWidth::Percent(50)));
        assert_eq!(size("width=2in"), Some(ImageWidth::Pixels(192)));
        assert_eq!(size("height=1in"), Some(ImageWidth::Height(96)));
        assert_eq!(size("scale=0.25"), Some(ImageWidth::Scale(25)));
        // `width=` wins over the others.
        assert_eq!(size("scale=2, width=1cm"), Some(ImageWidth::Pixels(38)));
        assert_eq!(size("angle=90"), None);
    }

    #[test]
    fn a_package_of_the_document_s_own() {
        // `\\usepackage{macros}` with `macros.sty` beside a document that
        // includes nothing: its definitions reach the formulas.
        let dir = std::env::temp_dir().join(format!("kalem-latex-sty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("macros.sty"),
            "\\ProvidesPackage{macros}\n\\newcommand{\\floor}[1]{\\lfloor #1\\rfloor}\n",
        )
        .unwrap();
        let text = "\\documentclass{article}\n\\usepackage{macros}\n\\begin{document}\n$\\floor{x}$\n\\end{document}\n";
        let f = dir.join("main.tex");
        std::fs::write(&f, text).unwrap();
        let base = crate::settings::Config::default().parse_base();
        let mut d = crate::DocumentState::open(
            &f,
            std::sync::Arc::new(org_model::Settings::default()),
            &base,
        )
        .unwrap();
        d.wait_for_latex_project();
        assert_eq!(formula_failures(&d), Vec::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn floats() {
        let dir = std::env::temp_dir().join(format!("kalem-latex-view-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("figs")).unwrap();
        std::fs::write(dir.join("figs/cat.png"), b"png").unwrap();
        let text = "\\usepackage[turkish]{babel}\n\\graphicspath{{figs/}}\n\\begin{figure}\n\\centering\n\\includegraphics[width=0.5\\textwidth]{cat}\n\\caption{A cat.}\\label{f}\n\\includegraphics{missing}\n\\end{figure}\n";
        let mut d = doc(text);
        d.meta.path = Some(dir.join("p.tex"));
        let end = Some(text.len());
        assert_eq!(shown(&d, 2, end).role, crate::view::LineRole::Delimiter);
        assert_eq!(shown(&d, 3, end).display(), "");
        let pic = shown(&d, 4, end);
        assert_eq!(pic.align, crate::rich::Align::Center);
        assert!(matches!(
            &pic.runs[0].widget,
            Some(crate::view::Widget::Image { path, width: Some(crate::view::ImageWidth::Percent(50)) }) if path == "figs/cat.png"
        ));
        assert_eq!(
            shown(&d, 5, end).display(),
            "\u{15e}ekil 1: A cat.\\label{f}"
        );
        // A missing file stays as its source.
        assert_eq!(shown(&d, 6, end).display(), "\\includegraphics{missing}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn citations_as_bibtex_styles_print_them() {
        // `tests/latex/citations`: one document per style citing
        // `refs.bib`, each line after `\clearpage` a citation, and what
        // pdflatex and bibtex print for it.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/latex/citations/");
        let mut wrong = Vec::new();
        // (`manual`, `manual-natbib`: a document's own `thebibliography`.)
        for style in [
            "plain",
            "unsrt",
            "alpha",
            "abbrv",
            "ieeetr",
            "plainnat",
            "manual",
            "manual-natbib",
        ] {
            let text = std::fs::read_to_string(format!("{dir}{style}.tex")).unwrap();
            let expected = std::fs::read_to_string(format!("{dir}{style}.expected")).unwrap();
            let mut d = doc(&text);
            d.meta.path = Some(std::path::PathBuf::from(format!("{dir}{style}.tex")));
            let first = text[..text.find("\\clearpage").unwrap()].lines().count() + 1;
            let end = Some(text.len());
            for (i, want) in expected.lines().enumerate() {
                let line = first + i;
                let got = shown(&d, line, end).display().replace('\u{a0}', " ");
                if got.trim() != want.trim() {
                    wrong.push(format!(
                        "{style}: {}: LaTeX {want:?}, Kalem {got:?}",
                        text.lines().nth(line).unwrap_or("")
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn references_as_latex_prints_them() {
        // `tests/latex/references`: each line after `\clearpage` a
        // reference, and what pdflatex typesets for it with hyperref and
        // cleveref (read from `\showbox`).
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/latex/references/");
        let text = std::fs::read_to_string(format!("{dir}references.tex")).unwrap();
        let expected = std::fs::read_to_string(format!("{dir}references.expected")).unwrap();
        let d = doc(&text);
        let first = text[..text.find("\\clearpage").unwrap()].lines().count() + 1;
        let end = Some(text.len());
        let mut wrong = Vec::new();
        for (i, want) in expected.lines().enumerate() {
            let line = first + i;
            let got = shown(&d, line, end).display().replace('\u{a0}', " ");
            if got.trim() != want.trim() {
                wrong.push(format!(
                    "{}: LaTeX {want:?}, Kalem {got:?}",
                    text.lines().nth(line).unwrap_or("")
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn references_and_citations() {
        let dir = std::env::temp_dir().join(format!("kalem-latex-cite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("refs.bib"), "@book{knuth,\n  author = {Knuth, Donald E.},\n  title = {The TeXbook},\n  year = {1984}\n}\n@article{ll,\n  author = {Lamport, Leslie and Lynch, Nancy},\n  year = 1990\n}\n").unwrap();
        let text = "\\section{One}\\label{s}\n\\begin{equation}\\label{e} a \\end{equation}\nSee \\ref{s}, \\eqref{e}, \\cref{e}, \\autoref{s}, \\ref{nope}.\n\\cite[p.~3]{knuth,ll} \\citet{knuth} \\citep[see][]{ll} \\cite{zzz}\nText\\footnote{A note.} \\url{https://x.org} \\href{https://y.org}{Y}\n\\bibliography{refs}\n";
        let mut d = doc(text);
        d.meta.path = Some(dir.join("p.tex"));
        let end = Some(text.len());
        let refs = shown(&d, 2, end);
        assert_eq!(
            refs.display(),
            "See 1, (1), eq.\u{a0}(1), section\u{a0}1, ??."
        );
        assert!(
            refs.runs
                .iter()
                .any(|r| r.text == "??" && r.style.todo == Some(true))
        );
        assert_eq!(
            shown(&d, 3, end).display(),
            "[Knuth 1984; Lamport and Lynch 1990, p.\u{a0}3] Knuth (1984) (see Lamport and Lynch 1990) [zzz]"
        );
        let foot = shown(&d, 4, end);
        assert_eq!(foot.display(), "Text1 A note. https://x.org Y");
        assert!(
            foot.runs
                .iter()
                .any(|r| r.text == "1" && r.style.superscript)
        );
        crate::l10n::set_language("en");
        let at = |s: &str| text.find(s).unwrap() + 2;
        assert_eq!(
            note_at(&d, at("\\ref{s}")).as_deref(),
            Some("Section 1: One")
        );
        assert_eq!(
            note_at(&d, at("\\ref{nope}")).as_deref(),
            Some("No label nope")
        );
        // The entry as the CSL style's bibliography has it.
        let card = note_at(&d, at("\\citet")).unwrap();
        assert_eq!(
            card, "@knuth: Knuth, Donald E. 1984. The Texbook.",
            "{card}"
        );
        assert!(note_at(&d, at("\\footnote")).unwrap().contains("A note."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accents_and_special_letters() {
        let text = "G\\\"odel, \\c{c}a, \\u{g}, \\'{\\i}, \\ss{} and \\AE, \\\"{U}ber, \\v s, \\H{o}, \\k{a}, \\r{a}, \\=a, \\.z, \\~n, \\^o, \\`e, \\o, \\l\n";
        let d = doc(text);
        assert_eq!(
            shown(&d, 0, Some(text.len())).display(),
            "Gödel, ça, ğ, í, ß and Æ, Über, š, ő, ą, å, ā, ż, ñ, ô, è, ø, ł"
        );
    }

    #[test]
    fn spacing_and_page_commands() {
        let text = "A \\medskip B \\vspace{1em}C \\hspace{2pt}D \\newpage E \\slash{} F\\-G \\guillemotleft x\\guillemotright{} \\listoffigures\n";
        let d = doc(text);
        assert_eq!(
            shown(&d, 0, Some(text.len())).display(),
            "A B C  D \u{21a1}E / FG «x» List of Figures"
        );
    }

    #[test]
    fn declarations_and_sizes() {
        let text = "Plain \\bfseries bold here.\n\n\\begin{center}\n\\itshape\nSlanted.\n\\end{center}\n\n{\\Large big} and {\\small\\bf tiny}.\n";
        let d = doc(text);
        let end = Some(text.len());
        let style_of = |line: usize, word: &str| {
            let v = shown(&d, line, end);
            v.runs
                .iter()
                .find(|r| r.text.contains(word) && !r.text.contains('\\'))
                .map(|r| r.style)
                .unwrap_or_else(|| panic!("{word}: {:?}", v.runs))
        };
        assert!(style_of(0, "bold").bold && !style_of(0, "Plain").bold);
        assert!(style_of(4, "Slanted").italic);
        assert_eq!(style_of(7, "big").rich.size, Some(172));
        let small = style_of(7, "tiny");
        assert!(small.bold && small.rich.size == Some(108), "{small:?}");
        assert_eq!(style_of(7, "and").rich.size, None);
    }

    #[test]
    fn verb_star() {
        let text = "A \\verb*|a b| and \\verb|c d|.\n";
        let d = doc(text);
        assert_eq!(
            shown(&d, 0, Some(text.len())).display(),
            "A a\u{2423}b and c d."
        );
    }

    #[test]
    fn ligatures() {
        let text = "!`Hola! ?`Qu\\'e? a--b \\texttt{a--b ``c''} {\\tt x--y}\n";
        let d = doc(text);
        assert_eq!(
            shown(&d, 0, Some(text.len())).display(),
            // Declarations stay as source, dimmed.
            "\u{a1}Hola! \u{bf}Qué? a\u{2013}b a--b ``c'' {\\tt x--y}"
        );
    }

    #[test]
    fn siunitx_in_text() {
        let text = "\\usepackage{siunitx}\n\\begin{document}\ng is \\SI{9.81}{\\metre\\per\\second\\squared}, \\qty{50}{\\percent} of \\num{12345}.\n\\end{document}\n";
        let d = doc(text);
        assert_eq!(
            shown(&d, 2, Some(text.len())).display(),
            "g is 9.81\u{2009}m\u{2009}s⁻², 50\u{2009}% of 12\u{2009}345."
        );
    }

    #[test]
    fn enumerate_counter_set() {
        let text = "\\begin{enumerate}\n\\setcounter{enumi}{4}\n\\item five\n\\item six\n\\end{enumerate}\n";
        let d = doc(text);
        let end = Some(text.len());
        assert!(
            shown(&d, 2, end).display().starts_with("5."),
            "{}",
            shown(&d, 2, end).display()
        );
        assert!(shown(&d, 3, end).display().starts_with("6."));
    }

    #[test]
    fn theorems() {
        let text = "\\newtheorem{thm}{Theorem}[section]\n\\section{A}\n\\begin{thm}[Pythagoras]\nText.\n\\end{thm}\n\\begin{proof}\nEasy.\n\\end{proof}\n\\paragraph{Run} in.\n";
        let d = doc(text);
        let end = Some(text.len());
        // LaTeX's own `\newtheorem`: no period after the head.
        assert_eq!(shown(&d, 2, end).display(), "Theorem 1.1 (Pythagoras) ");
        assert_eq!(shown(&d, 4, end).role, crate::view::LineRole::Delimiter);
        assert_eq!(shown(&d, 5, end).display(), "Proof. ");
        assert_eq!(shown(&d, 7, end).display(), "\u{220e}");
        let run = shown(&d, 8, end);
        assert_eq!((run.display().as_str(), run.heading), ("Run in.", 0));
        assert!(run.runs[0].style.bold);
        // amsthm: a period; a proof's note replaces "Proof".
        let text = "\\usepackage{amsthm}\n\\newtheorem{thm}{Theorem}\n\\begin{thm}[P]\nT.\n\\end{thm}\n\\begin{proof}[Sketch]\nE.\n\\end{proof}\n";
        let d = doc(text);
        let end = Some(text.len());
        assert_eq!(shown(&d, 2, end).display(), "Theorem 1 (P). ");
        assert_eq!(shown(&d, 5, end).display(), "Sketch. ");
    }

    #[test]
    fn code_and_skipped_text() {
        let text = "\\begin{lstlisting}[language=Python]\ndef f():\n    pass\n\\end{lstlisting}\n\\iffalse\nhidden \\ifx a \\fi text\n\\fi\nshown\n\\begin{comment}\nnote\n\\end{comment}\n";
        let d = doc(text);
        let end = Some(text.len());
        assert_eq!(shown(&d, 0, end).role, crate::view::LineRole::Delimiter);
        let code = shown(&d, 1, end);
        assert!(code.mono && code.display() == "def f():");
        assert_eq!(shown(&d, 3, end).role, crate::view::LineRole::Delimiter);
        assert!(shown(&d, 5, end).runs.iter().all(|r| r.style.dim));
        assert!(shown(&d, 7, end).runs.iter().all(|r| !r.style.dim));
        assert!(shown(&d, 9, end).runs.iter().all(|r| r.style.dim));
        let b = blocks(&d);
        assert_eq!(
            b[0].kind,
            crate::view::BlockKind::Code {
                language: Some("python".into())
            }
        );
    }

    #[test]
    fn roots_across_the_project() {
        // A main document in a sibling folder, and `latex.root` (T2.7h.4).
        let dir = std::env::temp_dir().join(format!("kalem-latex-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("chapters")).unwrap();
        std::fs::write(dir.join(".kalem"), "").unwrap();
        std::fs::write(
            dir.join("src/main.tex"),
            "\\documentclass{book}\n\\begin{document}\n\\input{../chapters/one}\n\\end{document}\n",
        )
        .unwrap();
        let one = dir.join("chapters/one.tex");
        std::fs::write(&one, "\\chapter{One}\n").unwrap();
        let found = find_root(&one, "\\chapter{One}\n");
        assert_eq!(
            dunce::canonicalize(found).unwrap(),
            dunce::canonicalize(dir.join("src/main.tex")).unwrap()
        );
        // A file no document includes: the setting names its root.
        let lone = dir.join("chapters/lone.tex");
        std::fs::write(&lone, "\\section{Lone}\n").unwrap();
        assert_eq!(find_root(&lone, "\\section{Lone}\n"), lone);
        set_root_setting("src/main.tex");
        let named = find_root(&lone, "\\section{Lone}\n");
        set_root_setting("");
        assert_eq!(
            dunce::canonicalize(named).unwrap(),
            dunce::canonicalize(dir.join("src/main.tex")).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn outline() {
        let d = doc("\\documentclass{book}\n\\chapter{A}\n\\section{B}\n\\section*{C}\n");
        let items = outline_items(&d).unwrap();
        let v: Vec<(usize, &str)> = items.iter().map(|i| (i.level, i.title.as_str())).collect();
        assert_eq!(v, [(1, "1\u{2003}A"), (2, "1.1\u{2003}B"), (2, "C")]);
    }

    #[test]
    fn follows_edits() {
        let mut d = doc("\\section{A}\n\\section{B}\n");
        let tx = {
            let mut tx = org_edit::Transaction::new("t");
            tx.replace(0..0, "\\section{New}\n").unwrap();
            tx
        };
        d.apply(
            &tx,
            org_edit::ChangeKind::Command,
            std::time::Instant::now(),
        );
        assert_eq!(shown(&d, 2, None).display(), "3\u{2003}B");
        assert_eq!(
            d.latex().unwrap().parse(),
            &latex_syntax::parse(d.text().as_str())
        );
    }
}
