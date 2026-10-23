//! The plugins of `tests/plugins`, built from their source and wrapped
//! as components for kalem-cli's tests (`extensions`, `plugin_check`,
//! `run`, `settings_at_start`, `viewer_choice`), and their permissions
//! approved.

use std::path::Path;
use std::process::Command;

/// The test plugin `name` built and wrapped as a component, or `None`
/// without the tools.
pub(crate) fn component(name: &str) -> Option<Vec<u8>> {
    let target = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()?;
    if !String::from_utf8_lossy(&target.stdout).contains("wasm32-unknown-unknown") {
        return None;
    }
    Command::new("wasm-tools").arg("--version").output().ok()?;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/plugins")
        .join(name);
    // One folder in the target directory for every test process and run,
    // not one per process left in the temporary folder (some 150 MB
    // each); a process builds and wraps under its lock, so none reads
    // what another is writing.
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kalem-cli-plugins");
    std::fs::create_dir_all(&out).ok()?;
    let lock = std::fs::File::create(out.join("lock")).ok()?;
    lock.lock().ok()?;
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
    assert!(ok, "{name} builds");
    let component = out.join(format!("{name}.wasm"));
    let ok = Command::new("wasm-tools")
        .args(["component", "new"])
        .arg(out.join(format!(
            "wasm32-unknown-unknown/release/kalem_plugin_{name}.wasm"
        )))
        .arg("-o")
        .arg(&component)
        .status()
        .ok()?
        .success();
    assert!(ok, "{name} wraps");
    std::fs::read(component).ok()
}

/// Records plugin `id` in the `plugins.toml` of the settings folder
/// `config` as Kalem's installer does, with the `permissions` the user
/// approved: the only ones the plugin is granted, whatever its manifest
/// asks for (one put in the folder without a record is granted none).
#[allow(dead_code)]
pub(crate) fn approve(config: &Path, id: &str, permissions: &[&str]) {
    use std::io::Write;
    std::fs::create_dir_all(config).unwrap();
    let mut record = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config.join("plugins.toml"))
        .unwrap();
    writeln!(
        record,
        "[plugins.\"{id}\"]\npermissions = {permissions:?}\n"
    )
    .unwrap();
}
