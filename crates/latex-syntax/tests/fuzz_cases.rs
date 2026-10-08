//! Inputs the fuzz targets (`fuzz/fuzz_targets/latex_*.rs`) found, kept
//! as tests: an edit reparsed incrementally gives the tree of a full parse.

use latex_syntax::TextEdit;

/// What `latex_reparse` checks: the incremental reparse, when it gives
/// one, is the full parse of the edited text.
fn check(doc: &str, edit: TextEdit) {
    let old = latex_syntax::parse(doc);
    let new = edit.apply(doc);
    if let Some(inc) = old.reparse_incremental(&new, &edit) {
        assert_eq!(inc, latex_syntax::parse(&new));
    }
}

#[test]
fn a_def_at_a_paragraph_break_as_the_edit_left_it() {
    // latex_reparse found this one (CI, 2026-10-08): replacing the `\x01`
    // after `\ee` with text that ends a paragraph and starts `e\t`.
    let doc = "\x0c%`%#\x0c!\x1e%%\n\x10\n\n\n\\def\n\n\n\\def\n\n\n\n\n\n\n\n\\ee\x01%\x02";
    let edit = TextEdit {
        range: 30..31,
        insert: "%f\n\n\n\ne\t".into(),
    };
    check(doc, edit);
}
