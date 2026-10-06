//! Diagnostics: problems worth telling the user about.
//!
//! Org has no syntax errors: every text is a valid document. Some text,
//! however, is almost certainly not what its author meant, such as a block
//! that is never closed and so is read as a paragraph. These checks follow
//! the spirit of Emacs's `org-lint`.

use std::collections::HashMap;

use rowan::{TextRange, TextSize};

use crate::SyntaxKind::*;
use crate::ast::{self, AstNode};
use crate::{Parse, SyntaxNode};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Probably intended but worth a look.
    Info,
    /// Probably a mistake.
    Warning,
}

/// A problem in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where the problem is.
    pub range: TextRange,
    /// How serious it is.
    pub severity: Severity,
    /// A stable identifier, such as `unterminated-block`.
    pub code: &'static str,
    /// A message for people.
    pub message: String,
}

impl Parse {
    /// Checks the document and returns its diagnostics, in document order.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        check(&self.syntax())
    }
}

fn diag(
    range: TextRange,
    severity: Severity,
    code: &'static str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        range,
        severity,
        code,
        message: message.into(),
    }
}

/// The range of the first line of `node`, without its line break.
fn first_line(node: &SyntaxNode) -> (TextRange, String) {
    let text = node.text().to_string();
    let start = usize::from(ast::post_affiliated(node) - node.text_range().start());
    let line = text[start..]
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    let begin = ast::post_affiliated(node);
    (
        TextRange::at(begin, TextSize::from(line.len() as u32)),
        line.to_string(),
    )
}

fn check(root: &SyntaxNode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut custom_ids: HashMap<String, Vec<TextRange>> = HashMap::new();
    let mut names: HashMap<String, Vec<TextRange>> = HashMap::new();
    // Footnotes, as org-lint checks them: definitions by label, the
    // labels an inline footnote defines, and references to a label.
    let mut footnotes: HashMap<String, Vec<TextRange>> = HashMap::new();
    let mut inline_labels: Vec<String> = Vec::new();
    let mut references: Vec<(String, TextRange)> = Vec::new();
    for n in root.descendants() {
        match n.kind() {
            PARAGRAPH => paragraph(&n, &mut out),
            FOOTNOTE_DEFINITION => {
                if let Some(f) = ast::FootnoteDefinition::cast(n.clone()) {
                    footnotes
                        .entry(f.label())
                        .or_default()
                        .push(first_line(&n).0);
                }
            }
            FOOTNOTE_REFERENCE => {
                if let Some(f) = ast::FootnoteReference::cast(n.clone())
                    && let Some(label) = f.label()
                {
                    if f.is_inline() {
                        inline_labels.push(label);
                    } else {
                        // Without the blanks after it.
                        let len = n.text().to_string().trim_end().len();
                        let range =
                            TextRange::at(n.text_range().start(), TextSize::from(len as u32));
                        references.push((label, range));
                    }
                }
            }
            KEYWORD => {
                if let Some(k) = ast::Keyword::cast(n.clone()) {
                    let key = k.key();
                    if is_affiliated_key(&key) {
                        out.push(diag(
                            n.text_range(),
                            Severity::Warning,
                            "orphaned-affiliated-keyword",
                            format!("#+{key} is not attached to anything; the next line is blank or cannot take it"),
                        ));
                    }
                }
            }
            BABEL_CALL => {
                let (range, line) = first_line(&n);
                let unbalanced =
                    |o: char, c: char| line.matches(o).count() != line.matches(c).count();
                if unbalanced('(', ')') || unbalanced('[', ']') {
                    out.push(diag(
                        range,
                        Severity::Warning,
                        "unbalanced-call",
                        "brackets in this #+CALL: line do not balance",
                    ));
                }
            }
            TIMESTAMP => timestamp(&n, &mut out),
            NODE_PROPERTY => {
                if let Some(p) = ast::NodeProperty::cast(n.clone())
                    && p.key().eq_ignore_ascii_case("CUSTOM_ID")
                {
                    custom_ids
                        .entry(p.value())
                        .or_default()
                        .push(n.text_range());
                }
            }
            _ => {}
        }
        if n.kind().is_element()
            && let Some(name) = ast::element_name(&n)
        {
            names.entry(name).or_default().push(first_line(&n).0);
        }
    }
    for (label, range) in references {
        if !footnotes.contains_key(&label) && !inline_labels.contains(&label) {
            out.push(diag(
                range,
                Severity::Warning,
                "undefined-footnote-reference",
                format!("footnote {label} has no definition; an export stops here"),
            ));
        }
    }
    for (label, ranges) in footnotes {
        if ranges.len() > 1 {
            for r in ranges {
                out.push(diag(
                    r,
                    Severity::Warning,
                    "duplicate-footnote-definition",
                    format!("footnote {label} is defined more than once; the first is used"),
                ));
            }
        }
    }
    for (what, code, map) in [
        ("CUSTOM_ID", "duplicate-custom-id", custom_ids),
        ("#+NAME", "duplicate-name", names),
    ] {
        for (value, ranges) in map {
            if ranges.len() > 1 && !value.is_empty() {
                for r in ranges {
                    out.push(diag(
                        r,
                        Severity::Warning,
                        code,
                        format!("{what} \"{value}\" is used more than once"),
                    ));
                }
            }
        }
    }
    out.sort_by_key(|d| (d.range.start(), d.code));
    out
}

fn is_affiliated_key(key: &str) -> bool {
    matches!(
        key,
        "CAPTION"
            | "DATA"
            | "HEADER"
            | "HEADERS"
            | "LABEL"
            | "NAME"
            | "PLOT"
            | "RESNAME"
            | "RESULT"
            | "RESULTS"
            | "SOURCE"
            | "SRCNAME"
            | "TBLNAME"
    ) || key.starts_with("ATTR_")
}

fn paragraph(n: &SyntaxNode, out: &mut Vec<Diagnostic>) {
    let text = n.text().to_string();
    let base = n.text_range().start();
    let mut offset = 0usize;
    for (i, raw_line) in text.split_inclusive('\n').enumerate() {
        let line = raw_line.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - trimmed.len();
        let range = TextRange::at(
            base + TextSize::from((offset + indent) as u32),
            TextSize::from(trimmed.len() as u32),
        );
        let upper = trimmed.to_ascii_uppercase();
        if i == 0 && upper.starts_with("#+BEGIN_") {
            let name: String = trimmed[8..]
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect();
            // Keep the author's case: `#+begin_src` pairs with `#+end_src`.
            let (begin, end) = if trimmed[2..7].chars().all(|c| c.is_ascii_lowercase()) {
                ("#+begin_", "#+end_")
            } else {
                ("#+BEGIN_", "#+END_")
            };
            out.push(diag(
                range,
                Severity::Warning,
                "unterminated-block",
                format!("{begin}{name} has no matching {end}{name}; it is read as text"),
            ));
        } else if i == 0 && upper.starts_with("\\BEGIN{") {
            out.push(diag(
                range,
                Severity::Warning,
                "unterminated-latex-environment",
                "LaTeX environment has no matching \\end; it is read as text",
            ));
        } else if i == 0 && is_drawer_line(trimmed) && upper != ":END:" {
            out.push(diag(
                range,
                Severity::Warning,
                "unterminated-drawer",
                format!("drawer {trimmed} has no :END: line; it is read as text"),
            ));
        } else if upper.trim_end() == ":END:" {
            out.push(diag(
                range,
                Severity::Warning,
                "stray-end",
                ":END: without an open drawer",
            ));
        } else if upper.starts_with("#+END_") || upper.trim_end() == "#+END:" {
            out.push(diag(
                range,
                Severity::Warning,
                "stray-end",
                format!("{} without a matching begin line", trimmed.trim_end()),
            ));
        }
        offset += raw_line.len();
    }
}

fn is_drawer_line(t: &str) -> bool {
    let t = t.trim_end_matches([' ', '\t']);
    t.len() > 2
        && t.starts_with(':')
        && t.ends_with(':')
        && t[1..t.len() - 1]
            .chars()
            .all(|c| c == '-' || c == '_' || crate::tables::is_word(c))
}

fn timestamp(n: &SyntaxNode, out: &mut Vec<Diagnostic>) {
    let Some(ts) = ast::Timestamp::cast(n.clone()) else {
        return;
    };
    let dates = [ts.start(), ts.end()];
    for d in dates.into_iter().flatten() {
        let bad_date =
            !(1..=12).contains(&d.month) || d.day == 0 || d.day > days_in_month(d.year, d.month);
        let bad_time = d.time.is_some_and(|(h, m)| h > 24 || m > 59);
        if bad_date || bad_time {
            let raw = ts.raw_value();
            let range = TextRange::at(n.text_range().start(), TextSize::from(raw.len() as u32));
            out.push(diag(
                range,
                Severity::Warning,
                "invalid-timestamp",
                format!("{raw} is not a valid date or time"),
            ));
            return;
        }
    }
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    fn codes(text: &str) -> Vec<&'static str> {
        crate::parse(text)
            .diagnostics()
            .into_iter()
            .map(|d| d.code)
            .collect()
    }

    #[test]
    fn finds_common_mistakes() {
        assert_eq!(codes("#+begin_src sh\necho\n"), vec!["unterminated-block"]);
        assert_eq!(codes(":LOGBOOK:\nx\n"), vec!["unterminated-drawer"]);
        assert_eq!(codes("text\n#+end_quote\n"), vec!["stray-end"]);
        assert_eq!(
            codes("#+NAME: x\n\npara\n"),
            vec!["orphaned-affiliated-keyword"]
        );
        assert_eq!(codes("<2026-02-30 Mon>\n"), vec!["invalid-timestamp"]);
        assert_eq!(codes("#+CALL: f(x\n"), vec!["unbalanced-call"]);
        assert_eq!(
            codes(
                "* a\n:PROPERTIES:\n:CUSTOM_ID: x\n:END:\n* b\n:PROPERTIES:\n:CUSTOM_ID: x\n:END:\n"
            ),
            vec!["duplicate-custom-id", "duplicate-custom-id"]
        );
    }

    #[test]
    fn footnotes_as_org_lint_checks_them() {
        assert_eq!(codes("See [fn:9].\n"), vec!["undefined-footnote-reference"]);
        assert_eq!(
            codes("See [fn:1].\n\n[fn:1] One.\n\n[fn:1] Two.\n"),
            vec![
                "duplicate-footnote-definition",
                "duplicate-footnote-definition"
            ]
        );
        // Defined by a definition, or by an inline footnote; anonymous.
        assert!(codes("See [fn:1].\n\n[fn:1] One.\n").is_empty());
        assert!(codes("See [fn:a:Note] and [fn:a].\n").is_empty());
        assert!(codes("See [fn::Note].\n").is_empty());
        let d = crate::parse("See [fn:9] here.\n").diagnostics();
        assert_eq!(u32::from(d[0].range.start()), 4);
        assert_eq!(u32::from(d[0].range.len()), 6);
    }

    #[test]
    fn clean_documents_have_none() {
        assert!(codes("* TODO a\nSCHEDULED: <2026-02-28 Sat>\n#+begin_src sh\necho\n#+end_src\n#+NAME: t\n| a |\n").is_empty());
    }
}
