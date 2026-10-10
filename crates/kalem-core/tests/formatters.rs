//! Format Document through the formatter set for a file's type
//! (`formatters.TYPE`, T2.7i.10a): the program runs in the background
//! with the text on its standard input, and its output comes back as an
//! edit; `off` leaves the type without one; a failure is a message.
#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::DocumentState;
use kalem_core::command::{Clipboard, CommandRegistry, EditorContext};
use kalem_core::lsp::{self, Outcome};
use kalem_core::settings::Config;

fn format(doc: &mut DocumentState) -> Result<(), String> {
    let reg = CommandRegistry::with_builtins();
    let config = Config::default();
    let mut clip = Clipboard::default();
    let mut ctx = EditorContext {
        document: Some(doc),
        clipboard: &mut clip,
        config: &config,
        now: Instant::now(),
        clock: jiff::civil::date(2026, 10, 10).at(9, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute("edit.formatDocument", &mut ctx, &serde_json::Value::Null)
        .map_err(|e| e.to_string())
}

fn outcome(path: &std::path::Path, version: u64) -> Outcome {
    let t = Instant::now();
    loop {
        if let Some(o) = lsp::take_outcomes(path, version)
            .into_iter()
            .find(|o| !matches!(o, Outcome::Message { error: false, .. }))
        {
            return o;
        }
        assert!(t.elapsed() < Duration::from_secs(15), "no answer");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_type_s_formatter_runs_on_the_text() {
    let dir = std::env::temp_dir().join(format!("kalem-formatters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.foo");
    std::fs::write(&file, "hello\n").unwrap();
    let mut doc = DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .unwrap();
    // A type nothing is known for: no formatter, and the command says so.
    kalem_core::formatters::set_user_table(None);
    assert!(!kalem_core::formatters::has_formatter(&doc));
    let why = format(&mut doc).unwrap_err();
    assert!(why.contains("formatters.foo"), "{why}");
    // The user's: the text through `tr`, back as an edit for this version.
    kalem_core::formatters::set_user_table(Some(&serde_json::json!({ "foo": "tr a-z A-Z" })));
    assert!(kalem_core::formatters::has_formatter(&doc));
    assert_eq!(
        doc.when_context().get("hasFormatter"),
        Some(&kalem_core::when::Value::Bool(true))
    );
    format(&mut doc).unwrap();
    match outcome(&file, doc.version()) {
        Outcome::Edits {
            version,
            edits,
            label,
            ..
        } => {
            assert_eq!(version, doc.version());
            let tx = lsp::transaction(&edits, &label).unwrap();
            doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
            assert_eq!(doc.text().as_str(), "HELLO\n");
        }
        o => panic!("{o:?}"),
    }
    // Already formatted: a plain message, no edit.
    format(&mut doc).unwrap();
    let t = Instant::now();
    let mut said = None;
    while said.is_none() && t.elapsed() < Duration::from_secs(15) {
        said = lsp::take_outcomes(&file, doc.version()).into_iter().next();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(said, Some(Outcome::Message { error: false, .. })),
        "{said:?}"
    );
    // A formatter that fails: its first line of errors, as an error.
    kalem_core::formatters::set_user_table(Some(&serde_json::json!({
        "foo": "sh -c 'echo boom >&2; exit 1'"
    })));
    format(&mut doc).unwrap();
    match outcome(&file, doc.version()) {
        Outcome::Message { text, error: true } => assert!(text.contains("boom"), "{text}"),
        o => panic!("{o:?}"),
    }
    // Turned off.
    kalem_core::formatters::set_user_table(Some(&serde_json::json!({ "foo": "off" })));
    assert!(!kalem_core::formatters::has_formatter(&doc));
    assert!(format(&mut doc).is_err());
    kalem_core::formatters::set_user_table(None);
    let _ = std::fs::remove_dir_all(&dir);
}
