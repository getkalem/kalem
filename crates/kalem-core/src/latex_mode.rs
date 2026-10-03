//! LaTeX on the document mode contract (§11.11, T2.7c.10): the tree of
//! `latex-syntax` mapped to the contract's kinds, the outline, the
//! formatter, the diagnostics and the keys Enter and Tab, from the code
//! the LaTeX mode already has.

use std::ops::Range;

use latex_syntax::{SyntaxKind as K, SyntaxNode};

use crate::modes::{Detect, EditKey, Kind, ModeDiagnostic, ModeSpec, TextEdit, Tree};

/// LaTeX on the contract.
#[derive(Debug, Default)]
pub struct LatexMode;

impl ModeSpec for LatexMode {
    fn id(&self) -> &'static str {
        "latex"
    }

    fn detect(&self) -> Detect {
        Detect {
            extensions: &["tex", "ltx", "sty", "cls", "dtx"],
            sniff: None,
        }
    }

    fn parse(&self, text: &str, _edit: Option<&TextEdit>, _previous: Option<&Tree>) -> Tree {
        to_tree(text)
    }

    fn edit(&self, text: &str, at: usize, key: EditKey) -> Option<Vec<(Range<usize>, String)>> {
        let parse = latex_syntax::parse(text);
        let root = parse.syntax();
        let tx = match key {
            EditKey::Enter => crate::latex_edit::enter(text, org_edit::Selection::caret(at), &root),
            EditKey::Tab => crate::latex_edit::indent_item(text, at, &root, true),
            EditKey::BackTab => crate::latex_edit::indent_item(text, at, &root, false),
        }?;
        Some(tx.edits.into_iter().map(|e| (e.range, e.insert)).collect())
    }

    fn outline(&self, text: &str, tree: &Tree) -> Vec<crate::view::OutlineItem> {
        // Each heading by its title, the argument of its command.
        let mut items = tree.headings(text);
        for it in &mut items {
            let t = latex_syntax::parse(&it.title);
            if let Some(cmd) = t.syntax().descendants().find(|n| n.kind() == K::COMMAND)
                && let Some(g) = first_group(&cmd)
            {
                it.title = it.title[g].trim().to_string();
            }
        }
        items
    }

    fn format(&self, text: &str) -> Option<String> {
        Some(crate::latex_fmt::format(text, true))
    }

    fn diagnostics(&self, text: &str) -> Vec<ModeDiagnostic> {
        crate::latex_check::check(std::path::Path::new("document.tex"), text)
            .into_iter()
            .map(|d| ModeDiagnostic {
                range: d.range,
                code: d.code.into(),
                message: d.message,
            })
            .collect()
    }
}

/// The level of a sectioning command, as LaTeX numbers them.
fn section_level(name: &str) -> Option<i8> {
    Some(match name.trim_end_matches('*') {
        "part" => -1,
        "chapter" => 0,
        "section" => 1,
        "subsection" => 2,
        "subsubsection" => 3,
        "paragraph" => 4,
        "subparagraph" => 5,
        _ => return None,
    })
}

fn span(n: &SyntaxNode) -> Range<usize> {
    let r = n.text_range();
    usize::from(r.start())..usize::from(r.end())
}

/// The inside of a command's first `{…}` argument.
fn first_group(n: &SyntaxNode) -> Option<Range<usize>> {
    let g = n.children().find(|c| c.kind() == K::GROUP)?;
    let r = span(&g);
    (r.len() >= 2).then(|| r.start + 1..r.end - usize::from(g.text().to_string().ends_with('}')))
}

/// The tree of a LaTeX document in the contract's kinds. Headings take
/// levels from the document's highest sectioning command (a book's
/// chapters, an article's sections are level 1).
pub fn to_tree(text: &str) -> Tree {
    let parse = latex_syntax::parse(text);
    let root = parse.syntax();
    let top = root
        .descendants()
        .filter(|n| n.kind() == K::COMMAND)
        .filter_map(|n| latex_syntax::name(&n).and_then(|s| section_level(&s)))
        .min()
        .unwrap_or(1);
    let mut tree = Tree::default();
    walk(&root, None, top, &mut tree);
    tree
}

/// The contract's kind of node `n`, when it has one; `None` for the
/// syntax that only holds others (groups, arguments, `\begin`, bodies).
fn kind_of(n: &SyntaxNode, top: i8) -> Option<Kind> {
    Some(match n.kind() {
        K::PARAGRAPH => Kind::Paragraph,
        K::INLINE_MATH => Kind::Math,
        K::DISPLAY_MATH => Kind::MathBlock,
        K::VERB => Kind::InlineCode,
        K::ENVIRONMENT => {
            let name = latex_syntax::name(n).unwrap_or_default();
            match name.trim_end_matches('*') {
                "itemize" | "description" => Kind::List { ordered: false },
                "enumerate" => Kind::List { ordered: true },
                "quote" | "quotation" | "verse" => Kind::Quote,
                "verbatim" | "Verbatim" | "lstlisting" | "minted" | "alltt" => Kind::Code {
                    language: code_language(n, &name),
                },
                "tabular" | "tabularx" | "tabulary" | "longtable" | "tabu" => Kind::Table,
                "equation" | "align" | "gather" | "multline" | "flalign" | "alignat"
                | "eqnarray" | "displaymath" | "math" => Kind::MathBlock,
                _ => Kind::Other(name),
            }
        }
        K::COMMAND => {
            let name = latex_syntax::name(n)?;
            if let Some(level) = section_level(&name) {
                return Some(Kind::Heading((level - top + 1).clamp(1, 6) as u8));
            }
            match name.as_str() {
                "emph" | "textit" | "textsl" => Kind::Emphasis,
                "textbf" => Kind::Strong,
                "texttt" => Kind::InlineCode,
                "href" | "url" => Kind::Link {
                    target: first_group(n),
                },
                "includegraphics" => Kind::Image {
                    target: first_group(n),
                },
                "footnote" => Kind::FootnoteRef,
                // `\item` is its list item's marker (see `walk`).
                "item" => return None,
                _ => Kind::Other(name),
            }
        }
        _ => return None,
    })
}

/// The language of a code environment: minted's argument, or the
/// `language=` option of `lstlisting`.
fn code_language(env: &SyntaxNode, name: &str) -> Option<String> {
    let begin = env.children().find(|c| c.kind() == K::BEGIN)?;
    match name {
        "minted" => {
            let g = begin.children().filter(|c| c.kind() == K::GROUP).nth(1)?;
            let t = g.text().to_string();
            Some(t.trim_matches(['{', '}']).trim().to_string()).filter(|s| !s.is_empty())
        }
        "lstlisting" => {
            let o = begin
                .children()
                .find(|c| c.kind() == K::OPT_ARG)?
                .text()
                .to_string();
            o.trim_matches(['[', ']'])
                .split(',')
                .find_map(|kv| kv.trim().strip_prefix("language="))
                .map(|l| l.trim().trim_matches(['{', '}']).to_string())
        }
        _ => None,
    }
}

/// Pushes the nodes under `n` with `parent`.
fn walk(n: &SyntaxNode, parent: Option<u32>, top: i8, tree: &mut Tree) {
    for c in n.children() {
        let kind = kind_of(&c, top);
        let id = kind
            .clone()
            .map(|k| tree.push(k, span(&c), parent))
            .or(parent);
        match kind {
            Some(Kind::List { .. }) => walk_body(&c, id, top, tree, Body::List),
            Some(Kind::Table) => walk_body(&c, id, top, tree, Body::Table),
            // Verbatim: nothing inside.
            Some(Kind::Code { .. }) | Some(Kind::InlineCode) => {}
            _ => walk(&c, id, top, tree),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Body {
    List,
    Table,
}

/// The body of a list (items from each `\item` to the next) or a table
/// (rows ended by `\\`, cells split by `&`), the nodes inside each under
/// it. The body's paragraphs are looked through: one can span items.
fn walk_body(env: &SyntaxNode, parent: Option<u32>, top: i8, tree: &mut Tree, body: Body) {
    let Some(b) = env.children().find(|c| c.kind() == K::BODY) else {
        walk(env, parent, top, tree);
        return;
    };
    let whole = span(&b);
    let mut elements = Vec::new();
    flatten(&b, &mut elements);
    let is_item =
        |n: &SyntaxNode| n.kind() == K::COMMAND && latex_syntax::name(n).as_deref() == Some("item");
    let is_row_end =
        |n: &SyntaxNode| n.kind() == K::COMMAND && latex_syntax::name(n).as_deref() == Some("\\");
    // The parts, each with its kind and whether it sits in a row.
    let mut parts: Vec<(Kind, Range<usize>, bool)> = Vec::new();
    match body {
        Body::List => {
            let starts: Vec<usize> = elements
                .iter()
                .filter_map(|e| e.as_node().filter(|n| is_item(n)).map(|n| span(n).start))
                .collect();
            for (i, &s) in starts.iter().enumerate() {
                let e = starts.get(i + 1).copied().unwrap_or(whole.end);
                parts.push((Kind::ListItem { checkbox: None }, s..e, false));
            }
        }
        Body::Table => {
            let mut row_start = whole.start;
            let mut cell_start = whole.start;
            let mut cells: Vec<Range<usize>> = Vec::new();
            for e in &elements {
                let r = usize::from(e.text_range().start())..usize::from(e.text_range().end());
                let amp = e.as_token().is_some_and(|t| t.kind() == K::AMPERSAND);
                let end = e.as_node().is_some_and(&is_row_end);
                if amp {
                    cells.push(cell_start..r.start);
                    cell_start = r.end;
                } else if end {
                    cells.push(cell_start..r.start);
                    parts.push((Kind::TableRow, row_start..r.end, false));
                    parts.extend(cells.drain(..).map(|c| (Kind::TableCell, c, true)));
                    row_start = r.end;
                    cell_start = r.end;
                }
            }
            // A last row without `\\`, when it has more than blanks.
            let rest = &b.text().to_string()[row_start - whole.start..];
            if !rest.trim().is_empty() {
                cells.push(cell_start..whole.end);
                parts.push((Kind::TableRow, row_start..whole.end, false));
                parts.extend(cells.drain(..).map(|c| (Kind::TableCell, c, true)));
            }
        }
    }
    // The nodes to place: not the markers and separators.
    let children: Vec<SyntaxNode> = elements
        .iter()
        .filter_map(|e| e.as_node().cloned())
        .filter(|n| !is_item(n) && !is_row_end(n))
        .collect();
    let mut next = 0;
    let first = parts.first().map_or(whole.end, |p| p.1.start);
    take_until(&children, &mut next, first, parent, parent, top, tree);
    let mut row: Option<u32> = parent;
    for (kind, range, nested) in parts {
        let under = if nested { row } else { parent };
        let id = tree.push(kind.clone(), range.clone(), under);
        if kind == Kind::TableRow {
            row = Some(id);
            // A row's children are in its cells.
            continue;
        }
        take_until(&children, &mut next, range.end, Some(id), parent, top, tree);
    }
    take_until(&children, &mut next, usize::MAX, parent, parent, top, tree);
}

/// The elements of `n`, its paragraphs looked through.
fn flatten(n: &SyntaxNode, out: &mut Vec<latex_syntax::SyntaxElement>) {
    for e in n.children_with_tokens() {
        match e.as_node() {
            Some(p) if p.kind() == K::PARAGRAPH => flatten(p, out),
            _ => out.push(e),
        }
    }
}

/// Pushes `children` from `*next` on that start before `end`, under
/// `under` (or `outer`, for one running past `end`).
fn take_until(
    children: &[SyntaxNode],
    next: &mut usize,
    end: usize,
    under: Option<u32>,
    outer: Option<u32>,
    top: i8,
    tree: &mut Tree,
) {
    while let Some(c) = children.get(*next) {
        if span(c).start >= end {
            break;
        }
        *next += 1;
        let p = if span(c).end <= end { under } else { outer };
        let kind = kind_of(c, top);
        let id = kind.clone().map(|k| tree.push(k, span(c), p)).or(p);
        match kind {
            Some(Kind::List { .. }) => walk_body(c, id, top, tree, Body::List),
            Some(Kind::Table) => walk_body(c, id, top, tree, Body::Table),
            Some(Kind::Code { .. }) | Some(Kind::InlineCode) => {}
            _ => walk(c, id, top, tree),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\\documentclass{article}\n\\begin{document}\n\\section{Intro}\nSome \\emph{text} and \\textbf{bold}, $x^2$ and \\verb|v|.\n\n\\subsection{Lists}\n\\begin{itemize}\n\\item One \\url{https://a.b}\n\\item Two\n  \\begin{enumerate}\n  \\item Inner\n  \\end{enumerate}\n\\end{itemize}\n\n\\begin{tabular}{ll}\na & b \\\\\nc & \\textbf{d} \\\\\n\\end{tabular}\n\n\\begin{equation}\nE = mc^2\n\\end{equation}\n\n\\begin{lstlisting}[language=Rust]\nfn main() {}\n\\end{lstlisting}\n\n\\includegraphics{fig.png}\\footnote{Note.}\n\\end{document}\n";

    #[test]
    fn latex_on_the_contract() {
        let spec = LatexMode;
        let edits: Vec<(Range<usize>, &str)> = (0..DOC.len())
            .step_by(7)
            .flat_map(|i| {
                [
                    (i..i, "x"),
                    (i..i, "\n\n"),
                    (i..(i + 3).min(DOC.len()), ""),
                    (i..i, "\\item "),
                ]
            })
            .collect();
        crate::modes::check(&spec, DOC, &edits).unwrap();
        let tree = spec.parse(DOC, None, None);
        let kinds: Vec<&Kind> = tree.nodes.iter().map(|n| &n.kind).collect();
        assert!(kinds.contains(&&Kind::Heading(1)) && kinds.contains(&&Kind::Heading(2)));
        assert!(kinds.contains(&&Kind::List { ordered: false }));
        assert!(kinds.contains(&&Kind::List { ordered: true }));
        assert_eq!(
            kinds
                .iter()
                .filter(|k| ***k == Kind::ListItem { checkbox: None })
                .count(),
            3
        );
        assert_eq!(kinds.iter().filter(|k| ***k == Kind::TableRow).count(), 2);
        assert_eq!(kinds.iter().filter(|k| ***k == Kind::TableCell).count(), 4);
        assert!(kinds.contains(&&Kind::Code {
            language: Some("Rust".into())
        }));
        assert!(kinds.contains(&&Kind::MathBlock) && kinds.contains(&&Kind::Math));
        assert!(kinds.contains(&&Kind::Emphasis) && kinds.contains(&&Kind::Strong));
        assert!(kinds.contains(&&Kind::InlineCode) && kinds.contains(&&Kind::FootnoteRef));
        // A link's and a picture's targets.
        let target = |k: &Kind| match k {
            Kind::Link { target } | Kind::Image { target } => target.clone().map(|r| &DOC[r]),
            _ => None,
        };
        let targets: Vec<&str> = kinds.iter().filter_map(|k| target(k)).collect();
        assert_eq!(targets, ["https://a.b", "fig.png"]);
        // The bold cell's command is under its cell, the cell under its row.
        let bold = tree
            .nodes
            .iter()
            .rposition(|n| n.kind == Kind::Strong)
            .unwrap();
        let cell = tree.nodes[bold].parent.unwrap() as usize;
        assert_eq!(tree.nodes[cell].kind, Kind::TableCell);
        let row = tree.nodes[cell].parent.unwrap() as usize;
        assert_eq!(tree.nodes[row].kind, Kind::TableRow);
        // The inner list is inside the second item.
        let inner = kinds
            .iter()
            .position(|k| **k == Kind::List { ordered: true })
            .unwrap();
        let item = tree.nodes[inner].parent.unwrap() as usize;
        assert_eq!(tree.nodes[item].kind, Kind::ListItem { checkbox: None });
        assert!(DOC[tree.nodes[item].range.clone()].starts_with("\\item Two"));
        // The outline: the headings.
        let outline = spec.outline(DOC, &tree);
        let titles: Vec<&str> = outline.iter().map(|o| o.title.as_str()).collect();
        assert_eq!(titles.len(), 2, "{titles:?}");
        // Enter in an item starts the next.
        let at = DOC.find("Two").unwrap() + 3;
        let edits = spec.edit(DOC, at, EditKey::Enter).expect("an item");
        assert!(edits.iter().any(|(_, s)| s.contains("\\item")), "{edits:?}");
        assert!(spec.format(DOC).is_some());
        assert!(crate::modes::Modes::with_builtins().get("latex").is_some());
    }

    #[test]
    fn chapters_lead_in_a_book() {
        let t = "\\chapter{One}\n\\section{A}\n";
        let tree = to_tree(t);
        let levels: Vec<u8> = tree
            .nodes
            .iter()
            .filter_map(|n| match n.kind {
                Kind::Heading(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(levels, [1, 2]);
    }
}
