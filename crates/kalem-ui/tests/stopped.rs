//! A viewer whose document stops answering (a plugin component that
//! failed or ran past its limits, wasm_todo W8): the graphical editor
//! closes the document at its next tick and says why in the document
//! shown then, never stopping itself.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::TestAppContext;
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;
use kalem_viewer::{
    Bitmap, Detection, FileHandle, RenderRequest, Rendered, Result, Stopped, Structure, Unit,
    UnitKind, Viewer, ViewerDocument,
};

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

#[gpui::test]
fn a_viewer_that_stops_closes_its_document_saying_why(cx: &mut TestAppContext) {
    kalem_core::viewer::register(Arc::new(Stopping));
    let dir = std::env::temp_dir().join(format!("kalem-ui-stopped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    std::fs::write(dir.join("x.stops"), [0u8, 1, 2, 0, 255, 0]).unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let notes = dir.join("notes.org");
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&notes), shared, Theme::light(), cx).unwrap();
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("x.stops"), None, window, cx)
    });
    cx.run_until_parked();
    let mode = |ws: &gpui::Entity<Workspace>, cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.mode.clone())
    };
    assert_eq!(mode(&ws, cx), DocumentMode::Viewer);
    let tick = |ws: &gpui::Entity<Workspace>, cx: &mut gpui::VisualTestContext| {
        let e = ws.read_with(cx, |ws, _| ws.editor.clone());
        e.update(cx, |e, cx| e.tick(cx));
        cx.run_until_parked();
    };
    // Working: a tick leaves it open.
    tick(&ws, cx);
    assert_eq!(mode(&ws, cx), DocumentMode::Viewer);
    STOP.store(true, Ordering::Relaxed);
    tick(&ws, cx);
    assert_eq!(mode(&ws, cx), DocumentMode::Org);
    let (count, status) = ws.read_with(cx, |ws, cx| {
        (
            ws.editors.len(),
            ws.editor.read(cx).status.clone().map(|s| s.0),
        )
    });
    assert_eq!(count, 1);
    let status = status.unwrap_or_default();
    assert!(
        status.contains("Stopping viewer stopped (it ran past its 10 s), and x.stops was closed"),
        "{status}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
