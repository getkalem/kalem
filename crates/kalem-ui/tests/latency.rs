//! Keystroke latency of the graphical editor (§15), as a benchmark: gpui's
//! test platform with the system's text system, from the key to the
//! painted frame (the scene; the GPU's work is not included).
//!
//! ```sh
//! cargo test --release -p kalem-ui --test latency -- --ignored
//! ```
//!
//! Without `--ignored` it does nothing: the system's text system must be
//! created on the main thread, so this test has its own `main`.

#![allow(clippy::print_stderr)]

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{TestAppContext, TestDispatcher};
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

fn percentile(times: &mut [Duration], p: f64) -> Duration {
    times.sort();
    times[((times.len() as f64 - 1.) * p).round() as usize]
}

fn main() {
    if !std::env::args().any(|a| a == "--ignored") {
        return;
    }
    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, text_system);
    let path = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/corpus/org-mode/org-manual.org"
    ));
    let mut shared = kalem_ui::shared_in(Config::default(), None);
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        std::env::temp_dir().join(format!(
            "kalem-latency-projects-{}.toml",
            std::process::id()
        )),
    )));
    let shared = Rc::new(shared);
    let start = Instant::now();
    let mut editor = None;
    let (_ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        let focus = gpui::Focusable::focus_handle(e.read(cx), cx);
        window.focus(&focus, cx);
        editor = Some(e.clone());
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    let open = start.elapsed();
    let e = editor.unwrap();
    // Typing at the start of a paragraph in the middle of the document.
    e.update(cx, |e, cx| {
        let text = e.doc.text().as_str();
        let at = text[..text.len() / 2].rfind("\n\n").unwrap() + 1;
        e.doc.move_cursor(at, false);
        e.after_change(cx);
    });
    cx.run_until_parked();
    let mut times = Vec::new();
    for i in 0..400 {
        let key = if i % 10 == 9 { "space" } else { "a" };
        let t = Instant::now();
        cx.simulate_keystrokes(key);
        times.push(t.elapsed());
    }
    let painted = e.read_with(cx, |e, _| e.painted.borrow().len());
    assert!(painted > 0, "frames are painted");
    let p50 = percentile(&mut times, 0.5);
    let p99 = percentile(&mut times, 0.99);
    eprintln!(
        "org manual: open and first frame {open:?}; keystroke to painted frame p50 {p50:?}, p99 {p99:?} ({painted} lines painted)"
    );
    if !cfg!(debug_assertions) {
        assert!(p50 < Duration::from_millis(16), "{p50:?}");
        assert!(p99 < Duration::from_millis(33), "{p99:?}");
    }
}
