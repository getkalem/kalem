//! The terminal editor in Turkish. In its own test binary: the language is
//! global to the process.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn turkish() {
    kalem_core::l10n::set_language("tr");
    let dir = std::env::temp_dir().join(format!("kalem-tui-language-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.org");
    std::fs::write(&path, "* A\none two\n").unwrap();
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(70, 10)).unwrap();
    let mut status = |app: &mut App| {
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.width)
            .map(|x| buf[(x, buf.area.height - 1)].symbol().to_string())
            .collect::<String>()
    };
    assert!(
        status(&mut app).contains("3 kelime"),
        "{}",
        status(&mut app)
    );
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    )));
    for c in "one".chars() {
        app.event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    assert!(
        status(&mut app).trim_start().starts_with("Bul: one"),
        "{}",
        status(&mut app)
    );
    // Yes and no are `e` and `h`; `y` works too.
    app.event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::NONE,
    )));
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
    )));
    assert!(
        status(&mut app).contains("kaydedilsin mi?"),
        "{}",
        status(&mut app)
    );
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('h'),
        KeyModifiers::NONE,
    )));
    assert!(app.quit, "h closes without saving");
    let _ = std::fs::remove_dir_all(dir);
}
