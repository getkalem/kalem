//! A list offered while the user types (a stopped plugin's restart)
//! opens once the keys rest, not under the fingers, where it took the
//! keys typed next. In a test binary of its own, as the offers and the
//! time of the last key are the process's.

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::palette::PaletteItem;
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn screen(term: &mut Terminal<TestBackend>, app: &mut App) -> String {
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The Word-like keys this test types with; Vim's are the default.
fn word_keys() -> Config {
    Config::from_layers(&[(
        kalem_core::settings::Layer::User,
        None,
        "editor.keymap_profile = \"word\"\n",
    )])
}

fn key(app: &mut App, c: char) {
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::NONE,
    )));
}

#[test]
fn an_offer_waits_for_the_keys_to_rest() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-offer-typing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("n.md");
    std::fs::write(&path, "\n").unwrap();
    let mut app =
        App::with_keymap(Some(&path), word_keys(), Caps::full(), &[], Vec::new()).unwrap();
    let mut term = Terminal::new(TestBackend::new(100, 12)).unwrap();
    let title = "Restart the plugin typed";
    key(&mut app, 'a');
    kalem_core::jobs::offer(vec![PaletteItem {
        id: "plugin.restart".into(),
        title: title.into(),
        category: String::new(),
        detail: String::new(),
        keys: String::new(),
        also: String::new(),
    }]);
    // Typing on: the offer waits, the keys go to the text.
    for c in ['b', 'c'] {
        app.tick(Instant::now());
        assert!(!screen(&mut term, &mut app).contains(title));
        key(&mut app, c);
    }
    app.tick(Instant::now());
    assert!(!screen(&mut term, &mut app).contains(title));
    assert_eq!(app.doc.text().as_str(), "abc\n");
    // The keys rest: it opens.
    std::thread::sleep(kalem_core::jobs::OFFER_PAUSE + Duration::from_millis(50));
    app.tick(Instant::now());
    let shown = screen(&mut term, &mut app);
    assert!(shown.contains(title), "{shown}");
    let _ = std::fs::remove_dir_all(&dir);
}
