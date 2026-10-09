//! A viewer turned off gives way, without a restart, to the copy of its
//! plugin installed while Kalem runs; and the viewers are chosen again as
//! plugins are turned off and on (the owner's report of 2026-10-09: xlsx
//! 0.0.8 turned off after three stops, 0.0.9 installed, and no workbook
//! opened until Kalem was started again).

use std::path::Path;
use std::time::{Duration, Instant};

use kalem_core::plugin_store;
use kalem_viewer::FileHandle;

/// The viewer that opens a workbook now.
fn workbook_viewer() -> Option<std::sync::Arc<dyn kalem_viewer::Viewer>> {
    kalem_core::viewer::find("book.xlsx", b"PK\x03\x04")
}

/// Whether a notice since the last look says `text`.
fn told(text: &str) -> bool {
    kalem_core::jobs::take_notices()
        .iter()
        .any(|(n, _)| n.contains(text))
}

#[test]
fn a_viewer_turned_off_gives_way_to_a_copy_installed_while_kalem_runs() {
    let Some(built_in) = kalem_components::components()
        .iter()
        .find(|c| c.id == "org.kalem.xlsx")
    else {
        return;
    };
    let version = built_in.manifest_json()["version"]
        .as_str()
        .unwrap()
        .to_string();
    let dir = std::env::temp_dir().join(format!("kalem-viewer-choice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("state")).unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", dir.join("config"));
        std::env::set_var("KALEM_STATE_DIR", dir.join("state"));
    }
    // The workbook viewer built in stopped three times: turned off.
    for _ in 0..plugin_store::STOPS_TO_TURN_OFF {
        plugin_store::record_stop("org.kalem.xlsx", &version, "a test");
    }
    kalem_cli::bundled_plugins();
    assert!(
        workbook_viewer().is_none(),
        "turned off, nothing in its place"
    );
    assert!(told("turned off"));

    // A newer copy installed while Kalem runs, as `kalem plugin install
    // DIR` installs one (from another process, the editor would only see
    // the files change).
    kalem_cli::watch_plugins();
    let copy = dir.join("xlsx-copy");
    std::fs::create_dir_all(copy.join("dist")).unwrap();
    std::fs::write(copy.join("dist/xlsx.wasm"), built_in.wasm()).unwrap();
    let mut manifest = built_in.manifest_json();
    manifest["version"] = "99.0.0".into();
    manifest["main"] = "dist/xlsx.wasm".into();
    std::fs::write(copy.join("plugin.json"), manifest.to_string()).unwrap();
    let prepared = plugin_store::prepare(copy.to_str().unwrap(), &[]).unwrap();
    plugin_store::install(&prepared).unwrap();
    let until = Instant::now() + Duration::from_secs(15);
    while workbook_viewer().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    let v = workbook_viewer().expect("the copy installed opens workbooks without a restart");
    assert!(told("99.0.0"), "the user is told which copy opens them");
    let book = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/budget.xlsx");
    let doc = v.open(FileHandle::new(&book)).unwrap();
    assert!(!doc.structure().units.is_empty());

    // The copy turned off in its turn: the copy built in, whose stops were
    // of another version, opens them (no restart, no watcher needed).
    for _ in 0..plugin_store::STOPS_TO_TURN_OFF {
        plugin_store::record_stop("org.kalem.xlsx", "99.0.0", "a test");
    }
    kalem_cli::plugins_changed();
    let v = workbook_viewer().expect("the copy built in in its place");
    assert!(told(&version));
    assert!(v.open(FileHandle::new(&book)).is_ok());

    // Turned on again (`kalem plugin enable`): the newer copy is back,
    // the same viewer as before.
    assert!(plugin_store::clear_stops("org.kalem.xlsx"));
    kalem_cli::plugins_changed();
    assert!(told("99.0.0"));
    // Chosen again with nothing changed: the same viewer, nothing said.
    let before = workbook_viewer().unwrap();
    kalem_cli::plugins_changed();
    assert!(std::sync::Arc::ptr_eq(&before, &workbook_viewer().unwrap()));
    assert!(kalem_core::jobs::take_notices().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}
