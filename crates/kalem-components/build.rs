//! Kalem's bundled plugins as released components (wasm_todo W9), with
//! the feature `embed`: each entry of `components.toml` downloaded from
//! its GitHub release (or read from `KALEM_COMPONENTS_DIR`), checked
//! against the SHA-256 the file pins, refused when it imports WASI, and
//! compressed into the binary. Downloads are kept under the target
//! folder by their hash, so a build fetches each once. Without the
//! feature the crate holds none.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// An entry of `components.toml`.
struct Entry {
    id: String,
    tag: String,
    file: String,
    sha256: String,
    manifest: String,
    manifest_sha256: String,
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=components.toml");
    println!("cargo:rerun-if-env-changed=KALEM_COMPONENTS_DIR");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let list = out.join("components.rs");
    if std::env::var_os("CARGO_FEATURE_EMBED").is_none() {
        write_if_changed(&list, b"static COMPONENTS: [Component; 0] = [];\n");
        return;
    }
    let here = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(here.join("components.toml")).expect("components.toml");
    let (repository, entries) = parse(&text);
    let cache = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| here.join("../../target").to_path_buf(), PathBuf::from)
        .join("kalem-components/downloads");
    std::fs::create_dir_all(&cache).expect("the download folder");
    let offline = std::env::var_os("KALEM_COMPONENTS_DIR").map(PathBuf::from);
    let mut rs = format!("static COMPONENTS: [Component; {}] = [\n", entries.len());
    for e in &entries {
        let stem = e.file.trim_end_matches(".wasm");
        let wasm = fetch(
            &cache,
            offline.as_deref().map(|d| d.join(&e.file)),
            &format!(
                "https://github.com/{repository}/releases/download/{}/{}",
                e.tag, e.file
            ),
            &e.sha256,
        );
        let manifest = fetch(
            &cache,
            offline
                .as_deref()
                .map(|d| d.join(format!("{stem}.plugin.json"))),
            &format!(
                "https://raw.githubusercontent.com/{repository}/{}/{}",
                e.tag, e.manifest
            ),
            &e.manifest_sha256,
        );
        let m: serde_json::Value = serde_json::from_slice(&manifest)
            .unwrap_or_else(|err| panic!("{}: its manifest: {err}", e.id));
        assert_eq!(
            m["id"].as_str(),
            Some(e.id.as_str()),
            "{}: the manifest of {} names another plugin",
            e.id,
            e.tag
        );
        refuse_wasi(&e.id, &wasm);
        let manifest_file = out.join(format!("{stem}.plugin.json"));
        write_if_changed(&manifest_file, &manifest);
        let deflated = out.join(format!("{stem}.wasm.deflate"));
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&wasm).expect("compressed");
        write_if_changed(&deflated, &enc.finish().expect("compressed"));
        rs.push_str(&format!(
            "    Component {{ id: {:?}, manifest: include_str!({:?}), deflated: include_bytes!({:?}), wasm: std::sync::OnceLock::new() }},\n",
            e.id,
            manifest_file.display().to_string(),
            deflated.display().to_string()
        ));
    }
    rs.push_str("];\n");
    write_if_changed(&list, rs.as_bytes());
}

/// `components.toml`: its repository and entries.
fn parse(text: &str) -> (String, Vec<Entry>) {
    let doc: toml_edit::DocumentMut = text.parse().expect("components.toml parses");
    let repository = doc["repository"]
        .as_str()
        .expect("components.toml names its repository")
        .to_string();
    let entries = doc["component"]
        .as_array_of_tables()
        .expect("components.toml has [[component]] entries")
        .iter()
        .map(|t| {
            let field = |k: &str| {
                t.get(k)
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| panic!("components.toml: an entry without `{k}`"))
                    .to_string()
            };
            Entry {
                id: field("id"),
                tag: field("tag"),
                file: field("file"),
                sha256: field("sha256"),
                manifest: field("manifest"),
                manifest_sha256: field("manifest_sha256"),
            }
        })
        .collect();
    (repository, entries)
}

/// The bytes whose SHA-256 is `sha256`: from the cache, else from
/// `offline`, else downloaded from `url`; kept in the cache.
fn fetch(cache: &Path, offline: Option<PathBuf>, url: &str, sha256: &str) -> Vec<u8> {
    let kept = cache.join(sha256);
    if let Ok(bytes) = std::fs::read(&kept)
        && hex(&bytes) == sha256
    {
        return bytes;
    }
    let bytes = match offline {
        Some(path) => std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        None => {
            let mut resp = ureq::get(url)
                .call()
                .unwrap_or_else(|e| panic!("{url}: {e} (KALEM_COMPONENTS_DIR builds offline)"));
            let mut bytes = Vec::new();
            resp.body_mut()
                .as_reader()
                .read_to_end(&mut bytes)
                .unwrap_or_else(|e| panic!("{url}: {e}"));
            bytes
        }
    };
    let got = hex(&bytes);
    assert!(
        got == sha256,
        "{url}: SHA-256 {got}, components.toml pins {sha256}"
    );
    write_if_changed(&kept, &bytes);
    bytes
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Refuses a component importing WASI: Kalem grants none of it.
fn refuse_wasi(id: &str, component: &[u8]) {
    assert!(
        wasmparser::Parser::is_component(component),
        "{id}: not a WebAssembly component"
    );
    for payload in wasmparser::Parser::new(0).parse_all(component) {
        if let Ok(wasmparser::Payload::ComponentImportSection(imports)) = payload {
            for import in imports.into_iter().flatten() {
                assert!(
                    !import.name.0.starts_with("wasi:"),
                    "{id} imports {}, which Kalem does not grant",
                    import.name.0
                );
            }
        }
    }
}

/// Writes `path` only when its bytes change, so Cargo rebuilds only what
/// did.
fn write_if_changed(path: &Path, bytes: &[u8]) {
    if std::fs::read(path).ok().as_deref() != Some(bytes) {
        std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}
