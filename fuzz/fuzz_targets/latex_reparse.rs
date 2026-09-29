//! An edit reparsed incrementally gives the tree of a full parse. The
//! first four bytes choose the edit; the rest is the document, split at
//! the first NUL into document and inserted text.
#![no_main]

use latex_syntax::TextEdit;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let Ok(rest) = std::str::from_utf8(&data[4..]) else { return };
    let (doc, insert) = rest.split_once('\0').unwrap_or((rest, ""));
    let mut a = (u16::from_le_bytes([data[0], data[1]]) as usize) % (doc.len() + 1);
    while !doc.is_char_boundary(a) {
        a -= 1;
    }
    let mut b = (a + data[2] as usize % 32).min(doc.len());
    while !doc.is_char_boundary(b) {
        b -= 1;
    }
    let edit = TextEdit { range: a..b, insert: insert.to_string() };
    let old = latex_syntax::parse(doc);
    let new = edit.apply(doc);
    if let Some(inc) = old.reparse_incremental(&new, &edit) {
        assert_eq!(inc, latex_syntax::parse(&new));
    }
});
