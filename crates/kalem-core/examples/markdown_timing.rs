//! The Markdown mode on a generated document (10 MB by default, `SIZE`):
//! a full parse, and a keystroke's reparse with a screen of lines drawn:
//! `cargo run --release -p kalem-core --example markdown_timing`.

#![allow(clippy::print_stdout)]

use std::time::Instant;

use kalem_core::markdown::Md;

fn main() {
    let size: usize = std::env::var("SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000_000);
    let mut s = String::from("# A long document\n\n");
    let mut i = 0;
    while s.len() < size {
        if i % 10 == 0 {
            s.push_str(&format!("## Part {i}\n\n"));
        }
        s.push_str(&format!(
            "Paragraph {i} with *emphasis*, **strong**, `code` and a [link](x{i}.md),\nand a second line of plain words to read.\n\n"
        ));
        if i % 5 == 0 {
            s.push_str("- one\n- two with `code`\n- [ ] a task\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n");
        }
        i += 1;
    }
    let t = Instant::now();
    let md = Md::parse(&s);
    let full = t.elapsed();
    let nodes = md.nodes.len();
    let mut text = s.clone();
    let mut md = md;
    let mut at = text.len() / 2;
    at += text[at..].find("plain").unwrap();
    let mut keys = Vec::new();
    for _ in 0..100 {
        let before = text.clone();
        text.insert(at, 'x');
        at += 1;
        let t = Instant::now();
        md = md.reparse_owned(&before, &text);
        keys.push(t.elapsed());
    }
    keys.sort();
    // A screen of lines' nodes, as the view asks for them.
    let mid = text[..text.len() / 2].matches('\n').count();
    let t = Instant::now();
    let mut n = 0;
    for l in mid..mid + 50 {
        n += md.on_line(l).count();
    }
    let screen = t.elapsed();
    println!("the nodes of 50 lines ({n}): {screen:?}");
    println!(
        "{} bytes, {nodes} nodes: full parse {full:?}, keystroke reparse p50 {:?} p99 {:?}",
        text.len(),
        keys[keys.len() / 2],
        keys[keys.len() * 99 / 100]
    );
}
