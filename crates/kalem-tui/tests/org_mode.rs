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

#[test]
fn links_into_the_document_itself_have_no_file() {
    // As `org-insert-link`: a stored link to a heading of this document,
    // or a typed `file:` link into it, is written without the file.
    let text = "* First\n\n* Second\n";
    let mut t = open(text, text.find("Second").unwrap());
    t.app.run_command("link.store", serde_json::json!({}));
    t.app.doc.move_cursor(8, false);
    t.app.run_command("org.link.insertStored", serde_json::json!({}));
    assert_eq!(t.text(), "* First\n[[*Second][Second]]\n* Second\n");
    t.app.run_command(
        "org.insert.link",
        serde_json::json!({ "link": "file:t.org::*First", "description": "up" }),
    );
    assert_eq!(t.text(), "* First\n[[*Second][Second]][[*First][up]]\n* Second\n");
}

#[test]
fn alt_enter_inserts_a_heading() {
    // M-RET: in a heading's title the rest of it goes to a new heading of
    // its level; in a list Alt+Enter stays Insert Item.
    let text = "* One\n** Two words\nBody.\n- item\n";
    let mut t = open(text, text.find("words").unwrap());
    t.key(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(t.text(), "* One\n** Two \n** words\nBody.\n- item\n");
    assert_eq!(t.app.doc.selection.head, "* One\n** Two \n** words".len());
    t.app.doc.move_cursor(t.text().find("item").unwrap() + 4, false);
    t.key(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(t.text(), "* One\n** Two \n** words\nBody.\n- item\n- \n");
    // After the subtree, from anywhere in it.
    let text = "* A\n** a1\ntext\n* B\n";
    let mut t = open(text, 2);
    t.app.run_command("org.headline.insertAfterSubtree", serde_json::json!({}));
    assert_eq!(t.text(), "* A\n** a1\ntext\n* \n* B\n");
    t.typ("New");
    assert_eq!(t.text(), "* A\n** a1\ntext\n* New\n* B\n");
}

#[test]
fn shift_arrows_on_a_timestamp() {
    // As `org-shiftup` and the others on a timestamp: the part at the
    // cursor, a day with Left and Right; on a heading's timestamp before
    // its priority; elsewhere Shift selects.
    let text = "* TODO Call <2026-10-05 Mon 10:03>\nSCHEDULED: <2026-10-31 Sat>\nplain\n";
    let mut t = open(text, text.find("10-05").unwrap() + 1);
    t.key(KeyCode::Up, KeyModifiers::SHIFT);
    assert!(t.text().starts_with("* TODO Call <2026-11-05 Thu 10:03>"), "{}", t.text());
    t.app.doc.move_cursor(t.text().find("10:03").unwrap() + 4, false);
    t.key(KeyCode::Up, KeyModifiers::SHIFT);
    assert!(t.text().starts_with("* TODO Call <2026-11-05 Thu 10:05>"), "{}", t.text());
    let s = t.text().find("<2026-10-31").unwrap() + 3;
    t.app.doc.move_cursor(s, false);
    t.key(KeyCode::Right, KeyModifiers::SHIFT);
    assert!(t.text().contains("SCHEDULED: <2026-11-01 Sun>"), "{}", t.text());
    t.key(KeyCode::Left, KeyModifiers::SHIFT);
    t.key(KeyCode::Left, KeyModifiers::SHIFT);
    assert!(t.text().contains("SCHEDULED: <2026-10-30 Fri>"), "{}", t.text());
    // Away from timestamps the keys are what they were.
    t.app.doc.move_cursor(t.text().find("plain").unwrap(), false);
    t.key(KeyCode::Right, KeyModifiers::SHIFT);
    assert_eq!(t.app.doc.selected_text(), Some("p"));
    t.app.doc.move_cursor(3, false);
    t.key(KeyCode::Up, KeyModifiers::SHIFT);
    assert!(t.text().starts_with("* TODO [#B] Call"), "{}", t.text());
}

#[test]
fn vim_z_keys_fold_as_in_doom() {
    let text = "* A\ntext\n** B\nmore\n* C\n";
    let mut t = open_with(text, "editor.keymap_profile = \"vim\"\n", text.find("more").unwrap());
    // `zc` in B's body closes B, the cursor on its heading.
    t.typ("zc");
    assert_eq!(t.row(2), " ** B …");
    assert_eq!(t.app.doc.selection.head, text.find("** B").unwrap());
    // `zo` opens it, `za` closes it again.
    t.typ("zo");
    assert_eq!(t.row(3), "     more");
    t.typ("za");
    assert_eq!(t.row(3), " ◉ C");
    // `zM` is the overview, `zR` opens everything; `zz` still scrolls.
    t.typ("zM");
    assert_eq!((t.row(0), t.row(1)), (" * A …".into(), " ◉ C".into()));
    t.typ("zR");
    assert_eq!(t.row(3), "     more");
    t.typ("zz");
    assert_eq!(t.text(), text);
}

#[test]
fn vim_keys_of_evil_org_and_doom() {
    let vim = "editor.keymap_profile = \"vim\"\n";
    // M-l and M-h demote and promote a heading; M-j moves its subtree.
    let text = "* A\n* B\n- [ ] x\n- [ ] y\n";
    let mut t = open_with(text, vim, 2);
    t.key(KeyCode::Char('l'), KeyModifiers::ALT);
    assert_eq!(t.text(), "** A\n* B\n- [ ] x\n- [ ] y\n");
    t.key(KeyCode::Char('h'), KeyModifiers::ALT);
    t.key(KeyCode::Char('j'), KeyModifiers::ALT);
    assert_eq!(t.text(), "* B\n- [ ] x\n- [ ] y\n* A\n");
    // On an item, M-j moves the item; Enter toggles its checkbox.
    let mut t = open_with(text, vim, text.find("x").unwrap());
    t.key(KeyCode::Char('j'), KeyModifiers::ALT);
    assert_eq!(t.text(), "* A\n* B\n- [ ] y\n- [ ] x\n");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "* A\n* B\n- [ ] y\n- [X] x\n");
    // Enter on a heading with a TODO keyword: done, then not done again;
    // on a plain line it does nothing (it does not move either).
    let text = "* TODO Task\nplain\n";
    let mut t = open_with(text, vim, 3);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.text().starts_with("* DONE Task\n"), "{}", t.text());
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.text().starts_with("* TODO Task\n"), "{}", t.text());
    t.app.doc.move_cursor(t.text().find("plain").unwrap(), false);
    let before = t.text();
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), before);
}

#[test]
fn toggle_heading_toggle_item_and_remove_link() {
    // Text to a heading under the entry, and back.
    let text = "* A\nsome text\n";
    let mut t = open(text, 6);
    t.app.run_command("org.headline.toggle", serde_json::json!({}));
    assert_eq!(t.text(), "* A\n** some text\n");
    t.app.run_command("org.headline.toggle", serde_json::json!({}));
    assert_eq!(t.text(), "* A\nsome text\n");
    // A TODO heading to an item with a checkbox; the item back to text.
    let text = "* TODO Buy milk :home:\n";
    let mut t = open(text, 3);
    t.app.run_command("list.toggleItem", serde_json::json!({}));
    assert_eq!(t.text(), "- [ ] Buy milk\n");
    t.app.run_command("list.toggleItem", serde_json::json!({}));
    assert_eq!(t.text(), "[ ] Buy milk\n");
    // A link gives way to its description.
    let text = "See [[https://orgmode.org][Org]] now\n";
    let mut t = open(text, 8);
    t.app.run_command("org.link.remove", serde_json::json!({}));
    assert_eq!(t.text(), "See Org now\n");
}
