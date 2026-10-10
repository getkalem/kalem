//! `kalem plugin check` on the installed extension plugins: the test
//! plugin of `tests/plugins/counter`, built for this plugin API, reported
//! as running, loaded as starting it would but not activated; one whose
//! file is no component reported as not running, and the check failed.
//! Skipped where the `wasm32-unknown-unknown` target or `wasm-tools` is
//! not installed.

use std::process::ExitCode;

mod test_plugins;
use test_plugins::component;

fn check() -> (Result<ExitCode, String>, String) {
    let mut out = Vec::new();
    let r = kalem_cli::plugin_check(&mut out);
    (r, String::from_utf8(out).unwrap())
}

#[test]
fn an_extension_plugin_built_for_this_api_runs() {
    let Some(bytes) = component("counter") else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let plugin = root.join("config/plugins/org.test.counter");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("counter.wasm"), bytes).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"id": "org.test.counter", "name": "Counter", "version": "0.1.0",
            "main": "counter.wasm", "activation": ["onStartup"]}"#,
    )
    .unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }

    let (r, out) = check();
    assert_eq!(r, Ok(ExitCode::SUCCESS), "{out}");
    assert!(out.contains("org.test.counter 0.1.0: runs"), "{out}");
    // Instantiated, not activated: none of its commands registered.
    assert!(!kalem_core::extensions::known("counter.count"));

    // A plugin whose file is no component cannot run: the check fails,
    // the other still reported as running.
    let broken = root.join("config/plugins/org.test.broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("broken.wasm"), b"not a component").unwrap();
    std::fs::write(
        broken.join("plugin.json"),
        r#"{"id": "org.test.broken", "name": "Broken", "version": "0.1.0",
            "main": "broken.wasm"}"#,
    )
    .unwrap();
    let (r, out) = check();
    assert_eq!(r, Ok(ExitCode::from(1)), "{out}");
    assert!(out.contains("org.test.counter 0.1.0: runs"), "{out}");
    assert!(out.contains("org.test.broken 0.1.0: cannot run: "), "{out}");
    let _ = std::fs::remove_dir_all(&root);
}
