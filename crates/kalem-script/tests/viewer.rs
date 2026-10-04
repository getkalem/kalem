//! The `document-viewer` world end to end: the fake viewer of
//! `tests/plugins/pages`, built against `kalem-plugin` from the same WIT
//! files `kalem-script` generates its side from (D6), opened and read
//! through the host. Skipped where the `wasm32-unknown-unknown` target or
//! `wasm-tools` is not installed.

use std::path::PathBuf;

mod common;

use common::component;
use kalem_script::viewer::{Viewer, api};
use kalem_script::{Error, Host, Limits};

fn pages() -> Option<Vec<u8>> {
    component("pages")
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

#[test]
fn a_component_is_offered_as_the_rust_contract() {
    use kalem_viewer::{Detection, FileHandle, RenderRequest, Rendered, Viewer as _};
    // A viewer written against the Rust contract, exported through
    // kalem-plugin's adapter, offered to Kalem through the host's.
    let Some(bytes) = component("adapted") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-script-adapted-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("lines.wasm");
    std::fs::write(&wasm, &bytes).unwrap();
    let host = std::sync::Arc::new(Host::new(None).unwrap());
    let v = kalem_script::viewer::ComponentViewer::new(
        host,
        &wasm,
        "lines",
        "Lines",
        &[".LINES".to_string()],
        kalem_script::viewer::VIEWER_LIMITS,
    );
    assert_eq!(
        (v.id(), v.name(), v.extensions()),
        ("lines", "Lines", &["lines"][..])
    );
    assert_eq!(v.detect("x.bin", b"LINES\n"), Detection::Magic);
    assert_eq!(v.detect("x.lines", b""), Detection::Extension);
    assert_eq!(v.detect("x.txt", b"hello"), Detection::No);

    let f = file("book.lines", b"LINES\nfirst line\nsecond");
    let mut doc = v.open(FileHandle::new(&f)).unwrap();
    let s = doc.structure();
    assert_eq!(s.units.len(), 2);
    assert_eq!(doc.text(1), "second");
    assert_eq!(doc.size(0), Some((10.0, 1.0)));
    let Rendered::Bitmap(b) = doc
        .render(
            1,
            RenderRequest {
                scale: 2.0,
                ..RenderRequest::default()
            },
        )
        .unwrap();
    assert_eq!((b.width, b.height), (12, 1));
    assert_eq!(doc.search("second"), [(1, 0..6)]);
    let info = doc.info();
    // The plugin sees the file's name, never its path.
    assert_eq!(info[0].value, f.file_name().unwrap().to_string_lossy());
    assert_eq!(info[1].value, "23");
    // The contract's error, from the plugin.
    let bad = file("bad.lines", b"nope");
    match v.open(FileHandle::new(&bad)) {
        Err(e) => assert_eq!(e.0, "not a lines file"),
        Ok(_) => panic!("a file that is not one opened"),
    }
}
