//! The renderer against KaTeX (T2.7h.34): every formula of
//! `tests/corpus/math/katex-corpus.txt`, a thousand KaTeX renders taken from two openly
//! licensed books, is read by the renderer too, prepared as Kalem's view
//! prepares it; and a sample of them renders to an image.

use org_math::{MathEngine, Ratex, Request, source};

/// The formulas of the corpus: where each comes from, whether it is
/// displayed, its text.
fn corpus() -> Vec<(String, bool, String)> {
    let text = include_str!("../../../tests/corpus/math/katex-corpus.txt");
    let mut out: Vec<(String, bool, String)> = Vec::new();
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("%% ") {
            out.push((head.to_string(), head.ends_with(" display"), String::new()));
        } else if line.starts_with('%') && out.is_empty() {
            continue;
        } else if let Some(last) = out.last_mut() {
            if !last.2.is_empty() {
                last.2.push('\n');
            }
            last.2.push_str(line);
        }
    }
    out
}

#[test]
fn the_renderer_reads_what_katex_renders() {
    let formulas = corpus();
    assert_eq!(formulas.len(), 1000);
    let failed: Vec<String> = formulas
        .iter()
        .filter_map(|(head, _, tex)| {
            org_math::check(&source::prepare(tex, ""))
                .err()
                .map(|e| format!("{head}: {tex:?}: {}", e.message))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn a_sample_renders() {
    for (head, display, tex) in corpus().iter().step_by(50) {
        let img = Ratex.render(&Request {
            latex: source::prepare(tex, ""),
            display: *display,
            size: 16.,
            scale: 1.,
            color: [0, 0, 0, 255],
        });
        let img = img.unwrap_or_else(|e| panic!("{head}: {tex:?}: {e:?}"));
        assert!(img.width > 0 && img.height > 0, "{head}");
    }
}
