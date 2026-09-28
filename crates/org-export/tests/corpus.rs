//! The extended corpus: every Worg file exported by Emacs
//! (`.cache/export-expected/worg`, made by
//! `KALEM_EXPORT_BACKENDS="html md" emacs -Q --batch -l tests/emacs/export.el
//! .cache/worg .cache/export-expected/worg recursive`) and by Kalem. The
//! test reports how many agree and fails if fewer agree than before
//! (the numbers below only go up; every file agrees now). Without the corpus it does nothing.

#![allow(clippy::print_stderr)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.cache"))
}

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

fn run(backend: &dyn org_export::Backend, ext: &str, known: usize) {
    let dir = root().join("export-expected/worg");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no extended corpus: skipped");
        return;
    };
    let mut names: Vec<PathBuf> = entries
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    names.sort();
    let (mut same, mut total, mut errors) = (0, 0, 0);
    let mut differing = Vec::new();
    for expected in names {
        let want = std::fs::read_to_string(&expected).unwrap_or_default();
        if want.starts_with("ERROR:") {
            errors += 1;
            continue;
        }
        let stem = expected
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .replace("__", "/");
        let source = root().join("worg").join(format!("{stem}.org"));
        let Ok(text) = std::fs::read_to_string(&source) else {
            continue;
        };
        total += 1;
        let t0 = std::time::Instant::now();
        if std::env::var_os("KALEM_EXPORT_CORPUS_TRACE").is_some() {
            eprintln!("start {stem}");
        }
        let got = org_export::export(
            &text,
            backend,
            &org_export::Settings {
                body_only: true,
                input_file: Some(source.clone()),
                now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
            },
        )
        .unwrap_or_else(|e| format!("ERROR: {e}\n"));
        if t0.elapsed().as_secs_f64() > 1.0 {
            eprintln!("slow: {stem} {:.1}s", t0.elapsed().as_secs_f64());
        }
        if normalize(&got) == normalize(&want) {
            same += 1;
        } else {
            differing.push(stem.clone());
            if let Some(d) = std::env::var_os("KALEM_EXPORT_CORPUS_DIFF") {
                let out = Path::new(&d).join(format!("{}.{ext}", stem.replace('/', "__")));
                std::fs::write(out, &got).unwrap();
            }
        }
    }
    eprintln!(
        "{ext}: {same} of {total} Worg files as Emacs exports them ({errors} Emacs errors skipped)"
    );
    if std::env::var_os("KALEM_EXPORT_CORPUS_LIST").is_some() {
        eprintln!("{differing:#?}");
    }
    assert!(
        same >= known,
        "{ext}: {same} agree, fewer than the {known} before"
    );
}

#[test]
fn worg_html() {
    run(&org_export::Html, "html", 287);
}

#[test]
fn worg_md() {
    run(&org_export::Markdown, "md", 287);
}
