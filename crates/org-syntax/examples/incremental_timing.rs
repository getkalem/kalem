//! Times typing into a document: full parse versus incremental reparse.
//!
//! Usage: incremental_timing FILE

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::needless_range_loop
)]

use std::time::Instant;

use org_syntax::{ParseContext, TextEdit, TextRange, TextSize};

fn main() {
    let path = std::env::args().nth(1).expect("file");
    let text = std::fs::read_to_string(&path).unwrap();
    let ctx = ParseContext::for_document(&text, &ParseContext::default());
    let t = Instant::now();
    let mut parse = org_syntax::parse_with(&text, &ctx);
    let full = t.elapsed();
    // Type a sentence in the middle of a paragraph, one character at a time.
    let mut pos = text.len() / 2;
    while !(text.as_bytes()[pos] == b' ' && text.as_bytes()[pos - 1].is_ascii_alphabetic()) {
        pos += 1;
    }
    let mut text = text;
    let sentence = " the quick brown fox jumps over the lazy dog".repeat(
        std::env::var("REPEAT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1),
    );
    let mut times = Vec::new();
    for c in sentence.chars() {
        let edit = TextEdit {
            range: TextRange::empty(TextSize::from(pos as u32)),
            insert: c.to_string(),
        };
        let new_text = edit.apply(&text);
        let t = Instant::now();
        let (p, level) = parse.reparse_with_level(&new_text, &edit);
        times.push((t.elapsed(), level));
        parse = p;
        text = new_text;
        pos += c.len_utf8();
    }
    times.sort_by_key(|t| t.0);
    let median = times[times.len() / 2].0;
    let max = times.last().unwrap().0;
    println!("{}: {} bytes", path, text.len());
    println!("full parse:          {full:?}");
    println!("incremental median:  {median:?}");
    println!(
        "incremental max:     {max:?} ({:?})",
        times.last().unwrap().1
    );
}
