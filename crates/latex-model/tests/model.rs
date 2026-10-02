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
    // Every label LaTeX writes, and no other.
    assert_eq!(
        model.labels.len() - model.unwritten_labels.len(),
        expected.lines().count()
    );
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

#[test]
fn addbibresource_keeps_its_extension() {
    let p = latex_syntax::parse(
        "\\addbibresource{refs.json}\n\\addbibresource{more}\n\\bibliography{a,b.bib}\n",
    );
    let m = latex_model::Cache::default().model(&p);
    let files: Vec<&str> = m
        .bibliography
        .iter()
        .flat_map(|b| b.files.iter().map(String::as_str))
        .collect();
    assert_eq!(files, ["refs.json", "more.bib", "a.bib", "b.bib"]);
}

#[test]
fn macro_parameters_are_not_keys() {
    let text = "\\newcommand\\citeproc[2]{\\cite{#1}\\label{#2}\\ref{#2}}\n\
                \\begin{document}\\cite{a,#1}\\end{document}\n";
    let parse = latex_syntax::parse(text);
    let m = latex_model::Model::new(&parse);
    assert_eq!(m.citations.len(), 1);
    assert_eq!(m.citations[0].keys, ["a"]);
    assert!(m.labels.is_empty());
    assert!(m.references.is_empty());
}

#[test]
fn numbering() {
    // Resets cascade (`\@stpelt`); book's `\theequation`, `\thefigure`
    // and `\thetable` leave out chapter 0.
    check_labels("numbering");
}

#[test]
fn classes() {
    // amsbook: sections, figures and tables without the chapter,
    // equations through the book; memoir: sections only.
    check_labels("class-amsbook");
    check_labels("class-memoir");
}

#[test]
fn signatures() {
    // `\newcounter{name}[within]`, `\footnotemark` stepping the footnote
    // counter, and commands whose arguments were read wrong.
    check_labels("signatures");
}

#[test]
fn items_and_counters() {
    // Items of enumerated lists at four levels, `\refstepcounter` and a
    // counter reset by sections; a definition's body does not run where
    // it is defined.
    check_labels("items");
}

#[test]
fn redefined_the() {
    // `\renewcommand{\thesection}{\Roman{section}}` and the like.
    check_labels("the");
}

#[test]
fn subfigures() {
    // subcaption's `subfigure` and subfig's `\subfloat` print the
    // float's number before their letter; `\captionof` steps its type.
    check_labels("subfigures");
    check_labels("subfig");
    check_labels("caption-above");
}

#[test]
fn enumitem_labels() {
    // enumitem's `label=`, `ref=`, `label*=` and `start=`, a level under
    // one it formats; with the caption package, a label before the
    // caption refers to nothing.
    let m = check_labels("enumitem");
    let before: Vec<&str> = m
        .labels_before_caption
        .iter()
        .map(|&i| m.labels[i].name.as_str())
        .collect();
    assert_eq!(before, ["early"]);
}

#[test]
fn minipage_footnotes() {
    // A minipage numbers its footnotes a, b, … and leaves the main
    // counter alone.
    check_labels("minipage");
}

#[test]
fn display_math_tags() {
    // `\tag` in `\[…\]` and `displaymath` numbers them; in `$$…$$` not.
    check_labels("display");
}

#[test]
fn math_environments() {
    // xalignat, breqn's dmath, empheq by its argument, and environments
    // inside equations that number nothing (CD, multlined, rcases).
    check_labels("mathenvs");
}

#[test]
fn thmtools() {
    // `\declaretheorem` with `numberwithin`, `sibling`, `name` and
    // `numbered=no`.
    check_labels("thmtools");
}

#[test]
fn more_citation_commands() {
    // natbib's and biblatex's other citation commands cite their keys.
    let text = "\\citeyearpar{a} \\citenum{b} \\Citeauthor{c} \\citetitle{d} \\fullcite{e} \\supercite{f} \\cites{g} \\parencites{h} \\textcites{i} \\footcitetext{j}\n";
    let m = latex_model::Model::new(&parse(text));
    let keys: Vec<&str> = m
        .citations
        .iter()
        .flat_map(|c| c.keys.iter().map(String::as_str))
        .collect();
    assert_eq!(keys, ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
}

#[test]
fn input_without_braces() {
    let m = latex_model::Model::new(&parse("\\input chapter1 more\n\\input{two}\n"));
    let files: Vec<&str> = m.includes.iter().map(|i| i.target.as_str()).collect();
    assert_eq!(files, ["chapter1", "two"]);
}

#[test]
fn macro_definitions_written_again() {
    let text = "\\newcommand{\\R}{\\mathbb{R}}\n\\newcommand{\\norm}[2][2]{\\lVert #2 \\rVert_{#1}}\n\\def\\half#1{#1/2}\n\\DeclareMathOperator{\\tr}{tr}\n";
    let m = Model::new(&parse(text));
    assert_eq!(
        m.macro_definitions(),
        [
            "\\newcommand{\\R}{\\mathbb{R}}",
            "\\newcommand{\\norm}[2][2]{\\lVert #2 \\rVert_{#1}}",
            "\\def\\half#1{#1/2}",
            "\\DeclareMathOperator{\\tr}{tr}",
        ]
    );
}

/// What the random documents of `tools/latex-numbering-fuzz.py` found
/// against pdflatex: a label on a line without a number (amsmath keeps
/// it for the next numbered line, even of a later environment; `gather`
/// and `equation*` write it with the number outside; `eqnarray` with the
/// number the line would have had), `\Alph` of zero, `\numberwithin`
/// before the first section.
#[test]
fn amsmath_labels() {
    let m = check_labels("amsmath-labels");
    // `\label{lost}` is never written; nothing clashes.
    let lost: Vec<&str> = m
        .unwritten_labels
        .iter()
        .map(|&i| m.labels[i].name.as_str())
        .collect();
    assert_eq!(lost, ["lost"]);
    assert!(m.label_clashes.is_empty());
}

/// Resets that cascade (`\chapter` resets the section, which resets the
/// equation), `\counterwithout` taking one reset away, `\appendix`
/// defining `\thechapter` anew over the document's.
#[test]
fn counters() {
    check_labels("counters");
}

/// The AMS classes number parts in Arabic numerals.
#[test]
fn amsart_parts() {
    check_labels("amsart-parts");
}

/// A second label while one waits: amsmath stops ("Multiple \label's").
#[test]
fn label_clash() {
    let t = "\\documentclass{article}\\usepackage{amsmath}\\begin{document}\n\
             \\begin{align*}a&=b\\label{x}\\end{align*}\n\
             \\begin{align}a&=b\\label{y}\\end{align}\n\\end{document}\n";
    let m = Model::new(&parse(t));
    let clash: Vec<&str> = m
        .label_clashes
        .iter()
        .map(|&i| m.labels[i].name.as_str())
        .collect();
    assert_eq!(clash, ["y"]);
    // As pdflatex goes on past it: the label before is lost, the last
    // written; `equation` and `\[…\]` drop a waiting label unwritten.
    let t = "\\documentclass{article}\\usepackage{amsmath}\\begin{document}\n\\
             \\begin{align}a&=b\\nonumber\\label{k1}\\\\c&=d\\label{k2}\\end{align}\n\\
             \\begin{gather}y\\label{k5}\\label{k6}\\end{gather}\n\\
             \\begin{align*}a&=b\\label{k3}\\end{align*}\n\\
             \\begin{equation}x\\label{k4}\\end{equation}\n\\
             \\begin{align*}a&=b\\label{k7}\\end{align*}\\[x\\]\n\\
             \\begin{gather}z\\end{gather}\n\\end{document}\n";
    let m = Model::new(&parse(t));
    let number = |k: &str| {
        m.labels
            .iter()
            .find(|l| l.name == k)
            .and_then(|l| l.number.clone())
    };
    assert_eq!(number("k1"), None);
    assert_eq!(number("k2").as_deref(), Some("1"));
    assert_eq!(number("k5"), None);
    assert_eq!(number("k6").as_deref(), Some("2"));
    assert_eq!(number("k3"), None);
    assert_eq!(number("k4").as_deref(), Some("3"));
    assert_eq!(number("k7"), None);
}
