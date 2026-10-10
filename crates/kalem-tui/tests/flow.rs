//! A document of flowing text (plugin API 0.2.7) in the terminal editor:
//! the fake document of `tests/plugins/flowdoc`, natively, opened from a
//! file, drawn with its look, typed into through the plugin, a refused
//! edit told, and saved as the plugin writes the file.

#[path = "../../../tests/plugins/flowdoc/src/lib.rs"]
#[allow(dead_code, unreachable_pub, missing_debug_implementations)]
mod flowdoc;

use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.event(Event::Key(KeyEvent::new(code, mods)));
}

#[test]
fn a_flowing_document_is_shown_and_edited() {
    kalem_core::viewer::register(Arc::new(flowdoc::Flows));
    let dir = std::env::temp_dir().join(format!("kalem-tui-flow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.flow");
    // The NUL makes the file binary: Kalem asks its viewers for it.
    std::fs::write(
        &file,
        b"# Title\nPlain and *bold* text^\n- an item\n|a|b|\n---\nThe note.\n\0\n",
    )
    .unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("notes.org")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    app.open_path(&file, None);
    let mut term = Terminal::new(TestBackend::new(80, 16)).unwrap();
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
    assert_eq!(app.doc.meta.mode, DocumentMode::Flow);
    let s = screen(&mut app);
    assert!(s.contains("Title"), "{s}");
    assert!(s.contains("Plain and bold text1"), "{s}");
    assert!(s.contains("• an item"), "{s}");
    assert!(s.contains("▏a │ b"), "{s}");
    assert!(s.contains("[1] The note."), "{s}");
    // Typed at the start of the second paragraph.
    let at = app.doc.text().as_str().find("Plain").unwrap();
    app.doc.move_cursor(at, false);
    for c in "Very ".chars() {
        key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    let s = screen(&mut app);
    assert!(s.contains("Very Plain and bold text1"), "{s}");
    assert!(app.doc.is_modified());
    // Deleting the note's mark is refused, and said.
    let mark = app.doc.text().as_str().find('\u{FFFC}').unwrap();
    app.doc.move_cursor(mark, false);
    key(&mut app, KeyCode::Delete, KeyModifiers::NONE);
    app.tick(std::time::Instant::now());
    let s = screen(&mut app);
    assert!(s.contains("note's mark"), "{s}");
    assert!(app.doc.text().as_str().contains('\u{FFFC}'));
    // Saved as the plugin writes the file (the fake keeps an edited
    // paragraph's text only).
    app.doc.save(Default::default(), true).unwrap();
    assert!(!app.doc.is_modified());
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(saved.contains("Very Plain and bold text^"), "{saved}");
}

/// A picture alone on its line, as the plugin draws it: its text without
/// a graphics protocol, the image over its rows with one, its text again
/// with the cursor on it.
#[test]
fn a_picture_is_drawn_by_its_plugin() {
    kalem_core::viewer::register(Arc::new(flowdoc::Flows));
    let dir = std::env::temp_dir().join(format!("kalem-tui-flow-pic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.flow");
    std::fs::write(&file, b"Above\n@\nBelow\nThe note.\n\0\n").unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("notes.org")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    app.open_path(&file, None);
    app.doc.move_cursor(0, false);
    let mut term = Terminal::new(TestBackend::new(60, 16)).unwrap();
    let mut draw = |app: &mut App| {
        term.draw(|f| app.draw(f)).unwrap();
        term.backend().buffer().clone()
    };
    let row = |buf: &ratatui::buffer::Buffer, y: u16| -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    };
    let buf = draw(&mut app);
    assert!(
        row(&buf, 1).contains("[Picture: A red bar]"),
        "{}",
        row(&buf, 1)
    );
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    app.editor.images.borrow_mut().picker = Some(picker);
    // The picture over two rows, kitty's escape in the first.
    let buf = draw(&mut app);
    assert!(row(&buf, 1).contains("\x1b_G"), "{:?}", row(&buf, 1));
    assert!(!row(&buf, 1).contains("[Picture"));
    assert!(row(&buf, 3).contains("Below"), "{}", row(&buf, 3));
    // The cursor on the picture's line: its text.
    let at = app.doc.text().as_str().find('\u{FFFC}').unwrap();
    app.doc.move_cursor(at, false);
    let buf = draw(&mut app);
    assert!(
        row(&buf, 1).contains("[Picture: A red bar]"),
        "{}",
        row(&buf, 1)
    );
}
