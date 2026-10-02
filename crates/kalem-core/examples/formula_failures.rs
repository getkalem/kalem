//! The formulas of LaTeX files the math renderer cannot read, with the
//! renderer's message: `FILE<TAB>MESSAGE<TAB>FORMULA` per line.
//!
//! `cargo run --release -p kalem-core --example formula_failures -- FILE...`

#![allow(clippy::print_stdout)]

use std::sync::Arc;

fn main() {
    let base = kalem_core::settings::Config::default().parse_base();
    let macros_too = std::env::var_os("FULL").is_some();
    for f in std::env::args().skip(1) {
        let path = std::path::PathBuf::from(&f);
        let Ok(d) =
            kalem_core::DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base)
        else {
            continue;
        };
        let text = d.text().as_str().to_string();
        for (r, _) in kalem_core::latex_view::formula_failures(&d) {
            let src = kalem_core::latex_view::math_source(&d, r.clone())
                .unwrap_or_else(|| text[r.clone()].to_string());
            let (inner, _) = org_math::source::body(&src);
            let macros = if macros_too {
                org_math::source::macros(&kalem_core::latex_view::math_definitions(&d))
            } else {
                String::new()
            };
            let msg = org_math::check(&org_math::source::prepare(inner, &macros))
                .err()
                .map(|e| e.message.replace('\n', " "))
                .unwrap_or_else(|| "(only with the document's macros)".into());
            let shown: String = text[r].replace('\n', " ").chars().take(160).collect();
            println!("{f}\t{msg}\t{shown}");
        }
    }
}
