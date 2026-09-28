//! Robustness: parsing never panics and always round-trips, on the corpus
//! and on mutated variants of it.
//!
//! Run with `KALEM_MUTATIONS=N` to change the number of mutations per file
//! (default 60).

use std::path::{Path, PathBuf};

/// A small deterministic PRNG so failures are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

const SNIPPETS: &[&str] = &[
    "*",
    "* ",
    "** ",
    "\n",
    "\n\n",
    "#+",
    "#+BEGIN_SRC",
    "#+END_SRC",
    "#+begin_quote\n",
    "#+end_quote\n",
    "[[",
    "]]",
    "][",
    "|",
    "|-",
    "- ",
    "1. ",
    "+ ",
    ":",
    "::",
    "<",
    ">",
    "[",
    "]",
    "=",
    "~",
    "/",
    "+",
    "_",
    "^",
    "{",
    "}",
    "\\",
    "$",
    "$$",
    "@@",
    "[fn:",
    "[fn::",
    "<<",
    ">>",
    "<<<",
    ">>>",
    "\t",
    " ",
    ":END:\n",
    ":PROPERTIES:\n",
    "#+TBLFM: ",
    "src_",
    "call_",
    "[cite:@",
    ";",
    "\\begin{x}",
    "\\end{x}",
    "SCHEDULED: <2026-01-01 Thu>",
    "CLOCK: [2026-01-01 Thu 10:00]",
    "[X] ",
    "[@3] ",
    "#+NAME: x\n",
    "#+CAPTION: c\n",
    "%%(",
    "-----",
    ": ",
    "# ",
    "{{{m(a)}}}",
    "\\\\",
    "ç",
    "ı",
    "中",
    "é",
    "\u{a0}",
    "\r\n",
];

fn mutate(src: &str, rng: &mut Rng) -> String {
    let mut s = src.to_string();
    for _ in 0..(1 + rng.below(4)) {
        if s.is_empty() {
            s.push_str(SNIPPETS[rng.below(SNIPPETS.len())]);
            continue;
        }
        let a = floor_char(&s, rng.below(s.len()));
        match rng.below(3) {
            0 => s.insert_str(a, SNIPPETS[rng.below(SNIPPETS.len())]),
            1 => {
                let b = floor_char(&s, (a + rng.below(40)).min(s.len()));
                s.replace_range(a..b, "");
            }
            _ => {
                let b = floor_char(&s, (a + rng.below(200)).min(s.len()));
                let piece = s[a..b].to_string();
                let c = floor_char(&s, rng.below(s.len()));
                s.insert_str(c, &piece);
            }
        }
    }
    s
}

fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).expect("corpus directory").flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "org") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn check(text: &str, what: &str) {
    let parse = org_syntax::parse(text);
    assert_eq!(
        parse.syntax().to_string(),
        text,
        "round-trip failed for {what}"
    );
}

#[test]
fn corpus_roundtrips() {
    let files = corpus();
    assert!(!files.is_empty());
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        check(&text, &f.display().to_string());
    }
}

#[test]
fn mutations_never_panic_and_roundtrip() {
    let n: usize = std::env::var("KALEM_MUTATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for f in corpus() {
        let text = std::fs::read_to_string(&f).unwrap();
        // Large files: mutate a window to keep the test fast.
        let window = if text.len() > 20_000 {
            let a = floor_char(&text, rng.below(text.len() - 10_000));
            let b = floor_char(&text, a + 10_000);
            text[a..b].to_string()
        } else {
            text.clone()
        };
        for i in 0..n {
            let m = mutate(&window, &mut rng);
            let result = std::panic::catch_unwind(|| check(&m, "mutation"));
            if result.is_err() {
                let out = std::env::temp_dir().join(format!(
                    "kalem-mutation-{}-{i}.org",
                    f.file_stem().unwrap().to_string_lossy()
                ));
                std::fs::write(&out, &m).unwrap();
                panic!(
                    "mutation {i} of {} failed; input saved to {}",
                    f.display(),
                    out.display()
                );
            }
        }
    }
}
