//! Cost of one keystroke on a large document: the text edit with its line
//! index, and the whole change (history and incremental reparse).
//!
//! Usage: `cargo run --release -p kalem-core --example text_timing [FILE.org]`

#![allow(clippy::print_stdout)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::{DocumentMode, DocumentState, LineEnding, Metadata, Text};
use org_edit::{ChangeKind, Selection, Transaction};

fn percentile(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 - 1.0) * p) as usize]
}

fn main() {
    let text = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(path).expect("file"),
        None => "* Heading\nSome text with *bold* and a [[https://orgmode.org][link]].\n- item\n"
            .repeat(50_000),
    };
    println!("{} bytes, {} lines", text.len(), text.lines().count());
    // Positions spread over the document, at line ends inside paragraphs.
    let positions: Vec<usize> = (1..500)
        .map(|i| text.len() * i / 500)
        .map(|p| text[p..].find('\n').map_or(p, |k| p + k))
        .collect();

    let mut t = Text::new(text.clone());
    let mut times = Vec::new();
    for &p in &positions {
        let s = Instant::now();
        t.replace(p..p, "x");
        times.push(s.elapsed());
    }
    println!(
        "text edit + line index: p50 {:?}, p99 {:?}",
        percentile(&mut times, 0.5),
        percentile(&mut times, 0.99)
    );

    let meta = Metadata {
        path: None,
        mode: DocumentMode::Org,
        line_ending: LineEnding::Lf,
        bom: false,
    };
    let mut d = DocumentState::new(text, meta, Arc::new(org_model::Settings::default()));
    let mut times = Vec::new();
    for (i, &p) in positions.iter().enumerate() {
        let p = p + i; // earlier inserts shift later positions
        let mut tx = Transaction::new("Type");
        tx.replace(p..p, "x").unwrap();
        let tx = tx.select(Selection::caret(p + 1));
        let s = Instant::now();
        d.apply(&tx, ChangeKind::Typing, Instant::now());
        times.push(s.elapsed());
    }
    println!(
        "keystroke with history and reparse: p50 {:?}, p99 {:?}",
        percentile(&mut times, 0.5),
        percentile(&mut times, 0.99)
    );
    println!("last reparse: {:?}", d.last_reparse());
}
