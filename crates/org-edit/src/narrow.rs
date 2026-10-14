//! Narrowing: the ranges of `org-narrow-to-subtree`,
//! `org-narrow-to-element` and `org-narrow-to-block`, and commands run on
//! a narrowed part of a document.
//!
//! Narrowing is view state: the editor keeps the range and widens by
//! dropping it. A command run while narrowed sees only that part, as in
//! Emacs, with the document's settings (TODO keywords, `#+STARTUP`), and
//! its edits are moved back into the whole document.

use std::ops::Range;

use org_model::Document;
use org_syntax::{SyntaxKind, SyntaxNode};

use crate::buffer::EditError;
use crate::transaction::{Edit, Selection, Transaction};

fn is_element(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        HEADLINE
            | INLINETASK
            | PLANNING
            | PROPERTY_DRAWER
            | NODE_PROPERTY
            | DRAWER
            | PLAIN_LIST
            | ITEM
            | TABLE
            | TABLE_ROW
            | CENTER_BLOCK
            | QUOTE_BLOCK
            | SPECIAL_BLOCK
            | DYNAMIC_BLOCK
            | FOOTNOTE_DEFINITION
            | BABEL_CALL
            | CLOCK
            | COMMENT
            | COMMENT_BLOCK
            | DIARY_SEXP
            | EXAMPLE_BLOCK
            | EXPORT_BLOCK
            | FIXED_WIDTH
            | HORIZONTAL_RULE
            | KEYWORD
            | LATEX_ENVIRONMENT
            | PARAGRAPH
            | SRC_BLOCK
            | VERSE_BLOCK
    )
}

fn is_greater(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        HEADLINE
            | INLINETASK
            | PROPERTY_DRAWER
            | DRAWER
            | PLAIN_LIST
            | ITEM
            | TABLE
            | CENTER_BLOCK
            | QUOTE_BLOCK
            | SPECIAL_BLOCK
            | DYNAMIC_BLOCK
            | FOOTNOTE_DEFINITION
    )
}

fn range(n: &SyntaxNode) -> Range<usize> {
    usize::from(n.text_range().start())..usize::from(n.text_range().end())
}

/// `org-element-at-point`: the element at `pos`, going into a greater
/// element only when `pos` is within its contents.
pub fn element_at(root: &SyntaxNode, pos: usize) -> Option<SyntaxNode> {
    let mut parent = root.clone();
    let mut found: Option<SyntaxNode> = None;
    loop {
        let len = usize::from(root.text_range().end());
        let child = parent.children().find(|c| {
            let r = range(c);
            r.start <= pos && (pos < r.end || (r.end == len && pos == len))
        });
        let Some(c) = child else { return found };
        if c.kind() == SyntaxKind::SECTION {
            parent = c;
            continue;
        }
        if !is_element(c.kind()) {
            return found;
        }
        found = Some(c.clone());
        if !is_greater(c.kind()) {
            return found;
        }
        // Into the contents when point is past their start (or at it, but
        // for lists and tables), and before their end, or at the end of
        // the text.
        let Some(r) = org_syntax::ast::contents_range(&c) else {
            return found;
        };
        let (cb, ce) = (usize::from(r.start()), usize::from(r.end()));
        let list_or_table = matches!(c.kind(), SyntaxKind::PLAIN_LIST | SyntaxKind::TABLE);
        let after_start = cb < pos || (cb == pos && !list_or_table);
        let before_end = ce > pos || (pos == ce && ce == len);
        if !(after_start && before_end) {
            return found;
        }
        parent = c;
    }
}

/// `org-narrow-to-subtree`: the headline around `point`, without the line
/// feed before the next headline.
pub fn subtree(doc: &Document, point: usize) -> Result<Range<usize>, EditError> {
    let root = doc.parse().syntax();
    let el = element_at(&root, point);
    let heading = el.and_then(|e| e.ancestors().find(|a| a.kind() == SyntaxKind::HEADLINE));
    let Some(h) = heading else {
        return Err(EditError::new("Before first headline"));
    };
    let r = range(&h);
    let len = usize::from(root.text_range().end());
    Ok(r.start..if r.end == len { r.end } else { r.end - 1 })
}

/// `org-narrow-to-element`: a headline whole, a greater element's
/// contents, or another element whole.
pub fn element(doc: &Document, point: usize) -> Result<Range<usize>, EditError> {
    let root = doc.parse().syntax();
    let Some(el) = element_at(&root, point) else {
        return Err(EditError::new("No element at point"));
    };
    if el.kind() != SyntaxKind::HEADLINE && is_greater(el.kind()) {
        let r =
            org_syntax::ast::contents_range(&el).ok_or_else(|| EditError::new("Empty element"))?;
        return Ok(usize::from(r.start())..usize::from(r.end()));
    }
    Ok(range(&el))
}

/// `org-narrow-to-block`.
pub fn block(doc: &Document, point: usize) -> Result<Range<usize>, EditError> {
    let root = doc.parse().syntax();
    let blockish =
        element_at(&root, point).is_some_and(|e| format!("{:?}", e.kind()).contains("BLOCK"));
    if !blockish {
        return Err(EditError::new("Not in a block"));
    }
    element(doc, point)
}

/// Runs `command` on the part `range` of `doc`, as Emacs runs a command in
/// a narrowed buffer, and returns its change to the whole document.
/// `command` gets the narrowed document and the point relative to it.
pub fn narrowed(
    doc: &Document,
    range: Range<usize>,
    point: usize,
    command: impl FnOnce(&Document, usize) -> Result<Transaction, EditError>,
) -> Result<Transaction, EditError> {
    let text = doc.parse().text();
    let part = &text[range.clone()];
    let sub = Document::with_settings(
        org_syntax::parse_with(part, doc.parse().context()),
        std::sync::Arc::new(doc.settings().clone()),
        None,
    );
    let local = point.clamp(range.start, range.end) - range.start;
    let t = command(&sub, local).map_err(|e| EditError {
        message: e.message,
        point: e.point.map(|p| p + range.start),
    })?;
    let mut out = Transaction::new(t.label.clone());
    out.edits = t
        .edits
        .iter()
        .map(|e| Edit {
            range: e.range.start + range.start..e.range.end + range.start,
            insert: e.insert.clone(),
        })
        .collect();
    out.selection_after = t.selection_after.map(|s| Selection {
        anchor: s.anchor + range.start,
        head: s.head + range.start,
    });
    Ok(out)
}

/// Runs `command` on `doc` as Emacs runs it on a file whose lines end in
/// CR LF: on the text with line feeds alone (Emacs reads such a file so,
/// and writes the CRs back), so that the end of a line is before its CR
/// and nothing typed there splits the pair. Its change comes back to the
/// document as it is, with a CR before each line feed it made, as edits
/// of the lines it changed. `command` gets the document and a function
/// that takes a position of `doc` to that text. A document whose line
/// feeds are not all after a CR is run as it is.
pub fn with_line_feeds(
    doc: &Document,
    command: impl FnOnce(&Document, &dyn Fn(usize) -> usize) -> Result<Transaction, EditError>,
) -> Result<Transaction, EditError> {
    let text = doc.parse().text();
    // Where each CR LF's CR is.
    let crs: Vec<usize> = text.match_indices("\r\n").map(|(i, _)| i).collect();
    if crs.is_empty() || crs.len() != text.matches('\n').count() {
        return command(doc, &|p| p);
    }
    let plain = text.replace("\r\n", "\n");
    // A position of the text, there: one byte fewer for each CR before it,
    // and at a line's end when it was between a CR and its line feed.
    let to_plain = |p: usize| {
        let p = p.min(text.len());
        p - crs.partition_point(|&c| c < p)
    };
    // A position of a text with line feeds alone, in the same text with
    // CR LF.
    fn with_crs(text: &str, q: usize) -> usize {
        q + text[..q.min(text.len())].matches('\n').count()
    }
    let sub = Document::with_settings(
        org_syntax::parse_with(&plain, doc.parse().context()),
        std::sync::Arc::new(doc.settings().clone()),
        None,
    );
    let t = command(&sub, &to_plain).map_err(|e| EditError {
        message: e.message,
        point: e.point.map(|p| with_crs(&plain, p)),
    })?;
    if !t.fits(&plain) {
        return Err(EditError {
            message: "The command's edit does not fit the text".into(),
            point: None,
        });
    }
    let after = t.apply(&plain);
    let mut out = Transaction::new(t.label.clone());
    for e in crate::buffer::line_edits(&text, &after.replace('\n', "\r\n")) {
        out.edit(e.range, e.insert);
    }
    out.selection_after = t.selection_after.map(|s| Selection {
        anchor: with_crs(&after, s.anchor),
        head: with_crs(&after, s.head),
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_lines_as_line_feeds() {
        // The end of a heading's line is before its CR, as Emacs has it:
        // a tag set at it does not split the CR LF.
        let text = "* A\r\nbody\r\n* B\r\n";
        let doc = Document::new(org_syntax::parse(text));
        let t = with_line_feeds(&doc, |d, at| {
            let eol = d.parse().text()[at(0)..].find('\n').unwrap() + at(0);
            let mut t = Transaction::new("t");
            t.edit(eol..eol, " :x:");
            Ok(t.select(Selection::caret(eol + 4)))
        })
        .unwrap();
        assert_eq!(t.apply(text), "* A :x:\r\nbody\r\n* B\r\n");
        assert_eq!(t.selection_after, Some(Selection::caret(7)));
        // A line added: with its CR.
        let t = with_line_feeds(&doc, |_, at| {
            let mut t = Transaction::new("t");
            t.edit(at(5)..at(5), "new\n");
            Ok(t)
        })
        .unwrap();
        assert_eq!(t.apply(text), "* A\r\nnew\r\nbody\r\n* B\r\n");
        // Between a CR and its line feed is the line's end.
        let t = with_line_feeds(&doc, |_, at| {
            let mut t = Transaction::new("t");
            t.edit(at(4)..at(4), "!");
            Ok(t)
        })
        .unwrap();
        assert_eq!(t.apply(text), "* A!\r\nbody\r\n* B\r\n");
    }

    #[test]
    fn ranges() {
        let text = "* A\ntext\n** B\n- x\n- y\n* C\n#+begin_quote\nq\n#+end_quote\n";
        let doc = Document::new(org_syntax::parse(text));
        assert_eq!(
            &text[subtree(&doc, 5).unwrap()],
            "* A\ntext\n** B\n- x\n- y"
        );
        assert_eq!(&text[subtree(&doc, 16).unwrap()], "** B\n- x\n- y");
        assert_eq!(&text[element(&doc, 21).unwrap()], "y\n");
        assert_eq!(&text[element(&doc, 18).unwrap()], "y\n");
        assert_eq!(&text[block(&doc, 30).unwrap()], "q\n");
        // At the contents, the element is the paragraph, as in Emacs.
        assert!(block(&doc, 40).is_err());
        assert!(block(&doc, 5).is_err());
    }
}
