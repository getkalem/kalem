//! A plugin's layer that answers past the drawing's time (100 ms): its
//! answer is dropped and the plugin answers on; so many late answers in
//! a row stop it, and the user is offered to start it again rather than
//! Kalem, which starts it. The layered test plugin, its layer `slow`
//! taking a quarter of a second. Its own process: it sets Kalem's
//! folders. Skipped where the `wasm32-unknown-unknown` target or
//! `wasm-tools` is not installed.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::DocumentState;
use kalem_core::extensions::{self, LATE_IN_A_ROW};

mod test_plugins;
use test_plugins::component;

/// A note of the folder `dir`, which a layer serves.
fn note(dir: &Path) -> DocumentState {
    let path = dir.join("a.md");
    let text = "- one\n  id:: 1\n- two\n";
    std::fs::write(&path, text).unwrap();
    let meta = kalem_core::Metadata {
        path: Some(path.clone()),
        mode: kalem_core::DocumentMode::detect(Some(&path), text.as_bytes()),
        line_ending: kalem_core::LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    DocumentState::new(text, meta, Arc::default())
}

/// Draws `d` and waits for its layer's answer, as the editors' ticks do;
/// the overlays drawn then.
fn drawn(d: &mut DocumentState) -> Option<Arc<kalem_core::layers::Overlays>> {
    let _ = kalem_core::mode_view::blocks(d);
    let end = Instant::now() + Duration::from_secs(60);
    while d.waiting_layer() {
        assert!(Instant::now() < end, "the layer never answered");
        std::thread::sleep(Duration::from_millis(5));
        d.poll();
    }
    d.overlays()
}

#[test]
fn late_answers_are_dropped_and_too_many_stop_the_plugin() {
    let Some(layered) = component("layered") else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-late-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("config/plugins/org.test.layered");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("layered.wasm"), layered).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        r#"{"id": "org.test.layered", "name": "Layered", "main": "layered.wasm",
            "activation": ["onStartup"],
            "layers": [
                {"id": "slow", "markers": ["slow.marker"], "modes": ["markdown"]},
                {"id": "ids", "markers": ["ids.marker"], "modes": ["markdown"]}
            ]}"#,
    )
    .unwrap();
    for (folder, marker) in [("slow", "slow.marker"), ("ids", "ids.marker")] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join(marker), "").unwrap();
    }
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }
    kalem_cli::bundled_plugins();
    let start = Instant::now();
    while !extensions::known("layered.now") {
        assert!(start.elapsed() < Duration::from_secs(120), "never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = kalem_core::jobs::take_notices();
    let mut slow = note(&root.join("slow"));
    let mut ids = note(&root.join("ids"));

    // One late answer: dropped, the plugin answering on.
    assert!(drawn(&mut slow).is_none());
    assert!(
        drawn(&mut ids).is_some_and(|o| o.lines.len() == 1),
        "it answers on"
    );
    assert!(extensions::known("layered.now"));
    assert!(extensions::stopped_plugins().is_empty());
    assert!(kalem_core::jobs::take_offers().is_empty());
    // Not asked again for the same text.
    let _ = kalem_core::mode_view::blocks(&slow);
    assert!(!slow.waiting_layer());

    // Late again and again, nothing in time between: stopped at the last,
    // the user told and offered to start it again.
    for n in 1..=LATE_IN_A_ROW {
        kalem_core::layers::refresh();
        assert!(drawn(&mut slow).is_none());
        assert_eq!(
            extensions::known("layered.now"),
            n < LATE_IN_A_ROW,
            "late answer {n}"
        );
    }
    assert_eq!(extensions::stopped_plugins(), ["layered"]);
    let notices = kalem_core::jobs::take_notices();
    assert!(
        notices
            .iter()
            .any(|(text, error)| *error && text.contains("layered") && text.contains("in a row")),
        "{notices:?}"
    );
    let offers = kalem_core::jobs::take_offers();
    assert!(
        offers
            .iter()
            .flatten()
            .any(|i| i.id.contains(extensions::RESTART) && i.id.contains("layered")),
        "{offers:?}"
    );

    // Started again from the offer, Kalem running on.
    extensions::restart("layered").unwrap();
    assert!(extensions::known("layered.now"));
    assert!(extensions::stopped_plugins().is_empty());
    kalem_core::layers::refresh();
    assert!(drawn(&mut ids).is_some_and(|o| o.lines.len() == 1));
    let _ = std::fs::remove_dir_all(root);
}
