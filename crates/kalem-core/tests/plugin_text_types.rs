//! A plugin language's id is the text type of the files it serves:
//! `2026.journal` is `hledger` where a plugin declares the language
//! `hledger` for `.journal` files, in the when-clauses' `textType` and
//! the type completers and menus see, and the status bar names it by the
//! language's name. A mode line naming another language wins, and other
//! files keep theirs. Its own process: it loads a language plugin.

use std::path::Path;
use std::sync::Arc;

use kalem_core::DocumentState;
use kalem_core::when::{Value, WhenClause};

fn open(dir: &Path, name: &str, text: &str) -> DocumentState {
    let file = dir.join(name);
    std::fs::write(&file, text).expect("file");
    DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .expect("open")
}

fn types(d: &DocumentState) -> (String, String, String) {
    (d.text_type(), d.document_type(), d.mode_title())
}

#[test]
fn a_plugin_languages_files_are_of_its_type() {
    let dir = std::env::temp_dir().join(format!("kalem-text-types-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("plugin")).expect("folder");
    std::fs::write(
        dir.join("plugin/plugin.json"),
        r#"{"id": "org.example.books", "name": "Books", "version": "1",
            "languages": [
              {"id": "hledger", "name": "hledger", "extensions": ["journal", "j"]},
              {"id": "beancount", "name": "Beancount", "extensions": ["beancount", "bean"]}
            ]}"#,
    )
    .expect("manifest");
    // Before the plugin: the extension's.
    let before = open(&dir, "2026.journal", "2026-01-01 Shop\n");
    assert_eq!(
        types(&before),
        ("journal".into(), "journal".into(), "journal".into())
    );
    kalem_core::languages::load_from(std::slice::from_ref(&dir));
    let j = open(
        &dir,
        "2026.journal",
        "2026-01-01 Shop\n    expenses:food  1\n    assets:cash\n",
    );
    assert_eq!(
        types(&j),
        ("hledger".into(), "hledger".into(), "hledger".into())
    );
    let b = open(&dir, "x.bean", "2026-01-01 open Assets:Cash\n");
    assert_eq!(
        types(&b),
        ("beancount".into(), "beancount".into(), "Beancount".into())
    );
    // The when-clauses see it, at the cursor and for the document.
    let is = |d: &DocumentState, w: &str| {
        WhenClause::parse(w)
            .expect("clause")
            .eval(&d.when_context())
    };
    assert!(is(&b, "textType == beancount"));
    assert!(!is(&b, "textType == bean"));
    assert_eq!(
        b.document_context().get("textType"),
        Some(&Value::Str("beancount".into()))
    );
    // The file's own language, the extension, stays where it is asked.
    assert_eq!(
        b.when_context().get("editorLanguage"),
        Some(&Value::Str("bean".into()))
    );
    // A mode line naming another language wins.
    let p = open(&dir, "notes.journal", "# -*- mode: python -*-\nx = 1\n");
    assert_eq!(p.text_type(), "python");
    assert_eq!(p.mode_title(), "python");
    // Files of no plugin's language, and a mode's own, as before.
    let t = open(&dir, "a.txt", "x\n");
    assert_eq!(t.text_type(), "text");
    let r = open(&dir, "main.rs", "fn main() {}\n");
    assert_eq!(
        (r.text_type(), r.document_type()),
        ("rust".into(), "rs".into())
    );
    let o = open(&dir, "a.org", "* A\n");
    assert_eq!(o.document_type(), "org");
    let _ = std::fs::remove_dir_all(dir);
}
