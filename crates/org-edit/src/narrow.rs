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
    let text = doc.parse().syntax().to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

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
