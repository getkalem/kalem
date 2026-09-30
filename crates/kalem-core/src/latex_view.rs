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
    files: RefCell<std::collections::HashMap<std::path::PathBuf, (std::time::SystemTime, String)>>,
}

/// The disk, with one file's text as edited.
struct Overlay<'a> {
    disk: &'a DiskCache,
    path: &'a std::path::Path,
    text: &'a str,
}

impl latex_model::project::Files for Overlay<'_> {
    fn read(&self, path: &std::path::Path) -> Option<String> {
        if path == self.path {
            return Some(self.text.to_string());
        }
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let mut files = self.disk.files.borrow_mut();
        if let Some((t, text)) = files.get(path)
            && *t == modified
        {
            return Some(text.clone());
        }
        let text = std::fs::read_to_string(path).ok()?;
        files.insert(path.to_path_buf(), (modified, text.clone()));
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
        own: &latex_model::Model,
    ) -> Option<Arc<latex_model::Model>> {
        // The root document that includes nothing needs no project.
        if self.root == self.path && own.includes.is_empty() {
            return None;
        }
        if let Some((g, m)) = &self.last
            && same(g, parse.green())
        {
            return Some(m.clone());
        }
        let text = parse.syntax().text().to_string();
        self.cache.set_parse(&self.path, &text, parse.clone());
        let files = Overlay {
            disk: &self.disk,
            path: &self.path,
            text: &text,
        };
        let project = self.cache.load(&self.root, &files);
        let this = project.model.files.iter().position(|f| *f == self.path)?;
        let mut m = project.model.seen_from(this);
        // The document's own preamble and body.
        m.preamble = own.preamble.clone();
        m.body = own.body.clone();
        let m = Arc::new(m);
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
            models: RefCell::new(latex_model::Cache::default()),
            titles: RefCell::new(None),
            skipped: RefCell::new(None),
            lists: RefCell::new(std::collections::HashMap::new()),
            diagnostics: crate::latex_check::Live::default(),
            project: RefCell::new(None),
            root: None,
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
    }

    /// The parse.
    pub fn parse(&self) -> &latex_syntax::Parse {
        &self.parse
    }

    /// The document model (numbers, labels, citations, definitions): in
    /// a project of several files, the project's, seen from this file
    /// (its numbers continue the files before it, and labels in the other
    /// files resolve).
    pub fn model(&self) -> Arc<latex_model::Model> {
        let own = self.models.borrow_mut().model(&self.parse);
        let mut project = self.project.borrow_mut();
        match project.as_mut().and_then(|p| p.model(&self.parse, &own)) {
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
            let root =
                latex_model::project::find_root(&p, &t, &latex_model::project::Disk, None, None);
            let _ = tx.send(root);
        });
        self.root = Some((path.to_path_buf(), rx));
    }

    /// Takes the root document when it is found; `true` then.
    pub(crate) fn poll_project(&mut self) -> bool {
        let Some((path, rx)) = &self.root else {
            return false;
        };
        match rx.try_recv() {
            Ok(root) => {
                let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                let root = std::fs::canonicalize(&root).unwrap_or(root);
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
            if let Some(g) = n.children().find(|c| c.kind() == K::GROUP) {
                let s = group_text(&g).replace("\\\\", "\\and");
                let parts: Vec<String> = s
                    .split("\\and")
                    .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|p| !p.is_empty())
                    .collect();
                v[i] = Some(parts.join(", "));
            }
        }
        let v = Arc::new(v);
        *t = Some((self.parse.green().clone(), v.clone()));
        v
    }
}

fn is_list(name: &str) -> bool {
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
    if src.starts_with('[') {
        start = src.find(']')? + 1;
    }
    let open = src[start..].chars().next()?;
    let close = if open == '{' { '}' } else { open };
    let body = start + open.len_utf8();
    let end = body + src[body..].rfind(close).filter(|_| src.ends_with(close))?;
    Some(r.start + body..r.start + end)
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
    for cmd in env.descendants().filter(|c| c.kind() == K::COMMAND) {
        if latex_syntax::name(&cmd).as_deref() != Some("item") {
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

fn merge(a: &mut Style, b: &Style) {
    a.bold |= b.bold;
    a.italic |= b.italic;
    a.code |= b.code;
    a.underline |= b.underline;
    a.strike |= b.strike;
    a.superscript |= b.superscript;
    a.subscript |= b.subscript;
}

/// Commands whose arguments are text a reader reads (typography applies).
fn prose(name: &str) -> bool {
    format_style(name).is_some()
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
                | "thanks"
                | "enquote"
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
                let format = format_style(&name);
                if let Some(s) = &format {
                    merge(&mut c.style, s);
                }
                if section {
                    c.heading = true;
                }
                if name == "footnote" {
                    c.style.dim = true;
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
                        Some(o) if o.kind() == K::OPT_ARG => section,
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
        "\\ " => " ",
        "\\@" => "",
        "\\/" => "",
        _ => return None,
    })
}

/// What a command without arguments typesets.
fn word(name: &str) -> Option<&'static str> {
    Some(match name {
        "ldots" | "dots" | "textellipsis" => "\u{2026}",
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
    let mut v = unflagged_line_view(doc, line, cursor);
    if let Some(diags) = doc.latex_diagnostics() {
        flag(&mut v, diags);
    }
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
                    )
            })
            && node_span(d).end >= line.start + text[line.clone()].trim_end().len()
        {
            // An abstract's title, as the article class prints it.
            let ds = node_span(d);
            if d.kind() == K::BEGIN
                && d.parent().and_then(|e| latex_syntax::name(&e)).as_deref() == Some("abstract")
                && !near(&ds)
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
    while let Some(t) = tok {
        let r = span(&t);
        if r.start >= line.end {
            break;
        }
        tok = t.next_token();
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
                b.verbatim(code.clone(), code_style);
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
                    let head = match number {
                        Some(n) => format!("{title} {n}"),
                        None => title,
                    };
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
                    let tail = match &note {
                        Some(n) => format!(" ({n}). "),
                        None => ". ".to_string(),
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
                "\\centering" => {
                    while let Some(n) = &tok
                        && span(n).start < cs.end
                    {
                        tok = n.next_token();
                    }
                    continue;
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
                        let label = match number {
                            // In `subfigure`: `(a) `.
                            Some(n) if sub => format!("({n}) "),
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
            K::CONTROL_SYMBOL if !c.math && !near(&r) => {
                let is_break = s == "\\\\";
                match symbol(s) {
                    Some(rep) => b.replace(r, rep, c.style),
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
                match (name, word(name)) {
                    ("maketitle", _) => {
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
                    _ => {
                        let known = format_style(name).is_some()
                            || !latex_syntax::signatures::command(name).is_empty();
                        let mut st = c.style;
                        st.dim = !known;
                        b.verbatim(r, st);
                    }
                }
            }
            K::TILDE if !c.math && !near(&r) => b.replace(r, "\u{a0}", c.style),
            K::COMMENT => b.verbatim(r, dim),
            _ => b.verbatim(r, c.style),
        }
    }
    v.runs = b.runs;
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
    v.align = if title_line {
        crate::rich::Align::Center
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

/// What a reference names its target by (`\autoref`, `\cref`, `\Cref`).
fn target_name(model: &latex_model::Model, target: &latex_model::Target, command: &str) -> String {
    use latex_model::Target;
    let (long, short) = match target {
        Target::Section(-1) => ("Part", "part"),
        Target::Section(0) => ("Chapter", "chapter"),
        Target::Section(_) => ("Section", "section"),
        Target::Equation => ("Equation", "eq."),
        Target::Float(k) if k == "table" => ("Table", "table"),
        Target::Float(_) => ("Figure", "fig."),
        Target::Footnote => ("Footnote", "footnote"),
        Target::Theorem(env) => {
            let title = model
                .theorem_kinds
                .iter()
                .find(|k| k.env == *env)
                .map_or_else(|| env.clone(), |k| k.title.clone());
            return if command == "cref" {
                title.to_lowercase()
            } else {
                title
            };
        }
        Target::None => ("", ""),
    };
    if command == "cref" {
        short.to_string()
    } else {
        long.to_string()
    }
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
        "ref" | "eqref" | "pageref" | "autoref" | "cref" | "Cref" | "nameref" | "vref" | "Vref" => {
            let mut all = true;
            let parts: Vec<String> = first
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(|k| {
                    let Some(l) = model.label(k) else {
                        all = false;
                        return "??".to_string();
                    };
                    let n = l.number.clone().unwrap_or_default();
                    match name {
                        "eqref" => format!("({n})"),
                        "pageref" => k.to_string(),
                        "nameref" => model
                            .sections
                            .iter()
                            .rev()
                            .find(|s| s.range.start <= l.range.start && s.file == l.file)
                            .map_or(n, |s| s.title.clone()),
                        "autoref" | "cref" | "Cref" => {
                            let what = target_name(model, &l.target, name);
                            let n =
                                if l.target == latex_model::Target::Equation && name != "autoref" {
                                    format!("({n})")
                                } else {
                                    n
                                };
                            if what.is_empty() {
                                n
                            } else {
                                format!("{what}\u{a0}{n}")
                            }
                        }
                        _ => n,
                    }
                })
                .collect();
            (parts.join(", "), all)
        }
        _ => {
            // A citation: author and year from the bibliography.
            let base = doc.meta.path.as_deref().and_then(std::path::Path::parent);
            let files: Vec<std::path::PathBuf> = model
                .bibliography
                .iter()
                .flat_map(|b| b.files.iter())
                .map(|f| base.map_or_else(|| std::path::PathBuf::from(f), |d| d.join(f)))
                .collect();
            let bib = crate::cite::load(&files);
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
        let base = doc.meta.path.as_deref().and_then(std::path::Path::parent);
        let files: Vec<std::path::PathBuf> = model
            .bibliography
            .iter()
            .flat_map(|b| b.files.iter())
            .map(|f| base.map_or_else(|| std::path::PathBuf::from(f), |d| d.join(f)))
            .collect();
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
                let what = target_name(&model, &l.target, "Cref");
                let n = l.number.clone().unwrap_or_default();
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
    Some(
        model
            .sections
            .iter()
            .filter(|s| s.file == 0)
            .map(|s| crate::view::OutlineItem {
                level: (s.level - top + 1).max(1) as usize,
                todo: None,
                title: match &s.number {
                    Some(n) => format!("{n}\u{2003}{}", s.title),
                    None => s.title.clone(),
                },
                start: s.range.start,
            })
            .collect(),
    )
}

/// Whether the view renders command `name` (for the report of what a
/// document leaves as source).
pub fn renders_command(name: &str) -> bool {
    format_style(name).is_some()
        || latex_syntax::signatures::is_sectioning(name)
        || chip_command(name)
        || word(name).is_some()
        || matches!(
            name,
            "item"
                | "caption"
                | "includegraphics"
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
        )
}

/// Whether the view renders environment `name` (theorems are the ones
/// the model declares).
pub fn renders_environment(name: &str, model: &latex_model::Model) -> bool {
    is_list(name)
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
        )
        || model.theorem_kinds.iter().any(|k| k.env == name)
}

/// What a float is called in its caption.
fn float_name(kind: &str, turkish: bool) -> Option<&'static str> {
    Some(match (kind.trim_end_matches('*'), turkish) {
        ("figure" | "wrapfigure" | "subfigure", false) => "Figure",
        ("figure" | "wrapfigure" | "subfigure", true) => "\u{15e}ekil",
        ("table" | "wraptable" | "subtable", false) => "Table",
        ("table" | "wraptable" | "subtable", true) => "Tablo",
        _ => return None,
    })
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
    }?;
    Some(
        lang.trim_start_matches('[')
            .split(']')
            .next_back()
            .unwrap_or(&lang)
            .to_lowercase(),
    )
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
    )
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
    for c in node.descendants().filter(|c| c.kind() == K::COMMAND) {
        if matches!(
            latex_syntax::name(&c).as_deref(),
            Some("label" | "nonumber" | "notag")
        ) {
            edits.push((node_span(&c), String::new()));
        }
    }
    let name = (node.kind() == K::ENVIRONMENT)
        .then(|| latex_syntax::name(&node))
        .flatten();
    if let Some(name) = &name {
        let model = state.model();
        for e in model
            .equations
            .iter()
            .filter(|e| e.file == 0 && r.start <= e.range.start && e.range.end <= r.end && !e.tag)
        {
            if let Some(n) = &e.number {
                edits.push((e.range.end..e.range.end, format!("\\tag{{{n}}}")));
            }
        }
        // Starred, so that only the tags number it.
        if !name.ends_with('*') {
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
    Some(out)
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
    for n in nodes {
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
        };
        crate::DocumentState::new(text, meta, Arc::new(org_model::Settings::default()))
    }

    fn shown(d: &crate::DocumentState, line: usize, cursor: Option<usize>) -> LineView {
        let r = d.text().line_range(line);
        let r = r.start..r.end - usize::from(d.text().as_str()[r.clone()].ends_with('\n'));
        line_view(d, r, cursor)
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
            "See 1, (1), eq.\u{a0}(1), Section\u{a0}1, ??."
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
    fn theorems() {
        let text = "\\newtheorem{thm}{Theorem}[section]\n\\section{A}\n\\begin{thm}[Pythagoras]\nText.\n\\end{thm}\n\\begin{proof}\nEasy.\n\\end{proof}\n\\paragraph{Run} in.\n";
        let d = doc(text);
        let end = Some(text.len());
        assert_eq!(shown(&d, 2, end).display(), "Theorem 1.1 (Pythagoras). ");
        assert_eq!(shown(&d, 4, end).role, crate::view::LineRole::Delimiter);
        assert_eq!(shown(&d, 5, end).display(), "Proof. ");
        assert_eq!(shown(&d, 7, end).display(), "\u{220e}");
        let run = shown(&d, 8, end);
        assert_eq!((run.display().as_str(), run.heading), ("Run in.", 0));
        assert!(run.runs[0].style.bold);
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
