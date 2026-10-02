//! The numbers the model gives the labels of LaTeX files, as pdflatex
//! writes them to the `.aux` file: `FILE key number` per line, for
//! `tools/latex-numbering-fuzz.py`.
//!
//! `cargo run --release -p latex-model --example labels -- FILE...`

#![allow(clippy::print_stdout)]

fn main() {
    for f in std::env::args().skip(1) {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let model = latex_model::Model::new(&latex_syntax::parse(&text));
        for l in &model.labels {
            println!("{f} {} {}", l.name, l.number.clone().unwrap_or_default());
        }
    }
}
