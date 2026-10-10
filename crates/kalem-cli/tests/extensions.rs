//! An extension plugin installed and loaded as the editors load it
//! (T3.1.12): the plugin of `tests/plugins/counter` in a plugin folder,
//! started by `bundled_plugins`, its commands in the registry the editors
//! build and run through it, its subscription vetoing a save on the
//! editors' event bus. Skipped where the `wasm32-unknown-unknown` target or
//! `wasm-tools` is not installed.

use std::path::Path;
use std::time::{Duration, Instant};

use kalem_core::command::{Clipboard, CommandSource, EditorContext};
use kalem_core::events::{DocumentId, Event, EventBus, EventKind};
use kalem_core::{CommandRegistry, Config, DocumentMode, DocumentState};
use serde_json::Value;

mod test_plugins;
use test_plugins::component;

#[test]
fn an_installed_plugin_adds_commands_the_registry_runs() {
    let (Some(bytes), Some(reach)) = (component("counter"), component("reach")) else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-ext-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let plugin = root.join("config/plugins/org.test.counter");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("counter.wasm"), bytes).unwrap();
    // Its commands write the settings file: a budget a slow CI runner
    // keeps to (the default 100 ms was passed on Windows).
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"id": "org.test.counter", "name": "Counter", "main": "counter.wasm",
            "limits": {"time_ms": 5000}}"#,
    )
    .unwrap();
    // A plugin reaching files and the network, as its permissions allow:
    // the project's folder, and this machine's address. With no
    // `activation`, it starts with the first file opened that it declares
    // it serves (a text file in a folder holding `notes.txt`).
    let reach_dir = root.join("config/plugins/org.test.reach");
    std::fs::create_dir_all(&reach_dir).unwrap();
    std::fs::write(reach_dir.join("reach.wasm"), reach).unwrap();
    std::fs::write(
        reach_dir.join("plugin.json"),
        r#"{"id": "org.test.reach", "name": "Reach", "main": "reach.wasm",
            "permissions": ["fs:read:workspace", "net:fetch:127.0.0.1"],
            "applies": {"extensions": [".txt"], "markers": ["notes.txt"]}}"#,
    )
    .unwrap();
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("notes.txt"), "in the project").unwrap();
    std::fs::write(
        root.join("config/projects.toml"),
        format!("[[project]]\npath = {:?}\n", project.to_string_lossy()),
    )
    .unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }

    let before = kalem_core::extensions::generation();
    kalem_cli::bundled_plugins();
    // Started on a thread: its commands arrive.
    let start = Instant::now();
    while !kalem_core::extensions::known("counter.count") {
        let notices = kalem_core::jobs::take_notices();
        assert!(notices.is_empty(), "{notices:?}");
        assert!(start.elapsed() < Duration::from_secs(120), "never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(kalem_core::extensions::generation() > before);

    // The editors' bus hands its events to the plugins.
    let mut bus = EventBus::new();
    bus.subscribe(None, kalem_core::extensions::event);
    // The plugin that serves some files starts with the first of them, not
    // with another file, nor with Kalem.
    assert!(!kalem_core::extensions::known("reach.last"));
    let open = |path: std::path::PathBuf| Event::DocumentOpen {
        doc: DocumentId(2),
        path: Some(path),
    };
    bus.emit(&open(root.join("todo.txt")));
    bus.emit(&open(project.join("plan.org")));
    assert!(!kalem_core::extensions::known("reach.last"));
    bus.emit(&open(project.join("draft.txt")));
    let start = Instant::now();
    while !kalem_core::extensions::known("reach.last") {
        assert!(start.elapsed() < Duration::from_secs(120), "never started");
        std::thread::sleep(Duration::from_millis(20));
    }

    let registry = CommandRegistry::with_builtins();
    let count = registry.get("counter.count").expect("registered");
    assert_eq!(count.source, CommandSource::Plugin("counter".into()));
    assert_eq!(count.title, "Count");
    let keymap = kalem_core::Keymap::build(&registry, kalem_core::Profile::Word, &[]);
    assert!(
        !keymap.0.keys_for("counter.count").is_empty(),
        "its default keys and binding"
    );

    // Run through the registry; another command it asks for runs right
    // after it, in the same context.
    let meta = kalem_core::Metadata {
        path: None,
        mode: DocumentMode::Org,
        line_ending: kalem_core::LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    let mut doc = DocumentState::new("* Task\n", meta, std::sync::Arc::default());
    let mut clipboard = Clipboard::default();
    let config = Config::default();
    let mut ctx = EditorContext::new(
        Some(&mut doc),
        &mut clipboard,
        &config,
        Instant::now(),
        jiff::civil::date(2026, 10, 4).at(10, 0, 0, 0),
    );
    registry
        .execute("counter.count", &mut ctx, &Value::Null)
        .unwrap();
    registry
        .execute("counter.cycle", &mut ctx, &Value::Null)
        .unwrap();
    drop(ctx);
    assert_eq!(doc.text().as_str(), "* TODO Task\n");

    // The editors' bus hands its events to the plugin, which vetoes a save
    // to a lock file.
    let save = |path: &str| Event::DocumentBeforeSave {
        doc: DocumentId(1),
        path: path.into(),
    };
    assert!(
        bus.emit_vetoable(&save("a.org"), Instant::now())
            .wait()
            .allowed()
    );
    let vetoed = bus.emit_vetoable(&save("x.lock"), Instant::now()).wait();
    assert!(!vetoed.allowed());
    assert_eq!(EventKind::DocumentBeforeSave.name(), "document:before-save");

    // The document: read as the command found it, edited when it returns,
    // as one undo step.
    let original = "* TODO Write report\nSome text.\n** Sub item\n* TODO Call Ada :home:\n* Numbers\n| a | b |\n|---+---|\n| 1 | 2 |\n#+TBLFM: $2=$1*10\n";
    let meta = kalem_core::Metadata {
        path: None,
        mode: DocumentMode::Org,
        line_ending: kalem_core::LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    let mut doc = DocumentState::new(original, meta, std::sync::Arc::default());
    let mut clipboard = Clipboard::default();
    let mut ctx = EditorContext::new(
        Some(&mut doc),
        &mut clipboard,
        &config,
        Instant::now(),
        jiff::civil::date(2026, 10, 4).at(10, 0, 0, 0),
    );
    kalem_core::jobs::take_notices();
    registry
        .execute("counter.organize", &mut ctx, &Value::Null)
        .unwrap();
    drop(ctx);
    let notices = kalem_core::jobs::take_notices();
    let read = &notices.last().expect("what it read").0;
    assert!(read.starts_with("counter: org 4 headlines"), "{read}");
    assert!(read.contains("[\"TODO\", \"DONE\"]"), "{read}");
    assert!(read.contains("body \"Some text.\\n\""), "{read}");
    assert!(
        read.contains("rows [[\"a\", \"b\"], [\"1\", \"2\"]]"),
        "{read}"
    );
    assert!(read.contains("formulas [\"$2=$1*10\"]"), "{read}");
    assert!(read.contains("id 36"), "{read}");
    assert!(read.contains("No headline starts at 999999"), "{read}");
    let text = doc.text().as_str().to_string();
    assert!(
        text.starts_with("#+TITLE: Plan\n* DONE Write report"),
        "{text}"
    );
    assert!(text.contains(":work:"), "{text}");
    assert!(
        text.contains(":EFFORT:   1h") || text.contains(":EFFORT: 1h"),
        "{text}"
    );
    assert!(text.contains(":ID:"), "{text}");
    assert!(text.contains("* DONE Call Grace"), "{text}");
    assert!(text.contains("| 3 | 30 |"), "{text}");
    doc.undo();
    assert_eq!(doc.text().as_str(), original, "one undo step");

    // Its panel, registered and filled at activation.
    let labels = || -> Vec<String> {
        let p = kalem_core::extensions::panel("counter.panel").expect("its panel");
        p.lines()
            .into_iter()
            .map(|(i, _)| kalem_core::extensions::widget_text(&p.widgets[i]))
            .collect()
    };
    assert_eq!(labels()[1], "[ Add one ]");
    let before = labels()[0].clone();
    // A click, through the command the palette's list of panel actions
    // runs: the plugin hears it and fills the panel again.
    let shown = kalem_core::extensions::shown();
    let mut ctx = EditorContext::new(
        None,
        &mut clipboard,
        &config,
        Instant::now(),
        jiff::civil::date(2026, 10, 4).at(10, 0, 0, 0),
    );
    registry
        .execute(
            "plugin.panelEvent",
            &mut ctx,
            &serde_json::json!({ "panel": "counter.panel", "key": "add" }),
        )
        .unwrap();
    assert_ne!(labels()[0], before);
    assert!(kalem_core::extensions::shown() > shown);

    // Its questions, as the editors' requests, answered through
    // `plugin.answer`.
    registry
        .execute("counter.ask", &mut ctx, &Value::Null)
        .unwrap();
    let asked = kalem_core::extensions::take_requests();
    assert_eq!(asked.len(), 3, "{asked:?}");
    let kalem_core::Request::Choose(yes_no) = &asked[0] else {
        panic!("{:?}", asked[0]);
    };
    let (cmd, args) = kalem_core::palette::split_invocation(&yes_no[0].id);
    registry.execute(cmd, &mut ctx, &args).unwrap();
    let kalem_core::Request::Ask { command, args, arg } = &asked[1] else {
        panic!("{:?}", asked[1]);
    };
    let mut args = args.clone();
    args[arg.as_str()] = "Ada".into();
    registry.execute(command, &mut ctx, &args).unwrap();
    drop(ctx);
    let notices = kalem_core::jobs::take_notices();
    assert!(
        notices.iter().any(|(t, _)| t == "counter: yes"),
        "{notices:?}"
    );
    let (left, _) = kalem_core::extensions::status_items();
    assert!(
        left.iter().any(|i| i.id == "name" && i.text == "Ada"),
        "{left:?}"
    );

    // Settings: the editors give theirs; the plugin's own is written into
    // the user's settings under its ID, and a change reaches its watch.
    kalem_core::extensions::set_config(&Config::default());
    let mut clipboard = Clipboard::default();
    let mut ctx = EditorContext::new(
        None,
        &mut clipboard,
        &config,
        Instant::now(),
        jiff::civil::date(2026, 10, 4).at(10, 0, 0, 0),
    );
    registry
        .execute("counter.settings", &mut ctx, &Value::Null)
        .unwrap();
    assert!(
        ctx.requests.contains(&kalem_core::Request::ReloadSettings),
        "the editors read the settings again: {:?}",
        ctx.requests
    );
    let settings = root.join("config/settings.toml");
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(
        text.contains("[plugins.\"org.test.counter\"]") && text.contains("greeting = \"hello\""),
        "{text}"
    );
    kalem_core::jobs::take_notices();
    kalem_core::extensions::set_config(&Config::load(Some(&settings), None));
    let notices = kalem_core::jobs::take_notices();
    assert!(
        notices
            .iter()
            .any(|(t, _)| t == "counter: greeting changed"),
        "{notices:?}"
    );

    // Files: inside the project, and not outside it.
    let read = |ctx: &mut EditorContext<'_>, path: &Path| {
        registry.execute(
            "reach.read",
            ctx,
            &serde_json::json!({ "path": path.canonicalize().unwrap_or(path.to_path_buf()) }),
        )
    };
    read(&mut ctx, &project.join("notes.txt")).unwrap();
    assert!(read(&mut ctx, &settings).is_err(), "outside the projects");

    // The network: a server on this machine answers; the response comes
    // later, from the thread that fetched it.
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        if let Ok((mut s, _)) = server.accept() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
            );
        }
    });
    registry
        .execute(
            "reach.fetch",
            &mut ctx,
            &serde_json::json!({ "url": format!("http://127.0.0.1:{port}/x") }),
        )
        .unwrap();
    assert!(
        registry
            .execute(
                "reach.fetch",
                &mut ctx,
                &serde_json::json!({ "url": "https://example.com/" }),
            )
            .is_err(),
        "not a granted domain"
    );
    drop(ctx);
    let start = Instant::now();
    loop {
        // `reach.last` shows the last response.
        let mut clipboard = Clipboard::default();
        let mut ctx = EditorContext::new(
            None,
            &mut clipboard,
            &config,
            Instant::now(),
            jiff::civil::date(2026, 10, 4).at(10, 0, 0, 0),
        );
        registry
            .execute("reach.last", &mut ctx, &Value::Null)
            .unwrap();
        let last = kalem_core::jobs::take_notices();
        if last.iter().any(|(t, _)| t == "reach: 200 hello") {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "no response: {last:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let _ = std::fs::remove_dir_all(&root);
}
