//! The document model of a LaTeX file (design §9.5, T2.7h.3): the class
//! and packages, the preamble and the body, the sectioning tree with the
//! numbers LaTeX gives it, labels with what they point at, references,
//! citations, floats and their captions, equations, theorems, footnotes,
//! the macros and environments defined, and the bibliography sources.
//!
//! Numbers follow the standard classes (`article`, `report`, `book` and
//! the classes built like them): `secnumdepth` (3 for articles, 2 for
//! reports and books) and `\setcounter`, the starred forms, `\appendix`,
//! `\frontmatter`, `\mainmatter` and `\backmatter`, counters within
//! chapters in reports and books and `\numberwithin`, one equation number
//! a line in `align` and its kin unless `\nonumber`, `\notag` or `\tag`
//! (an empty line after a final `\\` too, as amsmath numbers it),
//! `subequations`, and theorem counters shared or within a section as
//! `\newtheorem` says.
//!
//! [`Cache`] keeps what each paragraph says between edits: after an edit
//! only the changed paragraphs are read again, and numbering runs over
//! what they said.

mod extract;
pub mod project;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use extract::{Event, Item};

/// The document class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentClass {
    /// `article`, `book`, ….
    pub name: String,
    /// Its options.
    pub options: Vec<String>,
    /// The `\documentclass` command.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A package loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Its name.
    pub name: String,
    /// The options given.
    pub options: Vec<String>,
    /// The `\usepackage` command.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A sectioning command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// -1 for `\part`, 0 for `\chapter`, 1 for `\section`, … 5 for
    /// `\subparagraph`.
    pub level: i8,
    /// The command's name.
    pub command: String,
    /// `\section*`.
    pub starred: bool,
    /// The title, as written.
    pub title: String,
    /// The short title (`\section[short]{…}`).
    pub short: Option<String>,
    /// The number LaTeX gives it, if any.
    pub number: Option<String>,
    /// The command.
    pub range: Range<usize>,
    /// The section it is in.
    pub parent: Option<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// What a label points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A section, at this level.
    Section(i8),
    /// An equation.
    Equation,
    /// A float's caption: `figure`, `table`.
    Float(String),
    /// A theorem-like environment.
    Theorem(String),
    /// A footnote.
    Footnote,
    /// Nothing numbered before it.
    None,
}

/// A `\label`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The key.
    pub name: String,
    /// The command.
    pub range: Range<usize>,
    /// The number `\ref` prints.
    pub number: Option<String>,
    /// What it points at.
    pub target: Target,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A reference: `\ref`, `\eqref`, `\cref`, ….
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The command's name.
    pub command: String,
    /// The keys.
    pub keys: Vec<String>,
    /// The command.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A citation: `\cite` and its kin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// The command's name.
    pub command: String,
    /// The keys.
    pub keys: Vec<String>,
    /// The optional arguments (the pre- and postnote).
    pub notes: Vec<String>,
    /// The command.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A caption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caption {
    /// The text.
    pub text: String,
    /// The short caption.
    pub short: Option<String>,
    /// Its number.
    pub number: Option<String>,
    /// The command.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A float: `figure`, `table` and their starred forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Float {
    /// `figure` or `table`.
    pub kind: String,
    /// The environment.
    pub env: String,
    /// The environment's range.
    pub range: Range<usize>,
    /// Its captions.
    pub captions: Vec<Caption>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// An equation: a numbered environment, or a line of one of the
/// environments numbered by line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Equation {
    /// The environment.
    pub env: String,
    /// The text of the equation (the line).
    pub range: Range<usize>,
    /// Its number, or `\tag`.
    pub number: Option<String>,
    /// The number is a `\tag`.
    pub tag: bool,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A theorem-like environment declared by `\newtheorem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheoremKind {
    /// The environment.
    pub env: String,
    /// The title printed (`Theorem`).
    pub title: String,
    /// The counter (shared with another theorem, or its own).
    pub counter: String,
    /// Whether it is numbered.
    pub numbered: bool,
}

/// A theorem-like environment in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theorem {
    /// The environment.
    pub env: String,
    /// The title printed.
    pub title: String,
    /// `[note]`.
    pub note: Option<String>,
    /// Its number.
    pub number: Option<String>,
    /// The environment's range.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A footnote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footnote {
    /// Its mark.
    pub number: String,
    /// The command.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A macro defined: `\newcommand`, `\renewcommand`, `\providecommand`,
/// `\def`, `\DeclareMathOperator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Macro {
    /// The macro, with its backslash.
    pub name: String,
    /// The defining command.
    pub command: String,
    /// The number of arguments.
    pub args: usize,
    /// The default of the first, optional, argument.
    pub default: Option<String>,
    /// The definition.
    pub body: String,
    /// The defining command's range.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// An environment defined by `\newenvironment` or `\renewenvironment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEnvironment {
    /// Its name.
    pub name: String,
    /// The number of arguments.
    pub args: usize,
    /// The default of the first, optional, argument.
    pub default: Option<String>,
    /// The code at `\begin`.
    pub begin: String,
    /// The code at `\end`.
    pub end: String,
    /// The defining command's range.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// A bibliography source: `\bibliography{a,b}` or `\addbibresource`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibliographySource {
    /// The command's name.
    pub command: String,
    /// The files, with `.bib`.
    pub files: Vec<String>,
    /// The command.
    pub range: Range<usize>,
    /// The file it is in: 0 for the document, an index into
    /// [`Model::files`] for the files it includes.
    pub file: usize,
}

/// An `\input`, `\include`, `\subfile`, `\import` or `\subimport`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    /// The command's name.
    pub command: String,
    /// The file as written (`\import`'s folder and file joined).
    pub target: String,
    /// The command.
    pub range: std::ops::Range<usize>,
    /// The file it is in.
    pub file: usize,
    /// The file it includes, in [`Model::files`], when found.
    pub resolved: Option<usize>,
    /// Left out by `\includeonly`.
    pub excluded: bool,
}

/// The model of a LaTeX document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Model {
    /// `\documentclass`.
    pub class: Option<DocumentClass>,
    /// The packages loaded, in order.
    pub packages: Vec<Package>,
    /// Before `\begin{document}` (everything, without one).
    pub preamble: Range<usize>,
    /// The body of the `document` environment.
    pub body: Option<Range<usize>>,
    /// The sectioning commands, in order, each with its parent.
    pub sections: Vec<Section>,
    /// The labels.
    pub labels: Vec<Label>,
    /// The references.
    pub references: Vec<Reference>,
    /// The citations.
    pub citations: Vec<Citation>,
    /// The floats.
    pub floats: Vec<Float>,
    /// The equations.
    pub equations: Vec<Equation>,
    /// The theorem-like environments declared.
    pub theorem_kinds: Vec<TheoremKind>,
    /// The theorem-like environments in the text.
    pub theorems: Vec<Theorem>,
    /// The footnotes.
    pub footnotes: Vec<Footnote>,
    /// The macros defined.
    pub macros: Vec<Macro>,
    /// The environments defined.
    pub environments: Vec<NewEnvironment>,
    /// The bibliography sources.
    pub bibliography: Vec<BibliographySource>,
    /// `\bibliographystyle`.
    pub bibliography_style: Option<String>,
    /// The files included, in order.
    pub includes: Vec<Include>,
    /// `\includeonly`.
    pub include_only: Option<Vec<String>>,
    /// `\graphicspath`.
    pub graphics_paths: Vec<String>,
    /// The files of a project: the root document first, then the files it
    /// includes, which the `file` fields index (empty for one file).
    pub files: Vec<std::path::PathBuf>,
}

impl Model {
    /// The model of a parsed document, without a cache.
    pub fn new(parse: &latex_syntax::Parse) -> Model {
        Cache::default().model(parse).as_ref().clone()
    }

    /// The label named `name`.
    pub fn label(&self, name: &str) -> Option<&Label> {
        self.labels.iter().find(|l| l.name == name)
    }

    /// The definitions of the macros, as written, for the math renderer
    /// (as `#+LATEX_HEADER` lines are for Org).
    pub fn macro_definitions<'a>(&self, text: &'a str) -> Vec<&'a str> {
        self.macros
            .iter()
            .filter_map(|m| text.get(m.range.clone()))
            .collect()
    }
}

/// The model of the last version of a document, and what each paragraph
/// said, for the next.
#[derive(Debug, Default)]
pub struct Cache {
    events: extract::Cache,
    last: Option<(rowan::GreenNode, Arc<Model>)>,
}

impl Cache {
    /// The model of `parse`.
    pub fn model(&mut self, parse: &latex_syntax::Parse) -> Arc<Model> {
        // The same tree: the same node (not a deep comparison).
        if let Some((g, m)) = &self.last
            && std::ptr::eq::<rowan::GreenNodeData>(&**g, &**parse.green())
        {
            return m.clone();
        }
        let root = parse.syntax();
        let items = self.events.document(&root);
        let len = usize::from(root.text_range().end());
        let m = Arc::new(number(&items, len, None));
        self.last = Some((parse.green().clone(), m.clone()));
        m
    }
}

/// Numbers the events of a document, reading included files through
/// `resolver`.
pub(crate) fn number<'r>(
    items: &[Item],
    len: usize,
    resolver: Option<&'r mut Resolver<'r>>,
) -> Model {
    let mut n = Numbering::new(len);
    n.resolver = resolver;
    n.run(items, 0);
    n.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassKind {
    Article,
    Report,
    Book,
}

fn class_kind(name: &str) -> ClassKind {
    match name {
        "book" | "amsbook" | "scrbook" | "memoir" | "extbook" => ClassKind::Book,
        "report" | "scrreprt" | "extreport" => ClassKind::Report,
        _ => ClassKind::Article,
    }
}

const SECTION_COUNTERS: [&str; 7] = [
    "part",
    "chapter",
    "section",
    "subsection",
    "subsubsection",
    "paragraph",
    "subparagraph",
];

fn roman(mut n: i64, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let table = [
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
    ];
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper { s.to_uppercase() } else { s }
}

fn alph(n: i64, upper: bool) -> String {
    if !(1..=26).contains(&n) {
        return n.to_string();
    }
    let c = (b'a' + (n - 1) as u8) as char;
    if upper {
        c.to_ascii_uppercase().to_string()
    } else {
        c.to_string()
    }
}

/// An equation environment being read.
#[derive(Debug)]
struct EqEnv {
    name: String,
    by_line: bool,
    numbered: bool,
    body_end: usize,
    line_start: usize,
    nonumber: bool,
    tag: Option<String>,
    labels: Vec<usize>,
    lines: usize,
}

/// Finds an included file: from the file being read, the command and its
/// arguments, the file's index and what it says.
pub(crate) type Resolver<'r> =
    dyn FnMut(usize, &str, &[String]) -> Option<(usize, Arc<Vec<Item>>)> + 'r;

/// Numbering over the events, in order.
struct Numbering<'r> {
    resolver: Option<&'r mut Resolver<'r>>,
    /// The files being read, innermost last (for cycles).
    active: Vec<usize>,
    /// In a file included by `\subfile`, outside its `document`
    /// environment: skipped, as the subfiles package skips it.
    skipping: bool,
    /// How deep in `document` environments of subfiles.
    subfile_document: Option<usize>,
    model: Model,
    class: ClassKind,
    secnumdepth: Option<i64>,
    counters: HashMap<String, i64>,
    within: HashMap<String, String>,
    appendix: bool,
    mainmatter: bool,
    current: (Option<String>, Target),
    envs: Vec<String>,
    eq: Vec<EqEnv>,
    subequations: Option<(String, i64)>,
    section_stack: Vec<(i8, usize)>,
    floats: Vec<usize>,
    sub_captions: i64,
    saved: Vec<(Option<String>, Target)>,
    /// The file being read.
    file: usize,
    len: usize,
}

impl<'r> Numbering<'r> {
    fn new(len: usize) -> Numbering<'r> {
        Numbering {
            resolver: None,
            active: vec![0],
            skipping: false,
            subfile_document: None,
            model: Model {
                preamble: 0..len,
                ..Model::default()
            },
            class: ClassKind::Article,
            secnumdepth: None,
            counters: HashMap::new(),
            within: HashMap::new(),
            appendix: false,
            mainmatter: true,
            current: (None, Target::None),
            envs: Vec::new(),
            eq: Vec::new(),
            subequations: None,
            section_stack: Vec::new(),
            floats: Vec::new(),
            sub_captions: 0,
            saved: Vec::new(),
            file: 0,
            len,
        }
    }

    fn finish(self) -> Model {
        let _ = self.len;
        self.model
    }

    fn set_class(&mut self, kind: ClassKind) {
        self.class = kind;
        self.within.clear();
        let chaptered = kind != ClassKind::Article;
        for w in SECTION_COUNTERS.windows(2).skip(1) {
            if w[0] == "chapter" && !chaptered {
                continue;
            }
            self.within.insert(w[1].into(), w[0].into());
        }
        if chaptered {
            for c in ["equation", "figure", "table", "footnote"] {
                self.within.insert(c.into(), "chapter".into());
            }
        }
    }

    fn secnumdepth(&self) -> i64 {
        self.secnumdepth.unwrap_or(match self.class {
            ClassKind::Article => 3,
            _ => 2,
        })
    }

    fn get(&self, c: &str) -> i64 {
        self.counters.get(c).copied().unwrap_or(0)
    }

    /// `\stepcounter`: the counter up, the counters within it reset.
    fn step(&mut self, c: &str) {
        *self.counters.entry(c.to_string()).or_insert(0) += 1;
        let reset: Vec<String> = self
            .within
            .iter()
            .filter(|(_, w)| *w == c)
            .map(|(k, _)| k.clone())
            .collect();
        for r in reset {
            self.counters.insert(r, 0);
        }
    }

    /// `\the<counter>`.
    fn the(&self, c: &str) -> String {
        let n = self.get(c);
        match c {
            "part" => roman(n, true),
            "chapter" if self.appendix => alph(n, true),
            "section" if self.appendix && self.class == ClassKind::Article => alph(n, true),
            "footnote" => n.to_string(),
            _ => match self.within.get(c) {
                Some(w) => format!("{}.{n}", self.the(w)),
                None => n.to_string(),
            },
        }
    }

    fn run(&mut self, items: &[Item], base: usize) {
        for item in items {
            match item {
                Item::Nested(off, inner) => self.run(inner, base + off),
                Item::Event(e) => self.event(e, base),
            }
        }
    }

    fn event(&mut self, e: &Event, base: usize) {
        let at = |r: &Range<usize>| r.start + base..r.end + base;
        if self.skipping {
            if let Event::EnvEnter { name, .. } = e
                && name == "document"
            {
                self.skipping = false;
                self.subfile_document = Some(self.envs.len());
                self.envs.push(name.clone());
                self.saved.push(self.current.clone());
            }
            return;
        }
        if self.file != 0 {
            match e {
                // An included file's class and document are the root's.
                Event::Class { .. } => return,
                Event::EnvExit
                    if self.subfile_document == Some(self.envs.len().saturating_sub(1)) =>
                {
                    self.envs.pop();
                    self.saved.pop();
                    self.subfile_document = None;
                    self.skipping = true;
                    return;
                }
                _ => {}
            }
        }
        match e {
            Event::Class {
                name,
                options,
                range,
            } => {
                self.set_class(class_kind(name));
                self.model.class = Some(DocumentClass {
                    name: name.clone(),
                    options: options.clone(),
                    range: at(range),
                    file: self.file,
                });
            }
            Event::Package {
                name,
                options,
                range,
            } => self.model.packages.push(Package {
                name: name.clone(),
                options: options.clone(),
                range: at(range),
                file: self.file,
            }),
            Event::Section {
                level,
                command,
                starred,
                title,
                short,
                range,
            } => {
                let number = if *starred {
                    None
                } else {
                    let depth = self.secnumdepth();
                    let steps = match level {
                        -1 => depth > -2,
                        0 => depth > -1 && self.mainmatter,
                        l => i64::from(*l) <= depth,
                    };
                    steps.then(|| {
                        let c = SECTION_COUNTERS[(*level + 1) as usize];
                        self.step(c);
                        let n = self.the(c);
                        self.current = (Some(n.clone()), Target::Section(*level));
                        n
                    })
                };
                while self.section_stack.last().is_some_and(|&(l, _)| l >= *level) {
                    self.section_stack.pop();
                }
                let parent = self.section_stack.last().map(|&(_, i)| i);
                self.section_stack.push((*level, self.model.sections.len()));
                self.model.sections.push(Section {
                    level: *level,
                    command: command.clone(),
                    starred: *starred,
                    title: title.clone(),
                    short: short.clone(),
                    number,
                    range: at(range),
                    file: self.file,
                    parent,
                });
            }
            Event::Appendix => {
                self.appendix = true;
                if self.class == ClassKind::Article {
                    self.counters.insert("section".into(), 0);
                    self.counters.insert("subsection".into(), 0);
                } else {
                    self.counters.insert("chapter".into(), 0);
                    self.counters.insert("section".into(), 0);
                }
            }
            Event::FrontMatter | Event::BackMatter => self.mainmatter = false,
            Event::MainMatter => self.mainmatter = true,
            Event::Label { name, range } => {
                let index = self.model.labels.len();
                let (number, target) = self.current.clone();
                self.model.labels.push(Label {
                    name: name.clone(),
                    range: at(range),
                    file: self.file,
                    number,
                    target,
                });
                if let Some(eq) = self.eq.last_mut() {
                    eq.labels.push(index);
                }
            }
            Event::Ref {
                command,
                keys,
                range,
            } => self.model.references.push(Reference {
                command: command.clone(),
                keys: keys.clone(),
                range: at(range),
                file: self.file,
            }),
            Event::Cite {
                command,
                keys,
                notes,
                range,
            } => self.model.citations.push(Citation {
                command: command.clone(),
                keys: keys.clone(),
                notes: notes.clone(),
                range: at(range),
                file: self.file,
            }),
            Event::Caption { text, short, range } => {
                let env = self
                    .envs
                    .iter()
                    .rev()
                    .find(|e| float_kind(e).is_some())
                    .cloned();
                let kind = env.as_deref().and_then(float_kind);
                let number = match (kind, env) {
                    // In `subfigure` (subcaption): (a), (b), … in the float.
                    (Some(k), Some(env)) if env.starts_with("sub") => {
                        self.sub_captions += 1;
                        let n = alph(self.sub_captions, false);
                        self.current = (Some(n.clone()), Target::Float(k.to_string()));
                        Some(n)
                    }
                    (Some(k), _) => {
                        self.step(k);
                        let n = self.the(k);
                        self.current = (Some(n.clone()), Target::Float(k.to_string()));
                        Some(n)
                    }
                    _ => None,
                };
                let caption = Caption {
                    text: text.clone(),
                    short: short.clone(),
                    number,
                    range: at(range),
                    file: self.file,
                };
                if let Some(&f) = self.floats.last() {
                    self.model.floats[f].captions.push(caption);
                }
            }
            Event::Footnote { explicit, range } => {
                let number = match explicit {
                    Some(n) => n.clone(),
                    None => {
                        self.step("footnote");
                        self.the("footnote")
                    }
                };
                self.saved.push(self.current.clone());
                self.current = (Some(number.clone()), Target::Footnote);
                self.model.footnotes.push(Footnote {
                    number,
                    range: at(range),
                    file: self.file,
                });
            }
            Event::Macro {
                name,
                command,
                args,
                default,
                body,
                range,
            } => self.model.macros.push(Macro {
                name: name.clone(),
                command: command.clone(),
                args: *args,
                default: default.clone(),
                body: body.clone(),
                range: at(range),
                file: self.file,
            }),
            Event::NewEnvironment {
                name,
                args,
                default,
                begin,
                end,
                range,
            } => self.model.environments.push(NewEnvironment {
                name: name.clone(),
                args: *args,
                default: default.clone(),
                begin: begin.clone(),
                end: end.clone(),
                range: at(range),
                file: self.file,
            }),
            Event::TheoremDef {
                env,
                title,
                shared,
                within,
                numbered,
            } => {
                let counter = shared.clone().unwrap_or_else(|| env.clone());
                if let (None, Some(w)) = (shared, within) {
                    self.within.insert(counter.clone(), w.clone());
                }
                self.model.theorem_kinds.push(TheoremKind {
                    env: env.clone(),
                    title: title.clone(),
                    counter,
                    numbered: *numbered,
                });
            }
            Event::Bibliography {
                command,
                files,
                range,
            } => self.model.bibliography.push(BibliographySource {
                command: command.clone(),
                files: files.clone(),
                range: at(range),
                file: self.file,
            }),
            Event::BibliographyStyle(s) => self.model.bibliography_style = Some(s.clone()),
            Event::SetCounter {
                counter,
                value,
                add,
            } => {
                if counter == "secnumdepth" {
                    let old = self.secnumdepth();
                    self.secnumdepth = Some(if *add { old + value } else { *value });
                } else {
                    let v = self.counters.entry(counter.clone()).or_insert(0);
                    *v = if *add { *v + value } else { *value };
                }
            }
            Event::NumberWithin { counter, within } => match within {
                Some(w) => {
                    self.within.insert(counter.clone(), w.clone());
                }
                None => {
                    self.within.remove(counter);
                }
            },
            Event::EnvEnter {
                name,
                range,
                body,
                note,
            } => self.enter(name, at(range), at(body), note),
            Event::FootnoteEnd => {
                if let Some(c) = self.saved.pop() {
                    self.current = c;
                }
            }
            Event::EnvExit => self.exit(),
            Event::Include {
                command,
                args,
                range,
            } => self.include(command, args, at(range)),
            Event::IncludeOnly(files) => self.model.include_only = Some(files.clone()),
            Event::GraphicsPath(dirs) => self.model.graphics_paths.extend(dirs.iter().cloned()),
            Event::LineBreak { at: pos } => {
                if self
                    .eq
                    .last()
                    .is_some_and(|e| e.by_line && self.envs.last() == Some(&e.name))
                {
                    self.end_line(pos + base);
                }
            }
            Event::NoNumber => {
                if let Some(e) = self.eq.last_mut() {
                    e.nonumber = true;
                }
            }
            Event::Tag { text } => {
                if let Some(e) = self.eq.last_mut() {
                    e.tag = Some(text.clone());
                }
            }
        }
    }

    fn include(&mut self, command: &str, args: &[String], range: Range<usize>) {
        let target = args.concat();
        let excluded = command == "include"
            && self
                .model
                .include_only
                .as_ref()
                .is_some_and(|only| !only.contains(&target));
        let from = self.file;
        let found = match self.resolver.as_mut() {
            Some(r) if self.active.len() < 32 => r(from, command, args),
            _ => None,
        };
        let found = found.filter(|(id, _)| !self.active.contains(id));
        self.model.includes.push(Include {
            command: command.to_string(),
            target,
            range,
            file: from,
            resolved: found.as_ref().map(|(id, _)| *id),
            excluded,
        });
        let Some((id, items)) = found else { return };
        // Read in place, numbered with the rest (an excluded `\include`
        // keeps its numbers too: LaTeX reads them from its `.aux`).
        let (skipping, subfile_document) = (self.skipping, self.subfile_document);
        self.skipping = command == "subfile";
        self.subfile_document = None;
        self.active.push(id);
        self.file = id;
        self.run(&items, 0);
        self.active.pop();
        self.file = from;
        self.skipping = skipping;
        self.subfile_document = subfile_document;
    }

    fn enter(
        &mut self,
        name: &str,
        range: Range<usize>,
        body: Range<usize>,
        note: &Option<String>,
    ) {
        self.envs.push(name.to_string());
        // An environment is a group: what `\label` would point at is
        // restored at its end.
        self.saved.push(self.current.clone());
        if name == "document" && self.file == 0 {
            self.model.preamble = 0..range.start;
            self.model.body = Some(body.clone());
        }
        if let Some(kind) = float_kind(name)
            && !name.starts_with("sub")
        {
            self.floats.push(self.model.floats.len());
            self.sub_captions = 0;
            self.model.floats.push(Float {
                kind: kind.to_string(),
                env: name.to_string(),
                range: range.clone(),
                captions: Vec::new(),
                file: self.file,
            });
        }
        if name == "subequations" {
            self.step("equation");
            let parent = self.the("equation");
            self.current = (Some(parent.clone()), Target::Equation);
            self.subequations = Some((parent, 0));
        }
        let base = name.trim_end_matches('*');
        let by_line = matches!(
            base,
            "align" | "gather" | "flalign" | "alignat" | "eqnarray"
        );
        if by_line || matches!(base, "equation" | "multline") {
            self.eq.push(EqEnv {
                name: name.to_string(),
                by_line,
                numbered: !name.ends_with('*'),
                body_end: body.end,
                line_start: body.start,
                nonumber: false,
                tag: None,
                labels: Vec::new(),
                lines: 0,
            });
        }
        if let Some(kind) = self
            .model
            .theorem_kinds
            .iter()
            .rev()
            .find(|k| k.env == name)
            .cloned()
        {
            let number = kind.numbered.then(|| {
                self.step(&kind.counter);
                let n = self.the(&kind.counter);
                self.current = (Some(n.clone()), Target::Theorem(name.to_string()));
                n
            });
            self.model.theorems.push(Theorem {
                env: name.to_string(),
                title: kind.title,
                note: note.clone(),
                number,
                range,
                file: self.file,
            });
        }
    }

    fn exit(&mut self) {
        let Some(name) = self.envs.pop() else { return };
        let restore = self.saved.pop();
        if self.eq.last().is_some_and(|e| e.name == name) {
            let end = self.eq.last().map_or(0, |e| e.body_end);
            self.end_line(end);
            self.eq.pop();
        }
        if name == "subequations" {
            self.subequations = None;
        }
        if float_kind(&name).is_some() && !name.starts_with("sub") {
            self.floats.pop();
        }
        if let Some(c) = restore {
            self.current = c;
        }
    }

    /// The end of an equation (a line, or the environment at `last`).
    fn end_line(&mut self, end: usize) {
        let Some(e) = self.eq.last_mut() else { return };
        let (tag, nonumber, numbered) = (e.tag.take(), e.nonumber, e.numbered);
        let labels = std::mem::take(&mut e.labels);
        let range = e.line_start..end;
        let env = e.name.clone();
        e.nonumber = false;
        e.lines += 1;
        e.line_start = end;
        let (number, is_tag) = match tag {
            Some(t) => (Some(t), true),
            None if numbered && !nonumber => {
                let n = match &mut self.subequations {
                    Some((parent, letter)) => {
                        *letter += 1;
                        format!("{parent}{}", alph(*letter, false))
                    }
                    None => {
                        self.step("equation");
                        self.the("equation")
                    }
                };
                (Some(n), false)
            }
            None => (None, false),
        };
        if let Some(n) = &number {
            self.current = (Some(n.clone()), Target::Equation);
            for l in labels {
                self.model.labels[l].number = Some(n.clone());
                self.model.labels[l].target = Target::Equation;
            }
        }
        self.model.equations.push(Equation {
            env,
            range,
            number,
            tag: is_tag,
            file: self.file,
        });
    }
}

/// The counter of the captions in environment `env`.
fn float_kind(env: &str) -> Option<&'static str> {
    match env.trim_end_matches('*') {
        "figure" | "wrapfigure" | "subfigure" => Some("figure"),
        "table" | "wraptable" | "longtable" | "subtable" => Some("table"),
        _ => None,
    }
}
