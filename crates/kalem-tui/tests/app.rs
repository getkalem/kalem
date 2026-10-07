//! The terminal editor end to end, on ratatui's test backend.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use kalem_core::settings::{Config, Layer};
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use std::path::{Path, PathBuf};

fn strip_osc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            while let Some(d) = it.next() {
                if d == '\x1b' && it.peek() == Some(&'\\') {
                    it.next();
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

struct T {
    app: App,
    term: Terminal<TestBackend>,
    dir: Option<std::path::PathBuf>,
}

impl Drop for T {
    fn drop(&mut self) {
        if let Some(d) = &self.dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// A configuration folder of the test's own in `dir`, its settings in
/// English: saving or reloading the settings neither touches the user's
/// files nor switches the shared interface language to the system's.
fn test_config(dir: &Path) -> PathBuf {
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("settings.toml"), "[ui]\nlanguage = \"en\"\n").unwrap();
    config
}

/// Files the tests trash go to a folder of their own, not to the user's
/// trash (and at once, not after the trash service).
fn test_trash() {
    kalem_core::kalem_fs::set_trash_dir(Some(
        std::env::temp_dir().join(format!("kalem-tui-trash-{}", std::process::id())),
    ));
}

fn with_config(text: &str, config: Config, size: (u16, u16)) -> T {
    with_file(text, "t.org", config, size)
}

/// A terminal editor on `text` saved as `name` in a new folder.
fn with_file(text: &str, name: &str, config: Config, size: (u16, u16)) -> T {
    test_trash();
    let dir = std::env::temp_dir().join(format!(
        "kalem-tui-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let mut app = App::with_keymap(Some(&path), config, Caps::full(), &[], Vec::new()).unwrap();
    app.config_dir = Some(test_config(&dir));
    let term = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir),
    };
    t.draw();
    t
}

fn open(text: &str) -> T {
    with_config(text, Config::default(), (60, 10))
}

impl T {
    fn draw(&mut self) -> Buffer {
        let app = &mut self.app;
        self.term.draw(|f| app.draw(f)).unwrap();
        self.term.backend().buffer().clone()
    }

    fn row(&mut self, y: u16) -> String {
        let buf = self.draw();
        (0..buf.area.width)
            .map(|x| strip_osc(buf[(x, y)].symbol()))
            .collect::<String>()
            .trim_end()
            .to_string()
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

    fn at(&mut self, pos: usize) {
        self.app.doc.move_cursor(pos, false);
        self.app.editor.follow = true;
        self.draw();
    }

    fn text(&self) -> String {
        self.app.doc.text().as_str().to_string()
    }
}

#[test]
fn styles_links_and_reveal() {
    let text = "Some *bold* and /it/ and [[https://orgmode.org][Org]] here\n* TODO Head\nend\n";
    let mut t = open(text);
    t.at(text.len());
    assert_eq!(t.row(0), " Some bold and it and Org here");
    let buf = t.draw();
    assert!(buf[(6, 0)].modifier.contains(Modifier::BOLD));
    assert!(buf[(15, 0)].modifier.contains(Modifier::ITALIC));
    // Every cell of a link carries it, with one id for the link.
    let cells: Vec<String> = (22..25).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    for c in &cells {
        assert!(
            c.starts_with("\x1b]8;id=") && c.contains(";https://orgmode.org\x1b\\"),
            "{c:?}"
        );
        assert!(c.ends_with("\x1b]8;;\x1b\\"));
    }
    let id = |c: &str| c.split(';').nth(1).map(str::to_string);
    assert!(cells.iter().all(|c| id(c) == id(&cells[0])));
    assert_eq!(t.row(1), " ◉ TODO Head");
    assert_eq!(buf[(3, 1)].fg, Color::Red);
    // The cursor inside the bold text reveals its markers.
    t.at(7);
    assert_eq!(t.row(0), " Some *bold* and it and Org here");
}

#[test]
fn typing_undo_and_keymap() {
    let mut t = open("* A\nx\n");
    t.at(5);
    t.typ("yz");
    assert_eq!(t.text(), "* A\nxyz\n");
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(t.text(), "* A\nxy\n");
    // Ctrl+Z undoes the typing group.
    t.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "* A\nx\n");
    // Ctrl+Enter is Ctrl+T in a terminal without the kitty protocol.
    t.at(1);
    t.key(KeyCode::Char('t'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "* TODO A\nx\n");
    // Alt+I is italic in a terminal (Ctrl+I is Tab there).
    t.at(9);
    t.key(KeyCode::Char('X'), KeyModifiers::SHIFT);
    t.key(KeyCode::Home, KeyModifiers::NONE);
    t.key(KeyCode::End, KeyModifiers::SHIFT);
    assert_eq!(t.app.doc.selected_text(), Some("Xx"));
    t.key(KeyCode::Char('i'), KeyModifiers::ALT);
    assert_eq!(t.text(), "* TODO A\n/Xx/\n");
}

#[test]
fn motion_skips_hidden_markup() {
    let mut t = open("Some *bold* text\n");
    // Away from the markup the star is hidden: one step to its edge, where
    // the cursor touches the bold text and reveals it, then into it.
    t.at(4);
    t.key(KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 5);
    assert_eq!(t.row(0), " Some *bold* text");
    t.key(KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 6);
    // Leaving it hides the stars again; the step skips the closing one.
    t.at(12);
    t.key(KeyCode::Left, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 11);
    t.key(KeyCode::Left, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 10);
    t.key(KeyCode::End, KeyModifiers::NONE);
    t.key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 17);
    t.key(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 16);
}

#[test]
fn folding_with_tab() {
    let mut t = open("* A\nbody\n** B\nmore\n* C\n");
    t.at(1);
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.row(0), " * A …");
    assert_eq!(t.row(1), " ◉ C");
    // Children, then everything.
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.row(1), "   ○ B …");
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.row(1), "   body");
    // Shift+Tab on a heading: the overview.
    t.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!((t.row(0), t.row(1)), (" * A …".into(), " ◉ C".into()));
    // Moving into folded text unfolds it.
    t.at(6);
    assert_eq!(t.row(1), "   body");
}

#[test]
fn clicking_a_checkbox() {
    let mut t = open("- [ ] task\nend\n");
    t.at(14);
    assert_eq!(t.row(0), " • ☐ task");
    t.app.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(t.text(), "- [X] task\nend\n");
}

#[test]
fn table_of_contents() {
    let text = "#+TOC: headlines 2\n* One\n** One A\n* Two\n";
    let mut t = open(text);
    t.at(text.len() - 1);
    assert_eq!(
        (1..4).map(|r| t.row(r)).collect::<Vec<_>>(),
        [" 1 One", "    1.1 One A", " 2 Two"]
    );
    assert_eq!(t.row(0), " Contents");
    // A click on a row leads to its heading.
    t.app.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: 2,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(t.app.doc.selection.head, 25);
    // On the cursor's line, the keyword is itself.
    t.at(3);
    assert_eq!(t.row(0), " #+TOC: headlines 2");
}

#[test]
fn prompts_for_arguments() {
    let mut t = open("see here\n");
    t.at(4);
    t.key(KeyCode::Char('k'), KeyModifiers::CONTROL);
    let buf = t.draw();
    let last: String = (0..buf.area.width)
        .map(|x| buf[(x, 9)].symbol().to_string())
        .collect();
    assert!(
        last.trim_start().starts_with("Insert Link link: "),
        "{last:?}"
    );
    t.typ("https://x.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "see [[https://x.org]]here\n");
}

#[test]
fn saving() {
    let mut t = open("* A\n");
    let path = t.app.doc.meta.path.clone().unwrap();
    t.at(3);
    t.typ("!");
    t.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "* A!\n");
    assert!(!t.app.doc.is_modified());
    // Quitting with unsaved changes asks first.
    t.typ("?");
    t.key(KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert!(!t.app.quit);
    t.key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(t.app.quit);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "* A!\n");
}

/// Save As takes a name beside the document, and asks before it
/// replaces another file (it wrote over it).
#[test]
fn save_as_takes_the_new_names_mode() {
    // Typed as Org, saved as `x.py`: Python from then on, not after
    // opening it again.
    let mut t = open("def f():\n    return 1\n");
    assert_eq!(t.app.doc.meta.mode, kalem_core::DocumentMode::Org);
    let current = t.app.doc.meta.path.clone().unwrap();
    t.app.run_command("app.saveAs", serde_json::json!({}));
    for _ in 0..current.display().to_string().chars().count() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ(&current.with_file_name("x.py").display().to_string());
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        t.app.doc.meta.mode,
        kalem_core::DocumentMode::Text {
            language: Some("py".into())
        }
    );
}

#[test]
fn save_as_does_what_a_save_does_first() {
    // `editor.trim_trailing_whitespace` on Save As too, as on Save.
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        "editor.trim_trailing_whitespace = true\n",
    )]);
    let mut t = with_config("* A   \nbody  \n", config, (60, 10));
    let current = t.app.doc.meta.path.clone().unwrap();
    let target = current.with_file_name("trimmed.org");
    t.app.run_command("app.saveAs", serde_json::json!({}));
    for _ in 0..current.display().to_string().chars().count() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ(&target.display().to_string());
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "* A\nbody\n");
}

#[test]
fn save_as_asks_before_replacing() {
    let mut t = open("* A\n");
    let dir = t
        .app
        .doc
        .meta
        .path
        .clone()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::write(dir.join("other.org"), "keep me\n").unwrap();
    // The prompt starts with the document's path: erased, then the name.
    let save_as = |t: &mut T, name: &str| {
        let current = t.app.doc.meta.path.clone().unwrap();
        t.app.run_command("app.saveAs", serde_json::json!({}));
        for _ in 0..current.display().to_string().chars().count() {
            t.key(KeyCode::Backspace, KeyModifiers::NONE);
        }
        t.typ(name);
        t.key(KeyCode::Enter, KeyModifiers::NONE);
    };
    // Asked (a long path may not fit the line): No leaves the file.
    save_as(&mut t, "other.org");
    t.key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(
        std::fs::read_to_string(dir.join("other.org")).unwrap(),
        "keep me\n"
    );
    save_as(&mut t, "other.org");
    t.key(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(
        std::fs::read_to_string(dir.join("other.org")).unwrap(),
        "* A\n"
    );
    // A new name, relative: beside the document, without a question.
    save_as(&mut t, "copy.org");
    assert_eq!(
        std::fs::read_to_string(dir.join("copy.org")).unwrap(),
        "* A\n"
    );
}

#[test]
fn vim_visual_mode_grows_with_page_down() {
    // Page Down and the arrows with Alt in visual mode grow the selection
    // from its start; the editor's own motion ended it.
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
    let mut t = with_config(&text, config, (40, 10));
    t.at(0);
    t.typ("vl");
    t.key(KeyCode::PageDown, KeyModifiers::NONE);
    let sel = t.app.doc.selection;
    assert!(status(&mut t).starts_with("VISUAL"), "{}", status(&mut t));
    assert_eq!(sel.anchor, 0);
    assert!(sel.head > "line 0\nline 1\n".len(), "{sel:?}");
    t.key(KeyCode::Down, KeyModifiers::ALT);
    t.key(KeyCode::PageUp, KeyModifiers::NONE);
    assert!(status(&mut t).starts_with("VISUAL"), "{}", status(&mut t));
    assert_eq!(t.app.doc.selection.anchor, 0);
}

#[test]
fn vim_profile() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let mut t = with_config("one two\nthree\n", config, (40, 5));
    t.at(0);
    assert!(status(&mut t).starts_with("NORMAL"), "{}", status(&mut t));
    assert!(t.app.take_output().contains(&"\x1b[2 q".to_string()));
    // Keys are commands; typed characters are not text.
    t.typ("wdw");
    assert_eq!(t.text(), "one \nthree\n");
    t.typ("uA!");
    // The mode as a label, without Vim's dashes.
    assert!(status(&mut t).starts_with("INSERT"), "{}", status(&mut t));
    assert!(t.app.take_output().contains(&"\x1b[6 q".to_string()));
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(t.text(), "one two!\nthree\n");
    // Control is Vim's: Ctrl+S is not the Word-like Save; `:w` saves.
    t.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(t.app.doc.is_modified());
    t.typ(":w");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(!t.app.doc.is_modified());
    // `:q!` closes without asking.
    t.typ("x:q!");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.app.quit);
}

#[test]
fn vim_block_selection() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let mut t = with_config("abcd\nefgh\n", config, (40, 5));
    t.at(1);
    t.key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    t.typ("jl");
    assert!(
        status(&mut t).starts_with("VISUAL BLOCK"),
        "{}",
        status(&mut t)
    );
    // Columns 1 and 2 of both lines are painted as selected, not more.
    let buf = t.draw();
    let reversed = |x: u16, y: u16| buf[(x, y)].modifier.contains(Modifier::REVERSED);
    let (x0, y0) = (0..buf.area.width)
        .flat_map(|x| (0..buf.area.height).map(move |y| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == "b")
        .expect("b drawn");
    assert!(reversed(x0, y0) && reversed(x0 + 1, y0), "b and c");
    assert!(reversed(x0, y0 + 1) && reversed(x0 + 1, y0 + 1), "f and g");
    assert!(
        !reversed(x0 - 1, y0) && !reversed(x0 + 2, y0 + 1),
        "a and h"
    );
    t.typ("d");
    assert_eq!(t.text(), "ad\neh\n");
}

#[test]
fn table_selection_statistics() {
    let text = "| a | 2 |\n| b | 4 |\n";
    let mut t = with_config(text, Config::default(), (100, 6));
    t.app.doc.selection = org_edit::Selection {
        anchor: text.find('2').unwrap(),
        head: text.find('4').unwrap(),
    };
    let s = status(&mut t);
    assert!(s.contains("Count: 2   Sum: 6   Average: 3"), "{s}");
}

#[test]
fn long_lines_wrap_and_scroll() {
    let body = "word ".repeat(40);
    let text = format!("- {body}\n{}", "line\n".repeat(30));
    let mut t = with_config(&text, Config::default(), (30, 6));
    t.at(0);
    // Wrapped rows of an item hang after the bullet.
    assert!(t.row(1).starts_with("   word"), "{:?}", t.row(1));
    t.at(text.len());
    // The last five lines: four `line` lines and the empty one.
    assert_eq!(t.app.editor.viewport.top, text.len() - 5 * 4);
}

#[test]
fn tables_as_grids() {
    let text = "| Name | Qty |\n|---+---|\n| *apple* | 3 |\n| b | 10 |\nafter\n";
    let mut t = open(text);
    t.at(text.len());
    // Away from the cursor: aligned, markup hidden, numbers on the right.
    assert_eq!(t.row(0), " │ Name  │ Qty │");
    assert_eq!(t.row(1), " ├───────┼─────┤");
    assert_eq!(t.row(2), " │ apple │   3 │");
    assert_eq!(t.row(3), " │ b     │  10 │");
    // Being edited: the source with bars.
    t.at(2);
    assert_eq!(t.row(2), " │ *apple* │ 3 │");
    assert_eq!(t.row(1), " ├───┼───┤");
}

#[test]
fn wide_tables_wrap_in_their_cells() {
    // Wider than the screen: the long column narrowed, its cell wrapping
    // in it, every row's bars in line (they wrapped row by row).
    let text = "| Path | Contents |\n|---|---|\n| `a` | one two three four five six seven eight nine ten |\n| b | short |\n\nafter\n";
    let mut t = with_file(text, "t.md", Config::default(), (40, 12));
    t.at(text.len());
    let rows: Vec<String> = (0..7).map(|y| t.row(y)).collect();
    assert_eq!(
        rows,
        [
            "1  │ Path │ Contents                  │",
            "2  ├──────┼───────────────────────────┤",
            "3  │ a    │ one two three four five   │",
            "   │      │ six seven eight nine ten  │",
            "4  │ b    │ short                     │",
            "5",
            "6  after",
        ]
    );
    // Org's tables the same.
    let text = "| Path | Contents |\n|------+----------|\n| a | one two three four five six seven eight nine ten |\nafter\n";
    let mut t = with_file(text, "t.org", Config::default(), (40, 12));
    t.at(text.len());
    let rows: Vec<String> = (0..5).map(|y| t.row(y)).collect();
    assert_eq!(
        rows,
        [
            " │ Path │ Contents                    │",
            " ├──────┼─────────────────────────────┤",
            " │ a    │ one two three four five six │",
            " │      │ seven eight nine ten        │",
            " after",
        ]
    );
}

#[test]
fn markdown_display_math_over_several_lines() {
    // One formula on its first line away from the cursor (here, without
    // pictures, its Unicode form), its other lines hidden; with the
    // cursor in it, its source.
    let text = "Text.\n\n$$\na^2 +\nb^2\n$$\n\nAfter.\n";
    let mut t = with_file(text, "t.md", Config::default(), (40, 10));
    t.at(text.len());
    let rows: Vec<String> = (0..5).map(|y| t.row(y)).collect();
    assert_eq!(rows, ["1  Text.", "2", "3    a² + b²", "7", "8  After."]);
    t.at(text.find("a^2").unwrap());
    let rows: Vec<String> = (2..6).map(|y| t.row(y)).collect();
    assert_eq!(rows, ["3  $$", "4  a^2 +", "5  b^2", "6  $$"]);
}

#[test]
fn grid_editing() {
    let text = "| ab   | c |\n| d    | e |\n";
    let mut t = open(text);
    t.at(4);
    t.typ("x");
    assert_eq!(t.text(), "| abx  | c |\n| d    | e |\n");
    // Tab realigns the table and moves to the next field; typing there
    // replaces it.
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    t.typ("Z");
    assert_eq!(t.text(), "| abx | Z |\n| d   | e |\n");
    // Enter goes to the next row.
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.text().line_of(t.app.doc.selection.head), 1);
}

#[test]
fn blocks_drawers_and_settings() {
    let text = "#+TITLE: Doc\n#+AUTHOR: Ada\n#+OPTIONS: toc:nil\n#+STARTUP: showall\n* H\n:PROPERTIES:\n:ID: 1\n:END:\n#+begin_src rust\nfn main() {}\n#+end_src\nend\n";
    let mut t = with_config(text, Config::default(), (40, 12));
    t.at(text.len());
    let rows: Vec<String> = (0..9).map(|y| t.row(y)).collect();
    // Under the heading, lines start at its title; frames fill the 36
    // columns left of the 38 of text.
    let top = format!("   ╭─ rust {}", "─".repeat(28));
    let bottom = format!("   ╰─{}", "─".repeat(34));
    assert_eq!(
        rows,
        [
            " Doc",
            " Ada",
            " #+OPTIONS: toc:nil …",
            " ◉ H",
            "   :PROPERTIES: …",
            &top,
            "   fn main() {}",
            &bottom,
            "   end",
        ]
    );
    // Inside the drawer, it opens; inside the block, its lines show.
    let drawer = text.find(":ID:").unwrap();
    t.at(drawer);
    assert_eq!(
        (t.row(5), t.row(6)),
        ("   :ID: 1".to_string(), "   :END:".to_string())
    );
    let code = text.find("fn main").unwrap();
    t.at(code);
    assert_eq!(t.row(5), "   #+begin_src rust");
}

#[test]
fn source_blocks_are_highlighted() {
    let text = "#+begin_src rust\nfn main() { 42 }\n#+end_src\nend\n";
    let mut t = open(text);
    t.at(text.len());
    let buf = t.draw();
    // `fn` is a keyword, `42` a number.
    assert_eq!(buf[(1, 1)].fg, Color::LightRed);
    assert_eq!(buf[(13, 1)].fg, Color::LightMagenta);
}

fn mouse(t: &mut T, kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) {
    t.app.event(Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    }));
    t.draw();
}

#[test]
fn selecting_with_mouse_and_keys() {
    let mut t = open("alpha beta gamma\nsecond line\n");
    t.at(17);
    // Click, drag: a selection.
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        1,
        0,
        KeyModifiers::NONE,
    );
    mouse(
        &mut t,
        MouseEventKind::Drag(MouseButton::Left),
        6,
        0,
        KeyModifiers::NONE,
    );
    assert_eq!(t.app.doc.selected_text(), Some("alpha"));
    let buf = t.draw();
    assert!(buf[(2, 0)].modifier.contains(Modifier::REVERSED));
    // Shift+click extends it.
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        11,
        0,
        KeyModifiers::SHIFT,
    );
    assert_eq!(t.app.doc.selected_text(), Some("alpha beta"));
    // Double click: a word.
    t.at(20);
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        8,
        0,
        KeyModifiers::NONE,
    );
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        8,
        0,
        KeyModifiers::NONE,
    );
    assert_eq!(t.app.doc.selected_text(), Some("beta"));
    // Word motion with Control, selection with Shift.
    t.at(0);
    t.key(KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(t.app.doc.selection.head, 6);
    t.key(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert_eq!(t.app.doc.selected_text(), Some("beta "));
    t.key(KeyCode::Left, KeyModifiers::CONTROL);
    assert_eq!(t.app.doc.selection.head, 6);
    // Copy and paste through the clipboard.
    t.key(KeyCode::End, KeyModifiers::SHIFT);
    t.key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    // Copying also reaches the system clipboard through OSC 52.
    assert_eq!(t.app.take_output(), ["\x1b]52;c;YmV0YSBnYW1tYQ==\x07"]);
    t.key(KeyCode::End, KeyModifiers::CONTROL);
    t.key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "alpha beta gamma\nsecond line\nbeta gamma");
    // Wheel scrolling moves the view, not the cursor.
    let head = t.app.doc.selection.head;
    mouse(&mut t, MouseEventKind::ScrollDown, 1, 0, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, head);
    assert_eq!(t.app.editor.viewport.top, 29);
}

#[test]
fn enter_continues_and_ends_lists() {
    let mut t = open("- [ ] one\n");
    t.at(9);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("two");
    assert_eq!(t.text(), "- [ ] one\n- [ ] two\n");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("after");
    assert_eq!(t.text(), "- [ ] one\n- [ ] two\nafter\n");
}

#[test]
fn pasting_tables() {
    let mut t = open("text\n");
    t.at(4);
    // A terminal's paste of spreadsheet cells.
    t.app.event(Event::Paste("a\tbb\n1\t2\n".into()));
    assert_eq!(t.text(), "text\n| a | bb |\n| 1 |  2 |\n");
    assert_eq!(t.app.doc.selection.head, 26);
    // Plain text stays as it is, and so does text in a source block.
    let mut t = open("#+begin_src sh\n\n#+end_src\n");
    t.at(15);
    t.app.event(Event::Paste("a\tb".into()));
    assert_eq!(t.text(), "#+begin_src sh\na\tb\n#+end_src\n");
}

#[test]
fn completion_menus() {
    let mut t = open("* Intro\n\n");
    t.at(8);
    t.typ("#+ti");
    let buf = t.draw();
    let menu: String = (0..buf.area.width)
        .map(|x| buf[(x, 2)].symbol().to_string())
        .collect();
    assert!(menu.contains("#+title:"), "{menu:?}");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("Doc");
    assert_eq!(t.text(), "* Intro\n#+title: Doc\n");
    // Links to headings.
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("see [[In");
    t.key(KeyCode::Down, KeyModifiers::NONE);
    t.key(KeyCode::Up, KeyModifiers::NONE);
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.text(), "* Intro\n#+title: Doc\nsee [[*Intro]]\n");
    // Esc closes the menu; typing goes on.
    t.typ(" [[");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.typ("x");
    assert!(t.text().ends_with("[[*Intro]] [[x\n"));
}

#[test]
fn formula_preview() {
    let text = "Energy $E=mc^2$ here\n";
    let mut t = open(text);
    t.at(9);
    assert_eq!(t.row(1).trim(), "= E=mc²");
}

#[test]
fn source_view() {
    let text = "* Head\nSome *bold* text\n";
    let mut t = open(text);
    t.at(text.len());
    assert_eq!(t.row(1), "   Some bold text");
    // Ctrl+/ arrives as Ctrl+_ in a terminal.
    t.key(KeyCode::Char('7'), KeyModifiers::CONTROL);
    // With line numbers.
    assert_eq!(
        (t.row(0), t.row(1)),
        ("1  * Head".to_string(), "2  Some *bold* text".to_string())
    );
    // Org highlighting: the heading in its level's style, bold text bold
    // with its markers.
    let buf = t.draw();
    assert!(buf[(5, 0)].modifier.contains(Modifier::BOLD));
    assert!(buf[(10, 1)].modifier.contains(Modifier::BOLD));
    assert!(!buf[(4, 1)].modifier.contains(Modifier::BOLD));
    t.key(KeyCode::Char('7'), KeyModifiers::CONTROL);
    assert_eq!(t.row(0), " ◉ Head");
}

fn status(t: &mut T) -> String {
    let buf = t.draw();
    let y = buf.area.height - 1;
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect::<String>()
        .trim()
        .to_string()
}

#[test]
fn command_palette() {
    let mut t = open("* A\n");
    t.at(1);
    t.key(KeyCode::F(1), KeyModifiers::NONE);
    t.typ("cycle todo");
    let buf = t.draw();
    let row: String = (0..buf.area.width)
        .map(|x| buf[(x, 2)].symbol().to_string())
        .collect();
    assert!(row.contains("TODO: Cycle TODO State"), "{row:?}");
    assert!(row.contains("ctrl+t"), "{row:?}");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "* TODO A\n");
    // Commands that do not apply here are not offered (Insert Column is
    // for tables; "insert row" would find Insert Item Below, which
    // applies).
    t.at(0);
    t.key(KeyCode::F(1), KeyModifiers::NONE);
    t.typ("insert column");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "* TODO A\n");
}

#[test]
fn export_dialog() {
    let mut t = open("* A\n");
    let shown = |t: &mut T, filter: &str| {
        t.key(
            KeyCode::Char('e'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        t.typ(filter);
        let s = screen(t).join("\n");
        t.key(KeyCode::Esc, KeyModifiers::NONE);
        s
    };
    // The formats and the settings, each found by typing.
    for (filter, item) in [
        ("html", "Export as HTML"),
        ("github", "Export as GitHub Markdown"),
        ("latex", "Export as LaTeX"),
        ("word", "Export as Word (pandoc)"),
        ("body", "Body only: off"),
        ("formulas", "Formulas: MathJax"),
    ] {
        let s = shown(&mut t, filter);
        assert!(s.contains(item), "{item} in {s}");
    }
    assert!(!screen(&mut t).join("\n").contains("Body only"));
}

#[test]
fn find_and_replace() {
    let text = "one two one\nthree one\n";
    let mut t = open(text);
    t.at(0);
    t.key(KeyCode::Char('f'), KeyModifiers::CONTROL);
    t.typ("one");
    assert_eq!(t.app.doc.selected_text(), Some("one"));
    assert_eq!(status(&mut t), "Find: one  1/3");
    let buf = t.draw();
    assert_eq!(buf[(9, 0)].bg, Color::Yellow);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.anchor, 8);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.anchor, 18);
    t.key(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.anchor, 8);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selected_text(), Some("one"));
    // Replace: Alt+H in a terminal (Ctrl+H is Backspace there).
    t.at(0);
    t.key(KeyCode::Char('h'), KeyModifiers::ALT);
    assert_eq!(status(&mut t), "Find: one  Replace:   1/3");
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    t.typ("1");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "1 two one\nthree one\n");
    t.key(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(t.text(), "1 two 1\nthree 1\n");
    t.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "1 two one\nthree one\n");
    // Regular expressions: Alt+R, with `$1` in the replacement.
    t.at(0);
    t.key(KeyCode::Char('h'), KeyModifiers::ALT);
    t.key(KeyCode::Char('r'), KeyModifiers::ALT);
    for _ in 0..3 {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("(o");
    assert_eq!(
        status(&mut t),
        "Find (regex): (o  Replace:   Unclosed group"
    );
    t.typ(r"\w+)e$");
    assert_eq!(status(&mut t), r"Find (regex): (o\w+)e$  Replace:   1/2");
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    t.typ("<$1>");
    t.key(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(t.text(), "1 two <on>\nthree <on>\n");
}

#[test]
fn outline_panel() {
    let text = "* One\ntext\n** TODO Two\n* Three\nend\n";
    let mut t = with_config(text, Config::default(), (60, 8));
    t.at(0);
    // Ctrl+Shift+O is Alt+O in a terminal.
    t.key(KeyCode::Char('o'), KeyModifiers::ALT);
    let row = |t: &mut T, y: u16| -> String {
        let buf = t.draw();
        (0..20)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    };
    assert_eq!(row(&mut t, 0), "One                │");
    assert_eq!(row(&mut t, 1), "  TODO Two         │");
    t.key(KeyCode::Down, KeyModifiers::NONE);
    t.key(KeyCode::Down, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, text.find("* Three").unwrap());
    // Typing goes to the editor again.
    t.typ("x");
    assert!(t.text().contains("x* Three"));
}

#[test]
fn images_in_the_terminal() {
    let text = "Before\n[[file:dot.png]]\nafter [[file:dot.png]] inline\n";
    let mut t = open(text);
    let dir = t
        .app
        .doc
        .meta
        .path
        .as_ref()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    image::RgbaImage::from_pixel(16, 32, image::Rgba([255, 0, 0, 255]))
        .save(dir.join("dot.png"))
        .unwrap();
    // Without a graphics protocol: the name.
    t.at(0);
    assert_eq!(t.row(1), " [image: dot.png]");
    // With kitty's: the image over its own rows (two, with 10 × 20
    // pixel cells); images inside text stay names.
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    t.app.editor.images.borrow_mut().picker = Some(picker);
    let buf = t.draw();
    let cells: String = (0..3)
        .map(|y| buf[(1, y + 1)].symbol().to_string())
        .collect();
    assert!(cells.contains("\x1b_G"), "{cells:?}");
    assert_eq!(t.row(3), " after [image: dot.png] inline");
    // The cursor on the line shows the link.
    t.at(9);
    assert_eq!(t.row(1), " [[file:dot.png]]");
}

#[test]
fn status_line_counts_words() {
    let text = "#+TITLE: T\n* One *two*\nthree four\n** Five\nsix\n";
    let mut t = open(text);
    t.at(text.find("six").unwrap());
    let buf = t.draw();
    let line: String = (0..buf.area.width)
        .map(|x| buf[(x, buf.area.height - 1)].symbol().to_string())
        .collect();
    assert!(line.contains("7 words, 2 in section"), "{line}");
}

#[test]
fn dates_and_tags() {
    let mut t = open("#+TAGS: work home\n* A\n");
    t.at(21);
    // Alt+Shift+D asks for a date as text.
    t.key(KeyCode::Char('D'), KeyModifiers::ALT | KeyModifiers::SHIFT);
    assert!(
        status(&mut t).starts_with("Insert Date date:"),
        "{}",
        status(&mut t)
    );
    t.typ("2026-12-24");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "#+TAGS: work home\n* A<2026-12-24 Thu>\n");
    // Tags complete after a colon at the end of a headline.
    t.typ(" :w");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let text = t.text();
    let line = text.lines().nth(1).unwrap();
    assert!(line.ends_with(" :work:") && line.len() == 77, "{line:?}");
}

#[test]
fn theme_colors() {
    use kalem_core::theme::ThemeColors;
    let text = "* TODO Head\nSee [[https://x.org][site]]\n";
    let dir = std::env::temp_dir().join(format!("kalem-tui-theme-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.org");
    std::fs::write(&path, text).unwrap();
    let theme = ThemeColors::builtin(true);
    let caps = Caps {
        colors: Some(std::sync::Arc::new(theme.clone())),
        ..Caps::full()
    };
    let mut app = App::with_keymap(Some(&path), Config::default(), caps, &[], Vec::new()).unwrap();
    app.doc.move_cursor(text.len(), false);
    let mut term = Terminal::new(TestBackend::new(40, 5)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    let rgb = |c: kalem_core::theme::Color| {
        let (r, g, b) = c.rgb();
        Color::Rgb(r, g, b)
    };
    let row: String = (0..40).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    let todo = row.find("TODO").expect("the keyword") as u16;
    assert_eq!(buf[(todo, 0)].fg, rgb(theme.todo));
    assert_eq!(buf[(todo + 6, 0)].fg, rgb(theme.levels[0]));
    let row: String = (0..40).map(|x| strip_osc(buf[(x, 1)].symbol())).collect();
    let site = row.find("site").expect("the link") as u16;
    assert_eq!(buf[(site, 1)].fg, rgb(theme.link));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn focus_mode_and_text_column() {
    let text = "intro\n* A\na\n* B\nb\n";
    let mut t = open(text);
    t.at(10);
    t.key(KeyCode::F(8), KeyModifiers::NONE);
    let rows: Vec<String> = (0..4).map(|y| t.row(y)).collect();
    assert_eq!(rows[1].trim(), "a", "{rows:?}");
    assert!(
        rows[2].is_empty() && !rows.iter().any(|r| r.contains("intro")),
        "{rows:?}"
    );
    // Off again: the other sections show; the view stays where it was.
    t.key(KeyCode::F(8), KeyModifiers::NONE);
    assert!((0..5).any(|y| t.row(y).contains('B')));
    // A wide terminal with `editor.center_text`: the 80-character column
    // in the middle.
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        "editor.center_text = true\neditor.line_width = 80\n",
    )]);
    let mut t = with_config(text, config, (120, 10));
    let row = t.row(0);
    let indent = row.len() - row.trim_start().len();
    assert_eq!(indent, (120 - 82) / 2 + 1, "{row:?}");
}

#[test]
fn document_modes() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-modes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.txt");
    std::fs::write(&path, "* A\ntext\n").unwrap();
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(app.doc.meta.mode.name(), "text");
    app.run_command("view.setMode", serde_json::json!({ "mode": "org" }));
    assert_eq!(app.doc.meta.mode, kalem_core::DocumentMode::Org);
    assert!(app.doc.parse().is_some());
    let mut term = Terminal::new(TestBackend::new(30, 4)).unwrap();
    app.doc.move_cursor(6, false);
    term.draw(|f| app.draw(f)).unwrap();
    let row: String = (0..30)
        .map(|x| term.backend().buffer()[(x, 0)].symbol().to_string())
        .collect();
    assert!(row.contains("◉ A"), "{row:?}");
    // Remembered in the workspace settings, and used when the file opens.
    let ws = dir.join(".kalem").join("settings.toml");
    assert!(
        std::fs::read_to_string(&ws)
            .unwrap()
            .contains("\"notes.txt\" = \"org\"")
    );
    let config = Config::load(None, Some(&ws));
    let app = App::with_keymap(Some(&path), config, Caps::full(), &[], Vec::new()).unwrap();
    assert_eq!(app.doc.meta.mode, kalem_core::DocumentMode::Org);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn plain_text_view() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-plain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.rs");
    let long = "x".repeat(80);
    let text = format!("fn main() {{\n    let a = 1;\n        // deep\n}}\n{long}\n");
    std::fs::write(&path, &text).unwrap();
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(40, 7)).unwrap();
    let row = |term: &Terminal<TestBackend>, y: u16| -> String {
        (0..40)
            .map(|x| term.backend().buffer()[(x, y)].symbol().to_string())
            .collect()
    };
    term.draw(|f| app.draw(f)).unwrap();
    // Line numbers, syntax colors, a guide every four columns.
    assert!(
        row(&term, 0).starts_with("1  fn main"),
        "{:?}",
        row(&term, 0)
    );
    let buf = term.backend().buffer().clone();
    assert_ne!(
        buf[(3, 0)].fg,
        ratatui::style::Color::Reset,
        "`fn` is a keyword"
    );
    assert!(
        row(&term, 2).starts_with("3      │   // deep"),
        "{:?}",
        row(&term, 2)
    );
    // The cursor's line has a background.
    assert_ne!(buf[(10, 0)].bg, ratatui::style::Color::Reset);
    // The long line wraps; Alt+Z unwraps it, and the view follows the
    // cursor sideways.
    assert!(row(&term, 5).trim_start().starts_with('x'));
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::ALT,
    )));
    app.doc.move_cursor(text.len() - 2, false);
    app.editor.follow = true;
    term.draw(|f| app.draw(f)).unwrap();
    assert!(app.editor.hscroll > 0);
    // One row a line: the empty last line is on the sixth row.
    assert!(row(&term, 5).starts_with('6'), "{:?}", row(&term, 5));
    assert!(row(&term, 4).trim_end().ends_with('x'));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn plain_text_indentation() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-indent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.py");
    std::fs::write(&path, "def f():\n    pass\n").unwrap();
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    app.doc.move_cursor(9, false);
    app.event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(app.doc.text().as_str(), "def f():\n        pass\n");
    app.event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
    )));
    app.event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
    )));
    assert_eq!(app.doc.text().as_str(), "def f():\npass\n");
    let _ = std::fs::remove_dir_all(dir);
}

/// Phase 1 exit criterion: the Org manual opens, is edited and saves
/// without a diff.
#[test]
fn org_manual_round_trip() {
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/corpus/org-mode/org-manual.org"
    );
    let original = std::fs::read(src).unwrap();
    let dir = std::env::temp_dir().join(format!("kalem-tui-manual-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("org-manual.org");
    std::fs::write(&path, &original).unwrap();
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    // Saved as it is.
    app.run_command("app.save", serde_json::Value::Null);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // Typing and deleting in the middle, then saving.
    let middle = app.doc.text().as_str()[..original.len() / 2]
        .rfind("\n\n")
        .unwrap()
        + 1;
    app.doc.move_cursor(middle, false);
    for c in "Kalem".chars() {
        app.event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    term.draw(|f| app.draw(f)).unwrap();
    for _ in 0..5 {
        app.event(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )));
    }
    app.run_command("app.save", serde_json::Value::Null);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn table_formulas() {
    let text = "| a | b | c |\n|---+---+---|\n| 2 | 3 |   |\n| 4 | 5 |   |\n#+TBLFM: $3=$1*$2\n";
    let mut t = with_config(text, Config::default(), (90, 10));
    // F9 recalculates.
    t.at(text.find("| 2").unwrap() + 2);
    t.key(KeyCode::F(9), KeyModifiers::NONE);
    assert_eq!(
        t.text(),
        "| a | b |  c |\n|---+---+----|\n| 2 | 3 |  6 |\n| 4 | 5 | 20 |\n#+TBLFM: $3=$1*$2\n"
    );
    // In a computed field the status line shows the formula, and the
    // fields it refers to are marked.
    let now = t.text();
    t.at(now.find(" 6 |").unwrap() + 1);
    let buf = t.draw();
    let line: String = (0..buf.area.width)
        .map(|x| buf[(x, buf.area.height - 1)].symbol().to_string())
        .collect();
    assert!(line.contains("$3 = $1*$2"), "{line}");
    assert_eq!(t.app.editor.references.len(), 2);
    // F2 edits it, starting from the formula.
    t.key(KeyCode::F(2), KeyModifiers::NONE);
    assert!(status(&mut t).contains("=$1*$2"), "{}", status(&mut t));
    for _ in 0.."*$2".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("+$2");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        t.text(),
        "| a | b | c |\n|---+---+---|\n| 2 | 3 | 5 |\n| 4 | 5 | 9 |\n#+TBLFM: $3=$1+$2\n"
    );
}

#[test]
fn tables_from_and_to_files() {
    let mut t = with_config("Text\n", Config::default(), (90, 10));
    let dir = t.dir.clone().unwrap();
    std::fs::write(dir.join("in.csv"), "a,b\n\"x, y\",2\n").unwrap();
    t.at(4);
    let id = "table.import";
    t.app
        .run_command(id, serde_json::json!({ "file": "in.csv" }));
    // As in Emacs: the table on a line of its own, the line break after
    // the cursor kept.
    assert_eq!(t.text(), "Text\n| a    | b |\n| x, y | 2 |\n\n");
    t.app
        .run_command("table.export", serde_json::json!({ "file": "out.csv" }));
    assert_eq!(
        std::fs::read_to_string(dir.join("out.csv")).unwrap(),
        "a,b\n\"x, y\",2\n"
    );
    // Sorting rows by the column at point.
    t.at(t.text().find("| x").unwrap() + 2);
    t.app
        .run_command("table.sortRows", serde_json::json!({ "by": "A" }));
    assert_eq!(t.text(), "Text\n| x, y | 2 |\n| a    | b |\n\n");
    // Semicolons and a line break in a quoted field: read as CSV mode
    // reads them (Emacs's reader would make one column).
    let mut t = with_config("Text\n", Config::default(), (90, 10));
    let dir = t.dir.clone().unwrap();
    std::fs::write(dir.join("tr.csv"), "ad;not\nAyşe;\"iki\nsatır\"\n").unwrap();
    t.at(4);
    t.app
        .run_command("table.import", serde_json::json!({ "file": "tr.csv" }));
    assert_eq!(
        t.text(),
        "Text\n| ad   | not       |\n|------+-----------|\n| Ayşe | iki satır |\n\n"
    );
}

#[test]
fn formulas_as_images() {
    let text = "Top\n\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n$$x^2$$\nafter\n";
    let mut t = open(text);
    t.at(0);
    // Without a graphics protocol: the environment's Unicode approximation
    // on its first line, as in a LaTeX document.
    assert_eq!(t.row(1), "   a = b ; c = d");
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    t.app.editor.images.borrow_mut().picker = Some(picker);
    let buf = t.draw();
    let cells: String = (0..3)
        .map(|y| buf[(1, y + 1)].symbol().to_string())
        .collect();
    assert!(cells.contains("\x1b_G"), "{cells:?}");
    // The environment's other lines do not show.
    let rows: Vec<String> = (0..9).map(|y| t.row(y)).collect();
    assert!(!rows.iter().any(|r| r.contains("c &= d")), "{rows:?}");
    assert!(rows.iter().any(|r| r.contains("after")), "{rows:?}");
    // In the environment: its source.
    t.at(text.find("c &=").unwrap());
    let rows: Vec<String> = (0..9).map(|y| t.row(y)).collect();
    assert!(rows.iter().any(|r| r.contains("c &= d")), "{rows:?}");
    // The preview toggled off: sources everywhere.
    t.at(0);
    t.app
        .run_command("view.toggleMath", serde_json::Value::Null);
    let rows: Vec<String> = (0..9).map(|y| t.row(y)).collect();
    assert!(rows.iter().any(|r| r.contains("$$x^2$$")), "{rows:?}");
    assert!(rows.iter().any(|r| r.contains("c &= d")), "{rows:?}");
}

/// A terminal editor on `proj/a.org` in a temporary folder with a project
/// `proj` (`a.org`, `sub/b.org`) and `loose.org` outside it, in a legacy
/// terminal (no kitty keyboard protocol).
fn project_app(config: Config) -> (T, std::path::PathBuf) {
    test_trash();
    let dir = std::env::temp_dir().join(format!(
        "kalem-tui-proj-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("proj/sub")).unwrap();
    let dir = kalem_core::projects::normal(&dir);
    std::fs::write(dir.join("proj/a.org"), "* A\nalpha\n").unwrap();
    std::fs::write(dir.join("proj/sub/b.org"), "* B\nbeta\nthe needle here\n").unwrap();
    std::fs::write(dir.join("loose.org"), "* Loose\n").unwrap();
    let mut caps = Caps::full();
    caps.kitty_keyboard = false;
    let mut app =
        App::with_keymap(Some(&dir.join("proj/a.org")), config, caps, &[], Vec::new()).unwrap();
    app.projects = kalem_core::projects::ProjectState::load(Some(dir.join("projects.toml")));
    app.projects.add(&dir.join("proj")).unwrap();
    let term = Terminal::new(TestBackend::new(80, 12)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir.clone()),
    };
    t.draw();
    (t, dir)
}

/// The active document's name; for the file manager, its folder's.
fn title(t: &T) -> String {
    match t.app.doc.dired.as_deref() {
        Some(d) => d.title(),
        None => t.app.open_files()[t.app.active_index()].title.clone(),
    }
}

/// Lets background walks and searches finish.
fn settle(t: &mut T) {
    for _ in 0..500 {
        t.app.tick(std::time::Instant::now());
        if t.app.timeout(std::time::Instant::now()) > std::time::Duration::from_millis(30) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    t.draw();
}

fn screen(t: &mut T) -> Vec<String> {
    let buf = t.draw();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect()
}

#[test]
fn folder_tree_in_the_terminal() {
    let (mut t, dir) = project_app(Config::default());
    // The project's files are found in the background.
    let mut rows = screen(&mut t);
    for _ in 0..300 {
        if rows
            .iter()
            .any(|r| r.contains("b.org") || r.contains("▸ sub"))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        rows = screen(&mut t);
    }
    let at = |rows: &[String], text: &str| rows.iter().position(|r| r.contains(text));
    let folders = at(&rows, "Folders").unwrap_or_else(|| panic!("{rows:?}"));
    assert!(rows[folders + 1].starts_with(" ▸ sub"), "{rows:?}");
    assert!(rows[folders + 2].starts_with("   a.org"), "{rows:?}");
    // A click opens the folder, another opens its file.
    let click = |t: &mut T, row: usize| {
        t.app.event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: row as u16,
            modifiers: KeyModifiers::NONE,
        }));
    };
    click(&mut t, folders + 1);
    let rows = screen(&mut t);
    assert!(rows[folders + 1].starts_with(" ▾ sub"), "{rows:?}");
    assert!(rows[folders + 2].starts_with("     b.org"), "{rows:?}");
    click(&mut t, folders + 2);
    assert_eq!(title(&t), "b.org");
    assert_eq!(
        t.app.doc.meta.path.as_deref(),
        Some(dir.join("proj/sub/b.org").as_path())
    );
    // Closed again, Reveal in Folder Tree opens it down to the file.
    let rows = screen(&mut t);
    let sub = at(&rows, "▾ sub").unwrap();
    click(&mut t, sub);
    assert!(!screen(&mut t).join("\n").contains("     b.org"));
    t.app
        .run_command("view.revealInTree", serde_json::Value::Null);
    assert!(screen(&mut t).join("\n").contains("     b.org"));
    // The setting hides it.
    let (mut t, _) = project_app(Config::from_layers(&[(
        Layer::User,
        None,
        "[ui]\nfolder_tree = false\n",
    )]));
    settle(&mut t);
    assert!(!screen(&mut t).join("\n").contains("Folders"));
}

#[test]
fn documents_and_projects_in_the_terminal() {
    let (mut t, dir) = project_app(Config::default());
    // The palette opens with Ctrl+G where Ctrl+Shift+P cannot be typed,
    // and the status line says so.
    let status_line = status(&mut t);
    assert!(status_line.ends_with("Ctrl+G  commands"), "{status_line}");
    t.key(KeyCode::Char('g'), KeyModifiers::CONTROL);
    assert!(t.app.context_flag("editorFocus"));
    let rows = screen(&mut t);
    assert!(rows[1].contains("> "), "{rows:?}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    // Ctrl+O asks for a file, starting in the document's folder.
    t.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    let last = status(&mut t);
    assert!(
        last.starts_with("Open file: ")
            && last.ends_with(&format!("proj{}", std::path::MAIN_SEPARATOR)),
        "{last}"
    );
    for _ in 0.."proj/".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("loose.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "loose.org");
    // The open files on the left: the project with its file, then the
    // loose one.
    let rows = screen(&mut t);
    assert!(rows[1].contains("▾ proj"), "{rows:?}");
    assert!(rows[2].starts_with("   a.org"), "{rows:?}");
    assert!(rows[3].starts_with(" loose.org"), "{rows:?}");
    // A click shows a document; Ctrl+PageDown and Ctrl+PageUp step.
    t.app.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row: 2,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(title(&t), "a.org");
    t.key(KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(title(&t), "loose.org");
    t.key(KeyCode::PageUp, KeyModifiers::CONTROL);
    assert_eq!(title(&t), "a.org");
    // Ctrl+P: the project's files.
    t.key(KeyCode::Char('p'), KeyModifiers::CONTROL);
    settle(&mut t);
    t.typ("b.o");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "b.org");
    // Ctrl+Alt+O: the open documents.
    t.key(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    t.typ("loose");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "loose.org");
    // Search in Project from outside a project: the projects first.
    t.app.run_command("project.search", serde_json::Value::Null);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("needle");
    settle(&mut t);
    let rows = screen(&mut t);
    assert!(
        rows.iter()
            .any(|r| r.replace('\\', "/").contains("sub/b.org:3") && r.contains("the needle here")),
        "{rows:?}"
    );
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "b.org");
    assert_eq!(t.app.doc.text().line_col(t.app.doc.selection.head).0, 2);
    // Closing asks about unsaved changes, then shows a neighbor.
    t.typ("!");
    t.key(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(status(&mut t).contains("b.org"));
    t.key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(t.app.open_files().len(), 2);
    // Quitting with changes in a background document asks too.
    t.typ("?");
    t.key(KeyCode::PageDown, KeyModifiers::CONTROL);
    t.key(KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert!(!t.app.quit);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    // The status line names the project.
    let _ = dir;
    t.key(KeyCode::PageDown, KeyModifiers::CONTROL);
    let s = status(&mut t);
    assert!(s.contains("proj ▸ a.org") || s.contains("loose.org"), "{s}");
}

#[test]
fn the_which_key_panel_shows_once_its_delay_has_passed() {
    // The loop draws only when something changed: the delay passing is
    // such a change.
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        "editor.keymap_profile = \"vim\"\nkeys.hints_delay = 50\n",
    )]);
    let mut t = with_config("* A\n", config, (80, 24));
    t.typ(" ");
    assert!(!screen(&mut t).join("\n").contains("+file"));
    t.app.tick(std::time::Instant::now());
    assert!(!t.app.dirty);
    std::thread::sleep(std::time::Duration::from_millis(60));
    t.app.tick(std::time::Instant::now());
    assert!(t.app.dirty);
    assert!(screen(&mut t).join("\n").contains("+file"));
    // Shown: not drawn again for it.
    t.app.tick(std::time::Instant::now());
    assert!(!t.app.dirty);
}

#[test]
fn doom_keys_in_the_terminal() {
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        "editor.keymap_profile = \"vim\"\nkeys.hints_delay = 0\n",
    )]);
    let (mut t, dir) = project_app(config);
    t.app.run_command(
        "file.open",
        serde_json::json!({ "path": dir.join("loose.org").display().to_string() }),
    );
    assert_eq!(title(&t), "loose.org");
    // Space shows what follows above the status line, with Doom's group
    // names; SPC b p goes back.
    t.typ(" ");
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("p → +project") && rows.contains("f → +file"),
        "{rows}"
    );
    t.typ("bp");
    assert_eq!(title(&t), "a.org");
    // `:bn`, `gt`, and SPC p f.
    t.typ(":bn");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "loose.org");
    t.typ("gt");
    assert_eq!(title(&t), "a.org");
    t.typ(" pf");
    settle(&mut t);
    t.typ("b.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "b.org");
    // `:e FILE` opens relative to the document.
    t.typ(":e ../../loose.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "loose.org");
    assert_eq!(t.text(), "* Loose\n");
    // SPC ': the list again (here, outside the project, the projects),
    // with what was typed in it.
    t.typ(" '");
    settle(&mut t);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains(": b.org"), "{rows}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    // SPC ~: no panel yet, then the outline shown and hidden again.
    t.typ(" ~");
    assert!(status(&mut t).contains("No panel"), "{}", status(&mut t));
    t.app.run_command("view.outline", serde_json::Value::Null);
    let shown = screen(&mut t).join("\n");
    // (The outline has the keys now.)
    t.app
        .run_command("view.toggleLastPanel", serde_json::Value::Null);
    let hidden = screen(&mut t).join("\n");
    assert_ne!(shown, hidden);
    // SPC u 3: the next command runs three times.
    t.typ(" u3");
    assert!(status(&mut t).contains("Count 3"), "{}", status(&mut t));
    t.app.run_command("edit.newline", serde_json::Value::Null);
    assert_eq!(t.text(), "\n\n\n* Loose\n", "{}", status(&mut t));
}

#[test]
fn old_kalem_formatting_is_plain_org() {
    // What an earlier Kalem wrote is shown as Org shows it: no colors,
    // the attribute line as a line of the file (T2.13.13).
    let text =
        "one @@kalem:color=#c00000@@two@@kalem:end@@ three\n\n#+ATTR_KALEM: :align right\nend\n";
    let mut t = with_file(text, "t.org", Config::default(), (60, 10));
    t.at(text.len());
    let buf = t.draw();
    let rows: Vec<String> = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect();
    let two = rows[0].find("two").unwrap() as u16;
    assert_ne!(buf[(two, 0)].fg, Color::Rgb(0xc0, 0, 0));
    assert!(rows.iter().any(|r| r.contains("ATTR_KALEM")), "{rows:?}");
    // The formatting commands are gone.
    t.app
        .run_command("format.alignRight", serde_json::Value::Null);
    assert_eq!(t.text(), text);
}

#[test]
fn text_starts_at_the_left() {
    // In a wide terminal the text starts at the left edge; an 80-column
    // one goes to the middle with `editor.center_text`.
    let mut t = with_config("* Heading\ntext\n", Config::default(), (160, 10));
    t.at(0);
    let buf = t.draw();
    let row: String = (0..160).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert!(row.starts_with(" * Heading"), "{row:?}");
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        "editor.center_text = true\neditor.line_width = 80\n",
    )]);
    let mut t = with_config("* Heading\ntext\n", config, (160, 10));
    t.at(0);
    let buf = t.draw();
    let row: String = (0..160).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert_eq!(row.find('*'), Some(40), "{row:?}");
}

fn cursor_line(t: &T) -> String {
    let text = t.app.doc.text();
    text.as_str()[text.line_range(text.line_of(t.app.doc.selection.head))].to_string()
}

#[test]
fn file_manager_in_the_terminal() {
    let (mut t, dir) = project_app(Config::default());
    // Ctrl+Alt+D: the document's folder, the cursor on its file.
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(title(&t), "proj/");
    assert!(cursor_line(&t).ends_with(" a.org"), "{}", cursor_line(&t));
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("sub/") && rows.contains("a.org"), "{rows}");
    // Folders are bold.
    let buf = t.draw();
    let (x, y) = (0..buf.area.height)
        .find_map(|y| {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            row.find("sub/").map(|x| (x as u16, y))
        })
        .unwrap();
    assert!(buf[(x, y)].modifier.contains(Modifier::BOLD));
    // `p` goes up a line; Enter lists the folder; `^` goes back up.
    t.typ("p");
    assert!(cursor_line(&t).ends_with(" sub/"), "{}", cursor_line(&t));
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "sub/");
    assert!(t.text().contains(" b.org"));
    t.typ("^");
    assert_eq!(title(&t), "proj/");
    assert!(cursor_line(&t).ends_with(" sub/"));
    // Typing changes nothing.
    t.typ("zz");
    assert!(!t.app.doc.is_modified());
    // `P`: every project as if in one folder; Enter goes into one;
    // Backspace from its folder shows the projects again.
    t.typ("P");
    assert!(t.text().contains("proj"), "{}", t.text());
    assert_eq!(t.app.doc.meta.path, None);
    assert!(cursor_line(&t).starts_with("  proj"), "{}", cursor_line(&t));
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "proj/");
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert!(cursor_line(&t).starts_with("  proj"));
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    // Mark a.org and copy it into sub: the target is asked for.
    let at = t.text().find(" a.org").unwrap() + 1;
    t.at(at);
    t.typ("m");
    assert!(t.text().contains("* "), "{}", t.text());
    t.typ("C");
    for _ in 0..300 {
        t.app.event(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )));
    }
    t.typ(&dir.join("proj/sub").display().to_string());
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    settle(&mut t);
    assert!(dir.join("proj/sub/a.org").is_file());
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("Copied 1 item"), "{rows}");
    // Copy again: a conflict; `k` keeps both.
    t.typ("C");
    for _ in 0..300 {
        t.app.event(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )));
    }
    t.typ(&dir.join("proj/sub").display().to_string());
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("already there"), "{rows}");
    t.typ("k");
    settle(&mut t);
    assert!(dir.join("proj/sub/a (2).org").is_file());
    // Delete for good asks first.
    t.typ("U");
    t.app
        .run_command("dired.newFile", serde_json::json!({ "name": "gone.txt" }));
    assert!(cursor_line(&t).ends_with(" gone.txt"));
    t.app
        .run_command("dired.deletePermanently", serde_json::Value::Null);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("for good"), "{rows}");
    t.typ("y");
    settle(&mut t);
    assert!(!dir.join("proj/gone.txt").exists());
    assert!(!t.text().contains("gone.txt"), "{}", t.text());
    // A rename, then Ctrl+Z takes it back.
    t.app
        .run_command("dired.newFile", serde_json::json!({ "name": "x.txt" }));
    t.app
        .run_command("dired.move", serde_json::json!({ "target": "y.txt" }));
    settle(&mut t);
    assert!(dir.join("proj/y.txt").is_file());
    t.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(dir.join("proj/x.txt").is_file() && !dir.join("proj/y.txt").exists());
    assert!(cursor_line(&t).ends_with(" x.txt"), "{}", cursor_line(&t));
    std::fs::remove_file(dir.join("proj/x.txt")).unwrap();
    t.typ("g");
    // A click on a file's name opens it.
    let buf = t.draw();
    let (x, y) = (0..buf.area.height)
        .find_map(|y| {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            row.find("a.org").map(|x| (x as u16, y))
        })
        .unwrap();
    t.app.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x + 1,
        row: y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(title(&t), "a.org");
    assert_eq!(t.text(), "* A\nalpha\n");
}

#[test]
fn file_manager_with_vim_keys() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, _dir) = project_app(config);
    // SPC o -: the file manager.
    t.typ(" o-");
    assert_eq!(title(&t), "proj/");
    // Vim moves; Dired's keys come first where they are bound.
    t.typ("gg");
    assert!(cursor_line(&t).ends_with(" sub/"), "{}", cursor_line(&t));
    t.typ("j");
    assert!(cursor_line(&t).ends_with(" a.org"), "{}", cursor_line(&t));
    t.typ("m");
    assert!(
        cursor_line(&t).starts_with("* "),
        "`m` marks, and the last line stays: {}",
        cursor_line(&t)
    );
    t.typ("u");
    assert!(!t.text().contains("* "));
    t.typ("k");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "sub/");
    t.typ("-");
    assert_eq!(title(&t), "proj/");
    // SPC p P: the projects.
    t.typ(" pP");
    assert!(t.text().contains("proj"));
    assert_eq!(t.app.doc.meta.path, None);
}

#[test]
fn enter_below_a_table_makes_a_line() {
    // At the end of the text, on the line after the table.
    let mut t = open("| a | b |\n");
    t.at(10);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("x");
    assert_eq!(t.text(), "| a | b |\n\nx");
    // On a blank line between a table and a paragraph.
    let mut t = open("| a | b |\n\nafter\n");
    t.at(10);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "| a | b |\n\n\nafter\n");
}

#[test]
fn switching_to_the_file_manager_and_back() {
    let (mut t, dir) = project_app(Config::default());
    // Ctrl+Alt+D there and back.
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(title(&t), "proj/");
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(title(&t), "a.org");
    // The list of open files offers the file manager and the projects.
    let buf = t.draw();
    let find = |buf: &Buffer, s: &str| {
        (0..buf.area.height).find_map(|y| {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            row.find(s).map(|x| (row[..x].chars().count() as u16, y))
        })
    };
    let (x, y) = find(&buf, "Projects").expect("the projects entry");
    t.app.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }));
    t.draw();
    assert_eq!(t.app.doc.meta.path, None);
    assert!(t.text().contains("proj"));
    // The status line names the view; Ctrl+Alt+D there shows a folder,
    // never the projects again, and up from the projects is a folder
    // (T2.7e.19).
    assert!(status(&mut t).contains("Projects"), "{}", status(&mut t));
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(title(&t), "proj/");
    assert!(
        status(&mut t).contains("File Manager"),
        "{}",
        status(&mut t)
    );
    t.app.run_command("dired.projects", serde_json::Value::Null);
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert!(t.app.doc.meta.path.is_some());
    let _ = dir;
}

#[test]
fn vim_dash_and_explore() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, _dir) = project_app(config);
    // `-` opens the file manager (vim-vinegar); there it goes up.
    t.typ("-");
    assert_eq!(title(&t), "proj/");
    t.typ(" o-");
    assert_eq!(title(&t), "a.org");
    t.typ(":Ex");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "proj/");
    t.typ("-");
    assert_ne!(title(&t), "proj/");
}

#[test]
fn a_file_under_version_control_opens_its_project_when_asked() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-auto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
    std::fs::create_dir_all(dir.join("repo/src")).unwrap();
    std::fs::write(dir.join("repo/src/a.org"), "* A\n").unwrap();
    std::fs::write(dir.join("repo/b.org"), "* B\n").unwrap();
    std::fs::write(dir.join("repo/c.org"), "* C\n").unwrap();
    let dir = kalem_core::projects::normal(&dir);
    let mut app = App::with_keymap(
        Some(&dir.join("repo/src/a.org")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    app.projects = kalem_core::projects::ProjectState::load(Some(dir.join("projects.toml")));
    app.config_dir = Some(test_config(&dir));
    let term = Terminal::new(TestBackend::new(80, 12)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir.clone()),
    };
    // By default projects are added by hand only.
    t.app.open_path(&dir.join("repo/b.org"), None);
    assert!(t.app.projects.list.list.is_empty());
    assert!(!t.app.context_flag("inProject"));
    // Turned on, opening another file of the repository makes it a
    // project; the setting is saved.
    t.app
        .run_command("project.toggleAutoAdd", serde_json::json!({}));
    assert!(t.app.config.bool("projects.auto_add"));
    let saved = std::fs::read_to_string(dir.join("config/settings.toml")).unwrap();
    assert!(saved.contains("auto_add = true"), "{saved}");
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("A folder under version control"), "{rows}");
    t.app.open_path(&dir.join("repo/c.org"), None);
    let names: Vec<String> = t
        .app
        .projects
        .list
        .list
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(names, vec!["repo"]);
    assert!(t.app.context_flag("inProject"));
    // Its files are ready for Ctrl+P.
    t.key(KeyCode::Char('p'), KeyModifiers::CONTROL);
    settle(&mut t);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("a.org") && rows.contains("b.org"), "{rows}");
}

#[test]
fn the_settings_panel_lists_and_changes_every_setting() {
    let mut t = with_config("* A\n", Config::default(), (90, 30));
    let saved = |t: &T| {
        std::fs::read_to_string(t.dir.as_ref().unwrap().join("config/settings.toml")).unwrap()
    };
    t.app.run_command("app.settings", serde_json::json!({}));
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("Settings") && rows.contains("editor") && rows.contains("font_family"),
        "{rows}"
    );
    // The chosen setting's key, default and description below.
    assert!(rows.contains("editor.font_family  Default:"), "{rows}");
    // `/` filters; a switch flips with Space, saved at once.
    t.typ("/auto_add");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("projects") && rows.contains("auto_add"),
        "{rows}"
    );
    assert!(!rows.contains("font_family"), "{rows}");
    t.typ(" ");
    assert!(t.app.config.bool("projects.auto_add"));
    assert!(saved(&t).contains("auto_add = true"), "{}", saved(&t));
    // `d` takes it back to its default, out of the file.
    t.typ("d");
    assert!(!t.app.config.bool("projects.auto_add"));
    assert!(!saved(&t).contains("auto_add"), "{}", saved(&t));
    // A change shows at once: line numbers off.
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.typ("/line_numbers");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.app.editor.line_numbers);
    t.typ("l");
    assert!(!t.app.editor.line_numbers);
    // Choices go round with h and l; Enter asks for text.
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.typ("/latex.engine");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    for _ in 0.."auto".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("tectonic");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.config.str("latex.engine"), "tectonic");
    // Still open; Escape clears the filter, then closes.
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("latex.engine  Default: auto"), "{rows}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(screen(&mut t).join("\n").contains("font_family"));
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!screen(&mut t).join("\n").contains("font_family"));
    assert_eq!(t.text(), "* A\n", "no key reached the document");
}

#[test]
fn lists_and_tables_change_in_the_settings_panel() {
    let mut t = with_config("* A\n", Config::default(), (90, 30));
    t.app.run_command("app.settings", serde_json::json!({}));
    // A list of choices: Enter shows them, Space puts one in.
    t.typ("/vim.modes");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(screen(&mut t).join("\n").contains("Enter shows them"));
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("[ ] org") && rows.contains("Space in or out"),
        "{rows}"
    );
    t.typ(" ");
    assert_eq!(
        t.app.config.get("editor.vim.modes"),
        Some(&serde_json::json!(["org"]))
    );
    assert!(screen(&mut t).join("\n").contains("[x] org"));
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    // A list of texts: `a` adds, `K` moves up, Enter edits, `x` removes.
    t.typ("/todo_keywords");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("a");
    t.typ("WAIT");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let keywords = |t: &T| t.app.config.get("org.todo_keywords").cloned();
    assert_eq!(
        keywords(&t),
        Some(serde_json::json!(["TODO", "|", "DONE", "WAIT"]))
    );
    t.typ("GK");
    assert_eq!(
        keywords(&t),
        Some(serde_json::json!(["TODO", "|", "WAIT", "DONE"]))
    );
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    for _ in 0.."WAIT".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("NEXT");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("kx");
    assert_eq!(
        keywords(&t),
        Some(serde_json::json!(["TODO", "NEXT", "DONE"]))
    );
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    // A table: `path = mode` typed, the mode stepped with l.
    t.typ("/files.modes");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(screen(&mut t).join("\n").contains("(empty)"));
    t.typ("a");
    t.typ("notes.txt = markdown");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        t.app.config.get("files.modes"),
        Some(&serde_json::json!({"notes.txt": "markdown"}))
    );
    t.typ("l");
    assert_eq!(
        t.app.config.get("files.modes"),
        Some(&serde_json::json!({"notes.txt": "csv"}))
    );
    let saved =
        std::fs::read_to_string(t.dir.as_ref().unwrap().join("config/settings.toml")).unwrap();
    assert!(
        saved.contains("notes.txt") && saved.contains("NEXT"),
        "{saved}"
    );
    assert_eq!(t.text(), "* A\n", "no key reached the document");
}

#[test]
fn text_under_a_heading_is_indented_to_its_title() {
    let text = "Before\n* One\nunder one\n*** Three\nunder three\n\n| a | b |\n";
    let mut t = open(text);
    t.at(0);
    let rows = screen(&mut t);
    let row = |s: &str| {
        rows.iter()
            .find(|r| r.contains(s))
            .cloned()
            .unwrap_or_default()
    };
    let col = |r: &str, s: &str| r[..r.find(s).unwrap()].chars().count();
    // The text before the first heading stays at the left.
    assert_eq!(col(&row("Before"), "Before"), col(&row("One"), "◉"));
    // Text lines up with its heading's title.
    assert_eq!(
        col(&row("under one"), "under one"),
        col(&row("One"), "One"),
        "{rows:#?}"
    );
    assert_eq!(
        col(&row("under three"), "under three"),
        col(&row("Three"), "Three"),
        "{rows:#?}"
    );
    // The table too.
    assert!(
        col(&row("│ a"), "│") >= col(&row("Three"), "Three"),
        "{rows:#?}"
    );
    // `#+STARTUP: noindent` turns it off for the file.
    let mut t = open(&format!("#+STARTUP: noindent\n{text}"));
    t.at(0);
    let rows = screen(&mut t);
    let r = rows.iter().find(|r| r.contains("under one")).unwrap();
    assert_eq!(r[..r.find("under one").unwrap()].trim(), "", "{rows:#?}");
    assert!(r.find("under one").unwrap() <= 1, "{rows:#?}");
}

#[test]
fn the_file_manager_is_listed_once() {
    let (mut t, _dir) = project_app(Config::default());
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    let files = t.app.open_files();
    let fm = files
        .iter()
        .find(|f| f.title == "File Manager")
        .expect("listed by its name");
    assert_eq!(fm.path, None, "not under the project");
    // The list at the left names the project once.
    let rows = screen(&mut t);
    let panel: Vec<String> = rows[..rows.len() - 1]
        .iter()
        .map(|r| r.split('│').next().unwrap_or("").to_string())
        .collect();
    assert!(!panel.iter().any(|r| r.contains("proj/")), "{panel:#?}");
    assert_eq!(
        panel.iter().filter(|r| r.contains("proj")).count(),
        1,
        "{panel:#?}"
    );
}

#[test]
fn citations() {
    let text = "#+bibliography: refs.bib\n\nAs [cite:@knuth84] said.\n";
    let mut t = with_config(text, Config::default(), (160, 10));
    std::fs::write(
        t.dir.as_ref().unwrap().join("refs.bib"),
        "@book{knuth84, author = {Donald E. Knuth}, title = {The {\\TeX}book}, year = 1984}\n\
         @article{doe20, author = {Jane Doe}, title = {A study}, journal = {Journal}, year = 2020}\n",
    )
    .unwrap();
    // The entry cited under the cursor, in the status line.
    let knuth = text.find("knuth84").unwrap();
    t.at(knuth);
    assert!(
        status(&mut t).contains("@knuth84: Donald E. Knuth (1984). The TeXbook."),
        "{}",
        status(&mut t)
    );
    // The picker: typing finds an entry, Enter cites it after the
    // citation…
    t.at(text.find(" said").unwrap());
    t.app
        .run_command("org.cite.insert", serde_json::Value::Null);
    t.typ("doe");
    assert!(
        screen(&mut t)
            .join("\n")
            .contains("@doe20  Jane Doe (2020). A study. Journal.")
    );
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        t.app.doc.text().as_str(),
        "#+bibliography: refs.bib\n\nAs [cite:@knuth84][cite:@doe20] said.\n"
    );
    // …or in it.
    t.at(knuth);
    t.app
        .run_command("org.cite.insert", serde_json::Value::Null);
    t.typ("doe");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        t.app.doc.text().as_str(),
        "#+bibliography: refs.bib\n\nAs [cite:@knuth84; @doe20][cite:@doe20] said.\n"
    );
}

#[test]
fn citations_without_a_bibliography() {
    let mut t = open("No bibliography.\n");
    t.app
        .run_command("org.cite.insert", serde_json::Value::Null);
    assert!(
        status(&mut t).contains("No bibliography"),
        "{}",
        status(&mut t)
    );
}

#[test]
fn dropped_pictures() {
    // A terminal pastes the path of a file dropped on it: a picture is
    // copied beside the document and linked.
    let mut t = open("Text.\n");
    let other = std::env::temp_dir().join(format!("kalem-tui-drop-{}", std::process::id()));
    std::fs::create_dir_all(&other).unwrap();
    let pic = other.join("my pic.png");
    std::fs::write(&pic, b"not really a png").unwrap();
    t.at(5);
    t.app.event(Event::Paste(format!("'{}'", pic.display())));
    assert_eq!(
        t.app.doc.text().as_str(),
        "Text.[[file:t_assets/my pic.png]]\n"
    );
    assert!(
        t.dir
            .as_ref()
            .unwrap()
            .join("t_assets/my pic.png")
            .is_file()
    );
    // Other text pastes as it is.
    t.app.event(Event::Paste("/no/such/file.png".into()));
    assert!(t.app.doc.text().as_str().contains("/no/such/file.png"));
}

#[test]
fn footnotes() {
    let mut t = with_config("* Notes\nSome text here.\n", Config::default(), (120, 12));
    // A new footnote: its definition in the footnote section, the cursor
    // in it.
    t.at(17);
    t.key(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(
        t.app.doc.text().as_str(),
        "* Notes\nSome text[fn:1] here.\n\n* Footnotes\n\n[fn:1] \n"
    );
    assert_eq!(t.app.doc.selection.head, 50);
    t.typ("The note.");
    // From the definition's label back to the reference, its text in the
    // status line.
    t.at(46);
    t.app
        .run_command("org.footnote.action", serde_json::Value::Null);
    assert_eq!(t.app.doc.selection.head, 17);
    assert!(
        status(&mut t).contains("Footnote 1: The note."),
        "{}",
        status(&mut t)
    );
}

#[test]
fn scheduling() {
    let mut t = with_config("* TODO Task\nBody\n", Config::default(), (120, 12));
    t.at(3);
    t.app.run_command(
        "org.schedule",
        serde_json::json!({"date": "2026-10-05 +1w"}),
    );
    assert_eq!(
        t.app.doc.text().as_str(),
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon +1w>\nBody\n"
    );
    assert!(
        status(&mut t).contains("Scheduled to <2026-10-05 Mon +1w>"),
        "{}",
        status(&mut t)
    );
    // A new date keeps the repeater.
    t.app.run_command(
        "org.schedule",
        serde_json::json!({"date": "2026-10-12 09:30"}),
    );
    assert_eq!(
        t.app.doc.text().as_str(),
        "* TODO Task\nSCHEDULED: <2026-10-12 Mon 09:30 +1w>\nBody\n"
    );
    t.app
        .run_command("org.deadline", serde_json::json!({"date": "2026-12-24"}));
    t.app
        .run_command("org.schedule.remove", serde_json::Value::Null);
    assert_eq!(
        t.app.doc.text().as_str(),
        "* TODO Task\nDEADLINE: <2026-12-24 Thu>\nBody\n"
    );
}

#[test]
fn editing_properties() {
    let text = "* A\n:PROPERTIES:\n:ID: 42\n:Effort: 1:00\n:END:\nBody\n";
    let mut t = with_config(text, Config::default(), (120, 12));
    t.at(2);
    t.app
        .run_command("org.property.edit", serde_json::Value::Null);
    let s = screen(&mut t).join("\n");
    assert!(s.contains("ID: 42") && s.contains("Effort: 1:00"), "{s}");
    // Choosing a property asks for its value, starting with the old one.
    t.typ("effort");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    for _ in 0.."1:00".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("2:30");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        t.app.doc.text().as_str(),
        "* A\n:PROPERTIES:\n:ID: 42\n:Effort:   2:30\n:END:\nBody\n"
    );
}

#[test]
fn word_completion() {
    let mut t = with_file("quartz quantum\n", "w.txt", Config::default(), (60, 10));
    let end = t.app.doc.text().len();
    t.at(end);
    t.typ("qua");
    let s = screen(&mut t).join("\n");
    assert!(s.contains("quantum") && s.contains("quartz"), "{s}");
    // Tab takes the word; Enter goes on writing.
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.app.doc.text().as_str(), "quartz quantum\nquantum");
    t.typ(" qua");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.text().as_str(), "quartz quantum\nquantum qua\n");
    // Alt+/ asks with one letter.
    t.typ("q");
    t.key(KeyCode::Char('/'), KeyModifiers::ALT);
    let s = screen(&mut t).join("\n");
    assert!(s.contains("quartz"), "{s}");
}

#[test]
fn line_commands() {
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        "editor.trim_trailing_whitespace = true\n",
    )]);
    let mut t = with_file("pear  \napple\nfig\n", "l.txt", config, (60, 8));
    t.at(0);
    t.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(t.app.doc.text().as_str(), "apple\npear  \nfig\n");
    // Terminals send Ctrl+Shift+D as Ctrl+D: from the palette.
    t.app
        .run_command("lines.duplicate", serde_json::Value::Null);
    assert_eq!(t.app.doc.text().as_str(), "apple\npear  \npear  \nfig\n");
    t.key(KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(t.app.doc.text().as_str(), "apple\npear  \npear  \nfig\n");
    // Sorting the whole text, then saving without the trailing blanks.
    t.app.run_command("edit.selectAll", serde_json::Value::Null);
    t.app.run_command("lines.sort", serde_json::Value::Null);
    assert_eq!(t.app.doc.text().as_str(), "apple\nfig\npear  \npear  \n");
    t.app.run_command("app.save", serde_json::Value::Null);
    let path = t.app.doc.meta.path.clone().unwrap();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "apple\nfig\npear\npear\n"
    );
    // Not in CSV, where trailing tabs are empty fields, nor in Markdown,
    // where two trailing spaces break the line.
    for (name, text) in [("t.tsv", "a\tb\t\n"), ("m.md", "one  \ntwo\n")] {
        let config = Config::from_layers(&[(
            Layer::User,
            None,
            "editor.trim_trailing_whitespace = true\n",
        )]);
        let mut t = with_file(text, name, config, (60, 8));
        t.at(0);
        if name.ends_with(".tsv") {
            // Editing the cell (F2), not replacing it.
            t.app
                .run_command("csv.editCell", serde_json::json!({ "here": true }));
        }
        t.typ("x");
        t.app.run_command("app.save", serde_json::Value::Null);
        let path = t.app.doc.meta.path.clone().unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), format!("x{text}"));
    }
    // Join, and a selection grown and shrunk.
    t.at(0);
    t.app.run_command("lines.join", serde_json::Value::Null);
    assert!(t.app.doc.text().as_str().starts_with("apple fig\n"));
    t.at(1);
    t.key(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::ALT);
    assert_eq!(t.app.doc.selected_text(), Some("apple"));
    t.key(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::ALT);
    assert_eq!(t.app.doc.selected_text(), Some("apple fig"));
    t.key(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::ALT);
    assert_eq!(t.app.doc.selected_text(), Some("apple"));
}

#[test]
fn editing_code() {
    let mut t = with_file("fn a() {}\n", "a.rs", Config::default(), (60, 8));
    t.at(8);
    // Enter between braces: the closing one on its own line.
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.text().as_str(), "fn a() {\n    \n}\n");
    t.typ("let x = (1);");
    // The matching brackets are marked.
    t.key(KeyCode::Left, KeyModifiers::NONE);
    t.key(KeyCode::Left, KeyModifiers::NONE);
    let buf = t.draw();
    let marked = (0..buf.area.width)
        .filter(|&x| buf[(x, 1)].bg != buf[(0, 1)].bg)
        .count();
    assert!(marked >= 2, "{marked}");
    // Toggle Comment, twice.
    t.app
        .run_command("edit.toggleComment", serde_json::Value::Null);
    assert_eq!(
        t.app.doc.text().as_str(),
        "fn a() {\n    // let x = (1);\n}\n"
    );
    t.app
        .run_command("edit.toggleComment", serde_json::Value::Null);
    assert_eq!(t.app.doc.text().as_str(), "fn a() {\n    let x = (1);\n}\n");
    // Go to Matching Bracket.
    t.at(7);
    t.app
        .run_command("edit.gotoBracket", serde_json::Value::Null);
    assert_eq!(
        t.app.doc.selection.head,
        t.app.doc.text().as_str().find('}').unwrap() + 1
    );
    // A closing bracket alone on its line goes back a level.
    let end = t.app.doc.text().as_str().find(';').unwrap() + 1;
    t.at(end);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("if y {");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("z");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("}");
    assert_eq!(
        t.app.doc.text().as_str(),
        "fn a() {\n    let x = (1);\n    if y {\n        z\n    }\n}\n"
    );
}

#[test]
fn large_and_long() {
    // Over 4 MB: colored a window at a time.
    let big = "fn main() { let x = 1; }\n".repeat(200_000);
    let mut t = with_file(&big, "big.rs", Config::default(), (60, 6));
    let colored = |t: &mut T| {
        let buf = t.draw();
        (0..buf.area.width).any(|x| buf[(x, 0)].fg != Color::Reset)
    };
    assert!(colored(&mut t));
    let end = t.app.doc.text().len() - 3;
    t.at(end);
    assert!(colored(&mut t));
    // A very long line shows the part around the cursor.
    let long = format!("{}\n", "abc ".repeat(100_000));
    let mut t = with_file(&long, "long.txt", Config::default(), (60, 6));
    t.at(200_000);
    let start = std::time::Instant::now();
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("abc") && rows.contains("1:200001"), "{rows}");
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn multiple_cursors() {
    let mut t = with_file("one\ntwo\nthree\n", "t.txt", Config::default(), (60, 10));
    t.at(0);
    t.key(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::ALT);
    t.key(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::ALT);
    t.typ("- ");
    assert_eq!(t.app.doc.text().as_str(), "- one\n- two\n- three\n");
    // Every cursor moves; typing goes on at each.
    t.key(KeyCode::End, KeyModifiers::NONE);
    t.typ(";");
    assert_eq!(t.app.doc.text().as_str(), "- one;\n- two;\n- three;\n");
    // Escape leaves one.
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(t.app.doc.extra.is_empty());
    // The next occurrence of the word, then typing over both.
    t.at(2);
    t.key(KeyCode::Char('d'), KeyModifiers::CONTROL);
    t.app
        .run_command("selection.addNextOccurrence", serde_json::Value::Null);
    assert_eq!(t.app.doc.extra.len(), 0, "no second \"one\"");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.at(0);
    t.key(KeyCode::Right, KeyModifiers::SHIFT);
    t.app
        .run_command("selection.allOccurrences", serde_json::Value::Null);
    assert_eq!(t.app.doc.extra.len(), 2);
    t.typ("*");
    assert_eq!(t.app.doc.text().as_str(), "* one;\n* two;\n* three;\n");
    // Copied one a line.
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.at(0);
    t.app
        .run_command("selection.columnDown", serde_json::Value::Null);
    assert_eq!(t.app.doc.extra.len(), 1);
}

#[test]
fn legacy_encodings() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-enc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notlar.txt");
    let turkish = "Ağaçların gölgesinde çalışan işçiler, güneşin doğuşunu şarkılarla karşıladı.\n";
    let (bytes, _, _) = kalem_core::encoding_rs::WINDOWS_1254.encode(turkish);
    std::fs::write(&path, &bytes).unwrap();
    let app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir.clone()),
    };
    assert_eq!(t.app.doc.text().as_str(), turkish);
    let s = status(&mut t);
    assert!(s.contains("windows-1254") && s.contains("Not UTF-8"), "{s}");
    // Saved as UTF-8.
    t.app.run_command(
        "file.saveWithEncoding",
        serde_json::json!({"encoding": "UTF-8"}),
    );
    assert_eq!(std::fs::read(&path).unwrap(), turkish.as_bytes());
    assert!(!status(&mut t).contains("windows-1254"));
    // Reopened in another encoding: the UTF-8 bytes read as Latin-1.
    t.app.run_command(
        "file.reopenWithEncoding",
        serde_json::json!({"encoding": "ISO-8859-1"}),
    );
    assert!(
        t.app.doc.text().as_str().starts_with("A\u{c4}\u{178}a"),
        "{:?}",
        t.app.doc.text().as_str()
    );
    assert!(status(&mut t).contains("windows-1252"));
}

#[test]
fn word_targets_and_chapters() {
    let text = "* One\nthree four five\n* Two\nsix\n";
    let mut t = with_config(text, Config::default(), (100, 10));
    t.at(8);
    t.app.run_command(
        "stats.setDocumentTarget",
        serde_json::json!({"words": "1k"}),
    );
    t.app
        .run_command("stats.setSectionTarget", serde_json::json!({"words": "10"}));
    // The counts catch up after a pause in typing.
    std::thread::sleep(std::time::Duration::from_millis(350));
    let s = status(&mut t);
    assert!(
        s.contains("6 of 1,000 (0%) words, 4 of 10 (40%) in section"),
        "{s}"
    );
    t.app.run_command("stats.chapters", serde_json::Value::Null);
    let screen = screen(&mut t).join("\n");
    assert!(screen.contains("4 of 10 (40%)"), "{screen}");
    t.typ("Two");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let text = t.app.doc.text().as_str().to_string();
    assert_eq!(t.app.doc.selection.head, text.find("* Two").unwrap());
    // Go to Line.
    t.app
        .run_command("edit.gotoLine", serde_json::json!({"line": 1}));
    assert_eq!(t.app.doc.selection.head, 0);
}

#[test]
fn copying_as_html() {
    let text = "Some *bold* text.\n";
    let mut t = with_config(text, Config::default(), (80, 10));
    t.at(0);
    t.app.doc.move_cursor(11, true);
    t.app.take_output();
    t.app.run_command("edit.copyHtml", serde_json::Value::Null);
    let out = t.app.take_output().concat();
    let b64 = out
        .trim_start_matches("\x1b]52;c;")
        .trim_end_matches('\x07');
    let html = String::from_utf8(base64_decode(b64)).unwrap();
    assert!(html.contains("<b>bold</b>"), "{html}");
    // Rich text: the terminal takes the plain text.
    t.app
        .run_command("edit.copyRichText", serde_json::Value::Null);
    let out = t.app.take_output().concat();
    let b64 = out
        .trim_start_matches("\x1b]52;c;")
        .trim_end_matches('\x07');
    assert_eq!(
        String::from_utf8(base64_decode(b64)).unwrap(),
        "Some *bold*"
    );
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').map(val).collect();
    bytes
        .chunks(4)
        .flat_map(|c| {
            let n = c.iter().fold(0u32, |a, &v| (a << 6) | v as u32) << (6 * (4 - c.len()));
            let b = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            b[..c.len() - 1].to_vec()
        })
        .collect()
}

#[test]
fn archiving_and_refiling() {
    let text = "* A\n** a1\n* B\nb\n* C\n";
    let mut t = with_config(text, Config::default(), (80, 12));
    t.at(11);
    // The picker lists the headings outside the subtree.
    t.app.run_command("org.refile", serde_json::Value::Null);
    let s = screen(&mut t).join("\n");
    assert!(s.contains("A/a1") && !s.contains("B/"), "{s}");
    t.typ("A/a1");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.text().as_str(), "* A\n** a1\n*** B\nb\n* C\n");
    // Archived under the Archive sibling, and tagged.
    t.at(1);
    t.app
        .run_command("org.archive.toggleTag", serde_json::Value::Null);
    assert!(
        t.app.doc.text().as_str().starts_with("* A")
            && t.app.doc.text().as_str().contains(":ARCHIVE:")
    );
    t.at(t.app.doc.text().as_str().find("*** B").unwrap());
    t.app
        .run_command("org.archive.sibling", serde_json::Value::Null);
    let text = t.app.doc.text().as_str().to_string();
    assert!(
        text.contains("*** Archive") && text.contains("**** B\n:PROPERTIES:\n:ARCHIVE_TIME:"),
        "{text}"
    );
}

#[test]
fn macros_and_snippets() {
    let text = "#+MACRO: v version $1\nThis is {{{v(2)}}} @@html:<br>@@ ok.\nend\n";
    let mut t = with_config(text, Config::default(), (80, 10));
    t.at(text.len() - 1);
    assert!(
        screen(&mut t)
            .join("\n")
            .contains("This is version 2 html:<br> ok."),
        "{:?}",
        screen(&mut t)
    );
    // At the cursor, as written.
    t.at(text.find("{{{").unwrap() + 4);
    assert!(
        screen(&mut t).join("\n").contains("This is {{{v(2)}}}"),
        "{:?}",
        screen(&mut t)
    );
}

#[test]
fn captions_names_and_references() {
    let text = "#+CAPTION: Old\n| 1 |\n\nSee \n";
    let mut t = with_config(text, Config::default(), (80, 10));
    t.at(16);
    // The prompt starts with the caption there.
    t.app
        .run_command("org.caption.set", serde_json::Value::Null);
    for _ in 0.."Old".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("Numbers");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.app
        .run_command("org.name.set", serde_json::json!({"name": "tab:n"}));
    assert_eq!(
        t.app.doc.text().as_str(),
        "#+NAME: tab:n\n#+CAPTION: Numbers\n| 1 |\n\nSee \n"
    );
    // The reference picker lists the name; choosing it links to it.
    let end = t.app.doc.text().len() - 1;
    t.at(end);
    t.app
        .run_command("org.insert.reference", serde_json::Value::Null);
    let s = screen(&mut t).join("\n");
    assert!(s.contains("tab:n  Numbers"), "{s}");
    t.typ("tab:n");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        t.app.doc.text().as_str().ends_with("See [[tab:n]]\n"),
        "{}",
        t.app.doc.text().as_str()
    );
}

#[test]
fn drawers_and_export_blocks() {
    let text = "Intro.\n#+begin_export html\n<b>x</b>\n#+end_export\n";
    let mut t = with_config(text, Config::default(), (60, 10));
    t.at(0);
    // Away from the cursor, an export block shows its back-end.
    assert!(
        screen(&mut t).join("\n").contains("export html"),
        "{:?}",
        screen(&mut t)
    );
    // A drawer at the cursor, the cursor inside it.
    t.at(6);
    t.app
        .run_command("org.insert.drawer", serde_json::json!({"name": "NOTES"}));
    assert_eq!(
        t.app.doc.text().as_str(),
        "Intro.\n:NOTES:\n\n:END:\n\n#+begin_export html\n<b>x</b>\n#+end_export\n"
    );
    assert_eq!(t.app.doc.selection.head, 15);
}

#[test]
fn markdown_reads_as_text() {
    let mut t = with_file(
        "# Notes\n\nSome **bold** and a [link](http://x.org).\n",
        "n.md",
        Config::default(),
        (70, 8),
    );
    t.at(0);
    let s = screen(&mut t);
    // The cursor on the heading shows its `#`; the paragraph hides its
    // markers.
    assert!(s.iter().any(|l| l.contains("# Notes")), "{s:#?}");
    assert!(
        s.iter().any(|l| l.contains("Some bold and a link.")),
        "{s:#?}"
    );
    t.at(15);
    let s = screen(&mut t);
    assert!(
        s.iter().any(|l| l.contains("Notes") && !l.contains('#')),
        "{s:#?}"
    );
    assert!(
        s.iter().any(|l| l.contains("Some **bold** and a link.")),
        "{s:#?}"
    );
}

#[test]
fn markdown_enter_continues_a_list() {
    let mut t = with_file("- [x] one\n", "n.md", Config::default(), (70, 8));
    t.at(9);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    for c in "two".chars() {
        t.key(KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert_eq!(t.app.doc.text().as_str(), "- [x] one\n- [ ] two\n");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    // The list ends with a blank line, so what follows is a paragraph of
    // its own, not a lazy continuation of the item before.
    t.typ("after");
    assert_eq!(t.app.doc.text().as_str(), "- [x] one\n- [ ] two\n\nafter\n");
    // Over a selection: it goes, and the item goes on, undone in one step.
    let mut t = with_file("- one two\n", "s.md", Config::default(), (70, 8));
    t.app.doc.selection = org_edit::Selection { anchor: 2, head: 6 };
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "- \n- two\n");
    assert_eq!(t.app.doc.selection.head, 5);
    t.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(t.text(), "- one two\n");
}

#[test]
fn bookmarks_set_and_jumped_to() {
    // `SPC b m` sets, `SPC RET` jumps (T2.7i.18); kept in the state
    // directory, the line followed when lines are added above it.
    let state = std::env::temp_dir().join(format!("kalem-tui-state-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    kalem_core::bookmarks::use_file(Some(state.join("bookmarks.json")));
    let mut t = with_file("one\ntwo\nthree\n", "b.org", Config::default(), (40, 6));
    t.at(5);
    t.app
        .run_command("bookmark.set", serde_json::json!({"name": "two"}));
    t.at(0);
    t.typ("zero\n");
    t.app
        .run_command("bookmark.goto", serde_json::json!({"name": "two"}));
    // The file was not saved: the bookmark's line as set, the second.
    assert_eq!(t.app.doc.text().line_of(t.app.doc.selection.head), 1);
    t.app
        .run_command("bookmark.delete", serde_json::json!({"name": "two"}));
    assert!(kalem_core::bookmarks::load().is_empty());
    let _ = std::fs::remove_dir_all(&state);
}

#[test]
fn csv_grid() {
    let mut t = with_file(
        "name,age\nAda,36\nBob,7\n",
        "p.csv",
        Config::default(),
        (70, 8),
    );
    let s = screen(&mut t);
    assert!(
        s.iter().any(|l| l.contains("Ada      │       36")),
        "{s:#?}"
    );
    // Tab goes from field to field; the column's numbers in the status bar.
    t.at(9);
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 13);
    assert!(status(&mut t).contains("43"), "{}", status(&mut t));
    t.app
        .run_command("csv.moveRowDown", serde_json::Value::Null);
    assert_eq!(t.app.doc.text().as_str(), "name,age\nBob,7\nAda,36\n");
}

#[test]
fn print_compiles_first() {
    // Without a file there is nothing to compile beside (a test must not
    // reach a real print dialog; `kalem-core/tests/book.rs` covers the rest).
    let mut t = with_file("Hello.\n", "p.org", Config::default(), (70, 8));
    t.app.doc.meta.path = None;
    t.app.run_command("file.print", serde_json::Value::Null);
    let s = status(&mut t);
    assert!(s.contains("Save"), "{s}");
}

#[test]
fn latex_rendered() {
    let text = "\\section{Intro}\nSome \\emph{very} ``good'' text---yes.\n\\begin{enumerate}\n\\item First $a^2$\n\\end{enumerate}\n";
    let mut t = with_file(text, "paper.tex", Config::default(), (60, 8));
    assert_eq!(t.app.doc.meta.mode, kalem_core::DocumentMode::Latex);
    // Away from the heading's command.
    t.at(text.len());
    let s = screen(&mut t);
    assert!(status(&mut t).contains("LaTeX"));
    assert!(s.iter().any(|l| l.contains("1. First a\u{b2}")), "{s:#?}");
    assert!(
        s.iter()
            .any(|l| l.contains("Intro") && !l.contains("\\section")),
        "{s:#?}"
    );
    assert!(
        s.iter()
            .any(|l| l.contains("Some very \u{201c}good\u{201d} text\u{2014}yes.")),
        "{s:#?}"
    );
    // At the command, its markers show for editing.
    let at = text.find("\\emph").unwrap();
    t.at(at);
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.contains("\\emph{very}")), "{s:#?}");
    // The source view shows the text.
    t.app
        .run_command("view.toggleSource", serde_json::Value::Null);
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.contains("\\section{Intro}")), "{s:#?}");
}

#[test]
fn latex_floats() {
    let text = "\\begin{figure}\n\\includegraphics[width=\\linewidth]{fig}\n\\caption{Cats.}\n\\end{figure}\n";
    let mut t = with_file(text, "f.tex", Config::default(), (60, 8));
    let dir = t
        .app
        .doc
        .meta
        .path
        .clone()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::write(dir.join("fig.png"), b"not really").unwrap();
    t.at(text.len());
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.contains("[image: fig.png]")), "{s:#?}");
    assert!(s.iter().any(|l| l.contains("Figure 1: Cats.")), "{s:#?}");
}

#[test]
fn latex_references() {
    let text = "\\section{One}\\label{s}\nSee \\ref{s} and \\cite{knuth}.\n\\bibliography{refs}\n";
    let mut t = with_file(text, "r.tex", Config::default(), (70, 8));
    let dir = t
        .app
        .doc
        .meta
        .path
        .clone()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::write(
        dir.join("refs.bib"),
        "@book{knuth, author = {Knuth, Donald}, title = {The TeXbook}, year = 1984}\n",
    )
    .unwrap();
    t.at(text.len());
    let s = screen(&mut t);
    assert!(
        s.iter().any(|l| l.contains("See 1 and [Knuth 1984].")),
        "{s:#?}"
    );
    // At the citation, the entry in the status line.
    t.at(text.find("\\cite").unwrap() + 3);
    assert!(
        status(&mut t).contains("@knuth: Knuth, Donald. 1984."),
        "{}",
        status(&mut t)
    );
}

#[test]
fn latex_theorems_and_code() {
    let text = "\\newtheorem{thm}{Theorem}\n\\begin{thm}[Main]\nTrue.\n\\end{thm}\n\\begin{proof}\nClear.\n\\end{proof}\n\\begin{verbatim}\n\\emph{raw}\n\\end{verbatim}\n";
    let mut t = with_file(text, "t.tex", Config::default(), (60, 14));
    t.at(text.len());
    let s = screen(&mut t);
    // Without amsthm, LaTeX puts no period after the head.
    for want in ["Theorem 1 (Main) ", "Proof.", "\u{220e}", "\\emph{raw}"] {
        assert!(s.iter().any(|l| l.contains(want)), "{want}: {s:#?}");
    }
    assert!(!s.iter().any(|l| l.contains("(Main).")), "{s:#?}");
}

#[test]
fn latex_build_command() {
    // Without a file there is nothing to build (a real build is
    // `kalem-core`'s and the command line's test).
    let mut t = with_file(
        "\\documentclass{article}\n",
        "b.tex",
        Config::default(),
        (70, 8),
    );
    t.app.doc.meta.path = None;
    t.key(KeyCode::F(5), KeyModifiers::NONE);
    assert!(status(&mut t).contains("Save"), "{}", status(&mut t));
}

#[test]
fn latex_paragraphs_show_as_one() {
    // The lines of a paragraph wrap as one away from the cursor, as TeX
    // sets them; at the cursor, the source's lines.
    let text = "\\documentclass{article}\n\\begin{document}\n\nThe well known theorem was\nproved invalid.\nMeaning this:\n\n\\end{document}\n";
    let mut t = with_file(text, "p.tex", Config::default(), (70, 12));
    t.at(text.find("\\end{document}").unwrap());
    let s = screen(&mut t);
    assert!(
        s.iter()
            .any(|l| l.contains("The well known theorem was proved invalid. Meaning this:")),
        "{s:#?}"
    );
    t.at(text.find("proved").unwrap());
    let s = screen(&mut t);
    assert!(
        s.iter().any(|l| l.trim_end().ends_with("theorem was")),
        "{s:#?}"
    );
    assert!(s.iter().any(|l| l.contains("proved invalid.")), "{s:#?}");
}

#[test]
fn latex_build_saves_first() {
    // The PDF is made from the text in the editor: unsaved changes are
    // saved before LaTeX runs.
    let search = std::env::var_os("PATH").unwrap_or_default();
    if !std::env::split_paths(&search).any(|d| d.join("pdflatex").is_file()) {
        return;
    }
    let mut t = with_file("", "b.tex", Config::default(), (70, 8));
    t.typ("\\documentclass{article}\\begin{document}Hi\\end{document}");
    t.key(KeyCode::F(5), KeyModifiers::NONE);
    let done = kalem_core::jobs::wait_all();
    let path = t.app.doc.meta.path.clone().unwrap();
    assert!(
        std::fs::read_to_string(&path).unwrap().contains("Hi"),
        "{done:?}"
    );
    assert!(!t.app.doc.is_modified());
    assert!(path.with_extension("pdf").is_file(), "{done:?}");
}

#[test]
fn latex_structural_editing() {
    let text = "\\begin{itemize}\n\\item One\n\\end{itemize}\nsome words\n";
    let mut t = with_file(text, "e.tex", Config::default(), (60, 10));
    // Enter continues the list.
    t.at(text.find("One").unwrap() + 3);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("Two");
    assert!(
        t.app
            .doc
            .text()
            .as_str()
            .contains("\\item One\n\\item Two\n")
    );
    // Ctrl+B on a word.
    let at = t.app.doc.text().as_str().find("words").unwrap() + 1;
    t.at(at);
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert!(t.app.doc.text().as_str().contains("some \\textbf{words}"));
    // `\begin{…}` gets its `\end{…}`, the body indented as the
    // document's are (not at all here).
    let end = t.app.doc.text().len();
    t.at(end);
    t.typ("\\begin{center}");
    assert!(
        t.app
            .doc
            .text()
            .as_str()
            .ends_with("\\begin{center}\n\n\\end{center}"),
        "{}",
        t.app.doc.text().as_str()
    );
    // Renaming one end renames the other.
    let at = t.app.doc.text().as_str().rfind("\\begin{center}").unwrap() + "\\begin{cent".len();
    t.at(at);
    t.typ("X");
    assert!(
        t.app
            .doc
            .text()
            .as_str()
            .ends_with("\\begin{centXer}\n\n\\end{centXer}")
    );
    // A heading level from the palette.
    t.at(t.app.doc.text().as_str().find("some").unwrap());
    t.app
        .run_command("latex.section.setLevel", serde_json::json!({"level": 1}));
    assert!(
        t.app
            .doc
            .text()
            .as_str()
            .contains("\\section{some \\textbf{words}}")
    );
}

#[test]
fn latex_completion() {
    let text = "\\section{A}\\label{sec:a}\n";
    let mut t = with_file(text, "c.tex", Config::default(), (60, 12));
    t.at(text.len());
    t.typ("\\begin{ite");
    let s = screen(&mut t).join("\n");
    assert!(s.contains("itemize"), "{s}");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        t.app
            .doc
            .text()
            .as_str()
            .ends_with("\\begin{itemize}\n  \\item \n\\end{itemize}"),
        "{}",
        t.app.doc.text().as_str()
    );
    t.typ("see \\ref{se");
    let s = screen(&mut t).join("\n");
    assert!(s.contains("sec:a"), "{s}");
}

#[test]
fn latex_math_and_inserts() {
    let text = "\\usepackage{booktabs}\nLet \n";
    let mut t = with_file(text, "m.tex", Config::default(), (60, 20));
    t.at(text.find("Let ").unwrap() + 4);
    t.typ("$x$ ok");
    assert!(
        t.app.doc.text().as_str().contains("Let $x$ ok\n"),
        "{}",
        t.app.doc.text().as_str()
    );
    t.app.run_command(
        "latex.insert.table",
        serde_json::json!({"columns": 2, "rows": 1}),
    );
    let s = t.app.doc.text().as_str().to_string();
    assert!(
        s.contains(
            "\\begin{tabular}{ll}\n    \\toprule\n     &  \\\\\n    \\midrule\n    \\bottomrule"
        ),
        "{s}"
    );
    t.app
        .run_command("latex.insert.citation", serde_json::json!({"key": "knuth"}));
    assert!(t.app.doc.text().as_str().contains("\\cite{knuth}"));
}

#[test]
fn latex_new_from_template() {
    let mut t = with_file("notes\n", "n.org", Config::default(), (60, 10));
    t.app.run_command(
        "file.newFromTemplate",
        serde_json::json!({"template": "article"}),
    );
    let path = t.app.doc.meta.path.clone().unwrap();
    assert!(path.ends_with("article.tex"), "{path:?}");
    assert_eq!(t.app.doc.meta.mode, kalem_core::DocumentMode::Latex);
    assert!(
        t.app
            .doc
            .text()
            .as_str()
            .starts_with("\\documentclass[11pt,a4paper]{article}")
    );
    // A second one does not overwrite the first.
    t.app.run_command(
        "file.newFromTemplate",
        serde_json::json!({"template": "article"}),
    );
    assert!(
        t.app
            .doc
            .meta
            .path
            .clone()
            .unwrap()
            .ends_with("article-2.tex")
    );
    let _ = std::fs::remove_file(path.with_file_name("article-2.tex"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn latex_code_colored_by_its_language() {
    let text = "plain words\n\\begin{lstlisting}[language=Python]\ndef f():\n    return 1\n\\end{lstlisting}\n";
    let mut t = with_file(text, "c.tex", Config::default(), (60, 8));
    t.at(0);
    let buf = t.draw();
    // Find `def` on its row.
    let row = (0..buf.area.height)
        .find(|&y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .contains("def f():")
        })
        .expect("the code line shows");
    let col = (0..buf.area.width)
        .find(|&x| buf[(x, row)].symbol() == "d")
        .unwrap();
    let words = (0..buf.area.width)
        .find(|&x| buf[(x, 0)].symbol() == "p")
        .unwrap();
    // The keyword colored as Python; the text as it is.
    assert_ne!(buf[(col, row)].fg, buf[(words, 0)].fg);
    assert_eq!(buf[(words, 0)].fg, Color::Reset);
}

#[test]
fn latex_inline_code_colored() {
    let text = "plain words\nSee \\lstinline[language=Rust]{fn main} here.\n";
    let mut t = with_file(text, "c.tex", Config::default(), (60, 8));
    t.at(0);
    let buf = t.draw();
    let row = (0..buf.area.height)
        .find(|&y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .contains("See fn main here.")
        })
        .expect("the code shows without its command");
    let line: String = (0..buf.area.width)
        .map(|x| buf[(x, row)].symbol().to_string())
        .collect();
    let col = line.find("fn main").unwrap() as u16;
    let see = line.find("See").unwrap() as u16;
    // `fn` in Rust's keyword color, the text around it as it is.
    assert_ne!(buf[(col, row)].fg, buf[(see, row)].fg);
}

#[test]
fn latex_class_front_matter() {
    let text = "\\documentclass{acmart}\n\\begin{document}\n\\title{Deep}\n\\keywords{a, b}\n\\end{document}\n";
    let mut t = with_file(text, "a.tex", Config::default(), (60, 8));
    t.at(0);
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.trim_end().ends_with(" Deep")), "{s:#?}");
    assert!(s.iter().any(|l| l.contains("Keywords: a, b")), "{s:#?}");
}

#[test]
fn latex_formulas_as_images() {
    let text = "Before\n\\begin{equation}\n  E = mc^2\n\\end{equation}\nafter\n";
    let mut t = with_file(text, "i.tex", Config::default(), (60, 12));
    t.at(0);
    // Without graphics: its Unicode approximation on its first line, the
    // others hidden.
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.contains("E = mc²")), "{s:#?}");
    assert!(!s.iter().any(|l| l.contains("\\begin{equation}")), "{s:#?}");
    // With kitty's: the equation as one image on its first line, the
    // others hidden.
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    t.app.editor.images.borrow_mut().picker = Some(picker);
    let buf = t.draw();
    let all: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .map(|p| buf[p].symbol().to_string())
        .collect();
    assert!(all.contains("\x1b_G"), "an image is drawn");
    assert!(!all.contains("E = mc^2"), "its source lines are hidden");
    assert!(all.contains("after"));
}

#[test]
fn latex_outline() {
    let text = "\\section{Intro}\ntext\n\\subsection{Details}\n\\section{End}\n";
    let mut t = with_file(text, "o.tex", Config::default(), (60, 8));
    t.at(text.len());
    t.key(KeyCode::Char('o'), KeyModifiers::ALT);
    let s = screen(&mut t).join("\n");
    assert!(
        s.contains("Intro") && s.contains("1.1") && s.contains("Details"),
        "{s}"
    );
}

#[test]
fn latex_tables_as_grids() {
    let text = "\\begin{tabular}{lr}\n\\toprule\nName & Qty \\\\\n\\midrule\n\\textbf{apple} & 3 \\\\\nb & 10 \\\\\n\\bottomrule\n\\end{tabular}\n\nafter\n";
    let mut t = with_file(text, "t.tex", Config::default(), (40, 12));
    t.at(text.len());
    // The rows after the line numbers.
    let rows: Vec<String> = (0..8).map(|y| t.row(y)[4..].to_string()).collect();
    assert_eq!(rows[2], "│ Name  │ Qty │", "{rows:#?}");
    assert_eq!(rows[3], "├───────┼─────┤");
    assert_eq!(rows[4], "│ apple │   3 │");
    assert_eq!(rows[5], "│ b     │  10 │");
    // In the table: the source, and Tab goes to the next cell.
    t.at(text.find("Name").unwrap());
    assert!(t.row(2).contains("Name & Qty"), "{}", t.row(2));
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, text.find("Qty").unwrap());
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, text.find("\\textbf").unwrap());
    t.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(t.app.doc.selection.head, text.find("Qty").unwrap());
    assert_eq!(t.text(), text);
}

#[test]
fn csv_typing_quotes_the_field() {
    let text = "name,note\napple,red\n";
    let mut t = with_file(text, "d.csv", Config::default(), (60, 8));
    // A comma typed in a field is part of its value: the field is quoted.
    t.at(text.find("red").unwrap() + 3);
    t.app
        .run_command("csv.editCell", serde_json::json!({ "here": true }));
    t.typ(", ripe");
    assert_eq!(t.text(), "name,note\napple,\"red, ripe\"\n");
    // A quote inside the quotes is doubled.
    t.typ(" \"x");
    assert_eq!(t.text(), "name,note\napple,\"red, ripe \"\"x\"\n");
    // In the source view the comma is a delimiter.
    t.app
        .run_command("view.toggleSource", serde_json::json!({}));
    t.at(t.text().find("apple").unwrap() + 5);
    t.typ(",");
    assert!(t.text().contains("apple,,"), "{}", t.text());
}

#[test]
fn bibtex_grid() {
    let text = "% refs\n@book{knuth84,\n  author = {Donald E. Knuth},\n  title = {The {\\TeX}book},\n  year = 1984,\n}\n\n@article{lamport,\n  author = {Lamport, Leslie},\n  title = {Paxos},\n  year = {1998}\n}\n";
    let mut t = with_file(text, "refs.bib", Config::default(), (70, 10));
    t.at(0);
    let s = screen(&mut t);
    // Each entry one row: key, type, authors, title, year; its field lines
    // hidden.
    assert!(
        s.iter()
            .any(|l| l.contains("knuth84 │ book    │ Knuth   │ The TeXbook │ 1984")),
        "{s:#?}"
    );
    assert!(
        s.iter()
            .any(|l| l.contains("lamport │ article │ Lamport │")),
        "{s:#?}"
    );
    assert!(!s.iter().any(|l| l.contains("author =")), "{s:#?}");
    // Sorted by year, descending: Lamport first; the file as it was.
    t.app.run_command(
        "bib.sortView",
        serde_json::json!({ "column": "year", "reverse": true }),
    );
    let s = screen(&mut t);
    let lamport = s.iter().position(|l| l.contains("lamport │")).unwrap();
    let knuth = s.iter().position(|l| l.contains("knuth84 │")).unwrap();
    assert!(lamport < knuth, "{s:#?}");
    assert_eq!(t.text(), text);
    t.app.run_command("bib.unsortView", serde_json::json!({}));
    // In an entry: its source; a field set by the smallest edit.
    t.at(text.find("Paxos").unwrap());
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.contains("title = {Paxos},")), "{s:#?}");
    t.app.run_command(
        "bib.setField",
        serde_json::json!({ "field": "doi", "value": "10.1/p" }),
    );
    assert!(
        t.text().contains("  year = {1998},\n  doi = {10.1/p}\n}"),
        "{}",
        t.text()
    );
}

#[test]
fn latex_table_spans() {
    let text = "\\begin{tabular}{lll}\n\\multicolumn{2}{c}{Head} & z \\\\ \\hline\nalpha & beta & gamma \\\\\n\\end{tabular}\n\nafter\n";
    let mut t = with_file(text, "t.tex", Config::default(), (50, 8));
    t.at(text.len());
    let rows: Vec<String> = (0..3)
        .map(|y| {
            t.row(y)
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ')
                .trim_end()
                .to_string()
        })
        .collect();
    // The span one cell over two columns, centered.
    assert_eq!(rows[1], "│     Head     │ z     │", "{rows:#?}");
    assert_eq!(rows[2], "│ alpha │ beta │ gamma │", "{rows:#?}");
    // The rule after the row's `\\\\`: the row underlined.
    let buf = t.draw();
    let full = t.row(1);
    let col = full[..full.find("Head").unwrap()].chars().count() as u16;
    assert!(buf[(col, 1)].modifier.contains(Modifier::UNDERLINED));
    assert!(!buf[(col, 2)].modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn latex_multirow_across_its_rows() {
    let text = "\\begin{tabular}{ll}\n\\multirow{3}*{Group} & a \\\\\n & b \\\\\n & c \\\\\n\\end{tabular}\n\nafter\n";
    let mut t = with_file(text, "t.tex", Config::default(), (50, 8));
    t.at(text.len());
    let rows: Vec<String> = (0..3)
        .map(|y| {
            t.row(y)
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ')
                .trim_end()
                .to_string()
        })
        .collect();
    assert_eq!(
        rows,
        ["│       │ a │", "│ Group │ b │", "│       │ c │"],
        "{rows:#?}"
    );
}

#[test]
fn file_manager_editable_names() {
    let (mut t, dir) = project_app(Config::default());
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert!(cursor_line(&t).ends_with(" a.org"), "{}", cursor_line(&t));
    // `e` makes the names text; typing edits them, Ctrl+S renames.
    t.typ("e");
    t.key(KeyCode::End, KeyModifiers::NONE);
    for _ in 0.."a.org".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("renamed.org");
    assert!(
        cursor_line(&t).ends_with(" renamed.org"),
        "{}",
        cursor_line(&t)
    );
    t.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(dir.join("proj/renamed.org").is_file() && !dir.join("proj/a.org").exists());
    assert!(
        status(&mut t).contains("Renamed 1 item"),
        "{}",
        status(&mut t)
    );
    // Letters are the file manager's keys again; Escape discards an edit.
    t.typ("e");
    t.typ("x");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(
        cursor_line(&t).ends_with(" renamed.org"),
        "{}",
        cursor_line(&t)
    );
    assert!(dir.join("proj/renamed.org").is_file());
    assert!(!t.app.doc.is_modified());
}

#[test]
fn file_manager_find_and_search() {
    let (mut t, dir) = project_app(Config::default());
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    // Find by name: every `.org` under the folder, by its path.
    t.app
        .run_command("dired.findName", serde_json::json!({ "pattern": "*.org" }));
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains(&format!("sub{}b.org", std::path::MAIN_SEPARATOR))
            && rows.contains("2 found"),
        "{rows}"
    );
    // `^` shows the folder again.
    t.typ("^");
    assert!(
        !screen(&mut t)
            .join("\n")
            .contains(&format!("sub{}b.org", std::path::MAIN_SEPARATOR))
    );
    // `A` searches the text of the folder's files; Enter opens the match.
    t.typ("A");
    t.typ("needle");
    settle(&mut t);
    let rows = screen(&mut t);
    assert!(
        rows.iter().any(|r| r.contains("the needle here")),
        "{rows:?}"
    );
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "b.org");
    let _ = dir;
}

#[test]
fn file_manager_stored_links() {
    let (mut t, _dir) = project_app(Config::default());
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    // Store a link to the file at the cursor, then insert it in a.org.
    let at = t.text().find(" sub/").unwrap() + 1;
    t.at(at);
    t.app.run_command("link.store", serde_json::Value::Null);
    assert!(status(&mut t).contains("sub"), "{}", status(&mut t));
    t.typ("q");
    assert_eq!(title(&t), "a.org");
    t.at(t.text().len());
    t.app
        .run_command("org.link.insertStored", serde_json::Value::Null);
    assert!(t.text().ends_with("[[file:sub][sub]]"), "{}", t.text());
}

#[cfg(unix)]
#[test]
fn file_manager_shell_command() {
    let (mut t, dir) = project_app(Config::default());
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert!(cursor_line(&t).ends_with(" a.org"));
    // `!` asks for the command, then whether to run it.
    t.typ("!");
    t.typ("cp ? copy.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("cp ? copy.org"), "{rows}");
    t.typ("y");
    settle(&mut t);
    assert_eq!(
        std::fs::read_to_string(dir.join("proj/copy.org")).unwrap(),
        "* A\nalpha\n"
    );
}

#[test]
fn latex_follow_reference() {
    let text = "\\section{Intro}\\label{intro}\ntext\nSee \\ref{intro}.\n";
    let mut t = with_file(text, "r.tex", Config::default(), (60, 8));
    t.at(text.find("\\ref").unwrap() + 2);
    t.app
        .run_command("latex.link.open", serde_json::Value::Null);
    assert_eq!(t.app.doc.selection.head, text.find("\\label").unwrap());
}

#[test]
fn empty_latex_document() {
    // An empty `.tex` file opens, draws and takes typing.
    let mut t = with_file("", "empty.tex", Config::default(), (40, 6));
    screen(&mut t);
    t.typ("\\section{A}");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    screen(&mut t);
    for _ in 0..20 {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    assert_eq!(t.text(), "");
    screen(&mut t);
}

#[test]
fn latex_diagnostics_in_the_terminal() {
    // `\bf` is deprecated: underlined once the diagnostics arrive, its
    // message in the status line at the cursor, Ctrl+. fixes it.
    let mut t = with_file("Some {\\bf x} here.\n", "d.tex", Config::default(), (50, 6));
    t.at(7);
    let mut found = false;
    for _ in 0..300 {
        t.app.tick(std::time::Instant::now());
        if t.app.doc.latex_diagnostics().is_some() {
            found = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(found, "diagnostics worked out");
    let buf = t.draw();
    let flagged = (0..buf.area.width).any(|x| {
        buf[(x, 0)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    });
    assert!(flagged, "the deprecated command is underlined");
    let rows = screen(&mut t);
    assert!(rows.iter().any(|r| r.contains("ⓘ")), "{rows:#?}");
    // Next Problem from the start of the text.
    t.at(0);
    t.key(KeyCode::F(8), KeyModifiers::ALT);
    assert_eq!(t.app.doc.selection.head, 6);
    // In the source view too.
    t.app
        .run_command("view.toggleSource", serde_json::json!({}));
    let buf = t.draw();
    let flagged = (0..buf.area.width).any(|x| {
        buf[(x, 0)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    });
    assert!(flagged, "flagged in the source view");
    t.app
        .run_command("view.toggleSource", serde_json::json!({}));
    t.at(7);
    // Ctrl+. as terminals can send it.
    t.key(KeyCode::Char('.'), KeyModifiers::ALT);
    let rows = screen(&mut t);
    assert_eq!(t.text(), "Some {\\bfseries x} here.\n", "{rows:#?}");
}

#[test]
fn csv_filter_and_header_in_the_terminal() {
    let mut rows = String::from("name,city\n");
    for i in 0..30 {
        let city = if i % 10 == 0 { "Izmir" } else { "Ankara" };
        rows.push_str(&format!("p{i},{city}\n"));
    }
    let mut t = with_file(&rows, "people.csv", Config::default(), (70, 9));
    // Scrolled down: the header stays under the column letters.
    t.at(rows.find("p20").unwrap());
    let screen_rows = screen(&mut t);
    assert!(screen_rows[1].contains("name"), "{screen_rows:#?}");
    assert!(screen_rows.iter().any(|r| r.contains("p20")));
    // A filter: the header and the matching rows (and the cursor's).
    t.at(0);
    t.app.doc.csv_filter = Some("izmir".into());
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("p0") && shown.contains("p10") && shown.contains("p20"));
    assert!(!shown.contains("p1 ") && !shown.contains("p5"), "{shown}");
    assert!(
        shown.contains("3 of 30"),
        "the count in the status line: {shown}"
    );
    t.app.doc.csv_filter = None;
    assert!(screen(&mut t).join("\n").contains("p5"));
}

#[test]
fn latex_project_numbers_across_files() {
    // A chapter file of a book: numbered after the main document's
    // chapter, and a reference to a label in the main document resolved.
    let dir = std::env::temp_dir().join(format!("kalem-tui-project-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.tex"),
        "\\documentclass{book}\n\\begin{document}\n\\chapter{One}\\label{one}\n\\include{two}\n\\end{document}\n",
    )
    .unwrap();
    let path = dir.join("two.tex");
    std::fs::write(&path, "\\chapter{Two}\nAfter chapter \\ref{one}.\n").unwrap();
    let app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let term = Terminal::new(TestBackend::new(50, 6)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir),
    };
    t.app.doc.wait_for_latex_project();
    t.app.doc.poll();
    // The cursor away from both lines.
    t.at(t.text().len());
    let rows = screen(&mut t).join("\n");
    // The number, an em space, the title.
    assert!(rows.contains("2\u{2003}Two"), "{rows:?}");
    assert!(rows.contains("After chapter 1."), "{rows}");
    // Completion offers the labels of the other files too.
    let labels = t.app.doc.latex().unwrap().model().labels.clone();
    assert!(labels.iter().any(|l| l.name == "one"));
}

#[test]
fn latex_outline_shows_included_files() {
    // The outline of the main document lists the chapter of the file it
    // includes, and choosing it opens that file at the heading (§9.5).
    let dir = std::env::temp_dir().join(format!("kalem-tui-outline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("main.tex");
    std::fs::write(
        &main,
        "\\documentclass{book}\n\\begin{document}\n\\chapter{One}\n\\include{two}\n\\end{document}\n",
    )
    .unwrap();
    std::fs::write(dir.join("two.tex"), "% a chapter\n\\chapter{Two}\nText.\n").unwrap();
    let app = App::with_keymap(
        Some(&main),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let term = Terminal::new(TestBackend::new(60, 8)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir.clone()),
    };
    t.app.doc.wait_for_latex_project();
    t.app.doc.poll();
    let items = kalem_core::latex_view::outline_items(&t.app.doc).unwrap();
    let titles: Vec<_> = items
        .iter()
        .map(|i| (i.title.as_str(), i.file.is_some()))
        .collect();
    assert_eq!(titles, [("1\u{2003}One", false), ("2\u{2003}Two", true)]);
    let two = items[1].clone();
    assert_eq!(
        kalem_core::view::position_in_file(two.file.as_deref().unwrap(), two.start),
        Some((2, 0))
    );
}

#[test]
fn insert_figure_asks_for_the_picture() {
    let mut t = with_file(
        "\\begin{document}\n\n\\end{document}\n",
        "f.tex",
        Config::default(),
        (60, 8),
    );
    t.at(17);
    t.app
        .run_command("latex.insert.figure", serde_json::json!({}));
    t.typ("figs/cat.png");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    // Then its width (0.8 of the line offered) and its caption.
    assert!(!t.text().contains("figure"));
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("A cat");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let text = t.text();
    assert!(
        text.contains("\\includegraphics[width=0.8\\linewidth]{figs/cat.png}"),
        "{text}"
    );
    assert!(text.contains("\\caption{A cat}"), "{text}");
    assert!(text.contains("\\label{fig:cat}"), "{text}");
    // A width as a share of the line.
    t.app.run_command(
        "latex.insert.figure",
        serde_json::json!({ "path": "dog.jpg", "width": "0.5" }),
    );
    assert!(
        t.text()
            .contains("\\includegraphics[width=0.5\\linewidth]{dog.jpg}")
    );
}

#[test]
fn latex_inserts_follow_the_document_style() {
    // Tabs for indentation, the label inside the caption, `f-` and `e-`
    // as prefixes, an equation's label on its first line (T2.7h.15).
    let doc = "\\begin{document}\n\\begin{figure}\n\t\\centering\n\t\\caption{Old.\\label{f-old}}\n\\end{figure}\n\\begin{equation}\\label{e-old}\n\tx\n\\end{equation}\n\n\\end{document}\n";
    let mut t = with_file(doc, "s.tex", Config::default(), (60, 12));
    let at = doc.find("\n\n\\end{document}").unwrap() + 1;
    t.at(at);
    t.app.run_command(
        "latex.insert.figure",
        serde_json::json!({ "path": "cat.png", "caption": "A cat" }),
    );
    let text = t.text();
    assert!(
        text.contains("\\begin{figure}[htbp]\n\t\\centering\n\t\\includegraphics[width=0.8\\linewidth]{cat.png}\n\t\\caption{A cat\\label{f-cat}}\n\\end{figure}\n"),
        "{text}"
    );
    t.at(at);
    t.app
        .run_command("latex.insert.equation", serde_json::json!({}));
    let text = t.text();
    assert!(
        // (The formula after the label: an empty line in a formula
        // would not compile.)
        text.contains("\\begin{equation}\\label{e-} \n\\end{equation}\n"),
        "{text}"
    );
}

#[test]
fn inserted_latex_table_is_a_grid() {
    // Insert Table writes a tabular the view shows as the grid: no `&`
    // or `\\` on screen away from the cursor.
    let mut t = with_file(
        "\\begin{document}\n\n\\end{document}\n",
        "t.tex",
        Config::default(),
        (60, 12),
    );
    t.at(17);
    t.app.run_command(
        "latex.insert.table",
        serde_json::json!({ "columns": 2, "rows": 2 }),
    );
    assert!(t.text().contains("\\begin{tabular}"));
    t.at(0);
    let rows = screen(&mut t).join("\n");
    assert!(!rows.contains(" & "), "{rows}");
}

#[test]
fn latex_table_of_contents_in_the_terminal() {
    let text = "\\documentclass{article}\n\\begin{document}\n\\tableofcontents\n\\section{One}\n\\subsection{Sub}\n\\section{Two}\n\\end{document}\n";
    let mut t = with_file(text, "c.tex", Config::default(), (50, 14));
    t.at(text.len());
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("1 One") && rows.contains("1.1 Sub") && rows.contains("2 Two"),
        "{rows}"
    );
    assert!(!rows.contains("\\tableofcontents"), "{rows}");
}

#[test]
fn latex_preamble_folds_away_from_the_cursor() {
    let text = "\\documentclass{article}\n\\usepackage{amsmath}\n\\usepackage{graphicx}\n\\begin{document}\nHello.\n\\end{document}\n";
    let mut t = with_file(text, "p.tex", Config::default(), (50, 10));
    t.at(text.find("Hello").unwrap());
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("documentclass"), "{rows}");
    assert!(!rows.contains("graphicx"), "{rows}");
    assert!(rows.contains("Hello"), "{rows}");
    // In the preamble: all of it.
    t.at(text.find("amsmath").unwrap());
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("graphicx"), "{rows}");
}

#[test]
fn long_unknown_environment_folds() {
    let mut text = String::from("Before.\n\\begin{tikzpicture}\n");
    for i in 0..10 {
        text.push_str(&format!("  \\draw (0,{i}) -- (1,{i});\n"));
    }
    text.push_str("\\end{tikzpicture}\nAfter.\n");
    let mut t = with_file(&text, "k.tex", Config::default(), (50, 10));
    t.at(0);
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("tikzpicture") && rows.contains("After."),
        "{rows}"
    );
    assert!(!rows.contains("(0,3)"), "{rows}");
    t.at(text.find("(0,3)").unwrap());
    assert!(screen(&mut t).join("\n").contains("(0,3)"));
}

#[test]
fn latex_problems_list() {
    let text = "One {\\bf x}.\nTwo \\ref{nope}.\n";
    let mut t = with_file(text, "p.tex", Config::default(), (60, 10));
    t.app.doc.update_latex_diagnostics();
    t.at(0);
    t.app.run_command("latex.problems", serde_json::json!({}));
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("1: ⓘ") && rows.contains("2: ⚠"), "{rows}");
    // The second item: to the unknown label.
    t.key(KeyCode::Down, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, text.find("\\ref").unwrap());
}

#[test]
fn latex_table_with_spans_is_a_grid() {
    let text = "\\begin{tabular}{lll}\n\\multicolumn{2}{c}{Head} & c \\\\\n\\multirow{2}*{A} & b & c \\\\\n\\end{tabular}\nAfter.\n";
    let mut t = with_file(text, "s.tex", Config::default(), (60, 10));
    t.at(text.find("After").unwrap());
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("Head") && !rows.contains("multicolumn") && !rows.contains("multirow"),
        "{rows}"
    );
}

#[test]
fn latex_footnotes_listed_at_the_end() {
    let text = "\\documentclass{article}\n\\begin{document}\nText.\\footnote{A first note.} More.\\footnote{And a\n  second.}\n\\end{document}\n";
    let mut t = with_file(text, "n.tex", Config::default(), (60, 14));
    t.at(text.find("Text").unwrap());
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("Notes"), "{rows}");
    assert!(
        rows.contains("1 A first note.") && rows.contains("2 And a second."),
        "{rows}"
    );
}

#[test]
fn latex_formula_preview_at_the_cursor() {
    let text = "A formula $x^2 + y$ here.\n";
    let mut t = with_file(text, "m.tex", Config::default(), (50, 6));
    t.at(text.find("x^2").unwrap() + 1);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("= x² + y"), "{rows}");
}

#[test]
fn latex_display_math_over_lines_in_the_terminal() {
    let text = "Before.\n\\begin{equation}\n  x^2\n  + y\n\\end{equation}\nAfter.\n";
    let mut t = with_file(text, "e.tex", Config::default(), (50, 10));
    t.at(0);
    let rows = screen(&mut t).join("\n");
    assert!(!rows.contains("\\begin{equation}"), "{rows}");
    assert!(rows.contains("x² + y") && rows.contains("(1)"), "{rows}");
    assert!(rows.contains("After."), "{rows}");
}

#[test]
fn latex_root_found_again_after_a_tex_root_line() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-magic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A main document without `\documentclass`: found only by the line.
    std::fs::write(
        dir.join("main.tex"),
        "\\section{One}\\label{one}\n\\input{part}\n",
    )
    .unwrap();
    let path = dir.join("part.tex");
    std::fs::write(&path, "See \\ref{one}.\n").unwrap();
    let app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let term = Terminal::new(TestBackend::new(50, 6)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir),
    };
    t.app.doc.wait_for_latex_project();
    t.app.doc.poll();
    t.at(t.text().len());
    assert!(screen(&mut t).join("\n").contains("See ??."));
    // The line added: the root found, the reference resolved.
    t.at(0);
    t.typ("% !TEX root = main.tex");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.app.doc.wait_for_latex_project();
    t.app.doc.poll();
    t.at(t.text().len());
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("See 1."), "{rows}");
}

#[test]
fn csv_view_sorted_by_a_column() {
    let text = "name,age\nAda,36\nBob,7\nCem,20\n";
    let mut t = with_file(text, "s.csv", Config::default(), (50, 8));
    t.at(text.find("36").unwrap());
    t.app.run_command("csv.sortView", serde_json::json!({}));
    let rows = screen(&mut t);
    let pos = |s: &str| rows.iter().position(|r| r.contains(s)).unwrap();
    assert!(
        pos("name") < pos("Bob") && pos("Bob") < pos("Cem") && pos("Cem") < pos("Ada"),
        "{rows:#?}"
    );
    // The file keeps its order.
    assert_eq!(t.text(), text);
    // Down goes in the order shown: from Bob's row to Cem's.
    t.at(text.find("Bob").unwrap());
    t.key(KeyCode::Down, KeyModifiers::NONE);
    let line = t.app.doc.text().line_of(t.app.doc.selection.head);
    assert_eq!(line, 3, "Cem's line");
    // Again: descending; then the file's order.
    t.at(text.find("36").unwrap());
    t.app.run_command("csv.sortView", serde_json::json!({}));
    let rows = screen(&mut t);
    let pos = |s: &str| rows.iter().position(|r| r.contains(s)).unwrap();
    assert!(
        pos("Ada") < pos("Cem") && pos("Cem") < pos("Bob"),
        "{rows:#?}"
    );
    t.app.run_command("csv.unsortView", serde_json::json!({}));
    let rows = screen(&mut t);
    let pos = |s: &str| rows.iter().position(|r| r.contains(s)).unwrap();
    assert!(pos("Ada") < pos("Bob") && pos("Bob") < pos("Cem"));
}

#[test]
fn latex_sections_fold() {
    let text =
        "\\section{One}\nFirst text.\n\\subsection{Sub}\nSub text.\n\\section{Two}\nSecond text.\n";
    let mut t = with_file(text, "f.tex", Config::default(), (50, 10));
    t.at(2);
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    t.at(text.find("Second").unwrap());
    let rows = screen(&mut t).join("\n");
    assert!(
        !rows.contains("First text.") && !rows.contains("Sub text."),
        "{rows}"
    );
    assert!(
        rows.contains("Two") && rows.contains("Second text."),
        "{rows}"
    );
    // The text is unchanged.
    assert_eq!(t.text(), text);
}

#[test]
fn enter_in_csv_keeps_no_indentation() {
    // Leading tabs are empty fields and leading blanks are part of a
    // value: Enter on the last record adds an empty one (the cell below,
    // as in a spreadsheet), with none of them copied.
    let text = "\ta\tb\n";
    let mut t = with_file(text, "d.tsv", Config::default(), (60, 8));
    t.at(text.find('b').unwrap() + 1);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "\ta\tb\n\t\t\n");
    let text = "  x,y\n";
    let mut t = with_file(text, "d.csv", Config::default(), (60, 8));
    t.at(text.find('y').unwrap() + 1);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "  x,y\n,\n");
}

#[test]
fn csv_backspace_and_delete_keep_the_delimiters() {
    // Editing a cell (F2) they delete within its value: at its start
    // Backspace, at its end Delete, merged two cells. In Ready mode, as in
    // Excel, Backspace clears the cell and types into it.
    let text = "a,b\n1,22\n";
    let mut t = with_file(text, "d.csv", Config::default(), (60, 8));
    let edit = |t: &mut T, at: usize| {
        t.at(at);
        t.app
            .run_command("csv.editCell", serde_json::json!({ "here": true }));
    };
    edit(&mut t, text.find("22").unwrap());
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(t.text(), text);
    edit(&mut t, text.find("1,").unwrap() + 1);
    t.key(KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(t.text(), text);
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(t.text(), "a,b\n,22\n");
    t.at(t.text().find("22").unwrap());
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    t.typ("7");
    assert_eq!(t.text(), "a,b\n,7\n");
}

#[test]
fn csv_cells_are_clicked_anywhere_in_them() {
    // A click anywhere between a cell's bars selects it: on the padding
    // after the bar it selected the cell before, and a short record's
    // missing cells went to its last field (typing there broke the field).
    let text = "name,age,city,note\nAda,36,İzmir,first\nÇağla,7\nBob,5,\"Bursa\",x\n";
    let mut t = with_file(text, "d.csv", Config::default(), (110, 10));
    t.draw();
    let mut bad = Vec::new();
    for (row, name) in [(0, "name"), (1, "Ada"), (2, "Çağla"), (3, "Bob")] {
        let y = (0..10).find(|&y| t.row(y).contains(name)).expect("the row");
        let bars: Vec<u16> = t
            .row(y)
            .chars()
            .enumerate()
            .filter(|(_, c)| *c == '│')
            .map(|(i, _)| i as u16)
            .collect();
        for j in 0..4 {
            let (l, r) = (bars[j], bars[j + 1]);
            for x in [l + 1, (l + r) / 2, r - 1] {
                mouse(
                    &mut t,
                    MouseEventKind::Down(MouseButton::Left),
                    x,
                    y,
                    KeyModifiers::NONE,
                );
                let got = kalem_core::csv::cell_at(&t.app.doc).map(|(_, r, _, c)| (r, c));
                if got != Some((row, j)) {
                    bad.push((row, j, x, got));
                }
            }
        }
    }
    assert_eq!(bad, vec![]);
    assert_eq!(t.text(), text);
    // Çağla's missing D cell, typed in.
    let y = (0..10).find(|&y| t.row(y).contains("Çağla")).unwrap();
    let bars: Vec<u16> = t
        .row(y)
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '│')
        .map(|(i, _)| i as u16)
        .collect();
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        (bars[3] + bars[4]) / 2,
        y,
        KeyModifiers::NONE,
    );
    t.typ("Q");
    assert!(t.text().contains("Çağla,7,,Q\n"), "{}", t.text());
    // Editing past Bursa's closing quote: typed inside it.
    t.at(t.text().find("\"Bursa\"").unwrap() + 7);
    t.app
        .run_command("csv.editCell", serde_json::json!({ "here": true }));
    t.typ("!");
    assert!(t.text().contains("\"Bursa!\""), "{}", t.text());
}

#[test]
fn csv_paste_goes_into_cells() {
    // Tab-separated lines are written over the cells from the cursor's
    // (they were inserted as text, splitting the row).
    let text = "a,b\n1,Ankara\n2,x\n";
    let mut t = with_file(text, "d.csv", Config::default(), (60, 8));
    t.at(text.find("Ankara").unwrap());
    t.app.event(Event::Paste("P1\tP2\nP3\tP4".into()));
    assert_eq!(t.text(), "a,b\n1,P1,P2\n2,P3,P4\n");
}

#[test]
fn csv_rows_do_not_wrap() {
    // A row wider than the terminal scrolls sideways rather than wrap:
    // wrapped, its bars and the letters no longer lined up, and Down went
    // through its pieces.
    let text = "name,note\nAda,a note much longer than this narrow terminal is wide\nBob,x\n";
    let mut t = with_file(text, "d.csv", Config::default(), (40, 8));
    t.at(0);
    let rows: Vec<String> = (0..8).map(|y| t.row(y)).collect();
    let ada = rows
        .iter()
        .position(|r| r.contains("Ada"))
        .expect("Ada's row");
    assert!(rows[ada + 1].contains("Bob"), "{rows:#?}");
    t.key(KeyCode::Down, KeyModifiers::NONE);
    t.key(KeyCode::Down, KeyModifiers::NONE);
    let row = kalem_core::csv::cell_at(&t.app.doc).map(|(_, r, _, _)| r);
    assert_eq!(row, Some(2));
}

#[test]
fn csv_row_numbers_stay_when_scrolled_sideways() {
    // As a spreadsheet's: scrolled to the far columns, the rows still
    // start with their numbers (they scrolled away).
    let mut text = String::from("a");
    for j in 0..12 {
        text.push_str(&format!(",column{j}"));
    }
    text.push_str("\nx");
    for j in 0..12 {
        text.push_str(&format!(",value{j:02}"));
    }
    text.push('\n');
    let mut t = with_file(&text, "wide.csv", Config::default(), (40, 8));
    t.at(text.find("value11").unwrap());
    let rows: Vec<String> = (0..8).map(|y| t.row(y)).collect();
    let row = rows
        .iter()
        .find(|r| r.contains("value11"))
        .expect("the row");
    assert!(row.trim_start().starts_with('2'), "{rows:#?}");
    assert!(!row.contains("value00"), "scrolled: {rows:#?}");
}

#[test]
fn csv_letters_and_row_numbers_select() {
    // A click on a column's letter selects the column, one on a row's
    // number the row, as in a spreadsheet (and in the graphical editor).
    let text = "a,b,c\n1,2,3\n4,5,6\n";
    let mut t = with_file(text, "heads.csv", Config::default(), (60, 8));
    t.at(0);
    let letters = (0..8)
        .find(|&y| {
            let r = t.row(y);
            r.contains(" A ") && r.contains(" B ") && r.contains(" C ")
        })
        .expect("the letters");
    let x = t.row(letters).chars().position(|c| c == 'B').unwrap() as u16;
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        x,
        letters,
        KeyModifiers::NONE,
    );
    assert_eq!(
        kalem_core::csv::cell_rectangle(&t.app.doc),
        Some(kalem_core::csv::Rectangle {
            rows: vec![0, 1, 2],
            cols: (1, 1)
        })
    );
    // Row 2's number.
    let y = (0..8)
        .find(|&y| t.row(y).contains('2') && t.row(y).contains('3'))
        .unwrap();
    let x = t.row(y).chars().position(|c| c == '2').unwrap() as u16;
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        x,
        y,
        KeyModifiers::NONE,
    );
    assert_eq!(
        kalem_core::csv::cell_rectangle(&t.app.doc),
        Some(kalem_core::csv::Rectangle {
            rows: vec![1],
            cols: (0, 2)
        })
    );
}

#[test]
fn csv_malformed_field_in_the_status_bar() {
    let text = "name,note\napple,6\" long\n";
    let mut t = with_file(text, "d.csv", Config::default(), (100, 8));
    t.at(text.find("long").unwrap());
    let s = status(&mut t);
    assert!(s.contains("quote inside an unquoted value"), "{s}");
}

#[test]
fn menus_from_the_keyboard() {
    // F10 lists the menus' items; choosing one runs its command.
    let mut t = open("* A\nText.\n");
    t.key(KeyCode::F(10), KeyModifiers::NONE);
    t.typ("edit select all");
    let s = screen(&mut t).join("\n");
    assert!(s.contains("Edit › Select All"), "{s}");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    let sel = t.app.doc.selection;
    assert_eq!(
        (sel.anchor.min(sel.head), sel.anchor.max(sel.head)),
        (0, 10)
    );
}

/// A right click in the file manager opens its menu for the entry under
/// the mouse; Control-click marks (T2.7e.17).
#[test]
fn file_manager_right_click() {
    let (mut t, _dir) = project_app(Config::default());
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(title(&t), "proj/");
    let rows = screen(&mut t);
    let row = rows
        .iter()
        .position(|r| r.split_once('│').is_some_and(|(_, l)| l.contains(" a.org")))
        .unwrap_or_else(|| panic!("{rows:?}"));
    // Columns, not bytes: the side bar's border is one cell.
    let col = rows[row].chars().position(|c| c == '│').unwrap()
        + rows[row]
            .split_once('│')
            .unwrap()
            .1
            .chars()
            .take_while(|c| *c != 'a')
            .count()
        + 1;
    let col = col as u16;
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Right),
        col,
        row as u16,
        KeyModifiers::NONE,
    );
    // The menu's order, not sorted: Open first, then Open with System
    // Application.
    let shown = screen(&mut t).join("\n");
    let open = shown
        .find("File: Open\n")
        .or(shown.find("File: Open "))
        .expect(&shown);
    let system = shown.find("Open with System Application").expect(&shown);
    assert!(open < system, "{shown}");
    for c in "relative".chars() {
        t.key(KeyCode::Char(c), KeyModifiers::NONE);
    }
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("Copy Relative Paths"), "{shown}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    let marks = |t: &T| t.app.doc.dired.as_deref().unwrap().marks.len();
    mouse(
        &mut t,
        MouseEventKind::Down(MouseButton::Left),
        col,
        row as u16,
        KeyModifiers::CONTROL,
    );
    assert_eq!(marks(&t), 1);
}

/// The arrows, Home, End and Delete edit a prompt's text: Shift+R in the
/// file manager, the cursor moved into the name (reported by the owner,
/// 2026-09-30).
#[test]
fn editing_a_prompt_with_the_arrows() {
    let (mut t, dir) = project_app(Config::default());
    t.key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    let at = t.text().find(" a.org").unwrap() + 1;
    t.at(at);
    t.key(KeyCode::Char('R'), KeyModifiers::SHIFT);
    for _ in 0..4 {
        t.key(KeyCode::Left, KeyModifiers::NONE);
    }
    t.typ("x");
    t.key(KeyCode::Right, KeyModifiers::NONE);
    t.key(KeyCode::Delete, KeyModifiers::NONE);
    t.key(KeyCode::Home, KeyModifiers::NONE);
    t.key(KeyCode::End, KeyModifiers::NONE);
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    settle(&mut t);
    assert!(dir.join("proj/ax.r").exists(), "{}", status(&mut t));
    // The palette's text too: Ctrl+Left goes a word back.
    t.app.run_command("view.palette", serde_json::Value::Null);
    t.typ("save as");
    t.key(KeyCode::Left, KeyModifiers::CONTROL);
    t.key(KeyCode::Backspace, KeyModifiers::NONE);
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("> saveas"), "{shown}");
}

/// Doom's file manager keys (T2.7e.18): `SPC .` from a listing opens a
/// new file in its folder; `R` moves the marked entries; `y y` copies
/// the path; `h` goes up and `l` opens.
#[test]
fn doom_keys_in_the_file_manager() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, dir) = project_app(config);
    t.typ("-");
    assert_eq!(title(&t), "proj/");
    t.typ(" .");
    t.typ("new.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        t.app.doc.meta.path.as_deref(),
        Some(dir.join("proj/new.org").as_path())
    );
    t.typ("-");
    assert_eq!(title(&t), "proj/");
    // `SPC p D` works from the listing, which is in the project.
    t.typ(" pD");
    assert_eq!(title(&t), "proj/");
    // Mark a.org, `R`, the folder `sub`: moved there.
    let at = t.text().find(" a.org").unwrap() + 1;
    t.at(at);
    t.typ("m");
    t.typ("R");
    // The prompt holds the entry's path: its name replaced by `sub`.
    for _ in 0.."a.org".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("sub");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    settle(&mut t);
    assert!(dir.join("proj/sub/a.org").exists(), "{}", status(&mut t));
    // `l` opens sub, `h` comes back, `y y` copies the path.
    let at = t.text().find(" sub").unwrap() + 1;
    t.at(at);
    t.typ("l");
    assert_eq!(title(&t), "sub/");
    t.typ("h");
    assert_eq!(title(&t), "proj/");
    t.app.take_output();
    t.typ("yy");
    let out = t.app.take_output();
    assert!(out.iter().any(|o| o.starts_with("\x1b]52;c;")), "{out:?}");
}

/// Doom's `SPC b` keys on the open documents (T2.7i.2): the last one,
/// bury, save all, close the others and close all.
#[test]
fn documents_keys() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, dir) = project_app(config);
    t.app.open_path(&dir.join("proj/sub/b.org"), None);
    t.app.open_path(&dir.join("loose.org"), None);
    assert_eq!(title(&t), "loose.org");
    // `SPC b l` and `` SPC ` ``: the one before, back and forth.
    t.typ(" bl");
    assert_eq!(title(&t), "b.org");
    t.typ(" `");
    assert_eq!(title(&t), "loose.org");
    // `SPC b z`: to the end of the list, the next one shown.
    let before = t.app.open_files().len();
    t.typ(" bz");
    assert_ne!(title(&t), "loose.org");
    assert_eq!(t.app.open_files().len(), before);
    // `SPC b S` saves every modified document with a file.
    t.typ("ggOnew");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    let shown = title(&t);
    t.typ(" bS");
    let path = t.app.doc.meta.path.clone().unwrap();
    assert!(
        std::fs::read_to_string(&path).unwrap().starts_with("new"),
        "{shown}"
    );
    // `SPC b O` keeps this one; `SPC b K` leaves an empty document.
    t.typ(" bO");
    assert_eq!(t.app.open_files().len(), 1);
    assert_eq!(title(&t), shown);
    t.typ(" bK");
    assert_eq!(t.app.open_files().len(), 1);
    assert_eq!(t.app.doc.meta.path, None);
}

/// Doom's `SPC f` keys on this file (T2.7i.3): rename and move with the
/// document following, copy, copy the path, delete (the document closes).
#[test]
fn this_file_keys() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, dir) = project_app(config);
    assert_eq!(title(&t), "a.org");
    // `SPC f R`: the prompt holds the path; its name replaced.
    t.typ(" fR");
    for _ in 0.."a.org".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("renamed.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    settle(&mut t);
    assert!(dir.join("proj/renamed.org").exists());
    assert_eq!(
        t.app.doc.meta.path.as_deref(),
        Some(dir.join("proj/renamed.org").as_path())
    );
    // `SPC f C`: a copy, this document stays.
    t.typ(" fC");
    for _ in 0.."renamed.org".len() {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("copy.org");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    settle(&mut t);
    assert!(dir.join("proj/copy.org").exists());
    assert_eq!(title(&t), "renamed.org");
    // `SPC f y`: the path on the clipboard.
    t.app.take_output();
    t.typ(" fy");
    assert!(!t.app.take_output().is_empty());
    // `SPC f D` asks, moves it to the trash (the tests' trash folder) and
    // closes the document.
    t.app.open_path(&dir.join("proj/copy.org"), None);
    t.typ(" fD");
    t.typ("y");
    settle(&mut t);
    assert!(!dir.join("proj/copy.org").exists());
    assert!(t.app.open_files().iter().all(|f| f.title != "copy.org"));
}

/// Doom's `SPC s b`: the live list of matching lines; the cursor follows
/// the chosen line, Escape goes back, Enter stays; `SPC s B` in every
/// open document, `SPC s i` the headings (T2.7i.4).
#[test]
fn live_line_search() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, dir) = project_app(config);
    // a.org is "* A\nalpha\n".
    t.at(0);
    t.typ(" sb");
    t.typ("alp");
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("alpha"), "{shown}");
    assert_eq!(t.app.doc.selection.head, 4);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 0);
    t.typ(" sb");
    t.typ("alp");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.doc.selection.head, 4);
    // Across the open documents: b.org's "the needle here".
    t.app.open_path(&dir.join("proj/sub/b.org"), None);
    t.app.open_path(&dir.join("proj/a.org"), None);
    t.typ(" sB");
    t.typ("needle");
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("b.org:3"), "{shown}");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "b.org");
    assert_eq!(t.app.doc.text().line_of(t.app.doc.selection.head), 2);
    // The headings.
    t.typ(" si");
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("* B"), "{shown}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
}

/// Doom's `SPC t r`: the document refuses edits, the cursor still moves
/// (T2.7i.6).
#[test]
fn read_only_toggle() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, _dir) = project_app(config);
    let before = t.text();
    t.typ(" tr");
    t.typ("ixyz");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.typ("j");
    assert_eq!(t.text(), before);
    assert_eq!(t.app.doc.text().line_of(t.app.doc.selection.head), 1);
    t.typ(" tr");
    t.typ("ixyz");
    // The first Escape may close a completion list.
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_ne!(t.text(), before);
    // Full screen is the terminal's.
    t.typ(" tF");
    assert!(status(&mut t).contains("terminal"), "{}", status(&mut t));
}

/// Doom's `SPC o f`: the terminal editor says it has one window
/// (T2.7i.7).
#[test]
fn one_window_in_the_terminal() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, _dir) = project_app(config);
    t.typ(" of");
    assert!(
        status(&mut t).contains("The terminal editor has"),
        "{}",
        status(&mut t)
    );
}

/// Doom's `SPC p` keys (T2.7i.8): another project in the file manager,
/// the project's TODOs, a shell command at its folder.
#[test]
fn project_keys() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, dir) = project_app(config);
    t.typ(" p>");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(title(&t), "proj/");
    t.typ("q");
    t.typ(" pt");
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("TODO"), "{shown}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.typ(" p!");
    t.typ("touch made.txt");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    t.typ("y");
    for _ in 0..200 {
        settle(&mut t);
        if dir.join("proj/made.txt").exists() {
            break;
        }
    }
    assert!(dir.join("proj/made.txt").exists(), "{}", status(&mut t));
}

/// Doom's `SPC h` keys (T2.7i.9): describe a key, this document and the
/// character; every binding listed.
#[test]
fn help_keys() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, _dir) = project_app(config);
    t.typ(" hk");
    t.key(KeyCode::Char(' '), KeyModifiers::CONTROL);
    assert!(
        status(&mut t).contains("ctrl+space runs Compl"),
        "{}",
        status(&mut t)
    );
    // Control is Vim's: the Word-like Ctrl+S is off.
    t.typ(" hk");
    t.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(
        status(&mut t).contains("ctrl+s runs no command"),
        "{}",
        status(&mut t)
    );
    t.typ(" hm");
    assert!(status(&mut t).contains("org"), "{}", status(&mut t));
    t.at(0);
    t.typ(" h'");
    assert!(status(&mut t).contains("U+002A"), "{}", status(&mut t));
    t.typ(" hbb");
    t.typ("save as");
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("Save As"), "{shown}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
}

/// Doom's `SPC c f` formats the document as `kalem fmt` does (T2.7i.10);
/// typing `a` and `P` on the way is text, not the projects view.
#[test]
fn format_key() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, _dir) = project_app(config);
    t.typ("jo| a |b|");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.typ(" cf");
    assert!(t.text().contains("| a | b |"), "{}", t.text());
}

#[test]
fn sessions_save_and_restore() {
    // T2.7i.14: the open documents, their cursors and the one shown.
    let (mut t, dir) = project_app(Config::default());
    let b = dir.join("proj/sub/b.org");
    t.app.run_command(
        "file.open",
        serde_json::json!({ "path": b.display().to_string() }),
    );
    let at = t.text().find("needle").unwrap();
    t.at(at);
    let file = dir.join("work.json").display().to_string();
    t.app
        .run_command("session.saveAs", serde_json::json!({ "name": file }));
    assert!(
        status(&mut t).contains("Session saved"),
        "{}",
        status(&mut t)
    );
    // A new editor on another file restores it.
    let mut caps = Caps::full();
    caps.kitty_keyboard = false;
    let app = App::with_keymap(
        Some(&dir.join("loose.org")),
        Config::default(),
        caps,
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut u = T {
        app,
        term: Terminal::new(TestBackend::new(80, 12)).unwrap(),
        dir: None,
    };
    u.app
        .run_command("session.restoreNamed", serde_json::json!({ "name": file }));
    assert_eq!(title(&u), "b.org");
    assert_eq!(u.app.doc.selection.head, at);
    let titles: Vec<String> = u.app.open_files().into_iter().map(|f| f.title).collect();
    assert_eq!(titles, ["loose.org", "a.org", "b.org"]);
}

#[test]
fn quitting_without_saving_and_restarting() {
    // T2.7i.14: `SPC q Q` asks, then quits with the changes lost; a
    // restart waits for the changes to be saved.
    let mut t = open("* A\n");
    t.typ("x");
    t.app.run_command("app.restart", serde_json::Value::Null);
    assert!(!t.app.quit && !t.app.restart);
    assert!(
        status(&mut t).contains("Save or close"),
        "{}",
        status(&mut t)
    );
    t.app
        .run_command("app.quitWithoutSaving", serde_json::Value::Null);
    assert!(!t.app.quit);
    t.key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(!t.app.quit);
    t.app
        .run_command("app.quitWithoutSaving", serde_json::Value::Null);
    t.key(KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(t.app.quit);
    assert_eq!(
        std::fs::read_to_string(t.app.doc.meta.path.clone().unwrap()).unwrap(),
        "* A\n"
    );
    // Nothing unsaved: Restart ends this editor to start another.
    let mut u = open("* B\n");
    u.app.run_command("app.restart", serde_json::Value::Null);
    assert!(u.app.quit && u.app.restart);
}

#[test]
fn markdown_front_matter_folded() {
    let text = "---\ntitle: Notes\ntags: [a]\n---\n# Heading\n\ntext\n";
    let mut t = with_file(text, "t.md", Config::default(), (50, 8));
    t.at(text.len() - 2);
    let rows = screen(&mut t).join("\n");
    assert!(
        !rows.contains("title:") && rows.contains("Heading"),
        "{rows}"
    );
    // The cursor in it: it opens.
    t.at(6);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("title: Notes"), "{rows}");
}

#[test]
fn inserting_names_and_copies() {
    // Doom's `SPC i` (T2.7i.13).
    let mut t = open("x\n");
    t.app
        .run_command("insert.fileName", serde_json::Value::Null);
    assert_eq!(t.text(), "t.orgx\n");
    kalem_core::command::record_history("copied text");
    t.app
        .run_command("insert.fromHistory", serde_json::Value::Null);
    t.typ("copied");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.text().starts_with("t.orgcopied text"), "{}", t.text());
    t.app.run_command("insert.unicode", serde_json::Value::Null);
    t.typ("U+2192");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.text().contains('→'), "{}", t.text());
}

#[test]
fn panes_in_the_terminal() {
    // T2.7i.5: two documents side by side; focus moves between them.
    let (mut t, dir) = project_app(Config::default());
    t.app
        .run_command("pane.splitRight", serde_json::Value::Null);
    let b = dir.join("proj/sub/b.org");
    t.app.run_command(
        "file.open",
        serde_json::json!({ "path": b.display().to_string() }),
    );
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("alpha") && rows.contains("beta"), "{rows}");
    assert_eq!(
        rows.lines().next().unwrap().matches('│').count(),
        2,
        "{rows}"
    );
    assert_eq!(title(&t), "b.org");
    t.app
        .run_command("pane.focus", serde_json::json!({ "dir": "left" }));
    assert_eq!(title(&t), "a.org");
    t.app.run_command("pane.next", serde_json::Value::Null);
    assert_eq!(title(&t), "b.org");
    // Only this pane, and back.
    t.app.run_command("pane.only", serde_json::Value::Null);
    let rows = screen(&mut t).join("\n");
    assert!(!rows.contains("alpha"), "{rows}");
    t.app.run_command("pane.only", serde_json::Value::Null);
    assert!(screen(&mut t).join("\n").contains("alpha"));
    // Closing the pane leaves a.org alone; the last pane stays.
    t.app.run_command("pane.close", serde_json::Value::Null);
    assert_eq!(title(&t), "a.org");
    let rows = screen(&mut t).join("\n");
    assert!(!rows.contains("beta"), "{rows}");
    t.app.run_command("pane.close", serde_json::Value::Null);
    assert!(status(&mut t).contains("only pane"), "{}", status(&mut t));
    // Undo brings the split back.
    t.app.run_command("pane.undo", serde_json::Value::Null);
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("beta"), "{rows}");
}

#[test]
fn workspaces_in_the_terminal() {
    // T2.7i.15: each workspace its own documents; deleting one keeps them.
    let (mut t, dir) = project_app(Config::default());
    t.app
        .run_command("workspace.newNamed", serde_json::json!({ "name": "notes" }));
    assert_eq!(title(&t), "Untitled");
    assert!(status(&mut t).contains("[notes]"), "{}", status(&mut t));
    let b = dir.join("proj/sub/b.org");
    t.app.run_command(
        "file.open",
        serde_json::json!({ "path": b.display().to_string() }),
    );
    let titles = |t: &T| -> Vec<String> {
        t.app
            .open_files()
            .into_iter()
            .filter(|f| !f.hidden)
            .map(|f| f.title)
            .collect()
    };
    // (The empty document gave its place to b.org.)
    assert_eq!(titles(&t), ["b.org"]);
    // Back to main: a.org alone.
    t.app.run_command("workspace.last", serde_json::Value::Null);
    assert_eq!(title(&t), "a.org");
    assert_eq!(titles(&t), ["a.org"]);
    t.app
        .run_command("workspace.switch", serde_json::json!({ "index": 1 }));
    assert_eq!(title(&t), "b.org");
    // Deleting it: its documents join main.
    t.app
        .run_command("workspace.delete", serde_json::Value::Null);
    assert_eq!(titles(&t).len(), 2);
    assert!(!status(&mut t).contains("[notes]"));
}

#[test]
fn markdown_code_coloured_with_the_lines_before() {
    // T2.7c.3: the second line of a string that spans lines is a string.
    let text = "```python\nx = \"\"\"first\nsecond line\n\"\"\"\n```\n\nafter\n";
    let mut t = with_file(text, "t.md", Config::default(), (50, 8));
    t.at(text.len());
    let buf = t.draw();
    let row = |y: u16| -> String { (0..50).map(|x| buf[(x, y)].symbol().to_string()).collect() };
    // The cell of `word`'s first character.
    let cell = |word: &str| {
        (0..8u16)
            .find_map(|y| {
                row(y)
                    .find(word)
                    .map(|x| (row(y)[..x].chars().count() as u16, y))
            })
            .unwrap_or_else(|| panic!("{word}: {:?}", (0..8).map(row).collect::<Vec<_>>()))
    };
    let first = buf[cell("first")].fg;
    let second = buf[cell("second")].fg;
    assert_eq!(first, second);
    assert_ne!(buf[cell("x =")].fg, first);
}

#[test]
fn markdown_properties_as_a_form() {
    // T2.7c.9: choose a field, its value offered to change.
    let text = "---\ntitle: Old\ntags: [a]\n---\n# Body\n";
    let mut t = with_file(text, "t.md", Config::default(), (60, 10));
    t.app
        .run_command("markdown.frontMatter.edit", serde_json::Value::Null);
    t.typ("title");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    // The value is offered: replaced by typing after clearing it.
    for _ in 0..3 {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ("New title");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.text(), "---\ntitle: New title\ntags: [a]\n---\n# Body\n");
}

#[test]
fn markdown_pictures_copied_into_images() {
    // A picture from elsewhere goes into `images/` beside the document.
    let mut t = with_file("Text\n", "notes.md", Config::default(), (60, 8));
    let dir = t
        .app
        .doc
        .meta
        .path
        .clone()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let elsewhere = std::env::temp_dir().join(format!("kalem-pic-{}.png", std::process::id()));
    std::fs::write(
        &elsewhere,
        kalem_core::images::svg_png(kalem_core::images::LOGO_SVG, 16).unwrap(),
    )
    .unwrap();
    t.at(5);
    t.app.run_command(
        "markdown.insert.image",
        serde_json::json!({ "path": elsewhere.display().to_string() }),
    );
    let name = elsewhere
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let stem = elsewhere
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(t.text(), format!("Text\n![{stem}](images/{name})"));
    assert!(dir.join("images").join(&name).exists());
    // Pasted paths too, as a terminal pastes a dropped file.
    t.app.paste(&elsewhere.display().to_string(), false);
    assert!(
        t.text().contains(&format!("images/{stem}-2.png")),
        "{}",
        t.text()
    );
    let _ = std::fs::remove_file(&elsewhere);
}

#[test]
fn closing_the_last_document_keeps_kalem_open() {
    let mut t = open("* A\n");
    t.app.run_command("file.close", serde_json::Value::Null);
    assert!(!t.app.quit);
    assert_eq!(title(&t), "Untitled");
    assert_eq!(t.app.open_files().len(), 1);
    // The empty one stays.
    t.app.run_command("file.close", serde_json::Value::Null);
    assert!(!t.app.quit);
    assert_eq!(t.app.open_files().len(), 1);
}

#[test]
fn vim_quit_closes_the_pane_first() {
    // `SPC w n` then `:q`: the pane closes, Kalem stays; then the
    // document, as a tab closes, while others are open (the owner,
    // 2026-10-06: `:q` closing a file quit Kalem); `:q` from the last
    // document quits, as in Vim.
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let mut t = with_config("* A\n", config, (60, 10));
    t.typ(" wn");
    assert_eq!(title(&t), "Untitled");
    t.typ(":q");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(!t.app.quit);
    assert_eq!(title(&t), "t.org");
    t.typ(":q");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(!t.app.quit);
    assert_eq!(title(&t), "Untitled");
    assert_eq!(t.app.open_files().len(), 1);
    t.typ(":q");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.app.quit);
}

#[test]
fn vim_force_quit_loses_one_document() {
    // `:q!` loses this document's changes, not the others': it closes
    // it while others are open.
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let mut t = with_config("* A\n", config, (60, 10));
    t.app.run_command("file.new", serde_json::Value::Null);
    t.typ("ix");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.app.run_command("file.next", serde_json::Value::Null);
    assert_eq!(title(&t), "t.org");
    t.typ("iy");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    t.typ(":q!");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(!t.app.quit);
    assert_eq!(t.app.open_files().len(), 1);
    assert_eq!(t.text(), "x");
}

#[test]
fn vim_add_project_asks_for_the_folder() {
    // `SPC p a` asks for the folder, the document's own offered.
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let mut t = with_config("* A\n", config, (60, 10));
    let path = t.app.doc.meta.path.clone().unwrap();
    let dir = path
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    std::fs::create_dir_all(path.parent().unwrap().join("chapters")).unwrap();
    t.typ(" pa");
    let shown = screen(&mut t);
    let last = &shown[shown.len() - 1];
    assert!(
        last.contains("Add Project") && last.contains(&dir),
        "{last}"
    );
    // The folder's folders over the prompt; Tab completes one.
    assert!(shown[shown.len() - 2].contains("chapters/"), "{shown:?}");
    t.typ("ch");
    t.key(KeyCode::Tab, KeyModifiers::NONE);
    let shown = screen(&mut t);
    assert!(shown[shown.len() - 1].contains("chapters/"), "{shown:?}");
}

#[test]
fn csv_looks_like_a_spreadsheet() {
    let text = "name,n\nAda,36\nBob,7\n";
    let mut t = with_file(text, "t.csv", Config::default(), (40, 8));
    t.at(text.find("36").unwrap());
    let rows = screen(&mut t);
    // The letters bar, then the rows numbered.
    assert!(
        rows[0].contains(" A ") && rows[0].contains(" B"),
        "{rows:#?}"
    );
    // Columns at least eight wide, between the grid's lines.
    assert!(rows[2].contains(" 2 │ Ada      │       36 │"), "{rows:#?}");
    // The letters bar's lines line up with the grid's.
    let bars = |r: &str| -> Vec<usize> {
        r.chars()
            .enumerate()
            .filter(|(_, c)| *c == '│')
            .map(|(i, _)| i)
            .collect()
    };
    assert_eq!(bars(&rows[0]), bars(&rows[2]), "{rows:#?}");
}

#[test]
fn projects_view_menu_removes_and_adds() {
    // Right click (the Menu key) on a project: Remove from Projects takes
    // it off the list and leaves its folder.
    let (mut t, dir) = project_app(Config::default());
    t.app.run_command("dired.projects", serde_json::Value::Null);
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("proj"), "{shown}");
    let row = t.app.doc.text().as_str().find("proj").unwrap();
    t.at(row);
    t.app
        .run_command("dired.contextMenu", serde_json::Value::Null);
    t.typ("Remove from Projects");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        t.app.projects.list.list.is_empty(),
        "{:?}",
        t.app.projects.list
    );
    assert!(dir.join("proj/a.org").exists());
    // Add Project Folder… asks for the folder.
    t.app
        .run_command("dired.contextMenu", serde_json::json!({ "listing": true }));
    t.typ("Add Project");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    for _ in 0..200 {
        t.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    t.typ(&dir.join("proj").display().to_string());
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(t.app.projects.list.list.len(), 1);
    assert!(screen(&mut t).join("\n").contains("proj"));
}

/// A completer like a language server's: program symbols, slow.
struct Symbols;

impl kalem_core::completers::Completer for Symbols {
    fn id(&self) -> &'static str {
        "symbols"
    }
    fn applies(&self, _: &kalem_core::completers::Context) -> bool {
        true
    }
    fn trigger(&self) -> kalem_core::completers::Trigger {
        kalem_core::completers::Trigger::WordOrAfter(1, &["."])
    }
    fn slow(&self) -> bool {
        true
    }
    fn complete(
        &self,
        ctx: &kalem_core::completers::Context,
        _: Option<&kalem_core::DocumentState>,
        _: &kalem_core::completers::Cancel,
    ) -> Vec<kalem_core::completers::Item> {
        let (start, _) = ctx.word_prefix();
        let mut i = kalem_core::completers::Item::new(
            "map(enumerable, fun)",
            "map()",
            start..ctx.point,
            kalem_core::completers::Kind::Symbol,
        );
        i.cursor = 4;
        i.documentation =
            Some("Returns a list where each element is the result of invoking `fun`.".into());
        vec![i]
    }
}

/// Enter and Tab take a program symbol from the menu, in both profiles;
/// in Vim's insert mode too, and one Escape there closes the menu and
/// leaves insert mode.
#[test]
fn code_completions_taken() {
    let wait = |t: &mut T| {
        for _ in 0..400 {
            t.app.tick(std::time::Instant::now());
            if t.app.completion_ready() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("no completion items");
    };
    for (vim, key) in [
        (false, KeyCode::Enter),
        (false, KeyCode::Tab),
        (true, KeyCode::Enter),
        (true, KeyCode::Tab),
    ] {
        let profile = if vim {
            "editor.keymap_profile = \"vim\"\n"
        } else {
            ""
        };
        let config = Config::from_layers(&[(Layer::User, None, profile)]);
        let mut t = with_file("x = 1\n", "t.ex", config, (60, 10));
        t.app.register_completer(std::sync::Arc::new(Symbols));
        t.at(6);
        if vim {
            t.typ("i");
        }
        t.typ("Enum.");
        wait(&mut t);
        // The chosen item's documentation shows beside the list.
        let screen: String = {
            let buf = t.draw();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(screen.contains("Returns a list"), "{screen}");
        t.key(key, KeyModifiers::NONE);
        assert_eq!(t.text(), "x = 1\nEnum.map()", "vim {vim}, {key:?}");
        assert_eq!(
            t.app.doc.selection.head,
            "x = 1\nEnum.map(".len(),
            "vim {vim}, {key:?}"
        );
    }
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let mut t = with_file("x = 1\n", "t.ex", config, (60, 10));
    t.app.register_completer(std::sync::Arc::new(Symbols));
    t.at(6);
    t.typ("iEnum.");
    wait(&mut t);
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!t.app.completion_ready());
    assert_eq!(
        t.app.vim.as_ref().map(|v| v.mode),
        Some(kalem_core::vim::Mode::Normal)
    );
}

#[test]
fn csv_frozen_and_hidden_columns_in_the_terminal() {
    // A wide file: the first column stays at the left edge when the rows
    // scroll sideways; a hidden column takes no room; the file is untouched.
    let mut text = String::from("name");
    for j in 0..12 {
        text.push_str(&format!(",col{j}"));
    }
    text.push_str("\nAda");
    for j in 0..12 {
        text.push_str(&format!(",value{j:02}"));
    }
    text.push('\n');
    let mut t = with_file(&text, "w.csv", Config::default(), (50, 8));
    t.app
        .run_command("csv.toggleFrozen", serde_json::Value::Null);
    t.at(text.find("value11").unwrap());
    let rows = screen(&mut t);
    let ada = rows
        .iter()
        .find(|r| r.contains("value11"))
        .expect("the row shows");
    assert!(ada.contains("Ada"), "{rows:#?}");
    assert!(!ada.contains("value00"), "{rows:#?}");
    // Not frozen, and not wrapping: the first column scrolls away.
    t.app
        .run_command("csv.toggleFrozen", serde_json::Value::Null);
    t.app
        .run_command("view.toggleWrap", serde_json::Value::Null);
    t.at(text.find("value11").unwrap());
    let rows = screen(&mut t);
    let ada = rows
        .iter()
        .find(|r| r.contains("value11"))
        .expect("the row shows");
    assert!(!ada.contains("Ada"), "{rows:#?}");
    // Hidden columns.
    t.at(text.find("value00").unwrap());
    t.app.run_command("csv.hideColumn", serde_json::Value::Null);
    let rows = screen(&mut t);
    let ada = rows
        .iter()
        .find(|r| r.contains("Ada"))
        .expect("the row shows");
    assert!(
        !ada.contains("value00") && ada.contains("value01"),
        "{rows:#?}"
    );
    assert_eq!(t.app.doc.text().as_str(), text);
}

#[test]
fn file_manager_opens_in_a_new_pane() {
    // T2.7e.17: Open in New Pane puts the file beside the listing.
    let (mut t, dir) = project_app(Config::default());
    let sub = dir.join("proj/sub");
    t.app.run_command(
        "file.open",
        serde_json::json!({ "path": sub.display().to_string() }),
    );
    let at = t.app.doc.text().as_str().find("b.org").expect("listed");
    t.at(at);
    let listing = title(&t);
    t.app
        .run_command("dired.openInPane", serde_json::Value::Null);
    assert_eq!(title(&t), "b.org");
    let rows = screen(&mut t).join("\n");
    assert!(rows.contains("beta") && rows.contains("b.org"), "{rows}");
    assert_eq!(
        rows.lines().next().unwrap().matches('│').count(),
        2,
        "{rows}"
    );
    t.app
        .run_command("pane.focus", serde_json::json!({ "dir": "left" }));
    assert_eq!(title(&t), listing);
}

#[test]
fn csv_rectangle_of_cells_in_the_terminal() {
    // T2.7d.9: a selection across rows of a grid is a rectangle of cells,
    // painted as one and copied as TSV.
    let text = "a,b,c\n11,22,33\n44,55,66\n";
    let mut t = with_file(text, "r.csv", Config::default(), (40, 8));
    t.at(text.find("22").unwrap());
    t.app.doc.move_cursor(text.find("55").unwrap() + 1, true);
    let buf = t.draw();
    let find = |s: &str| {
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        rows.iter()
            .enumerate()
            .find_map(|(y, r)| r.find(s).map(|i| (r[..i].chars().count() as u16, y as u16)))
            .unwrap_or_else(|| panic!("{s} drawn: {rows:#?}"))
    };
    let reversed = |(x, y): (u16, u16)| buf[(x, y)].modifier.contains(Modifier::REVERSED);
    assert!(
        reversed(find("22")) && reversed(find("55")),
        "the column's cells"
    );
    for s in ["11", "33", "44", "66"] {
        assert!(!reversed(find(s)), "{s} is outside the rectangle");
    }
    t.app.take_output();
    t.app.run_command("edit.copy", serde_json::Value::Null);
    // "22\n55\n", through OSC 52.
    assert_eq!(t.app.take_output(), ["\x1b]52;c;MjIKNTUK\x07"]);
    assert_eq!(t.text(), text);
}

#[test]
fn file_menu_separators_in_the_terminal() {
    // T2.7e.17: the file menu's groups are parted by rules, as in the
    // graphical editor's menu, while nothing is typed.
    let (mut t, dir) = project_app(Config::default());
    t.app.run_command(
        "file.open",
        serde_json::json!({ "path": dir.join("proj").display().to_string() }),
    );
    let at = t.app.doc.text().as_str().find("a.org").expect("listed");
    t.at(at);
    t.app
        .run_command("dired.contextMenu", serde_json::Value::Null);
    let rows = screen(&mut t);
    let rules = rows.iter().filter(|r| r.contains("────────")).count();
    assert!(rules >= 2, "{rows:#?}");
    let open = rows.iter().position(|r| r.contains("Open")).expect("Open");
    let rule = rows.iter().position(|r| r.contains("────────")).unwrap();
    assert!(open < rule, "{rows:#?}");
    // Typed: the matches only.
    t.typ("cop");
    let rows = screen(&mut t);
    assert!(!rows.iter().any(|r| r.contains("────────")), "{rows:#?}");
}
