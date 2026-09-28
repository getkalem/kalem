//! Time to build the model of a document, and after an edit.
//!
//! `cargo run --release --example model_timing -p org-model -- FILE`

#![allow(clippy::print_stdout)]

use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("file");
    let text = std::fs::read_to_string(&path).expect("read");
    let parse = org_syntax::parse(&text);
    let t = Instant::now();
    let doc = org_model::Document::new(parse.clone());
    let n = doc.outline().entries.len();
    let t_outline = t.elapsed();
    let t = Instant::now();
    let _ = doc.info();
    let t_info = t.elapsed();
    let t = Instant::now();
    let mut tags = 0;
    for i in 0..n {
        tags += doc.tags(org_model::EntryId(i)).len();
    }
    let t_tags = t.elapsed();
    let t = Instant::now();
    let _ = doc.statistics_cookies();
    let t_stats = t.elapsed();
    let t = Instant::now();
    let _ = doc.clock_sums();
    let t_clock = t.elapsed();
    println!("{path}: {} bytes, {n} entries, {tags} tags", text.len());
    println!(
        "outline {t_outline:.2?}, info {t_info:.2?}, all tags {t_tags:.2?}, statistics {t_stats:.2?}, clock sums {t_clock:.2?}"
    );

    // Typing in the middle of the document, with a cache across versions.
    let settings = std::sync::Arc::new(org_model::Settings::default());
    let cache = org_model::ModelCache::new();
    let mut parse = parse;
    let mut text = text;
    let d = org_model::Document::with_cache(parse.clone(), settings.clone(), None, cache.clone());
    let _ = (d.outline(), d.info());
    let mut at = text.len() / 2;
    while !text.is_char_boundary(at) {
        at += 1;
    }
    let mut times = Vec::new();
    for _ in 0..50 {
        let edit = org_syntax::TextEdit {
            range: org_syntax::TextRange::new((at as u32).into(), (at as u32).into()),
            insert: "x".into(),
        };
        let new_text = edit.apply(&text);
        parse = parse.reparse(&new_text, &edit);
        text = new_text;
        at += 1;
        let t = Instant::now();
        let d =
            org_model::Document::with_cache(parse.clone(), settings.clone(), None, cache.clone());
        let _ = (d.outline(), d.info());
        times.push(t.elapsed());
    }
    times.sort();
    println!(
        "after a keystroke, outline + info with the cache: median {:.2?}, max {:.2?}",
        times[times.len() / 2],
        times[times.len() - 1]
    );
}
