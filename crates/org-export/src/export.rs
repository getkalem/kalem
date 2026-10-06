//! The transcoder of `ox.el`: the export environment, the pruned tree,
//! `org-export-data` and the helpers back-ends call.

use std::collections::{HashMap, HashSet};

use org_syntax::SyntaxKind::{self, *};
use org_syntax::{ParseContext, SyntaxNode, ast};

use crate::options::{self, Behavior, Value};
use crate::tree::{Id, Kind, Tree};

/// An export back-end: a transcoder per element and object type.
pub trait Backend {
    /// Its name (`html`, `md`, `ascii`, `latex`).
    fn name(&self) -> &'static str;

    /// Back-ends it derives from, for `org-export-derived-backend-p`.
    fn parents(&self) -> &'static [&'static str] {
        &[]
    }

    /// Whether it has a transcoder for `kind`.
    fn has_transcoder(&self, kind: SyntaxKind) -> bool;

    /// Transcodes node `id` with its transcoded `contents`; `None` for
    /// nothing (a nil result).
    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String>;

    /// Transcodes plain text.
    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        let _ = ex;
        text.to_string()
    }

    /// Wraps the body: tables of contents, footnotes.
    fn inner_template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let _ = ex;
        body
    }

    /// Wraps the whole document.
    fn template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let _ = ex;
        body
    }

    /// Changes the tree before transcoding (`:filter-parse-tree`).
    fn filter_parse_tree(&self, ex: &mut Exporter<'_>) {
        let _ = ex;
    }

    /// Changes the final output.
    fn filter_final_output(&self, ex: &mut Exporter<'_>, out: String) -> String {
        let _ = ex;
        out
    }

    /// Changes an element's or object's output, blank lines after it
    /// included (`:filter-headline` and the like).
    fn filter_output(&self, ex: &mut Exporter<'_>, id: Id, out: String) -> String {
        let _ = (ex, id);
        out
    }

    /// Back-end options: property, keyword, `#+OPTIONS:` item, behavior
    /// and default.
    fn options(&self) -> Vec<BackendOption> {
        Vec::new()
    }
}

/// A back-end's option: property, keyword, `#+OPTIONS:` item, behavior
/// and default.
pub type BackendOption = (
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Behavior,
    Value,
);

/// Fuzzy link targets by their normalized search cells.
type FuzzyCache = HashMap<Vec<(u8, String)>, Vec<Id>>;

/// What export works with (`info` in `ox.el`).
#[derive(Debug, Clone, Default)]
pub struct Info {
    /// Option values by property.
    pub values: HashMap<String, Value>,
    /// Parsed keyword values (`title`, `date`, `author`) as nodes.
    pub parsed: HashMap<String, Vec<Id>>,
    /// Keyword values as strings (`email`, `language`, back-end keywords).
    pub strings: HashMap<String, String>,
    /// `#+FILETAGS`.
    pub filetags: Vec<String>,
    /// Every keyword of the document, in order.
    pub keywords: Vec<(String, String)>,
    /// The time of the export, for time stamps in templates.
    pub now: Option<jiff::Zoned>,
    /// Draws formulas as images.
    pub math: Option<crate::MathRenderer>,
    /// `#+OPTIONS:` items that apply unless the document sets them.
    pub ext_options: Option<String>,
    /// `:headline-offset`.
    pub headline_offset: i64,
    /// `:headline-numbering`.
    pub numbering: HashMap<Id, Vec<usize>>,
    /// Nodes kept in the tree but not exported (special table rows and
    /// cells).
    pub ignore: HashSet<Id>,
    /// Body only, without the template.
    pub body_only: bool,
    /// The file being exported.
    pub input_file: Option<std::path::PathBuf>,
}

/// An export in progress.
pub struct Exporter<'b> {
    /// The tree.
    pub tree: Tree,
    /// The environment.
    pub info: Info,
    /// The parse context (TODO keywords).
    pub ctx: ParseContext,
    backend: &'b dyn Backend,
    memo: HashMap<Id, String>,
    refs: HashMap<Id, String>,
    ref_state: u64,
    fuzzy_cache: Option<FuzzyCache>,
    footnote_defs: Option<HashMap<String, Vec<Id>>>,
    tables: std::cell::RefCell<HashMap<Id, std::rc::Rc<TableInfo>>>,
    /// The plain text being transcoded, for smart quotes.
    pub current_text: Option<Id>,
    /// The first error that stops the export, as Emacs's `user-error`.
    pub error: Option<String>,
}

/// `expand-file-name`: `file` from folder `dir`, `~` for the home
/// folder, `.` and `..` resolved as written.
pub fn expand_file_name(file: &str, dir: &std::path::Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let joined = if let Some(rest) = file.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else if file == "~" {
        home
    } else if file.starts_with('/') {
        file.to_string()
    } else if dir.is_absolute() {
        format!("{}/{file}", dir.display())
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        format!("{}/{file}", cwd.join(dir).display())
    };
    // On Windows, Emacs writes `C:/dir/file`: the separators as `/`, the
    // drive kept first.
    let joined = if cfg!(windows) {
        joined.replace('\\', "/")
    } else {
        joined
    };
    let drive = |s: &str| {
        let b = s.as_bytes();
        cfg!(windows) && b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    };
    let joined = if drive(file) {
        file.replace('\\', "/")
    } else {
        joined
    };
    let mut parts: Vec<&str> = Vec::new();
    for c in joined.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    if parts.first().is_some_and(|p| drive(p)) {
        return parts.join("/");
    }
    format!("/{}", parts.join("/"))
}

/// `org-info--link-file-node`: the manual and the node of an `info:`
/// path (`org#Search options`, `org:Tables`).
fn info_file_node(path: &str) -> (String, String) {
    let (file, node) = match path.find(['#', ':']) {
        Some(i) => {
            let rest = &path[i + 1..];
            (&path[..i], Some(rest.strip_prefix(':').unwrap_or(rest)))
        }
        None => (path, None),
    };
    let file = file.trim();
    let node = node.map(str::trim).filter(|n| !n.is_empty());
    (
        if file.is_empty() {
            "dir".into()
        } else {
            file.to_string()
        },
        node.unwrap_or("Top").to_string(),
    )
}

/// The manuals on gnu.org (`org-info-emacs-documents`).
const EMACS_MANUALS: &[&str] = &[
    "ada-mode",
    "auth",
    "autotype",
    "bovine",
    "calc",
    "ccmode",
    "cl",
    "dbus",
    "dired-x",
    "ebrowse",
    "ede",
    "ediff",
    "edt",
    "efaq-w32",
    "efaq",
    "eglot",
    "eieio",
    "eintr",
    "elisp",
    "emacs-gnutls",
    "emacs-mime",
    "emacs",
    "epa",
    "erc",
    "ert",
    "eshell",
    "eudc",
    "eww",
    "flymake",
    "forms",
    "gnus",
    "htmlfontify",
    "idlwave",
    "ido",
    "info",
    "mairix-el",
    "message",
    "mh-e",
    "modus-themes",
    "newsticker",
    "nxml-mode",
    "octave-mode",
    "org",
    "pcl-cvs",
    "pgg",
    "rcirc",
    "reftex",
    "remember",
    "sasl",
    "sc",
    "semantic",
    "ses",
    "sieve",
    "smtpmail",
    "speedbar",
    "srecode",
    "todo-mode",
    "tramp",
    "transient",
    "url",
    "use-package",
    "vhdl-mode",
    "vip",
    "viper",
    "vtable",
    "widget",
    "wisent",
    "woman",
];

/// `org-info-map-html-url`.
fn info_url(manual: &str) -> String {
    match manual {
        "dir" => "https://www.gnu.org/manual/manual.html".into(),
        "libc" => "https://www.gnu.org/software/libc/manual/html_mono/libc.html".into(),
        "make" => "https://www.gnu.org/software/make/manual/make.html".into(),
        m if EMACS_MANUALS.contains(&m) => {
            format!("https://www.gnu.org/software/emacs/manual/html_mono/{m}.html")
        }
        m => format!("{m}.html"),
    }
}

/// `org-info--expand-node-name`: Texinfo's HTML cross-reference name of
/// a node (blanks become `-`, other characters `_XXXX`).
fn info_node_anchor(node: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for c in node.trim().chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            if !blank {
                out.push('-');
            }
            blank = true;
            continue;
        }
        blank = false;
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push_str(&format!("_{:04x}", c as u32));
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "g_t");
    }
    out
}

/// What export needs of a table.
#[derive(Debug)]
struct TableInfo {
    special_column: bool,
    special_rows: HashSet<Id>,
    groups: HashMap<Id, usize>,
    has_header: bool,
    align: Vec<&'static str>,
    /// Width cookies (`<10>`, `<l5>`) of each column.
    widths: Vec<Option<usize>>,
}

impl std::fmt::Debug for Exporter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Exporter")
            .field("backend", &self.backend.name())
            .field("nodes", &self.tree.nodes.len())
            .finish_non_exhaustive()
    }
}

/// `org-element-recursive-objects`.
pub fn recursive_object(k: SyntaxKind) -> bool {
    matches!(
        k,
        BOLD | FOOTNOTE_REFERENCE
            | ITALIC
            | LINK
            | SUBSCRIPT
            | RADIO_TARGET
            | STRIKE_THROUGH
            | SUPERSCRIPT
            | TABLE_CELL
            | UNDERLINE
            | CITATION
    )
}

/// `org-element-normalize-string`: one line feed at the end, unless
/// empty.
pub fn normalize_string(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    // `\(\n[ \t]*\)*\'` replaced by "\n".
    let mut end = s.len();
    loop {
        let t = s[..end].trim_end_matches([' ', '\t']);
        if t.ends_with('\n') {
            end = t.len() - 1;
        } else {
            break;
        }
    }
    format!("{}\n", &s[..end])
}

/// `org-trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\r'])
}

impl<'b> Exporter<'b> {
    /// An exporter for the (macro-expanded) text of a document.
    pub fn new(root: &SyntaxNode, ctx: ParseContext, backend: &'b dyn Backend) -> Exporter<'b> {
        Exporter {
            tree: Tree::build(root),
            info: Info::default(),
            ctx,
            backend,
            memo: HashMap::new(),
            refs: HashMap::new(),
            ref_state: 0x5eed,
            fuzzy_cache: None,
            footnote_defs: None,
            tables: std::cell::RefCell::new(HashMap::new()),
            current_text: None,
            error: None,
        }
    }

    /// The back-end.
    pub fn backend(&self) -> &'b dyn Backend {
        self.backend
    }

    /// Runs `f` with another back-end (`org-export-with-backend`): a
    /// fresh memo, the same environment.
    pub fn with_backend<R>(&mut self, other: &'b dyn Backend, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = std::mem::replace(&mut self.backend, other);
        let memo = std::mem::take(&mut self.memo);
        let r = f(self);
        self.backend = old;
        self.memo = memo;
        r
    }

    // Options.

    /// An option's value.
    pub fn opt(&self, prop: &str) -> Value {
        self.info.values.get(prop).cloned().unwrap_or(Value::Nil)
    }

    /// Whether an option is non-nil.
    pub fn flag(&self, prop: &str) -> bool {
        self.opt(prop).truthy()
    }

    /// A keyword option's string.
    /// `org-export-custom-protocol-maybe`: the output of the link types
    /// Org gives an export function (the default `org-modules`: `doi`,
    /// `info`, `irc`, `bbdb`, `docview`) for `backend` (`html`, `md`,
    /// `latex`, `ascii`), or `None` for the back-end's own handling.
    pub fn custom_protocol(
        &self,
        ty: &str,
        path: &str,
        desc: Option<&str>,
        backend: &str,
    ) -> Option<String> {
        match ty {
            "doi" => {
                let uri = format!("https://doi.org/{path}");
                Some(match backend {
                    "html" => format!("<a href=\"{uri}\">{}</a>", desc.unwrap_or(&uri)),
                    "latex" => match desc {
                        Some(d) => format!("\\href{{{uri}}}{{{d}}}"),
                        None => format!("\\url{{{uri}}}"),
                    },
                    "ascii" => match desc {
                        None => format!("<{uri}>"),
                        Some(d) if self.flag("ascii-links-to-notes") => format!("[{d}]"),
                        Some(d) => format!("[{d}] (<{uri}>)"),
                    },
                    _ => uri,
                })
            }
            "info" if backend == "html" => {
                let (manual, node) = info_file_node(path);
                Some(format!(
                    "<a href=\"{}#{}\">{}</a>",
                    info_url(&manual),
                    info_node_anchor(&node),
                    desc.unwrap_or(path)
                ))
            }
            "irc" => {
                let d = desc.unwrap_or(path);
                match backend {
                    "html" => Some(format!("<a href=\"irc:{path}\">{d}</a>")),
                    "md" => Some(format!("[{d}](irc:{path})")),
                    _ => None,
                }
            }
            "bbdb" => {
                let own = format!("bbdb:{path}");
                let d = match desc {
                    Some(d) if d == own => path,
                    Some(d) => d,
                    None => "nil",
                };
                Some(match backend {
                    "html" => format!("<i>{d}</i>"),
                    "latex" => format!("\\textit{{{d}}}"),
                    _ => d.to_string(),
                })
            }
            "docview" => {
                let file = path.split_once("::").map_or(path, |(f, _)| f);
                let dir = self
                    .info
                    .input_file
                    .as_ref()
                    .and_then(|f| f.parent())
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default();
                let full = expand_file_name(file, &dir);
                let d = desc.unwrap_or(path);
                Some(match backend {
                    "html" => format!("<a href=\"{full}\">{d}</a>"),
                    "latex" => format!("\\href{{{full}}}{{{d}}}"),
                    "ascii" => format!("[{d}] (<{full}>)"),
                    _ => full,
                })
            }
            _ => None,
        }
    }

    /// `org-export-insert-image-links`: a link whose whole description is
    /// a plain or angle link that `rule` (type, path) calls an image gets
    /// that link as its contents: `[[file:big.jpg][file:small.png]]` shows
    /// the small image, linked to the big one.
    pub fn insert_image_links(&mut self, rule: fn(&str, &str) -> bool) {
        for id in self.tree.descendants(self.tree.root) {
            if self.tree.kind(id) != Some(SyntaxKind::LINK) {
                continue;
            }
            let children = self.tree.children(id).to_vec();
            if children.is_empty() {
                continue;
            }
            let contents: String = children.iter().map(|c| self.tree.source(*c)).collect();
            let inner = contents
                .strip_prefix('<')
                .and_then(|c| c.strip_suffix('>'))
                .unwrap_or(&contents);
            let Some((ty, path)) = inner.split_once(':') else {
                continue;
            };
            let plain_path = !path.is_empty()
                && !path
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '[' | ']'))
                && path
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_alphanumeric() || matches!(c, '-' | '/' | ')'));
            if contents.trim().is_empty()
                || !ty.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                || !(plain_path || inner.len() < contents.len())
                || !rule(ty, path)
            {
                continue;
            }
            let parse = org_syntax::parse(&contents);
            let Some(link) = parse
                .syntax()
                .descendants()
                .find(|n| n.kind() == SyntaxKind::LINK)
            else {
                continue;
            };
            let new = self.tree.graft(&link, id);
            self.tree.nodes[id].children = vec![new];
        }
    }

    /// `org-export-translate`: `s` in the document's language for
    /// `encoding` (`html`, `latex`, `utf-8`, `ascii`, `latin1`), else its
    /// `default` translation, else `s`.
    pub fn translate(&self, s: &str, encoding: &str) -> String {
        let lang = self.string("language").unwrap_or("en");
        let find = |enc: &str| {
            crate::dictionary::DICTIONARY
                .iter()
                .find(|(k, l, e, _)| *k == s && *l == lang && *e == enc)
                .map(|r| r.3)
        };
        find(encoding)
            .or_else(|| find("default"))
            .unwrap_or(s)
            .to_string()
    }

    pub fn string(&self, prop: &str) -> Option<&str> {
        self.info.strings.get(prop).map(String::as_str)
    }

    /// Reads the environment: defaults, keywords, `#+OPTIONS:` lines.
    pub fn read_environment(&mut self, keywords: &[(String, String)]) {
        self.info.keywords = keywords.to_vec();
        let backend_opts = self.backend.options();
        // Defaults.
        for s in options::OPTIONS {
            self.info
                .values
                .insert(s.property.to_string(), options::default(s.property));
        }
        for (p, _, _, _, d) in &backend_opts {
            self.info.values.insert(p.to_string(), d.clone());
        }
        // The caller's `#+OPTIONS:` items, below the document's own
        // (`ext-plist`).
        if let Some(line) = self.info.ext_options.clone() {
            for (item, value) in options::parse_options(&line) {
                let prop = backend_opts
                    .iter()
                    .find(|o| o.2 == Some(item.as_str()))
                    .map(|o| o.0)
                    .or_else(|| {
                        options::OPTIONS
                            .iter()
                            .find(|o| o.option == Some(item.as_str()))
                            .map(|o| o.property)
                    });
                if let Some(prop) = prop {
                    self.info.values.insert(prop.to_string(), value);
                }
            }
        }
        // Keywords, back-end ones first.
        let mut specs: Vec<(String, Option<&str>, Option<&str>, Behavior)> = backend_opts
            .iter()
            .map(|(p, k, o, b, _)| (p.to_string(), *k, *o, *b))
            .collect();
        specs.extend(
            options::OPTIONS
                .iter()
                .map(|s| (s.property.to_string(), s.keyword, s.option, s.behavior)),
        );
        let mut collected: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in keywords {
            collected
                .entry(k.to_ascii_uppercase())
                .or_default()
                .push(v.clone());
        }
        if let Some(opts) = collected.get("OPTIONS") {
            for line in opts {
                for (item, value) in options::parse_options(line) {
                    if let Some(s) = specs.iter().find(|s| s.2.is_some_and(|o| o == item)) {
                        self.info.values.insert(s.0.clone(), value);
                    }
                }
            }
        }
        if let Some(f) = collected.get("FILETAGS") {
            let mut tags: Vec<String> = Vec::new();
            for v in f {
                for t in v.split(':').filter(|t| !t.trim().is_empty()) {
                    let t = t.trim().to_string();
                    if !tags.contains(&t) {
                        tags.push(t);
                    }
                }
            }
            self.info.filetags = tags;
        }
        let mut seen: HashSet<String> = HashSet::new();
        for (prop, kw, _, behavior) in &specs {
            let Some(kw) = kw else { continue };
            if !seen.insert(prop.clone()) {
                continue;
            }
            let Some(values) = collected.get(*kw) else {
                continue;
            };
            self.set_option(prop, *behavior, values);
        }
    }

    /// The options of the subtree being exported
    /// (`org-export--get-subtree-options`), over those of the buffer:
    /// `EXPORT_OPTIONS`, `EXPORT_TITLE` (else the headline's `title`), and
    /// `EXPORT_` followed by any other option keyword, from the headline's
    /// `properties`.
    pub fn read_subtree_options(&mut self, properties: &[(String, String)], title: &str) {
        let get = |name: &str| {
            properties
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        };
        let backend_opts = self.backend.options();
        let mut specs: Vec<(String, Option<&str>, Option<&str>, Behavior)> = backend_opts
            .iter()
            .map(|(p, k, o, b, _)| (p.to_string(), *k, *o, *b))
            .collect();
        specs.extend(
            options::OPTIONS
                .iter()
                .map(|s| (s.property.to_string(), s.keyword, s.option, s.behavior)),
        );
        if let Some(line) = get("EXPORT_OPTIONS") {
            for (item, value) in options::parse_options(&line) {
                if let Some(s) = specs.iter().find(|s| s.2.is_some_and(|o| o == item)) {
                    self.info.values.insert(s.0.clone(), value);
                }
            }
        }
        let mut seen: HashSet<String> = HashSet::new();
        for (prop, kw, _, behavior) in &specs {
            let Some(kw) = kw else { continue };
            if !seen.insert(prop.clone()) {
                continue;
            }
            let value = match get(&format!("EXPORT_{kw}")) {
                Some(v) => v,
                None if *kw == "TITLE" => title.to_string(),
                None => continue,
            };
            self.set_option(prop, *behavior, &[value]);
        }
    }

    /// Sets option `prop` from keyword `values` as `behavior` says.
    fn set_option(&mut self, prop: &str, behavior: Behavior, values: &[String]) {
        let prop = prop.to_string();
        match behavior {
            Behavior::Parse => {
                let text = values.join("\n");
                // The Org text too, for `org-element-interpret-data`.
                self.info
                    .strings
                    .insert(prop.clone(), text.replace('\n', " "));
                let ids = self.parse_secondary(&text);
                self.info.parsed.insert(prop.clone(), ids);
                self.info.values.insert(prop.clone(), Value::T);
            }
            Behavior::Split => {
                let words: Vec<Value> = values
                    .iter()
                    .flat_map(|v| v.split_whitespace())
                    .map(|w| Value::Str(w.to_string()))
                    .collect();
                self.info.values.insert(prop.clone(), Value::List(words));
            }
            b => {
                let v = match b {
                    Behavior::First => values[0].clone(),
                    Behavior::Space => values.join(" "),
                    Behavior::Newline => values.join("\n"),
                    _ => values.last().cloned().unwrap_or_default(),
                };
                self.info.strings.insert(prop.clone(), v.clone());
                self.info.values.insert(prop.clone(), Value::Str(v));
            }
        }
    }

    /// `org-export-get-alt-title`: the headline's `ALT_TITLE` property,
    /// parsed, or its title.
    pub fn alt_title(&mut self, id: Id) -> Vec<Id> {
        match self.node_property(id, "ALT_TITLE", false) {
            Some(a) => self.parse_secondary(&a),
            None => self
                .tree
                .secondary(id, crate::tree::Secondary::Title)
                .map(<[Id]>::to_vec)
                .unwrap_or_default(),
        }
    }

    /// Org text parsed as objects (a keyword's value), added to the tree
    /// outside the document; line feeds become spaces.
    pub fn parse_secondary(&mut self, text: &str) -> Vec<Id> {
        let p = org_syntax::parse(text);
        let root = p.syntax();
        let Some(par) = root.descendants().find(|n| n.kind() == PARAGRAPH) else {
            if text.trim().is_empty() {
                return Vec::new();
            }
            let id = self.tree.text_node(text.replace('\n', " "), None);
            return vec![id];
        };
        let sub = Tree::build(&par);
        // Graft the paragraph's contents into our tree.
        let base = self.tree.nodes.len();
        let mut ids = Vec::new();
        for mut n in sub.nodes.into_iter() {
            n.parent = n.parent.map(|p| p + base);
            n.children = n.children.iter().map(|c| c + base).collect();
            for (_, v) in &mut n.secondary {
                for c in v.iter_mut() {
                    *c += base;
                }
            }
            if n.kind == Kind::Text {
                n.text = n.text.replace('\n', " ");
            }
            self.tree.nodes.push(n);
        }
        let root_id = sub.root + base;
        for c in self.tree.nodes[root_id].children.clone() {
            self.tree.nodes[c].parent = None;
            ids.push(c);
        }
        ids
    }

    // The tree.

    /// Takes out what is not exported (`org-export--prune-tree`), and
    /// turns what the options leave uninterpreted back into text.
    pub fn prune(&mut self) {
        let selected = self.selected_trees();
        let excluded: Vec<String> = self.opt("exclude-tags").strings();
        // A select tag in use: the section before the first headline goes.
        if !selected.is_empty() {
            let root = self.tree.root;
            if let Some(&first) = self.tree.children(root).first()
                && self.tree.kind(first) == Some(SECTION)
            {
                self.tree.extract(first);
            }
        }
        let root = self.tree.root;
        let mut stack = vec![root];
        let mut ignore = HashSet::new();
        while let Some(id) = stack.pop() {
            if id != root && self.skip_p(id, &selected, &excluded) {
                let k = self.tree.kind(id);
                if matches!(k, Some(TABLE_CELL | TABLE_ROW)) {
                    ignore.insert(id);
                } else if let Some(spaces) = self.keep_spaces(id, &ignore) {
                    let s = self.tree.text_node(spaces, None);
                    self.tree.insert_before(id, s);
                    self.tree.extract(id);
                } else {
                    self.tree.extract(id);
                }
                continue;
            }
            let archived_title_only = self.tree.kind(id) == Some(HEADLINE)
                && self.opt("with-archived-trees").sym() == Some("headline")
                && self.headline(id).is_some_and(|h| h.is_archived());
            if archived_title_only {
                self.tree.nodes[id].children.clear();
            }
            let n = &self.tree.nodes[id];
            let mut next: Vec<Id> = Vec::new();
            next.extend(&n.children);
            for (_, v) in &n.secondary {
                next.extend(v);
            }
            stack.extend(next.into_iter().rev());
        }
        for ids in self.info.parsed.values().cloned().collect::<Vec<_>>() {
            let _ = ids;
        }
        self.info.ignore = ignore;
        // Table facts computed while pruning did not know what is ignored.
        self.tables.borrow_mut().clear();
        self.install_missing_footnotes();
        self.remove_uninterpreted();
    }

    /// `org-export--selected-trees`.
    fn selected_trees(&self) -> HashSet<Id> {
        let select: Vec<String> = self.opt("select-tags").strings();
        let mut out = HashSet::new();
        if self.info.filetags.iter().any(|t| select.contains(t)) {
            for id in self.tree.descendants(self.tree.root) {
                if matches!(self.tree.kind(id), Some(HEADLINE | INLINETASK)) {
                    out.insert(id);
                }
            }
            return out;
        }
        fn walk(
            ex: &Exporter<'_>,
            id: Id,
            genealogy: &mut Vec<Id>,
            select: &[String],
            out: &mut HashSet<Id>,
        ) {
            let t = &ex.tree;
            match t.kind(id) {
                Some(HEADLINE | INLINETASK) => {
                    let tags = ex.own_tags(id);
                    if tags.iter().any(|t| select.contains(t)) {
                        out.extend(genealogy.iter().copied());
                        for d in t.descendants(id) {
                            if matches!(t.kind(d), Some(HEADLINE | INLINETASK)) {
                                out.insert(d);
                            }
                        }
                    } else if t.kind(id) == Some(HEADLINE) {
                        genealogy.push(id);
                        for &c in t.children(id) {
                            walk(ex, c, genealogy, select, out);
                        }
                        genealogy.pop();
                    }
                }
                Some(k) if k == DOCUMENT || k.is_greater_element() => {
                    for &c in t.children(id) {
                        walk(ex, c, genealogy, select, out);
                    }
                }
                _ => {}
            }
        }
        walk(self, self.tree.root, &mut Vec::new(), &select, &mut out);
        out
    }

    /// `org-export--skip-p`.
    fn skip_p(&mut self, id: Id, selected: &HashSet<Id>, excluded: &[String]) -> bool {
        let Some(k) = self.tree.kind(id) else {
            return false;
        };
        match k {
            COMMENT | COMMENT_BLOCK => {
                if let Some(prev) = self.previous_element(id) {
                    let before = self.tree.nodes[prev].post_blank;
                    let after = self.tree.nodes[id].post_blank;
                    self.tree.nodes[prev].post_blank = before.max(after).max(1);
                }
                true
            }
            CLOCK => !self.flag("with-clocks"),
            DRAWER => {
                let d = self.opt("with-drawers");
                if !d.truthy() {
                    return true;
                }
                match &d {
                    Value::List(items) => {
                        let name = self
                            .syntax(id)
                            .and_then(|s| ast::AstNode::cast(s.clone()))
                            .map(|d: ast::Drawer| d.name())
                            .unwrap_or_default();
                        let not = items.first().and_then(Value::sym) == Some("not");
                        let names: Vec<String> = items
                            .iter()
                            .skip(usize::from(not))
                            .filter_map(|v| match v {
                                Value::Str(s) | Value::Sym(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect();
                        let member = names.iter().any(|n| n.eq_ignore_ascii_case(&name));
                        if not { member } else { !member }
                    }
                    _ => false,
                }
            }
            FIXED_WIDTH => !self.flag("with-fixed-width"),
            FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE => !self.flag("with-footnotes"),
            HEADLINE | INLINETASK => {
                let Some(h) = self.headline(id) else {
                    return false;
                };
                let tasks = self.opt("with-tasks");
                let todo = h.todo_keyword().map(|t| t.text().to_string());
                let done = todo.as_deref().is_some_and(|t| self.ctx.is_done_keyword(t));
                let archived = self.opt("with-archived-trees");
                let tags = self.tags(id, &[], true);
                (k == INLINETASK && !self.flag("with-inlinetasks"))
                    || tags.iter().any(|t| excluded.contains(t))
                    || (!selected.is_empty() && !selected.contains(&id))
                    || h.is_commented()
                    || (!archived.truthy() && h.is_archived())
                    || todo.is_some() && {
                        match &tasks {
                            Value::Nil => true,
                            Value::Sym(s) if s == "todo" => done,
                            Value::Sym(s) if s == "done" => !done,
                            Value::List(_) => {
                                !tasks.strings().contains(todo.as_ref().expect("todo"))
                            }
                            _ => false,
                        }
                    }
            }
            LATEX_ENVIRONMENT | LATEX_FRAGMENT => !self.flag("with-latex"),
            NODE_PROPERTY => {
                let set = self.opt("with-properties");
                match &set {
                    Value::Nil => true,
                    Value::List(_) => {
                        let key = self
                            .syntax(id)
                            .and_then(|s| ast::AstNode::cast(s.clone()))
                            .map(|p: ast::NodeProperty| p.key())
                            .unwrap_or_default();
                        !set.strings().iter().any(|s| s.eq_ignore_ascii_case(&key))
                    }
                    _ => false,
                }
            }
            PLANNING => !self.flag("with-planning"),
            PROPERTY_DRAWER => !self.flag("with-properties"),
            STATISTICS_COOKIE => !self.flag("with-statistics-cookies"),
            TABLE => !self.flag("with-tables"),
            TABLE_CELL => {
                let table = self.tree.ancestor(id, TABLE);
                table.is_some_and(|t| self.table_has_special_column(t)) && self.first_sibling_p(id)
            }
            TABLE_ROW => !self.flag("with-special-rows") && self.table_row_is_special(id),
            TIMESTAMP => {
                let parent = self
                    .tree
                    .ancestors(id)
                    .find(|a| self.tree.kind(*a).is_some_and(|k| k.is_element()));
                let isolated = parent.is_some_and(|p| {
                    matches!(self.tree.kind(p), Some(PARAGRAPH | VERSE_BLOCK))
                        && self.tree.children(p).iter().all(|&c| {
                            self.tree.kind(c) == Some(TIMESTAMP)
                                || (self.tree.is_text(c)
                                    && self.tree.nodes[c].text.trim().is_empty())
                        })
                });
                if !isolated {
                    return false;
                }
                let ty = self
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|t: ast::Timestamp| t.timestamp_type());
                let active = ty.is_some_and(|t| {
                    matches!(
                        t,
                        ast::TimestampType::Active | ast::TimestampType::ActiveRange
                    )
                });
                match self.opt("with-timestamps") {
                    Value::Nil => true,
                    Value::Sym(s) if s == "active" => !active,
                    Value::Sym(s) if s == "inactive" => active,
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// `org-export--keep-spaces`.
    fn keep_spaces(&self, id: Id, ignore: &HashSet<Id>) -> Option<String> {
        let blank = self.tree.nodes[id].post_blank;
        if blank == 0 || !self.tree.is_object(id) {
            return None;
        }
        let prev = self.previous_with(id, ignore)?;
        let prev_blank = if self.tree.is_text(prev) {
            self.tree.nodes[prev]
                .text
                .ends_with([' ', '\t', '\r', '\n'])
        } else {
            self.tree.nodes[prev].post_blank > 0
        };
        (!prev_blank).then(|| " ".repeat(blank))
    }

    fn previous_with(&self, id: Id, ignore: &HashSet<Id>) -> Option<Id> {
        let mut p = self.tree.previous(id);
        while let Some(x) = p {
            if !ignore.contains(&x) {
                return Some(x);
            }
            p = self.tree.previous(x);
        }
        None
    }

    /// `org-export-get-previous-element`.
    pub fn previous_element(&self, id: Id) -> Option<Id> {
        self.previous_with(id, &self.info.ignore)
    }

    /// `org-export-get-next-element`.
    pub fn next_element(&self, id: Id) -> Option<Id> {
        let mut n = self.tree.next(id);
        while let Some(x) = n {
            if !self.info.ignore.contains(&x) {
                return Some(x);
            }
            n = self.tree.next(x);
        }
        None
    }

    /// `org-export-first-sibling-p`.
    pub fn first_sibling_p(&self, id: Id) -> bool {
        match self.previous_element(id) {
            None => true,
            Some(p) => self.tree.kind(p) == Some(SECTION),
        }
    }

    /// `org-export-last-sibling-p`.
    pub fn last_sibling_p(&self, id: Id) -> bool {
        match self.next_element(id) {
            None => true,
            Some(n) => {
                self.tree.kind(id) == Some(HEADLINE) && self.true_level(id) > self.true_level(n)
            }
        }
    }

    /// `org-export--remove-uninterpreted-data`.
    fn remove_uninterpreted(&mut self) {
        let with_entities = self.flag("with-entities");
        let with_emphasize = self.flag("with-emphasize");
        let latex_verbatim = self.opt("with-latex").sym() == Some("verbatim");
        let subsup = self.opt("with-sub-superscript");
        let mut roots = vec![self.tree.root];
        roots.extend(self.info.parsed.values().flatten().copied());
        let mut all = Vec::new();
        for r in roots {
            all.extend(self.tree.descendants(r));
        }
        for id in all {
            let Some(k) = self.tree.kind(id) else {
                continue;
            };
            let blank = self.tree.nodes[id].post_blank;
            let pb = |c: char| c.to_string().repeat(blank);
            let replace: Option<Vec<Result<String, Id>>> = match k {
                ENTITY if !with_entities => Some(vec![Ok(format!(
                    "{}{}",
                    self.expand(id).trim_end_matches([' ', '\t']),
                    pb(' ')
                ))]),
                BOLD | ITALIC | STRIKE_THROUGH | UNDERLINE if !with_emphasize => {
                    let m = match k {
                        BOLD => "*",
                        ITALIC => "/",
                        STRIKE_THROUGH => "+",
                        _ => "_",
                    };
                    let mut v = vec![Ok(m.to_string())];
                    v.extend(self.tree.children(id).iter().map(|c| Err(*c)));
                    v.push(Ok(format!("{m}{}", pb(' '))));
                    Some(v)
                }
                LATEX_ENVIRONMENT | LATEX_FRAGMENT if latex_verbatim => {
                    let c = if k == LATEX_ENVIRONMENT { '\n' } else { ' ' };
                    Some(vec![Ok(format!("{}{}", self.expand_trimmed(id), pb(c)))])
                }
                SUBSCRIPT | SUPERSCRIPT => {
                    let bracket = self
                        .syntax(id)
                        .map(|s| {
                            if k == SUBSCRIPT {
                                ast::AstNode::cast(s.clone())
                                    .is_some_and(|x: ast::Subscript| x.uses_brackets())
                            } else {
                                ast::AstNode::cast(s.clone())
                                    .is_some_and(|x: ast::Superscript| x.uses_brackets())
                            }
                        })
                        .unwrap_or(false);
                    let off = !subsup.truthy() || (subsup.sym() == Some("{}") && !bracket);
                    off.then(|| {
                        let mut v = vec![Ok(format!(
                            "{}{}",
                            if k == SUBSCRIPT { "_" } else { "^" },
                            if bracket { "{" } else { "" }
                        ))];
                        v.extend(self.tree.children(id).iter().map(|c| Err(*c)));
                        v.push(Ok(format!("{}{}", if bracket { "}" } else { "" }, pb(' '))));
                        v
                    })
                }
                _ => None,
            };
            if let Some(parts) = replace {
                for p in parts {
                    match p {
                        Ok(s) if s.is_empty() => {}
                        Ok(s) => {
                            let t = self.tree.text_node(s, None);
                            self.tree.insert_before(id, t);
                        }
                        Err(c) => {
                            self.tree.insert_before(id, c);
                        }
                    }
                }
                self.tree.extract(id);
            }
        }
    }

    /// `org-export-expand` without contents: the node as written.
    pub fn expand(&self, id: Id) -> String {
        self.tree.source(id)
    }

    fn expand_trimmed(&self, id: Id) -> String {
        let s = self.tree.source(id);
        let blank = self.tree.nodes[id].post_blank;
        if self.tree.is_object(id) {
            s[..s.len() - s.len().min(blank)].to_string()
        } else {
            normalize_string(&s).trim_end_matches('\n').to_string() + "\n"
        }
    }

    /// Computes `:headline-offset` and `:headline-numbering`.
    pub fn collect_tree_properties(&mut self) {
        let root = self.tree.root;
        let mut min = usize::MAX;
        for &c in self.tree.children(root) {
            if self.tree.kind(c) == Some(HEADLINE)
                && !self.info.ignore.contains(&c)
                && !self.footnote_section_p(c)
            {
                min = min.min(self.true_level(c));
            }
        }
        let min = if min == usize::MAX { 1 } else { min };
        self.info.headline_offset = 1 - min as i64;
        let mut numbering = [0usize; 19];
        let mut out = HashMap::new();
        for id in self.tree.descendants(root) {
            if self.tree.kind(id) != Some(HEADLINE) || self.info.ignore.contains(&id) {
                continue;
            }
            if !self.numbered_p(id) || self.footnote_section_p(id) {
                continue;
            }
            let rel = (self.relative_level(id) - 1).max(0) as usize;
            let mut v = Vec::new();
            for (idx, n) in numbering.iter_mut().enumerate() {
                if idx < rel {
                    v.push(*n);
                } else if idx == rel {
                    *n += 1;
                    v.push(*n);
                } else {
                    *n = 0;
                }
            }
            out.insert(id, v);
        }
        self.info.numbering = out;
    }

    // Transcoding.

    /// `org-export-data`.
    pub fn data(&mut self, id: Id) -> String {
        if let Some(s) = self.memo.get(&id) {
            return s.clone();
        }
        let backend = self.backend;
        let node_kind = self.tree.nodes[id].kind;
        let results: Option<String> = if self.info.ignore.contains(&id) {
            None
        } else {
            match node_kind {
                Kind::Raw => Some(self.tree.nodes[id].text.clone()),
                Kind::Text => {
                    let t = self.tree.nodes[id].text.clone();
                    let saved = self.current_text.replace(id);
                    let r = backend.plain_text(self, &t);
                    self.current_text = saved;
                    Some(r)
                }
                Kind::Node(DOCUMENT) => {
                    let children = self.tree.children(id).to_vec();
                    Some(children.into_iter().map(|c| self.data(c)).collect())
                }
                Kind::Node(k) => {
                    let archived = k == HEADLINE
                        && self.opt("with-archived-trees").sym() == Some("headline")
                        && self.headline(id).is_some_and(|h| h.is_archived());
                    if self.tree.children(id).is_empty() || archived {
                        if backend.has_transcoder(k) {
                            self.transcode_link_safe(id, None)
                        } else {
                            None
                        }
                    } else if !backend.has_transcoder(k) {
                        None
                    } else {
                        let greater = k.is_greater_element();
                        let objectp = !greater && recursive_object(k);
                        let contents = if greater || objectp {
                            let children = self.tree.children(id).to_vec();
                            children
                                .into_iter()
                                .map(|c| self.data(c))
                                .collect::<String>()
                        } else {
                            let ignore_first = k == PARAGRAPH
                                && self.tree.parent(id).is_some_and(|p| {
                                    matches!(self.tree.kind(p), Some(FOOTNOTE_DEFINITION | ITEM))
                                        && self.tree.children(p).first() == Some(&id)
                                        && self.contents_begin_is_begin(p, id)
                                });
                            let children = self.normalized_contents(id, ignore_first);
                            children
                                .into_iter()
                                .map(|c| self.data(c))
                                .collect::<String>()
                        };
                        let contents = if greater {
                            normalize_string(&contents)
                        } else {
                            contents
                        };
                        self.transcode_link_safe(id, Some(contents))
                    }
                }
            }
        };
        let out = match results {
            // Nothing: the spaces after an object stay when the text
            // before has none (`org-export--keep-spaces`).
            None => {
                let ignore = std::mem::take(&mut self.info.ignore);
                let spaces = self.keep_spaces(id, &ignore).unwrap_or_default();
                self.info.ignore = ignore;
                spaces
            }
            Some(r) => match node_kind {
                Kind::Node(DOCUMENT) | Kind::Text | Kind::Raw => r,
                _ => {
                    let blank = self.tree.nodes[id].post_blank;
                    let out = if self.tree.is_object(id) {
                        r + &" ".repeat(blank)
                    } else {
                        normalize_string(&r) + &"\n".repeat(blank)
                    };
                    backend.filter_output(self, id, out)
                }
            },
        };
        self.memo.insert(id, out.clone());
        out
    }

    /// Transcodes `id`, turning a broken link into a mark
    /// (`org-export-with-broken-links` set to `mark`).
    fn transcode_link_safe(&mut self, id: Id, contents: Option<String>) -> Option<String> {
        let backend = self.backend;
        let r = backend.transcode(self, id, contents);
        if self.tree.kind(id) == Some(LINK)
            && let Some(path) = self.tree.nodes[id].props.remove("broken")
        {
            return match self.opt("with-broken-links") {
                Value::Sym(s) if s == "mark" => {
                    let t = self.tree.text_node(format!("[BROKEN LINK: {path}]"), None);
                    Some(self.data(t))
                }
                Value::T => None,
                // `nil`: Emacs stops the export.
                _ => {
                    self.error
                        .get_or_insert_with(|| format!("Org export aborted: unable to resolve link {path:?} (see broken-links in #+OPTIONS)"));
                    None
                }
            };
        }
        r
    }

    /// Whether the contents of item or footnote definition `p` begin
    /// where its first child `id` begins (on the item's first line).
    fn contents_begin_is_begin(&self, p: Id, id: Id) -> bool {
        let (Some(ps), Some(cs)) = (self.syntax(p), self.syntax(id)) else {
            return false;
        };
        // `(= (org-element-contents-begin parent) (org-element-begin data))`
        // and the paragraph starts on the parent's first line.
        let first_line_end = ps
            .text()
            .to_string()
            .find('\n')
            .map_or(ps.text_range().end(), |i| {
                ps.text_range().start() + org_syntax::TextSize::from(i as u32)
            });
        ast::contents_range(ps).is_some_and(|r| r.start() == cs.text_range().start())
            && cs.text_range().start() < first_line_end
    }

    /// Forgets the transcoded output of `id`, after the tree changed.
    pub fn forget(&mut self, id: Id) {
        self.memo.remove(&id);
    }

    /// Transcodes a secondary string or any list of nodes.
    pub fn data_list(&mut self, ids: &[Id]) -> String {
        ids.iter().map(|&i| self.data(i)).collect()
    }

    /// `org-element-normalize-contents`: the children of `id`, with the
    /// indentation common to its lines removed from its plain text.
    fn normalized_contents(&mut self, id: Id, ignore_first: bool) -> Vec<Id> {
        // The minimal indentation.
        fn find_min(t: &Tree, id: Id, first: &mut bool, min: &mut usize) -> bool {
            for &c in t.children(id) {
                if *first {
                    *first = false;
                    if !t.is_text(c) {
                        return false;
                    }
                    let s = &t.nodes[c].text;
                    let blanks = s.len() - s.trim_start_matches([' ', '\t']).len();
                    if blanks == 0 {
                        return false;
                    }
                    let after = s[blanks..].chars().next();
                    if after != Some('\n') {
                        *min = (*min).min(width(&s[..blanks]));
                    }
                }
                if t.is_text(c) {
                    let s = &t.nodes[c].text;
                    let mut i = 0;
                    while let Some(nl) = s[i..].find('\n') {
                        let start = i + nl + 1;
                        let blanks =
                            s[start..].len() - s[start..].trim_start_matches([' ', '\t']).len();
                        let after = s[start + blanks..].chars().next();
                        if blanks == 0 {
                            if !matches!(after, None | Some('\n')) {
                                return false;
                            }
                        } else if after != Some('\n') {
                            *min = (*min).min(width(&s[start..start + blanks]));
                        }
                        i = start;
                    }
                } else if t.kind(c) == Some(LINE_BREAK) {
                    *first = true;
                } else if t.kind(c).is_some_and(recursive_object) && !find_min(t, c, first, min) {
                    return false;
                }
            }
            true
        }
        let mut first = !ignore_first;
        let mut min = usize::MAX;
        let ok = find_min(&self.tree, id, &mut first, &mut min);
        if !ok || min == 0 || min == usize::MAX {
            return self.tree.children(id).to_vec();
        }
        // The text with `min` fewer columns of indentation, changed in
        // place as `org-element-normalize-contents` does.
        fn rebuild(ex: &mut Exporter<'_>, id: Id, first: &mut bool, min: usize) {
            for c in ex.tree.children(id).to_vec() {
                let at_first = std::mem::take(first);
                if ex.tree.is_text(c) {
                    let s = ex.tree.nodes[c].text.clone();
                    let mut new = String::new();
                    let mut rest = s.as_str();
                    let mut line_start = at_first;
                    loop {
                        if line_start {
                            let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                            let after = rest[blanks..].chars().next();
                            if blanks > 0 {
                                if after != Some('\n') {
                                    let w = width(&rest[..blanks]);
                                    new.push_str(&" ".repeat(w.saturating_sub(min)));
                                }
                                rest = &rest[blanks..];
                            }
                        }
                        match rest.find('\n') {
                            Some(i) => {
                                new.push_str(&rest[..=i]);
                                rest = &rest[i + 1..];
                                line_start = true;
                            }
                            None => {
                                new.push_str(rest);
                                break;
                            }
                        }
                    }
                    // A new string in the same place, as Emacs builds the
                    // contents again: lists taken before keep the old one
                    // (inline footnote definitions in the footnote cache).
                    if new != s {
                        let mut node = ex.tree.nodes[c].clone();
                        node.text = new;
                        ex.tree.nodes.push(node);
                        let fresh = ex.tree.nodes.len() - 1;
                        if let Some(slot) = ex.tree.nodes[id].children.iter_mut().find(|x| **x == c)
                        {
                            *slot = fresh;
                        }
                    }
                } else if ex.tree.kind(c) == Some(LINE_BREAK) {
                    *first = true;
                } else if ex.tree.kind(c).is_some_and(recursive_object) {
                    rebuild(ex, c, first, min);
                }
            }
        }
        let mut first = !ignore_first;
        rebuild(self, id, &mut first, min);
        self.tree.children(id).to_vec()
    }

    // Headlines.

    /// A node's syntax node.
    pub fn syntax(&self, id: Id) -> Option<&SyntaxNode> {
        self.tree.syntax(id)
    }

    /// The headline (or inlinetask) at `id`.
    pub fn headline(&self, id: Id) -> Option<ast::Headline> {
        let s = self.syntax(id)?;
        match s.kind() {
            HEADLINE => ast::AstNode::cast(s.clone()),
            _ => None,
        }
    }

    /// The level as written (stars, or halved with `#+STARTUP: odd`).
    pub fn true_level(&self, id: Id) -> usize {
        match self.syntax(id) {
            Some(s) if s.kind() == HEADLINE => ast::AstNode::cast(s.clone())
                .map(|h: ast::Headline| h.level(&self.ctx))
                .unwrap_or(1),
            Some(s) if s.kind() == INLINETASK => ast::AstNode::cast(s.clone())
                .map(|h: ast::Inlinetask| h.level(&self.ctx))
                .unwrap_or(1),
            _ => 1,
        }
    }

    /// `org-export-get-relative-level`.
    pub fn relative_level(&self, id: Id) -> i64 {
        self.true_level(id) as i64 + self.info.headline_offset
    }

    /// `org-export-low-level-p`: how far below `:headline-levels`.
    pub fn low_level_p(&self, id: Id) -> Option<i64> {
        let limit = self.opt("headline-levels").int()?;
        let level = self.relative_level(id);
        (level > limit).then_some(level - limit)
    }

    /// `org-export-numbered-headline-p`.
    pub fn numbered_p(&self, id: Id) -> bool {
        if self
            .node_property(id, "UNNUMBERED", true)
            .is_some_and(|v| v != "nil")
        {
            return false;
        }
        match self.opt("section-numbers") {
            Value::Int(n) => self.relative_level(id) <= n,
            v => v.truthy(),
        }
    }

    /// `org-export-get-headline-number`.
    pub fn headline_number(&self, id: Id) -> Option<Vec<usize>> {
        if !self.numbered_p(id) {
            return None;
        }
        self.info.numbering.get(&id).cloned()
    }

    /// Whether the headline is the footnote section.
    pub fn footnote_section_p(&self, id: Id) -> bool {
        self.headline(id)
            .is_some_and(|h| h.is_footnote_section(&self.ctx))
    }

    /// The node's own tags.
    pub fn own_tags(&self, id: Id) -> Vec<String> {
        match self.syntax(id) {
            Some(s) if s.kind() == HEADLINE => ast::AstNode::cast(s.clone())
                .map(|h: ast::Headline| h.tags())
                .unwrap_or_default(),
            Some(s) if s.kind() == INLINETASK => ast::AstNode::cast(s.clone())
                .map(|h: ast::Inlinetask| h.tags())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// `org-export-get-tags`.
    pub fn tags(&self, id: Id, remove: &[String], inherited: bool) -> Vec<String> {
        let mut tags = self.own_tags(id);
        if inherited {
            let mut list = tags.clone();
            for a in self.tree.ancestors(id) {
                if matches!(self.tree.kind(a), Some(HEADLINE | INLINETASK)) {
                    for t in self.own_tags(a) {
                        if !list.contains(&t) {
                            list.insert(0, t);
                        }
                    }
                }
            }
            let mut all = self.info.filetags.clone();
            for t in list {
                if !all.contains(&t) {
                    all.push(t);
                }
            }
            tags = all;
        }
        tags.retain(|t| !remove.contains(t));
        tags
    }

    /// A headline's node property (`:CUSTOM_ID`), inherited if asked.
    pub fn node_property(&self, id: Id, prop: &str, inherited: bool) -> Option<String> {
        let own = |h: Id| -> Option<String> {
            let s = self.syntax(h)?;
            let props: Vec<(String, String)> = match s.kind() {
                HEADLINE => ast::AstNode::cast(s.clone()).map(|h: ast::Headline| h.properties())?,
                INLINETASK => {
                    ast::AstNode::cast(s.clone()).map(|h: ast::Inlinetask| h.properties())?
                }
                _ => return None,
            };
            props
                .into_iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(prop))
                .map(|(_, v)| v)
        };
        let start = if matches!(self.tree.kind(id), Some(HEADLINE)) {
            Some(id)
        } else {
            self.tree.ancestor(id, HEADLINE)
        };
        let start = start?;
        if !inherited {
            return own(start);
        }
        std::iter::once(start)
            .chain(self.tree.ancestors(start))
            .filter(|a| self.tree.kind(*a) == Some(HEADLINE))
            .find_map(own)
    }

    // References.

    /// `org-export-get-reference`: a unique `orgXXXXXXX` reference.
    pub fn reference(&mut self, id: Id) -> String {
        if let Some(r) = self.refs.get(&id) {
            return r.clone();
        }
        loop {
            // A linear congruential sequence of 28-bit numbers.
            self.ref_state = self
                .ref_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let n = (self.ref_state >> 36) & 0xfff_ffff;
            let r = format!("org{n:07x}");
            if !self.refs.values().any(|v| *v == r) {
                self.refs.insert(id, r.clone());
                return r;
            }
        }
    }

    // Footnotes.

    fn footnote_label(&self, id: Id) -> Option<String> {
        let s = self.syntax(id)?;
        match s.kind() {
            FOOTNOTE_REFERENCE => {
                ast::AstNode::cast(s.clone()).and_then(|f: ast::FootnoteReference| f.label())
            }
            FOOTNOTE_DEFINITION => {
                ast::AstNode::cast(s.clone()).map(|f: ast::FootnoteDefinition| f.label())
            }
            _ => None,
        }
    }

    fn footnote_is_standard(&self, id: Id) -> bool {
        self.syntax(id)
            .and_then(|s| ast::AstNode::cast(s.clone()))
            .is_some_and(|f: ast::FootnoteReference| !f.is_inline())
    }

    /// `org-export-get-footnote-definition`: the contents of a
    /// reference's definition.
    pub fn footnote_definition(&mut self, id: Id) -> Option<Vec<Id>> {
        let Some(label) = self.footnote_label(id) else {
            return Some(self.tree.children(id).to_vec());
        };
        // Each label's definition is taken at its first lookup, with its
        // contents as they are then (`:footnote-definition-cache`).
        if let Some(d) = self.footnote_defs.as_ref().and_then(|m| m.get(&label)) {
            return Some(d.clone());
        }
        let found = self
            .tree
            .descendants(self.tree.root)
            .into_iter()
            .find(|&d| {
                let k = self.tree.kind(d);
                (k == Some(FOOTNOTE_DEFINITION)
                    || (k == Some(FOOTNOTE_REFERENCE) && !self.footnote_is_standard(d)))
                    && self.footnote_label(d).as_deref() == Some(label.as_str())
            });
        // As Emacs, which stops the export there.
        let Some(found) = found else {
            self.error
                .get_or_insert_with(|| format!("Definition not found for footnote {label}"));
            return None;
        };
        let contents = self.tree.children(found).to_vec();
        self.footnote_defs
            .get_or_insert_with(HashMap::new)
            .insert(label.clone(), contents);
        self.footnote_defs.as_ref()?.get(&label).cloned()
    }

    /// Calls `f` on every footnote reference in reading order, entering
    /// definitions at their first reference
    /// (`org-export--footnote-reference-map`).
    fn footnote_reference_map(
        &mut self,
        data: &[Id],
        f: &mut dyn FnMut(&Exporter<'_>, Id) -> bool,
    ) -> bool {
        let mut seen: Vec<String> = Vec::new();
        self.footnote_search(data, &mut seen, f)
    }

    fn footnote_search(
        &mut self,
        data: &[Id],
        seen: &mut Vec<String>,
        f: &mut dyn FnMut(&Exporter<'_>, Id) -> bool,
    ) -> bool {
        for &d in data {
            // `org-element-map` over references, not entering
            // definitions.
            let mut stack = vec![d];
            while let Some(x) = stack.pop() {
                if self.info.ignore.contains(&x) {
                    continue;
                }
                let k = self.tree.kind(x);
                if k == Some(FOOTNOTE_DEFINITION) {
                    continue;
                }
                if k == Some(FOOTNOTE_REFERENCE) {
                    if f(self, x) {
                        return true;
                    }
                    let label = self.footnote_label(x);
                    let new = label.as_ref().is_none_or(|l| !seen.contains(l));
                    if new {
                        if let Some(l) = label {
                            seen.push(l);
                        }
                        if self.footnote_is_standard(x)
                            && let Some(def) = self.footnote_definition(x)
                            && self.footnote_search(&def, seen, f)
                        {
                            return true;
                        }
                    }
                }
                let n = &self.tree.nodes[x];
                let mut next: Vec<Id> = Vec::new();
                for (_, v) in &n.secondary {
                    next.extend(v);
                }
                next.extend(&n.children);
                stack.extend(next.into_iter().rev());
            }
        }
        false
    }

    /// `org-export-collect-footnote-definitions`: (number, label, contents).
    pub fn collect_footnote_definitions(&mut self) -> Vec<(usize, Option<String>, Vec<Id>)> {
        let root = self.tree.root;
        let mut refs: Vec<Id> = Vec::new();
        self.footnote_reference_map(&[root], &mut |_, r| {
            refs.push(r);
            false
        });
        let mut out = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        let mut n = 0;
        for r in refs {
            let l = self.footnote_label(r);
            if l.as_ref().is_none_or(|l| !labels.contains(l)) {
                n += 1;
                let def = self.footnote_definition(r).unwrap_or_default();
                out.push((n, l.clone(), def));
            }
            if let Some(l) = l {
                labels.push(l);
            }
        }
        out
    }

    /// `org-export-footnote-first-reference-p`.
    pub fn footnote_first_reference_p(&mut self, id: Id) -> bool {
        let Some(label) = self.footnote_label(id) else {
            return true;
        };
        let root = self.tree.root;
        let mut first = None;
        self.footnote_reference_map(&[root], &mut |ex, r| {
            if ex.footnote_label(r).as_deref() == Some(label.as_str()) {
                first = Some(r);
                return true;
            }
            false
        });
        first == Some(id)
    }

    /// `org-export-get-footnote-number`.
    pub fn footnote_number(&mut self, id: Id) -> usize {
        let label = self.footnote_label(id);
        let root = self.tree.root;
        let mut count = 0;
        let mut seen: Vec<String> = Vec::new();
        let mut found = None;
        self.footnote_reference_map(&[root], &mut |ex, r| {
            let l = ex.footnote_label(r);
            match (&l, &label) {
                (None, None) if r == id => {
                    found = Some(count + 1);
                    return true;
                }
                (Some(a), Some(b)) if a == b => {
                    found = Some(count + 1);
                    return true;
                }
                (None, _) => count += 1,
                (Some(a), _) if !seen.contains(a) => {
                    seen.push(a.clone());
                    count += 1;
                }
                _ => {}
            }
            false
        });
        found.unwrap_or(count + 1)
    }

    /// `org-export--install-footnote-definitions` for definitions outside
    /// the tree: nothing to do, since definitions are looked up in the
    /// whole document.
    fn install_missing_footnotes(&mut self) {}

    // Links.

    /// The link's properties.
    pub fn link_info(&self, id: Id) -> Option<ast::LinkInfo> {
        let s = self.syntax(id)?;
        let l: ast::Link = ast::AstNode::cast(s.clone())?;
        Some(l.info(&self.ctx))
    }

    /// Marks a link as broken for the transcoder's caller.
    pub fn broken_link(&mut self, id: Id, path: &str) {
        self.tree.nodes[id].props.insert("broken", path.to_string());
    }

    fn search_cells(&self, id: Id) -> Vec<(u8, String)> {
        let upcase_words = |s: &str| -> String {
            s.split_whitespace()
                .map(|w| w.to_uppercase())
                .collect::<Vec<_>>()
                .join(" ")
        };
        match self.tree.kind(id) {
            Some(HEADLINE) => {
                let raw = self.headline(id).map(|h| h.raw_value()).unwrap_or_default();
                let cleaned = strip_cookies(&raw);
                let t = upcase_words(&cleaned);
                vec![(0, t.clone()), (3, t)]
            }
            Some(TARGET) => {
                let v = self
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|t: ast::Target| t.value())
                    .unwrap_or_default();
                vec![(2, upcase_words(&v))]
            }
            Some(k) if k.is_element() => {
                let s = self.syntax(id).expect("syntax");
                let name = ast::affiliated_keywords(s)
                    .find(|k| k.key() == "NAME" || k.key() == "RESULTS")
                    .map(|k| k.value());
                match name {
                    Some(n) if !n.is_empty() => {
                        vec![(3, n.split_whitespace().collect::<Vec<_>>().join(" "))]
                    }
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// `org-export-resolve-fuzzy-link`: the target, named element or
    /// headline a fuzzy path points to.
    pub fn resolve_fuzzy(&mut self, path: &str) -> Option<Id> {
        let cells: Vec<(u8, String)> = if let Some(rest) = path.strip_prefix('*') {
            vec![(
                0,
                rest.split_whitespace()
                    .map(|w| w.to_uppercase())
                    .collect::<Vec<_>>()
                    .join(" "),
            )]
        } else if let Some(rest) = path.strip_prefix('#') {
            vec![(1, rest.to_string())]
        } else {
            let words = path.split_whitespace().collect::<Vec<_>>().join(" ");
            let up = path
                .split_whitespace()
                .map(|w| w.to_uppercase())
                .collect::<Vec<_>>()
                .join(" ");
            let mut v = vec![(2, words.clone()), (3, words), (2, up.clone()), (3, up)];
            v.dedup();
            v
        };
        if self.fuzzy_cache.is_none() {
            let mut table: HashMap<Vec<(u8, String)>, Vec<Id>> = HashMap::new();
            let mut single: HashMap<(u8, String), Vec<Id>> = HashMap::new();
            for d in self.tree.descendants(self.tree.root) {
                let k = self.tree.kind(d);
                if !(k == Some(TARGET) || k.is_some_and(|k| k.is_element())) {
                    continue;
                }
                for c in self.search_cells(d) {
                    single.entry(c).or_default().push(d);
                }
            }
            // Emacs pushes each match on the front of its cell's list:
            // later elements come first.
            for (c, mut v) in single {
                v.reverse();
                table.insert(vec![c], v);
            }
            self.fuzzy_cache = Some(table);
        }
        let table = self.fuzzy_cache.as_ref()?;
        let mut matches: Vec<Id> = Vec::new();
        for c in &cells {
            if let Some(v) = table.get(&vec![c.clone()]) {
                matches.extend(v);
            }
        }
        matches
            .iter()
            .copied()
            .find(|m| self.tree.kind(*m) != Some(HEADLINE))
            .or_else(|| matches.first().copied())
    }

    /// `org-export-resolve-id-link` for `#custom-id` and `id:` links in
    /// the document.
    pub fn resolve_id(&self, id_value: &str) -> Option<Id> {
        self.tree
            .descendants(self.tree.root)
            .into_iter()
            .find(|&d| {
                self.tree.kind(d) == Some(HEADLINE)
                    && (self.node_property(d, "ID", false).as_deref() == Some(id_value)
                        || self.node_property(d, "CUSTOM_ID", false).as_deref() == Some(id_value))
            })
    }

    /// `org-export-resolve-radio-link`.
    pub fn resolve_radio(&self, path: &str) -> Option<Id> {
        let clean = |s: &str| {
            s.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let want = clean(path);
        self.tree
            .descendants(self.tree.root)
            .into_iter()
            .find(|&d| {
                self.tree.kind(d) == Some(RADIO_TARGET)
                    && self
                        .syntax(d)
                        .and_then(|s| ast::AstNode::cast(s.clone()))
                        .is_some_and(|r: ast::RadioTarget| clean(&r.value()) == want)
            })
    }

    // Tables.

    /// The cells of a row (ids of TABLE_CELL nodes, ignored ones too).
    pub fn row_cells(&self, row: Id) -> Vec<Id> {
        self.tree.children(row).to_vec()
    }

    /// Whether `row` is a horizontal rule.
    pub fn rule_row_p(&self, row: Id) -> bool {
        self.syntax(row)
            .and_then(|s| ast::AstNode::cast(s.clone()))
            .is_some_and(|r: ast::TableRow| r.is_rule())
    }

    fn cell_text(&self, cell: Id) -> String {
        let s = self.tree.source(cell);
        s.trim_matches(|c: char| c == '|' || c.is_whitespace())
            .to_string()
    }

    /// What export needs of a table, computed once.
    fn table_info(&self, table: Id) -> std::rc::Rc<TableInfo> {
        if let Some(t) = self.tables.borrow().get(&table) {
            return t.clone();
        }
        let rows: Vec<Id> = self.tree.children(table).to_vec();
        let rule: Vec<bool> = rows.iter().map(|&r| self.rule_row_p(r)).collect();
        let texts: Vec<Vec<String>> = rows
            .iter()
            .map(|&r| {
                self.tree
                    .children(r)
                    .iter()
                    .map(|&c| self.cell_text(c))
                    .collect()
            })
            .collect();
        // `org-export-table-has-special-column-p`.
        let mut special_column = false;
        let mut plain = false;
        for (i, t) in texts.iter().enumerate() {
            if rule[i] {
                continue;
            }
            match t.first().map(String::as_str) {
                Some("/" | "#" | "!" | "$" | "*" | "_" | "^") => special_column = true,
                Some("") | None => {}
                Some(_) => plain = true,
            }
        }
        let special_column = special_column && !plain;
        // `org-export-table-row-is-special-p`.
        let special: Vec<bool> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if rule[i] {
                    return false;
                }
                let first = t.first().map(String::as_str).unwrap_or("");
                if first == "/" || (special_column && matches!(first, "^" | "_" | "$" | "!")) {
                    return true;
                }
                let mut cookie = false;
                for c in t {
                    if c.is_empty() {
                        continue;
                    }
                    if is_cookie(c) {
                        cookie = true;
                    } else {
                        return false;
                    }
                }
                cookie
            })
            .collect();
        // Groups of the rows that are exported.
        let mut groups = HashMap::new();
        let mut group = 0;
        let mut prev_rule = true;
        for (i, &r) in rows.iter().enumerate() {
            if self.info.ignore.contains(&r) {
                continue;
            }
            if rule[i] {
                prev_rule = true;
                continue;
            }
            if prev_rule {
                group += 1;
                prev_rule = false;
            }
            groups.insert(r, group);
        }
        // `org-export-table-has-header-p`: a rule after the first group,
        // and data after it.
        let mut has_header = false;
        {
            let mut seen_rule = false;
            let mut data_before = false;
            for (i, &r) in rows.iter().enumerate() {
                if self.info.ignore.contains(&r) {
                    continue;
                }
                if rule[i] {
                    if data_before {
                        seen_rule = true;
                    }
                } else if seen_rule {
                    has_header = true;
                    break;
                } else {
                    data_before = true;
                }
            }
        }
        // Column alignments (`org-export-table-cell-alignment`): the last
        // cookie of the column in a special row, else numbers make it
        // right-aligned (an empty cell after a number counts as one).
        let width = texts.iter().map(Vec::len).max().unwrap_or(0);
        let mut align = Vec::with_capacity(width);
        for col in 0..width {
            let mut cookie: Option<&'static str> = None;
            let (mut numbers, mut total) = (0usize, 0usize);
            let mut prev_number = false;
            for (i, t) in texts.iter().enumerate() {
                if special[i] {
                    if let Some(c) = t.get(col)
                        && is_cookie(c)
                    {
                        match c[1..].chars().next() {
                            Some('l') => cookie = Some("left"),
                            Some('r') => cookie = Some("right"),
                            Some('c') => cookie = Some("center"),
                            _ => {}
                        }
                    }
                } else if rule[i] {
                } else if cookie.is_none() {
                    let v = t.get(col).map(String::as_str).unwrap_or("");
                    total += 1;
                    if is_number(v) || (v.is_empty() && prev_number) {
                        prev_number = true;
                        numbers += 1;
                    } else {
                        prev_number = false;
                    }
                }
            }
            let a = cookie.unwrap_or(if total > 0 && numbers as f64 / total as f64 >= 0.5 {
                "right"
            } else {
                "left"
            });
            align.push(a);
        }
        // Width cookies, the last one of the column.
        let mut widths = vec![None; width];
        for (i, t) in texts.iter().enumerate() {
            if !special[i] {
                continue;
            }
            for (col, c) in t.iter().enumerate() {
                let digits = c
                    .strip_prefix('<')
                    .and_then(|x| x.strip_suffix('>'))
                    .map(|x| x.trim_start_matches(['l', 'r', 'c']))
                    .filter(|d| !d.is_empty() && d.chars().all(|ch| ch.is_ascii_digit()));
                if let Some(d) = digits
                    && let Ok(w) = d.parse::<usize>()
                {
                    widths[col] = Some(w);
                }
            }
        }
        let info = std::rc::Rc::new(TableInfo {
            special_column,
            special_rows: rows
                .iter()
                .zip(&special)
                .filter(|(_, s)| **s)
                .map(|(r, _)| *r)
                .collect(),
            groups,
            has_header,
            align,
            widths,
        });
        self.tables.borrow_mut().insert(table, info.clone());
        info
    }

    /// `org-export-table-has-special-column-p`.
    pub fn table_has_special_column(&self, table: Id) -> bool {
        self.table_info(table).special_column
    }

    /// `org-export-table-row-is-special-p`.
    pub fn table_row_is_special(&self, row: Id) -> bool {
        let Some(table) = self.tree.parent(row) else {
            return false;
        };
        self.table_info(table).special_rows.contains(&row)
    }

    /// The rows of a table that are exported (special rows left out).
    pub fn table_rows(&self, table: Id) -> Vec<Id> {
        self.tree
            .children(table)
            .iter()
            .copied()
            .filter(|r| !self.info.ignore.contains(r))
            .collect()
    }

    /// `org-export-table-row-group`: the row's group number, from 1, or
    /// `None` for rules.
    pub fn row_group(&self, row: Id) -> Option<usize> {
        let table = self.tree.parent(row)?;
        self.table_info(table).groups.get(&row).copied()
    }

    /// `org-export-table-has-header-p`.
    pub fn table_has_header(&self, table: Id) -> bool {
        self.table_info(table).has_header
    }

    /// `org-export-table-row-in-header-p`.
    pub fn row_in_header(&self, row: Id) -> bool {
        let Some(table) = self.tree.parent(row) else {
            return false;
        };
        self.table_has_header(table) && self.row_group(row) == Some(1)
    }

    /// The cell's alignment (`org-export-table-cell-alignment`): `left`,
    /// `right` or `center`.
    pub fn cell_alignment(&self, cell: Id) -> &'static str {
        let Some(row) = self.tree.parent(cell) else {
            return "left";
        };
        let Some(table) = self.tree.parent(row) else {
            return "left";
        };
        let col = self
            .tree
            .children(row)
            .iter()
            .position(|c| *c == cell)
            .unwrap_or(0);
        self.table_info(table)
            .align
            .get(col)
            .copied()
            .unwrap_or("left")
    }

    /// `org-export-table-cell-width`: the width cookie of the cell's
    /// column, if it has one.
    pub fn cell_cookie_width(&self, cell: Id) -> Option<usize> {
        let row = self.tree.parent(cell)?;
        let table = self.tree.parent(row)?;
        let col = self.tree.children(row).iter().position(|c| *c == cell)?;
        self.table_info(table).widths.get(col).copied().flatten()
    }

    /// The column of a cell, among exported cells.
    pub fn cell_column(&self, cell: Id) -> usize {
        let Some(row) = self.tree.parent(cell) else {
            return 0;
        };
        self.tree
            .children(row)
            .iter()
            .filter(|c| !self.info.ignore.contains(c))
            .position(|c| *c == cell)
            .unwrap_or(0)
    }
}

/// `org-table-number-regexp` (the default):
/// `^\([<>]?[-+^.0-9]*[0-9][-+^.0-9eEdDx()%:]*\|[<>]?[-+]?0[xX][[:xdigit:].]+\|[<>]?[-+]?[0-9]+#[0-9a-zA-Z.]+\|nan\|[-+u]?inf\)$`.
pub fn is_number(s: &str) -> bool {
    if s == "nan" {
        return true;
    }
    if matches!(s, "inf" | "-inf" | "+inf" | "uinf") {
        return true;
    }
    let t = s.strip_prefix(['<', '>']).unwrap_or(s);
    // `[-+]?0[xX][[:xdigit:].]+`
    {
        let u = t.strip_prefix(['-', '+']).unwrap_or(t);
        if let Some(h) = u.strip_prefix("0x").or_else(|| u.strip_prefix("0X"))
            && !h.is_empty()
            && h.chars().all(|c| c.is_ascii_hexdigit() || c == '.')
        {
            return true;
        }
        // `[-+]?[0-9]+#[0-9a-zA-Z.]+`
        if let Some((a, b)) = u.split_once('#')
            && !a.is_empty()
            && a.chars().all(|c| c.is_ascii_digit())
            && !b.is_empty()
            && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        {
            return true;
        }
    }
    // `[-+^.0-9]*[0-9][-+^.0-9eEdDx()%:]*`: some digit such that what is
    // before it is from the first set and what follows from the second.
    let first = |c: char| matches!(c, '-' | '+' | '^' | '.') || c.is_ascii_digit();
    let rest = |c: char| {
        matches!(
            c,
            '-' | '+' | '^' | '.' | 'e' | 'E' | 'd' | 'D' | 'x' | '(' | ')' | '%' | ':'
        ) || c.is_ascii_digit()
    };
    let chars: Vec<char> = t.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if !c.is_ascii_digit() {
            if !first(*c) {
                return false;
            }
            continue;
        }
        return chars[i + 1..].iter().all(|c| rest(*c));
    }
    false
}

/// `\`<[lrc]?\([0-9]+\)?>\'`.
fn is_cookie(t: &str) -> bool {
    let Some(inner) = t.strip_prefix('<').and_then(|x| x.strip_suffix('>')) else {
        return false;
    };
    let rest = inner.strip_prefix(['l', 'r', 'c']).unwrap_or(inner);
    rest.chars().all(|c| c.is_ascii_digit())
}

/// Statistics cookies in a title replaced by a space.
fn strip_cookies(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'[' {
            let rest = &s[i + 1..];
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            let after = &rest[digits..];
            let close = if after.starts_with("%]") {
                Some(digits + 2)
            } else if let Some(a) = after.strip_prefix('/') {
                let d2 = a.chars().take_while(|c| c.is_ascii_digit()).count();
                a[d2..].starts_with(']').then_some(digits + 1 + d2 + 1)
            } else {
                None
            };
            if let Some(len) = close {
                out.push(' ');
                i += 1 + len;
                continue;
            }
        }
        let c = s[i..].chars().next().expect("a char");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The display width of blanks (tabs to the next multiple of 8).
fn width(s: &str) -> usize {
    let mut w = 0;
    for c in s.chars() {
        if c == '\t' {
            w = (w / 8 + 1) * 8;
        } else {
            w += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        }
    }
    w
}

/// `Secondary` for back-ends.
pub use crate::tree::Secondary as Sec;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(normalize_string("a\n  \n\n"), "a\n");
        assert_eq!(normalize_string("a"), "a\n");
        assert_eq!(normalize_string(""), "");
        assert!(is_number("3") && is_number("-2.5e3") && is_number("10%"));
        assert!(!is_number("apple") && !is_number("."));
        assert_eq!(strip_cookies("Task [2/3] done [50%]"), "Task   done  ");
    }
}
