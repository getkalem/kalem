//! Random sequences of the LaTeX editing commands, for
//! `tools/latex-edit-fuzz.py` to compile: each variant of TEMPLATE is the
//! template after COUNT commands at random cursors and selections, written
//! to OUT/edit-SEED-N.tex with the commands in OUT/edit-SEED-N.log.
//!
//! `cargo run --release -p kalem-core --example latex_edits -- TEMPLATE OUT
//! [VARIANTS] [SEED] [COUNT]`

#![allow(clippy::print_stdout)]

use std::fmt::Write as _;
use std::sync::Arc;

/// xorshift64*: a fixed sequence for a seed, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn commands(rng: &mut Rng) -> (&'static str, serde_json::Value) {
    use serde_json::json;
    let all: [(&str, serde_json::Value); 21] = [
        ("latex.format.bold", json!(null)),
        ("latex.format.italic", json!(null)),
        ("latex.format.code", json!(null)),
        ("latex.format.underline", json!(null)),
        ("latex.enter", json!(null)),
        ("latex.list.indent", json!(null)),
        ("latex.list.outdent", json!(null)),
        ("latex.section.promote", json!(null)),
        ("latex.section.demote", json!(null)),
        ("latex.section.moveUp", json!(null)),
        ("latex.section.moveDown", json!(null)),
        ("latex.section.setLevel", json!({ "level": rng.below(6) })),
        ("latex.math.toggleDisplay", json!(null)),
        ("latex.math.toggleNumbering", json!(null)),
        (
            "latex.insert.figure",
            json!({ "path": "example-image", "width": "0.3", "caption": "Inserted." }),
        ),
        ("latex.insert.figure", json!({ "path": "example-image" })),
        (
            "latex.insert.table",
            json!({ "columns": 1 + rng.below(4), "rows": 1 + rng.below(3) }),
        ),
        ("latex.insert.equation", json!(null)),
        ("latex.insert.citation", json!({ "key": "knuth" })),
        ("latex.fix", json!(null)),
        ("latex.enter", json!(null)),
    ];
    let i = rng.below(all.len());
    all[i].clone()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let template = std::path::PathBuf::from(&args[1]);
    let out = std::path::PathBuf::from(&args[2]);
    let variants: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(50);
    let seed: u64 = args.get(4).and_then(|a| a.parse().ok()).unwrap_or(1);
    let count: usize = args.get(5).and_then(|a| a.parse().ok()).unwrap_or(4);
    std::fs::create_dir_all(&out).expect("out");
    let base = kalem_core::settings::Config::default().parse_base();
    let reg = kalem_core::CommandRegistry::with_builtins();
    let config = kalem_core::Config::default();
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    for v in 0..variants {
        let path = out.join(format!("edit-{seed}-{v}.tex"));
        std::fs::copy(&template, &path).expect("copy");
        let mut doc =
            kalem_core::DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base)
                .expect("open");
        let mut log = String::new();
        for _ in 0..count {
            let text = doc.text().as_str().to_string();
            let body = text.find("\\begin{document}").map_or(0, |i| i + 16);
            let end = text.rfind("\\end{document}").unwrap_or(text.len());
            let snap = |mut p: usize| {
                while !text.is_char_boundary(p) {
                    p -= 1;
                }
                p
            };
            let (name, a) = commands(&mut rng);
            let mut head = snap(body + rng.below(end - body));
            // A selection (within a few lines) for formatting; Enter and
            // Tab at a line's end, where they make items and nest them (in
            // a word they only type a line break or blanks, as asked).
            let anchor = if name.starts_with("latex.format.") && rng.below(2) == 0 {
                snap((head + rng.below(80)).min(end))
            } else {
                if matches!(
                    name,
                    "latex.enter" | "latex.list.indent" | "latex.list.outdent"
                ) {
                    head = text[head..].find('\n').map_or(text.len(), |i| head + i);
                }
                head
            };
            doc.move_cursor(anchor, false);
            doc.move_cursor(head, true);
            let mut clip = kalem_core::command::Clipboard::default();
            let mut ctx = kalem_core::command::EditorContext {
                document: Some(&mut doc),
                clipboard: &mut clip,
                config: &config,
                now: std::time::Instant::now(),
                clock: jiff::civil::date(2026, 10, 2).at(10, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            let r = reg.execute(name, &mut ctx, &a);
            // After Enter, a word typed on the new line, as one does.
            // (A row, ended, in an environment of rows.)
            if name == "latex.enter" && r.is_ok() {
                let t = doc.text().as_str();
                let at = doc.selection.head;
                let prev = t[..at].trim_end_matches([' ', '\t']).trim_end_matches('\n');
                let row = prev.trim_end().ends_with("\\\\");
                // Not before a table's rule (a row would need its end).
                let next = t[at..].trim_start();
                let rule = ["\\toprule", "\\midrule", "\\bottomrule", "\\hline"]
                    .iter()
                    .any(|r| next.starts_with(r));
                if row {
                    doc.insert_text("x \\\\", std::time::Instant::now());
                } else if !rule {
                    doc.insert_text("x", std::time::Instant::now());
                }
            }
            let line = |p: usize| text[..p].matches('\n').count() + 1;
            let _ = writeln!(
                log,
                "{name} {a} at {}..{} (lines {}-{}): {}",
                anchor.min(head),
                anchor.max(head),
                line(anchor.min(head)),
                line(anchor.max(head)),
                if r.is_ok() { "done" } else { "refused" }
            );
        }
        std::fs::write(&path, doc.text().as_str()).expect("write");
        std::fs::write(path.with_extension("log.txt"), log).expect("log");
    }
    println!("{variants} variants in {}", out.display());
}
