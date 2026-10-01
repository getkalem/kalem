//! A model after an edit is the model of the new text (T2.7h.35): when
//! the edit only moves the events, the last model is moved instead of
//! numbering again, and either way the result must equal a model built
//! from scratch, for one document and for a file of a project.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use latex_model::project::{Files, ProjectCache};
use latex_syntax::TextEdit;

const DOC: &str = "\\documentclass{book}\n\\begin{document}\n\\chapter{One}\\label{ch:one}\nText \\ref{eq:a} and \\cite{k}.\n\n\\section{Intro}\\label{sec:intro}\nSome words here.\n\\begin{equation}\\label{eq:a}\nx = 1\n\\end{equation}\nMore text\\footnote{A note.} after.\n\n\\begin{figure}\\caption{Fig}\\label{fig:a}\\end{figure}\n\\section{Next}\n\\begin{align}\na &= b \\\\\nc &= d \\label{eq:b}\n\\end{align}\n\\input{part}\n\\section{Last}\nEnd.\n\\end{document}\n";

const PART: &str = "\\section{Part}\\label{sec:part}\nWords \\ref{sec:intro}.\n\\begin{equation}y\\end{equation}\n";

/// A small generator of numbers (xorshift).
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

/// A random edit of `text`: mostly typing, sometimes a deletion or a
/// piece of markup.
fn edit(text: &str, rng: &mut Rng) -> TextEdit {
    let mut at = rng.below(text.len() + 1);
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let pieces = [
        "a",
        "b",
        " ",
        "x",
        "\n",
        "word ",
        "\n\n",
        "\\label{z}",
        "\\section{S}",
        "}",
        "{",
        "\\ref{z}",
        "\\\\",
    ];
    if rng.below(5) == 0 {
        let mut end = (at + 1 + rng.below(6)).min(text.len());
        while !text.is_char_boundary(end) {
            end += 1;
        }
        return TextEdit {
            range: at..end,
            insert: String::new(),
        };
    }
    let insert = if rng.below(4) == 0 {
        pieces[rng.below(pieces.len())]
    } else {
        pieces[rng.below(4)]
    };
    TextEdit {
        range: at..at,
        insert: insert.to_string(),
    }
}

#[test]
fn a_document_after_edits() {
    for seed in 1..=20u64 {
        let mut rng = Rng(seed * 7919);
        let mut text = DOC.to_string();
        let mut parse = latex_syntax::parse(&text);
        let mut cache = latex_model::Cache::default();
        cache.model(&parse);
        for step in 0..60 {
            let e = edit(&text, &mut rng);
            let new = e.apply(&text);
            parse = parse.reparse(&new, &e);
            text = new;
            let got = cache.model(&parse);
            let want = latex_model::Model::new(&latex_syntax::parse(&text));
            assert_eq!(*got, want, "seed {seed}, step {step}, after {e:?}:\n{text}");
        }
    }
}

/// The files of a project in memory, the edited one shared as the editor
/// shares it.
struct Mem(RefCell<HashMap<PathBuf, Arc<str>>>);

impl Files for Mem {
    fn read(&self, path: &Path) -> Option<String> {
        self.0.borrow().get(path).map(|t| t.to_string())
    }

    fn read_shared(&self, path: &Path) -> Option<Arc<str>> {
        self.0.borrow().get(path).cloned()
    }

    fn list(&self, _dir: &Path) -> Vec<PathBuf> {
        self.0.borrow().keys().cloned().collect()
    }
}

#[test]
fn a_file_of_a_project_after_edits() {
    let (root, part) = (PathBuf::from("/p/main.tex"), PathBuf::from("/p/part.tex"));
    for (seed, edited) in (1..=12u64).zip([&root, &part].into_iter().cycle()) {
        let mut rng = Rng(seed * 104729);
        let files = Mem(RefCell::new(HashMap::from([
            (root.clone(), Arc::from(DOC)),
            (part.clone(), Arc::from(PART)),
        ])));
        let mut text = files.read(edited).unwrap();
        let mut parse = latex_syntax::parse(&text);
        let mut own = latex_model::Cache::default();
        let mut cache = ProjectCache::default();
        cache.load(&root, &files);
        for step in 0..40 {
            let e = edit(&text, &mut rng);
            let new = e.apply(&text);
            parse = parse.reparse(&new, &e);
            text = new;
            let shared: Arc<str> = Arc::from(text.as_str());
            files.0.borrow_mut().insert(edited.clone(), shared.clone());
            let mine = own.model(&parse);
            cache.set_parse_from(edited, shared, parse.clone(), &own);
            let got = cache.load(&root, &files);
            let want = ProjectCache::default().load(&root, &files);
            assert_eq!(
                *got.model,
                *want.model,
                "seed {seed}, step {step}, {} after {e:?}:\n{text}",
                edited.display()
            );
            // Seen from the edited file, as the editor shows it.
            let this = want.model.files.iter().position(|f| f == edited).unwrap();
            drop(got);
            let seen = cache
                .seen_from(this, mine.preamble.clone(), mine.body.clone())
                .unwrap();
            let mut expected = want.model.seen_from(this);
            if this != 0 || expected.preamble != mine.preamble || expected.body != mine.body {
                expected.preamble = mine.preamble.clone();
                expected.body = mine.body.clone();
            }
            assert_eq!(*seen, expected, "seen, seed {seed}, step {step}");
        }
    }
}
