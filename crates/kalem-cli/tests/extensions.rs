//! An extension plugin installed and loaded as the editors load it
//! (T3.1.12): the plugin of `tests/plugins/counter` in a plugin folder,
//! started by `bundled_plugins`, its commands in the registry the editors
//! build and run through it, its subscription vetoing a save on the
//! editors' event bus. Skipped where the `wasm32-unknown-unknown` target or
//! `wasm-tools` is not installed.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use kalem_core::command::{Clipboard, CommandSource, EditorContext};
use kalem_core::events::{DocumentId, Event, EventBus, EventKind};
use kalem_core::{CommandRegistry, Config, DocumentMode, DocumentState};
use serde_json::Value;

/// The counter plugin built and wrapped as a component, or `None` without
/// the tools.
fn counter() -> Option<Vec<u8>> {
    let target = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()?;
    if !String::from_utf8_lossy(&target.stdout).contains("wasm32-unknown-unknown") {
        return None;
    }
    Command::new("wasm-tools").arg("--version").output().ok()?;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/plugins/counter");
    let out = std::env::temp_dir().join(format!("kalem-cli-counter-{}", std::process::id()));
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
        ])
        .arg(dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&out)
        .status()
        .ok()?
        .success();
    assert!(ok, "counter builds");
    let component = out.join("counter.wasm");
    let ok = Command::new("wasm-tools")
        .args(["component", "new"])
        .arg(out.join("wasm32-unknown-unknown/release/kalem_plugin_counter.wasm"))
        .arg("-o")
        .arg(&component)
        .status()
        .ok()?
        .success();
    assert!(ok, "counter wraps");
    std::fs::read(component).ok()
}

#[test]
fn an_installed_plugin_adds_commands_the_registry_runs() {
    let Some(bytes) = counter() else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-ext-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let plugin = root.join("config/plugins/org.test.counter");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("counter.wasm"), bytes).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"id": "org.test.counter", "name": "Counter", "main": "counter.wasm"}"#,
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
    let mut bus = EventBus::new();
    bus.subscribe(None, kalem_core::extensions::event);
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

    let _ = std::fs::remove_dir_all(&root);
}
