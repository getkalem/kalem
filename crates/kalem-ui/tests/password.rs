//! A file protected by a password (publish_todo 3.8): the graphical
//! editor asks for it in the palette, hidden, and opens the file with it.

use std::rc::Rc;
use std::sync::Arc;

use gpui::TestAppContext;
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;
use kalem_viewer::{
    Bitmap, Detection, FileHandle, RenderRequest, Rendered, Result, Structure, Unit, UnitKind,
    Viewer, ViewerDocument, ViewerError,
};

struct Locked;

struct Doc;

impl Viewer for Locked {
    fn id(&self) -> &str {
        "locked"
    }

    fn name(&self) -> &str {
        "Locked viewer"
    }

    fn extensions(&self) -> &[&str] {
        &["locked"]
    }

    fn detect(&self, name: &str, _head: &[u8]) -> Detection {
        if name.ends_with(".locked") {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(&self, _file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        Err(ViewerError::needs_password())
    }

    fn open_with_password(
        &self,
        _file: FileHandle,
        password: &str,
    ) -> Result<Box<dyn ViewerDocument>> {
        if password == "gizli" {
            Ok(Box::new(Doc))
        } else {
            Err(ViewerError::needs_password())
        }
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
}

#[gpui::test]
fn a_file_with_a_password_is_asked_for_it(cx: &mut TestAppContext) {
    kalem_core::viewer::register(Arc::new(Locked));
    let dir = std::env::temp_dir().join(format!("kalem-ui-password-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    std::fs::write(dir.join("x.locked"), [0u8, 1, 2, 0, 255, 0]).unwrap();
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
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("x.locked"), None, window, cx)
    });
    cx.run_until_parked();
    let label = ws.read_with(cx, |ws, cx| {
        ws.editor
            .read(cx)
            .palette
            .as_ref()
            .and_then(|p| p.arg.as_ref().map(|a| a.label.clone()))
    });
    assert_eq!(label.as_deref(), Some("Open with Password…: the password"));
    cx.simulate_input("gizli");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let mode = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.mode.clone());
    assert_eq!(mode, DocumentMode::Viewer);
    let _ = std::fs::remove_dir_all(&dir);
}
