//! Org files and what earlier versions of Kalem added to them. A `.org`
//! file is strict Org (D24), and Kalem writes nothing into it that Org does
//! not define (principle 8, T2.13.13). Earlier versions wrote their own
//! formatting into Org files (`@@kalem:…@@` snippets, `#+ATTR_KALEM:` and
//! `#+KALEM:` lines); such a file keeps its bytes and shows them as Emacs
//! does, and [`markup`] finds them for `kalem check` and [`strip_markup`]
//! takes them out.

use std::path::Path;

use crate::document::DocumentState;
use crate::mode::DocumentMode;

/// The file kind of an Org document: `org`, a document not saved yet
/// included; `None` for other modes.
pub fn file_kind(doc: &DocumentState) -> Option<&'static str> {
    (doc.meta.mode == DocumentMode::Org).then_some("org")
}

/// What Kalem adds to Org, found in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markup {
    /// A formatting snippet: `@@kalem:…@@`.
    Span,
    /// A paragraph attribute line: `#+ATTR_KALEM:`.
    Paragraph,
    /// The document's options: `#+KALEM:`.
    Document,
}

/// Kalem's additions in the document `root`, in order: each one's range
/// (whole lines for `#+ATTR_KALEM:` and `#+KALEM:`, with their line
/// feeds) and kind.
pub fn markup(root: &org_syntax::SyntaxNode) -> Vec<(std::ops::Range<usize>, Markup)> {
    use org_syntax::SyntaxKind::*;
    use org_syntax::ast::{self, AstNode};
    let range = |n: &org_syntax::SyntaxNode| {
        let r = n.text_range();
        usize::from(r.start())..usize::from(r.end())
    };
    let mut out = Vec::new();
    for n in root.descendants() {
        match n.kind() {
            EXPORT_SNIPPET => {
                if let Some(s) = ast::ExportSnippet::cast(n.clone())
                    && s.backend().eq_ignore_ascii_case("kalem")
                {
                    let r = range(&n);
                    let end = r.end - ast::post_blank(&n);
                    out.push((r.start..end, Markup::Span));
                }
            }
            KEYWORD => {
                if let Some(k) = ast::Keyword::cast(n.clone())
                    && k.key().eq_ignore_ascii_case("KALEM")
                {
                    out.push((range(&n), Markup::Document));
                }
            }
            _ => {
                for k in ast::affiliated_keywords(&n) {
                    if k.key().eq_ignore_ascii_case("ATTR_KALEM") {
                        out.push((range(k.syntax()), Markup::Paragraph));
                    }
                }
            }
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out.dedup();
    out
}

/// `text` as strict Org: Kalem's additions taken out (a snippet goes, a
/// line of its own goes with its line feed), and how many of each kind
/// went (spans, paragraph attributes, document options). Org's own center
/// blocks stay. A line that ended a paragraph right above it becomes a
/// blank line, so that the paragraph stays apart from what follows.
pub fn strip_markup(text: &str) -> (String, [usize; 3]) {
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let found = markup(&root);
    // Where paragraphs end without blank lines.
    let ends: std::collections::HashSet<usize> = root
        .descendants()
        .filter(|n| n.kind() == org_syntax::SyntaxKind::PARAGRAPH)
        .filter(|n| org_syntax::ast::post_blank(n) == 0)
        .map(|n| usize::from(n.text_range().end()))
        .collect();
    let mut counts = [0usize; 3];
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (r, kind) in found {
        if r.start < at {
            continue;
        }
        counts[kind as usize] += 1;
        out.push_str(&text[at..r.start]);
        if kind != Markup::Span && ends.contains(&r.start) && r.end < text.len() {
            out.push('\n');
        }
        at = r.end;
    }
    out.push_str(&text[at..]);
    (out, counts)
}

/// What [`strip_markup`] took out, for a message: `3 formatted spans,
/// 1 paragraph attribute`.
pub fn dropped_summary(counts: [usize; 3]) -> String {
    let parts: Vec<String> = [
        ("kind-dropped-spans", counts[0]),
        ("kind-dropped-paragraphs", counts[1]),
        ("kind-dropped-document", counts[2]),
    ]
    .into_iter()
    .filter(|(_, n)| *n > 0)
    .map(|(id, n)| crate::tr!(id, count = n))
    .collect();
    if parts.is_empty() {
        crate::l10n::tr("kind-dropped-nothing")
    } else {
        parts.join(", ")
    }
}

/// `to` relative to the folder `from`, with `/` between names: `b.org`,
/// `sub/b.org`, `../b.org`.
pub fn relative(from: &Path, to: &Path) -> Option<String> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripping_kalem_markup() {
        let text = "#+TITLE: T\n#+KALEM: font=\"Georgia\" size=12\nSome @@kalem:color=red@@red@@kalem:end@@ text.\n\n#+ATTR_KALEM: :align right\n#+CAPTION: kept\nRight.\n\n#+begin_center\nMiddle\n#+end_center\n";
        let (out, counts) = strip_markup(text);
        assert_eq!(
            out,
            "#+TITLE: T\nSome red text.\n\n#+CAPTION: kept\nRight.\n\n#+begin_center\nMiddle\n#+end_center\n"
        );
        assert_eq!(counts, [2, 1, 1]);
        crate::l10n::set_language("en");
        assert_eq!(
            dropped_summary(counts),
            "2 formatted spans, 1 paragraph attribute, 1 document option line"
        );
        assert_eq!(strip_markup("* A\n").0, "* A\n");
        // Paragraphs a line kept apart stay apart.
        assert_eq!(
            strip_markup("a\n#+KALEM: size=2\nb\n#+ATTR_KALEM: :align right\nc\n#+KALEM: size=3\n")
                .0,
            "a\n\nb\n\nc\n"
        );
    }

    #[test]
    fn kinds_and_relative_paths() {
        assert_eq!(
            relative(Path::new("/p/a"), Path::new("/p/a/b.org")).as_deref(),
            Some("b.org")
        );
        assert_eq!(
            relative(Path::new("/p/a/c"), Path::new("/p/a/b.org")).as_deref(),
            Some("../b.org")
        );
        assert_eq!(
            relative(Path::new("/p"), Path::new("/p/a/b.org")).as_deref(),
            Some("a/b.org")
        );
    }
}
