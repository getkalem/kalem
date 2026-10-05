//! Formatting LaTeX documents (T2.7h.21, `kalem fmt FILE.tex`): the
//! lines of each environment indented by its depth with the document's
//! own indentation step (a document that does not indent its
//! environments is left as it is), runs of blank lines made one, blanks
//! at line ends removed, one line ending at the end; verbatim text is not
//! touched. With `align`, the `&` of tables and alignments line up.

use std::collections::HashMap;
use std::ops::Range;

use latex_syntax::{SyntaxKind as K, SyntaxNode};

/// Verbatim text: environments' bodies and verbatim arguments, and the
/// bodies of `alltt` and of the listings `\lstnewenvironment` declares,
/// which the parser reads as LaTeX but LaTeX typesets line by line.
fn protected(root: &SyntaxNode) -> Vec<Range<usize>> {
    let mut keep: Vec<Range<usize>> = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == K::VERBATIM)
        .map(|t| usize::from(t.text_range().start())..usize::from(t.text_range().end()))
        .collect();
    let text = root.text().to_string();
    let mut lines_kept = vec!["alltt".to_string()];
    let mut rest = text.as_str();
    while let Some(i) = rest.find("\\lstnewenvironment") {
        rest = &rest[i + "\\lstnewenvironment".len()..];
        let r = rest.trim_start();
        if let Some(inner) = r.strip_prefix('{')
            && let Some(close) = inner.find('}')
        {
            lines_kept.push(inner[..close].trim().to_string());
        }
    }
    for env in root.descendants().filter(|n| n.kind() == K::ENVIRONMENT) {
        let name = latex_syntax::name(&env).unwrap_or_default();
        if lines_kept.contains(&name)
            && let Some(body) = env.children().find(|c| c.kind() == K::BODY)
        {
            keep.push(usize::from(body.text_range().start())..usize::from(body.text_range().end()));
        }
    }
    keep
}

/// How many environments (not `document`) hold the line starting its
/// text at `p`, the `\begin` and `\end` lines counting outside theirs.
fn depth(root: &SyntaxNode, p: usize) -> usize {
    let Some(t) = latex_syntax::token_at(root, p) else {
        return 0;
    };
    t.parent_ancestors()
        .filter(|a| a.kind() == K::BODY)
        .filter(|b| {
            b.parent()
                .and_then(|e| latex_syntax::name(&e))
                .is_some_and(|n| n != "document")
        })
        .count()
}

fn lead(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The document's indentation step: what a line in an environment adds to
/// its `\begin` line, the most common one; `None` when environments are
/// not indented.
fn step(text: &str, root: &SyntaxNode) -> Option<String> {
    let mut votes: HashMap<String, usize> = HashMap::new();
    for env in root.descendants().filter(|n| n.kind() == K::ENVIRONMENT) {
        let name = latex_syntax::name(&env).unwrap_or_default();
        if name == "document" || latex_syntax::signatures::is_verbatim(&name) {
            continue;
        }
        let begin = usize::from(env.text_range().start());
        let bl = text[..begin].rfind('\n').map_or(0, |i| i + 1);
        let begin_lead = lead(&text[bl..]);
        // The first line after `\begin`'s.
        let Some(nl) = text[begin..].find('\n') else {
            continue;
        };
        let next = begin + nl + 1;
        let line_end = text[next..].find('\n').map_or(text.len(), |i| next + i);
        let line = &text[next..line_end];
        // A row whose first cell is empty starts with the padding that
        // lines its `&` up (`--align`), not with the step.
        let t = line.trim_start();
        if t.is_empty() || t.starts_with("\\end") || t.starts_with('&') {
            continue;
        }
        let l = lead(line);
        if let Some(extra) = l.strip_prefix(begin_lead) {
            *votes.entry(extra.to_string()).or_insert(0) += 1;
        }
    }
    let (best, n) = votes.iter().max_by_key(|(k, n)| (**n, k.len()))?;
    let flat = votes.get("").copied().unwrap_or(0);
    (!best.is_empty() && *n >= flat).then(|| best.clone())
}

/// The document formatted; `align` lines up the `&` of tables.
pub fn format(text: &str, align: bool) -> String {
    let parse = latex_syntax::parse(text);
    let root = parse.syntax();
    let keep = protected(&root);
    let unit = step(text, &root);
    let inside = |p: usize| keep.iter().any(|r| r.start < p && p < r.end);
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    let mut pos = 0;
    for raw in text.split_inclusive('\n') {
        let start = pos;
        pos += raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let ending = &raw[line.len()..];
        // Verbatim: as it is (its first line starts before the text).
        if inside(start) {
            out.push_str(raw);
            blank = 0;
            continue;
        }
        let body = line.trim_end_matches([' ', '\t']);
        if body.trim().is_empty() {
            blank += 1;
            if blank == 1 {
                out.push_str(if ending.is_empty() { "" } else { ending });
            }
            continue;
        }
        blank = 0;
        let content = body.trim_start_matches([' ', '\t']);
        match &unit {
            Some(u) => {
                let p = start + (body.len() - content.len());
                out.push_str(&u.repeat(depth(&root, p)));
                out.push_str(content);
            }
            None => out.push_str(body),
        }
        out.push_str(ending);
    }
    // One line ending at the end, blank lines before it dropped.
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    while out.ends_with(&format!("{nl}{nl}")) {
        out.truncate(out.len() - nl.len());
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(nl);
    }
    if align { align_ampersands(&out) } else { out }
}

/// Environments whose rows are split by `&`.
fn aligned(name: &str) -> bool {
    matches!(
        name.trim_end_matches('*'),
        "tabular"
            | "tabularx"
            | "longtable"
            | "array"
            | "align"
            | "alignat"
            | "flalign"
            | "eqnarray"
            | "matrix"
            | "pmatrix"
            | "bmatrix"
            | "Bmatrix"
            | "vmatrix"
            | "Vmatrix"
            | "cases"
    )
}

/// Where a row's cells split: its top-level `&` (not `\&`, not in
/// braces).
fn cells(row: &str) -> Vec<&str> {
    let b = row.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'&' if depth == 0 => {
                out.push(&row[start..i]);
                start = i + 1;
            }
            b'%' => break,
            _ => {}
        }
        i += 1;
    }
    out.push(&row[start..]);
    out
}

/// The `&` of each table's and alignment's rows lined up, a row a line
/// (rows spanning lines and `\multicolumn` rows are left).
fn align_ampersands(text: &str) -> String {
    use unicode_width::UnicodeWidthStr;
    let parse = latex_syntax::parse(text);
    let root = parse.syntax();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for env in root.descendants().filter(|n| n.kind() == K::ENVIRONMENT) {
        if !latex_syntax::name(&env).is_some_and(|n| aligned(&n)) {
            continue;
        }
        let Some(body) = env.children().find(|c| c.kind() == K::BODY) else {
            continue;
        };
        let bs = usize::from(body.text_range().start());
        let be = usize::from(body.text_range().end());
        // The body's lines that are rows of this environment.
        let mut rows: Vec<(Range<usize>, String, Vec<String>, String)> = Vec::new();
        let mut at = text[bs..be].find('\n').map_or(be, |i| bs + i + 1);
        while at < be {
            let end = text[at..be].find('\n').map_or(be, |i| at + i);
            let line = &text[at..end];
            let content = line.trim_start();
            let indent = line[..line.len() - content.len()].to_string();
            let nested = latex_syntax::token_at(&root, at + indent.len())
                .and_then(|t| t.parent_ancestors().find(|a| a.kind() == K::ENVIRONMENT))
                .is_some_and(|e| e != env);
            if content.contains('&') && !content.contains("\\multicolumn") && !nested {
                let (row, tail) = match content.find("\\\\") {
                    Some(i) => (&content[..i], &content[i..]),
                    None => (content, ""),
                };
                let cs: Vec<String> = cells(row).iter().map(|c| c.trim().to_string()).collect();
                if cs.len() > 1 {
                    rows.push((at..end, indent, cs, tail.trim_end().to_string()));
                }
            }
            at = end + 1;
        }
        if rows.len() < 2 {
            continue;
        }
        // A row whose first cell is empty starts with the padding that
        // lined its `&` up last time: its indentation is the other rows'.
        if let Some(common) = rows
            .iter()
            .find(|r| !r.2[0].is_empty())
            .map(|r| r.1.clone())
        {
            for r in rows.iter_mut().filter(|r| r.2[0].is_empty()) {
                r.1.clone_from(&common);
            }
        }
        let columns = rows.iter().map(|r| r.2.len()).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|c| {
                rows.iter()
                    .filter_map(|r| r.2.get(c))
                    .map(|s| s.width())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        for (range, indent, cs, tail) in rows {
            let n = cs.len();
            let mut line = indent;
            for (i, c) in cs.iter().enumerate() {
                line.push_str(c);
                if i + 1 < n {
                    line.push_str(&" ".repeat(widths[i] - c.width()));
                    // No first cell in any row: the `&` starts the line,
                    // with no space that the next run would read as
                    // indentation.
                    line.push_str(if i == 0 && widths[0] == 0 {
                        "& "
                    } else {
                        " & "
                    });
                }
            }
            if !tail.is_empty() {
                line.push(' ');
                line.push_str(&tail);
            }
            let line = line.trim_end().to_string();
            if text[range.clone()] != line {
                edits.push((range, line));
            }
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (r, s) in edits {
        if r.start < at {
            continue;
        }
        out.push_str(&text[at..r.start]);
        out.push_str(&s);
        at = r.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_bodies_kept() {
        let text = "\\lstnewenvironment{code}{}{}\n\\begin{alltt}\na\n\n\n  b\n\\end{alltt}\n\\begin{code}\nx\n\n\n    y\n\\end{code}\n";
        assert_eq!(format(text, false), text);
    }

    #[test]
    fn indentation_blank_lines_verbatim() {
        let text = "\\begin{document}\n\\begin{itemize}\n  \\item a\n\\begin{enumerate}\n\\item b   \n\\end{enumerate}\n      \\item c\n\\end{itemize}\n\n\n\nText\n\\begin{verbatim}\n   keep   \n\\end{verbatim}\n\\end{document}\n\n";
        let out = format(text, false);
        assert_eq!(
            out,
            "\\begin{document}\n\\begin{itemize}\n  \\item a\n  \\begin{enumerate}\n    \\item b\n  \\end{enumerate}\n  \\item c\n\\end{itemize}\n\nText\n\\begin{verbatim}\n   keep   \n\\end{verbatim}\n\\end{document}\n"
        );
        assert_eq!(format(&out, false), out);
        // Not indented: left as it is (apart from blanks).
        let flat = "\\begin{itemize}\n\\item a\n\\end{itemize}\n";
        assert_eq!(format(flat, false), flat);
    }

    #[test]
    fn a_row_with_an_empty_first_cell_is_not_an_indentation() {
        // The padding before its first `&` is alignment, not the step:
        // read as the step, it grew the indentation at every run
        // (publish_todo 3.3).
        let text = "\\begin{document}\n\\begin{tabular}{ll}\n & b \\\\\nlonger & c \\\\\n\\end{tabular}\n\\begin{itemize}\n  \\item x\n\\end{itemize}\n\\end{document}\n";
        let once = format(text, true);
        assert_eq!(format(&once, true), once, "{once}");
        assert!(once.contains("\n  longer & c"), "{once}");
        assert!(once.contains("\n         & b"), "{once}");
        // A document that does not indent keeps its lines' indentation,
        // but not the padding of such a row.
        let flat = "\\begin{tabular}{ll}\n & b \\\\\nlonger & c \\\\\n\\end{tabular}\n";
        let once = format(flat, true);
        assert_eq!(format(&once, true), once, "{once}");
        assert!(once.contains("\n       & b"), "{once}");
        // Rows that all start with `&`: no space added before it.
        let all = "\\begin{align}\n  x =\n  & a \\\\\n  & bb\n\\end{align}\n";
        let once = format(all, true);
        assert_eq!(format(&once, true), once, "{once}");
        assert!(once.contains("\n  & a \\\\\n  & bb\n"), "{once}");
    }

    #[test]
    fn ampersands() {
        let text = "\\begin{tabular}{ll}\n  a & bb \\\\\n  ccc & d \\\\ \\hline\n  \\multicolumn{2}{c}{x} \\\\\n\\end{tabular}\n";
        let out = format(text, true);
        assert_eq!(
            out,
            "\\begin{tabular}{ll}\n  a   & bb \\\\\n  ccc & d \\\\ \\hline\n  \\multicolumn{2}{c}{x} \\\\\n\\end{tabular}\n"
        );
        assert_eq!(format(&out, true), out);
        assert_eq!(cells("a & {b & c} & \\& d"), ["a ", " {b & c} ", " \\& d"]);
    }

    use proptest::prelude::*;

    fn piece() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("\\begin{itemize}\n".to_string()),
            Just("\\end{itemize}\n".to_string()),
            Just("\\begin{tabular}{ll}\n".to_string()),
            Just("\\end{tabular}\n".to_string()),
            Just("\\begin{verbatim}\n".to_string()),
            Just("\\end{verbatim}\n".to_string()),
            Just("\\item x\n".to_string()),
            Just("a & b \\\\\n".to_string()),
            Just("  ".to_string()),
            Just("\t".to_string()),
            Just("\n".to_string()),
            Just("\n\n\n".to_string()),
            Just("% c\n".to_string()),
            Just("{".to_string()),
            Just("}".to_string()),
            "[a-z ]{1,6}",
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn idempotent_and_only_blanks_change(v in prop::collection::vec(piece(), 0..40), align in any::<bool>()) {
            let text = v.concat();
            let once = format(&text, align);
            prop_assert_eq!(&format(&once, align), &once);
            let solid = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            prop_assert_eq!(solid(&once), solid(&text));
        }
    }
}
