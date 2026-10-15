//! Lines a plugin's layer draws under a line (plugin API 0.2.13) in the
//! terminal: under their line, not part of the text: the cursor steps
//! over them, a click on one puts the cursor at the end of their line,
//! the source view leaves them out. In a test binary of its own, as the
//! layers are the process's.

use std::sync::Arc;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use kalem_core::layers::{self, Below, Insert, LayerSpec, Outcome, Overlays, VirtualLine};
use kalem_core::settings::Config;
use kalem_core::view::Style;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const TEXT: &str = "- first\n- embed here\n- third\n";

fn row(term: &mut Terminal<TestBackend>, app: &mut App, y: u16) -> String {
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn lines_under_a_line_are_drawn_and_stepped_over() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-layer-lines-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.marker"), "").unwrap();
    let path = dir.join("n.md");
    std::fs::write(&path, TEXT).unwrap();
    layers::register(LayerSpec {
        plugin: "test".into(),
        id: "notes".into(),
        markers: vec!["notes.marker".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    layers::set_provider(Arc::new(|_, _, text: &str| {
        let line = |t: &str| VirtualLine {
            indent: 2,
            pieces: vec![(t.to_string(), Style::default())],
            source: None,
        };
        Outcome::Done(Overlays {
            below: vec![Below {
                // Any byte of the line.
                line: text.find("here").unwrap(),
                lines: vec![line("• child one"), line("• child two")],
            }],
            // A layer's text at the end of the first line (API 0.2.20).
            inserts: vec![Insert {
                at: text.find("\n- embed").unwrap(),
                text: " 2".into(),
                style: Style::default(),
            }],
            ..Overlays::default()
        })
    }));
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(40, 8)).unwrap();
    let rows: Vec<String> = (0..5).map(|y| row(&mut term, &mut app, y)).collect();
    assert_eq!(
        rows,
        [
            "1  - first 2",
            "2  - embed here",
            "     • child one",
            "     • child two",
            "3  - third"
        ]
    );
    // The cursor steps over them, down and up, keeping its column.
    let at = |app: &App| app.doc.selection.head;
    app.doc.move_cursor(TEXT.find("embed").unwrap(), false);
    app.event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    assert_eq!(at(&app), TEXT.find("third").unwrap(), "down");
    app.event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert_eq!(at(&app), TEXT.find("embed").unwrap(), "up");
    // At the line's end, the cursor is drawn on the line, not under it.
    app.doc.move_cursor(TEXT.find("\n- third").unwrap(), false);
    term.draw(|f| app.draw(f)).unwrap();
    assert_eq!(term.get_cursor_position().unwrap().y, 1);
    // A click on one: the cursor at the end of its line.
    app.doc.move_cursor(0, false);
    term.draw(|f| app.draw(f)).unwrap();
    app.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(at(&app), TEXT.find("\n- third").unwrap(), "click");
    // Not in the source view, nor the text at a line's end.
    app.editor.source = true;
    let rows: Vec<String> = (0..3).map(|y| row(&mut term, &mut app, y)).collect();
    assert_eq!(rows, ["1  - first", "2  - embed here", "3  - third"]);
    let _ = std::fs::remove_dir_all(&dir);
}
