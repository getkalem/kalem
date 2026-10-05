//! The bundled plugins built in as components (wasm_todo W5): with the
//! feature `components`, `kalem plugin list` names them as built in, and
//! `kalem plugin check` binds each, in a configuration of its own.
#![cfg(feature = "components")]

use std::process::Command;

fn kalem(dir: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_kalem"))
        .args(args)
        .env("KALEM_CONFIG_DIR", dir.join("config"))
        .env("KALEM_STATE_DIR", dir.join("state"))
        .env("LANG", "en_US.UTF-8")
        .output()
        .expect("kalem runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn the_built_in_components_run() {
    let dir = std::env::temp_dir().join(format!("kalem-components-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (_, list) = kalem(&dir, &["plugin", "list"]);
    for id in [
        "org.kalem.image-viewer",
        "org.kalem.pdf-viewer",
        "org.kalem.xlsx",
    ] {
        assert!(list.contains(id) && list.contains("(built in)"), "{list}");
    }
    let (ok, check) = kalem(&dir, &["plugin", "check"]);
    assert!(ok, "{check}");
    assert_eq!(check.matches("(built in): runs").count(), 3, "{check}");
    let _ = std::fs::remove_dir_all(&dir);
}
