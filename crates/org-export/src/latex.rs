//! The LaTeX back-end, as `ox-latex.el` writes: the article, report and
//! book classes, `#+LATEX_CLASS`, `#+LATEX_HEADER`, `#+ATTR_LATEX`, the
//! default packages for pdfLaTeX, verbatim source blocks, and captions
//! and labels on figures, tables and equations. With
//! [`Latex::source_lines`], a `%% org:LINE` comment before each element of
//! a section gives the line of the Org file it comes from, so that LaTeX's
//! errors can be shown at the right place.

use org_syntax::SyntaxKind::{self, *};
use org_syntax::ast;

use crate::export::{Backend, Exporter, trim};
use crate::html::{self, read_attribute};
use crate::options::{Behavior, Value};
use crate::tree::{Id, Secondary};

/// The LaTeX back-end.
#[derive(Debug, Clone, Copy, Default)]
pub struct Latex {
    /// Write `%% org:LINE` before each element of a section.
    pub source_lines: bool,
}

/// Headline titles: underline, code and verbatim as a sectioning command
/// takes them (`section-backend` in `org-latex-headline`), and footnotes
/// left out for the short title.
#[derive(Debug, Clone, Copy)]
struct Title {
    footnotes: bool,
}

const TITLE: Title = Title { footnotes: true };
const TITLE_NO_FOOTNOTES: Title = Title { footnotes: false };

/// `org-latex-math-environments-re`.
const MATH_ENVIRONMENTS: &[&str] = &[
    "equation",
    "eqnarray",
    "math",
    "displaymath",
    "align",
    "gather",
    "multline",
    "flalign",
    "alignat",
    "xalignat",
    "xxalignat",
    "subequations",
    "dmath",
    "dseries",
    "dgroup",
    "darray",
    "empheq",
];

/// `org-latex-table-matrix-macros`.
const MATRIX_MACROS: &[(&str, &str)] = &[
    ("bordermatrix", "\\cr"),
    ("qbordermatrix", "\\cr"),
    ("kbordermatrix", "\\\\"),
];

/// `org-latex-default-packages-alist`: options, package, whether a
/// formula snippet needs it, and the compilers it is for.
const DEFAULT_PACKAGES: &[(&str, &str, bool, &[&str])] = &[
    ("", "amsmath", true, &["lualatex", "xetex"]),
    ("", "fontspec", true, &["lualatex", "xetex"]),
    ("AUTO", "inputenc", true, &["pdflatex"]),
    ("T1", "fontenc", true, &["pdflatex"]),
    ("", "graphicx", true, &[]),
    ("", "longtable", false, &[]),
    ("", "wrapfig", false, &[]),
    ("", "rotating", false, &[]),
    ("normalem", "ulem", true, &[]),
    ("", "amsmath", true, &["pdflatex"]),
    ("", "amssymb", true, &["pdflatex"]),
    ("", "capt-of", false, &[]),
    ("", "hyperref", false, &[]),
];

/// A LaTeX class: its name, its `\documentclass` line and its sectioning
/// commands, numbered and not.
type Class = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// `org-latex-classes`.
const CLASSES: &[Class] = &[
    (
        "article",
        "\\documentclass[11pt]{article}",
        &[
            ("\\section{%s}", "\\section*{%s}"),
            ("\\subsection{%s}", "\\subsection*{%s}"),
            ("\\subsubsection{%s}", "\\subsubsection*{%s}"),
            ("\\paragraph{%s}", "\\paragraph*{%s}"),
            ("\\subparagraph{%s}", "\\subparagraph*{%s}"),
        ],
    ),
    (
        "report",
        "\\documentclass[11pt]{report}",
        &[
            ("\\part{%s}", "\\part*{%s}"),
            ("\\chapter{%s}", "\\chapter*{%s}"),
            ("\\section{%s}", "\\section*{%s}"),
            ("\\subsection{%s}", "\\subsection*{%s}"),
            ("\\subsubsection{%s}", "\\subsubsection*{%s}"),
        ],
    ),
    (
        "book",
        "\\documentclass[11pt]{book}",
        &[
            ("\\part{%s}", "\\part*{%s}"),
            ("\\chapter{%s}", "\\chapter*{%s}"),
            ("\\section{%s}", "\\section*{%s}"),
            ("\\subsection{%s}", "\\subsection*{%s}"),
            ("\\subsubsection{%s}", "\\subsubsection*{%s}"),
        ],
    ),
];

/// `org-latex-language-alist`: code, Babel name, Babel name through ini
/// files only, alternative ini name, Polyglossia name and variant, and
/// the language's name.
#[rustfmt::skip]
const LANGUAGES: &[(&str, &str, &str, &str, &str, &str, &str)] = &[
    ("af", "afrikaans", "", "", "afrikaans", "", "Afrikaans"),
    ("am", "", "amharic", "", "amharic", "", "Amharic"),
    ("ar", "", "arabic", "", "arabic", "", "Arabic"),
    ("ast", "", "asturian", "", "asturian", "", "Asturian"),
    ("bg", "bulgarian", "", "", "bulgarian", "", "Bulgarian"),
    ("bn", "", "bengali", "", "bengali", "", "Bengali"),
    ("bo", "", "tibetan", "", "tibetan", "", "Tibetan"),
    ("br", "breton", "", "", "breton", "", "Breton"),
    ("ca", "catalan", "", "", "catalan", "", "Catalan"),
    ("cop", "", "coptic", "", "coptic", "", "Coptic"),
    ("cs", "czech", "", "", "czech", "", "Czech"),
    ("cy", "welsh", "", "", "welsh", "", "Welsh"),
    ("da", "danish", "", "", "danish", "", "Danish"),
    ("de", "ngerman", "", "german", "german", "german", "German"),
    ("de-de", "ngerman", "", "german", "german", "german", "German"),
    ("de-at", "naustrian", "", "german-austria", "german", "austrian", "German"),
    ("dsb", "lowersorbian", "", "lsorbian", "sorbian", "lower", "Lower Sorbian"),
    ("dv", "", "", "", "divehi", "", "Dhivehi"),
    ("el", "greek", "", "", "greek", "", "Greek"),
    ("el-polyton", "greek", "", "polytonicgreek", "greek", "polytonic", "Polytonic Greek"),
    ("en", "american", "", "english", "english", "", "English"),
    ("en-au", "australian", "", "", "english", "australian", "English"),
    ("en-gb", "british", "", "", "english", "uk", "English"),
    ("en-nz", "newzealand", "", "", "english", "newzealand", "English"),
    ("en-us", "american", "", "", "english", "usmax", "English"),
    ("eo", "esperanto", "", "", "esperanto", "", "Esperanto"),
    ("es", "spanish", "", "", "spanish", "", "Spanish"),
    ("es-mx", "spanishmx", "", "spanish-mexico", "spanish", "mexican", "Spanish"),
    ("et", "estonian", "", "", "estonian", "", "Estonian"),
    ("eu", "basque", "", "", "basque", "", "Basque"),
    ("fa", "", "persian", "", "persian", "", "Persian"),
    ("fi", "finnish", "", "", "finnish", "", "Finnish"),
    ("fr", "french", "", "", "french", "", "French"),
    ("fr-ca", "canadien", "", "french-canadian", "french", "canadian", "French"),
    ("fur", "friulian", "", "", "friulian", "", "Friulian"),
    ("ga", "irish", "", "", "irish", "", "Irish Gaelic"),
    ("gd", "scottish", "", "", "scottish", "", "Scottish Gaelic"),
    ("gl", "galician", "", "", "galician", "", "Galician"),
    ("he", "hebrew", "", "", "hebrew", "", "Hebrew"),
    ("hi", "", "hindi", "", "hindi", "", "Hindi"),
    ("hr", "croatian", "", "", "croatian", "", "Croatian"),
    ("hsb", "uppersorbian", "", "usorbian", "sorbian", "upper", "Upper Sorbian"),
    ("hu", "hungarian", "", "", "hungarian", "", "Magyar"),
    ("hy", "", "armenian", "", "armenian", "", "Armenian"),
    ("ia", "interlingua", "", "", "interlingua", "", "Interlingua"),
    ("id", "", "bahasai", "", "bahasai", "", "Indonesian"),
    ("is", "icelandic", "", "", "icelandic", "", "Icelandic"),
    ("it", "italian", "", "", "italian", "", "Italian"),
    ("kn", "", "kannada", "", "kannada", "", "Kannada"),
    ("la", "latin", "", "", "latin", "", "Latin"),
    ("la-classic", "classiclatin", "", "", "latin", "classic", "Classic Latin"),
    ("la-medieval", "medievallatin", "", "", "latin", "medieval", "Medieval Latin"),
    ("la-ecclesiastic", "ecclesiasticlatin", "", "", "latin", "ecclesiastic", "Ecclesiastic Latin"),
    ("lo", "", "lao", "", "lao", "", "Lao"),
    ("lt", "lithuanian", "", "", "lithuanian", "", "Lithuanian"),
    ("lv", "latvian", "", "", "latvian", "", "Latvian"),
    ("ml", "", "malayalam", "", "malayalam", "", "Malayalam"),
    ("mr", "", "maratih", "", "maratih", "", "Marathi"),
    ("nb", "norsk", "", "", "norsk", "", "Norwegian Bokmål"),
    ("nl", "dutch", "", "", "dutch", "", "Dutch"),
    ("nn", "nynorsk", "", "", "norwegian", "nynorsk", "Norwegian Nynorsk"),
    ("no", "norsk", "", "", "norsk", "", "Norwegian"),
    ("oc", "occitan", "", "", "occitan", "", "Occitan"),
    ("pl", "polish", "", "", "polish", "", "Polish"),
    ("pms", "piedmontese", "", "", "piedmontese", "", "Piedmontese"),
    ("pt", "portuges", "", "portuguese", "portuguese", "", "Portuges"),
    ("pt-br", "brazil", "", "", "portuguese", "brazilian", "Portuges"),
    ("rm", "romansh", "", "", "romansh", "", "Romansh"),
    ("ro", "romanian", "", "", "romanian", "", "Romanian"),
    ("ru", "russian", "", "", "russian", "", "Russian"),
    ("sa", "", "sanskrit", "", "sanskrit", "", "Sanskrit"),
    ("sk", "slovak", "", "", "slovak", "", "Slovak"),
    ("sl", "slovene", "", "slovenian", "slovenian", "", "Slovene"),
    ("sq", "albanian", "", "", "albanian", "", "Albanian"),
    ("sr", "serbian", "", "", "serbian", "", "Serbian"),
    ("sv", "swedish", "", "", "swedish", "", "Swedish"),
    ("syr", "", "syriac", "", "syriac", "", "Syriac"),
    ("ta", "", "tamil", "", "tamil", "", "Tamil"),
    ("te", "", "telugu", "", "telugu", "", "Telugu"),
    ("th", "thai", "", "", "thai", "", "Thai"),
    ("tk", "turkmen", "", "", "turkmen", "", "Turkmen"),
    ("tr", "turkish", "", "", "turkish", "", "Turkish"),
    ("uk", "ukrainian", "", "", "ukrainian", "", "Ukrainian"),
    ("ur", "", "urdu", "", "urdu", "", "Urdu"),
    ("vi", "vietnamese", "", "", "vietnamese", "", "Vietnamese"),
    ("zh", "", "chinese", "", "chinese", "simplified", "Chinese Simplified"),
    ("zh-cn", "", "chinese", "", "chinese", "simplified", "Chinese Simplified"),
    ("zh-tw", "", "chinese", "", "chinese", "traditional", "Chinese Traditional"),
];

fn language(
    code: &str,
) -> Option<&'static (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    LANGUAGES.iter().find(|l| l.0 == code)
}

/// `org-latex--protect-text`: a backslash before `\{}$%&_#~^`.
pub fn protect_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '{' | '}' | '$' | '%' | '&' | '_' | '#' | '~' | '^'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `org-latex--protect-texttt`.
fn protect_texttt(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 10);
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        let two = rest.get(..2);
        match two {
            Some("--") => {
                out.push_str("-{}-{}");
                rest = &rest[2..];
                continue;
            }
            Some("<<") => {
                out.push_str("<{}<{}");
                rest = &rest[2..];
                continue;
            }
            Some(">>") => {
                out.push_str(">{}>{}");
                rest = &rest[2..];
                continue;
            }
            _ => {}
        }
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '{' | '}' | '$' | '%' | '&' | '_' | '#' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
        rest = &rest[c.len_utf8()..];
    }
    format!("\\texttt{{{out}}}")
}

/// `org-latex--text-markup` with `org-latex-text-markup-alist`.
fn text_markup(text: &str, kind: SyntaxKind) -> String {
    match kind {
        BOLD => format!("\\textbf{{{text}}}"),
        ITALIC => format!("\\emph{{{text}}}"),
        STRIKE_THROUGH => format!("\\sout{{{text}}}"),
        UNDERLINE => format!("\\uline{{{text}}}"),
        CODE | VERBATIM => protect_texttt(text),
        _ => text.to_string(),
    }
}

/// `org-remove-blank-lines`.
fn remove_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        if line.trim_matches([' ', '\t', '\n']).is_empty() && line.ends_with('\n') {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// `org-latex-clean-invalid-line-breaks`: `\\` alone on a line, or right
/// after an `\end{…}`, goes.
fn clean_invalid_line_breaks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let t = body.trim_end_matches([' ', '\t']);
        if let Some(before) = t.strip_suffix("\\\\") {
            let before_trim = before.trim_end_matches([' ', '\t']);
            if before_trim.is_empty() {
                out.push_str(nl);
                continue;
            }
            if let Some(i) = before_trim.rfind("\\end{")
                && before_trim.ends_with('}')
                && before_trim[i + 5..before_trim.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '*')
                && before_trim.len() > i + 6
            {
                out.push_str(before_trim);
                out.push_str(nl);
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

/// Whether `value` starts a math environment
/// (`org-latex-math-environments-re`).
pub(crate) fn math_environment(value: &str) -> bool {
    let v = value.trim_start_matches([' ', '\t']);
    let Some(rest) = v.strip_prefix("\\begin{") else {
        return false;
    };
    MATH_ENVIRONMENTS.iter().any(|e| {
        rest.strip_prefix(e)
            .is_some_and(|r| r.starts_with('}') || r.starts_with("*}"))
    })
}

/// The inline image rules of `org-latex-inline-image-rules`.
pub fn image_path(link_type: &str, path: &str) -> bool {
    let lower = path.to_lowercase();
    let exts: &[&str] = match link_type {
        "file" => &[
            ".pdf", ".jpeg", ".jpg", ".png", ".ps", ".eps", ".tikz", ".pgf", ".svg",
        ],
        "https" => &[
            ".jpeg", ".jpg", ".png", ".ps", ".eps", ".tikz", ".pgf", ".svg",
        ],
        _ => return false,
    };
    exts.iter().any(|e| lower.ends_with(e))
}

/// The type of a LaTeX environment (`org-latex--environment-type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvType {
    Math,
    Table,
    Image,
    SrcBlock,
    SpecialBlock,
}

fn environment_type(value: &str) -> EnvType {
    let value = html::remove_indentation(value);
    let env = value
        .find("\\begin{")
        .and_then(|i| {
            let rest = &value[i + 7..];
            let end = rest.find('}')?;
            let name = &rest[..end];
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '*')
                .then(|| name.to_string())
        })
        .unwrap_or_default();
    if math_environment(&value) {
        EnvType::Math
    } else if ["table", "longtable", "tabular", "tabu", "longtabu"]
        .iter()
        .any(|t| env.contains(t))
    {
        EnvType::Table
    } else if env.contains("figure") {
        EnvType::Image
    } else if ["lstlisting", "listing", "verbatim", "minted"]
        .iter()
        .any(|t| env.contains(t))
    {
        EnvType::SrcBlock
    } else {
        EnvType::SpecialBlock
    }
}

fn attr(attrs: &[(String, Option<String>)], key: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.clone())
}

fn has(attrs: &[(String, Option<String>)], key: &str) -> bool {
    attrs.iter().any(|(k, _)| k == key)
}

/// `org-string-nw-p`.
fn nw(s: &str) -> bool {
    !s.trim().is_empty()
}

/// `org-element-normalize-string`.
fn normalize(s: &str) -> String {
    crate::export::normalize_string(s)
}

impl Latex {
    /// The syntax node of `id` as an AST node.
    fn cast<T: ast::AstNode>(ex: &Exporter<'_>, id: Id) -> Option<T> {
        ex.syntax(id).and_then(|s| T::cast(s.clone()))
    }

    /// The first `#+NAME` (or `#+RESULTS`) of an element.
    fn name(ex: &Exporter<'_>, id: Id) -> Option<String> {
        let s = ex.syntax(id)?;
        if !s.kind().is_element() {
            return None;
        }
        let mut name = None;
        let mut results = None;
        for k in ast::affiliated_keywords(s) {
            match k.key().as_str() {
                "NAME" if name.is_none() => name = Some(k.value()),
                "RESULTS" if results.is_none() => results = Some(k.value()),
                _ => {}
            }
        }
        name.or(results)
    }

    fn has_caption(ex: &Exporter<'_>, id: Id) -> bool {
        ex.tree.secondary(id, Secondary::Caption(0)).is_some()
    }

    /// `org-latex--label`: the element's label, forced (made from its
    /// reference) or only from a name, `CUSTOM_ID` or target; as
    /// `\label{…}` when `full`.
    fn label(&self, ex: &mut Exporter<'_>, id: Id, force: bool) -> Option<String> {
        let kind = ex.tree.kind(id)?;
        let user = match kind {
            HEADLINE | INLINETASK => ex.node_property(id, "CUSTOM_ID", false),
            TARGET => Self::cast::<ast::Target>(ex, id).map(|t| t.value()),
            _ => Self::name(ex, id),
        };
        if user.is_none() && !force {
            return None;
        }
        let prefix = match kind {
            HEADLINE => "sec:",
            TABLE if ex.tree.nodes[id].props.contains_key("matrices") => "eq:",
            TABLE => "tab:",
            LATEX_ENVIRONMENT => {
                let v = Self::cast::<ast::LatexEnvironment>(ex, id)
                    .map(|l| l.value())
                    .unwrap_or_default();
                if math_environment(&v) { "eq:" } else { "" }
            }
            PARAGRAPH if Self::has_caption(ex, id) => "fig:",
            SRC_BLOCK => "lst:",
            _ => "",
        };
        Some(format!("{prefix}{}", ex.reference(id)))
    }

    /// `(org-latex--label element info force t)`.
    fn full_label(&self, ex: &mut Exporter<'_>, id: Id, force: bool) -> String {
        match self.label(ex, id, force) {
            Some(l) if ex.tree.kind(id) == Some(TARGET) => format!("\\label{{{l}}}"),
            Some(l) => format!("\\label{{{l}}}\n"),
            None => String::new(),
        }
    }

    /// `org-latex--wrap-label`.
    fn wrap_label(&self, ex: &mut Exporter<'_>, id: Id, output: String) -> String {
        match self.label(ex, id, false) {
            Some(l) if nw(&output) => format!("\\phantomsection\n\\label{{{l}}}\n{output}"),
            _ => output,
        }
    }

    /// `org-latex--caption-above-p`: only tables have their caption above.
    fn caption_above(ex: &Exporter<'_>, id: Id) -> bool {
        ex.tree.kind(id) == Some(TABLE) && !ex.tree.nodes[id].props.contains_key("matrices")
    }

    /// `org-latex--caption/label-string`.
    fn caption_label(&self, ex: &mut Exporter<'_>, id: Id, env: Option<EnvType>) -> String {
        let label = self.full_label(ex, id, false);
        let main = html::caption_ids(ex, id);
        let attrs = read_attribute(ex, id, "ATTR_LATEX");
        let kind = ex.tree.kind(id);
        let has_main = !main.is_empty();
        let nonfloat = (has(&attrs, ":float") && attr(&attrs, ":float").is_none() && has_main)
            || (kind == Some(SRC_BLOCK) && attr(&attrs, ":float").is_none());
        if let Some(c) = attr(&attrs, ":caption").filter(|c| nw(c)) {
            return format!("{c}\n");
        }
        if !has_main {
            return label;
        }
        let ty = if !nonfloat {
            String::new()
        } else {
            let t = match (kind, env) {
                (Some(LATEX_ENVIRONMENT), Some(EnvType::Math)) => "math",
                (Some(LATEX_ENVIRONMENT), Some(EnvType::Table)) => "table",
                (Some(LATEX_ENVIRONMENT), Some(EnvType::SrcBlock)) => "figure",
                (Some(LATEX_ENVIRONMENT), _) => "figure",
                (Some(TABLE), _) => "table",
                _ => "figure",
            };
            format!("{{{t}}}")
        };
        let mut short: Vec<Id> = Vec::new();
        let mut i = 0;
        while let Some(s) = ex.tree.secondary(id, Secondary::ShortCaption(i)) {
            short = s.to_vec();
            i += 1;
        }
        let short = if short.is_empty() {
            String::new()
        } else {
            format!("[{}]", ex.data_list(&short))
        };
        let main = ex.data_list(&main);
        format!(
            "{}{ty}{short}{{{}{main}}}\n",
            if nonfloat { "\\captionof" } else { "\\caption" },
            trim(&label)
        )
    }

    /// `org-latex--delayed-footnotes-definitions`: `\footnotetext` for the
    /// footnotes first referenced in `ids`, where `\footnote` cannot go.
    fn delayed_footnotes(&self, ex: &mut Exporter<'_>, ids: &[Id]) -> String {
        let mut refs: Vec<Id> = Vec::new();
        self.search_refs(ex, ids, &mut refs);
        let mut out = String::new();
        for r in refs {
            let def = ex.footnote_definition(r).unwrap_or_default();
            let n = ex.footnote_number(r);
            let label = self.definition_label(ex, r, &def);
            let text = ex.data_list(&def);
            out.push_str(&format!(
                "\\footnotetext[{n}]{{{}{}}}",
                trim(&label),
                trim(&text)
            ));
        }
        out
    }

    fn search_refs(&self, ex: &mut Exporter<'_>, ids: &[Id], refs: &mut Vec<Id>) {
        for &d in ids {
            let mut stack = vec![d];
            while let Some(x) = stack.pop() {
                if ex.info.ignore.contains(&x) {
                    continue;
                }
                if ex.tree.kind(x) == Some(FOOTNOTE_REFERENCE) && ex.footnote_first_reference_p(x) {
                    refs.push(x);
                    let standard =
                        Self::cast::<ast::FootnoteReference>(ex, x).is_some_and(|f| !f.is_inline());
                    if standard && let Some(def) = ex.footnote_definition(x) {
                        self.search_refs(ex, &def, refs);
                    }
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

    /// The label of a footnote's definition (its contents `def`), for the
    /// references after the first.
    fn definition_label(&self, ex: &mut Exporter<'_>, r: Id, def: &[Id]) -> String {
        let key = def.first().copied().unwrap_or(r);
        format!("\\label{{{}}}\n", ex.reference(key))
    }

    fn footnote_reference(&self, ex: &mut Exporter<'_>, id: Id) -> String {
        let label = Self::cast::<ast::FootnoteReference>(ex, id).and_then(|f| f.label());
        let sep = match ex.previous_element(id) {
            Some(p) if ex.tree.kind(p) == Some(FOOTNOTE_REFERENCE) => "\\textsuperscript{,}\\,",
            _ => "",
        };
        let def = ex.footnote_definition(id).unwrap_or_default();
        if !ex.footnote_first_reference_p(id) {
            let key = def.first().copied().unwrap_or(id);
            return format!("{sep}\\textsuperscript{{\\ref{{{}}}}}", ex.reference(key));
        }
        let nested = ex.tree.ancestors(id).any(|a| {
            matches!(
                ex.tree.kind(a),
                Some(FOOTNOTE_REFERENCE | FOOTNOTE_DEFINITION | TABLE_CELL | VERSE_BLOCK)
            )
        }) || ex.tree.nodes[id].parent.is_some_and(|p| {
            ex.tree.kind(p) == Some(ITEM)
                && ex
                    .tree
                    .secondary(p, Secondary::Tag)
                    .is_some_and(|t| t.contains(&id))
        });
        if nested {
            return format!("{sep}\\footnotemark");
        }
        let text = ex.data_list(&def);
        let others = label.as_ref().is_some_and(|l| {
            ex.tree.descendants(ex.tree.root).into_iter().any(|f| {
                f != id
                    && ex.tree.kind(f) == Some(FOOTNOTE_REFERENCE)
                    && Self::cast::<ast::FootnoteReference>(ex, f)
                        .and_then(|x| x.label())
                        .as_ref()
                        == Some(l)
            })
        });
        let def_label = if others {
            trim(&self.definition_label(ex, id, &def)).to_string()
        } else {
            String::new()
        };
        let delayed = self.delayed_footnotes(ex, &def);
        // `#+LATEX_FOOTNOTE_COMMAND`, a `format` string of the text and
        // the label.
        let command = option_string(ex, "latex-default-footnote-command")
            .unwrap_or_else(|| "\\footnote{%s%s}".into());
        let mut args = [trim(&text).to_string(), def_label].into_iter();
        let mut note = String::new();
        let mut rest = command.as_str();
        while let Some(i) = rest.find('%') {
            note.push_str(&rest[..i]);
            match rest[i + 1..].chars().next() {
                Some('s') => note.push_str(&args.next().unwrap_or_default()),
                Some('%') => note.push('%'),
                Some(c) => {
                    note.push('%');
                    note.push(c);
                }
                None => note.push('%'),
            }
            rest = rest.get(i + 2..).unwrap_or("");
        }
        note.push_str(rest);
        format!("{sep}{note}{delayed}")
    }

    /// `org-latex-format-headline-default-function`.
    fn format_headline(
        todo: Option<&str>,
        priority: Option<char>,
        text: &str,
        tags: Option<&[String]>,
    ) -> String {
        let mut out = String::new();
        if let Some(t) = todo {
            out.push_str(&format!("{{\\bfseries\\sffamily {t}}} "));
        }
        if let Some(p) = priority {
            out.push_str(&format!("\\framebox{{\\#{p}}} "));
        }
        out.push_str(text);
        if let Some(tags) = tags.filter(|t| !t.is_empty()) {
            let t: Vec<String> = tags.iter().map(|t| protect_text(t)).collect();
            out.push_str(&format!("\\hfill{{}}\\textsc{{{}}}", t.join(":")));
        }
        out
    }

    fn todo(&self, ex: &mut Exporter<'_>, id: Id) -> Option<String> {
        if !ex.flag("with-todo-keywords") {
            return None;
        }
        let kw = ex
            .headline(id)
            .and_then(|h| h.todo_keyword())
            .map(|t| t.text().to_string())
            .or_else(|| {
                Self::cast::<ast::Inlinetask>(ex, id)
                    .and_then(|h| h.todo_keyword())
                    .map(|t| t.text().to_string())
            })?;
        let t = ex.tree.text_node(kw, None);
        Some(ex.data(t))
    }

    fn priority(ex: &Exporter<'_>, id: Id) -> Option<char> {
        if !ex.flag("with-priority") {
            return None;
        }
        ex.headline(id)
            .and_then(|h| h.priority())
            .or_else(|| Self::cast::<ast::Inlinetask>(ex, id).and_then(|h| h.priority()))
    }

    fn headline(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        if ex.footnote_section_p(id) {
            return None;
        }
        let class = option_string(ex, "latex-class").unwrap_or_else(|| "article".into());
        let level = ex.relative_level(id);
        let numbered = ex.numbered_p(id);
        let sectioning = CLASSES
            .iter()
            .find(|c| c.0 == class)
            .map(|c| c.2)
            .unwrap_or(&[]);
        let section_fmt = (level >= 1)
            .then(|| sectioning.get((level - 1) as usize))
            .flatten()
            .map(|(n, u)| format!("{}\n%s", if numbered { n } else { u }));
        let title_ids = ex
            .tree
            .secondary(id, Secondary::Title)
            .map(<[Id]>::to_vec)
            .unwrap_or_default();
        let text = finish(&ex.with_backend(&TITLE, |ex| ex.data_list(&title_ids)));
        let text_no_foot =
            finish(&ex.with_backend(&TITLE_NO_FOOTNOTES, |ex| ex.data_list(&title_ids)));
        let todo = self.todo(ex, id);
        let tags = if ex.flag("with-tags") {
            Some(ex.tags(id, &[], false))
        } else {
            None
        };
        let priority = Self::priority(ex, id);
        let full = Self::format_headline(todo.as_deref(), priority, &text, tags.as_deref());
        let full_no_foot =
            Self::format_headline(todo.as_deref(), priority, &text_no_foot, tags.as_deref());
        let label = self.full_label(ex, id, true);
        let pre_blanks = "\n".repeat(ex.headline(id).map_or(0, |h| h.pre_blank()));
        let contents = contents.unwrap_or_default();
        let Some(section_fmt) = section_fmt.filter(|_| ex.low_level_p(id).is_none()) else {
            let env = if numbered { "enumerate" } else { "itemize" };
            let mut body = String::new();
            if ex.first_sibling_p(id) {
                body.push_str(&format!("\\begin{{{env}}}\n"));
            }
            body.push_str("\\item");
            if full.trim_start_matches([' ', '\t']).starts_with('[') {
                body.push_str("\\relax");
            }
            body.push_str(&format!(" {full}\n{label}{pre_blanks}{contents}"));
            if ex.last_sibling_p(id) {
                let t = body.trim_end_matches([' ', '\n']).len();
                body.truncate(t);
                body.push_str(&format!("\n\\end{{{env}}}"));
            }
            return Some(body);
        };
        // The short title: an alternative title, the title without its
        // tags, or without footnotes.
        let alt_ids = ex.alt_title(id);
        let alt_text = finish(&ex.with_backend(&TITLE, |ex| ex.data_list(&alt_ids)));
        let tags_t = if matches!(ex.opt("with-tags"), Value::T) {
            tags.clone()
        } else {
            None
        };
        let opt_title =
            Self::format_headline(todo.as_deref(), priority, &alt_text, tags_t.as_deref());
        let mut contents = contents;
        if let Some(stop) = self.local_toc_stop(ex, id, level) {
            contents.push_str(&stop);
        }
        let body = format!("{label}{pre_blanks}{contents}");
        let short = if opt_title != full {
            Some(opt_title)
        } else if full_no_foot != full {
            Some(full_no_foot)
        } else {
            None
        };
        let fmt = match short {
            Some(s) if section_fmt.starts_with('\\') && section_fmt[1..].contains('{') => {
                let brace = section_fmt.find('{').unwrap_or(0);
                let s = s.replace(']', ")").replace('[', "(");
                format!("{}[{s}]{}", &section_fmt[..brace], &section_fmt[brace..])
            }
            _ => section_fmt,
        };
        Some(format_two(&fmt, &full, &body))
    }

    /// `\stopcontents` after a section with a local table of contents.
    fn local_toc_stop(&self, ex: &Exporter<'_>, id: Id, level: i64) -> Option<String> {
        let first = *ex.tree.children(id).first()?;
        if ex.tree.kind(first) != Some(SECTION) {
            return None;
        }
        ex.tree.descendants(first).into_iter().find_map(|k| {
            let kw: ast::Keyword = Self::cast(ex, k)?;
            if kw.key() != "TOC" {
                return None;
            }
            let v = kw.value().to_lowercase();
            let words: Vec<&str> = v.split_whitespace().collect();
            (words.contains(&"headlines") && words.contains(&"local"))
                .then(|| format!("\\stopcontents[level-{level}]"))
        })
    }

    fn item(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> String {
        let list = ex.tree.parent(id);
        let ordered = list.is_some_and(|l| html::list_type(ex, l) == html::ListType::Ordered);
        let mut level = 0;
        let mut p = list;
        while let Some(x) = p {
            match ex.tree.kind(x) {
                Some(PLAIN_LIST) => {
                    if html::list_type(ex, x) == html::ListType::Ordered {
                        level += 1;
                    }
                }
                Some(ITEM) => {}
                _ => break,
            }
            p = ex.tree.parent(x);
        }
        let item: Option<ast::Item> = Self::cast(ex, id);
        let counter = item
            .as_ref()
            .and_then(|i| i.counter())
            .filter(|_| level < 5)
            .map(|c| {
                let n = ["i", "ii", "iii", "iv"][level.max(1) - 1];
                format!("\\setcounter{{enum{n}}}{{{}}}\n", c as i64 - 1)
            })
            .unwrap_or_default();
        let checkbox = match item.as_ref().and_then(|i| i.checkbox()) {
            Some(ast::Checkbox::On) => Some("$\\boxtimes$"),
            Some(ast::Checkbox::Off) => Some("$\\square$"),
            Some(ast::Checkbox::Partial) => Some("$\\boxminus$"),
            None => None,
        };
        let tag_ids = ex.tree.secondary(id, Secondary::Tag).map(<[Id]>::to_vec);
        let tag = tag_ids.as_ref().map(|t| ex.data_list(t));
        let tag_foot = match &tag_ids {
            Some(t) => self.delayed_footnotes(ex, t),
            None => String::new(),
        };
        let fmt3 = |a: &str, b: &str, c: &str| {
            if ordered {
                format!("{{{a} {b}}} {c}")
            } else {
                format!("[{{{a} {b}}}] {c}")
            }
        };
        let fmt2 = |a: &str, c: &str| {
            if ordered {
                format!("{{{a}}} {c}")
            } else {
                format!("[{{{a}}}] {c}")
            }
        };
        let mid = match (checkbox, &tag) {
            (Some(cb), Some(t)) => fmt3(cb, t, &tag_foot),
            (Some(cb), None) => fmt2(cb, &tag_foot),
            (None, Some(t)) => fmt2(t, &tag_foot),
            (None, None) => {
                let bracket = contents
                    .as_deref()
                    .is_some_and(|c| c.trim_start_matches([' ', '\t']).starts_with('['));
                let snippet_first = ex.tree.children(id).first().is_some_and(|&e| {
                    ex.tree.kind(e) == Some(PARAGRAPH)
                        && ex.tree.children(e).first().is_some_and(|&o| {
                            Self::cast::<ast::ExportSnippet>(ex, o)
                                .is_some_and(|s| s.backend() == "latex")
                        })
                });
                if bracket && !snippet_first {
                    "\\relax ".to_string()
                } else {
                    " ".to_string()
                }
            }
        };
        let body = contents.map(|c| trim(&c).to_string()).unwrap_or_default();
        format!("{counter}\\item{mid}{body}")
    }

    fn keyword(&self, ex: &mut Exporter<'_>, id: Id) -> Option<String> {
        let k: ast::Keyword = Self::cast(ex, id)?;
        let value = k.value();
        match k.key().as_str() {
            "LATEX" => Some(value),
            "INDEX" => Some(format!("\\index{{{value}}}")),
            "TOC" => {
                let lower = value.to_lowercase();
                let words: Vec<&str> = lower
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                    .collect();
                if words.contains(&"headlines") {
                    let local = words.contains(&"local");
                    let parent = ex
                        .tree
                        .ancestors(id)
                        .find(|a| ex.tree.kind(*a) == Some(HEADLINE));
                    let level = match (local, parent) {
                        (true, Some(p)) => ex.relative_level(p),
                        _ => 0,
                    };
                    let depth = words
                        .iter()
                        .find_map(|w| w.parse::<i64>().ok())
                        .map(|d| format!("\\setcounter{{tocdepth}}{{{}}}", d + level));
                    if local && parent.is_some() {
                        Some(format!(
                            "\\startcontents[level-{level}]\n\\printcontents[level-{level}]{{}}{{0}}{{{}}}",
                            depth.unwrap_or_default()
                        ))
                    } else {
                        Some(match depth {
                            Some(d) => format!("{d}\n\\tableofcontents"),
                            None => "\\tableofcontents".to_string(),
                        })
                    }
                } else if words.contains(&"tables") {
                    Some("\\listoftables".into())
                } else if words.contains(&"listings") {
                    Some("\\lstlistoflistings".into())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn latex_environment(&self, ex: &mut Exporter<'_>, id: Id) -> Option<String> {
        if !ex.flag("with-latex") {
            return None;
        }
        let v = Self::cast::<ast::LatexEnvironment>(ex, id)?.value();
        let value = html::remove_indentation(&v);
        let ty = environment_type(&v);
        let has_name = Self::name(ex, id).is_some();
        if !has_name && !Self::has_caption(ex, id) {
            return Some(value);
        }
        let caption = if ty == EnvType::Math {
            self.full_label(ex, id, false)
        } else {
            self.caption_label(ex, id, Some(ty))
        };
        let above = ty == EnvType::Math;
        let at = if above {
            value.find('\n').map_or(value.len(), |i| i + 1)
        } else {
            // The start of the last line (`forward-line -1` from the end).
            let body = value.strip_suffix('\n').unwrap_or(&value);
            if value.ends_with('\n') {
                body.rfind('\n').map_or(0, |i| i + 1)
            } else {
                let before = &value[..value.rfind('\n').unwrap_or(0)];
                before.rfind('\n').map_or(0, |i| i + 1)
            }
        };
        let mut out = value.clone();
        if above && !value[..at].ends_with('\n') {
            out.push('\n');
            out.push_str(&caption);
            return Some(out);
        }
        out.insert_str(at, &caption);
        Some(out)
    }

    fn inline_image(&self, ex: &mut Exporter<'_>, link: Id) -> Option<String> {
        let info = ex.link_info(link)?;
        let parent = ex
            .tree
            .ancestors(link)
            .find(|a| ex.tree.kind(*a).is_some_and(|k| k.is_element()))?;
        // A remote image is left to LaTeX as its address (Emacs downloads
        // it, when allowed).
        let path = if info.link_type == "file" {
            info.path.clone()
        } else {
            format!("{}:{}", info.link_type, info.path)
        };
        let filetype = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let caption = self.caption_label(ex, parent, None);
        let above = false;
        let attrs = read_attribute(ex, parent, "ATTR_LATEX");
        #[derive(PartialEq)]
        enum Float {
            Env(String),
            Wrap,
            Sideways,
            Multicolumn,
            Figure,
            Plain,
        }
        // Not a float when the paragraph holds more than the image.
        let alone = ex
            .tree
            .children(parent)
            .iter()
            .all(|&n| n == link || (ex.tree.is_text(n) && !nw(&ex.tree.nodes[n].text)));
        let fl = attr(&attrs, ":float");
        let float = if !alone {
            Float::Plain
        } else {
            match fl.as_deref() {
                Some("wrap") => Float::Wrap,
                Some("sideways") => Float::Sideways,
                Some("multicolumn") => Float::Multicolumn,
                Some("t") => Float::Figure,
                None if has(&attrs, ":float") => Float::Plain,
                Some(f) => Float::Env(f.to_string()),
                None if Self::has_caption(ex, parent)
                    || attr(&attrs, ":caption").is_some_and(|c| nw(&c)) =>
                {
                    Float::Figure
                }
                None => Float::Plain,
            }
        };
        let placement = match attr(&attrs, ":placement") {
            Some(p) => p,
            None if float == Float::Wrap => "{l}{0.5\\textwidth}".into(),
            None if float == Float::Figure => format!(
                "[{}]",
                option_string(ex, "latex-default-figure-position").unwrap_or_else(|| "htbp".into())
            ),
            None => String::new(),
        };
        let in_link = ex
            .tree
            .parent(link)
            .is_some_and(|p| ex.tree.kind(p) == Some(LINK));
        let center = if in_link {
            false
        } else if has(&attrs, ":center") {
            attr(&attrs, ":center").is_some()
        } else {
            true
        };
        let comment = if attr(&attrs, ":comment-include").is_some() {
            "%"
        } else {
            ""
        };
        let scale = if float == Float::Wrap {
            String::new()
        } else {
            attr(&attrs, ":scale").unwrap_or_default()
        };
        let width = if nw(&scale) {
            String::new()
        } else if let Some(w) = attr(&attrs, ":width") {
            w
        } else if attr(&attrs, ":height").is_some() {
            String::new()
        } else if float == Float::Wrap {
            "0.48\\textwidth".into()
        } else {
            ".9\\linewidth".into()
        };
        let height = if nw(&scale) {
            String::new()
        } else {
            attr(&attrs, ":height").unwrap_or_default()
        };
        let mut options = attr(&attrs, ":options").unwrap_or_default();
        if let Some(inner) = options.strip_prefix('[').and_then(|o| o.strip_suffix(']')) {
            options = inner.to_string();
        }
        let image = if filetype == "tikz" || filetype == "pgf" {
            let mut code = format!("\\input{{{path}}}");
            if nw(&options) {
                code = format!("\\begin{{tikzpicture}}[{options}]\n{code}\n\\end{{tikzpicture}}");
            }
            if nw(&scale) {
                format!("\\scalebox{{{scale}}}{{{code}}}")
            } else if nw(&width) || nw(&height) {
                format!(
                    "\\resizebox{{{}}}{{{}}}{{{code}}}",
                    if nw(&width) { &width } else { "!" },
                    if nw(&height) { &height } else { "!" }
                )
            } else {
                code
            }
        } else {
            if nw(&scale) {
                options.push_str(&format!(",scale={scale}"));
            } else {
                if nw(&width) {
                    options.push_str(&format!(",width={width}"));
                }
                if nw(&height) {
                    options.push_str(&format!(",height={height}"));
                }
            }
            if let Some(search) = info.search_option.as_deref()
                && filetype == "pdf"
                && !search.is_empty()
                && search.chars().all(|c| c.is_ascii_digit())
                && !options.contains("page=")
            {
                options.push_str(&format!(",page={search}"));
            }
            let opts = if !nw(&options) {
                String::new()
            } else if let Some(o) = options.strip_prefix(',') {
                format!("[{o}]")
            } else {
                format!("[{options}]")
            };
            let p = if filetype == "svg" && !path.is_ascii() {
                format!("\\detokenize{{{path}}}")
            } else {
                path.clone()
            };
            let mut code = format!("\\includegraphics{opts}{{{p}}}");
            if filetype == "svg" {
                code = code.replacen("\\includegraphics", "\\includesvg", 1);
                code = code.replace(".svg}", "}");
            }
            code
        };
        let (cap_above, cap_below) = if above {
            (caption.as_str(), "")
        } else {
            ("", caption.as_str())
        };
        let centering = if center { "\\centering" } else { "" };
        Some(match float {
            Float::Env(env) => format!(
                "\\begin{{{env}}}{placement}\n{cap_above}{centering}\n{comment}{image}\n{cap_below}\\end{{{env}}}"
            ),
            Float::Wrap => format!(
                "\\begin{{wrapfigure}}{placement}\n{cap_above}{centering}\n{comment}{image}\n{cap_below}\\end{{wrapfigure}}"
            ),
            Float::Sideways => format!(
                "\\begin{{sidewaysfigure}}\n{cap_above}{centering}\n{comment}{image}\n{cap_below}\\end{{sidewaysfigure}}"
            ),
            Float::Multicolumn => format!(
                "\\begin{{figure*}}{placement}\n{cap_above}{centering}\n{comment}{image}\n{cap_below}\\end{{figure*}}"
            ),
            Float::Figure => format!(
                "\\begin{{figure}}{placement}\n{cap_above}{centering}\n{comment}{image}\n{cap_below}\\end{{figure}}"
            ),
            Float::Plain if center => {
                format!("\\begin{{center}}\n{cap_above}{image}\n{cap_below}\\end{{center}}")
            }
            Float::Plain => format!("{cap_above}{image}{cap_above}"),
        })
    }

    fn link(&self, ex: &mut Exporter<'_>, id: Id, desc: Option<String>) -> Option<String> {
        let info = ex.link_info(id)?;
        let ty = info.link_type.clone();
        let raw = info.path.clone();
        let desc = desc.filter(|d| !d.is_empty());
        let imagep = image_path(&ty, &raw) && ex.tree.children(id).is_empty();
        let path = protect_text(&if ty == "file" {
            html::file_uri(&raw)
        } else {
            format!("{ty}:{raw}")
        });
        if let Some(out) = ex.custom_protocol(&ty, &raw, desc.as_deref(), "latex") {
            return Some(out);
        }
        if imagep {
            return self.inline_image(ex, id);
        }
        match ty.as_str() {
            "radio" => {
                let dest = ex.resolve_radio(&raw);
                Some(match dest {
                    None => desc.unwrap_or_default(),
                    Some(d) => format!(
                        "\\hyperref[{}]{{{}}}",
                        ex.reference(d),
                        desc.unwrap_or_default()
                    ),
                })
            }
            "custom-id" | "fuzzy" | "id" => {
                let dest = if ty == "fuzzy" {
                    ex.resolve_fuzzy(&raw)
                } else {
                    ex.resolve_id(&raw)
                };
                let Some(dest) = dest else {
                    ex.broken_link(id, &raw);
                    return None;
                };
                if ex.tree.kind(dest) == Some(HEADLINE) {
                    let label = self.label(ex, dest, true).unwrap_or_default();
                    if desc.is_none() && ex.numbered_p(dest) {
                        return Some(format!("\\ref{{{label}}}"));
                    }
                    let d = match desc {
                        Some(d) => d,
                        None => {
                            let ids = ex
                                .tree
                                .secondary(dest, Secondary::Title)
                                .map(<[Id]>::to_vec)
                                .unwrap_or_default();
                            ex.data_list(&ids)
                        }
                    };
                    return Some(format!("\\hyperref[{label}]{{{d}}}"));
                }
                let r = self.label(ex, dest, true).unwrap_or_default();
                Some(match desc {
                    None => format!("\\ref{{{r}}}"),
                    Some(d) => format!("\\hyperref[{r}]{{{d}}}"),
                })
            }
            "coderef" => {
                let Some(target) = html::resolve_coderef(ex, &raw) else {
                    ex.broken_link(id, &raw);
                    return None;
                };
                Some(html::coderef_format(&path, desc.as_deref()).replace("%s", &target))
            }
            _ => Some(match desc {
                Some(d) => format!("\\href{{{path}}}{{{d}}}"),
                None => format!("\\url{{{path}}}"),
            }),
        }
    }

    /// `org-latex--align-string`.
    fn align_string(&self, ex: &Exporter<'_>, table: Id, math: bool) -> String {
        if let Some(a) = attr(&read_attribute(ex, table, "ATTR_LATEX"), ":align") {
            return a;
        }
        let Some(row) = ex
            .table_rows(table)
            .into_iter()
            .find(|&r| !ex.rule_row_p(r))
        else {
            return String::new();
        };
        let mut align = String::new();
        let cells: Vec<Id> = ex
            .row_cells(row)
            .into_iter()
            .filter(|c| !ex.info.ignore.contains(c))
            .collect();
        for c in cells {
            let (left, right) = html::cell_borders(ex, c);
            if left && align.is_empty() {
                align.push('|');
            }
            align.push_str(if math {
                "c"
            } else {
                match ex.cell_alignment(c) {
                    "right" => "r",
                    "center" => "c",
                    _ => "l",
                }
            });
            if right {
                align.push('|');
            }
        }
        align
    }

    /// `org-latex--decorate-table`.
    fn decorate_table(
        &self,
        ex: &Exporter<'_>,
        table: String,
        attrs: &[(String, Option<String>)],
        caption: &str,
        above: bool,
    ) -> String {
        let fl = attr(attrs, ":float");
        let float_env: Option<String> = if fl.is_none() && has(attrs, ":float") {
            None
        } else {
            match fl.as_deref() {
                Some("sidewaystable" | "sideways") => Some("sidewaystable".into()),
                Some("multicolumn") => Some("table*".into()),
                Some("t") => Some("table".into()),
                Some(f) => Some(f.to_string()),
                None if nw(caption) => Some("table".into()),
                None => None,
            }
        };
        let placement = attr(attrs, ":placement").unwrap_or_else(|| {
            format!(
                "[{}]",
                option_string(ex, "latex-default-figure-position").unwrap_or_else(|| "htbp".into())
            )
        });
        let center = if has(attrs, ":center") {
            attr(attrs, ":center").is_some()
        } else {
            true
        };
        let fontsize = attr(attrs, ":font").map(|f| format!("{f}\n"));
        let mut out = String::new();
        if let Some(env) = &float_env {
            out.push_str(&format!("\\begin{{{env}}}{placement}\n"));
            if above {
                out.push_str(caption);
            }
            if center {
                out.push_str("\\centering\n");
            }
            if let Some(f) = &fontsize {
                out.push_str(f);
            }
        } else {
            if center {
                out.push_str("\\begin{center}\n");
            }
            if above {
                out.push_str(caption);
            }
            match (&fontsize, center) {
                (Some(f), true) => out.push_str(f),
                (Some(f), false) => out.push_str(&format!("{{{f}")),
                _ => {}
            }
        }
        out.push_str(&table);
        if let Some(env) = &float_env {
            if !above {
                out.push('\n');
                out.push_str(caption);
            }
            out.push_str(&format!("\n\\end{{{env}}}"));
        } else {
            if !above {
                out.push('\n');
                out.push_str(caption);
            }
            if center {
                out.push_str("\n\\end{center}");
            }
            if fontsize.is_some() && !center {
                out.push('}');
            }
        }
        out
    }

    fn table(&self, ex: &mut Exporter<'_>, id: Id, contents: String) -> Option<String> {
        let t: ast::Table = Self::cast(ex, id)?;
        if t.table_type() == ast::TableType::TableEl {
            return Some(self.table_el(ex, id));
        }
        let attrs = read_attribute(ex, id, "ATTR_LATEX");
        if let Some(members) = ex.tree.nodes[id].props.get("matrices").cloned() {
            return Some(self.matrices(ex, id, &members));
        }
        let mode = attr(&attrs, ":mode").unwrap_or_else(|| "table".into());
        match mode.as_str() {
            "verbatim" => {
                let raw: String = ex
                    .tree
                    .children(id)
                    .iter()
                    .map(|&r| ex.tree.source(r))
                    .collect();
                Some(format!(
                    "\\begin{{verbatim}}\n{}\n\\end{{verbatim}}",
                    trim(&raw)
                ))
            }
            "math" | "inline-math" => Some(self.math_table(ex, id)),
            "tabbing" => {
                let count = ex
                    .table_rows(id)
                    .into_iter()
                    .find(|&r| !ex.rule_row_p(r))
                    .map_or(0, |r| {
                        ex.row_cells(r)
                            .into_iter()
                            .filter(|c| !ex.info.ignore.contains(c))
                            .count()
                    });
                let align = attr(&attrs, ":align").unwrap_or_else(|| {
                    let w = 1.0 / count.max(1) as f64 - 0.01;
                    format!(
                        "{}\\kill",
                        format!("\\hspace{{{}\\textwidth}} \\= ", lisp_float(w)).repeat(count)
                    )
                });
                Some(format!(
                    "\\begin{{tabbing}}\n{align}\n{contents}\\end{{tabbing}}"
                ))
            }
            _ => {
                let table = self.org_table(ex, id, contents, &attrs);
                let foot = self.delayed_footnotes(ex, &[id]);
                Some(format!("{table}{foot}"))
            }
        }
    }

    /// `org-latex--org-table`.
    fn org_table(
        &self,
        ex: &mut Exporter<'_>,
        id: Id,
        contents: String,
        attrs: &[(String, Option<String>)],
    ) -> String {
        let alignment = self.align_string(ex, id, false);
        let opt = attr(attrs, ":options");
        let env = attr(attrs, ":environment").unwrap_or_else(|| "tabular".into());
        let width = match attr(attrs, ":width") {
            None => String::new(),
            Some(_) if env == "tabular" || env == "longtable" => String::new(),
            Some(w) if env == "tabu" || env == "longtabu" => {
                if attr(attrs, ":spread").is_some() {
                    format!(" spread {w} ")
                } else {
                    format!(" to {w} ")
                }
            }
            Some(w) => format!("{{{w}}}"),
        };
        let caption = self.caption_label(ex, id, None);
        let above = Self::caption_above(ex, id);
        if env == "longtable" || env == "longtabu" {
            let fontsize = attr(attrs, ":font").map(|f| format!("{f}\n"));
            let mut out = String::new();
            if let Some(f) = &fontsize {
                out.push_str(&format!("{{{f}"));
            }
            out.push_str(&format!("\\begin{{{env}}}{width}{{{alignment}}}\n"));
            if above && nw(&caption) {
                out.push_str(&format!("{caption}\\\\\n"));
            }
            out.push_str(&contents);
            if !above && nw(&caption) {
                out.push_str(&format!("{caption}\\\\\n"));
            }
            out.push_str(&format!("\\end{{{env}}}"));
            if fontsize.is_some() {
                out.push('}');
            }
            return out;
        }
        let output = format!(
            "\\begin{{{env}}}{}{width}{{{alignment}}}\n{contents}\\end{{{env}}}",
            opt.map(|o| format!("[{o}]")).unwrap_or_default()
        );
        self.decorate_table(ex, output, attrs, &caption, above)
    }

    /// `org-latex--table.el-table`: the table as `table-generate-source`
    /// writes it for LaTeX, in the table's decoration (caption, float,
    /// `:rmlines`).
    fn table_el(&self, ex: &mut Exporter<'_>, id: Id) -> String {
        let value = Self::cast::<ast::Table>(ex, id)
            .and_then(|t| t.table_el_value())
            .unwrap_or_default();
        let mut output = table_el_latex(&value);
        let attrs = read_attribute(ex, id, "ATTR_LATEX");
        let caption = self.caption_label(ex, id, None);
        let above = Self::caption_above(ex, id);
        if attr(&attrs, ":rmlines").is_some() {
            // Every `\\hline` but the second (below the heading) out.
            let mut n = 0;
            output = output
                .split_inclusive('\n')
                .filter(|l| {
                    if l.trim_end() == "\\hline" {
                        n += 1;
                        n == 2
                    } else {
                        true
                    }
                })
                .collect();
        }
        self.decorate_table(ex, output, &attrs, &caption, above)
    }

    /// The rows of a table in math mode (`org-latex--math-table`).
    fn math_table(&self, ex: &mut Exporter<'_>, id: Id) -> String {
        let attrs = read_attribute(ex, id, "ATTR_LATEX");
        let env = attr(&attrs, ":environment").unwrap_or_else(|| "tabular".into());
        let end = MATRIX_MACROS
            .iter()
            .find(|m| m.0 == env)
            .map_or("\\\\", |m| m.1);
        let mut contents = String::new();
        for r in ex.table_rows(id) {
            if ex.rule_row_p(r) {
                contents.push_str("\\hline");
                continue;
            }
            let cells: Vec<String> = ex
                .row_cells(r)
                .into_iter()
                .filter(|c| !ex.info.ignore.contains(c))
                // `org-element-interpret-data` without the bar: the
                // contents between spaces.
                .map(|c| {
                    let s = ex.tree.source(c);
                    format!(" {} ", s.trim_end_matches('|').trim())
                })
                .collect();
            contents.push_str(&cells.join("&"));
            contents.push_str(end);
            contents.push('\n');
        }
        let body = if env == "array" || env == "tabular" {
            format!(
                "\\begin{{{env}}}{{{}}}\n{contents}\\end{{{env}}}",
                self.align_string(ex, id, true)
            )
        } else if MATRIX_MACROS.iter().any(|m| m.0 == env) {
            format!(
                "\\{env}{}{{\n{contents}}}",
                attr(&attrs, ":math-arguments").unwrap_or_default()
            )
        } else {
            format!("\\begin{{{env}}}\n{contents}\\end{{{env}}}")
        };
        format!(
            "{}{body}{}",
            attr(&attrs, ":math-prefix").unwrap_or_default(),
            attr(&attrs, ":math-suffix").unwrap_or_default()
        )
    }

    /// Tables in math mode, grouped (`org-latex-matrices`): `members` is
    /// the list of the tables' ids.
    fn matrices(&self, ex: &mut Exporter<'_>, id: Id, members: &str) -> String {
        let ids: Vec<Id> = members.split(',').filter_map(|s| s.parse().ok()).collect();
        let markup = ex.tree.nodes[id]
            .props
            .get("matrices-markup")
            .cloned()
            .unwrap_or_default();
        let mut contents = String::new();
        for &t in &ids {
            let m = self.math_table(ex, t);
            let blank = if t == *ids.last().unwrap_or(&t) {
                0
            } else {
                ex.tree.nodes[t].post_blank
            };
            contents.push_str(&normalize(&m));
            contents.push_str(&"\n".repeat(blank));
        }
        match markup.as_str() {
            "inline" => format!("\\({contents}\\)"),
            // The container inherits the first table's name, not its
            // caption (a FIXME in `org-latex--wrap-latex-matrices`).
            "equation" => {
                let caption = self.full_label(ex, id, false);
                format!("\\begin{{equation}}\n{contents}{caption}\\end{{equation}}")
            }
            _ => format!("\\[\n{contents}\\]"),
        }
    }

    fn table_row(&self, ex: &mut Exporter<'_>, id: Id, contents: String) -> Option<String> {
        let table = ex.tree.parent(id)?;
        let attrs = read_attribute(ex, table, "ATTR_LATEX");
        let booktabs = attr(&attrs, ":booktabs").is_some();
        let env = attr(&attrs, ":environment").unwrap_or_else(|| "tabular".into());
        let longtable = env == "longtable" || env == "longtabu";
        let rows = ex.table_rows(table);
        let pos = rows.iter().position(|&r| r == id)?;
        let prev = pos.checked_sub(1).map(|p| rows[p]);
        let next = rows.get(pos + 1).copied();
        if ex.rule_row_p(id) {
            return Some(
                if !booktabs {
                    "\\hline"
                } else if prev.is_none() {
                    "\\toprule"
                } else if next.is_none() {
                    "\\bottomrule"
                } else if longtable && prev.is_some_and(|p| self.ends_header(ex, p)) {
                    ""
                } else {
                    "\\midrule"
                }
                .to_string(),
            );
        }
        if ex.row_in_header(id) {
            let head = ex.tree.nodes[table].props.entry("latex-head").or_default();
            if head.is_empty() {
                *head = contents.clone();
            } else {
                *head = format!("{head}\\\\\n{contents}");
            }
        }
        let mut out = String::new();
        if booktabs && prev.is_none() {
            out.push_str("\\toprule\n");
        }
        out.push_str(&contents);
        out.push_str("\\\\\n");
        if longtable && self.ends_header(ex, id) {
            let columns = ex
                .row_cells(id)
                .into_iter()
                .filter(|c| !ex.info.ignore.contains(c))
                .count();
            let rule = if booktabs { "\\midrule" } else { "\\hline" };
            let starts = if !self.starts_header(ex, id) {
                ""
            } else if booktabs {
                "\\toprule\n"
            } else {
                "\\hline\n"
            };
            let head = ex.tree.nodes[table]
                .props
                .get("latex-head")
                .cloned()
                .unwrap_or_default();
            out.push_str(&format!(
                "{rule}\n\\endfirsthead\n\\multicolumn{{{columns}}}{{l}}{{{}}} \\\\\n{starts}\n{head} \\\\\n\n{rule}\n\\endhead\n{rule}\\multicolumn{{{columns}}}{{r}}{{{}}} \\\\\n\\endfoot\n\\endlastfoot",
                ex.translate("Continued from previous page", "latex"),
                ex.translate("Continued on next page", "latex")
            ));
        } else if booktabs && next.is_none() {
            out.push_str("\\bottomrule");
        }
        Some(out)
    }

    /// `org-export-table-row-ends-header-p`.
    fn ends_header(&self, ex: &Exporter<'_>, row: Id) -> bool {
        if !ex.row_in_header(row) || ex.rule_row_p(row) {
            return false;
        }
        let Some(table) = ex.tree.parent(row) else {
            return false;
        };
        let rows = ex.table_rows(table);
        let pos = rows.iter().position(|&r| r == row).unwrap_or(0);
        !rows[pos + 1..]
            .iter()
            .any(|&r| !ex.rule_row_p(r) && ex.row_group(r) == ex.row_group(row))
    }

    /// `org-export-table-row-starts-header-p`.
    fn starts_header(&self, ex: &Exporter<'_>, row: Id) -> bool {
        if !ex.row_in_header(row) || ex.rule_row_p(row) {
            return false;
        }
        let Some(table) = ex.tree.parent(row) else {
            return false;
        };
        let rows = ex.table_rows(table);
        let pos = rows.iter().position(|&r| r == row).unwrap_or(0);
        !rows[..pos]
            .iter()
            .any(|&r| !ex.rule_row_p(r) && ex.row_group(r) == ex.row_group(row))
    }

    fn table_cell(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> String {
        let table = ex.tree.parent(id).and_then(|r| ex.tree.parent(r));
        let tabbing = table.is_some_and(|t| {
            attr(&read_attribute(ex, t, "ATTR_LATEX"), ":mode").as_deref() == Some("tabbing")
        });
        let mut out = finish(&contents.unwrap_or_default());
        if ex.next_element(id).is_some() {
            out.push_str(if tabbing { " \\> " } else { " & " });
        }
        out
    }

    fn verse_block(&self, ex: &mut Exporter<'_>, id: Id, contents: String) -> String {
        let attrs = read_attribute(ex, id, "ATTR_LATEX");
        let lines = attr(&attrs, ":lines");
        let latexcode = attr(&attrs, ":latexcode");
        let center = attr(&attrs, ":center").is_some();
        let literal = attr(&attrs, ":literal").is_some();
        let mut a = String::new();
        if center {
            a.push_str("[\\versewidth]");
        }
        if let Some(l) = &lines {
            a.push_str(&format!("\n\\poemlines{{{l}}}"));
        }
        if let Some(c) = &latexcode {
            a.push_str(&format!("\n{c}"));
        }
        let vwidth = attr(&attrs, ":versewidth")
            .map(|w| format!("\\settowidth{{\\versewidth}}{{{w}}}\n"))
            .unwrap_or_default();
        let reset = if lines.is_some() {
            "\n\\poemlines{0}"
        } else {
            ""
        };
        let text = if literal {
            contents
        } else {
            format!("{}\n", contents.trim_end_matches([' ', '\t', '\n', '\r']))
        };
        // Line ends become `\\`.
        let mut body = String::new();
        for line in text.split_inclusive('\n') {
            match line.strip_suffix('\n') {
                Some(l) => {
                    let l = l.trim_end_matches([' ', '\t']);
                    let l = l.strip_suffix("\\\\").unwrap_or(l);
                    body.push_str(l.trim_end_matches([' ', '\t']));
                    body.push_str("\\\\\n");
                }
                None => body.push_str(line),
            }
        }
        // Blank lines: stanzas (or vertical space, literally).
        let body = if !literal {
            let lines: Vec<&str> = body.split_inclusive('\n').collect();
            let mut out = String::new();
            let mut i = 0;
            let blank = |l: &str| l.trim_start_matches([' ', '\t']) == "\\\\\n";
            while i < lines.len() {
                let l = lines[i];
                if l.ends_with("\\\\\n") && i + 1 < lines.len() && blank(lines[i + 1]) {
                    out.push_str(&l[..l.len() - 3]);
                    out.push_str(if attr(&attrs, ":lines").is_none() {
                        "\n\n"
                    } else {
                        "\\\\!\n\n"
                    });
                    i += 1;
                    while i < lines.len() && blank(lines[i]) {
                        i += 1;
                    }
                    continue;
                }
                out.push_str(l);
                i += 1;
            }
            out
        } else {
            body.split_inclusive('\n')
                .map(|l| {
                    let t = l.trim_start_matches([' ', '\t']);
                    if t == "\\\\\n" || t == "\\\\" {
                        format!(
                            "\\vspace*{{\\baselineskip}}{}",
                            if l.ends_with('\n') { "\n" } else { "" }
                        )
                    } else {
                        l.to_string()
                    }
                })
                .collect()
        };
        // Indentation.
        let body: String = body
            .split_inclusive('\n')
            .map(|l| {
                let n = l.len() - l.trim_start_matches([' ', '\t']).len();
                if n > 0 {
                    format!("\\hspace*{{{n}\\fontdimen2\\font}}{}", &l[n..])
                } else {
                    l.to_string()
                }
            })
            .collect();
        let out = format!("{vwidth}\\begin{{verse}}{a}\n{body}\\end{{verse}}{reset}");
        let out = self.wrap_label(ex, id, out);
        let foot = self.delayed_footnotes(ex, &[id]);
        clean_invalid_line_breaks(&format!("{out}{foot}"))
    }

    fn src_block(&self, ex: &mut Exporter<'_>, id: Id) -> Option<String> {
        let b: ast::SrcBlock = Self::cast(ex, id)?;
        if !nw(&b.value()) {
            return None;
        }
        let attrs = read_attribute(ex, id, "ATTR_LATEX");
        let float = attr(&attrs, ":float");
        let caption = self.caption_label(ex, id, None);
        let verbatim = format!(
            "\\begin{{verbatim}}\n{}\\end{{verbatim}}",
            crate::md::format_code_default(ex, id)
        );
        Some(if float.as_deref() == Some("multicolumn") {
            format!(
                "\\begin{{figure*}}[{}]\n{verbatim}\n{caption}\\end{{figure*}}",
                option_string(ex, "latex-default-figure-position").unwrap_or_else(|| "htbp".into())
            )
        } else if Self::has_caption(ex, id) {
            format!("{verbatim}\n{caption}")
        } else {
            verbatim
        })
    }

    /// The line of the Org text an element starts on, for `%% org:LINE`.
    fn source_line(ex: &Exporter<'_>, id: Id) -> Option<usize> {
        let s = ex.syntax(id)?;
        let start = usize::from(s.text_range().start());
        let root = ex.syntax(ex.tree.root)?;
        let text = root.text().to_string();
        Some(text[..start.min(text.len())].matches('\n').count() + 1)
    }
}

/// `(format FMT a b)` for a format with two `%s`.
fn format_two(fmt: &str, a: &str, b: &str) -> String {
    match fmt.split_once("%s") {
        Some((pre, rest)) => match rest.split_once("%s") {
            Some((mid, post)) => format!("{pre}{a}{mid}{b}{post}"),
            None => format!("{pre}{a}{rest}"),
        },
        None => fmt.to_string(),
    }
}

/// A float as Emacs prints it (`0.49`, `0.24`).
fn lisp_float(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// The value of option `prop` as a string, if it has one.
fn option_string(ex: &Exporter<'_>, prop: &str) -> Option<String> {
    match ex.opt(prop) {
        Value::Str(s) | Value::Sym(s) => Some(s),
        Value::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Backend for Title {
    fn name(&self) -> &'static str {
        "latex"
    }

    fn has_transcoder(&self, kind: SyntaxKind) -> bool {
        Latex::default().has_transcoder(kind)
    }

    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        match ex.tree.kind(id)? {
            UNDERLINE => Some(format!("\\underline{{{}}}", contents.unwrap_or_default())),
            CODE => Latex::cast::<ast::Code>(ex, id).map(|c| protect_texttt(&c.value())),
            VERBATIM => Latex::cast::<ast::Verbatim>(ex, id).map(|c| protect_texttt(&c.value())),
            FOOTNOTE_REFERENCE if !self.footnotes => None,
            _ => Latex::default().transcode(ex, id, contents),
        }
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        Latex::default().plain_text(ex, text)
    }
}

/// Groups formulas that follow each other into one math block
/// (`org-latex--wrap-latex-math-block`): the first keeps the others'
/// ids in `math-block`, and they leave the tree.
fn wrap_math_blocks(ex: &mut Exporter<'_>, ids: &mut Vec<Id>) {
    let valid = |ex: &Exporter<'_>, id: Id| match ex.tree.kind(id) {
        Some(ENTITY) => Latex::cast::<ast::Entity>(ex, id)
            .and_then(|e| e.latex())
            .is_some_and(|(_, math)| math),
        Some(LATEX_FRAGMENT) => Latex::cast::<ast::LatexFragment>(ex, id).is_some_and(|f| {
            let v = f.value();
            v.starts_with("\\(") || (v.starts_with('$') && !v.starts_with("$$"))
        }),
        _ => false,
    };
    let roots = ids.clone();
    for root in roots {
        for id in ex.tree.descendants(root) {
            if !valid(ex, id) || ex.tree.nodes[id].props.contains_key("math-member") {
                continue;
            }
            // In a keyword's value, the objects are a list of their own.
            let parent = ex.tree.nodes[id].parent.filter(|_| !ids.contains(&id));
            let mut members = vec![id];
            if ex.tree.nodes[id].post_blank == 0 {
                let next_of = |ex: &Exporter<'_>, x: Id, ids: &[Id]| match parent {
                    Some(_) => ex.next_element(x),
                    None => ids
                        .iter()
                        .position(|&i| i == x)
                        .and_then(|p| ids.get(p + 1).copied()),
                };
                let mut n = next_of(ex, id, ids);
                while let Some(next) = n {
                    if !valid(ex, next) {
                        break;
                    }
                    let last = *members.last().unwrap_or(&id);
                    ex.tree.nodes[last].post_blank = 1;
                    members.push(next);
                    ex.tree.nodes[next]
                        .props
                        .insert("math-member", String::new());
                    if ex.tree.nodes[next].post_blank > 0 {
                        break;
                    }
                    n = next_of(ex, next, ids);
                }
            }
            let last = *members.last().unwrap_or(&id);
            let blank = ex.tree.nodes[last].post_blank;
            let list = members
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            ex.tree.nodes[id].props.insert("math-block", list);
            ex.tree.nodes[id]
                .props
                .insert("math-blank", blank.to_string());
            // The block takes the others' places.
            match parent {
                Some(parent) => {
                    let children = &mut ex.tree.nodes[parent].children;
                    children.retain(|c| *c == id || !members.contains(c));
                    let secondary = &mut ex.tree.nodes[parent].secondary;
                    for (_, v) in secondary.iter_mut() {
                        v.retain(|c| *c == id || !members.contains(c));
                    }
                }
                None => ids.retain(|c| *c == id || !members.contains(c)),
            }
            // The block's blanks are the last member's; the first
            // member's own gap stays for the joining.
            let gap = ex.tree.nodes[id].post_blank;
            ex.tree.nodes[id].props.insert("math-gap", gap.to_string());
            ex.tree.nodes[id].post_blank = blank;
        }
    }
}

/// Groups tables in math mode that follow each other
/// (`org-latex--wrap-latex-matrices`).
fn wrap_matrices(ex: &mut Exporter<'_>) {
    let mode = |ex: &Exporter<'_>, t: Id| {
        attr(&read_attribute(ex, t, "ATTR_LATEX"), ":mode").unwrap_or_else(|| "table".into())
    };
    for id in ex.tree.descendants(ex.tree.root) {
        if ex.tree.kind(id) != Some(TABLE) || ex.tree.nodes[id].props.contains_key("matrix-member")
        {
            continue;
        }
        let is_org = Latex::cast::<ast::Table>(ex, id)
            .is_some_and(|t| !(t.table_type() == ast::TableType::TableEl));
        let m = mode(ex, id);
        if !is_org || !(m == "math" || m == "inline-math") {
            continue;
        }
        let Some(parent) = ex.tree.parent(id) else {
            continue;
        };
        let has_caption = Latex::has_caption(ex, id);
        let has_name = Latex::name(ex, id).is_some();
        let markup = if m == "inline-math" {
            "inline"
        } else if has_caption || has_name {
            "equation"
        } else {
            "math"
        };
        let mut members = vec![id];
        let mut previous = id;
        loop {
            if ex.tree.nodes[previous].post_blank != 0 {
                break;
            }
            let Some(next) = ex.next_element(previous) else {
                break;
            };
            let ok = ex.tree.kind(next) == Some(TABLE)
                && Latex::cast::<ast::Table>(ex, next)
                    .is_some_and(|t| !(t.table_type() == ast::TableType::TableEl))
                && mode(ex, next) == m;
            if !ok {
                break;
            }
            ex.tree.nodes[next]
                .props
                .insert("matrix-member", String::new());
            members.push(next);
            previous = next;
        }
        let blank = ex.tree.nodes[previous].post_blank;
        let list = members
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        ex.tree.nodes[id].props.insert("matrices", list);
        ex.tree.nodes[id]
            .props
            .insert("matrices-markup", markup.to_string());
        ex.tree.nodes[parent]
            .children
            .retain(|c| *c == id || !members.contains(c));
        ex.tree.nodes[id].post_blank = blank;
    }
}

impl Backend for Latex {
    fn name(&self) -> &'static str {
        "latex"
    }

    fn filter_parse_tree(&self, ex: &mut Exporter<'_>) {
        wrap_matrices(ex);
        let mut root = vec![ex.tree.root];
        wrap_math_blocks(ex, &mut root);
        // The title, author and date too.
        for k in ["title", "author", "date"] {
            if let Some(mut ids) = ex.info.parsed.get(k).cloned() {
                wrap_math_blocks(ex, &mut ids);
                ex.info.parsed.insert(k.to_string(), ids);
            }
        }
        ex.insert_image_links(image_path);
    }

    fn has_transcoder(&self, kind: SyntaxKind) -> bool {
        matches!(
            kind,
            BOLD | CENTER_BLOCK
                | CLOCK
                | CODE
                | DRAWER
                | DYNAMIC_BLOCK
                | ENTITY
                | EXAMPLE_BLOCK
                | EXPORT_BLOCK
                | EXPORT_SNIPPET
                | FIXED_WIDTH
                | FOOTNOTE_REFERENCE
                | HEADLINE
                | HORIZONTAL_RULE
                | INLINE_SRC_BLOCK
                | INLINETASK
                | ITALIC
                | ITEM
                | KEYWORD
                | LATEX_ENVIRONMENT
                | LATEX_FRAGMENT
                | LINE_BREAK
                | LINK
                | NODE_PROPERTY
                | PARAGRAPH
                | PLAIN_LIST
                | PLANNING
                | PROPERTY_DRAWER
                | QUOTE_BLOCK
                | RADIO_TARGET
                | SECTION
                | SPECIAL_BLOCK
                | SRC_BLOCK
                | STATISTICS_COOKIE
                | STRIKE_THROUGH
                | SUBSCRIPT
                | SUPERSCRIPT
                | TABLE
                | TABLE_CELL
                | TABLE_ROW
                | TARGET
                | TIMESTAMP
                | UNDERLINE
                | VERBATIM
                | VERSE_BLOCK
        )
    }

    fn options(&self) -> Vec<crate::export::BackendOption> {
        let opt = |p, k, b, v| (p, Some(k), None, b, v);
        vec![
            opt(
                "latex-default-footnote-command",
                "LATEX_FOOTNOTE_COMMAND",
                Behavior::First,
                Value::Str("\\footnote{%s%s}".into()),
            ),
            opt(
                "latex-engraved-theme",
                "LATEX_ENGRAVED_THEME",
                Behavior::First,
                Value::Nil,
            ),
            opt(
                "latex-class",
                "LATEX_CLASS",
                Behavior::Last,
                Value::Str("article".into()),
            ),
            opt(
                "latex-class-options",
                "LATEX_CLASS_OPTIONS",
                Behavior::Last,
                Value::Nil,
            ),
            opt(
                "latex-header",
                "LATEX_HEADER",
                Behavior::Newline,
                Value::Nil,
            ),
            opt(
                "latex-header-extra",
                "LATEX_HEADER_EXTRA",
                Behavior::Newline,
                Value::Nil,
            ),
            opt("description", "DESCRIPTION", Behavior::Parse, Value::Nil),
            opt("keywords", "KEYWORDS", Behavior::Parse, Value::Nil),
            opt("subtitle", "SUBTITLE", Behavior::Parse, Value::Nil),
            opt(
                "latex-compiler",
                "LATEX_COMPILER",
                Behavior::First,
                Value::Str("pdflatex".into()),
            ),
            (
                "latex-default-figure-position",
                None,
                None,
                Behavior::First,
                Value::Str("htbp".into()),
            ),
        ]
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        let special = ex.flag("with-special-strings");
        let mut out = String::with_capacity(text.len() + 8);
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if special {
                        match chars.peek() {
                            Some('-') => out.push('\\'),
                            Some(&n) => {
                                out.push_str("$\\backslash$");
                                out.push(n);
                                chars.next();
                            }
                            None => out.push_str("$\\backslash$"),
                        }
                    } else {
                        out.push_str("$\\backslash$");
                    }
                }
                '~' => out.push_str("\\textasciitilde{}"),
                '^' => out.push_str("\\^{}"),
                '%' | '$' | '#' | '&' | '{' | '}' | '_' => {
                    out.push('\\');
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        let mut out = tex_logos(&out);
        if ex.flag("with-smart-quotes") {
            out = ex.smart_quotes(ex.current_text, &out, crate::quotes::Encoding::Latex);
        }
        if special {
            out = out.replace("...", "\\ldots{}");
        }
        if ex.flag("preserve-breaks") {
            let mut s = String::new();
            for line in out.split_inclusive('\n') {
                match line.strip_suffix('\n') {
                    Some(l) => {
                        let l = l.trim_end_matches([' ', '\t']);
                        let l = l.strip_suffix("\\\\").unwrap_or(l);
                        s.push_str(l.trim_end_matches([' ', '\t']));
                        s.push_str("\\\\\n");
                    }
                    None => s.push_str(line),
                }
            }
            out = s;
        }
        // A bracket at the start of a line would read as an option.
        let mut s = String::with_capacity(out.len());
        let mut bol = true;
        for c in out.chars() {
            if bol && c == '[' {
                s.push_str("{[}");
                bol = false;
                continue;
            }
            if c == '\n' {
                bol = true;
            } else if !c.is_whitespace() {
                bol = false;
            }
            s.push(c);
        }
        s
    }

    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        let kind = ex.tree.kind(id)?;
        let out = self.transcode_node(ex, id, kind, contents)?;
        if self.source_lines
            && kind.is_element()
            && !matches!(kind, SECTION | ITEM | TABLE_ROW)
            && ex
                .tree
                .parent(id)
                .is_some_and(|p| matches!(ex.tree.kind(p), Some(SECTION | DOCUMENT)))
            && let Some(line) = Self::source_line(ex, id)
        {
            return Some(format!("%% org:{line}\n{out}"));
        }
        Some(out)
    }

    fn filter_final_output(&self, _: &mut Exporter<'_>, out: String) -> String {
        out.replace([OPEN_MARK, crate::kalem::END_MARK], "")
    }

    fn template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let title_ids = ex.info.parsed.get("title").cloned().unwrap_or_default();
        let title = finish(&ex.data_list(&title_ids));
        let mut out = String::new();
        if ex.flag("time-stamp-file")
            && let Some(now) = ex.info.now.clone()
        {
            out.push_str(&crate::macros::format_time(
                "%% Created %Y-%m-%d %a %H:%M\n",
                &now,
            ));
        }
        let compiler = option_string(ex, "latex-compiler").unwrap_or_default();
        if ["pdflatex", "xelatex", "lualatex"].contains(&compiler.as_str()) {
            out.push_str(&format!("% Intended LaTeX compiler: {compiler}\n"));
        }
        out.push_str(&preamble(ex, &compiler));
        // Kalem's formatting: colors, and the document's defaults.
        let defaults = crate::kalem::defaults(&ex.info.keywords);
        if uses_kalem_colors(ex) {
            out.push_str("\\usepackage{xcolor}\n");
        }
        if let Some(sp) = &defaults.spacing {
            out.push_str(&format!("\\usepackage{{setspace}}\n\\setstretch{{{sp}}}\n"));
        }
        if let Some(font) = defaults.font.as_ref().filter(|_| unicode_engine(ex)) {
            out.push_str(&format!("\\setmainfont{{{}}}\n", protect_text(font)));
        }
        if let Value::Int(n) = ex.opt("section-numbers") {
            out.push_str(&format!("\\setcounter{{secnumdepth}}{{{n}}}\n"));
        }
        let author = if ex.flag("with-author") {
            ex.info
                .parsed
                .get("author")
                .cloned()
                .filter(|a| !a.is_empty())
                .map(|a| ex.data_list(&a))
        } else {
            None
        };
        let email = if ex.flag("with-email") {
            ex.string("email").map(str::to_string)
        } else {
            None
        };
        match (&author, &email) {
            (Some(a), Some(e)) if !e.is_empty() => {
                out.push_str(&format!("\\author{{{a}\\thanks{{{e}}}}}\n"));
            }
            (Some(a), _) => out.push_str(&format!("\\author{{{a}}}\n")),
            (None, Some(e)) => out.push_str(&format!("\\author{{{e}}}\n")),
            (None, None) => {}
        }
        let date = if ex.flag("with-date") {
            match ex.info.parsed.get("date").cloned() {
                Some(d) => ex.data_list(&d),
                None => "\\today".into(),
            }
        } else {
            String::new()
        };
        out.push_str(&format!("\\date{{{date}}}\n"));
        let subtitle = ex
            .info
            .parsed
            .get("subtitle")
            .cloned()
            .map(|s| ex.data_list(&s))
            .map(|s| format!("\\\\\\medskip\n\\large {s}"));
        out.push_str(&format!(
            "\\title{{{title}{}}}\n",
            subtitle.unwrap_or_default()
        ));
        let lang = ex.string("language").unwrap_or("en").to_string();
        let lang_name = language(&lang).map_or(lang.clone(), |l| l.6.to_string());
        let keywords = ex
            .info
            .parsed
            .get("keywords")
            .cloned()
            .map(|k| ex.data_list(&k))
            .unwrap_or_default();
        let description = ex
            .info
            .parsed
            .get("description")
            .cloned()
            .map(|k| ex.data_list(&k))
            .unwrap_or_default();
        let creator = option_string(ex, "creator").unwrap_or_default();
        let author_plain = ex
            .info
            .parsed
            .get("author")
            .cloned()
            .map(|a| ex.data_list(&a))
            .unwrap_or_default();
        out.push_str(&format!(
            "\\hypersetup{{\n pdfauthor={{{author_plain}}},\n pdftitle={{{title}}},\n pdfkeywords={{{keywords}}},\n pdfsubject={{{description}}},\n pdfcreator={{{creator}}}, \n pdflang={{{}}}}}\n",
            capitalize(&lang_name)
        ));
        out.push_str("\\begin{document}\n\n");
        if let Some(size) = defaults.size.as_deref().and_then(|s| s.parse::<f64>().ok()) {
            out.push_str(&format!(
                "\\fontsize{{{}pt}}{{{}pt}}\\selectfont\n",
                lisp_number(size),
                lisp_number((size * 12.0).round() / 10.0)
            ));
        }
        if ex.flag("with-title") && !title.is_empty() {
            out.push_str("\\maketitle\n");
        }
        match ex.opt("with-toc") {
            Value::Nil => {}
            Value::Int(n) => {
                out.push_str(&format!(
                    "\\setcounter{{tocdepth}}{{{n}}}\n\\tableofcontents\n\n"
                ));
            }
            _ => out.push_str("\\tableofcontents\n\n"),
        }
        out.push_str(&body);
        if ex.flag("with-creator") {
            out.push_str(&format!("{creator}\n"));
        }
        out.push_str("\\end{document}");
        out
    }
}

impl Latex {
    fn transcode_node(
        &self,
        ex: &mut Exporter<'_>,
        id: Id,
        kind: SyntaxKind,
        contents: Option<String>,
    ) -> Option<String> {
        let c = || contents.clone().unwrap_or_default();
        Some(match kind {
            BOLD | ITALIC | STRIKE_THROUGH | UNDERLINE => text_markup(&c(), kind),
            CODE => text_markup(&Self::cast::<ast::Code>(ex, id)?.value(), CODE),
            VERBATIM => text_markup(&Self::cast::<ast::Verbatim>(ex, id)?.value(), VERBATIM),
            CENTER_BLOCK => {
                let out = format!("\\begin{{center}}\n{}\\end{{center}}", c());
                self.wrap_label(ex, id, out)
            }
            CLOCK => {
                let cl: ast::Clock = Self::cast(ex, id)?;
                let ts = cl
                    .timestamp()
                    .map(|t| crate::timestamps::interpret(ast::AstNode::syntax(&t)))
                    .unwrap_or_default();
                let dur = cl.duration().map(|d| format!(" ({d})")).unwrap_or_default();
                format!("\\noindent\\textbf{{CLOCK:}} \\textit{{{ts}{dur}}}\\\\")
            }
            DRAWER | DYNAMIC_BLOCK => self.wrap_label(ex, id, c()),
            ENTITY => {
                if let Some(members) = ex.tree.nodes[id].props.get("math-block").cloned() {
                    return self.math_block(ex, &members);
                }
                Self::cast::<ast::Entity>(ex, id)?.latex()?.0.to_string()
            }
            EXAMPLE_BLOCK => {
                let b: ast::ExampleBlock = Self::cast(ex, id)?;
                if !nw(&b.value()) {
                    return None;
                }
                let env = attr(&read_attribute(ex, id, "ATTR_LATEX"), ":environment")
                    .unwrap_or_else(|| "verbatim".into());
                let out = format!(
                    "\\begin{{{env}}}\n{}\\end{{{env}}}",
                    crate::md::format_code_default(ex, id)
                );
                self.wrap_label(ex, id, out)
            }
            EXPORT_BLOCK => {
                let b: ast::ExportBlock = Self::cast(ex, id)?;
                match b.backend().as_deref() {
                    Some("LATEX" | "TEX") => html::remove_indentation(&b.value()),
                    _ => return None,
                }
            }
            EXPORT_SNIPPET => {
                let s: ast::ExportSnippet = Self::cast(ex, id)?;
                match s.backend().as_str() {
                    "latex" => s.value(),
                    // Kalem's formatting; a span ends where its container
                    // does at the latest (`finish`).
                    "kalem" => match crate::kalem::Format::parse(&s.value()) {
                        Some(f) => kalem_open(ex, &f),
                        None => crate::kalem::END_MARK.to_string(),
                    },
                    _ => return None,
                }
            }
            FIXED_WIDTH => {
                let v = Self::cast::<ast::FixedWidth>(ex, id)?.value();
                let v = html::remove_indentation(&v);
                let v = v.strip_suffix('\n').unwrap_or(&v);
                let out = format!("\\begin{{verbatim}}\n{v}\n\\end{{verbatim}}");
                self.wrap_label(ex, id, out)
            }
            FOOTNOTE_REFERENCE => self.footnote_reference(ex, id),
            HEADLINE => return self.headline(ex, id, contents),
            HORIZONTAL_RULE => {
                let attrs = read_attribute(ex, id, "ATTR_LATEX");
                let prev = ex.previous_element(id);
                let nl = if prev.is_some_and(|p| ex.tree.nodes[p].post_blank == 0) {
                    "\n"
                } else {
                    ""
                };
                let out = format!(
                    "\\noindent\\rule{{{}}}{{{}}}",
                    attr(&attrs, ":width").unwrap_or_else(|| "\\textwidth".into()),
                    attr(&attrs, ":thickness").unwrap_or_else(|| "0.5pt".into())
                );
                format!("{nl}{}", self.wrap_label(ex, id, out))
            }
            INLINE_SRC_BLOCK => {
                let b: ast::InlineSrcBlock = Self::cast(ex, id)?;
                text_markup(&b.value(), CODE)
            }
            INLINETASK => {
                let ids = ex
                    .tree
                    .secondary(id, Secondary::Title)
                    .map(<[Id]>::to_vec)
                    .unwrap_or_default();
                let title = ex.data_list(&ids);
                let todo = self.todo(ex, id);
                let tags = if ex.flag("with-tags") {
                    ex.tags(id, &[], false)
                } else {
                    Vec::new()
                };
                let priority = Self::priority(ex, id);
                let label = self.label(ex, id, false).unwrap_or_default();
                let contents = format!("{label}{}", c());
                let mut full = String::new();
                if let Some(t) = todo {
                    full.push_str(&format!("\\textbf{{\\textsf{{\\textsc{{{t}}}}}}} "));
                }
                if let Some(p) = priority {
                    full.push_str(&format!("\\framebox{{\\#{p}}} "));
                }
                full.push_str(&title);
                if !tags.is_empty() {
                    let t: Vec<String> = tags.iter().map(|t| protect_text(t)).collect();
                    full.push_str(&format!("\\hfill{{}}\\textsc{{:{}:}}", t.join(":")));
                }
                let rule = if nw(&contents) {
                    format!("\\rule[.8em]{{\\linewidth}}{{2pt}}\n\n{contents}")
                } else {
                    String::new()
                };
                format!(
                    "\\begin{{center}}\n\\fbox{{\n\\begin{{minipage}}[c]{{.6\\linewidth}}\n{full}\n\n{rule}\\end{{minipage}}\n}}\n\\end{{center}}"
                )
            }
            ITEM => self.item(ex, id, contents),
            KEYWORD => return self.keyword(ex, id),
            LATEX_ENVIRONMENT => return self.latex_environment(ex, id),
            LATEX_FRAGMENT => {
                if let Some(members) = ex.tree.nodes[id].props.get("math-block").cloned() {
                    return self.math_block(ex, &members);
                }
                let v = Self::cast::<ast::LatexFragment>(ex, id)?.value();
                fragment_inner(&v)
            }
            LINE_BREAK => "\\\\\n".to_string(),
            LINK => return self.link(ex, id, contents),
            NODE_PROPERTY => {
                let p: ast::NodeProperty = Self::cast(ex, id)?;
                let v = p.value();
                if v.is_empty() {
                    format!("{}:", p.key())
                } else {
                    format!("{}: {v}", p.key())
                }
            }
            PARAGRAPH => {
                let p = clean_invalid_line_breaks(&finish(&remove_blank_lines(&c())));
                kalem_paragraph(ex, id, p)
            }
            PLAIN_LIST => {
                let attrs = read_attribute(ex, id, "ATTR_LATEX");
                let env = attr(&attrs, ":environment").unwrap_or_else(|| {
                    match html::list_type(ex, id) {
                        html::ListType::Ordered => "enumerate",
                        html::ListType::Descriptive => "description",
                        html::ListType::Unordered => "itemize",
                    }
                    .to_string()
                });
                let out = format!(
                    "\\begin{{{env}}}{}\n{}\\end{{{env}}}",
                    attr(&attrs, ":options").unwrap_or_default(),
                    c()
                );
                self.wrap_label(ex, id, out)
            }
            PLANNING => {
                let p: ast::Planning = Self::cast(ex, id)?;
                let mut parts = Vec::new();
                for (label, ts) in [
                    ("CLOSED:", p.closed()),
                    ("DEADLINE:", p.deadline()),
                    ("SCHEDULED:", p.scheduled()),
                ] {
                    if let Some(t) = ts {
                        let raw = crate::timestamps::interpret(ast::AstNode::syntax(&t));
                        parts.push(format!("\\textbf{{{label}}} \\textit{{{raw}}}"));
                    }
                }
                format!("\\noindent{}\\\\", parts.join(" "))
            }
            PROPERTY_DRAWER => {
                let c = c();
                if !nw(&c) {
                    return None;
                }
                format!("\\begin{{verbatim}}\n{c}\\end{{verbatim}}")
            }
            QUOTE_BLOCK => {
                let attrs = read_attribute(ex, id, "ATTR_LATEX");
                let env = attr(&attrs, ":environment").unwrap_or_else(|| "quote".into());
                let out = format!(
                    "\\begin{{{env}}}{}\n{}\\end{{{env}}}",
                    attr(&attrs, ":options").unwrap_or_default(),
                    c()
                );
                self.wrap_label(ex, id, out)
            }
            RADIO_TARGET => format!("\\label{{{}}}{}", ex.reference(id), c()),
            SECTION => return contents,
            SPECIAL_BLOCK => {
                let b: ast::SpecialBlock = Self::cast(ex, id)?;
                let ty = b.block_type();
                let opt =
                    attr(&read_attribute(ex, id, "ATTR_LATEX"), ":options").unwrap_or_default();
                let caption = self.caption_label(ex, id, None);
                format!("\\begin{{{ty}}}{opt}\n{}{caption}\\end{{{ty}}}", c())
            }
            SRC_BLOCK => return self.src_block(ex, id),
            STATISTICS_COOKIE => ex.tree.source(id).trim_end().replace('%', "\\%"),
            SUBSCRIPT => format!("\\textsubscript{{{}}}", c()),
            SUPERSCRIPT => format!("\\textsuperscript{{{}}}", c()),
            TABLE => return self.table(ex, id, c()),
            TABLE_CELL => self.table_cell(ex, id, contents),
            TABLE_ROW => return self.table_row(ex, id, c()),
            TARGET => format!(
                "\\label{{{}}}",
                self.label(ex, id, false).unwrap_or_default()
            ),
            TIMESTAMP => {
                let raw = crate::timestamps::interpret(ex.syntax(id)?);
                let v = self.plain_text(ex, &raw);
                format!("\\textit{{{v}}}")
            }
            VERSE_BLOCK => self.verse_block(ex, id, c()),
            _ => return None,
        })
    }

    /// `org-latex-math-block`: the formulas of a math block in `\(…\)`.
    fn math_block(&self, ex: &mut Exporter<'_>, members: &str) -> Option<String> {
        let ids: Vec<Id> = members.split(',').filter_map(|s| s.parse().ok()).collect();
        let mut contents = String::new();
        for (i, &m) in ids.iter().enumerate() {
            let one = match ex.tree.kind(m) {
                Some(ENTITY) => Self::cast::<ast::Entity>(ex, m)
                    .and_then(|e| e.latex())
                    .map(|l| l.0.to_string())
                    .unwrap_or_default(),
                _ => Self::cast::<ast::LatexFragment>(ex, m)
                    .map(|f| fragment_inner(&f.value()))
                    .unwrap_or_default(),
            };
            contents.push_str(&one);
            if i + 1 < ids.len() {
                let gap = match i {
                    0 => ex.tree.nodes[m]
                        .props
                        .get("math-gap")
                        .and_then(|g| g.parse().ok())
                        .unwrap_or(ex.tree.nodes[m].post_blank),
                    _ => ex.tree.nodes[m].post_blank,
                };
                contents.push_str(&" ".repeat(gap));
            }
        }
        nw(&contents).then(|| format!("\\({}\\)", trim(&contents)))
    }
}

/// `org-latex-latex-fragment`: `$x$` and `\(x\)` without delimiters.
fn fragment_inner(v: &str) -> String {
    if v.starts_with('$') && !v.starts_with("$$") && v.len() >= 2 {
        v[1..v.len() - 1].to_string()
    } else if let Some(inner) = v.strip_prefix("\\(") {
        inner.strip_suffix("\\)").unwrap_or(inner).to_string()
    } else {
        v.to_string()
    }
}

/// What opens a span of Kalem's formatting until its container is
/// finished ([`finish`]); [`crate::kalem::END_MARK`] closes one.
const OPEN_MARK: char = '\u{E001}';

/// Kalem's formatting of a span as LaTeX: a group, or a `\colorbox` for
/// a highlight, with the size (`\fontsize`), the color (`\color`) and,
/// for XeLaTeX and LuaLaTeX, the font (`\fontspec`).
fn kalem_open(ex: &Exporter<'_>, f: &crate::kalem::Format) -> String {
    let mut cmds = String::new();
    if let Some(font) = &f.font
        && unicode_engine(ex)
    {
        cmds.push_str(&format!("\\fontspec{{{}}}", protect_text(font)));
    }
    if let Some(size) = f.size.as_deref().and_then(|s| s.parse::<f64>().ok()) {
        cmds.push_str(&format!(
            "\\fontsize{{{}pt}}{{{}pt}}\\selectfont ",
            lisp_number(size),
            lisp_number((size * 12.0).round() / 10.0)
        ));
    }
    if let Some(c) = &f.color {
        cmds.push_str(&format!(
            "\\color[HTML]{{{}}}",
            c.trim_start_matches('#').to_uppercase()
        ));
    }
    match &f.background {
        Some(bg) => format!(
            "{OPEN_MARK}\\colorbox[HTML]{{{}}}{{{cmds}",
            bg.trim_start_matches('#').to_uppercase()
        ),
        None => format!("{OPEN_MARK}{{{cmds}"),
    }
}

/// Whether the document is for XeLaTeX or LuaLaTeX, which read system
/// fonts.
fn unicode_engine(ex: &Exporter<'_>) -> bool {
    matches!(
        option_string(ex, "latex-compiler").as_deref(),
        Some("xelatex" | "lualatex")
    )
}

/// A number as written: `14`, `10.5`.
fn lisp_number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// `latex` with each span of Kalem's formatting closed, by its end or at
/// the end of the container (a paragraph, a title, a cell), and ends
/// without a span left out.
fn finish(latex: &str) -> String {
    if !latex.contains([OPEN_MARK, crate::kalem::END_MARK]) {
        return latex.to_string();
    }
    let mut out = String::with_capacity(latex.len());
    let mut depth = 0usize;
    for c in latex.chars() {
        if c == OPEN_MARK {
            depth += 1;
        } else if c == crate::kalem::END_MARK {
            if depth > 0 {
                depth -= 1;
                out.push('}');
            }
        } else {
            out.push(c);
        }
    }
    let body = out.trim_end().len();
    let tail = out.split_off(body);
    out.push_str(&"}".repeat(depth));
    out.push_str(&tail);
    out
}

/// Whether the document uses Kalem's colors or highlights.
fn uses_kalem_colors(ex: &Exporter<'_>) -> bool {
    ex.tree.descendants(ex.tree.root).into_iter().any(|id| {
        Latex::cast::<ast::ExportSnippet>(ex, id).is_some_and(|s| {
            s.backend() == "kalem"
                && crate::kalem::Format::parse(&s.value())
                    .is_some_and(|f| f.color.is_some() || f.background.is_some())
        })
    })
}

/// A paragraph with Kalem's alignment and spacing
/// (`#+ATTR_KALEM: :align right :before 12 :after 6`).
fn kalem_paragraph(ex: &Exporter<'_>, id: Id, text: String) -> String {
    let attrs = read_attribute(ex, id, "ATTR_KALEM");
    if attrs.is_empty() {
        return text;
    }
    let get = |key: &str| attr(&attrs, key).map(|v| v.trim().to_ascii_lowercase());
    let space = |key: &str| {
        get(key)
            .map(|v| v.trim_end_matches("pt").to_string())
            .filter(|v| v.parse::<f64>().is_ok_and(|x| (0.0..=1000.0).contains(&x)))
    };
    let mut out = text;
    let env = match get(":align").as_deref() {
        Some("right") => Some("flushright"),
        Some("left") => Some("flushleft"),
        Some("center") => Some("center"),
        _ => None,
    };
    if let Some(env) = env {
        out = format!(
            "\\begin{{{env}}}\n{}\n\\end{{{env}}}",
            out.trim_end_matches('\n')
        );
    }
    if let Some(b) = space(":before") {
        out = format!("\\vspace{{{b}pt}}\n{out}");
    }
    if let Some(a) = space(":after") {
        out = format!("{}\n\\vspace{{{a}pt}}", out.trim_end_matches('\n'));
    }
    out
}

/// `\TeX{}` and `\LaTeX{}` for the words `TeX` and `LaTeX`.
fn tex_logos(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    // Word characters of Org's syntax table: an apostrophe is one
    // (`LaTeX's` is left alone).
    let word = |c: char| c.is_alphanumeric() || c == '\'';
    while i < s.len() {
        let before_ok = i == 0 || !s[..i].chars().next_back().is_some_and(word);
        let rest = &s[i..];
        let m = if before_ok {
            ["LaTeX", "TeX"]
                .into_iter()
                .find(|w| rest.starts_with(w) && !rest[w.len()..].chars().next().is_some_and(word))
        } else {
            None
        };
        if let Some(w) = m {
            out.push('\\');
            out.push_str(w);
            out.push_str("{}");
            i += w.len();
            continue;
        }
        let c = rest.chars().next().unwrap_or(b[i] as char);
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn capitalize(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `org-latex-make-preamble`: the class line, the default packages for
/// the compiler, and `#+LATEX_HEADER` lines.
fn preamble(ex: &Exporter<'_>, compiler: &str) -> String {
    let class = option_string(ex, "latex-class").unwrap_or_else(|| "article".into());
    let mut header = CLASSES.iter().find(|c| c.0 == class).map_or_else(
        || format!("\\documentclass{{{class}}}"),
        |c| c.1.to_string(),
    );
    if let Some(opts) = option_string(ex, "latex-class-options")
        && let Some(i) = header.find("\\documentclass")
    {
        let after = i + "\\documentclass".len();
        let end = if header[after..].starts_with('[') {
            header[after..].find(']').map_or(after, |j| after + j + 1)
        } else {
            after
        };
        header.replace_range(after..end, &opts);
    }
    let known = ["pdflatex", "xelatex", "lualatex"].contains(&compiler);
    let packages: Vec<String> = DEFAULT_PACKAGES
        .iter()
        .filter(|(_, _, _, compilers)| {
            !known
                || compilers.is_empty()
                || compilers.iter().any(|c| c.eq_ignore_ascii_case(compiler))
        })
        .map(|(o, p, _, _)| {
            if o.is_empty() {
                format!("\\usepackage{{{p}}}")
            } else {
                format!("\\usepackage[{o}]{{{p}}}")
            }
        })
        .collect();
    let extra: String = [
        option_string(ex, "latex-header"),
        option_string(ex, "latex-header-extra"),
    ]
    .into_iter()
    .flatten()
    .map(|s| normalize(&s))
    .collect();
    // `org-splice-latex-header`.
    let mut tpl = header;
    let mut end = String::new();
    let def = packages.join("\n");
    if let Some((s, e)) = placeholder(&tpl, "DEFAULT-PACKAGES") {
        let no = tpl[s..e].contains("NO-");
        tpl.replace_range(
            s..e,
            &if no {
                String::new()
            } else {
                format!("{def}\n")
            },
        );
    } else {
        end = def;
    }
    if let Some((s, e)) = placeholder(&tpl, "PACKAGES") {
        tpl.replace_range(s..e, "");
    }
    if let Some((s, e)) = placeholder(&tpl, "EXTRA") {
        let no = tpl[s..e].contains("NO-");
        tpl.replace_range(
            s..e,
            &if no || extra.is_empty() {
                String::new()
            } else {
                format!("{extra}\n")
            },
        );
    } else if nw(&extra) {
        end = format!("{end}\n{extra}");
    }
    let spliced = if nw(&end) {
        format!("{tpl}\n{end}")
    } else {
        tpl
    };
    let mut out = normalize(&spliced).replace(
        "\\usepackage[AUTO]{inputenc}",
        "\\usepackage[utf8]{inputenc}",
    );
    out = guess_babel(ex, &out);
    guess_polyglossia(ex, &out)
}

/// A `[DEFAULT-PACKAGES]`-style placeholder line: its range, with the
/// blanks and line feed after it.
fn placeholder(tpl: &str, name: &str) -> Option<(usize, usize)> {
    for pat in [format!("[NO-{name}]"), format!("[{name}]")] {
        if let Some(i) = tpl.find(&pat) {
            let mut e = i + pat.len();
            while tpl[e..].starts_with([' ', '\t']) {
                e += 1;
            }
            if tpl[e..].starts_with('\n') {
                e += 1;
            }
            return Some((i, e));
        }
    }
    None
}

/// `org-latex-guess-babel-language`: `AUTO` in the options of Babel
/// becomes the document's language.
fn guess_babel(ex: &Exporter<'_>, header: &str) -> String {
    let code = ex.string("language").unwrap_or("en");
    let Some(lang) = language(code) else {
        return header.to_string();
    };
    let (babel, ini_only, ini_alt) = (lang.1, lang.2, lang.3);
    let mut header = header.to_string();
    // `\\usepackage\[\(.*\)\]{babel}` on one line.
    let found = header
        .split_inclusive('\n')
        .scan(0, |at, l| {
            let start = *at;
            *at += l.len();
            Some((start, l))
        })
        .find_map(|(start, l)| {
            let i = l.find("\\usepackage[")?;
            let close = l.rfind("]{babel}").filter(|&c| c > i)?;
            Some((start + i + "\\usepackage[".len(), start + close))
        });
    if ini_only.is_empty()
        && let Some((opts_start, opts_end)) = found
    {
        let options: Vec<String> = header[opts_start..opts_end]
            .split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect();
        let new: Vec<String> = if options.iter().any(|o| o == babel) {
            options.into_iter().filter(|o| o != "AUTO").collect()
        } else if options.iter().any(|o| o == "AUTO") {
            options
                .into_iter()
                .map(|o| if o == "AUTO" { babel.to_string() } else { o })
                .collect()
        } else {
            let mut o = options;
            o.push(babel.to_string());
            o
        };
        header.replace_range(opts_start..opts_end, &new.join(", "));
    }
    if let Some(i) = header.find("\\babelprovide[")
        && let Some(j) = header[i..].find("]{AUTO}")
    {
        let name = if !ini_alt.is_empty() {
            ini_alt
        } else if !babel.is_empty() {
            babel
        } else {
            ini_only
        };
        let at = i + j + 2;
        header.replace_range(at..at + 4, name);
    }
    header
}

/// `org-latex-guess-polyglossia-language`.
fn guess_polyglossia(ex: &Exporter<'_>, header: &str) -> String {
    let code = ex.string("language").unwrap_or("en").to_string();
    // `\\usepackage\(?:\[\([^]]+?\)\]\){polyglossia}\n`.
    let found = header
        .match_indices("\\usepackage[")
        .find_map(|(start, _)| {
            let from = start + "\\usepackage[".len();
            let close = header[from..].find(']')?;
            header[from + close..]
                .starts_with("]{polyglossia}\n")
                .then_some((start, from + close - start))
        });
    let Some((start, close)) = found else {
        return header.to_string();
    };
    let opts = &header[start + "\\usepackage[".len()..start + close];
    let end = start + close + "]{polyglossia}\n".len();
    let mut langs: Vec<String> = Vec::new();
    for l in opts.replace("AUTO", &code).split(',') {
        let l = l.trim().to_string();
        if !l.is_empty() && !langs.contains(&l) {
            langs.push(l);
        }
    }
    langs.reverse();
    let mut main_set = header.contains("\\setmainlanguage{");
    let mut out = String::from("\\usepackage{polyglossia}\n");
    let this = language(&code);
    for l in langs {
        let name = if l == code {
            this.map_or(l.clone(), |t| t.4.to_string())
        } else {
            l.clone()
        };
        let variant = this
            .filter(|t| !t.5.is_empty())
            .map(|t| format!("[variant={}]", t.5))
            .unwrap_or_default();
        if main_set {
            out.push_str(&format!("\\setotherlanguage{{{name}}}\n"));
        } else {
            main_set = true;
            out.push_str(&format!("\\setmainlanguage{variant}{{{name}}}\n"));
        }
    }
    format!("{}{out}{}", &header[..start], &header[end..])
}

/// A table.el table as `table-generate-source` writes it for LaTeX (the
/// comment it starts with removed, as ox-latex removes it): a `tabular`
/// with a bar between every column, a row for each line of text, cells
/// spanning columns as `\\multicolumn`, rules as `\\hline` or, under cells
/// spanning rows, `\\cline`; the text trimmed, `#$~_^%{}&` escaped with a
/// backslash, `\\` as `$\\backslash$` and `<>|` in math.
pub(crate) fn table_el_latex(value: &str) -> String {
    let text = crate::html::remove_indentation(value);
    let lines: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
    let Some(top) = lines.iter().find(|l| l.first() == Some(&'+')) else {
        return String::new();
    };
    let plus: Vec<usize> = top
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == '+')
        .map(|(i, _)| i)
        .collect();
    if plus.len() < 2 {
        return String::new();
    }
    // Where each column's text starts, and where the last one ends.
    let starts: Vec<usize> = plus[..plus.len() - 1].iter().map(|p| p + 1).collect();
    let right = plus[plus.len() - 1];
    let escape = |s: &str| {
        let mut out = String::new();
        for c in s.chars() {
            match c {
                '#' | '$' | '~' | '_' | '^' | '%' | '{' | '}' | '&' => {
                    out.push('\\');
                    out.push(c);
                }
                '\\' => out.push_str("$\\backslash$"),
                '<' | '>' | '|' => {
                    out.push('$');
                    out.push(c);
                    out.push('$');
                }
                c => out.push(c),
            }
        }
        out
    };
    let at = |l: &Vec<char>, i: usize| l.get(i).copied().unwrap_or(' ');
    let mut out = format!(
        "\\begin{{tabular}}{{|{}}}\n\\hline\n",
        "l|".repeat(starts.len())
    );
    let first = lines
        .iter()
        .position(|l| l.first() == Some(&'+'))
        .unwrap_or(0);
    let last = lines
        .iter()
        .rposition(|l| l.first() == Some(&'+'))
        .unwrap_or(0);
    for l in &lines[first + 1..last] {
        if l.first() == Some(&'+') {
            // A rule: whole, or under the columns it crosses.
            let horizontal = |c: char| c == '-' || c == '=';
            let marks: Vec<bool> = starts.iter().map(|&x| horizontal(at(l, x))).collect();
            if marks.iter().all(|m| *m) {
                out.push_str("\\hline\n");
            } else {
                let mut start: Option<usize> = None;
                for (i, m) in marks.iter().enumerate() {
                    if let Some(s) = start
                        && !m
                    {
                        out.push_str(&format!("\\cline{{{}-{i}}}\n", s + 1));
                        start = None;
                    }
                    if start.is_none() && *m {
                        start = Some(i);
                    }
                }
                if let Some(s) = start {
                    out.push_str(&format!("\\cline{{{}-{}}}\n", s + 1, marks.len()));
                }
            }
            continue;
        }
        // A line of text: its cells, a bar before each column's start
        // ending the cell before, no bar spanning it.
        let mut first_cell = true;
        let mut span = 1;
        let mut from = starts[0];
        let cell =
            |from: usize, to: usize, span: usize, out: &mut String, first_cell: &mut bool| {
                let t: String = (from..to).map(|i| at(l, i)).collect();
                let t = escape(t.trim());
                if !*first_cell {
                    out.push_str(if out.ends_with(' ') { "& " } else { " & " });
                }
                if span > 1 {
                    out.push_str(&format!(
                        "\\multicolumn{{{span}}}{{{}l|}}{{{t}}}",
                        if *first_cell { "|" } else { "" }
                    ));
                } else {
                    out.push_str(&t);
                }
                *first_cell = false;
            };
        for &x in &starts[1..] {
            if at(l, x - 1) == '|' {
                cell(from, x - 1, span, &mut out, &mut first_cell);
                span = 1;
                from = x;
            } else {
                span += 1;
            }
        }
        cell(from, right, span, &mut out, &mut first_cell);
        out.push_str(if out.ends_with(' ') {
            "\\\\\n"
        } else {
            " \\\\\n"
        });
    }
    out.push_str("\\hline\n\\end{tabular}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_el_as_emacs_writes_it() {
        // `table-generate-source` for LaTeX through ox-latex, Org 9.7.11.
        let value =
            "+-----+----+\n| a   | b  |\n| two | &< |\n+-----+----+\n| 1   |    |\n+-----+----+\n";
        assert_eq!(
            super::table_el_latex(value),
            "\\begin{tabular}{|l|l|}\n\\hline\na & b \\\\\ntwo & \\&$<$ \\\\\n\\hline\n1 & \\\\\n\\hline\n\\end{tabular}"
        );
    }

    #[test]
    fn as_emacs_writes_them() {
        // Org 9.7.11's output for the same text.
        let text = "#+TOC: listings\n\n#+CAPTION: Cap\n#+ATTR_LATEX: :mode math\n| a | b |\n\nAn \\alpha\\beta and \\alpha \\beta{} x \\alpha{}\\beta.\n";
        let out = crate::export(
            text,
            &Latex::default(),
            &crate::Settings {
                body_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            out,
            "\\lstlistoflistings\n\n\\begin{equation}\n\\begin{tabular}{cc}\n a & b \\\\\n\\end{tabular}\n\\end{equation}\n\nAn \\(\\alpha \\beta\\) and \\(\\alpha\\) \\(\\beta\\) x \\(\\alpha \\beta\\).\n"
        );
    }

    #[test]
    fn source_lines() {
        let text = "Para one.\n\n* H\nText\n#+begin_quote\nq\n#+end_quote\n";
        let out = crate::export(
            text,
            &Latex { source_lines: true },
            &crate::Settings {
                body_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(out.starts_with("%% org:1\nPara one."), "{out}");
        assert!(out.contains("%% org:3\n\\section{H}"), "{out}");
        assert!(out.contains("%% org:4\nText"), "{out}");
        assert!(
            out.contains("%% org:5\n\\begin{quote}\nq\n\\end{quote}"),
            "{out}"
        );
    }

    #[test]
    fn kalem_formatting() {
        let text = "#+KALEM: size=12 spacing=1.5\nSome @@kalem:color=red size=14@@red @@kalem:bg=yellow@@marked@@kalem:end@@ text@@kalem:end@@ and @@kalem:color=blue@@open\n\n#+ATTR_KALEM: :align right :before 6\nRight.\n";
        let settings = crate::Settings {
            body_only: false,
            now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
            ..Default::default()
        };
        let out = crate::export(text, &Latex::default(), &settings).unwrap();
        assert!(
            out.contains("Some {\\fontsize{14pt}{16.8pt}\\selectfont \\color[HTML]{C00000}red \\colorbox[HTML]{FFF2A8}{marked} text} and {\\color[HTML]{1F5FBF}open}\n"),
            "{out}"
        );
        assert!(
            out.contains("\\vspace{6pt}\n\\begin{flushright}\nRight.\n\\end{flushright}"),
            "{out}"
        );
        assert!(
            out.contains("\\usepackage{xcolor}\n\\usepackage{setspace}\n\\setstretch{1.5}\n"),
            "{out}"
        );
        assert!(
            out.contains("\\begin{document}\n\n\\fontsize{12pt}{14.4pt}\\selectfont\n"),
            "{out}"
        );
        assert!(!out.contains(OPEN_MARK) && !out.contains(crate::kalem::END_MARK));
    }

    #[test]
    fn protection() {
        assert_eq!(protect_text("a_b%c"), "a\\_b\\%c");
        assert_eq!(
            protect_texttt("a--b\\c~"),
            "\\texttt{a-{}-{}b\\textbackslash{}c\\textasciitilde{}}"
        );
        assert_eq!(
            tex_logos("LaTeX and TeX, not TeXt"),
            "\\LaTeX{} and \\TeX{}, not TeXt"
        );
        assert!(math_environment("\\begin{align*}"));
        assert!(!math_environment("\\begin{alignment}"));
        assert_eq!(
            clean_invalid_line_breaks("a\\\\\n\\\\\n\\end{x}\\\\\n"),
            "a\\\\\n\n\\end{x}\n"
        );
        assert_eq!(format_two("\\section{%s}\n%s", "T", "B"), "\\section{T}\nB");
    }
}
