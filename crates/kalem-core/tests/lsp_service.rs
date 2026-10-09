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

fn outcome(path: &Path, version: u64) -> Outcome {
    until("an answer", || {
        lsp::take_outcomes(path, version)
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
        "commands": {"format": [exe, "--fake-format", "{file}"]},
        "servers": {"f": {"name": "FakeLS", "command": [exe], "env": {"KALEM_LSP_FAKE": "normal"},
                          "rootMarkers": ["root.marker"], "requireRoot": true,
                          "settings": {"elixirLS": {"x": 1}}}}
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
    // nextest asks each test binary for its tests before it runs them:
    // this one is a single test, `main` (docs/ci_todo.md, C4).
    if std::env::args().any(|a| a == "--list") {
        if !std::env::args().any(|a| a == "--ignored") {
            println!("main: test");
        }
        return;
    }
    // The plugin's formatter command: runs of spaces become one.
    if std::env::args().any(|a| a == "--fake-format") {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).unwrap();
        while text.contains("  ") {
            text = text.replace("  ", " ");
        }
        print!("{text}");
        return;
    }
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
    match outcome(&file, doc.version()) {
        Outcome::Hover { text, .. } => assert_eq!(text, "at 1:3"),
        o => panic!("{o:?}"),
    }
    // An answer nobody takes does not keep the editors redrawing, and
    // documentation for text that changed meanwhile is dropped.
    lsp::request(&doc, Kind::Hover).unwrap();
    until("the answer", || (!lsp::busy()).then_some(()));
    assert!(!lsp::tick(), "an untaken answer is not a change");
    edit(&mut doc, 0..0, " ");
    assert!(lsp::take_outcomes(&file, doc.version()).is_empty());
    edit(&mut doc, 0..1, "");
    let lines = lsp::hover_lines("```elixir\ndef f\n```\n\na b c d e f", 5, 3);
    assert_eq!(lines, ["def f", "", "a b c …"]);
    println!("test hover ... ok");

    lsp::request(&doc, Kind::Definition).unwrap();
    match outcome(&file, doc.version()) {
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
    // The snippet's place to fill left empty, the cursor in it.
    assert_eq!(greet.insert, "greet()");
    assert_eq!(greet.cursor, "greet(".len());
    assert_eq!(&doc.text().as_str()[greet.range.clone()], "gr");
    // Documentation: given with an item, or fetched when it is chosen.
    assert_eq!(greet.documentation.as_deref(), Some("Greets `name`."));
    let goodbye = items.iter().find(|i| i.label == "goodbye/0").unwrap();
    assert!(goodbye.documentation.is_none() && goodbye.data.is_some());
    use kalem_core::completers::Completer;
    assert_eq!(
        lsp::LspCompleter.resolve(goodbye).as_deref(),
        Some("Docs of goodbye/0.")
    );
    let mut menu = kalem_core::completers::Menu::open(&reg, &mut doc, true, 0).unwrap();
    let t = Instant::now();
    while menu.session.waiting() || menu.items().len() < 2 {
        menu.session.poll();
        assert!(t.elapsed() < Duration::from_secs(5), "no items");
        std::thread::sleep(Duration::from_millis(5));
    }
    while menu.current().map(|i| i.label.as_str()) != Some("goodbye/0") {
        menu.step(true);
    }
    while menu.documentation().is_none() {
        menu.fetch_documentation();
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "no documentation fetched"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(menu.documentation(), Some("Docs of goodbye/0."));
    drop(menu);
    // An item's edits elsewhere come with it, in the same change.
    let before = doc.text().as_str().to_string();
    kalem_core::completers::apply(&mut doc, goodbye, Instant::now());
    let after = doc.text().as_str().to_string();
    assert!(after.starts_with("use Bye\n"), "{after:?}");
    assert!(after.contains(" goodbye()"), "{after:?}");
    let caret = doc.selection.head;
    assert_eq!(
        &after[..caret],
        format!("use Bye\n{}", &before[..greet.range.start]) + "goodbye()"
    );
    doc.undo();
    assert_eq!(doc.text().as_str(), before, "one undo step");
    lsp::sync(&doc);
    let before = doc.text().as_str().to_string();
    kalem_core::completers::apply(&mut doc, greet, Instant::now());
    let after = doc.text().as_str().to_string();
    assert_eq!(after, before.replacen(" gr", " greet()", 1));
    assert_eq!(doc.selection.head, end + 1 + "greet(".len());
    println!("test completion ... ok");

    // A crash: restarted, the document opened again.
    let n = doc.text().len();
    edit(&mut doc, n..n, "CRASH");
    until("the restart notice", || {
        kalem_core::jobs::take_notices()
            .into_iter()
            .find(|(text, error)| *error && text.contains("restarting"))
    });
    until("the restarted server", || {
        lsp::can(&doc, Kind::Hover).then_some(())
    });
    assert!(
        lsp::report()[0].contains("ready, 1 documents"),
        "{:?}",
        lsp::report()
    );
    // The crash's trigger out of the text before anything else is sent.
    let t = doc.text().as_str().to_string();
    let at = t.find("CRASH").unwrap();
    edit(&mut doc, at..at + "CRASH".len(), "");
    println!("test restart ... ok");

    // The plugin updated: its server stops, and starts again for the
    // document with the plugin as it is now.
    lsp::plugin_changed("org.example.fake");
    assert!(!lsp::serves(&doc));
    lsp::sync(&doc);
    assert!(lsp::serves(&doc));
    until("the server again", || {
        lsp::can(&doc, Kind::Hover).then_some(())
    });
    println!("test plugin changed ... ok");

    // Settings reach open documents: servers off, then on again, without
    // reopening the file.
    kalem_core::languages::set_user_settings(Some(&serde_json::json!({
        "org.example.fake": {"server": "off"}
    })));
    lsp::settings_changed();
    lsp::sync(&doc);
    assert!(!lsp::serves(&doc), "servers off");
    kalem_core::languages::set_user_settings(None);
    lsp::settings_changed();
    lsp::sync(&doc);
    assert!(lsp::serves(&doc), "servers on again");
    until("the server after the settings", || {
        lsp::can(&doc, Kind::Hover).then_some(())
    });
    // Without a server, the plugin's formatter command formats.
    kalem_core::languages::set_user_settings(Some(&serde_json::json!({
        "org.example.fake": {"server": "off"}
    })));
    lsp::settings_changed();
    lsp::sync(&doc);
    edit(&mut doc, 0..0, "a  b ");
    assert!(!lsp::can(&doc, Kind::Format) && lsp::has_format_command(&doc));
    lsp::format_with_command(&doc).unwrap();
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
            assert!(
                doc.text().as_str().starts_with("a b "),
                "{:?}",
                doc.text().as_str()
            );
        }
        o => panic!("{o:?}"),
    }
    edit(&mut doc, 0..4, "");
    kalem_core::languages::set_user_settings(None);
    lsp::settings_changed();
    lsp::sync(&doc);
    until("the server back", || {
        lsp::can(&doc, Kind::Hover).then_some(())
    });
    println!("test format command ... ok");

    // A setting changed reaches the running server.
    kalem_core::languages::set_user_settings(Some(&serde_json::json!({
        "org.example.fake": {"settings": {"elixirLS": {"x": 9}}}
    })));
    lsp::settings_changed();
    until("the new settings", || {
        lsp::log(&file)
            .iter()
            .any(|l| l.contains("settings") && l.contains("\"x\":9"))
            .then_some(())
    });
    kalem_core::languages::set_user_settings(None);
    println!("test settings ... ok");

    // Problems in the text: flagged in the line's view, marked in the
    // gutter, said under the mouse; a file not open listed too.
    let mut v =
        kalem_core::view::plain_line_view(doc.text().as_str(), doc.text().line_range(0), None);
    lsp::flag_diagnostics(&doc, &mut v);
    let flagged: Vec<&str> = v
        .runs
        .iter()
        .filter(|r| r.style.flagged == Some(true))
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(flagged, ["TODO", "bad"], "{:?}", v.runs);
    assert_eq!(lsp::line_mark(&doc, 0), Some(lsp::Severity::Error));
    assert_eq!(lsp::line_mark(&doc, 1), None);
    let bad = doc.text().as_str().find("bad").unwrap();
    assert_eq!(
        lsp::diagnostic_at(&doc, bad + 1).as_deref(),
        Some("fake: bad found")
    );
    std::fs::write(file.with_file_name("other.fk"), "bad\n").unwrap();
    let n = doc.text().len();
    edit(&mut doc, n..n, "PROJECT");
    until("the project's problems", || {
        lsp::all_problems()
            .iter()
            .any(|p| p.path.ends_with("other.fk") && p.preview.contains("bad found"))
            .then_some(())
    });
    edit(&mut doc, n..n + "PROJECT".len(), "");
    println!("test problems in the text ... ok");

    // Typing a call's `(` shows its signature; its `)` closes it.
    let n = doc.text().len();
    edit(&mut doc, n..n, "greet");
    edit(&mut doc, n + 5..n + 5, "(");
    match outcome(&file, doc.version()) {
        Outcome::Signature { text: Some(t), .. } => {
            assert!(
                t.contains("greet(name, greeting)") && t.contains("Who.") && t.contains("Greets."),
                "{t}"
            );
        }
        o => panic!("{o:?}"),
    }
    let n = doc.text().len();
    edit(&mut doc, n..n, ")");
    assert!(matches!(
        lsp::take_outcomes(&file, doc.version()).as_slice(),
        [Outcome::Signature { text: None, .. }]
    ));
    let n = doc.text().len();
    edit(&mut doc, n - "greet()".len()..n, "");
    println!("test signature ... ok");

    // A file changed outside the editor is told to the server; build
    // output is not.
    std::fs::create_dir_all(file.parent().unwrap().join("_build")).unwrap();
    std::fs::write(file.parent().unwrap().join("_build/out.fk"), "x").unwrap();
    std::fs::write(file.with_file_name("new.fk"), "x").unwrap();
    until("the watched change", || {
        lsp::log(&file)
            .iter()
            .any(|l| l.starts_with("watched") && l.contains("new.fk"))
            .then_some(())
    });
    assert!(!lsp::log(&file).iter().any(|l| l.contains("out.fk")));
    println!("test watched files ... ok");

    // Saved under another name: the server has the document once, under
    // the new name.
    let renamed = file.with_file_name("b.fk");
    doc.save_as(&renamed, kalem_core::Config::default().save_options())
        .unwrap();
    lsp::sync(&doc);
    assert!(lsp::serves(&doc));
    assert!(
        lsp::report()[0].contains("1 documents"),
        "{:?}",
        lsp::report()
    );
    assert!(lsp::diagnostics(&file).is_empty(), "the old name is closed");
    let file = renamed;
    println!("test save as ... ok");

    // Rename: a list to confirm, then the open document edited by its
    // editor and the file not open written.
    let other = file.with_file_name("other.fk");
    std::fs::write(&other, "bad\n").unwrap();
    let take_id = |item: &kalem_core::palette::PaletteItem| {
        kalem_core::palette::split_invocation(&item.id).1["id"]
            .as_u64()
            .unwrap()
    };
    lsp::rename(&doc, "good").unwrap();
    let items = match outcome(&file, doc.version()) {
        Outcome::Choose(items) => items,
        o => panic!("{o:?}"),
    };
    assert!(items[0].title.contains("in 2 files"), "{}", items[0].title);
    let said = lsp::apply_plan(take_id(&items[0])).unwrap();
    assert!(said.contains("1 files written"), "{said}");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "good\n");
    match outcome(&file, doc.version()) {
        Outcome::Edits { edits, label, .. } => {
            let tx = lsp::transaction(&edits, &label).unwrap();
            doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
            lsp::sync(&doc);
        }
        o => panic!("{o:?}"),
    }
    assert!(doc.text().as_str().contains("good") && !doc.text().as_str().contains("bad"));
    println!("test rename ... ok");

    // Code actions: one with its edit, one whose command makes the
    // server ask for an edit.
    lsp::code_actions(&doc).unwrap();
    let items = match outcome(&file, doc.version()) {
        Outcome::Choose(items) => items,
        o => panic!("{o:?}"),
    };
    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["Mark the start", "Run a command"]);
    lsp::run_offer(take_id(&items[0])).unwrap();
    for expect in ["@", "#"] {
        if expect == "#" {
            lsp::run_offer(take_id(&items[1])).unwrap();
        }
        match outcome(&file, doc.version()) {
            Outcome::Edits { edits, label, .. } => {
                let tx = lsp::transaction(&edits, &label).unwrap();
                doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
                lsp::sync(&doc);
            }
            o => panic!("{o:?}"),
        }
        assert!(
            doc.text().as_str().starts_with(expect),
            "{:?}",
            doc.text().as_str()
        );
    }
    until("the server's edit answered", || {
        lsp::log(&file)
            .iter()
            .any(|l| l == "applied true")
            .then_some(())
    });
    edit(&mut doc, 0..2, "");
    println!("test code actions ... ok");

    // Outside a project: no server, and the reason said.
    let loose = dir.join("loose.fk");
    std::fs::write(&loose, "x").unwrap();
    let loose_doc = DocumentState::open(
        &loose,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .unwrap();
    lsp::sync(&loose_doc);
    assert!(!lsp::serves(&loose_doc));
    let why = lsp::request(&loose_doc, Kind::Hover).unwrap_err();
    assert!(why.contains("outside a project"), "{why}");
    println!("test outside a project ... ok");

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
