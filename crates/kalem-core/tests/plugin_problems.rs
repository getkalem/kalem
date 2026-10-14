//! The problems plugins find in files (plugin API 0.2.14) drawn as a
//! language server's diagnostics are: in a file of a language with no
//! server, at their lines and columns, merged across plugins, moved to
//! the text's lines as it changes, and gone when cleared. Its own
//! process: it loads a language plugin.

use std::sync::Arc;
use std::time::Instant;

use kalem_core::DocumentState;
use kalem_core::extensions::{self, PluginProblem};
use kalem_core::lsp::{self, Severity};

fn problem(
    line: u32,
    column: u32,
    length: u32,
    severity: Severity,
    message: &str,
) -> PluginProblem {
    PluginProblem {
        line,
        column,
        length,
        severity,
        message: message.into(),
        source: "tool".into(),
    }
}

/// The text each diagnostic of `doc` covers.
fn covered(doc: &DocumentState) -> Vec<String> {
    let path = doc.meta.path.clone().expect("a file");
    lsp::diagnostics(&path)
        .iter()
        .map(|d| doc.text().as_str()[d.range.clone()].to_string())
        .collect()
}

#[test]
fn a_plugins_problems_are_drawn_as_diagnostics() {
    let dir = std::env::temp_dir().join(format!("kalem-plugin-problems-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("plugins/plain")).expect("folder");
    std::fs::write(
        dir.join("plugins/plain/plugin.json"),
        r#"{"id": "org.example.plain", "name": "Plain", "version": "1",
            "languages": [{"id": "plain", "name": "Plain", "extensions": ["pk"]}]}"#,
    )
    .expect("manifest");
    kalem_core::languages::load_from(&[dir.join("plugins")]);
    let file = dir.join("a.pk");
    std::fs::write(&file, "  first line\nsecond é line\nthird\n").expect("file");
    let mut doc = DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .expect("open");
    lsp::sync(&doc);
    assert!(lsp::diagnostics(&file).is_empty());
    assert_eq!(lsp::line_mark(&doc, 0), None);

    // Column 0 is the line from its first non-blank, a length of 0 the
    // rest of the line; columns count characters; a line past the text
    // is dropped. Another plugin's problems come in among them.
    extensions::set_problems(
        "plain",
        &file,
        vec![
            problem(1, 0, 0, Severity::Error, "whole line"),
            problem(2, 10, 4, Severity::Warning, "unknown word\nmore"),
            problem(3, 2, 0, Severity::Information, "the rest"),
            problem(9, 0, 0, Severity::Error, "past the end"),
        ],
    )
    .expect("set");
    extensions::set_problems(
        "other",
        &file,
        vec![problem(3, 1, 1, Severity::Hint, "a hint")],
    )
    .expect("set");
    assert_eq!(covered(&doc), ["first line", "line", "t", "hird"]);
    assert_eq!(lsp::line_mark(&doc, 0), Some(Severity::Error));
    assert_eq!(lsp::line_mark(&doc, 1), Some(Severity::Warning));
    assert_eq!(lsp::line_mark(&doc, 2), Some(Severity::Information));
    let at = doc.text().as_str().find("é line").expect("word") + "é ".len();
    assert_eq!(
        lsp::status(&file, at).as_deref(),
        Some("tool: unknown word")
    );
    let hover = lsp::diagnostic_at(&doc, at).expect("a hover");
    assert!(hover.contains("unknown word"), "{hover}");
    let places = lsp::problems_of(Some(&file));
    assert_eq!(places.len(), 4);
    assert_eq!((places[0].line, places[0].column), (1, 2));
    assert_eq!(places[0].preview, "error: whole line");

    // They stay at their lines as the text changes, until set again.
    let mut tx = org_edit::Transaction::new("edit");
    tx.replace(0..0, "x").expect("edit");
    doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
    lsp::sync(&doc);
    assert_eq!(covered(&doc), ["x  first line", "line", "t", "hird"]);

    // Too many are refused; cleared, they are gone, one plugin's or all.
    let many = vec![problem(1, 0, 0, Severity::Hint, "many"); extensions::MAX_PROBLEMS + 1];
    let e = extensions::set_problems("plain", &file, many).expect_err("too many");
    assert!(e.contains("At most"), "{e}");
    extensions::clear_problems("other", None);
    assert_eq!(covered(&doc), ["x  first line", "line", "hird"]);
    extensions::clear_problems("plain", Some(&file));
    assert!(lsp::diagnostics(&file).is_empty());
    assert_eq!(lsp::line_mark(&doc, 0), None);
    let _ = std::fs::remove_dir_all(&dir);
}
