//! Install Plugin from GitHub in the terminal editor: the prompt says what
//! it asks for, and a link that is not GitHub's is refused with why.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn install_from_github_asks_for_a_link_and_refuses_others() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-gh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("notes.org")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(160, 20)).unwrap();
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
    app.run_command("plugin.installGitHub", serde_json::json!({}));
    let s = screen(&mut app);
    assert!(
        s.contains("Install Plugin from GitHub…: GitHub link (github.com/you/your-plugin"),
        "{s}"
    );
    for c in "elixir".chars() {
        app.event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    let s = screen(&mut app);
    assert!(s.contains("Not a GitHub link: elixir"), "{s}");
    let _ = std::fs::remove_dir_all(&dir);
}
