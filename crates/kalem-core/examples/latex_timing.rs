//! The LaTeX mode on a generated 1 MB paper: opening, drawing a screen of
//! lines, a keystroke: `cargo run --release -p kalem-core --example
//! latex_timing`.

#![allow(clippy::print_stdout)]

use std::sync::Arc;
use std::time::Instant;

fn main() {
    let mut s = String::from("\\documentclass{book}\n\\usepackage{amsmath}\n\n\\begin{document}\n");
    let mut i = 0;
    let size: usize = std::env::var("SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    while s.len() < size {
        if i % 20 == 0 {
            s.push_str(&format!("\\section{{Part {i}}}\\label{{s{i}}}\n\n"));
        }
        s.push_str(&format!(
            "Paragraph {i} cites \\cite[p.~{i}]{{key{i}}} and has $x_{{{i}}}^2 + \\frac{{a}}{{b}}$ in it, \\emph{{some}} \\textbf{{words}} % a comment\nand a second line with more text to read.\n\n"
        ));
        if i % 7 == 0 {
            s.push_str("\\begin{itemize}\n\\item One\n\\item Two \\ref{s0}\n\\end{itemize}\n\n\\begin{equation}\n  E = mc^2 \\label{e}\n\\end{equation}\n\n");
        }
        i += 1;
    }
    s.push_str("\\end{document}\n");
    let meta = kalem_core::Metadata {
        path: None,
        mode: kalem_core::DocumentMode::Latex,
        line_ending: kalem_core::LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    let t = Instant::now();
    let mut d =
        kalem_core::DocumentState::new(s.clone(), meta, Arc::new(org_model::Settings::default()));
    let open = t.elapsed();
    let screen = |d: &kalem_core::DocumentState, first: usize| {
        let text = d.text();
        for l in first..first + 50 {
            let mut r = text.line_range(l);
            if text.as_str()[r.clone()].ends_with('\n') {
                r.end -= 1;
            }
            std::hint::black_box(kalem_core::latex_view::line_view(
                d,
                r,
                Some(d.selection.head),
            ));
        }
    };
    let mid = d.text().line_of(s.len() / 2);
    let t = Instant::now();
    screen(&d, mid);
    let first_screen = t.elapsed();
    let t = Instant::now();
    let blocks = kalem_core::latex_view::blocks(&d);
    let blocks_time = t.elapsed();
    let at = d.text().line_start(mid + 3);
    d.selection = org_edit::Selection::caret(at);
    let mut keys = Vec::new();
    for _ in 0..50 {
        let t = Instant::now();
        d.type_text("x", false, Instant::now());
        screen(&d, mid);
        std::hint::black_box(kalem_core::latex_view::blocks(&d));
        keys.push(t.elapsed());
    }
    keys.sort();
    println!(
        "{} bytes: open {open:?}, a screen of 50 lines {first_screen:?}, blocks {blocks_time:?} ({} blocks), keystroke with a screen and blocks p50 {:?} p99 {:?}",
        s.len(),
        blocks.len(),
        keys[keys.len() / 2],
        keys[keys.len() * 99 / 100]
    );
}
