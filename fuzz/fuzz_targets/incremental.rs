//! An edit applied incrementally gives the same tree as a full parse.
//! The first four bytes choose the edit; the rest is the document, split
//! at the first NUL into document and inserted text.
#![no_main]

use libfuzzer_sys::fuzz_target;
use org_syntax::{TextEdit, TextRange, TextSize};

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
    let edit = TextEdit { range: TextRange::new(TextSize::from(a as u32), TextSize::from(b as u32)), insert: insert.to_string() };
    let old = org_syntax::parse(doc);
    let new = edit.apply(doc);
    let inc = old.reparse(&new, &edit);
    let fresh = org_syntax::parse(&new);
    assert!(inc.green() == fresh.green());
});
