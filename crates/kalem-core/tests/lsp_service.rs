//! The editor's language server service (`kalem_core::lsp`) on a language
//! plugin whose server is a fake: this binary started again with
//! `KALEM_LSP_FAKE` set.

#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::DocumentState;
use kalem_core::lsp::{self, Kind, Outcome};

fn until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let t = Instant::now();
    loop {
        lsp::tick();
        if let Some(v) = f() {
            return v;
        }
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn outcome(path: &Path) -> Outcome {
    until("an answer", || {
        lsp::take_outcomes(Some(path))
            .into_iter()
            .find(|o| !matches!(o, Outcome::Message { error: false, .. }))
    })
}

fn edit(doc: &mut DocumentState, at: std::ops::Range<usize>, text: &str) {
    let mut tx = org_edit::Transaction::new("edit");
    tx.replace(at, text).unwrap();
    doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
    lsp::sync(doc);
}

fn setup() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("kalem-lsp-service-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let plug = dir.join("plugins/fake");
    std::fs::create_dir_all(&plug).unwrap();
    let exe = std::env::current_exe().unwrap();
    let manifest = serde_json::json!({
        "id": "org.example.fake", "name": "Fake", "version": "1",
        "languages": [{"id": "fakelang", "name": "Fake", "extensions": ["fk"], "servers": ["f"]}],
        "servers": {"f": {"name": "FakeLS", "command": [exe], "env": {"KALEM_LSP_FAKE": "normal"},
                          "rootMarkers": ["root.marker"], "settings": {"elixirLS": {"x": 1}}}}
    });
    std::fs::write(plug.join("plugin.json"), manifest.to_string()).unwrap();
    // The project is reached through a link, as `/tmp` is on macOS: the
    // fake server names files by their real paths, as Expert does.
    let real = dir.join("real-project");
    std::fs::create_dir_all(real.join("src")).unwrap();
    let project = dir.join("project");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &project).unwrap();
    #[cfg(not(unix))]
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("root.marker"), "").unwrap();
    let file = project.join("src/a.fk");
    std::fs::write(&file, "one  two bad\n😀 x\n").unwrap();
    kalem_core::languages::load_from(&[dir.join("plugins")]);
    (dir, file)
}

fn main() {
    if let Ok(b) = std::env::var("KALEM_LSP_FAKE") {
        kalem_lsp::fake::serve(&b);
        return;
    }
    let (dir, file) = setup();
    let mut doc = DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .unwrap();
    lsp::sync(&doc);
    assert!(lsp::serves(&doc));
    until("ready", || lsp::can(&doc, Kind::Hover).then_some(()));
    assert!(lsp::describe(&doc).unwrap().contains("FakeLS"));
    assert!(
        lsp::describe(&doc).unwrap().contains("project"),
        "the root marker's folder"
    );

    // Diagnostics, kept in step through edits.
    until("diagnostics", || {
        (lsp::diagnostics(&file).len() == 1).then_some(())
    });
    let status = lsp::status(&file, 0).unwrap();
    assert!(status.contains("bad found"), "{status}");
    edit(&mut doc, 0..0, "TODO ");
    until("new diagnostics", || {
        (lsp::diagnostics(&file).len() == 2).then_some(())
    });
    let d = lsp::diagnostics(&file);
    assert_eq!(&doc.text().as_str()[d[1].range.clone()], "bad");
    println!("test diagnostics ... ok");

    // Hover after the emoji: UTF-16 on the wire, bytes here.
    let at = doc.text().as_str().find('x').unwrap();
    doc.selection = org_edit::Selection::caret(at);
    lsp::request(&doc, Kind::Hover).unwrap();
    match outcome(&file) {
        Outcome::Hover { text, .. } => assert_eq!(text, "at 1:3"),
        o => panic!("{o:?}"),
    }
    let lines = lsp::hover_lines("```elixir\ndef f\n```\n\na b c d e f", 5, 3);
    assert_eq!(lines, ["def f", "", "a b c …"]);
    println!("test hover ... ok");

    lsp::request(&doc, Kind::Definition).unwrap();
    match outcome(&file) {
        Outcome::Jump(p) => {
            assert_eq!((p.path.as_path(), p.line, p.column), (file.as_path(), 1, 2))
        }
        o => panic!("{o:?}"),
    }
    assert!(
        lsp::request(&doc, Kind::Symbols)
            .unwrap_err()
            .contains("does not provide")
    );
    println!("test definition ... ok");

    // Formatting: edits for this version, applied as one transaction.
    lsp::request(&doc, Kind::Format).unwrap();
    match outcome(&file) {
        Outcome::Edits {
            version,
            edits,
            label,
            ..
        } => {
            assert_eq!(version, doc.version());
            let tx = lsp::transaction(&edits, &label).unwrap();
            doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
            assert_eq!(doc.text().as_str(), "TODO one two bad\n😀 x\n");
        }
        o => panic!("{o:?}"),
    }
    println!("test format ... ok");

    // Completion: the server's items on the completer contract.
    lsp::sync(&doc);
    let end = doc.text().as_str().find(" x").unwrap() + 2;
    edit(&mut doc, end..end, " gr");
    doc.selection = org_edit::Selection::caret(end + 3);
    let reg = kalem_core::completers::Registry::with_builtins();
    let items = reg.complete(&mut doc, true, Duration::from_secs(5));
    let greet = items
        .iter()
        .find(|i| i.source == "lsp" && i.label == "greet/1")
        .unwrap();
    assert_eq!(greet.insert, "greet(name)");
    assert_eq!(&doc.text().as_str()[greet.range.clone()], "gr");
    println!("test completion ... ok");

    // A crash: restarted, the document opened again.
    let n = doc.text().len();
    edit(&mut doc, n..n, "CRASH");
    until("the restart notice", || {
        lsp::take_outcomes(Some(&file)).into_iter().find(
            |o| matches!(o, Outcome::Message { text, error: true } if text.contains("restarting")),
        )
    });
    until("the restarted server", || {
        lsp::can(&doc, Kind::Hover).then_some(())
    });
    assert!(
        lsp::report()[0].contains("ready, 1 documents"),
        "{:?}",
        lsp::report()
    );
    println!("test restart ... ok");

    // No server for other files, said with the reason.
    let other = dir.join("project/notes.txt");
    std::fs::write(&other, "x").unwrap();
    let txt = DocumentState::open(
        &other,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .unwrap();
    assert!(
        lsp::request(&txt, Kind::Hover)
            .unwrap_err()
            .contains("language plugin")
    );

    lsp::closed(&file);
    assert!(!lsp::serves(&doc));
    std::thread::sleep(Duration::from_millis(300));
    assert!(lsp::report().is_empty());
    println!("test close ... ok");
    let _ = std::fs::remove_dir_all(&dir);
}
