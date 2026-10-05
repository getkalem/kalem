//! A viewer whose document stops answering (a plugin component that
//! failed or ran past its limits, wasm_todo W8): the terminal editor
//! closes the document at its next tick and says why, never stopping
//! itself.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use kalem_viewer::{
    Bitmap, Detection, FileHandle, RenderRequest, Rendered, Result, Stopped, Structure, Unit,
    UnitKind, Viewer, ViewerDocument,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Set by the test: the documents stop.
static STOP: AtomicBool = AtomicBool::new(false);

struct Stopping;

struct Doc;

impl Viewer for Stopping {
    fn id(&self) -> &str {
        "stopping"
    }

    fn name(&self) -> &str {
        "Stopping viewer"
    }

    fn extensions(&self) -> &[&str] {
        &["stops"]
    }

    fn detect(&self, name: &str, _head: &[u8]) -> Detection {
        if name.ends_with(".stops") {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(&self, _file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        Ok(Box::new(Doc))
    }
}

impl ViewerDocument for Doc {
    fn structure(&self) -> Structure {
        Structure {
            units: vec![Unit {
                kind: UnitKind::Page,
                label: "1".into(),
                duration_ms: None,
            }],
            outline: Vec::new(),
        }
    }

    fn render(&mut self, _unit: usize, _request: RenderRequest) -> Result<Rendered> {
        Ok(Rendered::Bitmap(Bitmap::new(4, 4, vec![128; 64])))
    }

    fn size(&self, _unit: usize) -> Option<(f32, f32)> {
        Some((4.0, 4.0))
    }

    fn text(&self, _unit: usize) -> String {
        String::new()
    }

    fn stopped(&self) -> Option<Stopped> {
        STOP.load(Ordering::Relaxed)
            .then(|| Stopped::Timeout(Duration::from_secs(10)))
    }
}

#[test]
fn a_viewer_that_stops_closes_its_document_saying_why() {
    kalem_core::viewer::register(Arc::new(Stopping));
    let dir = std::env::temp_dir().join(format!("kalem-tui-stopped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    std::fs::write(dir.join("x.stops"), [0u8, 1, 2, 0, 255, 0]).unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("notes.org")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(160, 20)).unwrap();
    app.open_path(&dir.join("x.stops"), None);
    term.draw(|f| app.draw(f)).unwrap();
    assert_eq!(app.doc.meta.mode, DocumentMode::Viewer);
    // Working: a tick leaves it open.
    app.tick(Instant::now());
    assert_eq!(app.doc.meta.mode, DocumentMode::Viewer);
    STOP.store(true, Ordering::Relaxed);
    app.tick(Instant::now());
    assert_eq!(app.doc.meta.mode, DocumentMode::Org);
    assert!(
        app.doc
            .meta
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with("notes.org"))
    );
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    let screen: String = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                + "\n"
        })
        .collect();
    assert!(
        screen.contains("Stopping viewer stopped (it ran past its 10 s), and x.stops was closed"),
        "{screen}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
