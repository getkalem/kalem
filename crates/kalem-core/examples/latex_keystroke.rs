//! Where a keystroke's time goes in a large LaTeX file of a project:
//! `cargo run --release -p kalem-core --example latex_keystroke -- FILE`.

#![allow(clippy::print_stdout)]

use std::sync::Arc;
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("a .tex file");
    let base = kalem_core::settings::Config::default().parse_base();
    let mut d = kalem_core::DocumentState::open(
        std::path::Path::new(&path),
        Arc::new(org_model::Settings::default()),
        &base,
    )
    .unwrap();
    d.wait_for_latex_project();
    d.poll();
    let mid = d.text().len() / 2;
    let at = d.text().line_range(d.text().line_of(mid)).start;
    let (mut apply, mut model, mut view) = (0.0, 0.0, 0.0);
    let mut models = Vec::new();
    let n = 20;
    for i in 0..n {
        let t = Instant::now();
        let mut tx = org_edit::Transaction::new("type");
        let _ = tx.insert(at + i, "a");
        d.apply(&tx, org_edit::ChangeKind::Typing, Instant::now());
        apply += t.elapsed().as_secs_f64();
        let t = Instant::now();
        std::hint::black_box(d.latex().unwrap().model());
        model += t.elapsed().as_secs_f64();
        models.push(t.elapsed().as_secs_f64());
        let t = Instant::now();
        let r = d.text().line_range(d.text().line_of(at));
        for l in 0..50 {
            let rr = d.text().line_range(d.text().line_of(r.start) + l);
            std::hint::black_box(kalem_core::latex_view::line_view(
                &d,
                rr.start..rr.end.saturating_sub(1).max(rr.start),
                Some(at + i),
            ));
        }
        view += t.elapsed().as_secs_f64();
    }
    // The document's own model alone, after an edit.
    let text = d.text().as_str().to_string();
    let mut cache = latex_model::Cache::default();
    let mut parse = latex_syntax::parse(&text);
    std::hint::black_box(cache.model(&parse));
    let mut own = 0.0;
    let mut t_text = text.clone();
    for i in 0..n {
        let edit = latex_syntax::TextEdit {
            range: at + i..at + i,
            insert: "a".into(),
        };
        let new = edit.apply(&t_text);
        parse = parse.reparse(&new, &edit);
        t_text = new;
        let t = Instant::now();
        std::hint::black_box(cache.model(&parse));
        own += t.elapsed().as_secs_f64();
    }
    let ms = |x: f64| x / n as f64 * 1000.0;
    println!("own model alone {:.2} ms", ms(own));
    models.sort_by(f64::total_cmp);
    println!("model p50 {:.2} ms", models[models.len() / 2] * 1000.0);
    println!(
        "per keystroke: apply {:.2} ms, model {:.2} ms, 50 lines drawn {:.2} ms",
        ms(apply),
        ms(model),
        ms(view)
    );
}

#[allow(dead_code)]
fn unused() {}
