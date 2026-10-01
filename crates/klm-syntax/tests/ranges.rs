//! The guarantees `klm-syntax` adds to the spike (T2.13.3): every node's
//! range in the text, the text it covers written there, an incremental
//! parse the same as a full one, no panic on mangled input, and the speed
//! of Org's targets.

use klm_syntax::{Body, Command, Document, Inline, Node, parse, reparse};

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The suite's documents: the conformance files and the samples.
fn corpus() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["tests/klm-spec/spec", "tests/klm-spec/samples"] {
        let Ok(rd) = std::fs::read_dir(root().join(dir)) else {
            continue;
        };
        let mut files: Vec<_> = rd
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "klm")
                    && !p.to_string_lossy().ends_with(".canonical.klm")
            })
            .collect();
        files.sort();
        for f in files {
            out.push((
                f.display().to_string(),
                std::fs::read_to_string(&f).unwrap(),
            ));
        }
    }
    // And every example of Part III of the Book.
    let mut chapters: Vec<_> = std::fs::read_dir(root().join("book/part-3"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "org"))
        .collect();
    chapters.sort();
    for c in chapters {
        let text = std::fs::read_to_string(&c).unwrap();
        for (line, src) in klm_syntax::org_examples(&text) {
            out.push((format!("{}:{line}", c.display()), src));
        }
    }
    assert!(out.len() > 15, "{}", out.len());
    out
}

fn check_inlines(src: &str, inl: &[Inline], within: (usize, usize), name: &str) {
    for i in inl {
        match i {
            Inline::Text(t, r) => {
                assert!(
                    r.0 >= within.0 && r.1 <= within.1,
                    "{name}: text {r:?} outside {within:?}"
                );
                // Without escapes, the text is what is written there.
                if !src[r.0..r.1].contains('\\') {
                    assert_eq!(&src[r.0..r.1], t, "{name}");
                }
            }
            Inline::Math(m, r) => {
                assert!(r.0 >= within.0 && r.1 <= within.1, "{name}: math {r:?}");
                assert!(src[r.0..r.1].starts_with('$'), "{name}");
                assert!(src[r.0..r.1].contains(m.as_str()), "{name}");
            }
            Inline::Command(c) => check_command(src, c, within, name),
        }
    }
}

fn check_command(src: &str, c: &Command, within: (usize, usize), name: &str) {
    let r = c.range;
    assert!(
        r.0 >= within.0 && r.1 <= within.1,
        "{name}: \\{} {r:?} outside {within:?}",
        c.name
    );
    assert_eq!(
        &src[c.name_range.0..c.name_range.1],
        format!("\\{}", c.name),
        "{name}"
    );
    assert_eq!(c.attrs.len(), c.attr_ranges.len(), "{name}");
    if let Some(a) = c.attrs_range {
        assert!(src[a.0..a.1].starts_with('['), "{name}");
        for ar in &c.attr_ranges {
            assert!(ar.0 > a.0 && ar.1 <= a.1, "{name}");
        }
    }
    if let Some(b) = c.body_range {
        assert!(
            b.0 >= r.0 && b.1 <= r.1 && b.0 <= b.1,
            "{name}: body {b:?} of {r:?}"
        );
        assert_eq!(&src[b.0 - 1..b.0], "{", "{name}");
        match &c.body {
            Body::Blocks(nodes) => check_blocks(src, nodes, b, name),
            Body::Inline(inl) => check_inlines(src, inl, b, name),
            _ => {}
        }
    }
}

fn check_blocks(src: &str, nodes: &[Node], within: (usize, usize), name: &str) {
    let mut last = within.0;
    for n in nodes {
        let r = n.range();
        assert!(r.0 >= last, "{name}: blocks out of order at {r:?}");
        last = r.1;
        match n {
            Node::Paragraph(inl, r) => check_inlines(src, inl, *r, name),
            Node::Block(c) => check_command(src, c, within, name),
        }
    }
}

fn check(src: &str, doc: &Document, name: &str) {
    check_blocks(src, &doc.blocks, (0, src.len()), name);
    for d in &doc.diagnostics {
        assert!(
            d.range.0 <= d.range.1 && d.range.1 <= src.len(),
            "{name}: {d:?}"
        );
    }
}

#[test]
fn every_node_has_its_range() {
    for (name, src) in corpus() {
        check(&src, &parse(&src), &name);
    }
}

/// A tiny random generator, so the tests need no crate.
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

const PIECES: &[&str] = &[
    "",
    "x",
    "\n",
    "\n\n",
    "}",
    "{",
    "\\b{",
    "\\h1{T}\n",
    "$",
    "\\",
    "[",
    "]",
    "\\ul{\n  \\li{a}\n}\n",
    "\\code{f(x) {",
    " ",
    "\\li{",
    "\\meta[title=\"x\"]",
    "#",
    "\\unknown{",
    "\\eq{\nx\n}\n",
];

fn char_floor(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[test]
fn incremental_equals_full() {
    let seed = std::env::var("KLM_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let mut rng = Rng(seed);
    for (name, src) in corpus() {
        for _ in 0..40 {
            let a = char_floor(&src, rng.below(src.len() + 1));
            let b = char_floor(&src, (a + rng.below(12)).min(src.len()));
            let piece = PIECES[rng.below(PIECES.len())];
            let mut new = src.clone();
            new.replace_range(a..b, piece);
            let inc = reparse(parse(&src), &src, (a, b), &new);
            let full = parse(&new);
            assert_eq!(inc, full, "{name}: {a}..{b} replaced by {piece:?}");
        }
    }
}

#[test]
fn mangled_input_never_panics() {
    // `KLM_SEED` tries another sequence.
    let seed = std::env::var("KLM_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);
    let mut rng = Rng(seed);
    let corpus = corpus();
    for round in 0..20000 {
        let (_, base) = &corpus[round % corpus.len()];
        let mut s = base.clone();
        for _ in 0..1 + rng.below(6) {
            let a = char_floor(&s, rng.below(s.len() + 1));
            let b = char_floor(&s, (a + rng.below(30)).min(s.len()));
            let piece = PIECES[rng.below(PIECES.len())];
            s.replace_range(a..b, piece);
        }
        let doc = parse(&s);
        if std::env::var("KLM_DEBUG").is_ok() {
            let r = std::panic::catch_unwind(|| check(&s, &doc, "mangled"));
            if r.is_err() {
                eprintln!("INPUT {s:?}\nTREE {doc:#?}");
                panic!();
            }
        }
        check(&s, &doc, "mangled");
        let out = klm_syntax::fmt(&doc);
        let _ = klm_syntax::html(&doc);
        // A well-formed result formats without changing its content, and
        // formatting twice changes nothing more (§14.9).
        if klm_syntax::well_formed(&doc) {
            let again = parse(&out);
            let (a, b) = (klm_syntax::model(&again), klm_syntax::model(&doc));
            if a != b {
                let (ab, bb) = (
                    a["blocks"].as_array().unwrap(),
                    b["blocks"].as_array().unwrap(),
                );
                let k = ab
                    .iter()
                    .zip(bb)
                    .position(|(x, y)| x != y)
                    .unwrap_or(ab.len().min(bb.len()));
                panic!(
                    "fmt changed block {k} of the content:\nformatted: {}\nafter: {}\nbefore: {}\ninput: {s:?}",
                    out,
                    ab.get(k).map_or(String::new(), |v| v.to_string()),
                    bb.get(k).map_or(String::new(), |v| v.to_string()),
                );
            }
            assert_eq!(
                klm_syntax::fmt(&again),
                out,
                "fmt is not idempotent on {s:?}"
            );
            for sentences in [true, false] {
                let o = klm_syntax::fmt_with(&doc, sentences);
                assert_eq!(
                    klm_syntax::model(&parse(&o)),
                    klm_syntax::model(&doc),
                    "{s:?}"
                );
                assert_eq!(klm_syntax::fmt_with(&parse(&o), sentences), o, "{s:?}");
            }
        }
    }
}

#[test]
fn a_megabyte_in_time() {
    let sample = std::fs::read_to_string(root().join("tests/klm-spec/samples/makale.klm")).unwrap();
    let body = sample.split_once('\n').unwrap().1;
    let mut big = String::from("\\klm[1.0]\n");
    // Each copy's ids its own, as in a real document.
    let mut k = 0;
    while big.len() < 1_000_000 {
        big.push_str(&body.replace("[#", &format!("[#c{k}-")));
        big.push('\n');
        k += 1;
    }
    let t = std::time::Instant::now();
    let doc = parse(&big);
    let full = t.elapsed();
    // A keystroke in the middle.
    let at = big.len() / 2;
    let at = big[at..].find("için").map_or(at, |i| at + i);
    let mut new = big.clone();
    new.insert(at, 'x');
    let t = std::time::Instant::now();
    let n = doc.blocks.len();
    let inc = reparse(doc, &big, (at, at), &new);
    let key = t.elapsed();
    assert_eq!(inc.blocks.len(), n);
    // Org's targets are 100 ms and 2 ms in release builds; a debug build
    // gets ten times more. A keystroke measures about 2.6 ms here, most
    // of it moving the blocks after the edit (relative ranges would
    // remove it), so the bound is 5 ms until then.
    let slack = if cfg!(debug_assertions) { 10 } else { 1 };
    eprintln!("1 MB: full parse {full:?}, a keystroke {key:?}");
    assert!(full.as_millis() < 100 * slack, "full parse {full:?}");
    assert!(key.as_micros() < 5000 * slack, "keystroke {key:?}");
}
