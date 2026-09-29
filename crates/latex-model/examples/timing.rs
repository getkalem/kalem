//! Building the model of a generated 1 MB paper, and again after an edit
//! with the cache: `cargo run --release -p latex-model --example timing`.

#![allow(clippy::print_stdout)]

use std::time::Instant;

use latex_model::Cache;
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
    let p = parse(&s);
    let t = Instant::now();
    let mut cache = Cache::default();
    let m = cache.model(&p);
    let first = t.elapsed();
    let at = s.len() / 2;
    let at = at + s[at..].find("Paragraph").unwrap() + 3;
    let edit = TextEdit {
        range: at..at,
        insert: "x".into(),
    };
    let new = edit.apply(&s);
    let p2 = p.reparse(&new, &edit);
    let t = Instant::now();
    let m2 = cache.model(&p2);
    let again = t.elapsed();
    println!(
        "{} bytes, {} sections, {} labels, {} citations: model {first:?}, after an edit {again:?}",
        s.len(),
        m.sections.len(),
        m2.labels.len(),
        m2.citations.len()
    );
}
