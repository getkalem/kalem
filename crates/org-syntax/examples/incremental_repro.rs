//! Reproduces an incremental reparse failure: prints the first place where
//! the incremental tree differs from a full parse.
//!
//! Usage: incremental_repro OLD.org START END INSERT

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::needless_range_loop
)]

use org_syntax::{ParseContext, SyntaxNode, TextEdit, TextRange, TextSize};

fn outline(n: &SyntaxNode, depth: usize, out: &mut Vec<String>) {
    if depth > 6 {
        return;
    }
    out.push(format!(
        "{}{:?} {:?}",
        "  ".repeat(depth),
        n.kind(),
        n.text_range()
    ));
    for c in n.children() {
        outline(&c, depth + 1, out);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let (a, b): (u32, u32) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let insert = args
        .get(4)
        .cloned()
        .unwrap_or_default()
        .replace("\\n", "\n");
    let edit = TextEdit {
        range: TextRange::new(TextSize::from(a), TextSize::from(b)),
        insert,
    };
    let ctx = ParseContext::for_document(&text, &ParseContext::default());
    let old = org_syntax::parse_with(&text, &ctx);
    let new_text = edit.apply(&text);
    let (inc, level) = old.reparse_with_level(&new_text, &edit);
    let fresh = org_syntax::parse_with(
        &new_text,
        &ParseContext::for_document(&new_text, &ParseContext::default()),
    );
    println!("level {level:?}, equal: {}", inc.green() == fresh.green());
    println!(
        "old text around edit: {:?}",
        &text[(a as usize).saturating_sub(120)..(b as usize + 80).min(text.len())]
    );
    let (mut x, mut y) = (Vec::new(), Vec::new());
    outline(&inc.syntax(), 0, &mut x);
    outline(&fresh.syntax(), 0, &mut y);
    for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
        if p != q {
            println!("first difference at line {i}:");
            for k in i.saturating_sub(3)..(i + 6).min(x.len()) {
                println!("  inc   {}", x[k]);
            }
            for k in i.saturating_sub(3)..(i + 6).min(y.len()) {
                println!("  fresh {}", y[k]);
            }
            return;
        }
    }
    println!(
        "outlines equal up to depth 6 ({} vs {} lines)",
        x.len(),
        y.len()
    );
    // Then compare every node and token.
    let flat = |n: &SyntaxNode| -> Vec<String> {
        n.descendants_with_tokens()
            .map(|e| match e {
                rowan::NodeOrToken::Node(n) => format!("{:?} {:?}", n.kind(), n.text_range()),
                rowan::NodeOrToken::Token(t) => {
                    format!("  {:?} {:?} {:?}", t.kind(), t.text_range(), t.text())
                }
            })
            .collect()
    };
    let (x, y) = (flat(&inc.syntax()), flat(&fresh.syntax()));
    for i in 0..x.len().min(y.len()) {
        if x[i] != y[i] {
            println!("first element difference at {i}:");
            for k in i.saturating_sub(4)..(i + 4).min(x.len()) {
                println!("  inc   {}", x[k]);
            }
            for k in i.saturating_sub(4)..(i + 4).min(y.len()) {
                println!("  fresh {}", y[k]);
            }
            return;
        }
    }
    println!("elements equal ({} vs {})", x.len(), y.len());
}
