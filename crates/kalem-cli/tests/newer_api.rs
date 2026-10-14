//! An extension plugin built for a later plugin API than this Kalem's is
//! not started, and the notice says that Kalem needs updating rather than
//! that the plugin asks for what it was not granted. Its own process, as
//! the plugins start once. Skipped where the `wasm32-unknown-unknown`
//! target or `wasm-tools` is not installed.

use std::time::{Duration, Instant};

mod test_plugins;
use test_plugins::component;

#[test]
fn a_plugin_for_a_later_api_asks_for_a_newer_kalem() {
    let Some(bytes) = component("counter") else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-newer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let plugin = root.join("config/plugins/org.test.counter");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("counter.wasm"), &bytes).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"id": "org.test.counter", "name": "Counter", "main": "counter.wasm",
            "version": "1.0.0", "api": "^0.2.99"}"#,
    )
    .unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }
    kalem_cli::bundled_plugins();
    // The notice, in the language of the process: the plugin, the API it
    // asks for and this Kalem's.
    let start = Instant::now();
    let mut notices = Vec::new();
    let why = loop {
        notices.extend(kalem_core::jobs::take_notices());
        let found = notices.iter().find(|(text, error)| {
            *error
                && text.contains("counter 1.0.0")
                && text.contains("^0.2.99")
                && text.contains(kalem_script::API_VERSION)
        });
        if let Some((text, _)) = found {
            break text.clone();
        }
        assert!(start.elapsed() < Duration::from_secs(120), "{notices:?}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!why.contains("not granted"), "{why}");
    assert!(!kalem_core::extensions::known("counter.count"));
    let _ = std::fs::remove_dir_all(&root);
}
