//! Diagnostics of LaTeX documents (T2.7h.20) for `kalem check FILE.tex`
//! and the editor: what the parser closed or skipped, labels referred to
//! and not defined or defined twice (across the files of the project),
//! citation keys no bibliography has, files and pictures not found,
//! commands LaTeX 2ε deprecates, a few of chktex's rules; and the report
//! of what a document leaves as source (T2.7h.13).

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use latex_model::project::{Disk, ProjectCache, find_root};
use latex_syntax::SyntaxKind as K;

/// How much a diagnostic matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Probably wrong.
    Warning,
    /// Style.
    Info,
}

/// A diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where, in bytes.
    pub range: Range<usize>,
    /// How much it matters.
    pub severity: Severity,
    /// A stable name for it.
    pub code: &'static str,
    /// What, for the user.
    pub message: String,
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// The diagnostics of the LaTeX file `path` with text `text`, in order.
pub fn check(path: &Path, text: &str) -> Vec<Diagnostic> {
    let parse = latex_syntax::parse(text);
    let mut out: Vec<Diagnostic> = parse
        .diagnostics()
        .iter()
        .map(|d| Diagnostic {
            range: d.range.clone(),
            severity: Severity::Warning,
            code: "latex-syntax",
            message: d.message.clone(),
        })
        .collect();
    // The project it belongs to, for labels and citations in other files.
    let root = find_root(path, text, &Disk, None, None);
    let project = ProjectCache::default().load(&root, &Disk);
    let (model, this) = match project.model.files.iter().position(|f| same_file(f, path)) {
        Some(i) if !same_file(&root, path) || i == 0 => (project.model.clone(), i),
        _ => (std::sync::Arc::new(latex_model::Model::new(&parse)), 0),
    };
    let root_dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for l in &model.labels {
        *counts.entry(l.name.as_str()).or_insert(0) += 1;
    }
    for l in model.labels.iter().filter(|l| l.file == this) {
        if counts.get(l.name.as_str()).copied().unwrap_or(0) > 1 {
            out.push(Diagnostic {
                range: l.range.clone(),
                severity: Severity::Warning,
                code: "latex-duplicate-label",
                message: crate::tr!("latex-duplicate-label", key = l.name.as_str()),
            });
        }
    }
    for r in model.references.iter().filter(|r| r.file == this) {
        for k in &r.keys {
            if !counts.contains_key(k.as_str()) {
                out.push(Diagnostic {
                    range: r.range.clone(),
                    severity: Severity::Warning,
                    code: "latex-undefined-reference",
                    message: crate::tr!("latex-unknown-label", key = k.as_str()),
                });
            }
        }
    }
    // Citations, when the document names its bibliography.
    let files: Vec<PathBuf> = model
        .bibliography
        .iter()
        .flat_map(|b| b.files.iter())
        .map(|f| root_dir.join(f))
        .collect();
    if !files.is_empty() {
        let (bib, errors) = org_cite::Bibliography::load(&files);
        for (f, e) in &errors {
            if let Some(b) = model.bibliography.iter().find(|b| b.file == this) {
                out.push(Diagnostic {
                    range: b.range.clone(),
                    severity: Severity::Warning,
                    code: "bibliography-unreadable",
                    message: crate::tr!(
                        "cite-bibliography-unreadable",
                        file = f.display().to_string(),
                        error = e.clone()
                    ),
                });
            }
        }
        if errors.len() < files.len() {
            for c in model.citations.iter().filter(|c| c.file == this) {
                for k in &c.keys {
                    if bib.get(k).is_none() {
                        out.push(Diagnostic {
                            range: c.range.clone(),
                            severity: Severity::Warning,
                            code: "cite-unknown-key",
                            message: crate::tr!("cite-unknown-key", key = k.as_str()),
                        });
                    }
                }
            }
        }
    }
    for i in model
        .includes
        .iter()
        .filter(|i| i.file == this && i.resolved.is_none())
    {
        out.push(Diagnostic {
            range: i.range.clone(),
            severity: Severity::Warning,
            code: "latex-missing-file",
            message: crate::tr!("latex-missing-file", file = i.target.as_str()),
        });
    }
    let root_node = parse.syntax();
    let base = path.parent();
    for n in root_node.descendants().filter(|n| n.kind() == K::COMMAND) {
        let range = usize::from(n.text_range().start())..usize::from(n.text_range().end());
        if latex_syntax::name(&n).as_deref() == Some("includegraphics")
            && let Some(g) = n.children().find(|c| c.kind() == K::GROUP)
        {
            let name = g.text().to_string();
            let name = name
                .trim_start_matches('{')
                .trim_end_matches('}')
                .trim()
                .to_string();
            let found = crate::latex_view::find_picture(base, &model.graphics_paths, &name)
                .or_else(|| {
                    crate::latex_view::find_picture(Some(&root_dir), &model.graphics_paths, &name)
                });
            if found.is_none() {
                out.push(Diagnostic {
                    range,
                    severity: Severity::Warning,
                    code: "latex-missing-picture",
                    message: crate::tr!("latex-missing-picture", file = name.as_str()),
                });
            }
        }
    }
    // Deprecated commands and chktex's rules, in text.
    for t in root_node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        let range = usize::from(t.text_range().start())..usize::from(t.text_range().end());
        let in_math = t.parent_ancestors().any(|a| {
            matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH)
                || (a.kind() == K::ENVIRONMENT
                    && latex_syntax::name(&a).is_some_and(|n| {
                        latex_syntax::signatures::is_math(&n)
                            || latex_syntax::signatures::is_verbatim(&n)
                    }))
        });
        let info = |code: &'static str, key: &str| Diagnostic {
            range: range.clone(),
            severity: Severity::Info,
            code,
            message: crate::l10n::tr(key),
        };
        match t.kind() {
            K::CONTROL_WORD => {
                let name = &t.text()[1..];
                if matches!(name, "bf" | "it" | "rm" | "sc" | "sf" | "tt" | "sl" | "cal") {
                    out.push(Diagnostic {
                        range: range.clone(),
                        severity: Severity::Info,
                        code: "latex-deprecated",
                        message: crate::tr!("latex-deprecated-font", command = name),
                    });
                }
                if matches!(
                    name,
                    "ref" | "eqref" | "cite" | "cref" | "autoref" | "pageref"
                ) && !in_math
                    && t.prev_token().is_some_and(|p| p.kind() == K::WHITESPACE)
                    && t.prev_token()
                        .and_then(|p| p.prev_token())
                        .is_some_and(|w| w.kind() == K::TEXT)
                {
                    out.push(info("latex-tie", "latex-tie"));
                }
            }
            K::DOUBLE_DOLLAR
                if t.parent()
                    .and_then(|p| p.children_with_tokens().next())
                    .is_some_and(|f| f.as_token() == Some(&t)) =>
            {
                out.push(info("latex-deprecated", "latex-double-dollar"));
            }
            K::TEXT if !in_math => {
                let s = t.text();
                if s.contains("...") {
                    out.push(info("latex-ellipsis", "latex-ellipsis"));
                }
                if s.contains('"') {
                    out.push(info("latex-quotes", "latex-quotes"));
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|d| (d.range.start, d.range.end));
    out
}

/// What the body of a LaTeX document leaves as source (T2.7h.13): the
/// commands and environments the view does not render, with how often,
/// most frequent first (`\foo`, `\begin{bar}`). Formulas are the math
/// renderer's and do not count.
pub fn unrendered(text: &str) -> Vec<(String, usize)> {
    let parse = latex_syntax::parse(text);
    let model = latex_model::Model::new(&parse);
    let body = model.body.clone().unwrap_or(0..text.len());
    let mut counts: HashMap<String, usize> = HashMap::new();
    for n in parse.syntax().descendants() {
        let start = usize::from(n.text_range().start());
        if !body.contains(&start) {
            continue;
        }
        let in_math = n.ancestors().skip(1).any(|a| {
            matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH)
                || (a.kind() == K::ENVIRONMENT
                    && latex_syntax::name(&a)
                        .is_some_and(|x| latex_syntax::signatures::is_math(&x)))
        });
        if in_math {
            continue;
        }
        match n.kind() {
            K::COMMAND => {
                if let Some(name) = latex_syntax::name(&n)
                    && !crate::latex_view::renders_command(&name)
                    && name.chars().all(|c| c.is_ascii_alphabetic() || c == '@')
                {
                    *counts.entry(format!("\\{name}")).or_insert(0) += 1;
                }
            }
            K::ENVIRONMENT => {
                if let Some(name) = latex_syntax::name(&n)
                    && !crate::latex_view::renders_environment(&name, &model)
                {
                    *counts.entry(format!("\\begin{{{name}}}")).or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics() {
        crate::l10n::set_language("en");
        let dir = std::env::temp_dir().join(format!("kalem-latex-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("refs.bib"),
            "@book{knuth, title = {T}, year = 1984}\n",
        )
        .unwrap();
        let text = "\\documentclass{article}\n\\begin{document}\n\\section{A}\\label{a}\\label{a}\nSee \\ref{b} and\\ref{a}, {\\bf x} $$y$$ \"q\" wait... \\cite{knuth,nope}\n\\input{missing}\n\\includegraphics{nofig}\n\\begin{itemize}\n\\bibliography{refs}\n\\end{document}\n";
        let path = dir.join("p.tex");
        std::fs::write(&path, text).unwrap();
        let codes: Vec<&str> = check(&path, text).iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            [
                "latex-duplicate-label",
                "latex-duplicate-label",
                "latex-tie",
                "latex-undefined-reference",
                "latex-deprecated",
                "latex-deprecated",
                "latex-quotes",
                "latex-ellipsis",
                "latex-tie",
                "cite-unknown-key",
                "latex-missing-file",
                "latex-missing-picture",
                "latex-syntax",
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unrendered_report() {
        let text = "\\documentclass{article}\\usepackage{tikz}\n\\begin{document}\n\\foo{x} \\foo \\emph{y} $\\alpha$\n\\begin{tikzpicture}\\draw;\\end{tikzpicture}\n\\end{document}\n";
        assert_eq!(
            unrendered(text),
            [
                ("\\foo".to_string(), 2),
                ("\\begin{tikzpicture}".to_string(), 1),
                ("\\draw".to_string(), 1)
            ]
        );
    }
}
