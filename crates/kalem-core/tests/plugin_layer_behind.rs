//! A plugin's layer asked behind the editors: the plugins' bridge is asked
//! on a thread of its own, so drawing a document never waits for it, even
//! when it takes longer than its budget. Until it answers, the document
//! is drawn with the overlays it gave last, moved with the edits made
//! since (as its mode draws it before the first); its answer is drawn at
//! the next drawing once it comes, and it is asked once at a time. In a
//! test binary of its own, as the layers are the process's.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kalem_core::DocumentState;
use kalem_core::layers::{self, LayerSpec, LineEffect, Lines, Outcome, Overlays};
use kalem_core::view::BlockKind;

/// Whether the layer may answer; it waits until then.
static OPEN: AtomicBool = AtomicBool::new(false);
static ASKED: AtomicUsize = AtomicUsize::new(0);

/// Hides every `id::` line, once `OPEN`.
fn layer(_: &LayerSpec, _: Option<&Path>, text: &str) -> Outcome {
    ASKED.fetch_add(1, Ordering::SeqCst);
    let end = Instant::now() + Duration::from_secs(20);
    while !OPEN.load(Ordering::SeqCst) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut o = Overlays::default();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("id::") {
            o.lines.push(Lines {
                range: at..at + line.len(),
                effect: LineEffect::Hidden,
            });
        }
        at += line.len();
    }
    Outcome::Done(o)
}

/// The lines hidden, from their start to the next's.
fn hidden(d: &DocumentState) -> Vec<(usize, usize)> {
    kalem_core::mode_view::blocks(d)
        .iter()
        .filter(|b| b.kind == BlockKind::Hidden)
        .map(|b| (b.range.start, b.range.end))
        .collect()
}

/// Waits for the layer's answer, as the editors' ticks do.
fn answered(d: &mut DocumentState) {
    let end = Instant::now() + Duration::from_secs(20);
    while !d.poll() {
        assert!(Instant::now() < end, "the layer never answered");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn asked(n: usize) {
    let end = Instant::now() + Duration::from_secs(20);
    while ASKED.load(Ordering::SeqCst) < n {
        assert!(Instant::now() < end, "the layer was never asked");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn drawing_does_not_wait_for_a_plugins_layer() {
    let dir = std::env::temp_dir().join(format!("kalem-layer-behind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("graph/logseq")).unwrap();
    std::fs::write(dir.join("graph/logseq/config.edn"), "{}").unwrap();
    layers::register(LayerSpec {
        plugin: "behind".into(),
        id: "ids".into(),
        markers: vec!["logseq/config.edn".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    layers::set_background_provider(Arc::new(layer));
    let path = dir.join("graph/a.md");
    let text = "- one\n  id:: 1\n- two\n";
    let meta = kalem_core::Metadata {
        path: Some(path.clone()),
        mode: kalem_core::DocumentMode::detect(Some(&path), text.as_bytes()),
        line_ending: kalem_core::LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    let mut d = DocumentState::new(text, meta, Arc::new(org_model::Settings::default()));

    // The first drawing asks the layer and does not wait for it, though it
    // waits far longer than its budget: the text as its mode draws it.
    let start = Instant::now();
    assert!(hidden(&d).is_empty());
    let key = kalem_core::mode_view::view_key(&d);
    for line in 0..3 {
        let _ = kalem_core::mode_view::line_view(&d, false, d.text().line_range(line), None, None);
    }
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "drawn without waiting: {:?}",
        start.elapsed()
    );
    assert!(d.waiting_layer());
    assert!(kalem_core::extensions::behind_running());
    // Asked once, however often it is drawn meanwhile.
    asked(1);
    let _ = hidden(&d);
    assert!(!d.poll());
    assert_eq!(ASKED.load(Ordering::SeqCst), 1);

    // Its answer comes: the next drawing has it, the views made anew.
    OPEN.store(true, Ordering::SeqCst);
    answered(&mut d);
    assert!(!d.waiting_layer());
    assert_ne!(kalem_core::mode_view::view_key(&d), key);
    assert_eq!(hidden(&d), [(6, 15)]);
    assert_eq!(ASKED.load(Ordering::SeqCst), 1, "not asked again");

    // An edit: drawn at once with the overlays moved with it, while the
    // layer is asked for the text as it is.
    OPEN.store(false, Ordering::SeqCst);
    let mut tx = org_edit::Transaction::new("type");
    tx.replace(0..0, "- zero\n").unwrap();
    d.apply(&tx, org_edit::ChangeKind::Typing, Instant::now());
    let start = Instant::now();
    assert_eq!(hidden(&d), [(13, 22)], "moved with the edit");
    assert!(start.elapsed() < Duration::from_secs(2));
    asked(2);
    // Typed into the hidden line: still hidden, and grown with it.
    let mut tx = org_edit::Transaction::new("type");
    tx.replace(20..20, "9").unwrap();
    d.apply(&tx, org_edit::ChangeKind::Typing, Instant::now());
    assert_eq!(hidden(&d), [(13, 23)]);
    // One answer at a time: the text typed since is asked for once the
    // answer for the one before it comes.
    assert_eq!(ASKED.load(Ordering::SeqCst), 2);
    OPEN.store(true, Ordering::SeqCst);
    answered(&mut d);
    let _ = hidden(&d);
    asked(3);
    answered(&mut d);
    assert_eq!(hidden(&d), [(13, 23)]);
    assert_eq!(d.text().as_str(), "- zero\n- one\n  id:: 91\n- two\n");

    layers::remove_plugin("behind");
    assert!(d.overlays().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
