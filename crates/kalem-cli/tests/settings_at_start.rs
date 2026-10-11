//! A plugin's own setting reaches it when the editor reads its settings
//! after the plugin started: `kalem` starts the installed plugins on a
//! thread before the editor reads `settings.toml`, so a plugin reading its
//! settings as it starts reads Kalem's defaults, and the settings the
//! editor gives first are a change from them. Its own process, the
//! plugins started once. Skipped where the `wasm32-unknown-unknown` target
//! or `wasm-tools` is not installed.

use std::time::{Duration, Instant};

use kalem_core::Config;

mod test_plugins;
use test_plugins::component;

#[test]
fn settings_read_after_the_plugins_started_reach_their_watches() {
    let Some(bytes) = component("counter") else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let plugin = root.join("config/plugins/org.test.counter");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("counter.wasm"), bytes).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"id": "org.test.counter", "name": "Counter", "main": "counter.wasm",
            "limits": {"time_ms": 5000}}"#,
    )
    .unwrap();
    let settings = root.join("config/settings.toml");
    std::fs::write(
        &settings,
        "[plugins.\"org.test.counter\"]\ngreeting = \"hi\"\n",
    )
    .unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }

    // As `kalem tui`: the plugins first, the settings once they run.
    kalem_cli::bundled_plugins();
    let start = Instant::now();
    while !kalem_core::extensions::known("counter.count") {
        assert!(start.elapsed() < Duration::from_secs(120), "never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Until then, Kalem's defaults.
    assert_eq!(
        kalem_core::extensions::setting(&["editor", "keymap_profile"]),
        Config::default()
            .get_path(&["editor", "keymap_profile"])
            .cloned()
    );
    assert_eq!(
        kalem_core::extensions::setting(&["plugins", "org.test.counter", "greeting"]),
        None
    );
    kalem_core::jobs::take_notices();
    kalem_core::extensions::set_config(&Config::load(Some(&settings), None));
    // Told now if the plugins were installed, or when they are.
    let start = Instant::now();
    loop {
        let notices = kalem_core::jobs::take_notices();
        if notices
            .iter()
            .any(|(t, _)| t == "counter: greeting changed")
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "the plugin never heard of its setting: {notices:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&root);
}
