//! A plugin started by the files it works with without serving them
//! (`"activation": ["onOpen:*.csv"]`): not with Kalem, nor with a file of
//! another name, but with the first whose name the pattern matches,
//! letters in any case. Its own process: it sets Kalem's folders. Skipped
//! where the `wasm32-unknown-unknown` target or `wasm-tools` is not
//! installed.

use std::time::{Duration, Instant};

use kalem_core::events::{DocumentId, Event, EventBus};

mod test_plugins;
use test_plugins::component;

#[test]
fn a_plugin_started_by_a_files_name() {
    let (Some(counter), Some(reach)) = (component("counter"), component("reach")) else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-on-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let install = |id: &str, wasm: &[u8], manifest: &str| {
        let dir = root.join("config/plugins").join(format!("org.test.{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.wasm")), wasm).unwrap();
        std::fs::write(dir.join("plugin.json"), manifest).unwrap();
    };
    install(
        "counter",
        &counter,
        r#"{"id": "org.test.counter", "name": "Counter", "main": "counter.wasm",
            "activation": ["onOpen:*.csv"], "limits": {"time_ms": 5000}}"#,
    );
    // Started with Kalem: once its command is known, the plugins are.
    install(
        "reach",
        &reach,
        r#"{"id": "org.test.reach", "name": "Reach", "main": "reach.wasm",
            "activation": ["onStartup"],
            "permissions": ["fs:read:workspace", "net:fetch:127.0.0.1"]}"#,
    );
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }
    kalem_cli::bundled_plugins();
    let start = Instant::now();
    while !kalem_core::extensions::known("reach.last") {
        assert!(start.elapsed() < Duration::from_secs(120), "never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!kalem_core::extensions::known("counter.count"));

    let mut bus = EventBus::new();
    bus.subscribe(None, kalem_core::extensions::event);
    let open = |name: &str| Event::DocumentOpen {
        doc: DocumentId(2),
        path: Some(root.join(name)),
    };
    bus.emit(&open("notes.txt"));
    bus.emit(&open("statement.csv.rules"));
    assert!(!kalem_core::extensions::known("counter.count"));
    bus.emit(&open("Statement-2026-01.CSV"));
    assert!(kalem_core::extensions::known("counter.count"));
    let _ = std::fs::remove_dir_all(root);
}
