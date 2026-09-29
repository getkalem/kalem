//! The model against LaTeX: every `\label` in `tests/latex/model/*.tex`
//! gets the number pdflatex wrote for it in the `.aux` file (kept in
//! `*.labels`, written by `pdflatex` from TeX Live 2023), and the rest of
//! the model on the same documents.

use std::path::PathBuf;

use latex_model::{Cache, Model, Target};
use latex_syntax::{TextEdit, parse};

fn dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/latex/model"
    ))
}

fn check_labels(name: &str) -> Model {
    let text = std::fs::read_to_string(dir().join(format!("{name}.tex"))).unwrap();
    let expected = std::fs::read_to_string(dir().join(format!("{name}.labels"))).unwrap();
    let model = Model::new(&parse(&text));
    let mut wrong = Vec::new();
    for line in expected.lines() {
        let (key, number) = line.split_once(' ').unwrap_or((line, ""));
        let got = model
            .label(key)
            .map(|l| l.number.clone().unwrap_or_default());
        if got.as_deref() != Some(number) {
            wrong.push(format!("{key}: LaTeX {number:?}, model {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(model.labels.len(), expected.lines().count());
    model
}

#[test]
fn article() {
    let m = check_labels("article");
    let class = m.class.as_ref().unwrap();
    assert_eq!(
        (class.name.as_str(), class.options.clone()),
        ("article", vec!["11pt".to_string(), "a4paper".into()])
    );
    let packages: Vec<&str> = m.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(packages, ["amsmath", "amsthm", "inputenc"]);
    assert_eq!(m.packages[2].options, ["utf8"]);
    assert!(m.body.is_some() && m.preamble.end > 0);
    // The tree.
    let titles: Vec<(i8, &str, Option<&str>, Option<usize>)> = m
        .sections
        .iter()
        .map(|s| (s.level, s.title.as_str(), s.number.as_deref(), s.parent))
        .collect();
    assert_eq!(
        titles,
        [
            (1, "Introduction", Some("1"), None),
            (2, "Background", Some("1.1"), Some(0)),
            (3, "Deep", Some("1.1.1"), Some(1)),
            (4, "Para", None, Some(2)),
            (1, "Unnumbered", None, None),
            (1, "Figures", Some("2"), None),
            (2, "Not numbered now", None, Some(5)),
            (1, "Proofs", Some("A"), None),
            (2, "First", None, Some(7)),
        ]
    );
    assert_eq!(m.label("sec:intro").unwrap().target, Target::Section(1));
    assert_eq!(m.label("eq:x").unwrap().target, Target::Equation);
    assert_eq!(
        m.label("fig:a").unwrap().target,
        Target::Float("figure".into())
    );
    assert_eq!(
        m.label("thm:py").unwrap().target,
        Target::Theorem("thm".into())
    );
    assert_eq!(m.label("fn:one").unwrap().target, Target::Footnote);
    // Equations: the empty line after the last `\\` of `align` is
    // numbered, as amsmath numbers it.
    let numbers: Vec<Option<&str>> = m.equations.iter().map(|e| e.number.as_deref()).collect();
    assert_eq!(
        numbers,
        [
            Some("1"),
            Some("2"),
            None,
            Some("star"),
            Some("3"),
            Some("4"),
            None,
            Some("5a"),
            Some("5b"),
            Some("6"),
            Some("7")
        ]
    );
    // Floats, theorems, footnotes.
    let floats: Vec<(&str, Option<&str>, Option<&str>)> = m
        .floats
        .iter()
        .map(|f| {
            (
                f.env.as_str(),
                f.captions[0].number.as_deref(),
                f.captions[0].short.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        floats,
        [
            ("figure", Some("1"), Some("Short")),
            ("table", Some("1"), None),
            ("figure*", Some("2"), None)
        ]
    );
    let theorems: Vec<(&str, Option<&str>, Option<&str>)> = m
        .theorems
        .iter()
        .map(|t| (t.title.as_str(), t.number.as_deref(), t.note.as_deref()))
        .collect();
    assert_eq!(
        theorems,
        [
            ("Theorem", Some("1.1"), Some("Pythagoras")),
            ("Lemma", Some("1.2"), None),
            ("Remark", None, None),
            ("Theorem", Some("2.1"), None)
        ]
    );
    assert_eq!(
        m.footnotes
            .iter()
            .map(|f| f.number.as_str())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
    // Definitions and sources.
    let macros: Vec<(&str, &str, usize)> = m
        .macros
        .iter()
        .map(|d| (d.name.as_str(), d.command.as_str(), d.args))
        .collect();
    assert_eq!(
        macros,
        [
            ("\\R", "newcommand", 0),
            ("\\norm", "newcommand", 1),
            ("\\tr", "DeclareMathOperator", 0),
            ("\\half", "def", 0)
        ]
    );
    assert_eq!(m.macros[1].body, "\\lVert #1 \\rVert");
    assert_eq!(m.environments[0].name, "note");
    assert_eq!(m.environments[0].default.as_deref(), Some("Note"));
    assert_eq!(m.bibliography[0].files, ["refs.bib", "more.bib"]);
    assert_eq!(m.bibliography_style.as_deref(), Some("plain"));
    assert_eq!(m.citations[0].keys, ["knuth", "lamport"]);
    assert_eq!(m.citations[0].notes, ["p.~3"]);
    let refs: Vec<(&str, &str)> = m
        .references
        .iter()
        .map(|r| (r.command.as_str(), r.keys[0].as_str()))
        .collect();
    assert_eq!(refs, [("ref", "sec:intro"), ("eqref", "eq:x")]);
}

#[test]
fn book() {
    let m = check_labels("book");
    let numbers: Vec<Option<&str>> = m.sections.iter().map(|s| s.number.as_deref()).collect();
    assert_eq!(
        numbers,
        [
            None,
            Some("0.1"),
            Some("I"),
            Some("1"),
            Some("1.1"),
            Some("1.1.1"),
            None,
            Some("2"),
            Some("2.1"),
            Some("II"),
            Some("3"),
            Some("A"),
            Some("A.1"),
            None
        ]
    );
}

#[test]
fn cached_across_edits() {
    let text = std::fs::read_to_string(dir().join("article.tex")).unwrap();
    let mut cache = Cache::default();
    let p = parse(&text);
    let first = cache.model(&p);
    assert!(std::sync::Arc::ptr_eq(&first, &cache.model(&p)));
    // A section added before the figures renumbers what follows.
    let at = text.find("\\section{Figures}").unwrap();
    let edit = TextEdit {
        range: at..at,
        insert: "\\section{New}\n".into(),
    };
    let new = edit.apply(&text);
    let p2 = p.reparse(&new, &edit);
    let m = cache.model(&p2);
    assert_eq!(*m, Model::new(&parse(&new)));
    assert_eq!(m.label("sec:fig").unwrap().number.as_deref(), Some("3"));
    assert_eq!(m.label("thm:two").unwrap().number.as_deref(), Some("3.1"));
}
