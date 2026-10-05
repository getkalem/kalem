//! Builds Kalem's bundled plugins as WebAssembly components (wasm_todo
//! W4) when the feature `build` is on: the sources of `getkalem/plugins`
//! at the revision Kalem's `Cargo.toml` pins (Cargo has them), compiled
//! for `wasm32-unknown-unknown` against this checkout's `kalem-plugin`
//! and `kalem-viewer`, wrapped by wit-component, a component importing
//! WASI refused. The plugins' workspace is copied under the target folder
//! with the patch to this checkout, so the copy Cargo keeps is not
//! touched; files are written only when they changed, so Cargo builds
//! again only what did.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The bundled plugins, by package name.
const PLUGINS: &[&str] = &[
    "kalem-plugin-image-viewer",
    "kalem-plugin-pdf-viewer",
    "kalem-plugin-xlsx",
];

/// WebAssembly's SIMD, which Wasmtime compiles, as `kalem plugin build`
/// adds it (a PDF page in 39 ms with it, 52 without: D28's record).
const SIMD: &str = "target.wasm32-unknown-unknown.rustflags=[\"-C\", \"target-feature=+simd128\"]";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let list = out.join("components.rs");
    if std::env::var_os("CARGO_FEATURE_BUILD").is_none() {
        write_if_changed(&list, b"const COMPONENTS: &[Component] = &[];\n");
        return;
    }
    let here = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let kalem = here
        .parent()
        .and_then(Path::parent)
        .expect("the workspace")
        .to_path_buf();
    for dep in [
        "Cargo.lock",
        "crates/kalem-plugin/Cargo.toml",
        "crates/kalem-plugin/src",
        "crates/kalem-plugin/wit",
        "crates/kalem-viewer/Cargo.toml",
        "crates/kalem-viewer/src",
    ] {
        println!("cargo:rerun-if-changed={}", kalem.join(dep).display());
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let sources = plugin_sources(&cargo, &kalem);
    let root = sources
        .values()
        .next()
        .and_then(|d| d.parent())
        .and_then(Path::parent)
        .expect("the plugins' workspace")
        .to_path_buf();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| kalem.join("target"), PathBuf::from)
        .join("kalem-components");
    let ws = target.join("plugins");
    workspace(&root, &sources, &kalem, &ws);
    let modules = build(&cargo, &ws, &target.join("target"));
    let mut rs = String::from("const COMPONENTS: &[Component] = &[\n");
    for (name, module) in modules {
        let dir = &sources[&name];
        let component = wrap(&name, &module);
        let file = out.join(format!("{name}.wasm"));
        write_if_changed(&file, &component);
        let manifest = dir.join("plugin.json");
        let id = std::fs::read_to_string(&manifest)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|m| m["id"].as_str().map(str::to_string))
            .unwrap_or_else(|| panic!("{}: no id", manifest.display()));
        rs.push_str(&format!(
            "    Component {{ id: {id:?}, manifest: include_str!({:?}), bytes: include_bytes!({:?}) }},\n",
            manifest.display().to_string(),
            file.display().to_string()
        ));
    }
    rs.push_str("];\n");
    write_if_changed(&list, rs.as_bytes());
}

/// Each bundled plugin's folder in the sources Cargo keeps, by package.
fn plugin_sources(cargo: &str, kalem: &Path) -> std::collections::BTreeMap<String, PathBuf> {
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .arg("--manifest-path")
        .arg(kalem.join("Cargo.toml"))
        .stderr(Stdio::inherit())
        .output()
        .expect("cargo metadata");
    assert!(out.status.success(), "cargo metadata failed");
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("metadata JSON");
    let mut found = std::collections::BTreeMap::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or_default();
        if PLUGINS.contains(&name)
            && let Some(dir) = p["manifest_path"]
                .as_str()
                .and_then(|m| Path::new(m).parent())
        {
            found.insert(name.to_string(), dir.to_path_buf());
        }
    }
    assert_eq!(
        found.len(),
        PLUGINS.len(),
        "the bundled plugins in Cargo's metadata"
    );
    found
}

/// The plugins' workspace copied to `ws`: its manifest with the bundled
/// plugins as its members and Kalem's crates patched to this checkout, its
/// lock file, its shims, and the plugins' folders.
fn workspace(
    root: &Path,
    sources: &std::collections::BTreeMap<String, PathBuf>,
    kalem: &Path,
    ws: &Path,
) {
    let manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).expect("the plugins' Cargo.toml");
    let members: Vec<String> = sources
        .values()
        .map(|d| {
            let rel = d.strip_prefix(root).expect("a plugin inside its workspace");
            format!("{:?}", rel.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    let mut text = String::new();
    for line in manifest.lines() {
        if line.trim_start().starts_with("members") {
            text.push_str(&format!("members = [{}]\n", members.join(", ")));
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
    let path = |p: &str| kalem.join(p).display().to_string().replace('\\', "/");
    text.push_str(&format!(
        "\n[patch.\"https://github.com/getkalem/kalem\"]\nkalem-viewer = {{ path = {:?} }}\nkalem-plugin = {{ path = {:?} }}\n",
        path("crates/kalem-viewer"),
        path("crates/kalem-plugin")
    ));
    write_if_changed(&ws.join("Cargo.toml"), text.as_bytes());
    if let Ok(lock) = std::fs::read(root.join("Cargo.lock")) {
        // Only when the plugins' own lock moved: Cargo rewrites the copy
        // for the patch, which must not be undone at every build.
        let marker = ws.join(".source-lock");
        if std::fs::read(&marker).ok().as_deref() != Some(&lock[..]) {
            write_if_changed(&ws.join("Cargo.lock"), &lock);
            write_if_changed(&marker, &lock);
        }
    }
    copy_dir(&root.join("shims"), &ws.join("shims"));
    for dir in sources.values() {
        let rel = dir
            .strip_prefix(root)
            .expect("a plugin inside its workspace");
        copy_dir(dir, &ws.join(rel));
    }
}

/// Compiles the plugins in `ws` for WebAssembly: each package's module.
/// One package a build, as `kalem plugin build` does: built together,
/// Cargo would unify their features, and the image viewer would export
/// the workbook's `grid` too.
fn build(cargo: &str, ws: &Path, target_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut modules = Vec::new();
    for p in PLUGINS {
        let mut c = Command::new(cargo);
        c.args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--message-format=json-render-diagnostics",
            "--config",
            SIMD,
        ])
        .arg("--manifest-path")
        .arg(ws.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target_dir)
        .args(["-p", p]);
        // The outer build's flags are for the host, not for WebAssembly.
        for var in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_DIR",
            "CARGO_BUILD_TARGET",
        ] {
            c.env_remove(var);
        }
        let out = c
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .expect("cargo build of a plugin");
        assert!(
            out.status.success(),
            "building {p} for wasm32-unknown-unknown failed (is the target installed? `rustup target add wasm32-unknown-unknown`)"
        );
        let wasm = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["reason"] == "compiler-artifact")
            .filter(|v| {
                v["target"]["name"]
                    .as_str()
                    .map(|n| n.replace('_', "-"))
                    .as_deref()
                    == Some(*p)
            })
            .flat_map(|v| {
                v["filenames"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|f| f.as_str().map(PathBuf::from))
            })
            .find(|f| f.extension().is_some_and(|e| e == "wasm"))
            .unwrap_or_else(|| panic!("{p}: no .wasm built"));
        modules.push(((*p).to_string(), wasm));
    }
    modules
}

/// The module at `path` wrapped as a component; one importing WASI (built
/// for the wrong target) refused, as Kalem grants none of it.
fn wrap(name: &str, path: &Path) -> Vec<u8> {
    let module = std::fs::read(path).expect("the module");
    let component = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .and_then(|mut e| e.encode())
        .unwrap_or_else(|e| panic!("{name}: not a component: {e:#}"));
    for payload in wasmparser::Parser::new(0).parse_all(&component) {
        if let Ok(wasmparser::Payload::ComponentImportSection(imports)) = payload {
            for import in imports.into_iter().flatten() {
                assert!(
                    !import.name.0.starts_with("wasi:"),
                    "{name} imports {}, which Kalem does not grant",
                    import.name.0
                );
            }
        }
    }
    component
}

/// Copies `from` into `to`, leaving out build outputs, writing only the
/// files that changed.
fn copy_dir(from: &Path, to: &Path) {
    let Ok(entries) = std::fs::read_dir(from) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        if matches!(name.to_str(), Some("target" | "dist")) {
            continue;
        }
        let Ok(kind) = e.file_type() else {
            continue;
        };
        if kind.is_dir() {
            copy_dir(&e.path(), &to.join(&name));
        } else if kind.is_file()
            && let Ok(bytes) = std::fs::read(e.path())
        {
            write_if_changed(&to.join(&name), &bytes);
        }
    }
}

fn write_if_changed(path: &Path, bytes: &[u8]) {
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        return;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("a folder for the build's files");
    }
    std::fs::write(path, bytes).expect("a build file");
}
