//! SyncTeX lookups on a large document (T2.7h.35): reading the file, and
//! lookups both ways.
//!
//! `cargo run --release -p kalem-core --example synctex_timing -- FILE.pdf FILE.tex`

#![allow(clippy::print_stdout)]

use std::path::Path;
use std::time::{Duration, Instant};

use kalem_core::synctex::Synctex;

fn p50(mut v: Vec<Duration>) -> (Duration, Duration) {
    v.sort();
    (v[v.len() / 2], v[v.len() - 1])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (pdf, tex) = (Path::new(&args[0]), Path::new(&args[1]));
    let file = Synctex::for_pdf(pdf).expect("a SyncTeX file");
    let t = Instant::now();
    let st = Synctex::load(&file).unwrap();
    let load = t.elapsed();
    // Kept for the lookups after the first.
    Synctex::cached(pdf).unwrap();
    let t = Instant::now();
    Synctex::cached(pdf).unwrap();
    let again = t.elapsed();
    let pages = st.records.iter().map(|r| r.page).max().unwrap_or(0);
    let lines = std::fs::read_to_string(tex).unwrap().lines().count();
    let mut forward = Vec::new();
    let mut points = Vec::new();
    for i in 0..500 {
        let line = 1 + i * lines / 500;
        let t = Instant::now();
        let p = st.forward(tex, line);
        forward.push(t.elapsed());
        if let Some(p) = p {
            points.push((p.page, p.x + 20.0, p.y + p.height / 2.0));
        }
    }
    let mut inverse = Vec::new();
    for (page, x, y) in points {
        let t = Instant::now();
        std::hint::black_box(st.inverse(page, x, y));
        inverse.push(t.elapsed());
    }
    let (f50, fmax) = p50(forward);
    let (i50, imax) = p50(inverse);
    println!(
        "{} records, {pages} pages: read in {load:?} (then kept: {again:?}); forward p50 {f50:?} max {fmax:?}; inverse p50 {i50:?} max {imax:?}",
        st.records.len()
    );
}
