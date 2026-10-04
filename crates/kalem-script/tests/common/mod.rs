//! Building the test plugins of `tests/plugins`.

use std::path::Path;
use std::process::Command;

/// The test plugin `name` of `tests/plugins` built and wrapped as a
/// component, or `None` without the tools.
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
    let out = std::env::temp_dir().join(format!("kalem-script-{name}-{}", std::process::id()));
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
