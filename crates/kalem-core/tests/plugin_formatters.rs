//! A plugin's formatter (plugin API 0.2.16, the `formatter` export) for
//! Format Document and on save: its text replaces the document's as the
//! least change, its refusal puts the cursor at its place, a busy plugin
//! is said so, and a type it does not format is left to the others. Its
//! own process: it sets the formatters' bridge.

use std::sync::Arc;
use std::time::Instant;

use kalem_core::DocumentState;
use kalem_core::command::{Clipboard, CommandRegistry, EditorContext};
use kalem_core::formatters::{self, plugins};
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
        clock: jiff::civil::date(2026, 10, 15).at(9, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute("edit.formatDocument", &mut ctx, &serde_json::Value::Null)
        .map_err(|e| e.to_string())
}

fn set(doc: &mut DocumentState, text: &str) {
    let mut tx = org_edit::Transaction::new("set");
    tx.replace(0..doc.text().len(), text).expect("edit");
    doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
}

#[test]
fn a_plugins_formatter_formats_its_types() {
    let dir = std::env::temp_dir().join(format!("kalem-plugin-fmt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("folder");
    let file = dir.join("a.fmtest");
    std::fs::write(&file, "  one  \ntwo\n").expect("file");
    let mut doc = DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .expect("open");
    assert!(!formatters::has_formatter(&doc), "no plugin formats it yet");

    plugins::register(plugins::PluginFormatterSpec {
        plugin: "fmtest".into(),
        id: "trim".into(),
        types: vec!["fmtest".into()],
    });
    plugins::set_provider(Arc::new(|spec, path, text, kind| {
        assert_eq!((spec.id.as_str(), kind), ("trim", "fmtest"));
        assert!(path.is_some_and(|p| p.ends_with("a.fmtest")));
        if let Some(at) = text.find("BAD") {
            return plugins::Answer::Refused {
                message: "not this".into(),
                at: Some(at),
            };
        }
        if text.contains("BUSY") {
            return plugins::Answer::Busy;
        }
        plugins::Answer::Text(text.lines().map(str::trim).collect::<Vec<_>>().join("\n") + "\n")
    }));
    assert!(formatters::has_formatter(&doc));

    // Format Document: the least change, one undo step.
    format(&mut doc).expect("formatted");
    assert_eq!(doc.text().as_str(), "one\ntwo\n");
    // On save, and `kalem fmt`'s way: the text formatted now.
    set(&mut doc, " three\n");
    assert_eq!(formatters::format_sync(&doc), Ok(Some("three\n".into())));

    // A refusal: said, the cursor at its place, the text as it was.
    set(&mut doc, "one\nBAD two\n");
    let e = format(&mut doc).expect_err("refused");
    assert_eq!(e, "not this");
    assert_eq!(doc.selection.head, 4);
    assert_eq!(doc.text().as_str(), "one\nBAD two\n");
    assert_eq!(formatters::format_sync(&doc), Err("not this".into()));

    // Busy with another call: said so.
    set(&mut doc, "BUSY\n");
    let e = format(&mut doc).expect_err("busy");
    assert!(e.contains("fmtest"), "{e}");

    // A type it does not format; its plugin stopped.
    let other = dir.join("b.zzzfmt");
    std::fs::write(&other, "x\n").expect("file");
    let b = DocumentState::open(
        &other,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .expect("open");
    assert!(!formatters::has_formatter(&b));
    plugins::remove_plugin("fmtest");
    assert!(!formatters::has_formatter(&doc));
    let _ = std::fs::remove_dir_all(&dir);
}
