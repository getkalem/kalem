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
        Err(Error::NotGranted(names)) => assert_eq!(
            names,
            [format!("kalem:plugin/files@{}", kalem_script::API_VERSION)]
        ),
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

/// A document whose component fails, runs past its time or out of its
/// memory stops (wasm_todo W8): it says why, answers no more without
/// calling the plugin again, and the viewer's hook hears it once.
#[test]
fn a_component_that_stops_says_why_and_answers_no_more() {
    use kalem_viewer::{FileHandle, Stopped, Viewer as _};
    let Some(bytes) = component("adapted") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-script-stops-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("lines.wasm");
    std::fs::write(&wasm, &bytes).unwrap();
    let f = file("stops.lines", b"LINES\nfirst line\nsecond");
    let host = std::sync::Arc::new(Host::new(None).unwrap());
    let base = kalem_script::viewer::VIEWER_LIMITS;
    for (query, limits) in [
        ("!panic", base),
        (
            "!loop",
            kalem_script::Limits {
                time: std::time::Duration::from_millis(300),
                ..base
            },
        ),
        (
            "!grow",
            kalem_script::Limits {
                memory: 64 << 20,
                ..base
            },
        ),
    ] {
        let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Stopped>::new()));
        let h = heard.clone();
        let v = kalem_script::viewer::ComponentViewer::new(
            host.clone(),
            &wasm,
            "lines",
            "Lines",
            &["lines".to_string()],
            limits,
        )
        .with_on_stop(std::sync::Arc::new(move |s| {
            h.lock().unwrap().push(s.clone())
        }));
        let mut doc = v.open(FileHandle::new(&f)).unwrap();
        assert_eq!(doc.search("second"), [(1, 0..6)]);
        assert_eq!(doc.stopped(), None);
        assert!(doc.search(query).is_empty(), "{query}");
        let why = doc.stopped();
        match (query, &why) {
            // The panic's message and place reach the error (and the
            // log) through the `diagnostics` import.
            ("!panic", Some(Stopped::Failed(detail))) => assert!(
                detail.contains("panicked at") && detail.contains("asked to"),
                "{detail}"
            ),
            ("!loop", Some(Stopped::Timeout(t))) => assert_eq!(t.as_millis(), 300),
            ("!grow", Some(Stopped::Memory(m))) => assert_eq!(*m, 64 << 20),
            _ => panic!("{query}: {why:?}"),
        }
        // No more answers, and the plugin is not called again.
        assert_eq!(doc.text(1), "");
        let e = doc
            .render(0, kalem_viewer::RenderRequest::default())
            .expect_err("an error");
        assert!(e.0.starts_with("Lines stopped: "), "{}", e.0);
        assert_eq!(heard.lock().unwrap().len(), 1, "{query}");
        // Another document of the viewer works.
        let other = v.open(FileHandle::new(&f)).unwrap();
        assert_eq!(other.text(1), "second");
        assert_eq!(other.stopped(), None);
    }
}

/// A file protected by a password through a component (API 0.2.2's
/// `password` interface): refused without it or with a wrong one, as the
/// contract's `needs_password`; opened with it.
#[test]
fn a_component_opens_a_file_with_its_password() {
    use kalem_viewer::{FileHandle, Viewer as _};
    let Some(bytes) = component("adapted") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-script-password-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("lines.wasm");
    std::fs::write(&wasm, &bytes).unwrap();
    let v = kalem_script::viewer::ComponentViewer::new(
        std::sync::Arc::new(Host::new(None).unwrap()),
        &wasm,
        "lines",
        "Lines",
        &["lines".to_string()],
        kalem_script::viewer::VIEWER_LIMITS,
    );
    let f = file("locked.lines", b"LINES\npassword: gizli\none\ntwo");
    let refused = |r: kalem_viewer::Result<Box<dyn kalem_viewer::ViewerDocument>>| {
        r.err().is_some_and(|e| e.is_needs_password())
    };
    assert!(refused(v.open(FileHandle::new(&f))));
    assert!(refused(v.open_with_password(FileHandle::new(&f), "yanlis")));
    let doc = v.open_with_password(FileHandle::new(&f), "gizli").unwrap();
    assert_eq!(doc.text(1), "two");
}

/// An installed component whose file changes (`kalem plugin dev` built it
/// again) is read again for the documents opened from then on (wasm_todo
/// W10); the one built into Kalem never is.
#[test]
fn a_component_built_again_is_read_again() {
    use kalem_viewer::{Detection, FileHandle, Viewer as _};
    let Some(bytes) = component("adapted") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-script-again-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("lines.wasm");
    // Written with a time of its own: a file system may keep seconds only.
    let write = |bytes: &[u8], secs: u64| {
        std::fs::write(&wasm, bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&wasm)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    };
    write(&bytes, 1_000_000);
    let f = file("again.lines", b"LINES\none\ntwo");
    let host = std::sync::Arc::new(Host::new(None).unwrap());
    let v = kalem_script::viewer::ComponentViewer::new(
        host,
        &wasm,
        "lines",
        "Lines",
        &["lines".to_string()],
        kalem_script::viewer::VIEWER_LIMITS,
    );
    let open = v.open(FileHandle::new(&f)).unwrap();
    assert_eq!(v.detect("x.lines", b""), Detection::Extension);
    // Built again, broken: refused, the document open keeps working.
    write(b"not a component", 2_000_000);
    assert!(v.open(FileHandle::new(&f)).is_err());
    assert_eq!(v.detect("x.lines", b""), Detection::No);
    assert_eq!(open.text(1), "two");
    // Built again, fixed.
    write(&bytes, 3_000_000);
    assert_eq!(v.open(FileHandle::new(&f)).unwrap().text(0), "one");
    assert_eq!(v.detect("x.lines", b""), Detection::Extension);
}

/// A component's interfaces are bound as it has them (wasm_todo W3): a
/// document viewer, which exports no `grid`, binds with no grid; a sheet
/// viewer binds its grid.
#[test]
fn interfaces_bound_as_the_component_has_them() {
    let host = Host::new(None).unwrap();
    if let Some(bytes) = pages() {
        let plugin = host.load(&bytes).unwrap();
        let v = Viewer::new(&host, &plugin, Limits::default()).unwrap();
        assert!(!v.is_grid());
    }
    if let Some(bytes) = component("sheet") {
        let plugin = host.load(&bytes).unwrap();
        let v = Viewer::new(&host, &plugin, Limits::default()).unwrap();
        assert!(v.is_grid());
    }
}

/// A file the host holds in memory, with no file on disk (a workbook
/// converted from `.ods` as it opened), reaches the plugin through the
/// same handle: its name, its size and its bytes.
#[test]
fn bytes_the_host_holds_reach_the_plugin() {
    let Some(bytes) = pages() else {
        return;
    };
    let host = Host::new(None).unwrap();
    let plugin = host.load(&bytes).unwrap();
    let mut v = Viewer::new(&host, &plugin, Limits::default()).unwrap();
    let content = b"PAGES and more".to_vec();
    let len = content.len() as u64;
    let file = kalem_viewer::FileHandle::from_reader("held.pages", len, move |at, n| {
        let a = (at as usize).min(content.len());
        content[a..(a + n).min(content.len())].to_vec()
    });
    let doc = v.open_file(file).unwrap().unwrap();
    let s = v.document(|d, st| d.call_structure(st, doc)).unwrap();
    assert_eq!(s.units.len(), 3);
}
