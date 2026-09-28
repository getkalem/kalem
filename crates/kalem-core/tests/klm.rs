//! Kalem documents read as Org (design §3.7, decision D24): each `.klm`
//! file in `tests/corpus/klm`, renamed to `.org`, parses and writes back
//! byte for byte, has no warnings, and exports in Emacs as its strict Org
//! form (Kalem's additions taken out) exports in Kalem. The Emacs side is
//! `tests/export/klm`, written by `tests/emacs/export.el`
//! (`KALEM_EXPORT_BACKENDS="html md"`); `kalem diff-emacs` in CI checks
//! that Emacs parses the corpus as Kalem does.

#![allow(clippy::print_stderr)]

use std::collections::HashMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests"))
}

/// `org[0-9a-f]{7}` references renumbered in order of appearance, and
/// blank lines dropped (Org 9.6 and 9.7 differ in those).
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
        {
            let n = map.len();
            let n = *map.entry(s[i..i + 10].to_string()).or_insert(n);
            out.push_str(&format!("REF{n}"));
            i += 10;
            continue;
        }
        let c = s[i..].chars().next().expect("a char");
        out.push(c);
        i += c.len_utf8();
    }
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn kalem_documents_are_org() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("corpus/klm"))
        .expect("the corpus")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "klm"))
        .collect();
    files.sort();
    assert!(files.len() >= 2);
    let mut failed = Vec::new();
    for file in &files {
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(file).unwrap();
        let parse = org_syntax::parse(&text);
        assert_eq!(parse.syntax().to_string(), text, "{name}");
        let errors: Vec<_> = parse
            .diagnostics()
            .into_iter()
            .filter(|d| d.severity == org_syntax::Severity::Warning)
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        let (strict, _) = kalem_core::kinds::strip_markup(&text);
        let backends: [(&dyn org_export::Backend, &str); 2] =
            [(&org_export::Html, "html"), (&org_export::Markdown, "md")];
        for (backend, ext) in backends {
            let want = std::fs::read_to_string(root().join(format!("export/klm/{name}.{ext}")))
                .expect("Emacs's export");
            let got = org_export::export(
                &strict,
                backend,
                &org_export::Settings {
                    body_only: true,
                    input_file: Some(file.clone()),
                    now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
                    subtree: None,
                    math: None,
                    options: None,
                },
            )
            .unwrap();
            if normalize(&got) != normalize(&want) {
                if std::env::var_os("KALEM_EXPORT_DIFF").is_some() {
                    let d = std::env::temp_dir().join(format!("kalem-klm-{name}.{ext}"));
                    std::fs::write(&d, normalize(&got)).unwrap();
                    let w = std::env::temp_dir().join(format!("kalem-klm-{name}.want.{ext}"));
                    std::fs::write(&w, normalize(&want)).unwrap();
                    eprintln!("{name}.{ext}: got {} want {}", d.display(), w.display());
                }
                failed.push(format!("{name}.{ext}"));
            }
        }
    }
    assert!(failed.is_empty(), "differ from Emacs: {failed:?}");
}
