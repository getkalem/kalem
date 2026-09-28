//! Load time of the syntax set and highlighting speed (D16).

#![allow(clippy::print_stdout)]
use std::time::Instant;

fn main() {
    let t = Instant::now();
    let rust = kalem_highlight::Language::find("rust").unwrap();
    let load = t.elapsed();
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kalem-core/src/view.rs"
    ))
    .unwrap();
    let t = Instant::now();
    let _ = kalem_highlight::highlight(rust, &src);
    println!(
        "first pass (compiling patterns) {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    let t = Instant::now();
    let lines = kalem_highlight::highlight(rust, &src);
    let el = t.elapsed();
    println!(
        "load {:.1} ms; {} lines, {} KB in {:.1} ms ({:.0} µs/line)",
        load.as_secs_f64() * 1e3,
        lines.len(),
        src.len() / 1024,
        el.as_secs_f64() * 1e3,
        el.as_secs_f64() * 1e6 / lines.len() as f64
    );
}
