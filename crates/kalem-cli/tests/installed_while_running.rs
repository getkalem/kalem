//! An extension plugin installed while Kalem runs starts without a
//! restart: told of the documents open, as if it had started with Kalem;
//! removed, it stops, what it registered taken back; installed again, it
//! starts again. Its own process, the plugins started once. Skipped where
//! the `wasm32-unknown-unknown` target or `wasm-tools` is not installed.

use std::time::{Duration, Instant};

use kalem_core::events::{DocumentId, Event, EventBus};
use serde_json::Value;

mod test_plugins;
use test_plugins::component;

/// Waits until `known` is what command `id` being known is.
fn until_known(id: &str, known: bool) {
    let start = Instant::now();
    while kalem_core::extensions::known(id) != known {
        assert!(start.elapsed() < Duration::from_secs(120), "{id} {known}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What the plugin says it heard opened.
fn opened() -> String {
    kalem_core::jobs::take_notices();
    kalem_core::extensions::run("counter.opened", &Value::Null).unwrap();
    let notices = kalem_core::jobs::take_notices();
    notices
        .iter()
        .find_map(|(t, _)| t.strip_prefix("counter: opened "))
        .unwrap_or_else(|| panic!("{notices:?}"))
        .to_string()
}

#[test]
fn a_plugin_installed_while_kalem_runs_starts_at_once() {
    let Some(bytes) = component("counter") else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("config/plugins")).unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }
    let install = || {
        let plugin = root.join("config/plugins/org.test.counter");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("counter.wasm"), &bytes).unwrap();
        std::fs::write(
            plugin.join("plugin.json"),
            r#"{"id": "org.test.counter", "name": "Counter", "main": "counter.wasm",
                "version": "1.0.0", "limits": {"time_ms": 5000}}"#,
        )
        .unwrap();
    };

    // Kalem started with no plugin; documents opened, one closed again.
    kalem_cli::bundled_plugins();
    let mut bus = EventBus::new();
    bus.subscribe(None, kalem_core::extensions::event);
    let open = |n: u64, name: &str| Event::DocumentOpen {
        doc: DocumentId(n),
        path: Some(root.join(name)),
    };
    bus.emit(&open(1, "closed.org"));
    bus.emit(&open(2, "kept.org"));
    bus.emit(&Event::DocumentClose { doc: DocumentId(1) });
    bus.emit(&Event::AppReady);
    assert!(!kalem_core::extensions::known("counter.count"));

    // Installed: it starts, told of the document open.
    install();
    kalem_cli::plugins_changed();
    until_known("counter.count", true);
    let kept = root.join("kept.org").to_string_lossy().into_owned();
    assert_eq!(opened(), kept);
    // And hears what follows.
    bus.emit(&open(3, "new.org"));
    let new = root.join("new.org").to_string_lossy().into_owned();
    assert_eq!(opened(), format!("{kept},{new}"));

    // Removed: stopped, its commands gone.
    std::fs::remove_dir_all(root.join("config/plugins/org.test.counter")).unwrap();
    kalem_cli::plugins_changed();
    until_known("counter.count", false);
    assert!(kalem_core::extensions::run("counter.opened", &Value::Null).is_err());

    // Installed again: started again, a new instance told of both.
    install();
    kalem_cli::plugins_changed();
    until_known("counter.count", true);
    assert_eq!(opened(), format!("{kept},{new}"));
    let notices = kalem_core::jobs::take_notices();
    assert!(
        !notices.iter().any(|(_, error)| *error),
        "no failure: {notices:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
