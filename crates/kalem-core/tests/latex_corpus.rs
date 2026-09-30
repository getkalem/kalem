//! The synthetic LaTeX corpus (tests/corpus/latex/synthetic: nesting,
//! unbalanced input, CRLF, a byte order mark, Turkish and CJK text, huge
//! equations, catcode tricks, empty files): each file parses back to its
//! bytes, every line is drawn, and random edits reparse incrementally as
//! a full parse does. The real corpus is run by the `latex_corpus`
//! example (tests/corpus/fetch-latex.sh).

use std::sync::Arc;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        if n == 0 {
            0
        } else {
            (self.0 % n as u64) as usize
        }
    }
}

fn floor(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

const PIECES: &[&str] = &[
    "\\",
    "{",
    "}",
    "$",
    "%",
    "\n",
    "&",
    "\\\\",
    "é",
    "日",
    "\\begin{itemize}",
    "\\end{itemize}",
    "\\verb|",
    "|",
    "\\iffalse",
    "\\fi",
    "\\makeatletter",
    "\\begin{verbatim}",
    "\\end{verbatim}",
    "\r\n",
    "\\[",
    "\\]",
];

#[test]
fn synthetic_corpus() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/latex/synthetic");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    assert!(files.len() >= 10);
    let base = kalem_core::settings::Config::default().parse_base();
    for (n, path) in files.iter().enumerate() {
        let bytes = std::fs::read(path).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let p = latex_syntax::parse(&text);
        assert_eq!(p.syntax().to_string(), text, "{}", path.display());
        let d =
            kalem_core::DocumentState::open(path, Arc::new(org_model::Settings::default()), &base)
                .unwrap();
        let t = d.text();
        for l in 0..t.line_count() {
            let mut r = t.line_range(l);
            let s = &t.as_str()[r.clone()];
            r.end -= s.len() - s.trim_end_matches(['\n', '\r']).len();
            kalem_core::latex_view::line_view(&d, r.clone(), None);
            kalem_core::latex_view::line_view(&d, r.clone(), Some(r.start));
        }
        kalem_core::latex_view::blocks(&d);
        kalem_core::latex_check::check(path, &text);
        // Random edits: the incremental parse is the full parse.
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ n as u64);
        let mut text = text;
        let mut parse = p;
        for step in 0..60 {
            let at = floor(&text, rng.below(text.len() + 1));
            let end = if rng.below(3) == 0 {
                floor(&text, at + rng.below(8))
            } else {
                at
            };
            let edit = latex_syntax::TextEdit {
                range: at..end,
                insert: PIECES[rng.below(PIECES.len())].to_string(),
            };
            let new = edit.apply(&text);
            let full = latex_syntax::parse(&new);
            if let Some(inc) = parse.reparse_incremental(&new, &edit) {
                assert_eq!(
                    format!("{:#?}", inc.syntax()),
                    format!("{:#?}", full.syntax()),
                    "{} step {step}: {edit:?}",
                    path.display()
                );
            }
            assert_eq!(full.syntax().to_string(), new);
            text = new;
            parse = full;
        }
    }
}
