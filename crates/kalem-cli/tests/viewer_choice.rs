//! The viewers chosen again while Kalem runs, as plugins are installed,
//! turned off, updated and removed, with no restart (the owner's report
//! of 2026-10-09: xlsx 0.0.8 turned off after three stops, 0.0.9
//! installed, and no workbook opened until Kalem was started again).
//! The viewer is `tests/plugins/pages`, built from its source, so the
//! test needs no plugin built into Kalem; skipped where the
//! `wasm32-unknown-unknown` target or `wasm-tools` is not installed.

use std::path::Path;
use std::time::{Duration, Instant};

use kalem_core::plugin_store;
use kalem_viewer::FileHandle;

mod test_plugins;

/// The viewer that opens a pages file now.
fn pages_viewer() -> Option<std::sync::Arc<dyn kalem_viewer::Viewer>> {
    kalem_core::viewer::find("three.pages", b"PAGES\0")
}

/// Waits for the watcher to choose again until `done`, for up to 15 s;
/// `done` is asked once a round, and may take what it looks at ([`told`]).
fn until(done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether a notice since the last look says `text`; the notices looked
/// at are taken.
fn told(text: &str) -> bool {
    kalem_core::jobs::take_notices()
        .iter()
        .any(|(n, _)| n.contains(text))
}

/// The pages plugin at `version` installed as `kalem plugin install DIR`
/// installs one.
fn install(dir: &Path, wasm: &[u8], version: &str) {
    let copy = dir.join(format!("pages-{version}"));
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::write(copy.join("pages.wasm"), wasm).unwrap();
    std::fs::write(
        copy.join("plugin.json"),
        serde_json::json!({
            "id": "org.test.pages",
            "name": "Pages",
            "version": version,
            "main": "pages.wasm",
            "opens": [".pages"],
            "applies": {"magic": ["50 41 47 45 53 00"]},
        })
        .to_string(),
    )
    .unwrap();
    let prepared = plugin_store::prepare(copy.to_str().unwrap(), &[]).unwrap();
    plugin_store::install(&prepared).unwrap();
}

#[test]
fn viewers_follow_installs_stops_and_removals_without_a_restart() {
    let Some(wasm) = test_plugins::component("pages") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-viewer-choice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("state")).unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", dir.join("config"));
        std::env::set_var("KALEM_STATE_DIR", dir.join("state"));
        // Compiled once for every run, not again in each state folder.
        if std::env::var_os("KALEM_COMPONENT_CACHE").is_none() {
            let cache = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kalem-component-cache");
            std::fs::create_dir_all(&cache).unwrap();
            std::env::set_var("KALEM_COMPONENT_CACHE", cache);
        }
    }
    let file = dir.join("three.pages");
    // A NUL byte: a file that is not text, which a viewer opens.
    std::fs::write(&file, b"PAGES\0 of a test").unwrap();
    kalem_cli::bundled_plugins();
    assert!(pages_viewer().is_none(), "nothing opens pages yet");
    kalem_cli::watch_plugins();

    // Installed while Kalem runs: it opens its files without a restart,
    // and the user is told which copy opens them.
    install(&dir, &wasm, "1.0.0");
    assert!(
        until(|| pages_viewer().is_some()),
        "the copy installed is used"
    );
    // The watcher's thread registers the viewer before it posts the
    // notice: waited for too, not read at once.
    assert!(
        until(|| told("Pages 1.0.0")),
        "the user is told which copy opens them"
    );
    // By the first bytes its manifest declares, with no extension: Kalem
    // chooses by the declaration, the plugin's code not asked.
    let bare = dir.join("three");
    assert!(kalem_core::viewer::find_at(&bare, b"PAGES\0 of a test").is_some());
    assert!(kalem_core::viewer::find_at(&bare, b"\0PAGES").is_none());
    let doc = pages_viewer()
        .unwrap()
        .open(FileHandle::new(&file))
        .unwrap();
    assert_eq!(doc.structure().units.len(), 3);

    // Turned off after three stops: nothing opens its files in its place.
    for _ in 0..plugin_store::STOPS_TO_TURN_OFF {
        plugin_store::record_stop("org.test.pages", "1.0.0", "a test");
    }
    kalem_cli::plugins_changed();
    assert!(pages_viewer().is_none(), "turned off");

    // A newer copy installed meanwhile (whose version has no stops) opens
    // them again, still with no restart.
    install(&dir, &wasm, "1.0.1");
    assert!(until(|| pages_viewer().is_some()), "the update is used");
    assert!(
        until(|| told("Pages 1.0.1")),
        "the user is told of the update"
    );
    assert!(pages_viewer().unwrap().open(FileHandle::new(&file)).is_ok());

    // Chosen again with nothing changed (`kalem plugin enable`): the same
    // viewer, nothing said.
    let before = pages_viewer().unwrap();
    plugin_store::clear_stops("org.test.pages");
    kalem_cli::plugins_changed();
    assert!(std::sync::Arc::ptr_eq(&before, &pages_viewer().unwrap()));
    assert!(!told("Pages"));

    // Removed while Kalem runs: its files have no viewer again.
    plugin_store::remove("org.test.pages").unwrap();
    assert!(until(|| pages_viewer().is_none()), "the removal is seen");
    std::fs::remove_dir_all(&dir).ok();
}
