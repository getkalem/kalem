//! Differential tests: every case in `tests/export/cases` exported as
//! Emacs 30.1 with Org 9.7.11 exports it (`tools/export-expected.sh`),
//! body only. The whole pages of `tests/export/full` and the setup-file
//! cases were written by Org 9.6; their normalization absorbs the
//! differences between the two versions. References Emacs draws at random (`orgXXXXXXX`) are
//! numbered by first appearance on both sides before comparing.

#![allow(clippy::print_stderr)]

use std::collections::HashMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/export"))
}

/// `org[0-9a-f]{7}` references renumbered in order of appearance.
fn normalize(s: &str) -> String {
    let mut map: HashMap<String, usize> = HashMap::new();
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if s[i..].starts_with("org")
            && i + 10 <= b.len()
            && b[i + 3..i + 10]
                .iter()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            && (i + 10 == b.len() || !b[i + 10].is_ascii_alphanumeric())
        {
            let key = s[i..i + 10].to_string();
            let n = map.len();
            let n = *map.entry(key).or_insert(n);
            out.push_str(&format!("REF{n}"));
            i += 10;
            continue;
        }
        let c = s[i..].chars().next().expect("a char");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The subtree a case exports: the headline with a `KALEM_TEST_SUBTREE`
/// property, as `tests/emacs/export.el` finds it.
fn subtree_of(text: &str) -> Option<usize> {
    let i = text.find(":KALEM_TEST_SUBTREE:")?;
    text[..i].rfind("\n*").map(|h| h + 1)
}

fn run(backend: &dyn org_export::Backend, ext: &str, known: &[&str]) {
    let mut failed = Vec::new();
    let mut total = 0;
    let mut cases: Vec<PathBuf> = std::fs::read_dir(root().join("cases"))
        .expect("the cases")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "org"))
        .collect();
    cases.sort();
    for case in cases {
        let name = case.file_stem().unwrap().to_string_lossy().to_string();
        let expected = root().join("expected").join(format!("{name}.{ext}"));
        let Ok(want) = std::fs::read_to_string(&expected) else {
            continue;
        };
        total += 1;
        let text = std::fs::read_to_string(&case).unwrap();
        let got = org_export::export(
            &text,
            backend,
            &org_export::Settings {
                body_only: true,
                input_file: Some(case.clone()),
                now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
                subtree: subtree_of(&text),
                math: None,
                options: None,
            },
        )
        .unwrap_or_else(|e| format!("ERROR: {e}\n"));
        let (g, w) = (normalize(&got), normalize(&want));
        if g != w {
            if known.contains(&name.as_str()) {
                continue;
            }
            if std::env::var_os("KALEM_EXPORT_DIFF").is_some() {
                let d = std::env::temp_dir().join(format!("kalem-export-{name}.{ext}"));
                std::fs::write(&d, &g).unwrap();
                eprintln!("{name}: got {} (want {})", d.display(), expected.display());
            }
            failed.push(name);
        } else if known.contains(&name.as_str()) {
            eprintln!("{name}.{ext} now agrees with Emacs: take it off the known list");
        }
    }
    assert!(
        failed.is_empty(),
        "{}/{} {ext} cases differ: {failed:?}",
        failed.len(),
        total
    );
}

/// A whole page as Kalem and Emacs both write it: time stamps, the style
/// sheet, the MathJax set-up and the generator are Kalem's own; blank
/// lines are dropped, since Org 9.6 (which wrote `tests/export/full`)
/// puts one before headlines where 9.7 does not.
fn normalize_page(s: &str) -> String {
    let mut s = normalize(s);
    let cut = |s: &mut String, start: &str, end: &str, with: &str| {
        while let Some(i) = s.find(start) {
            let Some(j) = s[i..].find(end) else { break };
            s.replace_range(i..i + j + end.len(), with);
        }
    };
    cut(&mut s, "<style>", "</style>", "STYLE");
    cut(&mut s, "<style type=\"text/css\">", "</style>", "STYLE");
    cut(&mut s, "<script>\n  window.MathJax", "</script>", "MATHJAX");
    cut(&mut s, "<script\n  id=\"MathJax-script\"", "</script>", "");
    cut(&mut s, "<script id=\"MathJax-script\"", "</script>", "");
    s = s.replace("content=\"Org Mode\"", "content=\"Kalem\"");
    // `2026-09-28 Mon 10:00` and the like.
    let b = s.as_bytes().to_vec();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let is_stamp = i + 20 <= b.len()
            && b[i..i + 4].iter().all(u8::is_ascii_digit)
            && b[i + 4] == b'-'
            && b[i + 7] == b'-'
            && b[i + 10] == b' '
            && b[i + 14] == b' '
            && b[i + 17] == b':';
        if is_stamp {
            out.push_str("TIME");
            i += 20;
        } else {
            let c = s[i..].chars().next().expect("a char");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whole HTML pages against `tests/export/full` (`KALEM_EXPORT_FULL=1
/// tools/export-expected.sh`, HTML only).
#[test]
fn html_page() {
    // Org 9.7 names footnotes by label, 9.6 by number.
    let known = ["footnotes", "includes"];
    let mut failed = Vec::new();
    let mut cases: Vec<PathBuf> = std::fs::read_dir(root().join("full"))
        .expect("the pages")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "html"))
        .collect();
    cases.sort();
    for page in &cases {
        let name = page.file_stem().unwrap().to_string_lossy().to_string();
        if known.contains(&name.as_str()) {
            continue;
        }
        let case = root().join("cases").join(format!("{name}.org"));
        let text = std::fs::read_to_string(&case).unwrap();
        let got = org_export::export(
            &text,
            &org_export::Html,
            &org_export::Settings {
                body_only: false,
                input_file: Some(case.clone()),
                now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
                subtree: subtree_of(&text),
                math: None,
                options: None,
            },
        )
        .unwrap_or_else(|e| format!("ERROR: {e}\n"));
        let want = std::fs::read_to_string(page).unwrap();
        if normalize_page(&got) != normalize_page(&want) {
            if std::env::var_os("KALEM_EXPORT_DIFF").is_some() {
                let d = std::env::temp_dir().join(format!("kalem-page-{name}.html"));
                std::fs::write(&d, normalize_page(&got)).unwrap();
                let w = std::env::temp_dir().join(format!("kalem-page-{name}.want.html"));
                std::fs::write(&w, normalize_page(&want)).unwrap();
                eprintln!("{name}: got {} want {}", d.display(), w.display());
            }
            failed.push(name);
        }
    }
    assert!(
        failed.is_empty(),
        "{} pages differ: {failed:?}",
        failed.len()
    );
}

#[test]
fn html() {
    run(&org_export::Html, "html", &[]);
}

#[test]
fn markdown() {
    run(&org_export::Markdown, "md", &[]);
}

/// Against the ox-gfm package (`tools/fetch-ox-gfm.sh`, then
/// `tools/export-expected.sh`), which is not part of Org.
#[test]
fn gfm() {
    run(&org_export::Gfm, "gfm", &[]);
}

#[test]
fn latex() {
    // Emacs refuses to download the remote image of `images` and stops.
    run(&org_export::Latex::default(), "tex", &["images"]);
}

/// A whole LaTeX document as Kalem and Emacs both write it: the creation
/// time and the creator are their own.
fn normalize_latex(s: &str) -> String {
    normalize(s)
        .lines()
        .filter(|l| !l.starts_with("% Created ") && !l.starts_with(" pdfcreator="))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whole documents of `ext` in `tests/export/full` (`KALEM_EXPORT_FULL=1
/// KALEM_EXPORT_BACKENDS=latex` or `ascii`) against Kalem's.
fn documents(
    backend: &dyn org_export::Backend,
    ext: &str,
    known: &[&str],
    norm: fn(&str) -> String,
) {
    let mut failed = Vec::new();
    let mut cases: Vec<PathBuf> = std::fs::read_dir(root().join("full"))
        .expect("the documents")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    cases.sort();
    for page in &cases {
        let name = page.file_stem().unwrap().to_string_lossy().to_string();
        if known.contains(&name.as_str()) {
            continue;
        }
        let case = root().join("cases").join(format!("{name}.org"));
        let text = std::fs::read_to_string(&case).unwrap();
        let got = org_export::export(
            &text,
            backend,
            &org_export::Settings {
                body_only: false,
                input_file: Some(case.clone()),
                now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
                subtree: subtree_of(&text),
                math: None,
                options: None,
            },
        )
        .unwrap_or_else(|e| format!("ERROR: {e}\n"));
        let want = std::fs::read_to_string(page).unwrap();
        if norm(&got) != norm(&want) {
            if std::env::var_os("KALEM_EXPORT_DIFF").is_some() {
                let d = std::env::temp_dir().join(format!("kalem-doc-{name}.{ext}"));
                std::fs::write(&d, norm(&got)).unwrap();
                let w = std::env::temp_dir().join(format!("kalem-doc-{name}.want.{ext}"));
                std::fs::write(&w, norm(&want)).unwrap();
                eprintln!("{name}: got {} want {}", d.display(), w.display());
            }
            failed.push(name);
        }
    }
    assert!(
        failed.is_empty(),
        "{} {ext} documents differ: {failed:?}",
        failed.len()
    );
}

#[test]
fn latex_document() {
    // Emacs stops on the remote image.
    documents(
        &org_export::Latex::default(),
        "tex",
        &["images"],
        normalize_latex,
    );
}

#[test]
fn text_document() {
    documents(&org_export::Text::default(), "txt", &["images"], normalize);
}

#[test]
fn text() {
    run(&org_export::Text::default(), "txt", &[]);
}

/// The sample book of `examples/book` (a part, two included chapters,
/// a figure, a table with formulas, an equation, citations, footnotes and
/// cross references) against Emacs's whole documents in
/// `tests/export/book` (`KALEM_EXPORT_FULL=1 emacs -Q --batch -l
/// tests/emacs/export.el examples/book tests/export/book`).
#[test]
fn sample_book() {
    let book = root().join("../../examples/book/book.org");
    let text = std::fs::read_to_string(&book).unwrap();
    type Case<'a> = (&'a dyn org_export::Backend, &'a str, fn(&str) -> String);
    let backends: [Case<'_>; 4] = [
        (&org_export::Html, "html", normalize_page),
        (&org_export::Latex::default(), "tex", normalize_latex),
        (&org_export::Text::default(), "txt", normalize),
        (&org_export::Markdown, "md", normalize),
    ];
    let mut failed = Vec::new();
    for (backend, ext, norm) in backends {
        let got = org_export::export(
            &text,
            backend,
            &org_export::Settings {
                body_only: false,
                input_file: Some(book.clone()),
                now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
                subtree: None,
                math: None,
                options: None,
            },
        )
        .unwrap_or_else(|e| format!("ERROR: {e}\n"));
        let want =
            std::fs::read_to_string(root().join("book").join(format!("book.{ext}"))).unwrap();
        if norm(&got) != norm(&want) {
            if std::env::var_os("KALEM_EXPORT_DIFF").is_some() {
                let d = std::env::temp_dir().join(format!("kalem-book.{ext}"));
                std::fs::write(&d, norm(&got)).unwrap();
                let w = std::env::temp_dir().join(format!("kalem-book.want.{ext}"));
                std::fs::write(&w, norm(&want)).unwrap();
                eprintln!("{ext}: got {} want {}", d.display(), w.display());
            }
            failed.push(ext);
        }
    }
    assert!(
        failed.is_empty(),
        "the sample book differs from Emacs in {failed:?}"
    );
}
