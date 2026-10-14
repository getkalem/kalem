//! Format on save, through the user's table of formatters. In its own
//! test binary: the table is the process's, and a setting saved in another
//! test (one of `app.rs` with a settings folder) set it again, taking the
//! formatter from under this one. The formatters are shell commands.
#![cfg(unix)]

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::settings::{Config, Layer};
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// The terminal editor on `text`, saved as `name` in a folder of its own,
/// with `settings`; its file.
fn editor(text: &str, name: &str, settings: &str) -> (App, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kalem-tui-fmt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let config = Config::from_layers(&[(Layer::User, None, settings)]);
    // What `apply_process_settings` does for the table at startup and on
    // a reload (the rest of it would change the process to no purpose).
    kalem_core::formatters::set_user_table(config.get("formatters"));
    let app = App::with_keymap(Some(&path), config, Caps::full(), &[], Vec::new()).unwrap();
    (app, path)
}

/// The screen's text.
fn screen(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(70, 8)).unwrap();
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
}

/// `editor.format_on_save`: Save runs the type's formatter first
/// (`formatters.foo`), and the file holds the formatted text; when the
/// formatter fails the file is saved as it is and the status bar says
/// why.
#[test]
fn format_on_save() {
    let on = "editor.format_on_save = true\n[formatters]\n";
    let (mut app, path) = editor("hello\n", "f.foo", &format!("{on}foo = \"tr a-z A-Z\"\n"));
    app.run_command("app.save", serde_json::Value::Null);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "HELLO\n");
    assert_eq!(app.doc.text().as_str(), "HELLO\n");
    // The formatter fails: saved all the same, the reason shown.
    let failing = format!("{on}foo = \"sh -c 'echo boom >&2; exit 1'\"\n");
    let (mut app, path) = editor("hello\n", "g.foo", &failing);
    screen(&mut app);
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::NONE,
    )));
    app.run_command("app.save", serde_json::Value::Null);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xhello\n");
    let shown = screen(&mut app);
    assert!(shown.contains("boom"), "{shown}");
    kalem_core::formatters::set_user_table(None);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
