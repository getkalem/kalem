//! Infers the style of Org files and prints a summary.
//!
//! Usage: `cargo run --release -p org-edit --example style_report FILE_OR_DIR...`

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for e in entries {
            collect(&e, out);
        }
    } else if p.extension().is_some_and(|e| e == "org") {
        out.push(p.to_path_buf());
    }
}

fn main() {
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        collect(Path::new(&a), &mut files);
    }
    let mut summary: BTreeMap<String, usize> = BTreeMap::new();
    let (mut bytes, mut total) = (0, std::time::Duration::ZERO);
    let mut slowest = (std::time::Duration::ZERO, PathBuf::new());
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        let parse = org_syntax::parse(&text);
        let t = Instant::now();
        let s = org_edit::style::Style::infer(&parse);
        let d = t.elapsed();
        total += d;
        bytes += text.len();
        if d > slowest.0 {
            slowest = (d, f.clone());
        }
        for key in [
            format!("indentation {:?}", s.indentation),
            format!("keyword case {:?}", s.keyword_case),
            format!("block case {:?}", s.block_case),
            format!("blank before heading {}", s.blank_before_heading),
            format!("startup indented {}", s.startup_indented),
            format!("src indentation {}", s.src_content_indentation),
            format!("tags column {}", s.tags_column),
            format!("todo {}/{}", s.todo_keyword, s.done_keyword),
            format!("crlf {}", s.crlf),
        ] {
            *summary.entry(key).or_default() += 1;
        }
    }
    for (k, n) in &summary {
        println!("{n:6}  {k}");
    }
    println!(
        "{} files, {} bytes, inference {:?} in total, slowest {:?} ({})",
        files.len(),
        bytes,
        total,
        slowest.0,
        slowest.1.display()
    );
}
