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

/// No plugin is built into Kalem: a file it cannot open yet (not text, no
/// plugin installed opens it) names the released plugins of the index
/// that open it, to install and then open it, and the system's
/// application; a command that needs such a plugin (New Workbook) offers
/// the one that makes its files (T3.7.9).
#[test]
fn a_file_without_its_plugin_offers_the_plugin_that_opens_it() {
    use kalem_core::settings::Layer;
    use std::time::{Duration, Instant};
    let dir = std::env::temp_dir().join(format!("kalem-tui-for-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    std::fs::write(dir.join("report.qqq"), [0u8, 159, 146, 150, 0, 7, 255]).unwrap();
    let index = dir.join("index.json");
    std::fs::write(
        &index,
        r#"{"schema":1,"plugins":[
          {"id":"org.example.unreleased","name":"Draft viewer","version":"0.1.0","api":"^0.2.1",
           "source":"x","download":null,"sha256":null,"opens":[".qqq"]},
          {"id":"org.example.qqq","name":"Q viewer","version":"1.2.0","description":"Opens Q files",
           "api":"^0.2.1","source":"x","download":"https://example.com/q.wasm","sha256":"00",
           "opens":[".QQQ"]},
          {"id":"org.example.sheets","name":"Sheets","version":"2.0.0","description":"Workbooks",
           "api":"^0.2.1","source":"x","download":"https://example.com/s.wasm","sha256":"00",
           "opens":[".xlsx"]}]}"#,
    )
    .unwrap();
    let url = format!("file://{}", index.display()).replace('\\', "/");
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        &format!("[plugins]\nindex = \"{url}\"\n"),
    )]);
    let mut app = App::with_keymap(
        Some(&dir.join("notes.org")),
        config,
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
    // The index read in the background, its offer shown when it is.
    let mut wait_for = |app: &mut App, text: &str| -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.tick(Instant::now());
            let s = screen(app);
            if s.contains(text) || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    app.open_path(&dir.join("report.qqq"), None);
    let s = wait_for(&mut app, "Install Q viewer 1.2.0 and open report.qqq");
    assert!(
        s.contains("Install Q viewer 1.2.0 and open report.qqq"),
        "{s}"
    );
    assert!(s.contains("Open with the System's Application"), "{s}");
    // A plugin not released yet cannot be installed.
    assert!(!s.contains("Draft viewer"), "{s}");
    app.event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    // New Workbook without the plugin that makes workbooks: it, offered.
    app.run_command("app.newWorkbook", serde_json::json!({}));
    let s = wait_for(
        &mut app,
        "Install Sheets 2.0.0, which opens and makes .xlsx files",
    );
    assert!(
        s.contains("Install Sheets 2.0.0, which opens and makes .xlsx files"),
        "{s}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
