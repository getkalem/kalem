//! Parse and keystroke times on a generated 1 MB paper, for
//! `docs/performance.md`: `cargo run --release -p latex-syntax --example
//! timing`.

#![allow(clippy::print_stdout)]

use std::time::Instant;

use latex_syntax::{TextEdit, parse};

fn main() {
    let mut s = String::from("\\documentclass{book}\n\\usepackage{amsmath}\n\n\\begin{document}\n");
    let mut i = 0;
    while s.len() < 1_000_000 {
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
    let mut full = Vec::new();
    let mut p = parse(&s);
    for _ in 0..10 {
        let t = Instant::now();
        p = parse(&s);
        full.push(t.elapsed());
    }
    full.sort();
    let mut keys = Vec::new();
    for k in 0..200 {
        let at = s.len() * k / 200;
        let at = at + s[at..].find("Paragraph").unwrap_or(0) + 3;
        let edit = TextEdit {
            range: at..at,
            insert: "x".into(),
        };
        let new = edit.apply(&s);
        let t = Instant::now();
        let r = p.reparse(&new, &edit);
        keys.push(t.elapsed());
        std::hint::black_box(r);
    }
    keys.sort();
    println!(
        "{} bytes: full parse p50 {:?}; keystroke p50 {:?}, p99 {:?}",
        s.len(),
        full[full.len() / 2],
        keys[keys.len() / 2],
        keys[keys.len() * 99 / 100]
    );
}
