//! Every formula of LaTeX files as the view reads it, without
//! delimiters, with the math renderer's verdict on it alone (prepared,
//! without the document's macros) and with them, one JSON object a line:
//! `{"file", "display", "tex", "ratex", "with_macros"}`, the verdicts
//! being `ok` or the renderer's message. For the math corpus (T2.7h.34):
//! `tools/math-corpus.py` sets KaTeX's verdict beside it.
//!
//! `cargo run --release -p kalem-core --example formulas -- FILE...`

#![allow(clippy::print_stdout)]

use std::sync::Arc;

fn main() {
    let base = kalem_core::settings::Config::default().parse_base();
    for f in std::env::args().skip(1) {
        let path = std::path::PathBuf::from(&f);
        let Ok(mut d) =
            kalem_core::DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base)
        else {
            continue;
        };
        d.wait_for_latex_project();
        let macros = org_math::source::macros(&kalem_core::latex_view::math_definitions(&d));
        let verdict = |tex: &str| match org_math::check(tex) {
            Ok(()) => "ok".to_string(),
            Err(e) => e.message.lines().next().unwrap_or("").to_string(),
        };
        for (_, tex, display) in kalem_core::latex_view::formulas(&d) {
            let ratex = verdict(&org_math::source::prepare(&tex, ""));
            let with_macros = verdict(&org_math::source::prepare(&tex, &macros));
            println!(
                "{}",
                serde_json::json!({ "file": f, "display": display, "tex": tex, "ratex": ratex, "with_macros": with_macros })
            );
        }
    }
}
