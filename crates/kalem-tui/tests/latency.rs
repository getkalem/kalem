//! Performance targets of §15 for the terminal editor, as benchmarks:
//!
//! ```sh
//! cargo test --release -p kalem-tui --test latency -- --ignored --nocapture
//! ```

#![allow(clippy::print_stderr)]

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn percentile(times: &mut [Duration], p: f64) -> Duration {
    times.sort();
    times[((times.len() as f64 - 1.) * p).round() as usize]
}

#[test]
#[ignore = "a benchmark; run it in a release build"]
fn org_manual() {
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/corpus/org-mode/org-manual.org"
    );
    let start = Instant::now();
    let mut app = App::with_keymap(
        Some(src.as_ref()),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let open = start.elapsed();
    // Typing in the middle of the document, a frame after each key, as the
    // terminal editor draws it.
    let middle = app.doc.text().len() / 2;
    let at = app.doc.text().as_str()[..middle].rfind("\n\n").unwrap() + 1;
    app.doc.move_cursor(at, false);
    term.draw(|f| app.draw(f)).unwrap();
    let mut times = Vec::new();
    for i in 0..400 {
        let key = if i % 10 == 9 {
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
        } else {
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)
        };
        let t = Instant::now();
        app.event(Event::Key(key));
        app.tick(Instant::now());
        term.draw(|f| app.draw(f)).unwrap();
        times.push(t.elapsed());
    }
    let p50 = percentile(&mut times, 0.5);
    let p99 = percentile(&mut times, 0.99);
    eprintln!(
        "org manual ({} bytes): open and first frame {open:?}; keystroke to frame p50 {p50:?}, p99 {p99:?}",
        app.doc.text().len()
    );
    if !cfg!(debug_assertions) {
        assert!(p50 < Duration::from_millis(16), "{p50:?}");
        assert!(p99 < Duration::from_millis(33), "{p99:?}");
        assert!(open < Duration::from_millis(1000), "{open:?}");
    }
}

/// About `n` bytes of the Org manual, repeated.
fn manual_text(n: usize) -> String {
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/corpus/org-mode/org-manual.org"
    );
    let manual = std::fs::read_to_string(src).unwrap();
    let mut text = String::with_capacity(n + manual.len());
    while text.len() < n {
        text.push_str(&manual);
    }
    let end = text[..n].rfind('\n').map_or(n, |i| i + 1);
    text.truncate(end);
    text
}

#[test]
#[ignore = "a benchmark; run it in a release build"]
fn saving_10mb() {
    let dir = std::env::temp_dir().join(format!("kalem-save-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.org");
    std::fs::write(&path, manual_text(10 << 20)).unwrap();
    let mut doc = kalem_core::DocumentState::open(
        &path,
        std::sync::Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .unwrap();
    let mut times = Vec::new();
    for _ in 0..5 {
        doc.insert_text("x", Instant::now());
        let t = Instant::now();
        doc.save(kalem_core::files::SaveOptions::default(), false)
            .unwrap();
        times.push(t.elapsed());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let p50 = percentile(&mut times, 0.5);
    eprintln!("saving a 10 MB document: {p50:?}");
    if !cfg!(debug_assertions) {
        assert!(p50 < Duration::from_millis(100), "{p50:?}");
    }
}
