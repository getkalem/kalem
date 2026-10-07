//! Org documents in the terminal editor: the keys and prompts of Org's
//! commands, end to end on ratatui's test backend.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::settings::{Config, Layer};
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

struct T {
    app: App,
    term: Terminal<TestBackend>,
    dir: PathBuf,
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A terminal editor on `text` as `t.org`, with `settings` (TOML) in a
/// configuration folder of its own, the cursor at `at`.
fn open_with(text: &str, settings: &str, at: usize) -> T {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "kalem-tui-org-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let config_dir = dir.join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("settings.toml"), "[ui]\nlanguage = \"en\"\n").unwrap();
    kalem_core::kalem_fs::set_trash_dir(Some(dir.join("trash")));
    let path = dir.join("t.org");
    std::fs::write(&path, text).unwrap();
    let config = Config::from_layers(&[(Layer::User, None, settings)]);
    let mut app = App::with_keymap(Some(&path), config, Caps::full(), &[], Vec::new()).unwrap();
    app.config_dir = Some(config_dir);
    let term = Terminal::new(TestBackend::new(60, 8)).unwrap();
    let mut t = T { app, term, dir };
    t.draw();
    t.app.doc.move_cursor(at, false);
    t.app.editor.follow = true;
    t.draw();
    t
}

fn open(text: &str, at: usize) -> T {
    open_with(text, "", at)
}

impl T {
    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| app.draw(f)).unwrap();
    }

    fn row(&mut self, y: u16) -> String {
        self.draw();
        let buf = self.term.backend().buffer();
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn status(&mut self) -> String {
        let y = self.term.backend().buffer().area.height - 1;
        self.row(y).trim().to_string()
    }

    fn key(&mut self, code: KeyCode, m: KeyModifiers) {
        self.app.event(Event::Key(KeyEvent::new(code, m)));
        self.draw();
    }

    fn typ(&mut self, s: &str) {
        for c in s.chars() {
            self.key(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    fn text(&self) -> String {
        self.app.doc.text().as_str().to_string()
    }
}

#[test]
fn set_tags_starts_with_the_headings_tags() {
    // As `org-set-tags-command`, the prompt holds the tags to edit: typing
    // adds to them rather than silently replacing them.
    let mut t = open("* TODO Task :old:\n", 3);
    t.app.run_command("org.tags.set", serde_json::json!({}));
    t.draw();
    assert!(t.status().ends_with(":old:"), "{}", t.status());
    t.typ("new");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.text().starts_with("* TODO Task "), "{}", t.text());
    assert!(t.text().trim_end().ends_with(":old:new:"), "{}", t.text());
}

#[test]
fn shift_tab_goes_on_from_the_startup_overview() {
    // `#+STARTUP: overview` is where the global cycle is: the first
    // Shift+Tab shows the contents, as in Emacs.
    let text = "#+STARTUP: overview\n* A\n** B\ntext\n* C\n";
    let mut t = open(text, text.find("* A").unwrap());
    assert_eq!(t.row(1), " * A …");
    t.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(t.row(2), "   ○ B …");
    assert!(t.status().ends_with("Contents"), "{}", t.status());
    t.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(t.row(3), "     text");
    t.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert!(t.status().ends_with("Overview"), "{}", t.status());
}

#[test]
fn bold_as_a_word_processor_does() {
    // Ctrl+B, the word, Ctrl+B: the word is bold and the rest plain.
    let mut t = open("Say \n", 4);
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    t.typ("bold");
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    t.typ(" text");
    assert_eq!(t.text(), "Say *bold* text\n");
    // Ctrl+B twice on a selection: bold, then plain again.
    t.key(KeyCode::End, KeyModifiers::NONE);
    for _ in 0..4 {
        t.key(KeyCode::Left, KeyModifiers::SHIFT);
    }
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "Say *bold* *text*\n");
    assert_eq!(t.app.doc.selected_text(), Some("text"));
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "Say *bold* text\n");
    assert_eq!(t.app.doc.selected_text(), Some("text"));
    // In the pair Ctrl+B inserted, Ctrl+B takes it away.
    let mut t = open("x \n", 2);
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "x **\n");
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "x \n");
}
