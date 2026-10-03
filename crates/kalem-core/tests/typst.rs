//! Typst files (T2.7h.25): read as Typst text with its highlighting and
//! outline, built with `typst compile`, the problems in the text. The
//! build is skipped when `typst` is not installed.

#![allow(clippy::print_stderr)]

use std::sync::Arc;

use kalem_core::DocumentState;

#[test]
fn typst_files_are_read_and_built() {
    let dir = std::env::temp_dir().join(format!("kalem-typst-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.typ");
    std::fs::write(&file, "= Intro\nHello $x^2$.\n== More\n").unwrap();
    let doc = DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &Default::default(),
    )
    .unwrap();
    assert!(kalem_core::typst::is_typst(&doc), "{:?}", doc.meta.mode);
    let kalem_core::DocumentMode::Text { language: Some(l) } = &doc.meta.mode else {
        panic!("{:?}", doc.meta.mode);
    };
    assert_eq!(
        kalem_highlight::Language::find(l).map(|l| l.name()),
        Some("Typst")
    );
    let titles: Vec<String> = kalem_core::packs::outline_items(&doc)
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, ["Intro", "More"]);

    let search = std::env::var_os("PATH").unwrap_or_default();
    if kalem_core::typst::find(&search).is_none() {
        eprintln!("typst not installed: the build skipped");
        return;
    }
    let built = kalem_core::typst::build(&file, None).unwrap();
    assert_eq!(built.pdf, Some(dir.join("main.pdf")));
    assert!(built.problems.is_empty(), "{:?}", built.problems);
    // An error: no PDF, the problem at its line, shown in the document.
    std::fs::write(&file, "= Intro\nHello #foo\n").unwrap();
    let built = kalem_core::typst::build(&file, Some(std::path::Path::new("out"))).unwrap();
    assert!(built.pdf.is_none());
    assert_eq!(built.problems.len(), 1, "{:?}", built.problems);
    assert_eq!(built.problems[0].line, Some(2));
    assert!(built.problems[0].message.contains("foo"));
    kalem_core::latex_build::record(&file, &built.problems);
    let doc = DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &Default::default(),
    )
    .unwrap();
    let d = kalem_core::typst::diagnostics(&doc);
    assert_eq!(d.len(), 1);
    assert_eq!(&doc.text().as_str()[d[0].range.clone()], "Hello #foo");
    let _ = std::fs::remove_dir_all(&dir);
}
