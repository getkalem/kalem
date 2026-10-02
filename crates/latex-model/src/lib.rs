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
    /// An item of an enumerated list.
    Item,
    /// A counter stepped by `\refstepcounter`.
    Counter(String),
    /// Nothing numbered before it.
    None,
}

/// An `\addcontentsline{toc}{…}{…}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentsLine {
    /// The sectioning level it is listed at.
    pub level: i8,
    /// Its title.
    pub title: String,
    /// How many of [`Model::sections`] come before it.
    pub after: usize,
    /// The command.
    pub range: Range<usize>,
    /// The file it is in.
    pub file: usize,
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

/// An entry of a document's own bibliography (`thebibliography`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibItem {
    /// The key `\cite` uses.
    pub key: String,
    /// What `\cite` prints: the optional argument, or the entry's number.
    pub label: String,
    /// The `\bibitem` command.
    pub range: Range<usize>,
    /// The file it is in (as [`Citation::file`]).
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
    /// `\setcounter{tocdepth}{n}`: the deepest level the table of
    /// contents lists, when the document sets it.
    pub toc_depth: Option<i64>,
    /// `\addcontentsline{toc}{level}{title}`: entries of the table of
    /// contents besides the sections, in document order.
    pub contents_lines: Vec<ContentsLine>,
    /// `\graphicspath`.
    pub graphics_paths: Vec<String>,
    /// The files of a project: the root document first, then the files it
    /// includes, which the `file` fields index (empty for one file).
    pub files: Vec<std::path::PathBuf>,
    /// Labels amsmath stops at ("Multiple \label's"): a second one while
    /// another waits for its line (indices into `labels`).
    pub label_clashes: Vec<usize>,
    /// Labels LaTeX never writes: on a line of `align*` or with
    /// `\nonumber` that no numbered line follows; a `\ref` to them
    /// prints "??" (indices into `labels`).
    pub unwritten_labels: Vec<usize>,
    /// Labels in a float before its caption: they refer to the section
    /// around, or with the caption package to nothing (`\ref` prints
    /// "??") (indices into `labels`).
    pub labels_before_caption: Vec<usize>,
    /// The entries of the document's own bibliography, in order.
    pub bib_items: Vec<BibItem>,
}

impl Model {
    /// The model of a parsed document, without a cache.
    pub fn new(parse: &latex_syntax::Parse) -> Model {
        Cache::default().model(parse).as_ref().clone()
    }

    /// The positions in file `file` moved by `region` (an edit that left
    /// its events as they were, moved); `false`, the model half moved,
    /// when one falls inside the region.
    pub(crate) fn move_positions(&mut self, file: usize, region: &extract::Region) -> bool {
        let f = |r: &mut Range<usize>| match (region.map(r.start), region.map(r.end)) {
            (Some(s), Some(e)) => {
                *r = s..e;
                true
            }
            _ => false,
        };
        let mut ok = true;
        if file == 0 {
            ok &= f(&mut self.preamble);
            if let Some(b) = &mut self.body {
                ok &= f(b);
            }
        }
        macro_rules! each {
            ($v:expr) => {
                for x in $v.iter_mut().filter(|x| x.file == file) {
                    ok &= f(&mut x.range);
                }
            };
        }
        if let Some(c) = self.class.as_mut().filter(|c| c.file == file) {
            ok &= f(&mut c.range);
        }
        each!(self.packages);
        each!(self.sections);
        each!(self.labels);
        each!(self.references);
        each!(self.citations);
        each!(self.bib_items);
        for fl in &mut self.floats {
            if fl.file == file {
                ok &= f(&mut fl.range);
            }
            each!(fl.captions);
        }
        each!(self.equations);
        each!(self.theorems);
        each!(self.footnotes);
        each!(self.macros);
        each!(self.environments);
        each!(self.bibliography);
        each!(self.includes);
        each!(self.contents_lines);
        ok
    }

    /// A project's model as seen from its file `this`: that file becomes
    /// file 0 and the root document takes its index, so that what is in
    /// `this` has `file == 0` as in the model of one document, and
    /// everything else (numbers, labels in other files) is kept.
    pub fn seen_from(&self, this: usize) -> Model {
        let mut m = self.clone();
        if this == 0 || this >= m.files.len() {
            return m;
        }
        let swap = |f: &mut usize| {
            if *f == this {
                *f = 0;
            } else if *f == 0 {
                *f = this;
            }
        };
        if let Some(c) = &mut m.class {
            swap(&mut c.file);
        }
        m.packages.iter_mut().for_each(|x| swap(&mut x.file));
        m.sections.iter_mut().for_each(|x| swap(&mut x.file));
        m.labels.iter_mut().for_each(|x| swap(&mut x.file));
        m.references.iter_mut().for_each(|x| swap(&mut x.file));
        m.citations.iter_mut().for_each(|x| swap(&mut x.file));
        m.bib_items.iter_mut().for_each(|x| swap(&mut x.file));
        for f in &mut m.floats {
            swap(&mut f.file);
            f.captions.iter_mut().for_each(|c| swap(&mut c.file));
        }
        m.equations.iter_mut().for_each(|x| swap(&mut x.file));
        m.theorems.iter_mut().for_each(|x| swap(&mut x.file));
        m.footnotes.iter_mut().for_each(|x| swap(&mut x.file));
        m.macros.iter_mut().for_each(|x| swap(&mut x.file));
        m.environments.iter_mut().for_each(|x| swap(&mut x.file));
        m.bibliography.iter_mut().for_each(|x| swap(&mut x.file));
        m.includes.iter_mut().for_each(|x| swap(&mut x.file));
        m.files.swap(0, this);
        m
    }

    /// The label named `name`.
    pub fn label(&self, name: &str) -> Option<&Label> {
        self.labels.iter().find(|l| l.name == name)
    }

    /// The definitions of the macros, as written, for the math renderer
    /// (as `#+LATEX_HEADER` lines are for Org).
    pub fn macro_definitions(&self) -> Vec<String> {
        // Written again from what the model read, so that a definition in
        // another file of the project is the right text.
        self.macros
            .iter()
            .map(|m| {
                let cmd = &m.command;
                match cmd.as_str() {
                    "def" | "gdef" | "edef" | "xdef" => {
                        let params: String = (1..=m.args).map(|i| format!("#{i}")).collect();
                        format!("\\{cmd}{}{params}{{{}}}", m.name, m.body)
                    }
                    "newcommand" | "renewcommand" | "providecommand" => {
                        let mut out = format!("\\{cmd}{{{}}}", m.name);
                        if m.args > 0 {
                            out.push_str(&format!("[{}]", m.args));
                            if let Some(d) = &m.default {
                                out.push_str(&format!("[{d}]"));
                            }
                        }
                        out.push_str(&format!("{{{}}}", m.body));
                        out
                    }
                    _ => format!("\\{cmd}{{{}}}{{{}}}", m.name, m.body),
                }
            })
            .collect()
    }
}

/// The model of the last version of a document, and what each paragraph
/// said, for the next.
#[derive(Debug, Default)]
pub struct Cache {
    events: extract::Cache,
    last: Option<(rowan::GreenNode, Arc<Model>)>,
    /// The events of the last version.
    items: Option<Arc<Vec<Item>>>,
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
        let items = Arc::new(self.events.document(&root));
        let len = usize::from(root.text_range().end());
        // The same events, moved by the edit: the same numbers, moved.
        let moved = match (self.last.take(), &self.items) {
            (Some((g, m)), Some(old)) => {
                let region = extract::Region::between(&g, parse.green());
                if extract::same_moved(old, 0, &items, 0, &region) {
                    // In place when nobody else holds the last model.
                    let mut m = Arc::try_unwrap(m).unwrap_or_else(|m| Model::clone(&m));
                    m.move_positions(0, &region).then_some(m)
                } else {
                    None
                }
            }
            _ => None,
        };
        let m = Arc::new(moved.unwrap_or_else(|| number(&items, len, None)));
        self.last = Some((parse.green().clone(), m.clone()));
        self.items = Some(items);
        m
    }

    /// The events of `parse`, when they are those of the last model.
    pub(crate) fn items_of(&self, parse: &latex_syntax::Parse) -> Option<Arc<Vec<Item>>> {
        let (g, _) = self.last.as_ref()?;
        std::ptr::eq::<rowan::GreenNodeData>(&**g, &**parse.green())
            .then(|| self.items.clone())
            .flatten()
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
    /// amsbook: sections, figures and tables numbered without the
    /// chapter, equations through the whole book.
    AmsBook,
    /// memoir: a book numbering sections only (`secnumdepth` 1).
    Memoir,
}

fn class_kind(name: &str) -> ClassKind {
    match name {
        "book" | "scrbook" | "extbook" => ClassKind::Book,
        "amsbook" => ClassKind::AmsBook,
        "memoir" => ClassKind::Memoir,
        "report" | "scrreprt" | "extreport" => ClassKind::Report,
        _ => ClassKind::Article,
    }
}

/// Whether document class `name` has chapters (`\chapter` above
/// `\section`).
pub fn has_chapters(name: &str) -> bool {
    class_kind(name) != ClassKind::Article
}

/// The counters of the four levels of `enumerate`.
const ENUM_COUNTERS: [&str; 4] = ["enumi", "enumii", "enumiii", "enumiv"];

const SECTION_COUNTERS: [&str; 7] = [
    "part",
    "chapter",
    "section",
    "subsection",
    "subsubsection",
    "paragraph",
    "subparagraph",
];

/// `\roman`: nothing for zero and below, as `\romannumeral` prints.
fn roman(mut n: i64, upper: bool) -> String {
    if n <= 0 {
        return String::new();
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

/// `\alph`: nothing for zero (`\appendix` before its first section
/// numbers an equation `.1`); past `z` LaTeX stops with an error.
fn alph(n: i64, upper: bool) -> String {
    if n <= 0 {
        return String::new();
    }
    if n > 26 {
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
    /// The label number outside the environment, which a `\label` on an
    /// unnumbered line of `gather`, `equation*` and their kin gets.
    outer: Option<String>,
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
    /// Counters `\numberwithin` (or `\counterwithin`) numbered within
    /// another: their `\the…` is `\the<parent>.\arabic{…}` always, not
    /// the class's, which leaves out a chapter that is still 0.
    rewithin: std::collections::HashSet<String>,
    /// Which counter resets which (`child`, `parent`), as `\@addtoreset`
    /// and `\counterwithout` keep them: apart from how a counter is
    /// printed, a counter may be reset by several.
    resets: Vec<(String, String)>,
    /// amsmath's label waiting for a line that writes it (`\df@label`):
    /// one on a line of `align*` or with `\nonumber` goes to the next
    /// numbered line, of this environment or a later one.
    pending: Vec<usize>,
    /// The AMS classes number parts `\arabic`, not `\Roman`.
    arabic_part: bool,
    appendix: bool,
    mainmatter: bool,
    current: (Option<String>, Target),
    envs: Vec<String>,
    eq: Vec<EqEnv>,
    subequations: Option<(String, i64)>,
    section_stack: Vec<(i8, usize)>,
    floats: Vec<usize>,
    sub_captions: i64,
    /// Whether the float being read has its caption yet.
    float_captioned: bool,
    /// Redefined `\the<counter>` bodies, by counter.
    formats: HashMap<String, String>,
    /// Counters reset by another but printed alone (`\newcounter`'s
    /// optional argument).
    plain: std::collections::HashSet<String>,
    /// The lists around, innermost last: whether each is `enumerate`.
    lists: Vec<bool>,
    /// Beside each list: enumitem's reference format (`label=`, `ref=`;
    /// whether `label*=`, after the parent's), and what `\ref` prints for
    /// its last item.
    list_refs: Vec<(Option<String>, bool, String)>,
    saved: Vec<(Option<String>, Target)>,
    /// The file being read.
    file: usize,
    len: usize,
}

impl<'r> Numbering<'r> {
    fn new(len: usize) -> Numbering<'r> {
        let mut n = Numbering::bare(len);
        // Without `\documentclass`, the article class's counters.
        n.set_class(ClassKind::Article);
        n
    }

    fn bare(len: usize) -> Numbering<'r> {
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
            rewithin: std::collections::HashSet::new(),
            resets: Vec::new(),
            pending: Vec::new(),
            arabic_part: false,
            appendix: false,
            mainmatter: true,
            current: (None, Target::None),
            envs: Vec::new(),
            eq: Vec::new(),
            subequations: None,
            section_stack: Vec::new(),
            floats: Vec::new(),
            sub_captions: 0,
            float_captioned: false,
            plain: Default::default(),
            formats: HashMap::new(),
            lists: Vec::new(),
            list_refs: Vec::new(),
            saved: Vec::new(),
            file: 0,
            len,
        }
    }

    fn finish(mut self) -> Model {
        let _ = self.len;
        let left = std::mem::take(&mut self.pending);
        self.model.unwritten_labels.extend(left);
        self.model
    }

    fn set_class(&mut self, kind: ClassKind) {
        self.class = kind;
        self.within.clear();
        self.resets.clear();
        let chaptered = kind != ClassKind::Article;
        for w in SECTION_COUNTERS.windows(2).skip(1) {
            if w[0] == "chapter" && !chaptered {
                continue;
            }
            self.within.insert(w[1].into(), w[0].into());
            self.resets.push((w[1].into(), w[0].into()));
        }
        if chaptered {
            for c in ["equation", "figure", "table", "footnote"] {
                if !(c == "equation" && kind == ClassKind::AmsBook) {
                    self.within.insert(c.into(), "chapter".into());
                    self.resets.push((c.into(), "chapter".into()));
                }
            }
        }
    }

    fn secnumdepth(&self) -> i64 {
        self.secnumdepth.unwrap_or(match self.class {
            ClassKind::Article | ClassKind::AmsBook => 3,
            ClassKind::Memoir => 1,
            _ => 2,
        })
    }

    fn get(&self, c: &str) -> i64 {
        self.counters.get(c).copied().unwrap_or(0)
    }

    /// `\stepcounter`: the counter up, the counters within it reset, and
    /// theirs in turn (`\@stpelt` steps each from -1).
    fn step(&mut self, c: &str) {
        *self.counters.entry(c.to_string()).or_insert(0) += 1;
        let mut pending = vec![c.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(parent) = pending.pop() {
            if !seen.insert(parent.clone()) {
                continue;
            }
            for (k, w) in &self.resets {
                if *w == parent {
                    self.counters.insert(k.clone(), 0);
                    pending.push(k.clone());
                }
            }
        }
    }

    /// What `\ref` prints for an item at enumerate level `depth`:
    /// `\p@enumN\theenumN` of the standard classes (`2`, `2a`,
    /// `2(b)i`, `2(b)iA`).
    fn item_label(&self, depth: usize) -> String {
        let n = |i: usize| self.get(ENUM_COUNTERS[i]);
        let (i, ii, iii, iv) = (n(0), n(1), n(2), n(3));
        match depth {
            1 => i.to_string(),
            2 => format!("{i}{}", alph(ii, false)),
            3 => format!("{i}({}){}", alph(ii, false), roman(iii, false)),
            _ => format!(
                "{i}({}){}{}",
                alph(ii, false),
                roman(iii, false),
                alph(iv, true)
            ),
        }
    }

    /// A `\the<counter>` body written by the document: `\arabic`,
    /// `\roman`, `\Roman`, `\alph`, `\Alph` of a counter, other
    /// `\the…`, and text; other commands and braces print nothing.
    fn format(&self, body: &str, depth: usize) -> String {
        let mut out = String::new();
        let mut rest = body;
        while let Some(c) = rest.chars().next() {
            if c == '\\' {
                let name: String = rest[1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphabetic() || *c == '@')
                    .collect();
                rest = &rest[1 + name.len()..];
                let arg = || -> Option<(String, usize)> {
                    let r = rest.trim_start();
                    let skipped = rest.len() - r.len();
                    let inner = r.strip_prefix('{')?;
                    let end = inner.find('}')?;
                    Some((inner[..end].trim().to_string(), skipped + end + 2))
                };
                let styled = |f: &dyn Fn(i64) -> String| {
                    arg().map(|(counter, used)| (f(self.get(&counter)), used))
                };
                let printed = match name.as_str() {
                    "arabic" => styled(&|n| n.to_string()),
                    "roman" => styled(&|n| roman(n, false)),
                    "Roman" => styled(&|n| roman(n, true)),
                    "alph" => styled(&|n| alph(n, false)),
                    "Alph" => styled(&|n| alph(n, true)),
                    t if t.starts_with("the") && t.len() > 3 && depth < 8 => {
                        Some((self.the_at(&t[3..], depth + 1), 0))
                    }
                    _ => None,
                };
                if let Some((s, used)) = printed {
                    out.push_str(&s);
                    rest = &rest[used..];
                }
            } else {
                if !matches!(c, '{' | '}') {
                    out.push(c);
                }
                rest = &rest[c.len_utf8()..];
            }
        }
        out.trim().to_string()
    }

    /// `\the<counter>`.
    fn the(&self, c: &str) -> String {
        self.the_at(c, 0)
    }

    fn the_at(&self, c: &str, depth: usize) -> String {
        if let Some(body) = self.formats.get(c) {
            return self.format(body, depth);
        }
        let n = self.get(c);
        match c {
            "part" if self.arabic_part => n.to_string(),
            "part" => roman(n, true),
            "chapter" if self.appendix => alph(n, true),
            "section" if self.appendix && self.class == ClassKind::Article => alph(n, true),
            "footnote" => n.to_string(),
            _ if self.plain.contains(c) => n.to_string(),
            "section" | "figure" | "table" if self.class == ClassKind::AmsBook => n.to_string(),
            // book and report: `\ifnum \c@chapter>\z@ \thechapter.\fi`.
            "equation" | "figure" | "table"
                if self.class != ClassKind::Article
                    && self.within.get(c).map(String::as_str) == Some("chapter")
                    && !self.rewithin.contains(c)
                    && self.get("chapter") <= 0 =>
            {
                n.to_string()
            }
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
                self.arabic_part = matches!(name.as_str(), "amsart" | "amsbook" | "amsproc");
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
            } => {
                self.model.packages.push(Package {
                    name: name.clone(),
                    options: options.clone(),
                    range: at(range),
                    file: self.file,
                });
                self.local_package(name);
            }
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
                // `\appendix` defines `\thesection` (`\thechapter`) anew.
                self.formats.remove(if self.class == ClassKind::Article {
                    "section"
                } else {
                    "chapter"
                });
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
                let (mut number, target) = self.current.clone();
                // In a float (not a subfigure) before its caption.
                let float = self.envs.iter().rev().find(|e| float_kind(e).is_some());
                // (After a sub-caption, `\subfloat`'s, it refers to that.)
                let sub = self.sub_captions > 0 && matches!(self.current.1, Target::Float(_));
                if float.is_some_and(|f| !f.starts_with("sub"))
                    && !self.float_captioned
                    && !sub
                    && self.eq.is_empty()
                {
                    self.model.labels_before_caption.push(index);
                    // The caption package writes it as `\caption@xref`.
                    if self
                        .model
                        .packages
                        .iter()
                        .any(|p| matches!(p.name.as_str(), "caption" | "subcaption" | "subfig"))
                    {
                        number = None;
                    }
                }
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
            Event::BibItem { key, label, range } => {
                // With a label of its own, an entry does not step the
                // counter.
                let label = match label {
                    Some(l) => l.clone(),
                    None => {
                        self.step("enumiv");
                        self.get("enumiv").to_string()
                    }
                };
                self.current = (Some(label.clone()), Target::Item);
                self.model.bib_items.push(BibItem {
                    key: key.clone(),
                    label,
                    range: at(range),
                    file: self.file,
                });
            }
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
            Event::Caption {
                text,
                short,
                range,
                of,
                sub,
            } => {
                let env = self
                    .envs
                    .iter()
                    .rev()
                    .find(|e| float_kind(e).is_some())
                    .cloned();
                let kind: Option<String> = match of {
                    Some(k) => float_kind(k).map(str::to_string),
                    None => env.as_deref().and_then(float_kind).map(str::to_string),
                };
                let in_sub = *sub || env.as_deref().is_some_and(|e| e.starts_with("sub"));
                let number = match kind {
                    // A sub-caption (subcaption's `subfigure`, subfig's
                    // `\subfloat`): (a), (b), … in the float; `\ref`
                    // prints the float's number before it, the one its
                    // caption has or is about to get.
                    Some(k) if in_sub && of.is_none() => {
                        self.sub_captions += 1;
                        let n = alph(self.sub_captions, false);
                        let float = if self.float_captioned {
                            self.the(&k)
                        } else {
                            let now = self.get(&k);
                            self.counters.insert(k.clone(), now + 1);
                            let next = self.the(&k);
                            self.counters.insert(k.clone(), now);
                            next
                        };
                        // The subfigure package prints `1(a)`; subfig and
                        // subcaption `1a`.
                        let old = self.model.packages.iter().any(|p| p.name == "subfigure");
                        let r = if old {
                            format!("{float}({n})")
                        } else {
                            format!("{float}{n}")
                        };
                        self.current = (Some(r), Target::Float(k.clone()));
                        Some(n)
                    }
                    Some(k) => {
                        self.step(&k);
                        if of.is_none() {
                            self.float_captioned = true;
                        }
                        let n = self.the(&k);
                        self.current = (Some(n.clone()), Target::Float(k.clone()));
                        Some(n)
                    }
                    None => None,
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
                    // In a minipage: its own counter, a, b, … (`mpfootnote`).
                    None if self.envs.iter().any(|e| e == "minipage") => {
                        self.step("mpfootnote");
                        alph(self.get("mpfootnote"), false)
                    }
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
            } => {
                // `\renewcommand{\thesection}{\Roman{section}}`: how the
                // counter prints from here on.
                if let Some(counter) = name.strip_prefix("\\the")
                    && *args == 0
                    && !counter.is_empty()
                {
                    self.formats.insert(counter.to_string(), body.clone());
                }
                self.model.macros.push(Macro {
                    name: name.clone(),
                    command: command.clone(),
                    args: *args,
                    default: default.clone(),
                    body: body.clone(),
                    range: at(range),
                    file: self.file,
                })
            }
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
                    self.resets.push((counter.clone(), w.clone()));
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
                } else if counter == "tocdepth" {
                    let old = self.model.toc_depth.unwrap_or(match self.class {
                        ClassKind::Article | ClassKind::AmsBook => 3,
                        _ => 2,
                    });
                    self.model.toc_depth = Some(if *add { old + value } else { *value });
                } else {
                    let v = self.counters.entry(counter.clone()).or_insert(0);
                    *v = if *add { *v + value } else { *value };
                }
            }
            Event::NumberWithin {
                counter,
                within,
                remove,
            } => {
                self.plain.remove(counter);
                if *remove {
                    self.resets.retain(|(c, w)| !(c == counter && w == within));
                    self.within.remove(counter);
                    self.rewithin.remove(counter);
                    // Printed alone from now on.
                    self.plain.insert(counter.clone());
                } else {
                    if !self.resets.iter().any(|(c, w)| c == counter && w == within) {
                        self.resets.push((counter.clone(), within.clone()));
                    }
                    self.within.insert(counter.clone(), within.clone());
                    self.rewithin.insert(counter.clone());
                }
            }
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
            Event::ContentsLine {
                level,
                title,
                range,
            } => self.model.contents_lines.push(ContentsLine {
                level: *level,
                title: title.clone(),
                after: self.model.sections.len(),
                range: at(range),
                file: self.file,
            }),
            Event::ResetWithin { counter, within } => {
                self.within.insert(counter.clone(), within.clone());
                self.resets.push((counter.clone(), within.clone()));
                self.plain.insert(counter.clone());
            }
            Event::Step { counter, refer } => {
                self.step(counter);
                if *refer {
                    self.current = (Some(self.the(counter)), Target::Counter(counter.clone()));
                }
            }
            Event::Item { explicit } => {
                let depth = self.lists.iter().filter(|e| **e).count();
                if !*explicit && self.lists.last() == Some(&true) && (1..=4).contains(&depth) {
                    let c = ENUM_COUNTERS[depth - 1];
                    self.step(c);
                    let n = self.list_refs.len();
                    // The enumerate around, if any: its last item's reference.
                    let parent = self.lists[..n - 1]
                        .iter()
                        .rposition(|e| *e)
                        .map(|i| (self.list_refs[i].0.is_some(), self.list_refs[i].2.clone()));
                    let label = match (&self.list_refs[n - 1], parent) {
                        // enumitem's format, after the parent's with `label*`.
                        ((Some(f), star, _), p) => {
                            let own = self.format(f, depth);
                            match p {
                                Some((_, r)) if *star => format!("{r}{own}"),
                                _ => own,
                            }
                        }
                        // Under an item enumitem formats: its reference and
                        // this level's number.
                        ((None, _, _), Some((true, r))) => {
                            let k = self.get(c);
                            let own = match depth {
                                2 => alph(k, false),
                                3 => roman(k, false),
                                _ => alph(k, true),
                            };
                            format!("{r}{own}")
                        }
                        _ => self.item_label(depth),
                    };
                    self.list_refs[n - 1].2 = label.clone();
                    self.current = (Some(label), Target::Item);
                }
            }
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

    /// A package beside the document (`\usepackage{mymacros}` and its
    /// `mymacros.sty`): its definitions read, its theorems and commands
    /// the document's; not an included file.
    fn local_package(&mut self, name: &str) {
        let from = self.file;
        let found = match self.resolver.as_mut() {
            Some(r) if self.active.len() < 32 => r(from, "usepackage", &[name.to_string()]),
            _ => None,
        };
        let Some((id, items)) = found.filter(|(id, _)| !self.active.contains(id)) else {
            return;
        };
        let skipping = self.skipping;
        self.skipping = false;
        self.active.push(id);
        self.file = id;
        self.run(&items, 0);
        self.active.pop();
        self.file = from;
        self.skipping = skipping;
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
        if name == "minipage" {
            self.counters.insert("mpfootnote".into(), 0);
        }
        // `\usecounter{enumiv}`: the entries numbered from 1.
        if name == "thebibliography" {
            self.counters.insert("enumiv".into(), 0);
        }
        if matches!(name, "enumerate" | "itemize" | "description") {
            self.lists.push(name == "enumerate");
            // `\usecounter`: the level's counter from 0.
            let depth = self.lists.iter().filter(|e| **e).count();
            let keys = note.as_deref().map(enumitem_keys).unwrap_or_default();
            let mut format = None;
            let mut star = false;
            if name == "enumerate" && (1..=4).contains(&depth) {
                let c = ENUM_COUNTERS[depth - 1];
                let start = keys
                    .iter()
                    .find(|(k, _)| k == "start")
                    .and_then(|(_, v)| v.trim().parse::<i64>().ok())
                    .unwrap_or(1);
                self.counters.insert(c.into(), start - 1);
                // enumitem's `\alph*`: the level's counter.
                let level = |v: &str| {
                    ["arabic", "alph", "Alph", "roman", "Roman"]
                        .iter()
                        .fold(v.to_string(), |acc, f| {
                            acc.replace(&format!("\\{f}*"), &format!("\\{f}{{{c}}}"))
                        })
                };
                let get = |k: &str| keys.iter().find(|(x, _)| x == k).map(|(_, v)| level(v));
                format = get("ref").or_else(|| get("label"));
                if format.is_none()
                    && let Some(l) = get("label*")
                {
                    format = Some(l);
                    star = true;
                }
            }
            self.list_refs.push((format, star, String::new()));
        }
        if name == "document" && self.file == 0 {
            self.model.preamble = 0..range.start;
            self.model.body = Some(body.clone());
        }
        if let Some(kind) = float_kind(name)
            && !name.starts_with("sub")
        {
            self.floats.push(self.model.floats.len());
            self.sub_captions = 0;
            self.float_captioned = false;
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
            "align"
                | "gather"
                | "flalign"
                | "alignat"
                | "eqnarray"
                | "xalignat"
                | "xxalignat"
                | "IEEEeqnarray"
        );
        // `equation`, `equation*` and `\[…\]` start with no label waiting:
        // one left by `align*` is lost, never written.
        if matches!(base, "equation" | "displaymath") {
            let lost = std::mem::take(&mut self.pending);
            self.model.unwritten_labels.extend(lost);
        }
        if by_line || matches!(base, "equation" | "multline" | "displaymath" | "dmath") {
            self.eq.push(EqEnv {
                name: name.to_string(),
                by_line,
                numbered: !name.ends_with('*') && !matches!(base, "displaymath" | "xxalignat"),
                body_end: body.end,
                line_start: body.start,
                nonumber: false,
                tag: None,
                labels: Vec::new(),
                lines: 0,
                outer: self.current.0.clone(),
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
            // An unnumbered one: a `\label` in it prints nothing.
            if number.is_none() {
                self.current = (Some(String::new()), Target::Theorem(name.to_string()));
            }
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
        if matches!(name.as_str(), "enumerate" | "itemize" | "description") {
            self.lists.pop();
            self.list_refs.pop();
        }
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
        let base = env.trim_end_matches('*');
        let mut labels = labels;
        // amsmath's displays write a waiting label (`equation` and
        // `equation*` label as LaTeX does, and leave it waiting).
        let amsmath = matches!(
            base,
            "align" | "flalign" | "alignat" | "xalignat" | "xxalignat" | "gather" | "multline"
        );
        if amsmath {
            labels.splice(0..0, std::mem::take(&mut self.pending));
            // amsmath takes one label a line: at each one more it stops
            // ("Multiple \label's", the clash) and the one before is
            // lost; the last is written.
            if labels.len() > 1 {
                let last = labels.len() - 1;
                self.model.label_clashes.extend(labels[1..].iter().copied());
                for l in labels.drain(..last) {
                    self.model.labels[l].number = None;
                    self.model.unwritten_labels.push(l);
                }
            }
        }
        if let Some(n) = &number {
            self.current = (Some(n.clone()), Target::Equation);
            for l in labels {
                self.model.labels[l].number = Some(n.clone());
                self.model.labels[l].target = Target::Equation;
            }
        } else if base == "eqnarray" {
            // `eqnarray` steps the counter at each line and takes it back
            // when the line has no number: a label there gets the number
            // the line would have had.
            let n = self.get("equation");
            self.counters.insert("equation".into(), n + 1);
            let would = self.the("equation");
            self.counters.insert("equation".into(), n);
            for l in labels {
                self.model.labels[l].number = Some(would.clone());
                self.model.labels[l].target = Target::Equation;
            }
        } else if matches!(base, "gather" | "equation" | "displaymath" | "dmath") {
            // A line of `gather` writes its label, numbered or not, and so
            // does `equation*`: with the number outside them, each being a
            // group of its own.
            let outer = self.eq.last().and_then(|e| e.outer.clone());
            for l in labels {
                self.model.labels[l].number = outer.clone();
            }
        } else {
            // `align` and its kin write a label on a numbered line only:
            // it waits for the next one (LaTeX never writes it when none
            // comes).
            for &l in &labels {
                self.model.labels[l].number = None;
            }
            self.pending.extend(labels);
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

/// The `key=value` pairs of an optional argument (enumitem's
/// `[label=(\alph*), start=3]`), split at the commas outside braces.
fn enumitem_keys(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut part = String::new();
    for c in s.chars().chain([',']) {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                if let Some((k, v)) = part.split_once('=') {
                    let v = v.trim();
                    let v = v
                        .strip_prefix('{')
                        .and_then(|x| x.strip_suffix('}'))
                        .unwrap_or(v);
                    out.push((k.trim().to_string(), v.to_string()));
                }
                part.clear();
                continue;
            }
            _ => {}
        }
        part.push(c);
    }
    out
}

/// The counter of the captions in environment `env`.
fn float_kind(env: &str) -> Option<&'static str> {
    match env.trim_end_matches('*') {
        "figure" | "wrapfigure" | "subfigure" => Some("figure"),
        "table" | "wraptable" | "longtable" | "subtable" => Some("table"),
        // The algorithm and algorithm2e packages' float.
        "algorithm" => Some("algorithm"),
        _ => None,
    }
}
