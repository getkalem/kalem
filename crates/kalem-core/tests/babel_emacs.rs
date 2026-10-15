//! Babel's results as Emacs writes them: every source block of
//! `tests/babel/cases.org` run in order through
//! `kalem_core::babel::run_now` (confirmation off, shell and Python
//! loaded, Python as `python3`), and the text compared with
//! `tests/babel/expected.org`, which Emacs's own Org 9.7.11 wrote from the
//! same file (`emacs -Q --batch FILE -l tools/babel-oracle.el`). The
//! blocks are those of the file as written: a block a result writes
//! (`:results code`) is not run in turn. Skipped where `sh` or `python3`
//! is not installed.
//!
//! And Babel on export: `tests/babel/export.org` exported as HTML, the
//! body only, its blocks whose results the export writes run first, and
//! compared with `tests/babel/export.html`, Emacs's export of the same file
//! (`emacs -Q --batch FILE -l tools/babel-export-oracle.el`).

#![allow(clippy::print_stderr)]

use std::path::Path;

use org_syntax::SyntaxKind;

fn installed(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("-c")
        .arg(if program == "sh" { "true" } else { "pass" })
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn every_block_as_emacs_writes_its_results() {
    if !installed("sh") || !installed("python3") {
        eprintln!("sh or python3 is not installed: Babel's results are not compared");
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/babel");
    let cases = std::fs::read_to_string(dir.join("cases.org")).expect("cases");
    let expected = std::fs::read_to_string(dir.join("expected.org")).expect("expected");
    let mut starts: Vec<usize> = org_syntax::parse(&cases)
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SRC_BLOCK)
        .map(|n| usize::from(n.text_range().start()))
        .collect();
    let loaded = ["shell".to_string(), "python".to_string()];
    let mut text = cases.clone();
    for i in 0..starts.len() {
        let ran = kalem_core::babel::run_now(&text, starts[i] + 1, &loaded, "python3", &dir)
            .unwrap_or_else(|e| panic!("block {i}: {e}"));
        // The blocks after the edit move with it.
        if let Some((range, len)) = &ran.edit {
            let delta = *len as isize - range.len() as isize;
            for s in starts.iter_mut().skip(i + 1) {
                if *s >= range.start {
                    *s = (*s as isize + delta) as usize;
                }
            }
        }
        text = ran.text;
    }
    if text != expected {
        let (got, want): (Vec<&str>, Vec<&str>) =
            (text.lines().collect(), expected.lines().collect());
        let line = got
            .iter()
            .zip(&want)
            .position(|(a, b)| a != b)
            .unwrap_or(got.len().min(want.len()));
        panic!(
            "Kalem's results differ from Emacs's at line {}:\n  kalem: {:?}\n  emacs: {:?}\n\n{text}",
            line + 1,
            got.get(line),
            want.get(line)
        );
    }
}

/// `html` with Org's made-up ids (`org` and seven hex digits), which differ
/// from one export to the next, as one.
fn ids_as_one(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest.find("org") {
        let after = &rest[i + 3..];
        let hex = after.bytes().take(7).filter(u8::is_ascii_hexdigit).count();
        out.push_str(&rest[..i]);
        if hex == 7 && !after.as_bytes().get(7).is_some_and(u8::is_ascii_hexdigit) {
            out.push_str("ID");
            rest = &after[7..];
        } else {
            out.push_str("org");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn the_export_as_emacs_writes_it() {
    if !installed("sh") {
        eprintln!("sh is not installed: Babel's export is not compared");
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/babel");
    let path = dir.join("export.org");
    let org = std::fs::read_to_string(&path).expect("export.org");
    let expected = std::fs::read_to_string(dir.join("export.html")).expect("export.html");
    let settings = kalem_core::babel::export::Settings {
        loaded: vec!["shell".into()],
        python: String::new(),
        confirm: false,
    };
    let copy = kalem_core::babel::export::evaluate_now(
        &org,
        Some(&path),
        None,
        &settings,
        &mut |_| None,
        &mut |_| {},
    );
    let html = org_export::export(
        &copy,
        &org_export::Html,
        &org_export::Settings {
            body_only: true,
            input_file: Some(path.clone()),
            now: None,
            subtree: None,
            math: Some(kalem_core::math::export_renderer()),
            options: None,
        },
    )
    .expect("export");
    assert_eq!(ids_as_one(&html), ids_as_one(&expected));
    // The file itself is not changed.
    assert_eq!(std::fs::read_to_string(&path).expect("export.org"), org);
}
