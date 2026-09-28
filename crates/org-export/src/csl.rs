//! The `csl` citation processor (`oc-csl.el`): citations and
//! bibliographies in a CSL style, rendered by `org_cite::csl`
//! (hayagriva) where Org uses `citeproc-el`. HTML (and the back-ends
//! derived from it) gets `citeproc`'s HTML, LaTeX its `cslbibliography`
//! environment and the preamble defining it, the others Org text.

use std::path::{Path, PathBuf};

use org_cite::csl::{self, CiteRequest, Display, ItemRequest, Library, Mode, Span};
use org_syntax::SyntaxKind::{self, *};

use crate::cite::{self as oc, Processor};
use crate::export::Exporter;
use crate::tree::Id;

/// `org-cite-csl-html-hanging-indent`.
const HTML_HANGING_INDENT: &str = "1.5em";
/// `org-cite-csl-html-label-width-per-char`, in em.
const HTML_LABEL_WIDTH_PER_CHAR: f64 = 0.6;

/// `org-cite-csl-latex-preamble`, with Org's default lengths.
const LATEX_PREAMBLE: &str = "\\usepackage{calc}
\\newlength{\\cslhangindent}
\\setlength{\\cslhangindent}{1.5em}
\\newlength{\\csllabelsep}
\\setlength{\\csllabelsep}{0.6em}
\\newlength{\\csllabelwidth}
\\setlength{\\csllabelwidth}{0.45em * [CSL-MAXLABEL-CHARS]}
\\newenvironment{cslbibliography}[2] % 1st arg. is hanging-indent, 2nd entry spacing.
 {% By default, paragraphs are not indented.
  \\setlength{\\parindent}{0pt}
  % Hanging indent is turned on when first argument is 1.
  \\ifodd #1
  \\let\\oldpar\\par
  \\def\\par{\\hangindent=\\cslhangindent\\oldpar}
  \\fi
  % Set entry spacing based on the second argument.
  \\setlength{\\parskip}{\\parskip +  #2\\baselineskip}
 }%
 {}
\\newcommand{\\cslblock}[1]{#1\\hfill\\break}
\\newcommand{\\cslleftmargin}[1]{\\parbox[t]{\\csllabelsep + \\csllabelwidth}{#1}}
\\newcommand{\\cslrightinline}[1]
  {\\parbox[t]{\\linewidth - \\csllabelsep - \\csllabelwidth}{#1}\\break}
\\newcommand{\\cslindent}[1]{\\hspace{\\cslhangindent}#1}
\\newcommand{\\cslbibitem}[2]
  {\\leavevmode\\vadjust pre{\\hypertarget{citeproc_bib_item_#1}{}}#2}
\\makeatletter
\\newcommand{\\cslcitation}[2]
 {\\protect\\hyper@linkstart{cite}{citeproc_bib_item_#1}#2\\hyper@linkend}
\\makeatother
";

/// What the back-end gets (`org-cite-csl--output-format`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Html,
    Latex,
    Org,
}

/// A citation's references as sent to the processor, with their Org
/// prefixes and suffixes.
struct Cite {
    id: Id,
    /// The references found in the library: the index of each among the
    /// citation's, its prefix and its suffix.
    refs: Vec<(Vec<Id>, Vec<Id>)>,
    bare: bool,
    caps: bool,
}

pub(crate) fn process(
    ex: &mut Exporter<'_>,
    processor: &Processor,
    files: &[PathBuf],
    dir: Option<&Path>,
) -> Result<Option<oc::Finalizer>, String> {
    let language = ex.string("language").map(str::to_string);
    let csl = csl::Processor::new(
        processor.bibliography_style.as_deref(),
        dir,
        language.as_deref(),
    )?;
    let (lib, _errors) = Library::load(files);
    let backend = ex.backend();
    let derived = |b: &str| backend.name() == b || backend.parents().contains(&b);
    let output = if derived("html") {
        Output::Html
    } else if derived("latex") {
        Output::Latex
    } else {
        Output::Org
    };
    let citations = oc::list_citations(ex);
    let mut requests: Vec<CiteRequest> = Vec::new();
    let mut cites: Vec<Cite> = Vec::new();
    for id in citations {
        let Some(c) = oc::citation(ex, id) else {
            continue;
        };
        let (style, variant) = oc::citation_style(&c, processor);
        let has = |names: &[&str]| variant.as_deref().is_some_and(|v| names.contains(&v));
        let bare = has(&["bare", "b", "bare-caps", "bc", "bare-full", "bf", "bcf"]);
        let caps = has(&["caps", "c", "bare-caps", "bc", "caps-full", "cf", "bcf"]);
        let (mode, hidden) = match style.as_deref() {
            Some("author" | "a") => (Mode::Author, false),
            Some("noauthor" | "na" | "year" | "y") => (Mode::Year, false),
            Some("bibentry") => (Mode::Full, false),
            Some("text" | "t") => (Mode::Text, false),
            Some("nocite" | "n") => (Mode::Normal, true),
            _ => (Mode::Normal, false),
        };
        // `org-cite-csl--parse-reference`.
        let refs: Vec<org_syntax::ast::CitationReference> = c.references().collect();
        let mut items = Vec::new();
        let mut affixes = Vec::new();
        let n = refs.len();
        for (i, r) in refs.iter().enumerate() {
            let suffix_text = r.suffix().map(|s| s.text().to_string()).unwrap_or_default();
            let (mut prefix, locator, suffix) = match csl::split_locator(&suffix_text) {
                Some((before, label, value, after)) => {
                    let mut p = objects(ex, r.prefix());
                    if !before.trim().is_empty() {
                        p.extend(ex.parse_secondary(before.trim()));
                    }
                    (p, Some((label, value)), text_objects(ex, &after))
                }
                None => (objects(ex, r.prefix()), None, objects(ex, r.suffix())),
            };
            let mut suffix = suffix;
            if i == 0
                && let Some(g) = c.prefix()
            {
                let mut p = objects(ex, Some(g));
                p.push(ex.tree.text_node(" ".into(), None));
                p.extend(prefix);
                prefix = p;
            }
            if i + 1 == n
                && let Some(g) = c.suffix()
            {
                suffix.push(ex.tree.text_node(" ".into(), None));
                suffix.extend(objects(ex, Some(g)));
            }
            items.push(ItemRequest {
                key: r.key(),
                locator,
                mode,
            });
            affixes.push((prefix, suffix));
        }
        // Notes: the citation goes into a footnote.
        let mut note_number = None;
        if !hidden && !oc::inside_footnote(ex, id) && csl.note_style() {
            oc::adjust_note(ex, id);
            let f = oc::wrap_citation(ex, id);
            note_number = Some(ex.footnote_number(f));
        }
        if !hidden && csl.superscript() {
            oc::set_previous_post_blank(ex, id, 0);
        }
        requests.push(CiteRequest {
            items,
            hidden,
            note_number,
        });
        cites.push(Cite {
            id,
            refs: affixes,
            bare,
            caps,
        });
    }
    let rendered = csl.render(&lib, &requests);
    let (layout_prefix, layout_suffix) = csl.affixes();
    for (cite, spans) in cites.iter().zip(&rendered.citations) {
        let id = cite.id;
        let blanks = ex.tree.nodes[id].post_blank;
        if spans.is_empty() {
            // `nocite`.
            oc::set_previous_post_blank(ex, id, blanks);
            ex.tree.extract(id);
            continue;
        }
        let mut spans = spans.clone();
        if cite.bare {
            strip_affixes(&mut spans, layout_prefix, layout_suffix);
        }
        if cite.caps {
            capitalize_first(&mut spans);
        }
        let ids = nodes(ex, &spans, output, Some(&cite.refs));
        let out = ex.data_list(&ids);
        if let Some(p) = ex.previous_element(id)
            && ex.tree.is_text(p)
            && ex.tree.nodes[p].text.ends_with('"')
        {
            oc::set_previous_post_blank(ex, id, 1);
        }
        let raw = ex
            .tree
            .raw_node(format!("{}{}", out.trim(), " ".repeat(blanks)), None);
        ex.tree.insert_before(id, raw);
        ex.tree.extract(id);
    }
    // The bibliography.
    let Some(bib) = rendered.bibliography else {
        for k in oc::bibliography_keywords(ex) {
            let blanks = ex.tree.nodes[k].post_blank;
            oc::set_raw(ex, k, "\n".repeat(blanks));
        }
        return Ok(None);
    };
    let keywords = oc::bibliography_keywords(ex);
    for &k in &keywords {
        let out = match output {
            Output::Html => html_bibliography(ex, &bib),
            Output::Latex => latex_bibliography(ex, &bib),
            Output::Org => org_bibliography(ex, &bib),
        };
        let blanks = ex.tree.nodes[k].post_blank;
        let out = format!(
            "{}{}",
            crate::export::normalize_string(&out),
            "\n".repeat(blanks)
        );
        oc::set_raw(ex, k, out);
    }
    Ok((output == Output::Latex && !keywords.is_empty()).then(|| {
        oc::Finalizer::Preamble(
            LATEX_PREAMBLE.replace("[CSL-MAXLABEL-CHARS]", &bib.max_label.to_string()),
        )
    }))
}

/// The objects of a prefix or suffix.
fn objects(ex: &mut Exporter<'_>, n: Option<org_syntax::SyntaxNode>) -> Vec<Id> {
    match n {
        Some(n) => ex.tree.objects_of(&n),
        None => Vec::new(),
    }
}

/// Org text as objects, its blanks kept.
fn text_objects(ex: &mut Exporter<'_>, t: &str) -> Vec<Id> {
    if t.trim().is_empty() {
        return Vec::new();
    }
    let lead = &t[..t.len() - t.trim_start().len()];
    let mut out = Vec::new();
    if !lead.is_empty() {
        out.push(ex.tree.text_node(lead.to_string(), None));
    }
    out.extend(ex.parse_secondary(t.trim()));
    out
}

/// `:suppress-affixes`: the style's text around the citation taken off.
fn strip_affixes(spans: &mut [Span], prefix: Option<&str>, suffix: Option<&str>) {
    if let Some(p) = prefix.filter(|p| !p.is_empty())
        && let Some(Span::Text(t, _)) = first_text(spans)
        && let Some(rest) = t.strip_prefix(p)
    {
        *t = rest.to_string();
    }
    if let Some(s) = suffix.filter(|s| !s.is_empty())
        && let Some(Span::Text(t, _)) = last_text(spans)
        && let Some(rest) = t.strip_suffix(s)
    {
        *t = rest.to_string();
    }
}

fn first_text(spans: &mut [Span]) -> Option<&mut Span> {
    for s in spans.iter_mut() {
        match s {
            Span::Text(t, _) if t.is_empty() => {}
            Span::Text(..) => return Some(s),
            Span::Group(_, c) | Span::Entry(_, c) | Span::Link(_, c) => {
                if let Some(t) = first_text(c) {
                    return Some(t);
                }
            }
        }
    }
    None
}

fn last_text(spans: &mut [Span]) -> Option<&mut Span> {
    for s in spans.iter_mut().rev() {
        match s {
            Span::Text(t, _) if t.is_empty() => {}
            Span::Text(..) => return Some(s),
            Span::Group(_, c) | Span::Entry(_, c) | Span::Link(_, c) => {
                if let Some(t) = last_text(c) {
                    return Some(t);
                }
            }
        }
    }
    None
}

/// `:capitalize-first`.
fn capitalize_first(spans: &mut [Span]) {
    if let Some(Span::Text(t, _)) = first_text(spans) {
        let mut c = t.chars();
        if let Some(f) = c.next() {
            *t = f.to_uppercase().chain(c).collect();
        }
    }
}

/// The nodes of rendered text; `refs` puts each reference's prefix and
/// suffix around its part of a citation.
fn nodes(
    ex: &mut Exporter<'_>,
    spans: &[Span],
    output: Output,
    refs: Option<&[(Vec<Id>, Vec<Id>)]>,
) -> Vec<Id> {
    let mut out = Vec::new();
    for s in spans {
        match s {
            Span::Text(t, f) => {
                // `citeproc-el` escapes its HTML and LaTeX itself.
                let mut ids = vec![match output {
                    Output::Html => ex.tree.raw_node(html_escape(t), None),
                    Output::Latex => ex.tree.raw_node(latex_escape(t), None),
                    Output::Org => ex.tree.text_node(t.clone(), None),
                }];
                let wrap = |ex: &mut Exporter<'_>, k: SyntaxKind, ids: Vec<Id>| {
                    vec![ex.tree.made_node(k, ids, None)]
                };
                if f.small_caps {
                    ids = match output {
                        Output::Html => raw_around(
                            ex,
                            "<span style=\"font-variant:small-caps;\">",
                            ids,
                            "</span>",
                        ),
                        Output::Latex => raw_around(ex, "\\textsc{", ids, "}"),
                        Output::Org => ids,
                    };
                }
                if f.superscript {
                    ids = wrap(ex, SUPERSCRIPT, ids);
                }
                if f.subscript {
                    ids = wrap(ex, SUBSCRIPT, ids);
                }
                if f.underline {
                    ids = wrap(ex, UNDERLINE, ids);
                }
                if f.bold {
                    ids = wrap(ex, BOLD, ids);
                }
                if f.italic {
                    ids = wrap(ex, ITALIC, ids);
                }
                out.extend(ids);
            }
            Span::Entry(i, children) => {
                let inner = nodes(ex, children, output, None);
                match refs.and_then(|r| r.get(*i)) {
                    Some((prefix, suffix)) => {
                        out.extend(prefix.iter().copied());
                        if !prefix.is_empty() && !ends_blank(ex, prefix) {
                            out.push(ex.tree.text_node(" ".into(), None));
                        }
                        out.extend(inner);
                        if !suffix.is_empty() && !starts_blank_or_punct(ex, suffix) {
                            out.push(ex.tree.text_node(" ".into(), None));
                        }
                        out.extend(suffix.iter().copied());
                    }
                    None => out.extend(inner),
                }
            }
            Span::Group(display, children) => {
                let inner = nodes(ex, children, output, refs);
                let class = |d: &Display| match d {
                    Display::Block => "block",
                    Display::LeftMargin => "left-margin",
                    Display::RightInline => "right-inline",
                    Display::Indent => "indent",
                };
                match (output, display) {
                    (Output::Html, Some(d)) => {
                        let open = format!("<div class=\"csl-{}\">", class(d));
                        out.extend(raw_around(ex, &open, inner, "</div>"));
                    }
                    (Output::Latex, Some(d)) => {
                        let open = format!("\\csl{}{{", class(d).replace('-', ""));
                        out.extend(raw_around(ex, &open, inner, "}"));
                    }
                    _ => out.extend(inner),
                }
            }
            Span::Link(url, children) => {
                let inner = nodes(ex, children, output, refs);
                match output {
                    Output::Html => {
                        let open = format!("<a href=\"{}\">", html_escape(url));
                        out.extend(raw_around(ex, &open, inner, "</a>"));
                    }
                    Output::Latex => {
                        let open = format!("\\href{{{}}}{{", latex_url(url));
                        out.extend(raw_around(ex, &open, inner, "}"));
                    }
                    Output::Org => out.extend(inner),
                }
            }
        }
    }
    out
}

fn ends_blank(ex: &Exporter<'_>, ids: &[Id]) -> bool {
    ids.last().is_some_and(|&i| {
        (ex.tree.is_text(i) && ex.tree.nodes[i].text.ends_with(char::is_whitespace))
            || (!ex.tree.is_text(i) && ex.tree.nodes[i].post_blank > 0)
    })
}

fn starts_blank_or_punct(ex: &Exporter<'_>, ids: &[Id]) -> bool {
    ids.first().is_some_and(|&i| {
        ex.tree.is_text(i)
            && ex.tree.nodes[i]
                .text
                .starts_with(|c: char| c.is_whitespace() || ",.;:!?)".contains(c))
    })
}

fn raw_around(ex: &mut Exporter<'_>, open: &str, inner: Vec<Id>, close: &str) -> Vec<Id> {
    let mut v = vec![ex.tree.raw_node(open.to_string(), None)];
    v.extend(inner);
    v.push(ex.tree.raw_node(close.to_string(), None));
    v
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `citeproc-fmt--latex-escape`.
fn latex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '$' | '&' | '#' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(c),
        }
    }
    out
}

fn latex_url(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('#', "\\#")
        .replace('%', "\\%")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

/// A bibliography entry's first field and contents, exported.
fn entry(
    ex: &mut Exporter<'_>,
    first: &Option<Vec<Span>>,
    content: &[Span],
    output: Output,
) -> (Option<String>, String) {
    let first = first.as_ref().map(|f| {
        let ids = nodes(ex, f, output, None);
        ex.data_list(&ids)
    });
    let ids = nodes(ex, content, output, None);
    (first, ex.data_list(&ids))
}

/// `citeproc-el`'s HTML bibliography, with Org's styles for hanging
/// indents and aligned labels.
fn html_bibliography(ex: &mut Exporter<'_>, bib: &csl::Bibliography) -> String {
    let mut out = String::new();
    if bib.second_field_align {
        let width = bib.max_label as f64 * HTML_LABEL_WIDTH_PER_CHAR;
        out.push_str(&format!(
            "<style>.csl-left-margin{{float: left; padding-right: 0em;}}\n .csl-right-inline{{margin: 0 0 0 {}em;}}</style>",
            format_number(width)
        ));
    }
    if bib.hanging_indent {
        out.push_str(&format!(
            "<style>.csl-entry{{text-indent: -{HTML_HANGING_INDENT}; margin-left: {HTML_HANGING_INDENT};}}</style>"
        ));
    }
    out.push_str("<div class=\"csl-bib-body\">\n");
    for (n, (_, first, content)) in bib.items.iter().enumerate() {
        let (first, text) = entry(ex, first, content, Output::Html);
        let body = match first {
            Some(f) if bib.second_field_align => format!(
                "\n    <div class=\"csl-left-margin\">{}</div><div class=\"csl-right-inline\">{}</div>\n  ",
                f.trim_end(),
                text.trim()
            ),
            Some(f) => format!("{f}{text}"),
            None => text,
        };
        out.push_str(&format!(
            "  <div class=\"csl-entry\"><a id=\"citeproc_bib_item_{}\"></a>{body}</div>\n",
            n + 1
        ));
    }
    out.push_str("</div>");
    out
}

fn format_number(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `citeproc-el`'s `org-latex` bibliography.
fn latex_bibliography(ex: &mut Exporter<'_>, bib: &csl::Bibliography) -> String {
    let mut out = format!(
        "\\begin{{cslbibliography}}{{{}}}{{{}}}\n",
        u8::from(bib.hanging_indent),
        bib.entry_spacing
    );
    for (n, (_, first, content)) in bib.items.iter().enumerate() {
        let (first, text) = entry(ex, first, content, Output::Latex);
        let body = match first {
            Some(f) if bib.second_field_align => format!(
                "\\cslleftmargin{{{}}}\\cslrightinline{{{}}}",
                f.trim_end(),
                text.trim()
            ),
            Some(f) => format!("{f}{text}"),
            None => text,
        };
        out.push_str(&format!("\\cslbibitem{{{}}}{{{body}}}\n\n", n + 1));
    }
    out.push_str("\\end{cslbibliography}");
    out
}

/// The bibliography as Org paragraphs, one per entry.
fn org_bibliography(ex: &mut Exporter<'_>, bib: &csl::Bibliography) -> String {
    let mut parts = Vec::new();
    for (_, first, content) in &bib.items {
        let mut children = Vec::new();
        if let Some(f) = first {
            children.extend(nodes(ex, f, Output::Org, None));
            children.push(ex.tree.text_node(" ".into(), None));
        }
        children.extend(nodes(ex, content, Output::Org, None));
        let p = ex.tree.made_node(PARAGRAPH, children, None);
        parts.push(ex.data(p));
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use crate::{Html, Latex, Markdown, Settings, Text, export};

    fn dir() -> std::path::PathBuf {
        // Each test its own folder: they run at the same time.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("org-export-csl-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("refs.bib"),
            "@book{knuth84, author = {Donald E. Knuth}, title = {The TeXbook}, publisher = {Addison-Wesley}, year = 1984}\n\
             @article{doe20, author = {Doe, Jane and Smith, John}, title = {A study}, journal = {Journal}, year = 2020, volume = 3, pages = {1--10}}\n",
        )
        .unwrap();
        d
    }

    fn run(text: &str, backend: &dyn crate::Backend) -> String {
        let d = dir();
        let file = d.join("doc.org");
        export(
            text,
            backend,
            &Settings {
                body_only: true,
                input_file: Some(file),
                now: None,
                subtree: None,
                math: None,
                options: None,
            },
        )
        .unwrap()
    }

    const DOC: &str = "#+cite_export: csl\n#+bibliography: refs.bib\n\nAs [cite:see @knuth84 p. 12] and [cite/t:@doe20] show, [cite/a:@doe20] agree[cite/n:@nope].\n\n#+print_bibliography:\n";

    #[test]
    fn author_date_html() {
        let out = run(DOC, &Html);
        assert!(
            out.contains(
                "As (see Knuth 1984, 12) and Doe and Smith (2020) show, Doe and Smith agree."
            ),
            "{out}"
        );
        assert!(out.contains("<div class=\"csl-bib-body\">"), "{out}");
        assert!(
            out.contains("<div class=\"csl-entry\"><a id=\"citeproc_bib_item_2\"></a>Knuth, Donald E. 1984. <i>The Texbook</i>. Addison-Wesley.</div>"),
            "{out}"
        );
        assert!(out.contains(".csl-entry{text-indent: -1.5em"), "{out}");
    }

    #[test]
    fn text_and_markdown() {
        let out = run(DOC, &Text { utf8: false });
        assert!(
            out.contains("As (see Knuth 1984, 12) and Doe and Smith (2020)"),
            "{out}"
        );
        assert!(
            out.contains("Knuth, Donald E. 1984. /The Texbook/. Addison-Wesley."),
            "{out}"
        );
        let md = run(DOC, &Markdown);
        assert!(md.contains("<div class=\"csl-bib-body\">"), "{md}");
    }

    #[test]
    fn numeric_latex_and_notes() {
        let doc = DOC.replace("#+cite_export: csl", "#+cite_export: csl ieee");
        let out = run(
            &doc,
            &Latex {
                source_lines: false,
            },
        );
        assert!(out.contains("As see [1, p. 12] and"), "{out}");
        assert!(out.contains("\\begin{cslbibliography}{0}{0}"), "{out}");
        assert!(
            out.contains("\\cslbibitem{1}{\\cslleftmargin{[1]}\\cslrightinline{"),
            "{out}"
        );
        let doc = DOC.replace("#+cite_export: csl", "#+cite_export: csl chicago-notes");
        let out = run(&doc, &Html);
        assert!(out.contains("As<sup>"), "{out}");
        assert!(out.contains("footnotes"), "{out}");
    }

    #[test]
    fn unknown_style() {
        let doc = DOC.replace("#+cite_export: csl", "#+cite_export: csl nothing-like-it");
        let err = export(&doc, &Html, &Settings::default()).unwrap_err();
        assert!(err.contains("CSL style file not found"), "{err}");
    }
}
