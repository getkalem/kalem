//! Building plugins (T3.1.2): `kalem plugin new` and `kalem plugin build`.
//!
//! Kalem carries no runtime for other languages (D28): a plugin is a Rust
//! crate compiled to WebAssembly. `build` wraps the toolchain: Cargo
//! compiles the crate for `wasm32-unknown-unknown`, and `wasm-tools`
//! wraps the module as a component at the path the manifest's `main`
//! names (the tool rather than its library in Kalem: 2.3 MB of the binary
//! for what only plugin authors run; `getkalem/plugins`'s CI uses the same
//! tool). Built for that target a component imports only
//! what its WIT world names; built for `wasm32-wasip2` a Rust component
//! imports fifteen WASI interfaces it never uses, which the host does not
//! grant (D28's record).
//!
//! `new` starts a plugin: inside a checkout of `getkalem/plugins`, the
//! repository's `template/` copied to `plugins/NAME`; anywhere else, a
//! crate of its own that builds alone.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The target plugins are compiled for.
pub const TARGET: &str = "wasm32-unknown-unknown";

/// The flags `build` adds for [`TARGET`].
const SIMD: &str = "target.wasm32-unknown-unknown.rustflags=[\"-C\", \"target-feature=+simd128\"]";

/// A plugin built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// The component written.
    pub path: PathBuf,
    /// Its size in bytes.
    pub size: u64,
    /// What it imports: what it asks the host for.
    pub imports: Vec<String>,
    /// What it exports.
    pub exports: Vec<String>,
}

/// The fields of a plugin's manifest `build` needs.
fn manifest(dir: &Path) -> Result<serde_json::Value, String> {
    let path = dir.join("plugin.json");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "{}: {e} (a plugin folder has a plugin.json)",
            path.display()
        )
    })?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The crate's name, from its `Cargo.toml`.
fn package_name(dir: &Path) -> Result<String, String> {
    let path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{}: no [package] name", path.display()))
}

/// Whether rustup has the target installed; `None` without rustup (a
/// toolchain installed otherwise is trusted to have it).
fn target_installed() -> Option<bool> {
    let out = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l.trim() == TARGET),
    )
}

/// Builds the plugin in `dir`: Cargo compiles it for [`TARGET`] in release
/// (its output goes to the terminal as it does), and the module becomes the
/// component at the manifest's `main`. With `names`, the functions' names
/// are kept whatever the crate's profile strips, so that a trap in
/// Kalem's log reads as a backtrace of them (`kalem plugin dev`).
pub fn build(dir: &Path, names: bool) -> Result<Built, String> {
    let m = manifest(dir)?;
    let Some(main) = m.get("main").and_then(|v| v.as_str()) else {
        return Err(
            "this plugin has no component (no `main` in plugin.json): a declarative plugin is used as it is"
                .into(),
        );
    };
    if main.starts_with('/') || main.contains("..") || !main.ends_with(".wasm") {
        return Err(format!(
            "`main` must be a .wasm path inside the plugin's folder, not {main}"
        ));
    }
    let name = package_name(dir)?;
    if target_installed() == Some(false) {
        return Err(format!(
            "the Rust target {TARGET} is not installed; install it with `rustup target add {TARGET}`"
        ));
    }
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "build",
            "--release",
            "--target",
            TARGET,
            "--message-format=json-render-diagnostics",
        ])
        // WebAssembly's SIMD, which wasmtime compiles: a PDF page renders
        // in 39 ms with it, 52 ms without (D28's record). Added to the
        // crate's own flags, not in their place.
        .args(["--config", SIMD])
        .args(
            names
                .then_some(["--config", "profile.release.strip=\"debuginfo\""])
                .into_iter()
                .flatten(),
        )
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .args(["-p", &name])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo: {e} (is Rust installed? https://rustup.rs)"))?;
    if !out.status.success() {
        return Err(format!("cargo build failed for {name}"));
    }
    let module = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact")
        .filter(|v| {
            v["target"]["name"].as_str().map(|n| n.replace('-', "_"))
                == Some(name.replace('-', "_"))
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
        .ok_or_else(|| {
            format!("cargo built no .wasm for {name}: its [lib] needs crate-type = [\"cdylib\"]")
        })?;
    let path = dir.join(main);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("wasm.tmp");
    wrap(&module, &tmp)?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let (imports, exports) = interface(&path)?;
    Ok(Built {
        path,
        size,
        imports,
        exports,
    })
}

/// The `wasm-tools` to run: `WASM_TOOLS`, else the one on the path.
fn wasm_tools() -> Command {
    Command::new(std::env::var_os("WASM_TOOLS").unwrap_or_else(|| "wasm-tools".into()))
}

/// Runs `wasm-tools` with `args`, its standard output given back.
fn run_wasm_tools(args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let out = wasm_tools().args(args).stdin(Stdio::null()).output().map_err(|e| {
        format!(
            "wasm-tools: {e}; it wraps a module as a component, install it once with `cargo install --locked wasm-tools`"
        )
    })?;
    if !out.status.success() {
        return Err(format!(
            "wasm-tools: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether `bytes` are a component rather than a core module: the layer
/// in the header (ISO/IEC WebAssembly binary format, the component
/// model's preamble).
pub fn is_component(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\0asm") && bytes.get(6..8) == Some(&[1, 0])
}

/// The module at `module` wrapped as the component at `out`, from the WIT
/// world wit-bindgen embedded in it (a module without one becomes a
/// component of nothing).
pub fn wrap(module: &Path, out: &Path) -> Result<(), String> {
    let bytes = std::fs::read(module).map_err(|e| format!("{}: {e}", module.display()))?;
    if is_component(&bytes) {
        return Err(format!(
            "the crate was built as a component already (for wasm32-wasip2?): build it for {TARGET}"
        ));
    }
    run_wasm_tools(&[
        "component".as_ref(),
        "new".as_ref(),
        module.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
    ])
    .map(|_| ())
}

/// A component's imports and exports, by name, from its WIT world as
/// `wasm-tools component wit` prints it.
pub fn interface(component: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let wit = run_wasm_tools(&["component".as_ref(), "wit".as_ref(), component.as_os_str()])?;
    Ok(parse_world(&wit))
}

/// The imports and exports of a printed WIT world: `import NAME: func…`,
/// `import NAME: interface {`, or `import PACKAGE:NAMESPACE/NAME@V;`.
fn parse_world(wit: &str) -> (Vec<String>, Vec<String>) {
    let (mut imports, mut exports) = (Vec::new(), Vec::new());
    for line in wit.lines() {
        let line = line.trim();
        let (list, rest) = if let Some(r) = line.strip_prefix("import ") {
            (&mut imports, r)
        } else if let Some(r) = line.strip_prefix("export ") {
            (&mut exports, r)
        } else {
            continue;
        };
        let name = match rest.split_once(": ") {
            Some((n, _)) => n,
            None => rest.trim_end_matches(';'),
        };
        list.push(name.trim().to_string());
    }
    (imports, exports)
}

/// Whether `name` is a plugin's name: lower-case letters, digits and
/// hyphens, starting with a letter.
fn valid_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The checkout of `getkalem/plugins` `from` is in: a folder up from it
/// with a `template/` and a workspace of `plugins/*`.
fn plugins_repository(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|d| {
            d.join("template/plugin.json").is_file()
                && std::fs::read_to_string(d.join("Cargo.toml"))
                    .is_ok_and(|t| t.contains("\"plugins/*\""))
        })
        .map(Path::to_path_buf)
}

/// The name as a title: `word-count` is "Word count".
fn title(name: &str) -> String {
    let mut t = name.replace('-', " ");
    if let Some(c) = t.get(0..1) {
        let upper = c.to_uppercase();
        t.replace_range(0..1, &upper);
    }
    t
}

/// Starts a plugin called `name` from `cwd`: in a checkout of
/// `getkalem/plugins`, its template copied to `plugins/NAME`; elsewhere,
/// a crate of its own in `cwd/NAME`. Returns the folder.
pub fn new(name: &str, cwd: &Path) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!(
            "`{name}`: a plugin's name is lower-case letters, digits and hyphens, starting with a letter"
        ));
    }
    if let Some(repo) = plugins_repository(cwd) {
        let dest = repo.join("plugins").join(name);
        if dest.exists() {
            return Err(format!("{} exists", dest.display()));
        }
        copy_dir(&repo.join("template"), &dest)?;
        let edit = |file: &str, from: &[(&str, String)]| -> Result<(), String> {
            let p = dest.join(file);
            let mut text =
                std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            for (a, b) in from {
                text = text.replace(a, b);
            }
            std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
        };
        edit(
            "Cargo.toml",
            &[
                ("kalem-plugin-template", format!("kalem-plugin-{name}")),
                (
                    "The crate a new Kalem plugin starts from: copy, rename, fill plugin.json",
                    format!("{}: a Kalem plugin", title(name)),
                ),
            ],
        )?;
        edit(
            "plugin.json",
            &[
                ("org.example.template", format!("org.kalem.{name}")),
                ("\"Template\"", format!("\"{}\"", title(name))),
                ("dist/template.wasm", format!("dist/{name}.wasm")),
                (
                    "Copy this folder to plugins/NAME and describe your plugin here",
                    "What the plugin does".to_string(),
                ),
            ],
        )?;
        let readme = dest.join("README.md");
        std::fs::write(&readme, readme_text(name, true))
            .map_err(|e| format!("{}: {e}", readme.display()))?;
        return Ok(dest);
    }
    let dest = cwd.join(name);
    if dest.exists() {
        return Err(format!("{} exists", dest.display()));
    }
    let files = [
        ("Cargo.toml", standalone_cargo(name)),
        ("plugin.json", standalone_manifest(name)),
        ("src/lib.rs", LIB_RS.to_string()),
        ("README.md", readme_text(name, false)),
        (".gitignore", "target/\ndist/\n".to_string()),
    ];
    for (file, text) in files {
        let p = dest.join(file);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(dest)
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

fn standalone_cargo(name: &str) -> String {
    format!(
        r#"[package]
name = "kalem-plugin-{name}"
description = "{title}: a Kalem plugin"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
# cdylib is the component (`kalem plugin build`); rlib lets the tests
# link the crate.
crate-type = ["cdylib", "rlib"]

[dependencies]

[dev-dependencies]
serde_json = "1"

# Optimized for speed: optimized for size, a PDF page renders twelve
# times slower, and the component is no smaller (D28's record). A
# component never unwinds across the boundary.
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = "symbols"
panic = "abort"

# A crate of its own, not a member of a workspace around it.
[workspace]
"#,
        title = title(name)
    )
}

fn standalone_manifest(name: &str) -> String {
    format!(
        r#"{{
  "id": "org.example.{name}",
  "name": "{title}",
  "version": "0.1.0",
  "description": "What the plugin does",
  "main": "dist/{name}.wasm",
  "api": "^0.2",
  "activation": ["onStartup"],
  "permissions": []
}}
"#,
        title = title(name)
    )
}

const LIB_RS: &str = r#"//! A Kalem plugin.
//!
//! Kalem loads a plugin as a WebAssembly component and calls it through the
//! contracts of its API, generated from Kalem's WIT definition as the
//! `kalem-plugin` crate. Until those bindings are published (Kalem's task
//! T3.1.3) this crate carries its manifest and a test; the plugin's code
//! goes here.

/// The manifest, embedded so that the component carries its own description.
pub const MANIFEST: &str = include_str!("../plugin.json");

#[cfg(test)]
mod tests {
    use super::MANIFEST;

    #[test]
    fn manifest_is_json() {
        let value: serde_json::Value = serde_json::from_str(MANIFEST).expect("plugin.json parses");
        assert!(value.is_object());
    }
}
"#;

fn readme_text(name: &str, in_repository: bool) -> String {
    let test = if in_repository {
        format!("cargo test -p kalem-plugin-{name}")
    } else {
        "cargo test".to_string()
    };
    format!(
        r#"# {title}

A Kalem plugin. Describe what it does in `plugin.json` (the fields are those of section 11.5 of Kalem's design document) and here.

```sh
{test}
kalem plugin build
```

`kalem plugin build` compiles the crate for `wasm32-unknown-unknown` (install it once with `rustup target add wasm32-unknown-unknown`) and writes the component to the path `main` names in `plugin.json`. It imports only what Kalem's API names; a build for `wasm32-wasip2` would import WASI interfaces Kalem does not grant.

While writing it, `kalem plugin dev` builds and installs it again whenever its sources change: a Kalem running opens files with the new build, and a panic in Kalem's log reads as a backtrace of the plugin's functions. The unit tests run natively, with `cargo test`.
"#,
        title = title(name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kalem-plugin-build-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_and_titles() {
        assert!(valid_name("word-count"));
        assert!(valid_name("pdf2"));
        assert!(!valid_name("Word"));
        assert!(!valid_name("2pdf"));
        assert!(!valid_name("a_b"));
        assert_eq!(title("word-count"), "Word count");
    }

    #[test]
    fn a_plugin_of_its_own() {
        let d = temp("own");
        let p = new("word-count", &d).unwrap();
        assert_eq!(p, d.join("word-count"));
        assert_eq!(package_name(&p).unwrap(), "kalem-plugin-word-count");
        let m = manifest(&p).unwrap();
        assert_eq!(m["id"], "org.example.word-count");
        assert_eq!(m["main"], "dist/word-count.wasm");
        assert!(new("word-count", &d).unwrap_err().contains("exists"));
        assert!(new("Bad", &d).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_plugin_of_the_repository() {
        let repo = temp("repo");
        std::fs::write(
            repo.join("Cargo.toml"),
            "[workspace]\nmembers = [\"template\", \"plugins/*\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(repo.join("template/src")).unwrap();
        std::fs::create_dir_all(repo.join("plugins/other")).unwrap();
        std::fs::write(
            repo.join("template/Cargo.toml"),
            "[package]\nname = \"kalem-plugin-template\"\nversion.workspace = true\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("template/plugin.json"),
            r#"{"id": "org.example.template", "name": "Template", "main": "dist/template.wasm"}"#,
        )
        .unwrap();
        std::fs::write(repo.join("template/src/lib.rs"), "").unwrap();
        // From anywhere inside the checkout.
        let p = new("kanban", &repo.join("plugins/other")).unwrap();
        assert_eq!(p, repo.join("plugins/kanban"));
        assert_eq!(package_name(&p).unwrap(), "kalem-plugin-kanban");
        let m = manifest(&p).unwrap();
        assert_eq!(m["id"], "org.kalem.kanban");
        assert_eq!(m["name"], "Kanban");
        assert_eq!(m["main"], "dist/kanban.wasm");
        assert!(p.join("src/lib.rs").is_file());
        let _ = std::fs::remove_dir_all(repo);
    }

    #[test]
    fn a_declarative_plugin_is_not_built() {
        let d = temp("declarative");
        std::fs::write(d.join("plugin.json"), r#"{"id": "org.example.lang"}"#).unwrap();
        assert!(build(&d, false).unwrap_err().contains("declarative"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_printed_world_is_read() {
        let wit = "package root:component;\n\nworld root {\n  import wasi:io/poll@0.2.12;\n  import log: func(x: u32);\n  import host: interface {\n  }\n  export add: func(a: u32, b: u32) -> u32;\n}\n";
        assert_eq!(
            parse_world(wit),
            (
                vec![
                    "wasi:io/poll@0.2.12".to_string(),
                    "log".into(),
                    "host".into()
                ],
                vec!["add".to_string()]
            )
        );
    }

    #[test]
    fn a_module_is_wrapped_and_a_component_refused() {
        // The smallest module, without a WIT world: a component of nothing.
        let module: &[u8] = b"\0asm\x01\0\0\0";
        assert!(!is_component(module));
        // Without wasm-tools installed the wrapping is not tried.
        if wasm_tools().arg("--version").output().is_err() {
            return;
        }
        let d = temp("wrap");
        let (m, c) = (d.join("m.wasm"), d.join("c.wasm"));
        std::fs::write(&m, module).unwrap();
        wrap(&m, &c).unwrap();
        assert!(is_component(&std::fs::read(&c).unwrap()));
        assert_eq!(interface(&c).unwrap(), (vec![], vec![]));
        assert!(
            wrap(&c, &d.join("again.wasm"))
                .unwrap_err()
                .contains(TARGET)
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
