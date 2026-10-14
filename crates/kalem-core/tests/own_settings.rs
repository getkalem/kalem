//! A plugin's own setting with a dotted key (`programs.python3`): written
//! in tables within the plugin's own, where the settings page and
//! `find_program` read it, and read back from there.

use kalem_core::{Config, extensions};
use serde_json::json;

#[test]
fn dotted_own_settings_are_tables_within_the_plugins() {
    let root = std::env::temp_dir().join(format!("kalem-own-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("config")).unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variable before any thread reads it.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
    }
    extensions::set_own_setting("org.test.p", "programs.python3", &json!("/opt/python3")).unwrap();
    extensions::set_own_setting("org.test.p", "plain", &json!(true)).unwrap();
    let path = root.join("config/settings.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("\"programs.python3\""), "{text}");
    extensions::set_config(&Config::load(Some(&path), None));
    assert_eq!(
        extensions::own_setting("org.test.p", "programs.python3"),
        Some(json!("/opt/python3"))
    );
    assert_eq!(
        extensions::setting(&["plugins", "org.test.p", "programs", "python3"]),
        Some(json!("/opt/python3"))
    );
    assert_eq!(
        extensions::own_setting("org.test.p", "plain"),
        Some(json!(true))
    );
    let _ = std::fs::remove_dir_all(&root);
}
