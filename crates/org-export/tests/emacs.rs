//! Differential tests: every case in `tests/export/cases` exported as
//! Emacs 30.1 with Org 9.7.11 exports it (`tools/export-expected.sh`),
//! body only. References Emacs draws at random (`orgXXXXXXX`) are
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

#[test]
fn html() {
    run(&org_export::Html, "html", &[]);
}

#[test]
fn markdown() {
    run(&org_export::Markdown, "md", &[]);
}
