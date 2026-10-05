//! A PDF file in the terminal editor through the pdf-viewer plugin
//! (T3.7.3): the page as an image with kitty's protocol, the next page,
//! the page's text, the outline panel, the find bar.

use std::path::PathBuf;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn status(app: &mut App) -> String {
    app.doc
        .viewer
        .as_deref_mut()
        .map(|v| v.status())
        .unwrap_or_default()
}

#[test]
fn a_pdf_opens_page_by_page() {
    kalem_core::viewer::register(kalem_components::viewer("org.kalem.pdf-viewer").unwrap());
    let dir = std::env::temp_dir().join(format!("kalem-tui-pdf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/pages.pdf");
    std::fs::copy(&data, dir.join("pages.pdf")).unwrap();
    // A second file in the folder, which Page Down at the end must not open.
    std::fs::copy(&data, dir.join("zz.pdf")).unwrap();

    let mut app = App::with_keymap(
        Some(&dir.join("pages.pdf")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let mut screen = |app: &mut App| {
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    screen(&mut app);
    assert_eq!(app.doc.meta.mode, DocumentMode::Viewer);
    let s = status(&mut app);
    assert!(
        s.starts_with("612 × 792 · ") && s.ends_with(" · 1/3"),
        "{s}"
    );

    // With kitty's protocol: the page as an image.
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    app.editor.images.borrow_mut().picker = Some(picker);
    assert!(screen(&mut app).contains("\x1b_G"));

    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::NONE,
    )));
    screen(&mut app);
    let s = status(&mut app);
    assert!(s.ends_with(" · 2/3"), "{s}");
    assert_eq!(app.doc.viewer.as_deref().unwrap().text(), "Page two");

    // The outline panel lists the PDF's outline; Enter on Chapter 2 goes
    // to page three.
    app.editor.images.borrow_mut().picker = None;
    app.run_command("view.outline", serde_json::Value::Null);
    let s = screen(&mut app);
    assert!(
        s.contains("Chapter 1") && s.contains("Section") && s.contains("Chapter 2"),
        "{s}"
    );
    for code in [KeyCode::Down, KeyCode::Enter] {
        app.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    screen(&mut app);
    let s = status(&mut app);
    assert!(s.ends_with(" · 3/3"), "{s}");
    // Page Down held at the last page stays there, in this file.
    for _ in 0..3 {
        app.event(Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::NONE,
        )));
    }
    screen(&mut app);
    let s = status(&mut app);
    assert!(s.ends_with(" · 3/3"), "{s}");
    assert!(app.doc.meta.path.as_ref().unwrap().ends_with("pages.pdf"));

    // The find bar searches the pages on a thread from the page shown on;
    // Enter goes round to page one.
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    )));
    for c in "page".chars() {
        app.event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    let settle = |app: &mut App| {
        while app.doc.viewer.as_deref().is_some_and(|v| v.searching()) {
            app.tick(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app.tick(std::time::Instant::now());
    };
    settle(&mut app);
    let s = screen(&mut app);
    assert!(s.contains("page  3/3"), "{s}");
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    let s = screen(&mut app);
    assert!(s.contains("page  1/3"), "{s}");
    assert_eq!(app.doc.viewer.as_deref().unwrap().unit, 0);

    // Opened at a "line" (Show in PDF, T2.7h.24): that page.
    let pdf = dir.join("pages.pdf");
    for page in [1, 2] {
        app.open_path(&pdf, Some((page, 0)));
        let s = status(&mut app);
        assert!(s.ends_with(&format!(" · {page}/3")), "{s}");
    }
    // Zoomed in, a height on the page (the line Show in PDF gives) comes
    // to the middle.
    app.doc.viewer.as_deref_mut().unwrap().zoom_by(4.0);
    app.open_path(&pdf, Some((2, 700)));
    let center = app.doc.viewer.as_deref().unwrap().center.unwrap();
    assert!(center.1 > 600.0, "{center:?}");
    // Built again (the file rewritten): reloaded at the same page.
    app.doc.reload(std::time::Instant::now()).unwrap();
    let s = status(&mut app);
    assert!(s.ends_with(" · 2/3"), "{s}");
    let _ = std::fs::remove_dir_all(&dir);
}
