//! Incremental reparsing gives exactly the tree a full parse gives.
//!
//! Random edit sequences are applied to corpus files. After every edit the
//! incrementally updated tree must equal a fresh parse of the new text.
//! `KALEM_EDITS=N` sets the number of edits per file (default 80) and
//! `KALEM_SEED=N` the random seed. The CRLF test inserts lone carriage
//! returns and bare line feeds too; `KALEM_CRLF_TYPING=1` limits it to what
//! an editor that keeps CRLF line endings would insert.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::needless_range_loop
)]

use std::path::{Path, PathBuf};

use org_syntax::{ReparseLevel, TextEdit, TextRange, TextSize};

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
    "a",
    "x",
    " ",
    "\n",
    "\n\n",
    "*",
    "* ",
    "** ",
    "- ",
    "1. ",
    "|",
    "| a |",
    "#+",
    "#+begin_src sh\n",
    "#+end_src\n",
    ":END:\n",
    ":LOGBOOK:\n",
    "[[",
    "]]",
    "=",
    "~",
    "/",
    "_",
    "+",
    "$",
    "\\alpha",
    "[fn:1]",
    "[fn:: x]",
    "<2026-01-01 Thu>",
    "SCHEDULED: ",
    ":PROPERTIES:\n",
    "#+NAME: n\n",
    "  ",
    "\t",
    "word ",
    "[X] ",
    "::",
    "#+TBLFM: $2=$1\n",
    "\\begin{x}\n",
    "\\end{x}\n",
    "#+begin_quote\n",
    "#+end_quote\n",
    "ç",
    "*************** task\n",
    "# comment\n",
    ": fixed\n",
    "-----\n",
    "%%(diary)\n",
];

fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).expect("corpus").flatten() {
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

fn random_edit(text: &str, rng: &mut Rng) -> TextEdit {
    let a = floor_char(text, rng.below(text.len() + 1));
    let (b, insert) = match rng.below(4) {
        0 => (a, SNIPPETS[rng.below(SNIPPETS.len())].to_string()),
        1 => (
            floor_char(text, (a + 1 + rng.below(30)).min(text.len())),
            String::new(),
        ),
        2 => (
            floor_char(text, (a + rng.below(10)).min(text.len())),
            SNIPPETS[rng.below(SNIPPETS.len())].to_string(),
        ),
        _ => (
            a,
            SNIPPETS[rng.below(SNIPPETS.len())]
                .chars()
                .next()
                .map(String::from)
                .unwrap_or_default(),
        ),
    };
    let b = b.max(a);
    TextEdit {
        range: TextRange::new(TextSize::from(a as u32), TextSize::from(b as u32)),
        insert,
    }
}

#[test]
fn incremental_matches_full_parse() {
    let n: usize = std::env::var("KALEM_EDITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);
    let seed: u64 = std::env::var("KALEM_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0xDEAD_BEEF_1234_5678);
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut counts = [0usize; 3];
    for f in corpus() {
        let full = std::fs::read_to_string(&f).unwrap();
        // Keep documents small enough for a fast test; big files get a
        // window.
        let mut text = if full.len() > 30_000 {
            let a = floor_char(&full, rng.below(full.len() - 30_000));
            full[a..floor_char(&full, a + 30_000)].to_string()
        } else {
            full
        };
        let mut parse = org_syntax::parse(&text);
        for i in 0..n {
            let edit = random_edit(&text, &mut rng);
            let new_text = edit.apply(&text);
            let (inc, level) = parse.reparse_with_level(&new_text, &edit);
            counts[match level {
                ReparseLevel::Elements => 0,
                ReparseLevel::Section => 1,
                ReparseLevel::Document => 2,
            }] += 1;
            let fresh = org_syntax::parse(&new_text);
            if inc.green() != fresh.green() {
                let out = std::env::temp_dir().join("kalem-incremental-failure.org");
                std::fs::write(&out, &text).unwrap();
                panic!(
                    "{} edit {i} ({:?}, level {level:?}) differs from a full parse; old text saved to {}",
                    f.display(),
                    edit,
                    out.display()
                );
            }
            text = new_text;
            parse = inc;
        }
    }
    eprintln!(
        "elements: {} section: {} document: {}",
        counts[0], counts[1], counts[2]
    );
    assert!(
        counts[0] > counts[2],
        "most edits should be incremental: {counts:?}"
    );
}

/// The same check on documents with CRLF line endings (some with a byte
/// order mark), with edits that insert CRLF, LF and lone carriage returns.
#[test]
fn incremental_matches_full_parse_with_crlf() {
    let n: usize = std::env::var("KALEM_EDITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);
    let seed: u64 = std::env::var("KALEM_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x0123_4567_89AB_CDEF);
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut counts = [0usize; 3];
    for (k, f) in corpus().into_iter().enumerate() {
        let full = std::fs::read_to_string(&f).unwrap();
        let full = if full.len() > 20_000 {
            let a = floor_char(&full, rng.below(full.len() - 20_000));
            full[a..floor_char(&full, a + 20_000)].to_string()
        } else {
            full
        };
        let mut text = full.replace("\r\n", "\n").replace('\n', "\r\n");
        if k % 3 == 0 {
            text.insert(0, '\u{feff}');
        }
        let mut parse = org_syntax::parse(&text);
        for i in 0..n {
            let mut edit = random_edit(&text, &mut rng);
            if !edit.insert.is_empty() {
                edit.insert = if std::env::var("KALEM_CRLF_TYPING").is_ok() {
                    edit.insert.replace('\n', "\r\n")
                } else {
                    match rng.below(6) {
                        0 => edit.insert.clone(),
                        1 => "\r".to_string(),
                        2 => "\r\n".to_string(),
                        _ => edit.insert.replace('\n', "\r\n"),
                    }
                };
            }
            let new_text = edit.apply(&text);
            let (inc, level) = parse.reparse_with_level(&new_text, &edit);
            counts[match level {
                ReparseLevel::Elements => 0,
                ReparseLevel::Section => 1,
                ReparseLevel::Document => 2,
            }] += 1;
            let fresh = org_syntax::parse(&new_text);
            if inc.green() != fresh.green() {
                let out = std::env::temp_dir().join("kalem-incremental-crlf-failure.org");
                std::fs::write(&out, &text).unwrap();
                panic!(
                    "{} edit {i} ({:?}, level {level:?}) differs from a full parse; old text saved to {}",
                    f.display(),
                    edit,
                    out.display()
                );
            }
            text = new_text;
            parse = inc;
        }
    }
    eprintln!(
        "CRLF: elements: {} section: {} document: {}",
        counts[0], counts[1], counts[2]
    );
    assert!(
        counts[0] > counts[2],
        "most CRLF edits should be incremental: {counts:?}"
    );
}

#[test]
fn typing_at_the_start_of_a_section_is_incremental() {
    // The token before an insertion there is the headline's line feed; the
    // edit belongs to the section after it.
    for (text, at) in [
        ("* Heading\nSome *bold* text\n- [ ] task\n", 10),
        ("* A\n** B\nbody\n", 9),
        ("Top.\n* A\nx\n", 0),
    ] {
        let parse = org_syntax::parse(text);
        let edit = TextEdit {
            range: TextRange::empty(TextSize::from(at as u32)),
            insert: "N".into(),
        };
        let new_text = edit.apply(text);
        let (inc, level) = parse.reparse_with_level(&new_text, &edit);
        assert_ne!(level, ReparseLevel::Document, "{text:?} at {at}");
        let full = org_syntax::parse(&new_text);
        assert_eq!(
            format!("{:#?}", inc.syntax()),
            format!("{:#?}", full.syntax())
        );
    }
}
