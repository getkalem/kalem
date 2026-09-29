//! Completions in LaTeX documents (T2.7h.17): command names (a built-in
//! list and the document's own macros), environment names after
//! `\begin{` (the environment completed with its `\end`) and `\end{`,
//! citation keys from the bibliography (matched fuzzily on key, authors
//! and title), labels for the reference commands with what they number,
//! files for `\input`, `\include` and `\includegraphics`, and packages
//! for `\usepackage`.

use std::path::Path;

use crate::DocumentState;
use crate::completers::{Cancel, Completer, Context, Item, Kind, Trigger};
use crate::mode::DocumentMode;

/// Commands offered after `\`, with their arguments' shapes from the
/// parser's signature table.
const COMMANDS: &[&str] = &[
    "section",
    "subsection",
    "subsubsection",
    "paragraph",
    "chapter",
    "part",
    "emph",
    "textbf",
    "textit",
    "texttt",
    "textsc",
    "underline",
    "label",
    "ref",
    "eqref",
    "pageref",
    "autoref",
    "cref",
    "Cref",
    "cite",
    "citep",
    "citet",
    "parencite",
    "textcite",
    "autocite",
    "footcite",
    "footnote",
    "caption",
    "includegraphics",
    "input",
    "include",
    "item",
    "frac",
    "sqrt",
    "sum",
    "int",
    "prod",
    "lim",
    "infty",
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "lambda",
    "mu",
    "pi",
    "sigma",
    "theta",
    "omega",
    "left",
    "right",
    "mathbb",
    "mathcal",
    "mathrm",
    "mathbf",
    "text",
    "usepackage",
    "documentclass",
    "newcommand",
    "renewcommand",
    "newenvironment",
    "newtheorem",
    "DeclareMathOperator",
    "title",
    "author",
    "date",
    "maketitle",
    "tableofcontents",
    "appendix",
    "bibliography",
    "bibliographystyle",
    "addbibresource",
    "centering",
    "hspace",
    "vspace",
    "noindent",
    "newline",
    "url",
    "href",
    "ldots",
    "cdots",
    "quad",
    "qquad",
    "textcolor",
    "color",
    "hline",
    "toprule",
    "midrule",
    "bottomrule",
    "multicolumn",
    "multirow",
    "setlength",
    "setcounter",
    "tag",
    "nonumber",
    "notag",
    "begin",
    "end",
    "verb",
    "lstinline",
    "today",
    "LaTeX",
    "TeX",
    "graphicspath",
    "numberwithin",
    "binom",
    "overline",
    "hat",
    "vec",
    "dot",
    "partial",
    "nabla",
    "times",
    "cdot",
    "leq",
    "geq",
    "neq",
    "approx",
    "equiv",
    "in",
    "subset",
    "cup",
    "cap",
    "forall",
    "exists",
    "rightarrow",
    "Rightarrow",
    "mapsto",
];

/// Environments offered after `\begin{`.
const ENVIRONMENTS: &[&str] = &[
    "document",
    "itemize",
    "enumerate",
    "description",
    "equation",
    "equation*",
    "align",
    "align*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "split",
    "cases",
    "matrix",
    "pmatrix",
    "bmatrix",
    "figure",
    "figure*",
    "table",
    "table*",
    "tabular",
    "tabularx",
    "center",
    "flushleft",
    "flushright",
    "quote",
    "quotation",
    "abstract",
    "verbatim",
    "lstlisting",
    "minted",
    "theorem",
    "lemma",
    "proof",
    "definition",
    "example",
    "remark",
    "corollary",
    "proposition",
    "minipage",
    "subfigure",
    "thebibliography",
    "frame",
    "columns",
    "column",
];

/// Packages offered for `\usepackage`.
const PACKAGES: &[&str] = &[
    "amsmath",
    "amssymb",
    "amsthm",
    "mathtools",
    "graphicx",
    "hyperref",
    "cleveref",
    "geometry",
    "babel",
    "polyglossia",
    "fontspec",
    "inputenc",
    "fontenc",
    "lmodern",
    "microtype",
    "csquotes",
    "biblatex",
    "natbib",
    "booktabs",
    "tabularx",
    "longtable",
    "multirow",
    "array",
    "xcolor",
    "tikz",
    "pgfplots",
    "listings",
    "minted",
    "siunitx",
    "physics",
    "bm",
    "enumitem",
    "caption",
    "subcaption",
    "float",
    "wrapfig",
    "url",
    "xparse",
    "etoolbox",
    "fancyhdr",
    "titlesec",
    "setspace",
    "parskip",
    "pdfpages",
    "import",
    "subfiles",
    "algorithm2e",
    "algorithmicx",
    "glossaries",
    "makeidx",
    "todonotes",
    "lipsum",
    "appendix",
    "tocbibind",
    "comment",
    "xspace",
];

/// Whether `pattern`'s characters occur in `text` in order (fuzzy).
fn fuzzy(pattern: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    pattern
        .to_lowercase()
        .chars()
        .all(|p| chars.any(|c| c == p))
}

/// The command whose argument the cursor is in on `line`, and the
/// argument's text before the cursor: `\cite[p.~3]{knu` gives `cite` and
/// `knu`.
fn argument(line: &str) -> Option<(&str, &str)> {
    let open = line.rfind('{')?;
    let arg = &line[open + 1..];
    if arg.contains('}') {
        return None;
    }
    let mut head = line[..open].trim_end();
    // Optional arguments and a star between the command and its brace.
    while head.ends_with(']') {
        head = head[..head.rfind('[')?].trim_end();
    }
    let head = head.strip_suffix('*').unwrap_or(head);
    let start = head.rfind('\\')?;
    let name = &head[start + 1..];
    name.chars()
        .all(|c| c.is_ascii_alphabetic())
        .then_some((name, arg))
}

/// LaTeX's completer.
pub(crate) struct LatexCompleter;

impl LatexCompleter {
    fn commands(
        &self,
        ctx: &Context,
        doc: Option<&DocumentState>,
        prefix: &str,
        start: usize,
    ) -> Vec<Item> {
        let mut names: Vec<String> = COMMANDS.iter().map(|s| s.to_string()).collect();
        if let Some(l) = doc.and_then(DocumentState::latex) {
            for m in &l.model().macros {
                names.push(m.name.trim_start_matches('\\').to_string());
            }
        }
        names.sort();
        names.dedup();
        names
            .into_iter()
            .filter(|n| n.starts_with(prefix) && n != prefix)
            .map(|n| {
                let sig = latex_syntax::signatures::command(&n);
                let braces = sig.matches('m').count();
                let insert = format!("{n}{}", "{}".repeat(braces));
                let mut it = Item::new(format!("\\{n}"), insert, start..ctx.point, Kind::Keyword);
                it.cursor = n.len() + usize::from(braces > 0);
                it.detail = (0..braces).map(|_| "{…}").collect();
                it.source = "latex";
                it
            })
            .collect()
    }

    fn environments(
        &self,
        ctx: &Context,
        doc: Option<&DocumentState>,
        prefix: &str,
        begin: bool,
    ) -> Vec<Item> {
        let start = ctx.point - prefix.len();
        let rest = ctx.slice(ctx.point..ctx.point + 1);
        if !begin {
            // `\end{`: the environment left open first.
            let open = doc.and_then(DocumentState::latex).and_then(|l| {
                let root = l.parse().syntax();
                let t = latex_syntax::token_before(&root, ctx.point.saturating_sub(1))?;
                t.parent_ancestors()
                    .filter(|a| a.kind() == latex_syntax::SyntaxKind::ENVIRONMENT)
                    .find_map(|a| latex_syntax::name(&a))
            });
            return open
                .into_iter()
                .filter(|n| n.starts_with(prefix))
                .map(|n| {
                    let close = if rest == "}" { "" } else { "}" };
                    let mut it = Item::new(
                        n.clone(),
                        format!("{n}{close}"),
                        start..ctx.point,
                        Kind::Keyword,
                    );
                    it.source = "latex";
                    it
                })
                .collect();
        }
        let mut names: Vec<String> = ENVIRONMENTS.iter().map(|s| s.to_string()).collect();
        if let Some(l) = doc.and_then(DocumentState::latex) {
            let m = l.model();
            names.extend(m.environments.iter().map(|e| e.name.clone()));
            names.extend(m.theorem_kinds.iter().map(|k| k.env.clone()));
        }
        names.sort();
        names.dedup();
        let line = ctx.line_before();
        let indent: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        names
            .into_iter()
            .filter(|n| n.starts_with(prefix))
            .map(|n| {
                // The environment with its `\end` and a line between.
                let (insert, cursor) = if rest == "}" {
                    (n.clone(), n.len())
                } else {
                    let item = if matches!(n.as_str(), "itemize" | "enumerate" | "description") {
                        "\\item "
                    } else {
                        ""
                    };
                    let body = format!("{n}}}\n{indent}  {item}");
                    (format!("{body}\n{indent}\\end{{{n}}}"), body.len())
                };
                let mut it = Item::new(n, insert, start..ctx.point, Kind::Snippet);
                it.cursor = cursor;
                it.source = "latex";
                it
            })
            .collect()
    }

    fn keys(
        &self,
        ctx: &Context,
        doc: Option<&DocumentState>,
        command: &str,
        arg: &str,
    ) -> Vec<Item> {
        let key = arg.rsplit(',').next().unwrap_or("").trim_start();
        let start = ctx.point - key.len();
        let Some(l) = doc.and_then(DocumentState::latex) else {
            return Vec::new();
        };
        let model = l.model();
        if latex_syntax::signatures::command(command) == "*oom" {
            let base = ctx.path.as_deref().and_then(Path::parent);
            let files: Vec<std::path::PathBuf> = model
                .bibliography
                .iter()
                .flat_map(|b| b.files.iter())
                .map(|f| base.map_or_else(|| std::path::PathBuf::from(f), |d| d.join(f)))
                .collect();
            let bib = crate::cite::load(&files);
            return bib
                .entries()
                .iter()
                .filter(|e| {
                    fuzzy(key, &e.key)
                        || fuzzy(key, e.field("author").unwrap_or(""))
                        || fuzzy(key, e.field("title").unwrap_or(""))
                })
                .map(|e| {
                    let mut it =
                        Item::new(e.key.clone(), e.key.clone(), start..ctx.point, Kind::Link);
                    it.detail = crate::cite::describe(e);
                    it.source = "latex";
                    it
                })
                .collect();
        }
        model
            .labels
            .iter()
            .filter(|lab| lab.name.starts_with(key) || fuzzy(key, &lab.name))
            .map(|lab| {
                let mut it = Item::new(
                    lab.name.clone(),
                    lab.name.clone(),
                    start..ctx.point,
                    Kind::Link,
                );
                let what = match &lab.target {
                    latex_model::Target::Section(_) => "section",
                    latex_model::Target::Equation => "equation",
                    latex_model::Target::Float(k) => k.as_str(),
                    latex_model::Target::Theorem(t) => t.as_str(),
                    latex_model::Target::Footnote => "footnote",
                    latex_model::Target::None => "",
                };
                it.detail = format!("{what} {}", lab.number.clone().unwrap_or_default())
                    .trim()
                    .to_string();
                it.source = "latex";
                it
            })
            .collect()
    }

    fn files(&self, ctx: &Context, command: &str, arg: &str) -> Vec<Item> {
        let Some(base) = ctx.path.as_deref().and_then(Path::parent) else {
            return Vec::new();
        };
        let (dir, name) = arg.rsplit_once('/').map_or(("", arg), |(d, n)| (d, n));
        let start = ctx.point - name.len();
        let Ok(entries) = std::fs::read_dir(base.join(dir)) else {
            return Vec::new();
        };
        let pictures = command == "includegraphics";
        let mut out: Vec<Item> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                if file.starts_with('.') || !file.starts_with(name) {
                    return None;
                }
                let is_dir = e.path().is_dir();
                let ext = Path::new(&file)
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let wanted = if pictures {
                    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "pdf" | "eps" | "svg")
                } else {
                    ext == "tex"
                };
                if !is_dir && !wanted {
                    return None;
                }
                // `\include` and `\input` without `.tex`, as usual.
                let insert = if is_dir {
                    format!("{file}/")
                } else if pictures {
                    file.clone()
                } else {
                    file.trim_end_matches(".tex").to_string()
                };
                let mut it = Item::new(insert.clone(), insert, start..ctx.point, Kind::Link);
                it.source = "latex";
                Some(it)
            })
            .collect();
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
    }
}

impl Completer for LatexCompleter {
    fn id(&self) -> &'static str {
        "latex"
    }

    fn priority(&self) -> i32 {
        10
    }

    fn applies(&self, ctx: &Context) -> bool {
        ctx.mode == DocumentMode::Latex
    }

    fn trigger(&self) -> Trigger {
        Trigger::Strings(&["\\", "{", ","])
    }

    fn complete(&self, ctx: &Context, doc: Option<&DocumentState>, _cancel: &Cancel) -> Vec<Item> {
        let line = ctx.line_before();
        if let Some((command, arg)) = argument(line) {
            return match command {
                "begin" => self.environments(ctx, doc, arg, true),
                "end" => self.environments(ctx, doc, arg, false),
                "input" | "include" | "includegraphics" | "subfile" => {
                    self.files(ctx, command, arg)
                }
                "usepackage" | "RequirePackage" => {
                    let name = arg.rsplit(',').next().unwrap_or("").trim_start();
                    PACKAGES
                        .iter()
                        .filter(|p| p.starts_with(name) && **p != name)
                        .map(|p| {
                            let mut it =
                                Item::new(*p, *p, ctx.point - name.len()..ctx.point, Kind::Keyword);
                            it.source = "latex";
                            it
                        })
                        .collect()
                }
                c if latex_syntax::signatures::command(c) == "*oom"
                    || matches!(
                        c,
                        "ref"
                            | "eqref"
                            | "pageref"
                            | "autoref"
                            | "cref"
                            | "Cref"
                            | "nameref"
                            | "vref"
                            | "Vref"
                    ) =>
                {
                    self.keys(ctx, doc, c, arg)
                }
                _ => Vec::new(),
            };
        }
        // `\name` being typed.
        let letters = line.len()
            - line
                .trim_end_matches(|c: char| c.is_ascii_alphabetic())
                .len();
        let before = &line[..line.len() - letters];
        if before.ends_with('\\') && !before.ends_with("\\\\") {
            let prefix = &line[line.len() - letters..];
            if prefix.is_empty() && !ctx.requested {
                return Vec::new();
            }
            return self.commands(ctx, doc, prefix, ctx.point - letters);
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments() {
        assert_eq!(argument("See \\cite[p.~3]{knu"), Some(("cite", "knu")));
        assert_eq!(argument("\\cite{a, b"), Some(("cite", "a, b")));
        assert_eq!(argument("\\section*{Ti"), Some(("section", "Ti")));
        assert_eq!(argument("\\emph{x} y"), None);
        assert!(fuzzy("knth", "Knuth84"));
    }

    fn labels_at(text: &str, dir: &Path) -> Vec<(String, String)> {
        let meta = crate::Metadata {
            path: Some(dir.join("p.tex")),
            mode: DocumentMode::Latex,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
        };
        let mut d = DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        );
        d.selection = org_edit::Selection::caret(text.len());
        crate::completers::Registry::with_builtins()
            .complete(&mut d, false, std::time::Duration::from_millis(200))
            .into_iter()
            .filter(|i| i.source == "latex")
            .map(|i| (i.label, i.insert))
            .collect()
    }

    #[test]
    fn completions() {
        let dir = std::env::temp_dir().join(format!("kalem-latex-complete-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ch")).unwrap();
        std::fs::write(dir.join("refs.bib"), "@book{knuth84, author = {Knuth, Donald}, title = {The TeXbook}, year = 1984}\n@book{lamport, author = {Lamport}, title = {LaTeX}, year = 1994}\n").unwrap();
        std::fs::write(dir.join("ch/one.tex"), "").unwrap();
        let head =
            "\\bibliography{refs}\n\\newcommand{\\RR}{\\mathbb{R}}\n\\section{A}\\label{sec:a}\n";
        let c = |s: &str| labels_at(&format!("{head}{s}"), &dir);
        let sec = c("\\subsec");
        assert!(
            sec.contains(&("\\subsection".to_string(), "subsection{}".to_string())),
            "{sec:?}"
        );
        // The document's macros too; what is typed in full is not offered.
        assert!(c("\\R").iter().any(|(l, _)| l == "\\RR"));
        assert!(!c("\\RR").iter().any(|(l, _)| l == "\\RR"));
        let env = c("\\begin{ali");
        assert_eq!(
            env[0],
            ("align".to_string(), "align}\n  \n\\end{align}".to_string())
        );
        assert_eq!(
            c("\\cite{knth"),
            [("knuth84".to_string(), "knuth84".to_string())]
        );
        assert_eq!(c("\\cite{knuth84, lam")[0].0, "lamport");
        assert_eq!(c("\\ref{sec")[0].0, "sec:a");
        assert_eq!(c("\\input{ch/"), [("one".to_string(), "one".to_string())]);
        assert!(c("\\usepackage{amss").iter().any(|(l, _)| l == "amssymb"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
