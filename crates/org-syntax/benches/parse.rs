//! Parser benchmarks, matching the performance targets in section 15 of
//! the design document.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use org_syntax::{ParseContext, TextEdit, TextRange, TextSize};

fn corpus(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus")
        .join(name);
    std::fs::read_to_string(p).expect("corpus file")
}

fn full(c: &mut Criterion) {
    let manual = corpus("org-mode/org-manual.org");
    let news = corpus("org-mode/ORG-NEWS.org");
    // About 10 MB: the manual repeated.
    let big = manual.repeat(12);
    let mut g = c.benchmark_group("full_parse");
    g.sample_size(10);
    for (name, text) in [("org-manual", &manual), ("ORG-NEWS", &news), ("10MB", &big)] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(name), text, |b, t| {
            b.iter(|| org_syntax::parse(black_box(t)));
        });
    }
    g.finish();
}

fn incremental(c: &mut Criterion) {
    let text = corpus("org-mode/org-manual.org");
    let ctx = ParseContext::for_document(&text, &ParseContext::default());
    let parse = org_syntax::parse_with(&text, &ctx);
    // A position inside a paragraph in the middle of the manual.
    let mut pos = text.len() / 2;
    while !(text.as_bytes()[pos] == b' ' && text.as_bytes()[pos - 1].is_ascii_alphabetic()) {
        pos += 1;
    }
    let edit = TextEdit {
        range: TextRange::empty(TextSize::from(pos as u32)),
        insert: "x".into(),
    };
    let new_text = edit.apply(&text);
    c.bench_function("incremental/keystroke-in-paragraph", |b| {
        b.iter(|| parse.reparse(black_box(&new_text), black_box(&edit)));
    });
}

criterion_group!(benches, full, incremental);
criterion_main!(benches);
