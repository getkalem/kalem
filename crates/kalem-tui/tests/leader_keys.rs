//! Doom Emacs's leader keys pressed in the terminal editor with Vim keys:
//! every row of `tests/keys/doom-leader.toml` and
//! `tests/keys/doom-localleader.toml` that names a command runs that
//! command, in a project, and the command applies there. The settings and
//! state folders are the test's own.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::key_tables::KeyRow;
use kalem_core::keys::KeySequence;
use kalem_core::settings::{Config, Layer};
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::{Path, PathBuf};

/// Commands that start another program (a terminal, a browser, Finder,
/// TeX): their keys are checked without pressing them.
const OUTSIDE: &[&str] = &[
    "app.terminal",
    "file.reveal",
    "export.htmlBrowser",
    "search.online",
    "latex.build",
    "latex.showInPdf",
];

const ORG: &str = "\
#+TITLE: Leader
* TODO Head :work:
Some text with a [[https://orgmode.org][link]].
- one
- [ ] two
| a | b |
|---+---|
| 1 | 2 |
** Child
More.
* Second
";

const MARKDOWN: &str = "\
# Head

Some *text* here.

- [ ] task

| a | b |
|---|---|
| 1 | 2 |
";

struct T {
    app: App,
    term: Terminal<TestBackend>,
    root: PathBuf,
    n: usize,
}

impl T {
    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| app.draw(f)).unwrap();
    }

    fn press(&mut self, keys: &str) {
        let seq = KeySequence::parse(keys).unwrap_or_else(|| panic!("{keys}"));
        for c in seq.0 {
            let mut m = KeyModifiers::NONE;
            if c.mods.ctrl {
                m |= KeyModifiers::CONTROL;
            }
            if c.mods.alt {
                m |= KeyModifiers::ALT;
            }
            let code = match c.key.as_str() {
                "space" => KeyCode::Char(' '),
                "tab" => KeyCode::Tab,
                "enter" => KeyCode::Enter,
                "escape" => KeyCode::Esc,
                "backspace" => KeyCode::Backspace,
                k => {
                    let ch = k.chars().next().unwrap();
                    assert_eq!(k.chars().count(), 1, "{keys}");
                    if c.mods.shift {
                        m |= KeyModifiers::SHIFT;
                        KeyCode::Char(ch.to_ascii_uppercase())
                    } else {
                        KeyCode::Char(ch)
                    }
                }
            };
            self.app.event(Event::Key(KeyEvent::new(code, m)));
            self.draw();
        }
    }

    /// Closes what the last command opened and shows a new copy of
    /// `text`, saved as `NAME.EXT`, the cursor at `at`.
    fn fresh(&mut self, text: &str, ext: &str, at: usize) {
        for _ in 0..3 {
            self.press("escape");
        }
        self.app.quit = false;
        self.app.restart = false;
        self.n += 1;
        let path = self.root.join(format!("proj/doc{}.{ext}", self.n));
        std::fs::write(&path, text).unwrap();
        self.app.open_path(&path, None);
        self.app.run_command("view.widen", serde_json::json!({}));
        self.app.doc.move_cursor(at, false);
        self.draw();
    }

    /// Presses `row`'s keys and says what went wrong, if anything.
    fn check(&mut self, row: &KeyRow, command: &str) -> Option<String> {
        self.press(&row.keys);
        let ran = self.app.last_command().map(str::to_string);
        let status = self.app.status_message().map(|(m, e)| (m.to_string(), e));
        let what = format!("{} ({})", row.theirs, row.keys);
        if ran.as_deref() != Some(command) {
            return Some(format!("{what}: ran {ran:?}, not {command}; {status:?}"));
        }
        match status {
            Some((m, true)) if m.contains("does not apply") || m.contains("Unknown command") => {
                Some(format!("{what}: {m}"))
            }
            _ => None,
        }
    }
}

/// A project with an Org document, the notes folder and settings of the
/// test's own, the Vim keys on.
fn app(root: &Path) -> T {
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(root.join("proj/.git")).unwrap();
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::write(root.join("notes/n.org"), "* Note\n").unwrap();
    std::fs::write(root.join("proj/a.org"), ORG).unwrap();
    let settings = format!(
        "editor.keymap_profile = \"vim\"\nkeys.hints_delay = 0\nnotes.directory = {:?}\n",
        root.join("notes").display().to_string()
    );
    // `SPC h r r` reads the settings again: the same ones.
    std::fs::write(
        config.join("settings.toml"),
        format!(
            "[ui]\nlanguage = \"en\"\n[editor]\nkeymap_profile = \"vim\"\n[keys]\nhints_delay = 0\n[notes]\ndirectory = {:?}\n",
            root.join("notes").display().to_string()
        ),
    )
    .unwrap();
    let mut caps = Caps::full();
    caps.kitty_keyboard = false;
    let mut app = App::with_keymap(
        Some(&root.join("proj/a.org")),
        Config::from_layers(&[(Layer::User, None, &settings)]),
        caps,
        &[],
        Vec::new(),
    )
    .unwrap();
    app.config_dir = Some(config);
    app.projects = kalem_core::projects::ProjectState::load(Some(root.join("projects.toml")));
    app.projects.add(&root.join("proj")).unwrap();
    let term = Terminal::new(TestBackend::new(100, 20)).unwrap();
    let mut t = T {
        app,
        term,
        root: root.to_path_buf(),
        n: 0,
    };
    t.draw();
    t
}

#[test]
fn every_bound_doom_key_runs_its_command() {
    let root = std::env::temp_dir().join(format!("kalem-leader-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = kalem_core::projects::normal(&root);
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }
    kalem_core::kalem_fs::set_trash_dir(Some(root.join("trash")));
    let mut t = app(&root);
    let mut wrong = Vec::new();
    let mut pressed = 0;
    let mut unsendable = 0;
    // The leader map, in an Org document of the project; deleting the
    // file last.
    let mut rows: Vec<KeyRow> = kalem_core::key_tables::doom_leader();
    rows.sort_by_key(|r| r.command.as_deref() == Some("file.delete"));
    for row in &rows {
        let Some(command) = row.command.as_deref() else {
            continue;
        };
        if OUTSIDE.contains(&command) {
            continue;
        }
        // Keys a terminal may not send (`SPC w C-h`, Ctrl+H being
        // Backspace in some) are the graphical editor's; `SPC w h` does
        // the same here.
        if !KeySequence::parse(&row.keys).is_some_and(|k| k.terminal_safe()) {
            unsendable += 1;
            continue;
        }
        t.fresh(ORG, "org", ORG.find("Head").unwrap());
        pressed += 1;
        wrong.extend(t.check(row, command));
    }
    // The local leader, in a document of each kind, the cursor where the
    // row's command applies.
    let csv = "a,b\n1,2\n3,4\n";
    let latex =
        "\\documentclass{article}\n\\begin{document}\n\\section{A}\nText $x$.\n\\end{document}\n";
    for row in kalem_core::key_tables::doom_local_leader() {
        let Some(command) = row.command.as_deref() else {
            continue;
        };
        if OUTSIDE.contains(&command) {
            continue;
        }
        let rest = row.keys.strip_prefix("space m ").unwrap_or_default();
        match row.mode.as_deref() {
            Some("org") => {
                let at = if rest.starts_with('b') {
                    ORG.find("| 1").unwrap() + 2
                } else if rest == "x" || rest == "+" {
                    ORG.find("[ ] two").unwrap() + 4
                } else {
                    ORG.find("Head").unwrap()
                };
                t.fresh(ORG, "org", at)
            }
            Some("markdown") => {
                let at = if rest.starts_with('b') {
                    MARKDOWN.find("| 1").unwrap() + 2
                } else if rest == "x" || rest == "t x" {
                    MARKDOWN.find("task").unwrap()
                } else {
                    MARKDOWN.find("text").unwrap()
                };
                t.fresh(MARKDOWN, "md", at)
            }
            Some("csv") => t.fresh(csv, "csv", 0),
            Some("latex") => t.fresh(latex, "tex", latex.find("Text").unwrap()),
            m => panic!("{}: mode {m:?}", row.theirs),
        }
        pressed += 1;
        wrong.extend(t.check(&row, command));
    }
    assert!(pressed > 300, "{pressed}");
    assert!(unsendable < 3, "{unsendable}");
    assert!(
        wrong.is_empty(),
        "{} of {pressed}:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    what_the_keys_do(&mut t);
    let _ = std::fs::remove_dir_all(&root);
}

/// What some of the keys do, beyond running their command.
fn what_the_keys_do(t: &mut T) {
    let title = |t: &T| t.app.open_files()[t.app.active_index()].title.clone();
    // SPC b - narrows to the element at the cursor, and again widens.
    t.fresh(ORG, "org", ORG.find("Some text").unwrap());
    t.press("space b -");
    let r = t.app.doc.narrowing.clone().expect("narrowed");
    assert!(ORG[r.clone()].starts_with("Some text"), "{:?}", &ORG[r]);
    t.press("space b -");
    assert_eq!(t.app.doc.narrowing, None);
    // SPC m n stores a link to the heading, SPC m l S inserts it.
    t.fresh(ORG, "org", ORG.find("Second").unwrap());
    t.press("space m n");
    t.app.doc.move_cursor(ORG.find("More.").unwrap(), false);
    t.press("space m l shift+s");
    assert!(
        t.app.doc.text().as_str().contains("*Second][Second]]More."),
        "{}",
        t.app.doc.text().as_str()
    );
    // SPC b X is the scratch document, SPC p X the project's.
    t.fresh(ORG, "org", 0);
    t.press("space b shift+x");
    assert_eq!(title(t), "scratch.org");
    t.press("space b l");
    t.press("space p shift+x");
    assert!(title(t).starts_with("proj-"), "{}", title(t));
    // SPC q F closes every document, as Doom's "clear the frame"; those
    // with unsaved changes stay.
    t.fresh(ORG, "org", 0);
    let before = t.app.open_files();
    assert!(before.iter().filter(|f| !f.modified).count() > 2);
    t.press("space q shift+f");
    assert!(!t.app.quit);
    // Every saved document without changes is closed; the window keeps
    // an empty one to show.
    let after = t.app.open_files();
    assert!(
        after.iter().all(|f| f.modified || f.path.is_none()),
        "{after:?}"
    );
    let changed = |l: &[kalem_core::projects::OpenFile]| l.iter().filter(|f| f.modified).count();
    assert_eq!(changed(&after), changed(&before));
    // SPC n F shows the notes folder in the file manager.
    t.press("space n shift+f");
    assert_eq!(
        t.app.doc.dired.as_deref().map(|d| d.title()).as_deref(),
        Some("notes/")
    );
}
