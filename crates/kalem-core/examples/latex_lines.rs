//! Draws lines of a LaTeX file and says how long they take, for finding
//! slow lines: `cargo run --release -p kalem-core --example latex_lines
//! -- FILE [FIRST] [COUNT]`.

#![allow(clippy::print_stdout)]

use std::sync::Arc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::PathBuf::from(&args[1]);
    let first: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(0);
    let count: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(500);
    let t = Instant::now();
    let base = kalem_core::settings::Config::default().parse_base();
    let d = kalem_core::DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base)
        .expect("open");
    let _ = kalem_core::latex_view::line_view(&d, 0..0, None);
    println!("open {:.1} ms", t.elapsed().as_secs_f64() * 1000.);
    let lines = d.text().line_count();
    let mut worst = (0.0, 0);
    let t = Instant::now();
    for l in first..(first + count).min(lines) {
        let r = d.text().line_range(l);
        let s = Instant::now();
        std::hint::black_box(kalem_core::latex_view::line_view(&d, r, None));
        let ms = s.elapsed().as_secs_f64() * 1000.;
        if ms > worst.0 {
            worst = (ms, l);
        }
    }
    println!(
        "{count} lines from {first}: {:.1} ms, slowest {:.2} ms (line {})",
        t.elapsed().as_secs_f64() * 1000.,
        worst.0,
        worst.1 + 1
    );
}
