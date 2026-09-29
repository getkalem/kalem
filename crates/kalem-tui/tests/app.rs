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

fn with_config(text: &str, config: Config, size: (u16, u16)) -> T {
    with_file(text, "t.org", config, size)
}

/// A terminal editor on `text` saved as `name` in a new folder.
fn with_file(text: &str, name: &str, config: Config, size: (u16, u16)) -> T {
    let dir = std::env::temp_dir().join(format!(
        "kalem-tui-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let app = App::with_keymap(Some(&path), config, Caps::full(), &[], Vec::new()).unwrap();
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
    // Chords Vim leaves alone go to the Word-like keys: Ctrl+S saves.
    t.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
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
    // Commands that do not apply here are not offered.
    t.at(0);
    t.key(KeyCode::F(1), KeyModifiers::NONE);
    t.typ("insert row");
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
    let config = Config::from_layers(&[(Layer::User, None, "editor.center_text = true\n")]);
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
}

#[test]
fn formulas_as_images() {
    let text = "Top\n\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n$$x^2$$\nafter\n";
    let mut t = open(text);
    t.at(0);
    // Without a graphics protocol: the source, fragments approximated.
    assert_eq!(t.row(1), " \\begin{align}");
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
        last.starts_with("Open file: ") && last.ends_with("proj/"),
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
            .any(|r| r.contains("sub/b.org:3") && r.contains("the needle here")),
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
fn doom_keys_in_the_terminal() {
    let config = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let (mut t, dir) = project_app(config);
    t.app.run_command(
        "file.open",
        serde_json::json!({ "path": dir.join("loose.org").display().to_string() }),
    );
    assert_eq!(title(&t), "loose.org");
    // Space shows what follows above the status line; SPC b p goes back.
    t.typ(" ");
    let rows = screen(&mut t).join("\n");
    assert!(
        rows.contains("p → +Project") && rows.contains("f → +File"),
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
}

#[test]
fn strict_org_offers_a_kalem_document() {
    let mut t = open("one two\n");
    t.at(0);
    t.app
        .run_command("format.alignRight", serde_json::Value::Null);
    // Nothing written; the choice is offered.
    assert_eq!(t.text(), "one two\n");
    let shown = screen(&mut t).join("\n");
    assert!(shown.contains("Make Kalem Document"), "{shown}");
    assert!(shown.contains("Allow Kalem's Formatting"), "{shown}");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    // Allowing it in this file: a `#+KALEM:` line, then formatting works.
    t.app
        .run_command("format.allowMarkup", serde_json::Value::Null);
    assert_eq!(t.text(), "#+KALEM: markup=yes\none two\n");
    t.at(t.text().find("one").unwrap());
    t.app
        .run_command("format.alignRight", serde_json::Value::Null);
    assert!(
        t.text().contains("#+ATTR_KALEM: :align right\none two"),
        "{}",
        t.text()
    );
    // Making a Kalem document renames the file.
    let mut t = open("one\n");
    let old = t.app.doc.meta.path.clone().unwrap();
    t.app
        .run_command("file.makeKalemDocument", serde_json::Value::Null);
    let new = t.app.doc.meta.path.clone().unwrap();
    assert_eq!(new.extension().unwrap(), "klm");
    assert!(new.exists() && !old.exists());
    assert!(status(&mut t).contains("Kalem"), "{}", status(&mut t));
}

#[test]
fn word_formatting_in_the_terminal() {
    let text = "one @@kalem:color=#c00000 bg=#fff2a8@@two@@kalem:end@@ three\n\n#+ATTR_KALEM: :align right\nend\n";
    let mut t = with_file(text, "t.klm", Config::default(), (60, 10));
    t.at(0);
    let buf = t.draw();
    let row: String = (0..buf.area.width)
        .map(|x| buf[(x, 0)].symbol().to_string())
        .collect();
    // The snippets do not show; the span has its colors.
    assert!(row.starts_with(" one two three"), "{row:?}");
    let two = row.find("two").unwrap() as u16;
    assert_eq!(buf[(two, 0)].fg, Color::Rgb(0xc0, 0, 0));
    assert_eq!(buf[(two, 0)].bg, Color::Rgb(0xff, 0xf2, 0xa8));
    // The attribute line hides; its paragraph is at the right.
    let row2: String = (0..buf.area.width)
        .map(|x| buf[(x, 2)].symbol().to_string())
        .collect();
    assert!(
        row2.trim_end().ends_with("end") && row2.starts_with("   "),
        "{row2:?}"
    );
    // Ctrl+] grows the word at the cursor (Alt+= in terminals without the
    // kitty keyboard protocol).
    t.at(4);
    t.key(KeyCode::Char(']'), KeyModifiers::CONTROL);
    assert!(
        t.text()
            .starts_with("one @@kalem:size=18 color=#c00000 bg=#fff2a8@@two"),
        "{}",
        t.text()
    );
}

#[test]
fn text_starts_at_the_left() {
    // In a wide terminal the 80-column text starts at the left edge,
    // unless `editor.center_text` asks for the middle.
    let mut t = with_config("* Heading\ntext\n", Config::default(), (160, 10));
    t.at(0);
    let buf = t.draw();
    let row: String = (0..160).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert!(row.starts_with(" * Heading"), "{row:?}");
    let config = Config::from_layers(&[(Layer::User, None, "editor.center_text = true\n")]);
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
    // SPC o P: the projects.
    t.typ(" oP");
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
fn a_file_under_version_control_opens_its_project() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-auto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
    std::fs::create_dir_all(dir.join("repo/src")).unwrap();
    std::fs::write(dir.join("repo/src/a.org"), "* A\n").unwrap();
    std::fs::write(dir.join("repo/b.org"), "* B\n").unwrap();
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
    let term = Terminal::new(TestBackend::new(80, 12)).unwrap();
    let mut t = T {
        app,
        term,
        dir: Some(dir.clone()),
    };
    // Opening another file of the repository makes it a project.
    t.app.open_path(&dir.join("repo/b.org"), None);
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
        status(&mut t).contains("@knuth84: Donald E. Knuth (1984). The \\TeXbook."),
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
fn csv_grid() {
    let mut t = with_file(
        "name,age\nAda,36\nBob,7\n",
        "p.csv",
        Config::default(),
        (70, 8),
    );
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.contains("Ada  │ 36")), "{s:#?}");
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
    for want in ["Theorem 1 (Main).", "Proof.", "\u{220e}", "\\emph{raw}"] {
        assert!(s.iter().any(|l| l.contains(want)), "{want}: {s:#?}");
    }
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
    // `\begin{…}` gets its `\end{…}`.
    let end = t.app.doc.text().len();
    t.at(end);
    t.typ("\\begin{center}");
    assert!(
        t.app
            .doc
            .text()
            .as_str()
            .ends_with("\\begin{center}\n  \n\\end{center}"),
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
            .ends_with("\\begin{centXer}\n  \n\\end{centXer}")
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
fn latex_formulas_as_images() {
    let text = "Before\n\\begin{equation}\n  E = mc^2\n\\end{equation}\nafter\n";
    let mut t = with_file(text, "i.tex", Config::default(), (60, 12));
    t.at(0);
    // Without graphics: the source over its lines.
    let s = screen(&mut t);
    assert!(s.iter().any(|l| l.contains("E = mc^2")), "{s:#?}");
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
