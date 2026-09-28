//! Citations left to LaTeX (`oc-natbib.el`, `oc-biblatex.el`): each
//! citation becomes a `natbib` or `biblatex` command, the bibliography a
//! `\bibliography` or `\printbibliography`, and the package is loaded in
//! the preamble.

use org_syntax::SyntaxNode;
use org_syntax::ast::{self, AstNode};

use crate::cite::{self as oc, Finalizer, Processor};
use crate::export::Exporter;

/// `org-cite-natbib-bibliography-style`.
const NATBIB_STYLE: &str = "unsrtnat";

/// `org-cite-biblatex-styles`: style, variant, command, multicite
/// command, no optional arguments.
type BiblatexStyle = (
    Option<&'static str>,
    Option<&'static str>,
    &'static str,
    Option<&'static str>,
    bool,
);

const BIBLATEX_STYLES: &[BiblatexStyle] = &[
    (Some("author"), Some("caps"), "Citeauthor*", None, false),
    (Some("author"), Some("full"), "citeauthor", None, false),
    (Some("author"), Some("caps-full"), "Citeauthor", None, false),
    (Some("author"), None, "citeauthor*", None, false),
    (Some("locators"), Some("bare"), "notecite", None, false),
    (Some("locators"), Some("caps"), "Pnotecite", None, false),
    (Some("locators"), Some("bare-caps"), "Notecite", None, false),
    (Some("locators"), None, "pnotecite", None, false),
    (Some("noauthor"), Some("bare"), "cite*", None, false),
    (Some("noauthor"), None, "autocite*", None, false),
    (Some("nocite"), None, "nocite", None, true),
    (
        Some("text"),
        Some("caps"),
        "Textcite",
        Some("Textcites"),
        false,
    ),
    (Some("text"), None, "textcite", Some("textcites"), false),
    (None, Some("bare"), "cite", Some("cites"), false),
    (None, Some("caps"), "Autocite", Some("Autocites"), false),
    (None, Some("bare-caps"), "Cite", Some("Cites"), false),
    (None, None, "autocite", Some("autocites"), false),
];

/// `org-cite-biblatex-style-shortcuts`.
const SHORTCUTS: &[(&str, &str)] = &[
    ("a", "author"),
    ("b", "bare"),
    ("bc", "bare-caps"),
    ("c", "caps"),
    ("cf", "caps-full"),
    ("f", "full"),
    ("l", "locators"),
    ("n", "nocite"),
    ("na", "noauthor"),
    ("t", "text"),
];

/// Replaces the citations and bibliography keywords; `files` are the
/// `#+BIBLIOGRAPHY:` files as written.
pub(crate) fn process(
    ex: &mut Exporter<'_>,
    processor: &Processor,
    files: Vec<String>,
) -> Finalizer {
    let natbib = processor.name == "natbib";
    for id in oc::list_citations(ex) {
        let Some(c) = oc::citation(ex, id) else {
            continue;
        };
        let style = oc::citation_style(&c, processor);
        let out = if natbib {
            natbib_citation(ex, &c, &style)
        } else {
            biblatex_citation(ex, &c, &style)
        };
        oc::replace_citation(ex, id, Some(out));
    }
    for k in oc::bibliography_keywords(ex) {
        let out = if natbib {
            format!(
                "\\bibliographystyle{{{}}}\n\\bibliography{{{}}}",
                processor
                    .bibliography_style
                    .as_deref()
                    .unwrap_or(NATBIB_STYLE),
                files
                    .iter()
                    .map(|f| sans_extension(f))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            let value = ex
                .tree
                .syntax(k)
                .and_then(|s| ast::Keyword::cast(s.clone()))
                .map(|kw| kw.value())
                .unwrap_or_default();
            format!(
                "\\printbibliography{}",
                biblatex_options(&parse_plist(&value))
            )
        };
        oc::replace_bibliography(ex, k, &out);
    }
    if natbib {
        Finalizer::Natbib
    } else {
        Finalizer::Biblatex {
            style: processor.bibliography_style.clone(),
            files,
        }
    }
}

/// `file-name-sans-extension`.
fn sans_extension(f: &str) -> &str {
    let name_start = f.rfind('/').map_or(0, |i| i + 1);
    match f[name_start..].rfind('.') {
        Some(i) if i > 0 => &f[..name_start + i],
        _ => f,
    }
}

/// The exported objects of a prefix or suffix, trimmed.
fn affix(ex: &mut Exporter<'_>, n: &SyntaxNode) -> String {
    let ids = ex.tree.objects_of(n);
    ex.data_list(&ids).trim().to_string()
}

/// `[prefix][suffix]`, `[prefix][]` or `[][suffix]`… as natbib and
/// biblatex take them.
fn optional_arguments(
    ex: &mut Exporter<'_>,
    prefix: Option<SyntaxNode>,
    suffix: Option<SyntaxNode>,
    open: char,
    close: char,
) -> String {
    let mut out = String::new();
    if let Some(p) = &prefix {
        out.push(open);
        out.push_str(&affix(ex, p));
        out.push(close);
    }
    match (&suffix, &prefix) {
        (Some(s), _) => {
            out.push(open);
            out.push_str(&affix(ex, s));
            out.push(close);
        }
        (None, Some(_)) => {
            out.push(open);
            out.push(close);
        }
        _ => {}
    }
    out
}

/// `org-cite-natbib-export-citation`.
fn natbib_citation(
    ex: &mut Exporter<'_>,
    c: &ast::Citation,
    (style, variant): &(Option<String>, Option<String>),
) -> String {
    let v = variant.as_deref();
    let command = match style.as_deref() {
        Some("author" | "a") => match v {
            Some("caps" | "c") => "\\Citeauthor",
            Some("full" | "f") => "\\citeauthor*",
            _ => "\\citeauthor",
        },
        Some("noauthor" | "na") => match v {
            Some("bare" | "b") => "\\citeyear",
            _ => "\\citeyearpar",
        },
        Some("nocite" | "n") => "\\nocite",
        Some("text" | "t") => match v {
            Some("bare" | "b") => "\\citealt",
            Some("caps" | "c") => "\\Citet",
            Some("full" | "f") => "\\citet*",
            Some("bare-caps" | "bc") => "\\Citealt",
            Some("bare-full" | "bf") => "\\citealt*",
            Some("caps-full" | "cf") => "\\Citet*",
            Some("bare-caps-full" | "bcf") => "\\Citealt*",
            _ => "\\citet",
        },
        _ => match v {
            Some("bare" | "b") => "\\citealp",
            Some("caps" | "c") => "\\Citep",
            Some("full" | "f") => "\\citep*",
            Some("bare-caps" | "bc") => "\\Citealp",
            Some("bare-full" | "bf") => "\\citealp*",
            Some("caps-full" | "cf") => "\\Citep*",
            Some("bare-caps-full" | "bcf") => "\\Citealp*",
            _ => "\\citep",
        },
    };
    // `org-cite-main-affixes`.
    let refs: Vec<ast::CitationReference> = c.references().collect();
    let (prefix, suffix) = match refs.as_slice() {
        [r] => (r.prefix(), r.suffix()),
        _ => (c.prefix(), c.suffix()),
    };
    let opt = optional_arguments(ex, prefix, suffix, '[', ']');
    format!("{command}{opt}{{{}}}", c.keys().join(","))
}

fn expand(s: Option<&str>) -> Option<String> {
    s.map(|s| {
        SHORTCUTS
            .iter()
            .find(|(k, _)| *k == s)
            .map_or(s, |(_, v)| v)
            .to_string()
    })
}

/// `org-cite-biblatex-export-citation`.
fn biblatex_citation(
    ex: &mut Exporter<'_>,
    c: &ast::Citation,
    (style, variant): &(Option<String>, Option<String>),
) -> String {
    let name = expand(style.as_deref());
    let variant = expand(variant.as_deref());
    let (name, variant) = (name.as_deref(), variant.as_deref());
    // The first exact match, else the last candidate pushed.
    let mut candidates: Vec<&BiblatexStyle> = Vec::new();
    let mut style_match = false;
    for s in BIBLATEX_STYLES {
        if s.0 == name && s.1 == variant {
            candidates = vec![s];
            break;
        } else if s.0.is_none() && s.1.is_none() {
            if candidates.is_empty() {
                candidates.insert(0, s);
            }
        } else if s.0 == name && s.1.is_none() {
            candidates.insert(0, s);
            style_match = true;
        } else if s.0.is_none() && s.1 == variant && !style_match {
            candidates.insert(0, s);
        }
    }
    let &(_, _, command, multi, no_opt) = candidates[0];
    let refs: Vec<ast::CitationReference> = c.references().collect();
    let multicite = refs.len() > 1
        && refs
            .iter()
            .any(|r| r.prefix().is_some() || r.suffix().is_some());
    match multi {
        Some(m) if multicite => {
            let mut out = format!("\\{m}");
            out.push_str(&optional_arguments(ex, c.prefix(), c.suffix(), '(', ')'));
            for r in &refs {
                out.push_str(&optional_arguments(ex, r.prefix(), r.suffix(), '[', ']'));
                out.push_str(&format!("{{{}}}", r.key()));
            }
            out
        }
        _ => {
            let mut out = format!("\\{command}");
            if !no_opt {
                let (prefix, suffix) = match refs.as_slice() {
                    [r] => (r.prefix(), r.suffix()),
                    _ => (c.prefix(), c.suffix()),
                };
                out.push_str(&optional_arguments(ex, prefix, suffix, '[', ']'));
            }
            out.push_str(&format!("{{{}}}", c.keys().join(",")));
            out
        }
    }
}

/// `org-cite--parse-as-plist`: `:key value :key "a value"`.
fn parse_plist(s: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut rest = s.trim_start_matches(' ');
    let mut value_flag = false;
    while !rest.is_empty() {
        if let Some(k) = rest.strip_prefix(':') {
            let end = k.find([' ', '\t', '\n']).unwrap_or(k.len());
            out.push((k[..end].to_string(), None));
            rest = &k[end..];
            value_flag = true;
        } else if !value_flag {
            let end = rest.find(' ').unwrap_or(rest.len());
            rest = &rest[end..];
        } else if let Some(q) = rest.strip_prefix('"') {
            match q.find('"') {
                Some(e) => {
                    if let Some(last) = out.last_mut() {
                        last.1 = Some(q[..e].to_string());
                    }
                    rest = &q[e + 1..];
                }
                None => {
                    let end = rest.find(' ').unwrap_or(rest.len());
                    if let Some(last) = out.last_mut() {
                        last.1 = Some(rest[..end].to_string());
                    }
                    rest = &rest[end..];
                }
            }
            value_flag = false;
        } else {
            let end = rest.find(' ').unwrap_or(rest.len());
            if let Some(last) = out.last_mut() {
                last.1 = Some(rest[..end].to_string());
            }
            rest = &rest[end..];
            value_flag = false;
        }
        rest = rest.trim_start_matches(' ');
    }
    out
}

/// `org-cite-biblatex-export-bibliography`'s options: `key=value` for
/// each value (comma-separated values repeat the key), a key without a
/// value as it is, except the last one.
fn biblatex_options(props: &[(String, Option<String>)]) -> String {
    if props.is_empty() {
        return String::new();
    }
    let mut results: Vec<String> = Vec::new();
    for (i, (k, v)) in props.iter().enumerate() {
        match v {
            Some(v) => results.push(
                v.split(',')
                    .filter(|x| !x.is_empty())
                    .map(|x| format!("{k}={x}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            // A key is pushed when the next one comes.
            None if i + 1 < props.len() => results.push(k.clone()),
            None => {}
        }
    }
    format!("[{}]", results.join(","))
}

/// `org-cite-natbib-use-package`: `\usepackage{natbib}` before the
/// document unless the preamble loads it.
pub(crate) fn natbib_use_package(out: String, at: usize) -> String {
    if find_package(&out[..at], "natbib").is_some() {
        return out;
    }
    format!("{}\\usepackage{{natbib}}\n{}", &out[..at], &out[at..])
}

/// The last `\usepackage[OPTIONS]{name}` in `pre`: where it starts and
/// the range of `[OPTIONS]`, if any.
fn find_package(pre: &str, name: &str) -> Option<(usize, Option<(usize, usize)>)> {
    let tail = format!("{{{name}}}");
    let mut from = pre.len();
    while let Some(i) = pre[..from].rfind("\\usepackage") {
        let after = i + "\\usepackage".len();
        let rest = &pre[after..];
        if rest.starts_with(&tail) {
            return Some((i, None));
        }
        if rest.starts_with('[')
            && let Some(j) = rest.find(&format!("]{tail}"))
        {
            return Some((i, Some((after, after + j + 1))));
        }
        from = i;
    }
    None
}

/// `org-cite-biblatex--package-options`.
fn package_options(initial: Option<&str>, style: Option<&str>) -> String {
    let mut options: Vec<String> = initial
        .map(|i| {
            let i = i.strip_prefix('[').unwrap_or(i);
            let i = i.strip_suffix(']').unwrap_or(i);
            i.split(',')
                .map(|o| o.trim_matches([' ', '\t']))
                .filter(|o| !o.is_empty())
                .filter(|o| {
                    !o.starts_with("bibstyle")
                        && !o.starts_with("citestyle")
                        && !o.starts_with("style")
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if let Some(style) = style {
        match style.split_once('/') {
            None => options.push(if biblatex_options_p(style) {
                style.to_string()
            } else {
                format!("style={style}")
            }),
            Some((b, c)) => {
                options.push(format!("bibstyle={b}"));
                options.push(format!("citestyle={c}"));
            }
        }
    }
    if options.is_empty() {
        String::new()
    } else {
        format!("[{}]", options.join(","))
    }
}

/// Whether `style` reads `key=value,key=value`
/// (`\`[^,=]+=[^,]+\(,[^=]+=[^,]+\)\'`).
fn biblatex_options_p(style: &str) -> bool {
    let Some((a, b)) = style.split_once(',') else {
        return false;
    };
    let pair = |s: &str, no_before: &[char], no_after: &[char]| {
        s.split_once('=').is_some_and(|(k, v)| {
            !k.is_empty() && !k.contains(no_before) && !v.is_empty() && !v.contains(no_after)
        })
    };
    pair(a, &[',', '='], &[',']) && pair(b, &['='], &[','])
}

/// `org-cite-biblatex-prepare-preamble`.
pub(crate) fn biblatex_prepare_preamble(
    out: String,
    at: usize,
    style: Option<&str>,
    files: &[String],
) -> String {
    let mut out = out;
    let forward_line = |s: &str, p: usize| s[p..].find('\n').map_or(s.len(), |i| p + i + 1);
    let point = match find_package(&out[..at], "biblatex") {
        None => {
            let line = format!("\\usepackage{}{{biblatex}}\n", package_options(None, style));
            out.insert_str(at, &line);
            forward_line(&out, at)
        }
        Some((start, None)) => {
            // After the first brace, as Org inserts them.
            let brace = start + out[start..].find('{').unwrap_or(0) + 1;
            out.insert_str(brace, &package_options(None, style));
            forward_line(&out, brace)
        }
        Some((_, Some((a, b)))) => {
            let new = package_options(Some(&out[a..b]), style);
            out.replace_range(a..b, &new);
            forward_line(&out, a + new.len())
        }
    };
    let resources: Vec<String> = files
        .iter()
        .map(|f| {
            let remote = f.starts_with("http://") || f.starts_with("https://");
            format!(
                "\\addbibresource{}{{{f}}}",
                if remote { "[location=remote]" } else { "" }
            )
        })
        .collect();
    out.insert_str(point, &format!("{}\n", resources.join("\n")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options() {
        assert_eq!(package_options(None, None), "");
        assert_eq!(
            package_options(None, Some("authoryear")),
            "[style=authoryear]"
        );
        assert_eq!(
            package_options(None, Some("authoryear/numeric")),
            "[bibstyle=authoryear,citestyle=numeric]"
        );
        assert_eq!(
            package_options(Some("[backend=biber, style=apa]"), Some("a=b,c=d")),
            "[backend=biber,a=b,c=d]"
        );
        assert_eq!(
            package_options(None, Some("backend=biber")),
            "[style=backend=biber]"
        );
        assert_eq!(
            parse_plist(":heading none :title \"My refs\" :keyword a,b"),
            [
                ("heading".to_string(), Some("none".to_string())),
                ("title".into(), Some("My refs".into())),
                ("keyword".into(), Some("a,b".into())),
            ]
        );
        assert_eq!(
            biblatex_options(&parse_plist(":heading none :keyword a,b :x")),
            "[heading=none,keyword=a,keyword=b]"
        );
        assert_eq!(sans_extension("bib/refs.bib"), "bib/refs");
        assert_eq!(sans_extension("a.b/refs"), "a.b/refs");
    }

    #[test]
    fn preambles() {
        let doc = "\\documentclass{article}\n\\begin{document}\nx\n\\end{document}\n";
        let at = doc.find("\\begin{document}").unwrap();
        assert_eq!(
            natbib_use_package(doc.to_string(), at),
            "\\documentclass{article}\n\\usepackage{natbib}\n\\begin{document}\nx\n\\end{document}\n"
        );
        assert_eq!(
            biblatex_prepare_preamble(doc.to_string(), at, Some("apa"), &["refs.bib".into()]),
            "\\documentclass{article}\n\\usepackage[style=apa]{biblatex}\n\\addbibresource{refs.bib}\n\\begin{document}\nx\n\\end{document}\n"
        );
        let doc =
            "\\documentclass{article}\n\\usepackage[backend=biber]{biblatex}\n\\begin{document}\n";
        let at = doc.find("\\begin{document}").unwrap();
        assert_eq!(
            biblatex_prepare_preamble(
                doc.to_string(),
                at,
                Some("apa"),
                &["a.bib".into(), "https://x/b.bib".into()]
            ),
            "\\documentclass{article}\n\\usepackage[backend=biber,style=apa]{biblatex}\n\\addbibresource{a.bib}\n\\addbibresource[location=remote]{https://x/b.bib}\n\\begin{document}\n"
        );
    }
}
