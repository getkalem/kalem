//! A file protected by a password (a PDF with a user password,
//! publish_todo 3.8): the terminal editor asks for it, hides what is
//! typed, asks again after a wrong one, and opens the file with the right
//! one.

use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use kalem_viewer::{
    Bitmap, Detection, FileHandle, RenderRequest, Rendered, Result, Structure, Unit, UnitKind,
    Viewer, ViewerDocument, ViewerError,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

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

#[test]
fn a_file_with_a_password_is_asked_for_it() {
    kalem_core::viewer::register(Arc::new(Locked));
    let dir = std::env::temp_dir().join(format!("kalem-tui-password-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    std::fs::write(dir.join("x.locked"), [0u8, 1, 2, 0, 255, 0]).unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("notes.org")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
    let mut screen = |app: &mut App| {
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    + "\n"
            })
            .collect::<String>()
    };
    let typ = |app: &mut App, s: &str| {
        for c in s.chars() {
            app.event(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            )));
        }
        app.event(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
    };
    app.open_path(&dir.join("x.locked"), None);
    let s = screen(&mut app);
    assert!(s.contains("Open with Password…: the password"), "{s}");
    // A wrong one: hidden as typed, and asked again.
    for c in "yanlis".chars() {
        app.event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    let s = screen(&mut app);
    assert!(s.contains("••••••") && !s.contains("yanlis"), "{s}");
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert_ne!(app.doc.meta.mode, DocumentMode::Viewer);
    assert!(screen(&mut app).contains("Open with Password…: the password"));
    // The right one: the file opens.
    typ(&mut app, "gizli");
    assert_eq!(app.doc.meta.mode, DocumentMode::Viewer);
    assert!(
        app.doc
            .meta
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with("x.locked"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}
