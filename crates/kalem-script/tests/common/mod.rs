//! Building the test plugins of `tests/plugins`.

use std::path::Path;
use std::process::Command;

/// The test plugin `name` of `tests/plugins` built and wrapped as a
/// component, or `None` without the tools.
pub(crate) fn component(name: &str) -> Option<Vec<u8>> {
    // Built once per test process: the tests running in parallel would
    // otherwise write the same files at once.
    static BUILT: std::sync::Mutex<std::collections::BTreeMap<String, Option<Vec<u8>>>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());
    let mut built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(b) = built.get(name) {
        return b.clone();
    }
    let b = build(name);
    built.insert(name.to_string(), b.clone());
    b
}

fn build(name: &str) -> Option<Vec<u8>> {
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
    // not one per process left in the temporary folder; a process builds
    // and wraps under its lock, so none reads what another is writing.
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kalem-script-plugins");
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
    let module = out.join(format!(
        "wasm32-unknown-unknown/release/kalem_plugin_{name}.wasm"
    ));
    let component = out.join(format!("{name}.wasm"));
    let ok = Command::new("wasm-tools")
        .args(["component", "new"])
        .arg(&module)
        .arg("-o")
        .arg(&component)
        .status()
        .ok()?
        .success();
    assert!(ok, "{name} wraps");
    std::fs::read(component).ok()
}
