//! `kalem fmt` (§7.7): a document tidied as its own conventions have it,
//! without changing what it says.
//!
//! Tables are aligned (`org-table-align`) and headline tags too
//! (`org-align-tags` on all headlines). Where the document puts a blank
//! line before headlines ([`Style::infer`]), every headline gets one.
//! Lines of only blanks become empty, except in blocks that keep text as
//! it is (source, example, export, verse); the text ends with one line
//! feed. Formatting a formatted document changes nothing.

use org_model::Document;
use org_syntax::{SyntaxKind, SyntaxNode};

use crate::style::Style;

fn range(n: &SyntaxNode) -> std::ops::Range<usize> {
    usize::from(n.text_range().start())..usize::from(n.text_range().end())
}

use crate::transaction::Transaction;

/// Aligns each table of `root` on its own: the replacements.
fn align_tables(root: &SyntaxNode, text: &str) -> Transaction {
    let mut tx = Transaction::new("Format");
    for n in root.descendants().filter(|n| n.kind() == SyntaxKind::TABLE) {
        // The rows, after `#+NAME:` and the like and without blank lines
        // after the table.
        let r = usize::from(org_syntax::ast::post_affiliated(&n))..range(&n).end;
        if !text[r.clone()].trim_start().starts_with('|') {
            continue;
        }
        let body = text[r.clone()].trim_end_matches(['\n', ' ', '\t', '\r']);
        let end = r.start + body.len();
        let end = text[end..].find('\n').map_or(text.len(), |i| end + i + 1);
        let r = r.start..end.min(r.end);
        let table = &text[r.clone()];
        let first = table.find('|').unwrap_or(0);
        let doc = Document::new(org_syntax::parse(table));
        if let Ok(t) = crate::table::align_table(&doc, first)
            && !t.is_empty()
        {
            let aligned = t.apply(table);
            if aligned != table {
                // Tables never overlap.
                let _ = tx.replace(r, aligned);
            }
        }
    }
    tx
}

/// Blocks whose text is kept as it is.
fn verbatim(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::SRC_BLOCK
            | SyntaxKind::EXAMPLE_BLOCK
            | SyntaxKind::EXPORT_BLOCK
            | SyntaxKind::VERSE_BLOCK
            | SyntaxKind::COMMENT_BLOCK
            | SyntaxKind::FIXED_WIDTH
    )
}

/// Maps ascending positions through `tx`, whose edits touch none of them
/// (tables and headline lines do not contain block or headline starts).
fn map_sorted(tx: &Transaction, positions: &mut [usize]) {
    let mut delta: isize = 0;
    let mut e = 0;
    for p in positions.iter_mut() {
        while let Some(edit) = tx.edits.get(e)
            && edit.range.end <= *p
        {
            delta += edit.insert.len() as isize - edit.range.len() as isize;
            e += 1;
        }
        *p = (*p as isize + delta) as usize;
    }
}

/// Blank lines: empty outside the `keep` ranges, one before each headline
/// start in `headings` if `before_heading`, one line feed at the end.
fn blank_lines(
    text: &str,
    keep: &[std::ops::Range<usize>],
    headings: &[usize],
    before_heading: bool,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut at = 0;
    // `keep` is sorted by start and its ranges nest or follow each other.
    let mut k = 0;
    let mut open: Vec<usize> = Vec::new();
    for line in text.split_inclusive('\n') {
        while let Some(r) = keep.get(k)
            && r.start <= at
        {
            open.push(r.end);
            k += 1;
        }
        open.retain(|&end| at < end);
        let kept = !open.is_empty();
        if before_heading
            && headings.binary_search(&at).is_ok()
            && at > 0
            && !(out.ends_with("\n\n") || out.ends_with("\n\r\n"))
        {
            out.push_str(if out.ends_with("\r\n") { "\r\n" } else { "\n" });
        }
        let body = line.trim_end_matches(['\n', '\r']);
        if !kept && !body.is_empty() && body.trim_matches([' ', '\t']).is_empty() {
            out.push_str(&line[body.len()..]);
        } else {
            out.push_str(line);
        }
        at += line.len();
    }
    // One line feed at the end.
    let trimmed = out.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        return String::new();
    }
    let crlf = out[trimmed.len()..].starts_with("\r\n");
    let mut out = trimmed.to_string();
    out.push_str(if crlf { "\r\n" } else { "\n" });
    out
}

/// The formatted text of `doc`. A document whose lines all end with CRLF
/// is formatted as Emacs sees it, with line feeds, and converted back.
pub fn format(doc: &Document) -> String {
    let text = doc.parse().syntax().to_string();
    let crlf = text.matches("\r\n").count();
    if crlf > 0 && crlf == text.bytes().filter(|&b| b == b'\n').count() {
        let lf = text.replace("\r\n", "\n");
        let doc = Document::with_settings(
            org_syntax::parse_with(&lf, doc.parse().context()),
            std::sync::Arc::new(doc.settings().clone()),
            None,
        );
        return format_parsed(&doc, lf).replace('\n', "\r\n");
    }
    format_parsed(doc, text)
}

/// [`format`] with one parse (the document's own, of `text`): the
/// positions it gives are mapped through the alignments.
fn format_parsed(doc: &Document, text: String) -> String {
    let root = doc.parse().syntax();
    let style = Style::infer_in(doc.parse(), &text);
    let mut starts: Vec<usize> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let mut headings: Vec<usize> = Vec::new();
    for n in root.descendants() {
        if verbatim(n.kind()) {
            let r = range(&n);
            starts.push(r.start);
            ends.push(r.end);
        } else if n.kind() == SyntaxKind::HEADLINE {
            headings.push(usize::from(n.text_range().start()));
        }
    }
    let tables = align_tables(&root, &text);
    let text = if tables.edits.is_empty() {
        text
    } else {
        tables.apply(&text)
    };
    let tags = crate::tags::align_tags_by_line(&text);
    let text = if tags.is_empty() {
        text
    } else {
        tags.apply(&text)
    };
    // Preorder gives starts in order; ends are sorted for mapping and
    // put back by their order.
    let mut order: Vec<usize> = (0..ends.len()).collect();
    order.sort_by_key(|&i| ends[i]);
    let mut sorted_ends: Vec<usize> = order.iter().map(|&i| ends[i]).collect();
    headings.sort_unstable();
    for tx in [&tables, &tags] {
        map_sorted(tx, &mut starts);
        map_sorted(tx, &mut sorted_ends);
        map_sorted(tx, &mut headings);
    }
    for (j, &i) in order.iter().enumerate() {
        ends[i] = sorted_ends[j];
    }
    let keep: Vec<std::ops::Range<usize>> =
        starts.into_iter().zip(ends).map(|(s, e)| s..e).collect();
    blank_lines(&text, &keep, &headings, style.blank_before_heading)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(t: &str) -> String {
        format(&Document::new(org_syntax::parse(t)))
    }

    #[test]
    fn formatting() {
        let t = "* A :x:\n| a | bb |\n|--|\n| ccc |\n   \n#+begin_src sh\n  \nx\n#+end_src\n\n\n";
        let out = fmt(t);
        assert!(
            out.contains("| a   | bb |\n|-----+----|\n| ccc |    |\n"),
            "{out}"
        );
        assert!(
            out.lines().next().unwrap().ends_with(" :x:")
                && out.lines().next().unwrap().len() == 77
        );
        assert!(
            out.contains("|\n\n#+begin_src sh\n  \nx\n#+end_src\n"),
            "{out:?}"
        );
        assert!(out.ends_with("#+end_src\n"));
        assert_eq!(fmt(&out), out);
        // Headlines get the blank line the others have.
        let t = "* A\n\ntext\n\n* B\n\n* C\nx\n* D\n";
        assert_eq!(fmt(t), "* A\n\ntext\n\n* B\n\n* C\nx\n\n* D\n");
        assert_eq!(fmt(""), "");
        // Tables with affiliated keywords are aligned too.
        assert_eq!(
            fmt("#+NAME: t\n#+ATTR_KALEM: :x 1\n| a | bb |\n| ccc |\n"),
            "#+NAME: t\n#+ATTR_KALEM: :x 1\n| a   | bb |\n| ccc |    |\n"
        );
        // Lines ending with CRLF: formatted as with line feeds.
        let t = "* A\r\n\r\ntext\r\n\r\n* B :x:\r\n\r\n* C\r\n|a|\r\n* D\r\n";
        let lf = fmt(&t.replace("\r\n", "\n"));
        assert_eq!(fmt(t), lf.replace('\n', "\r\n"));
        assert!(lf.contains("| a |\n\n* D"), "{lf:?}");
        // Mixed line ends: the blank line before a headline is recognized.
        let t = "* A\r\n\r\n* B\r\n\r\n* C\n* D\r\n";
        assert_eq!(fmt(t), "* A\r\n\r\n* B\r\n\r\n* C\n\n* D\r\n");
    }

    #[test]
    fn idempotent_on_the_corpus() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/corpus/org-mode");
        let mut n = 0;
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "org") {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(&p) else {
                continue;
            };
            let once = fmt(&t);
            assert_eq!(fmt(&once), once, "{}", p.display());
            // The same headlines and tables.
            let count = |s: &str, k: SyntaxKind| {
                org_syntax::parse(s)
                    .syntax()
                    .descendants()
                    .filter(|n| n.kind() == k)
                    .count()
            };
            for k in [
                SyntaxKind::HEADLINE,
                SyntaxKind::TABLE,
                SyntaxKind::SRC_BLOCK,
            ] {
                assert_eq!(count(&t, k), count(&once, k), "{} {k:?}", p.display());
            }
            n += 1;
        }
        assert!(n > 3);
    }
}
