//! The `document-viewer` world end to end: the fake viewer of
//! `tests/plugins/pages`, built against `kalem-plugin` from the same WIT
//! files `kalem-script` generates its side from (D6), opened and read
//! through the host. Skipped where the `wasm32-unknown-unknown` target or
//! `wasm-tools` is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_script::viewer::{Viewer, api};
use kalem_script::{Error, Host, Limits};

/// The fake viewer built and wrapped as a component, or `None` without
/// the tools.
fn pages() -> Option<Vec<u8>> {
    let target = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()?;
    if !String::from_utf8_lossy(&target.stdout).contains("wasm32-unknown-unknown") {
        return None;
    }
    Command::new("wasm-tools").arg("--version").output().ok()?;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/plugins/pages");
    let out = std::env::temp_dir().join(format!("kalem-script-pages-{}", std::process::id()));
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
    assert!(ok, "the fake viewer builds");
    let module = out.join("wasm32-unknown-unknown/release/kalem_plugin_pages.wasm");
    let component = out.join("pages.wasm");
    let ok = Command::new("wasm-tools")
        .args(["component", "new"])
        .arg(&module)
        .arg("-o")
        .arg(&component)
        .status()
        .ok()?
        .success();
    assert!(ok, "the fake viewer wraps");
    std::fs::read(component).ok()
}

fn file(name: &str, bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("kalem-script-{}-{name}", std::process::id()));
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn a_viewer_component_opens_a_file() {
    let Some(bytes) = pages() else {
        return;
    };
    let host = Host::new(None).unwrap();
    let plugin = host.load(&bytes).unwrap();
    let mut v = Viewer::new(&host, &plugin, Limits::default()).unwrap();

    let d = v.describe().unwrap();
    assert_eq!(
        (d.id.as_str(), d.extensions.as_slice()),
        ("pages", &["pages".to_string()][..])
    );
    assert_eq!(
        v.detect("a.bin", b"PAGES\0").unwrap(),
        api::Detection::Magic
    );
    assert_eq!(v.detect("a.pages", b"").unwrap(), api::Detection::Extension);
    assert_eq!(v.detect("a.txt", b"hello").unwrap(), api::Detection::No);

    // A file that is not one: the plugin's own error.
    let bad = file("bad.pages", b"nope");
    assert_eq!(v.open(&bad).unwrap().unwrap_err(), "not a pages file");

    // The file is read through the handle: its name, not its path.
    let good = file("book.pages", b"PAGES and more");
    let doc = v.open(&good).unwrap().unwrap();
    let s = v.document(|d, st| d.call_structure(st, doc)).unwrap();
    assert_eq!(s.units.len(), 3);
    assert_eq!(s.units[2].label, "3");
    assert_eq!(s.outline[0].unit, 2);
    assert_eq!(
        v.document(|d, st| d.call_size(st, doc, 0)).unwrap(),
        Some((100.0, 50.0))
    );
    assert_eq!(
        v.document(|d, st| d.call_text(st, doc, 1)).unwrap(),
        "page 2"
    );
    let request = api::RenderRequest {
        scale: 2.0,
        theme: api::Theme {
            dark: false,
            background: api::Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            foreground: api::Rgb { r: 0, g: 0, b: 0 },
        },
    };
    let b = v
        .document(|d, st| d.call_render(st, doc, 2, request))
        .unwrap()
        .unwrap();
    assert_eq!((b.width, b.height, b.rgba.len()), (200, 100, 200 * 100 * 4));
    assert!(b.rgba.iter().all(|&p| p == 3));
    let hits = v
        .document(|d, st| d.call_search(st, doc, "page 3"))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].unit, 2);
    let info = v.document(|d, st| d.call_info(st, doc)).unwrap();
    let get = |l: &str| info.iter().find(|f| f.label == l).map(|f| f.value.clone());
    assert!(get("Name").unwrap().ends_with("book.pages"));
    assert!(
        !get("Name").unwrap().contains('/'),
        "no path reaches the plugin"
    );
    assert_eq!(get("Size").as_deref(), Some("14"));
    assert_eq!(get("Renders").as_deref(), Some("1"));
    assert!(v.document(|d, st| d.call_save(st, doc)).unwrap().is_err());
}

#[test]
fn a_viewer_without_its_files_is_refused() {
    let Some(bytes) = pages() else {
        return;
    };
    let host = Host::new(None).unwrap();
    let plugin = host.load(&bytes).unwrap();
    // The `files` interface not granted: the plugin is refused, named.
    match plugin.instantiate(&host, &host.linker::<()>(), (), Limits::default()) {
        Err(Error::NotGranted(names)) => assert_eq!(names, ["kalem:plugin/files@0.1.0"]),
        other => panic!("{other:?}"),
    }
}
