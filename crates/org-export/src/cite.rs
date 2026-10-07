//! Citations and bibliographies in export (`oc.el`'s export part): each
//! citation and `#+PRINT_BIBLIOGRAPHY:` keyword replaced, after the tree's
//! properties are collected, by what the export processor makes of it.
//! The processor is `basic` (`oc-basic.el`): author–year citations and a
//! bibliography sorted by author, from the files `#+BIBLIOGRAPHY:` names.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use org_cite::bib::{Bibliography, Entry};
use org_syntax::SyntaxKind::*;
use org_syntax::ast::{self, AstNode};

use crate::export::Exporter;
use crate::tree::{Id, Kind};

/// `org-cite-punctuation-marks`.
const PUNCTUATION: [&str; 6] = [".", ",", ";", ":", "!", "?"];

/// `org-cite--default-region-alist`.
const REGIONS: [(&str, &str); 20] = [
    ("af", "za"),
    ("ca", "ad"),
    ("cs", "cz"),
    ("cy", "gb"),
    ("da", "dk"),
    ("el", "gr"),
    ("et", "ee"),
    ("fa", "ir"),
    ("he", "ir"),
    ("ja", "jp"),
    ("km", "kh"),
    ("ko", "kr"),
    ("nb", "no"),
    ("nn", "no"),
    ("sl", "si"),
    ("sr", "rs"),
    ("sv", "se"),
    ("uk", "ua"),
    ("vi", "vn"),
    ("zh", "cn"),
];

/// The export processor of `#+CITE_EXPORT:` (`org-cite-read-processor-declaration`):
/// its name, bibliography style and citation style.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Processor {
    /// `basic`, `csl`, `natbib`, `biblatex`.
    pub name: String,
    /// The bibliography style.
    pub bibliography_style: Option<String>,
    /// The citation style, `style/variant`.
    pub citation_style: Option<String>,
}

impl Processor {
    /// Reads a declaration: one to three tokens, the last two possibly
    /// in double quotes; `nil` is none.
    pub fn read(s: &str) -> Option<Processor> {
        let mut tokens: Vec<Option<String>> = Vec::new();
        let mut rest = s.trim_start();
        while !rest.is_empty() && tokens.len() < 3 {
            let (tok, after) = if let Some(q) = rest.strip_prefix('"') {
                match q.find('"') {
                    Some(e) => (&q[..e], &q[e + 1..]),
                    None => (q, ""),
                }
            } else {
                let e = rest.find([' ', '\t']).unwrap_or(rest.len());
                (&rest[..e], &rest[e..])
            };
            tokens.push((tok != "nil").then(|| tok.to_string()));
            rest = after.trim_start_matches([' ', '\t']);
        }
        let name = tokens.first().cloned().flatten()?;
        Some(Processor {
            name,
            bibliography_style: tokens.get(1).cloned().flatten(),
            citation_style: tokens.get(2).cloned().flatten(),
        })
    }
}

/// A piece of what a citation or bibliography entry exports to
/// (`org-cite-concat`'s arguments).
#[derive(Debug, Clone)]
enum Piece {
    /// Plain text.
    Str(String),
    /// Text output as it is (`org-export-raw-string`).
    Raw(String),
    /// Objects of the document (a prefix or suffix).
    Ids(Vec<Id>),
    /// Italic (`org-cite-emphasize`).
    Italic(Vec<Piece>),
}

fn s(t: &str) -> Piece {
    Piece::Str(t.to_string())
}

/// Emacs's `capitalize`: each word's first letter up, the others down.
pub(crate) fn capitalize(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut in_word = false;
    for c in t.chars() {
        if c.is_alphanumeric() {
            if in_word {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            in_word = true;
        } else {
            out.push(c);
            in_word = false;
        }
    }
    out
}

/// `org-cite-basic--number-to-suffix`.
fn number_to_suffix(mut n: usize) -> String {
    let mut result: Vec<usize> = Vec::new();
    loop {
        result.insert(0, n % 26);
        n /= 26;
        if n == 0 {
            break;
        } else if n < 27 {
            result.insert(0, n - 1);
            break;
        } else if n == 27 {
            result.insert(0, 0);
            result.insert(0, 0);
            break;
        }
    }
    result
        .into_iter()
        .map(|d| (b'a' + d as u8) as char)
        .collect()
}

/// An author and a year, as `oc-basic` caches them.
type AuthorYearKey = (Option<String>, Option<String>);

/// The `basic` processor's state for one export.
struct Basic {
    bib: Bibliography,
    latex: bool,
    /// `org-cite-list-keys`.
    keys: Vec<String>,
    /// `:cite-basic/author-date-cache`: (author, year) and the first key
    /// seen with them.
    years: Vec<(AuthorYearKey, String)>,
}

impl Basic {
    fn entry(&self, key: &str) -> Option<&Entry> {
        self.bib.get(key)
    }

    /// `org-cite-basic--get-field` with `raw`.
    fn field(&self, key: &str, name: &str) -> Option<String> {
        self.entry(key)?.field(name).map(str::to_string)
    }

    /// `org-cite-basic--get-field`: raw text in LaTeX.
    fn value(&self, key: &str, name: &str) -> Option<Piece> {
        let v = self.field(key, name)?;
        Some(if self.latex {
            Piece::Raw(v)
        } else {
            Piece::Str(v)
        })
    }

    fn author(&self, key: &str) -> Option<Piece> {
        self.value(key, "author")
            .or_else(|| self.value(key, "editor"))
    }

    fn author_raw(&self, key: &str) -> Option<String> {
        self.field(key, "author")
            .or_else(|| self.field(key, "editor"))
    }

    /// `org-cite-basic--get-year`, with its cache: the first key seen
    /// with an author and a year has no suffix, any other one `a` (the
    /// cache is never extended past its first key).
    fn year(&mut self, key: &str, no_suffix: bool) -> Option<String> {
        let author = self.author_raw(key);
        let year = self.field(key, "year").or_else(|| {
            let date = self.field(key, "date")?;
            let b = date.as_bytes();
            (b.len() >= 4
                && b[..4].iter().all(u8::is_ascii_digit)
                && b.get(4).is_none_or(|c| !c.is_ascii_digit()))
            .then(|| date[..4].to_string())
        });
        let cache_key = (author, year.clone());
        let Some((_, first)) = self.years.iter().find(|(k, _)| *k == cache_key) else {
            self.years.push((cache_key, key.to_string()));
            return year;
        };
        let suffix = if first == key {
            String::new()
        } else {
            number_to_suffix(0)
        };
        if no_suffix {
            year
        } else {
            Some(format!("{}{suffix}", year.unwrap_or_default()))
        }
    }

    /// `org-cite-basic--sort-keys`: by the `author` field (not the
    /// editor), ignoring case; a missing one sorts as `nil`, as
    /// `string-collate-lessp` reads it.
    fn sort_keys(&self, keys: &[String]) -> Vec<String> {
        let mut v = keys.to_vec();
        v.sort_by_cached_key(|k| {
            self.field(k, "author")
                .unwrap_or_else(|| "nil".into())
                .to_lowercase()
        });
        v
    }

    /// `org-cite-basic--key-number`.
    fn key_number(&self, key: &str) -> Option<usize> {
        self.sort_keys(&self.keys)
            .iter()
            .position(|k| k == key)
            .map(|p| p + 1)
    }

    /// `org-cite-basic--citation-numbers`.
    fn citation_numbers(&self, keys: &[String]) -> String {
        let mut numbers: Vec<usize> = keys.iter().filter_map(|k| self.key_number(k)).collect();
        numbers.sort_unstable();
        let mut numbers = numbers.into_iter().peekable();
        let Some(first) = numbers.next() else {
            return String::new();
        };
        let mut last = first;
        let mut result = vec![first.to_string()];
        while let Some(current) = numbers.next() {
            let next = numbers.peek().copied();
            if next.is_some_and(|n| current == last + 1 && current + 1 == n) {
                if result.last().map(String::as_str) != Some("-") {
                    result.push("-".into());
                }
            } else if result.last().map(String::as_str) == Some("-") {
                result.push(current.to_string());
            } else {
                result.push(format!(", {current}"));
            }
            last = current;
        }
        result.concat()
    }

    /// `org-cite-basic--print-entry`.
    fn print_entry(&mut self, key: &str, style: Option<&str>) -> Vec<Piece> {
        let author = self.author(key);
        let title = self.value(key, "title");
        let from = ["publisher", "journal", "institution", "school"]
            .iter()
            .find_map(|f| self.value(key, f));
        let mut out: Vec<Piece> = Vec::new();
        match style {
            Some("plain") => {
                let year = self.year(key, true);
                if let Some(a) = author {
                    out.push(self.shorten_names(a));
                }
                out.push(s(". "));
                out.extend(title);
                if let Some(f) = from {
                    out.push(s(", "));
                    out.push(f);
                }
                out.push(s(", "));
                out.extend(year.map(Piece::Str));
                out.push(s("."));
            }
            Some("numeric") => {
                let n = self.key_number(key).unwrap_or(0);
                let year = self.year(key, true);
                out.push(Piece::Str(format!("[{n}] ")));
                out.extend(author);
                out.push(s(", "));
                out.push(Piece::Italic(title.into_iter().collect()));
                if let Some(f) = from {
                    out.push(s(", "));
                    out.push(f);
                }
                out.push(s(", "));
                out.extend(year.map(Piece::Str));
                out.push(s("."));
            }
            _ => {
                let year = self.year(key, false);
                out.extend(author);
                out.push(s(" ("));
                out.extend(year.map(Piece::Str));
                out.push(s("). "));
                out.push(Piece::Italic(title.into_iter().collect()));
                if let Some(f) = from {
                    out.push(s(", "));
                    out.push(f);
                }
                out.push(s("."));
            }
        }
        out
    }

    /// `org-cite-basic--shorten-names`: family names.
    fn shorten_names(&self, names: Piece) -> Piece {
        let short = |t: &str| {
            t.split(" and ")
                .map(|name| {
                    if name.chars().count() == 1 {
                        ""
                    } else {
                        name.split(", ").next().unwrap_or("")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        match names {
            Piece::Str(t) => Piece::Str(short(&t)),
            Piece::Raw(t) => Piece::Raw(short(&t)),
            other => other,
        }
    }
}

/// How `basic` formats the references of an author–year citation.
#[derive(Clone, Copy)]
enum AuthorYear {
    /// `author`: the authors.
    Author { caps: bool },
    /// `noauthor`: the years.
    NoAuthor { bare: bool },
    /// `text` and `note`: Author (Year).
    Text { bare: bool, caps: bool },
    /// The default: (Author, Year).
    Default { bare: bool, caps: bool },
}

/// Replaces the citations and bibliography keywords of the tree
/// (`org-cite-process-citations`, `org-cite-process-bibliography`).
/// What the processor does to the whole output (its export finalizer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Finalizer {
    /// `csl`: its definitions go into the LaTeX preamble.
    Preamble(String),
    /// `natbib`: the package is loaded.
    Natbib,
    /// `biblatex`: the package is loaded with the style, and the files
    /// added as resources.
    Biblatex {
        style: Option<String>,
        files: Vec<String>,
    },
}

/// Applies the processor's finalizer to the output.
pub(crate) fn finalize(out: String, f: &Finalizer) -> String {
    let Some(at) = out.find("\\begin{document}") else {
        return out;
    };
    match f {
        Finalizer::Preamble(p) => format!("{}{p}{}", &out[..at], &out[at..]),
        Finalizer::Natbib => crate::cite_latex::natbib_use_package(out, at),
        Finalizer::Biblatex { style, files } => {
            crate::cite_latex::biblatex_prepare_preamble(out, at, style.as_deref(), files)
        }
    }
}

/// Replaces the citations and bibliography keywords of the tree; what
/// the processor then does to the whole output comes back.
pub(crate) fn process(
    ex: &mut Exporter<'_>,
    keywords: &[(String, String)],
) -> Result<Option<Finalizer>, String> {
    let processor = match keywords
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("CITE_EXPORT"))
    {
        Some((_, v)) => match Processor::read(v) {
            Some(p) => p,
            None => return Ok(None),
        },
        None => Processor {
            name: "basic".into(),
            ..Processor::default()
        },
    };
    let dir = ex
        .info
        .input_file
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let mut files: Vec<PathBuf> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for (_, v) in keywords
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("BIBLIOGRAPHY"))
    {
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(v);
        if v.is_empty() {
            continue;
        }
        let p = match &dir {
            Some(d) => d.join(v),
            None => PathBuf::from(v),
        };
        if !files.contains(&p) {
            files.push(p);
            names.push(v.to_string());
        }
    }
    match processor.name.as_str() {
        "csl" => return crate::csl::process(ex, &processor, &files, dir.as_deref()),
        "natbib" | "biblatex" => {
            return Ok(Some(crate::cite_latex::process(ex, &processor, names)));
        }
        _ => {}
    }
    let (bib, _errors) = Bibliography::load(&files);
    let backend = ex.backend();
    let latex = backend.name() == "latex" || backend.parents().contains(&"latex");
    let citations = list_citations(ex);
    let mut keys: Vec<String> = Vec::new();
    for &c in &citations {
        for k in citation(ex, c).map(|c| c.keys()).unwrap_or_default() {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    let mut basic = Basic {
        bib,
        latex,
        keys,
        years: Vec::new(),
    };
    for c in citations {
        process_citation(ex, &mut basic, &processor, c);
    }
    for k in bibliography_keywords(ex) {
        let out = bibliography(ex, &mut basic, &processor);
        let blanks = ex.tree.nodes[k].post_blank;
        let out = format!(
            "{}{}",
            crate::export::normalize_string(&out),
            "\n".repeat(blanks)
        );
        set_raw(ex, k, out);
    }
    Ok(None)
}

/// The `#+PRINT_BIBLIOGRAPHY:` keywords of the tree.
pub(crate) fn bibliography_keywords(ex: &Exporter<'_>) -> Vec<Id> {
    ex.tree
        .descendants(ex.tree.root)
        .into_iter()
        .filter(|&k| {
            !ex.info.ignore.contains(&k)
                && ex.tree.kind(k) == Some(KEYWORD)
                && ex
                    .tree
                    .syntax(k)
                    .and_then(|s| ast::Keyword::cast(s.clone()))
                    .is_some_and(|kw| kw.key().eq_ignore_ascii_case("PRINT_BIBLIOGRAPHY"))
        })
        .collect()
}

/// Turns node `k` into raw output `out`.
pub(crate) fn set_raw(ex: &mut Exporter<'_>, k: Id, out: String) {
    let n = &mut ex.tree.nodes[k];
    n.kind = Kind::Raw;
    n.text = out;
    n.children.clear();
    n.post_blank = 0;
}

pub(crate) fn citation(ex: &Exporter<'_>, id: Id) -> Option<ast::Citation> {
    ex.tree.syntax(id).cloned().and_then(ast::Citation::cast)
}

/// `org-cite-list-citations`: the citations in reading order, those in
/// footnote definitions where the definitions are referred to.
pub(crate) fn list_citations(ex: &Exporter<'_>) -> Vec<Id> {
    /// The citations found, in order.
    struct Found {
        list: Vec<Id>,
        seen: HashSet<Id>,
    }
    fn search(
        ex: &Exporter<'_>,
        definitions: &HashMap<String, Id>,
        data: &[Id],
        out: &mut Found,
        depth: usize,
    ) {
        for &d in data {
            let mut stack = vec![d];
            while let Some(x) = stack.pop() {
                if ex.info.ignore.contains(&x) {
                    continue;
                }
                match ex.tree.kind(x) {
                    Some(FOOTNOTE_DEFINITION) => continue,
                    Some(CITATION) => {
                        if out.seen.insert(x) {
                            out.list.push(x);
                        }
                        continue;
                    }
                    Some(FOOTNOTE_REFERENCE) if depth < 16 => {
                        let r = ex
                            .tree
                            .syntax(x)
                            .cloned()
                            .and_then(ast::FootnoteReference::cast);
                        if let Some(r) = r.filter(|r| !r.is_inline())
                            && let Some(label) = r.label()
                        {
                            if let Some(&def) = definitions.get(&label) {
                                let contents = ex.tree.children(def).to_vec();
                                search(ex, definitions, &contents, out, depth + 1);
                            }
                            continue;
                        }
                    }
                    _ => {}
                }
                let n = &ex.tree.nodes[x];
                let mut next: Vec<Id> = Vec::new();
                for (_, v) in &n.secondary {
                    next.extend(v);
                }
                next.extend(&n.children);
                stack.extend(next.into_iter().rev());
            }
        }
    }
    let mut out = Found {
        list: Vec::new(),
        seen: HashSet::new(),
    };
    search(ex, &definitions(ex), &[ex.tree.root], &mut out, 0);
    out.list
}

/// The first footnote definition of each label, not counting ignored
/// ones.
fn definitions(ex: &Exporter<'_>) -> HashMap<String, Id> {
    let mut out = HashMap::new();
    for d in ex.tree.descendants(ex.tree.root) {
        if ex.info.ignore.contains(&d) || ex.tree.kind(d) != Some(FOOTNOTE_DEFINITION) {
            continue;
        }
        if let Some(f) = ex
            .tree
            .syntax(d)
            .cloned()
            .and_then(ast::FootnoteDefinition::cast)
        {
            out.entry(f.label()).or_insert(d);
        }
    }
    out
}

/// `org-cite--set-post-blank`.
fn set_post_blank(ex: &mut Exporter<'_>, id: Id, blanks: usize) {
    if ex.tree.is_text(id) {
        let t = &mut ex.tree.nodes[id].text;
        let kept = t.trim_end_matches([' ', '\n']).len();
        t.truncate(kept);
        t.push_str(&" ".repeat(blanks));
    } else {
        ex.tree.nodes[id].post_blank = blanks;
    }
}

/// `org-cite--set-previous-post-blank`.
pub(crate) fn set_previous_post_blank(ex: &mut Exporter<'_>, id: Id, blanks: usize) {
    if let Some(p) = ex.previous_element(id) {
        set_post_blank(ex, p, blanks);
    }
}

/// `org-cite-citation-style`: the style and the variant.
pub(crate) fn citation_style(
    c: &ast::Citation,
    processor: &Processor,
) -> (Option<String>, Option<String>) {
    let separate = |s: Option<&str>| -> (Option<String>, Option<String>) {
        match s {
            None => (None, None),
            Some(s) => match s.split_once('/') {
                None => (Some(s.to_string()), None),
                Some((a, b)) => (
                    Some(a.to_string()),
                    Some(b.to_string()).filter(|b| !b.trim().is_empty()),
                ),
            },
        }
    };
    let not_nil = |s: Option<String>| s.filter(|s| s != "nil");
    let local = match c.style() {
        Some(st) => separate(Some(&st)),
        None => (None, None),
    };
    let global = separate(processor.citation_style.as_deref());
    if local.0.as_ref().is_some_and(|s| !s.trim().is_empty()) {
        (not_nil(local.0), local.1)
    } else {
        (not_nil(global.0), local.1.or(global.1))
    }
}

fn process_citation(ex: &mut Exporter<'_>, basic: &mut Basic, processor: &Processor, id: Id) {
    let Some(c) = citation(ex, id) else { return };
    let (style, variant) = citation_style(&c, processor);
    let variant = variant.as_deref();
    let has = |names: &[&str]| variant.is_some_and(|v| names.contains(&v));
    let bare = has(&["bare", "bare-caps", "b", "bc"]);
    let caps = has(&["caps", "bare-caps", "c", "bc"]);
    let pieces: Option<Vec<Piece>> = match style.as_deref() {
        Some("author" | "a") => Some(author_year(
            ex,
            basic,
            &c,
            AuthorYear::Author {
                caps: has(&["caps", "c"]),
            },
        )),
        Some("noauthor" | "na") => Some(author_year(ex, basic, &c, AuthorYear::NoAuthor { bare })),
        Some("nocite" | "n") => None,
        Some(st @ ("text" | "note" | "t" | "ft")) => {
            if matches!(st, "note" | "ft") && !inside_footnote(ex, id) {
                adjust_note(ex, id);
                wrap_citation(ex, id);
            }
            Some(author_year(ex, basic, &c, AuthorYear::Text { bare, caps }))
        }
        Some("numeric" | "nb") => {
            let refs: Vec<ast::CitationReference> = c.references().collect();
            let (prefix, suffix) = match refs.as_slice() {
                [r] => (r.prefix(), r.suffix()),
                _ => (c.prefix(), c.suffix()),
            };
            let mut out = vec![s("(")];
            if let Some(p) = prefix {
                out.push(Piece::Ids(ex.tree.objects_of(&p)));
            }
            out.push(Piece::Str(basic.citation_numbers(&c.keys())));
            if let Some(p) = suffix {
                out.push(Piece::Ids(ex.tree.objects_of(&p)));
            }
            out.push(s(")"));
            Some(out)
        }
        _ => Some(author_year(
            ex,
            basic,
            &c,
            AuthorYear::Default { bare, caps },
        )),
    };
    let out = pieces.map(|pieces| {
        let ids = build(ex, pieces, false);
        ex.data_list(&ids)
    });
    replace_citation(ex, id, out);
}

/// Puts the processor's output for citation `id` in its place
/// (`org-cite-process-citations`); nothing takes the citation out.
pub(crate) fn replace_citation(ex: &mut Exporter<'_>, id: Id, out: Option<String>) {
    let blanks = ex.tree.nodes[id].post_blank;
    match out {
        None => set_previous_post_blank(ex, id, blanks),
        Some(out) => {
            if let Some(p) = ex.previous_element(id)
                && ex.tree.is_text(p)
                && ex.tree.nodes[p].text.ends_with('"')
            {
                set_previous_post_blank(ex, id, 1);
            }
            let raw = ex
                .tree
                .raw_node(format!("{}{}", out.trim(), " ".repeat(blanks)), None);
            ex.tree.insert_before(id, raw);
        }
    }
    ex.tree.extract(id);
}

/// Puts the processor's output for bibliography keyword `k` in its
/// place (`org-cite-process-bibliography`).
pub(crate) fn replace_bibliography(ex: &mut Exporter<'_>, k: Id, out: &str) {
    let blanks = ex.tree.nodes[k].post_blank;
    let out = format!(
        "{}{}",
        crate::export::normalize_string(out),
        "\n".repeat(blanks)
    );
    set_raw(ex, k, out);
}

/// `org-cite-basic--format-author-year`.
fn author_year(
    ex: &mut Exporter<'_>,
    basic: &mut Basic,
    c: &ast::Citation,
    how: AuthorYear,
) -> Vec<Piece> {
    let mut contents: Vec<Piece> = Vec::new();
    for (i, r) in c.references().enumerate() {
        if i > 0 {
            contents.push(s(", "));
        }
        let key = r.key();
        let author = basic.author(&key).unwrap_or_else(|| s("??"));
        let year = basic
            .year(&key, false)
            .map_or_else(|| s("????"), Piece::Str);
        let cap = |p: Piece| match p {
            Piece::Str(t) => Piece::Str(capitalize(&t)),
            Piece::Raw(t) => Piece::Raw(capitalize(&t)),
            other => other,
        };
        if let Some(p) = r.prefix() {
            contents.push(Piece::Ids(ex.tree.objects_of(&p)));
        }
        match how {
            AuthorYear::Author { caps } => {
                contents.push(if caps { cap(author) } else { author });
            }
            AuthorYear::NoAuthor { .. } => contents.push(year),
            AuthorYear::Text { bare, caps } => {
                contents.push(if caps { cap(author) } else { author });
                contents.push(s(if bare { " " } else { " (" }));
                contents.push(year);
                if !bare {
                    contents.push(s(")"));
                }
            }
            AuthorYear::Default { caps, .. } => {
                contents.push(if caps { cap(author) } else { author });
                contents.push(s(", "));
                contents.push(year);
            }
        }
        if let Some(p) = r.suffix() {
            contents.push(Piece::Ids(ex.tree.objects_of(&p)));
        }
    }
    let parens = match how {
        AuthorYear::NoAuthor { bare } | AuthorYear::Default { bare, .. } => !bare,
        _ => false,
    };
    let mut out = Vec::new();
    if parens {
        out.push(s("("));
    }
    if let Some(p) = c.prefix() {
        out.push(Piece::Ids(ex.tree.objects_of(&p)));
    }
    out.extend(contents);
    if let Some(p) = c.suffix() {
        out.push(Piece::Ids(ex.tree.objects_of(&p)));
    }
    if parens {
        out.push(s(")"));
    }
    out
}

/// `org-cite-basic-export-bibliography`.
fn bibliography(ex: &mut Exporter<'_>, basic: &mut Basic, processor: &Processor) -> String {
    let keys = basic.sort_keys(&basic.keys);
    let mut parts: Vec<String> = Vec::new();
    for k in keys {
        if basic.entry(&k).is_none() {
            continue;
        }
        let entry = basic.print_entry(&k, processor.bibliography_style.as_deref());
        let mut children = Vec::new();
        if basic.latex {
            children.push(ex.tree.raw_node("\\noindent\n".into(), None));
        }
        children.extend(build(ex, entry, !basic.latex));
        let p = ex.tree.made_node(PARAGRAPH, children, None);
        parts.push(ex.data(p));
    }
    parts.join("\n")
}

/// The nodes of `pieces`; with `bibtex`, the text in emphasis is read as
/// `org-cite-basic--print-bibtex-string` reads it (LaTeX fragments and
/// entities, braces removed). Its `org-element-map` does not visit the
/// strings the entry holds directly, only those inside objects.
fn build(ex: &mut Exporter<'_>, pieces: Vec<Piece>, bibtex: bool) -> Vec<Id> {
    build_in(ex, pieces, bibtex, false)
}

fn build_in(ex: &mut Exporter<'_>, pieces: Vec<Piece>, bibtex: bool, inner: bool) -> Vec<Id> {
    let mut out = Vec::new();
    for p in pieces {
        match p {
            Piece::Str(t) if bibtex && inner => out.extend(bibtex_string(ex, &t)),
            Piece::Str(t) => out.push(ex.tree.text_node(t, None)),
            Piece::Raw(t) => out.push(ex.tree.raw_node(t, None)),
            Piece::Ids(ids) => out.extend(ids),
            Piece::Italic(contents) => {
                let children = build_in(ex, contents, bibtex, true);
                out.push(ex.tree.made_node(ITALIC, children, None));
            }
        }
    }
    out
}

/// A BibTeX value as objects: its LaTeX fragments and entities, and the
/// text between them without braces.
fn bibtex_string(ex: &mut Exporter<'_>, t: &str) -> Vec<Id> {
    let unbrace = |t: &str| t.replace(['{', '}'], "");
    let body = t.trim_matches([' ', '\t', '\n']);
    if !body.contains(['\\', '$']) {
        return vec![ex.tree.text_node(unbrace(t), None)];
    }
    let lead = &t[..t.len() - t.trim_start_matches([' ', '\t', '\n']).len()];
    let trail = &t[t.trim_end_matches([' ', '\t', '\n']).len()..];
    let ids = ex.parse_secondary(body);
    let plain = ids
        .iter()
        .all(|&i| ex.tree.is_text(i) || matches!(ex.tree.kind(i), Some(ENTITY | LATEX_FRAGMENT)));
    if !plain {
        return vec![ex.tree.text_node(unbrace(t), None)];
    }
    let mut out = Vec::new();
    if !lead.is_empty() {
        out.push(ex.tree.text_node(lead.to_string(), None));
    }
    for i in ids {
        if ex.tree.is_text(i) {
            let u = unbrace(&ex.tree.nodes[i].text);
            ex.tree.nodes[i].text = u;
        }
        out.push(i);
    }
    if !trail.is_empty() {
        out.push(ex.tree.text_node(trail.to_string(), None));
    }
    out
}

/// `org-cite-inside-footnote-p`.
pub(crate) fn inside_footnote(ex: &Exporter<'_>, id: Id) -> bool {
    ex.tree.ancestors(id).any(|a| {
        matches!(
            ex.tree.kind(a),
            Some(FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE)
        )
    })
}

/// `org-cite-wrap-citation`: the citation put in an anonymous inline
/// footnote.
pub(crate) fn wrap_citation(ex: &mut Exporter<'_>, id: Id) -> Id {
    let blanks = ex.tree.nodes[id].post_blank;
    set_previous_post_blank(ex, id, 0);
    let foot = ex.tree.made_node(FOOTNOTE_REFERENCE, Vec::new(), None);
    ex.tree.nodes[foot].post_blank = blanks;
    ex.tree.insert_before(id, foot);
    ex.tree.extract(id);
    ex.tree.nodes[foot].children.push(id);
    ex.tree.nodes[id].parent = Some(foot);
    // A new footnote: the numbers count it from now on.
    ex.footnote_added();
    foot
}

/// `org-cite--get-note-rule`: where punctuation goes around a note, and
/// the note around the punctuation, for the document's language.
fn note_rule(ex: &Exporter<'_>) -> (&'static str, &'static str, &'static str) {
    let lang = ex.string("language").unwrap_or("en").to_string();
    let tags: Vec<String> = lang.split(['-', '_']).map(str::to_lowercase).collect();
    let (language, region) = match tags.as_slice() {
        [l] => (
            l.clone(),
            REGIONS
                .iter()
                .find(|(a, _)| a == l)
                .map_or_else(|| l.clone(), |(_, r)| r.to_string()),
        ),
        [l, r] => (l.clone(), r.clone()),
        _ => return ("adaptive", "outside", "after"),
    };
    let rules = [
        ("en-us", ("inside", "outside", "after")),
        ("fr", ("adaptive", "same", "before")),
    ];
    let lr = format!("{language}-{region}");
    rules
        .iter()
        .find(|(k, _)| *k == lr)
        .or_else(|| rules.iter().find(|(k, _)| *k == language))
        .map_or(("adaptive", "outside", "after"), |(_, r)| *r)
}

/// The punctuation, quote and spacing ending `t` (`previous-punct-re`
/// in `org-cite-adjust-note`), with where the match of each starts.
struct Ending {
    /// Group 0: where the match starts.
    start: usize,
    /// Group 1: blanks and a punctuation mark.
    punct: Option<(usize, usize)>,
    /// Group 2: a double quote.
    quote: Option<(usize, usize)>,
    /// Group 3: blanks.
    spacing: Option<(usize, usize)>,
}

fn ending(t: &str) -> Ending {
    let blank = |c: char| matches!(c, ' ' | '\t' | '\n');
    let mut end = t.len();
    let body = t.trim_end_matches(blank);
    let spacing = (body.len() < end).then_some((body.len(), end));
    end = body.len();
    let mut start = end;
    let mut quote = None;
    if t[..end].ends_with('"') {
        quote = Some((end - 1, end));
        end -= 1;
        end = t[..end].trim_end_matches(blank).len();
        start = end;
    }
    let mut punct = None;
    if let Some(m) = PUNCTUATION.iter().find(|m| t[..end].ends_with(**m)) {
        let p = end - m.len();
        let b = t[..p].trim_end_matches(blank).len();
        punct = Some((b, end));
        start = b;
    } else if quote.is_none() {
        start = spacing.map_or(t.len(), |s| s.0);
    }
    // Blanks before the quote belong to the match.
    if punct.is_none() && quote.is_some() {
        start = t[..start].trim_end_matches(blank).len();
    }
    Ending {
        start,
        punct,
        quote,
        spacing,
    }
}

/// `org-cite-adjust-note`: punctuation moved around a citation that
/// becomes a note.
pub(crate) fn adjust_note(ex: &mut Exporter<'_>, id: Id) {
    let rule = note_rule(ex);
    let blank = |c: char| matches!(c, ' ' | '\t' | '\n');
    let mut next = ex.next_element(id).filter(|&n| ex.tree.is_text(n));
    let next_punct = |t: &str| -> Option<String> {
        let rest = t.trim_start_matches(blank);
        PUNCTUATION
            .iter()
            .find(|m| rest.starts_with(**m))
            .map(|m| t[..t.len() - rest.len() + m.len()].to_string())
    };
    let mut final_punct = next.and_then(|n| next_punct(&ex.tree.nodes[n].text));
    let previous = ex.previous_element(id).and_then(|p| last_object(ex, p));
    let previous = previous.filter(|&p| ex.tree.is_text(p));
    let (mut punct, quote, spacing) = match previous {
        Some(p) => {
            let e = ending(&ex.tree.nodes[p].text);
            let t = &ex.tree.nodes[p].text;
            (
                e.punct.map(|(a, b)| t[a..b].to_string()),
                e.quote.is_some(),
                e.spacing.is_some(),
            )
        }
        None => (None, false, false),
    };
    if quote || punct.is_some() != final_punct.is_some() {
        let inside = rule.0 == "inside" || (rule.0 == "adaptive" && !spacing);
        if !quote {
        } else if inside {
            if punct.is_none()
                && let (Some(fp), Some(p), Some(n)) = (final_punct.clone(), previous, next)
            {
                let t = ex.tree.nodes[p].text.clone();
                if let Some((a, b)) = ending(&t).quote {
                    ex.tree.nodes[p].text = format!("{}{fp}\"{}", &t[..a], &t[b..]);
                }
                let nt = ex.tree.nodes[n].text.clone();
                ex.tree.nodes[n].text = nt.strip_prefix(fp.as_str()).unwrap_or(&nt).to_string();
                punct = Some(fp);
                final_punct = None;
            }
        } else if let (Some(pn), None, Some(p)) = (punct.clone(), &final_punct, previous) {
            let t = ex.tree.nodes[p].text.clone();
            if let Some((a, b)) = ending(&t).punct {
                ex.tree.nodes[p].text = format!("{}{}", &t[..a], &t[b..]);
            }
            match ex.next_element(id) {
                Some(n) if ex.tree.is_text(n) => {
                    let nt = format!("{pn}{}", ex.tree.nodes[n].text);
                    ex.tree.nodes[n].text = nt;
                    next = Some(n);
                }
                Some(n) => {
                    let t = ex.tree.text_node(pn.clone(), None);
                    ex.tree.insert_before(n, t);
                    next = Some(t);
                }
                None => {
                    if let Some(parent) = ex.tree.parent(id) {
                        let t = ex.tree.text_node(pn.clone(), Some(parent));
                        ex.tree.nodes[parent].children.push(t);
                        next = Some(t);
                    }
                }
            }
            final_punct = Some(pn);
            punct = None;
        }
    }
    let place = if rule.1 == "same" {
        if punct.is_some() && final_punct.is_some() {
            "outside"
        } else if punct.is_some() {
            "inside"
        } else if final_punct.is_some() {
            "outside"
        } else {
            ""
        }
    } else {
        rule.1
    };
    match (place, rule.2) {
        // `org-cite-adjust-note` quotes its call here: nothing moves.
        ("inside", "after") => {}
        ("inside", "before") => {
            if (punct.is_some() || quote)
                && let Some(p) = previous
            {
                insert_at_split(ex, p, id);
            }
        }
        ("outside", "after") => {
            if let (Some(fp), Some(n)) = (final_punct, next) {
                move_punct_before(ex, &fp, id, n);
            }
        }
        ("outside", "before") => {
            if punct.is_some()
                && !quote
                && let Some(p) = previous
            {
                insert_at_split(ex, p, id);
            }
        }
        _ => {}
    }
}

/// The last object of `id` `org-cite-adjust-note` looks at: `id` itself
/// or its last object or text, not entering citations, subscripts and
/// superscripts.
fn last_object(ex: &Exporter<'_>, id: Id) -> Option<Id> {
    let wanted = |k: Option<org_syntax::SyntaxKind>| {
        matches!(
            k,
            None | Some(
                CITATION
                    | CODE
                    | ENTITY
                    | EXPORT_SNIPPET
                    | FOOTNOTE_REFERENCE
                    | LINE_BREAK
                    | LATEX_FRAGMENT
                    | LINK
                    | RADIO_TARGET
                    | STATISTICS_COOKIE
                    | TIMESTAMP
                    | VERBATIM
            )
        )
    };
    let mut last = None;
    let mut stack = vec![id];
    while let Some(x) = stack.pop() {
        if ex.info.ignore.contains(&x) {
            continue;
        }
        let k = ex.tree.kind(x);
        if matches!(k, Some(SUBSCRIPT | SUPERSCRIPT)) {
            continue;
        }
        if ex.tree.nodes[x].kind != Kind::Raw && wanted(k) {
            last = Some(x);
        }
        if k == Some(CITATION) {
            continue;
        }
        stack.extend(ex.tree.children(x).iter().rev());
    }
    last
}

/// `org-cite--insert-at-split`: the citation moved into the text before
/// it, where its final punctuation (or quote) starts.
fn insert_at_split(ex: &mut Exporter<'_>, s: Id, citation: Id) {
    let pb = ex.tree.nodes[citation].post_blank;
    if pb > 0 {
        let t = ex.tree.text_node(" ".repeat(pb), None);
        ex.tree.insert_before(citation, t);
    }
    ex.tree.extract(citation);
    ex.tree.nodes[citation].post_blank = 0;
    ex.tree.insert_before(s, citation);
    let t = ex.tree.nodes[s].text.clone();
    let split = ending(&t).start;
    let first = &t[..split];
    let last = t[split..].trim_end_matches([' ', '\t', '\n']).to_string();
    if !first.trim().is_empty() {
        let f = ex.tree.text_node(first.to_string(), None);
        ex.tree.insert_before(citation, f);
    }
    ex.tree.nodes[s].text = last;
}

/// `org-cite--move-punct-before`.
fn move_punct_before(ex: &mut Exporter<'_>, punct: &str, citation: Id, s: Id) {
    if ex.tree.nodes[s].text == punct {
        ex.tree.extract(s);
    } else {
        let t = ex.tree.nodes[s].text[punct.len()..].to_string();
        ex.tree.nodes[s].text = t;
    }
    set_previous_post_blank(ex, citation, 0);
    let pb = ex.tree.nodes[citation].post_blank;
    let t = ex
        .tree
        .text_node(format!("{}{punct}", " ".repeat(pb)), None);
    ex.tree.insert_before(citation, t);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations() {
        assert_eq!(
            Processor::read("basic numeric"),
            Some(Processor {
                name: "basic".into(),
                bibliography_style: Some("numeric".into()),
                citation_style: None,
            })
        );
        assert_eq!(
            Processor::read("csl \"my style.csl\" nil"),
            Some(Processor {
                name: "csl".into(),
                bibliography_style: Some("my style.csl".into()),
                citation_style: None,
            })
        );
        assert_eq!(Processor::read("  "), None);
    }

    #[test]
    fn helpers() {
        assert_eq!(
            capitalize("jane DOE and smith, john"),
            "Jane Doe And Smith, John"
        );
        assert_eq!(number_to_suffix(0), "a");
        assert_eq!(number_to_suffix(25), "z");
        assert_eq!(number_to_suffix(26), "aa");
        let e = ending("said \"yes\" ");
        assert!(e.quote.is_some() && e.spacing.is_some() && e.punct.is_none());
        let e = ending("a note,");
        assert_eq!(e.punct, Some((6, 7)));
        assert_eq!(e.start, 6);
    }
}
