//! The text (not the markup shown dimmed) the rendered view shows for each line after `\clearpage` in a
//! LaTeX file, one per line (a line break shown as `\n`), for
//! `tools/latex-typeset-fuzz.py` to compare with what pdflatex typesets.
//!
//! `cargo run --release -p kalem-core --example latex_shown -- FILE...`
//! prints `FILE<TAB>LINE<TAB>TEXT`.

#![allow(clippy::print_stdout)]

use std::sync::Arc;

fn main() {
    let base = kalem_core::settings::Config::default().parse_base();
    for f in std::env::args().skip(1) {
        let path = std::path::PathBuf::from(&f);
        let Ok(d) =
            kalem_core::DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base)
        else {
            continue;
        };
        let text = d.text().as_str().to_string();
        let Some(clear) = text.find("\\clearpage") else {
            continue;
        };
        let first = text[..clear].lines().count() + 1;
        for line in first..d.text().line_count() {
            let r = d.text().line_range(line);
            let src = &text[r.clone()];
            if src.contains("\\end{document}") {
                break;
            }
            let r = r.start..r.end - usize::from(src.ends_with('\n'));
            let v = kalem_core::latex_view::line_view(&d, r, None);
            // The text, not the markup shown dimmed beside it.
            let shown: String = v
                .runs
                .iter()
                .filter(|r| !r.style.dim)
                .map(|r| r.text.as_str())
                .collect();
            println!("{f}\t{line}\t{}", shown.replace('\n', "\\n"));
        }
    }
}
