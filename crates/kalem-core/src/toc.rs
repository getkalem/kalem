//! The live table of contents: a `#+TOC: headlines N` line shown as the
//! list of headings an export would print there, numbered as the export
//! numbers them, each one leading to its heading.

use std::ops::Range;

use org_model::Document;

use crate::document::DocumentState;
use org_syntax::SyntaxKind;
use org_syntax::ast::{self, AstNode};

/// A heading in a table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// Its depth in the table, from 1.
    pub depth: usize,
    /// Its section number (`2.1`), for numbered headings.
    pub number: Option<String>,
    /// Its title, as written.
    pub title: String,
    /// Where the heading starts.
    pub start: usize,
}

/// What `#+TOC:` asks for: how deep, and only below its own heading.
fn request(value: &str) -> Option<(Option<usize>, bool)> {
    let mut words = value.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("headlines") {
        return None;
    }
    let mut depth = None;
    let mut local = false;
    for w in words {
        if let Ok(n) = w.parse::<usize>() {
            depth = Some(n);
        } else if w.eq_ignore_ascii_case("local") {
            local = true;
        }
    }
    Some((depth, local))
}

/// The table of contents a `#+TOC: headlines` keyword on the source line
/// `line` stands for, or `None` when the line is not one.
pub fn toc_at(doc: &Document, line: Range<usize>) -> Option<Vec<TocEntry>> {
    let root = doc.parse().syntax();
    let kw = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::KEYWORD)
        .find(|n| usize::from(n.text_range().start()) == line.start)
        .and_then(ast::Keyword::cast)?;
    if !kw.key().eq_ignore_ascii_case("TOC") {
        return None;
    }
    let (depth, local) = request(&kw.value())?;
    let outline = doc.outline();
    // The heading the keyword is under, for `local`.
    let under = local
        .then(|| {
            outline
                .entries
                .iter()
                .rev()
                .filter(|e| !e.inlinetask && usize::from(e.range.start()) < line.start)
                .find(|e| line.start < usize::from(e.range.end()))
        })
        .flatten();
    let base = under.map_or(0, |e| e.level);
    let range = under.map(|e| usize::from(e.range.start())..usize::from(e.range.end()));
    // Numbers as the export gives them: every exported heading counts.
    let mut counters: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    let mut skip_below: Option<usize> = None;
    let mut unnumbered_below: Option<usize> = None;
    for e in outline.entries.iter().filter(|e| !e.inlinetask) {
        if let Some(l) = skip_below {
            if e.level > l {
                continue;
            }
            skip_below = None;
        }
        let noexport = e.local_tags.iter().any(|t| t == "noexport") || e.commented;
        if noexport {
            skip_below = Some(e.level);
            continue;
        }
        // `:UNNUMBERED:` headings, and those below, have no number.
        let unnumbered = e
            .drawer
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("UNNUMBERED") && v != "nil")
            || unnumbered_below.is_some_and(|l| e.level > l);
        if unnumbered_below.is_some_and(|l| e.level <= l) {
            unnumbered_below = None;
        }
        if unnumbered && unnumbered_below.is_none() {
            unnumbered_below = Some(e.level);
        }
        if !unnumbered {
            if counters.len() < e.level {
                counters.resize(e.level, 0);
            }
            counters.truncate(e.level);
            counters[e.level - 1] += 1;
        }
        let start = usize::from(e.range.start());
        if let Some(r) = &range
            && !(r.start < start && start < r.end)
        {
            continue;
        }
        let rel = e.level - base;
        if rel == 0 || depth.is_some_and(|d| rel > d) {
            continue;
        }
        let number = (!unnumbered).then(|| {
            counters
                .iter()
                .map(|c| c.max(&1).to_string())
                .collect::<Vec<_>>()
                .join(".")
        });
        out.push(TocEntry {
            depth: rel,
            number,
            title: e.raw_title.clone(),
            start,
        });
    }
    Some(out)
}

/// The table of contents to show for the source line `line` of `state`,
/// as lines of text with the start of their heading: when the line is a
/// `#+TOC: headlines` keyword and the cursor is elsewhere.
pub fn shown(state: &mut DocumentState, line: Range<usize>) -> Option<Vec<(String, usize)>> {
    if !wanted(state.text().as_str(), state.selection.head, &line) {
        return None;
    }
    let doc = state.model()?;
    toc_at(&doc, line).map(|e| lines(&e))
}

/// Whether the source line `line` of `text` may show a table of contents:
/// it starts with `#+TOC:` and the cursor is elsewhere.
pub fn wanted(text: &str, cursor: usize, line: &Range<usize>) -> bool {
    text.get(line.clone())
        .and_then(|s| s.get(..6))
        .is_some_and(|s| s.eq_ignore_ascii_case("#+toc:"))
        && !(line.start <= cursor && cursor <= line.end)
}

/// A table of contents as lines of text, indented by depth.
pub fn lines(entries: &[TocEntry]) -> Vec<(String, usize)> {
    entries
        .iter()
        .map(|e| {
            let indent = "   ".repeat(e.depth.saturating_sub(1));
            let t = match &e.number {
                Some(n) => format!("{indent}{n} {}", e.title),
                None => format!("{indent}{}", e.title),
            };
            (t, e.start)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(t: &str) -> Document {
        Document::new(org_syntax::parse(t))
    }

    #[test]
    fn tables_of_contents() {
        let t = "#+TOC: headlines 2\n* One\n** One A\n*** Deep\n* COMMENT Hidden\n** Under hidden\n* Two :noexport:\n* Extra\n:PROPERTIES:\n:UNNUMBERED: t\n:END:\n** Extra A\n* Three\n#+TOC: headlines 1 local\n** Three A\n** Three B\n";
        let d = doc(t);
        let first = toc_at(&d, 0..18).unwrap();
        assert_eq!(
            lines(&first)
                .iter()
                .map(|l| l.0.as_str())
                .collect::<Vec<_>>(),
            [
                "1 One",
                "   1.1 One A",
                "Extra",
                "   Extra A",
                "2 Three",
                "   2.1 Three A",
                "   2.2 Three B"
            ]
        );
        assert_eq!(first[0].start, 19);
        let at = t.find("#+TOC: headlines 1 local").unwrap();
        let local = toc_at(&d, at..at + 24).unwrap();
        assert_eq!(
            lines(&local)
                .iter()
                .map(|l| l.0.as_str())
                .collect::<Vec<_>>(),
            ["2.1 Three A", "2.2 Three B"]
        );
        assert_eq!(toc_at(&d, 19..24), None);
        assert!(toc_at(&doc("#+TOC: tables\n"), 0..13).is_none());
    }
}
