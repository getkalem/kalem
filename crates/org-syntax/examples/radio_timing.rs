//! Parse time of a document with many radio targets (T0.3.15d).
//!
//! `cargo run --release --example radio_timing -p org-syntax`

#![allow(clippy::print_stdout)]

use std::time::Instant;

fn doc(targets: usize) -> String {
    let mut s = String::new();
    for i in 0..targets {
        s.push_str(&format!("- <<<Term {i} alpha>>>\n"));
    }
    s.push('\n');
    let mut i = 0usize;
    while s.len() < 1_000_000 {
        let t = (i * 7919) % targets;
        s.push_str(&format!("Paragraph {i} mentions term {t} alpha and TERM {} ALPHA, then plain words follow here.\n", (t + 1) % targets));
        i += 1;
    }
    s
}

fn main() {
    for n in [100, 1_000, 5_000, 10_000] {
        let text = doc(n);
        let t = Instant::now();
        let p = org_syntax::parse(&text);
        let took = t.elapsed();
        let links = p
            .syntax()
            .descendants()
            .filter(|d| d.kind() == org_syntax::SyntaxKind::LINK)
            .count();
        println!(
            "{n:>6} targets, {:.2} MB, {links} radio links: {took:.2?}",
            text.len() as f64 / 1e6
        );
    }
}
