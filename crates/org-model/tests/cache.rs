//! The model built with a cache across incremental edits equals the model
//! built from scratch.

use std::sync::Arc;

use org_model::{Document, ModelCache, Settings};
use org_syntax::{TextEdit, TextRange, TextSize};

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

const SNIPPETS: &[&str] = &[
    "a",
    " ",
    "\n",
    "* ",
    "** New :t:\n",
    "#+FILETAGS: :x:\n",
    ":PROPERTIES:\n:A: 1\n:END:\n",
    "TODO ",
    "[/]",
    "- [X] ",
    "#+TODO: A | B\n",
    "\n*** Deep\n",
];

fn floor(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[test]
fn cached_model_matches_fresh_model() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let files = [
        "org-mode/org-manual.org",
        "org-mode/ORG-NEWS.org",
        "model/basic.org",
        "model/stats.org",
        "worg/org-faq.org",
    ];
    let settings = Arc::new(Settings::default());
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for f in files {
        let Ok(mut text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        let cache = ModelCache::new();
        let mut parse = org_syntax::parse(&text);
        let first = Document::with_cache(parse.clone(), settings.clone(), None, cache.clone());
        let _ = (first.outline(), first.info());
        for _ in 0..60 {
            let a = floor(&text, rng.below(text.len() + 1));
            let b = floor(&text, (a + rng.below(8)).min(text.len()));
            let insert = SNIPPETS[rng.below(SNIPPETS.len())].to_string();
            let edit = TextEdit {
                range: TextRange::new(TextSize::from(a as u32), TextSize::from(b as u32)),
                insert,
            };
            let new_text = edit.apply(&text);
            parse = parse.reparse(&new_text, &edit);
            text = new_text;
            let cached = Document::with_cache(parse.clone(), settings.clone(), None, cache.clone());
            let fresh = Document::new(parse.clone());
            assert_eq!(
                cached.outline(),
                fresh.outline(),
                "{f}: outline after {edit:?}"
            );
            assert_eq!(
                cached.info().keywords,
                fresh.info().keywords,
                "{f}: keywords after {edit:?}"
            );
        }
    }
}
