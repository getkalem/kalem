//! Lines a plugin's layer draws under a line that show a file's text
//! (plugin API 0.2.15), typed into in the terminal where they are drawn:
//! Edit Line Under Line or a click starts, Enter writes what was typed
//! into that file, Escape drops it. In a test binary of its own, as the
//! layers are the process's.

use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use kalem_core::layers::{self, Below, LayerSpec, LineSource, Outcome, Overlays, VirtualLine};
use kalem_core::settings::Config;
use kalem_core::view::Style;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const NOTE: &str = "- first\n- embed here\n- third\n";

fn row(term: &mut Terminal<TestBackend>, app: &mut App, y: u16) -> String {
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn key(app: &mut App, code: KeyCode) {
    app.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

#[test]
fn a_line_under_a_line_typed_into_writes_its_file() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-rows-typed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.marker"), "").unwrap();
    let path = dir.join("n.md");
    std::fs::write(&path, NOTE).unwrap();
    let page: PathBuf = dir.join("page.md");
    std::fs::write(&page, "- TODO buy milk\n- other\n").unwrap();
    // The folder counts with Kalem's projects: its files may be written.
    kalem_core::extensions::name_folder(dir.clone());
    layers::register(LayerSpec {
        plugin: "test".into(),
        id: "notes".into(),
        markers: vec!["notes.marker".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    // An embed of the page's first block, as the file holds it now.
    let embedded = page.clone();
    layers::set_provider(Arc::new(move |_, _, text: &str| {
        let file = std::fs::read_to_string(&embedded).unwrap_or_default();
        let first = file.lines().next().unwrap_or("").trim_start_matches("- ");
        Outcome::Done(Overlays {
            below: vec![Below {
                line: text.find("here").unwrap(),
                lines: vec![
                    VirtualLine {
                        indent: 2,
                        pieces: vec![(format!("• {first}"), Style::default())],
                        source: Some(LineSource {
                            path: embedded.clone(),
                            line: 0,
                            column: 2,
                            text: first.to_string(),
                        }),
                    },
                    VirtualLine {
                        indent: 2,
                        pieces: vec![("only shown".into(), Style::default())],
                        source: None,
                    },
                ],
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
    assert_eq!(row(&mut term, &mut app, 2), "     • TODO buy milk");
    // Edit Line Under Line on the embed's line: its text, the cursor at
    // its end, in it.
    app.doc.move_cursor(NOTE.find("embed").unwrap(), false);
    app.run_command("view.editRow", serde_json::json!({}));
    assert!(app.doc.row_edit.is_some());
    assert_eq!(row(&mut term, &mut app, 2), "     TODO buy milk");
    assert_eq!(term.get_cursor_position().unwrap(), (18, 2).into());
    // Typed into, as a one-line input; Enter writes it into the page.
    for _ in 0.."milk".len() {
        key(&mut app, KeyCode::Backspace);
    }
    typ(&mut app, "bread");
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Delete);
    typ(&mut app, "D");
    assert_eq!(row(&mut term, &mut app, 2), "     DODO buy bread");
    key(&mut app, KeyCode::Enter);
    assert!(app.doc.row_edit.is_none());
    assert_eq!(
        std::fs::read_to_string(&page).unwrap(),
        "- DODO buy bread\n- other\n"
    );
    assert_eq!(app.doc.text().as_str(), NOTE);
    assert_eq!(row(&mut term, &mut app, 2), "     • DODO buy bread");
    // A click on it starts typing into it, the cursor at its end (what is
    // drawn is not what is typed); in it, a click puts the cursor where it
    // was clicked. Escape drops what was typed.
    let click = |app: &mut App, column: u16, row: u16| {
        app.event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    };
    click(&mut app, 9, 2);
    let r = app.doc.row_edit.clone().expect("typing into it");
    assert_eq!(r.cursor(), r.input.len());
    term.draw(|f| app.draw(f)).unwrap();
    click(&mut app, 7, 2);
    let r = app.doc.row_edit.clone().expect("still typing into it");
    assert_eq!(&r.input[..r.cursor()], "DO");
    typ(&mut app, "X");
    key(&mut app, KeyCode::Esc);
    assert!(app.doc.row_edit.is_none());
    assert_eq!(
        std::fs::read_to_string(&page).unwrap(),
        "- DODO buy bread\n- other\n"
    );
    // A line only shown is not typed into; neither is a page changed
    // since the layer read it.
    app.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 7,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.doc.row_edit.is_none());
    app.doc.move_cursor(NOTE.find("embed").unwrap(), false);
    app.run_command("view.editRow", serde_json::json!({}));
    typ(&mut app, "!");
    std::fs::write(&page, "- DONE elsewhere\n").unwrap();
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        std::fs::read_to_string(&page).unwrap(),
        "- DONE elsewhere\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
