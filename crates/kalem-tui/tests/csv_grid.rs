//! A CSV grid in the terminal editor, with Word's keys and Vim's: the
//! cell the arrows and Vim's `j` and `k` reach, what typing changes, the
//! caret and the pinned header (found by trying the grid as a user
//! would, 2026-10-07).

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::settings::{Config, Layer};
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;
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

/// A terminal editor on `text` saved as `name` in a folder of its own,
/// with a configuration folder of its own (the user's stays untouched).
fn open(name: &str, text: &str, config: Config, size: (u16, u16)) -> T {
    let dir = std::env::temp_dir().join(format!(
        "kalem-csv-grid-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/settings.toml"),
        "[ui]\nlanguage = \"en\"\n",
    )
    .unwrap();
    kalem_core::kalem_fs::set_trash_dir(Some(dir.join("trash")));
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let mut app = App::with_keymap(Some(&path), config, Caps::full(), &[], Vec::new()).unwrap();
    app.config_dir = Some(dir.join("config"));
    let term = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    let mut t = T { app, term, dir };
    t.draw();
    t
}

fn vim() -> Config {
    Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")])
}

impl T {
    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| app.draw(f)).unwrap();
    }

    fn key(&mut self, code: KeyCode, m: KeyModifiers) {
        self.app.event(Event::Key(KeyEvent::new(code, m)));
        self.draw();
    }

    fn k(&mut self, code: KeyCode) {
        self.key(code, KeyModifiers::NONE);
    }

    fn typ(&mut self, s: &str) {
        for c in s.chars() {
            self.k(KeyCode::Char(c));
        }
    }

    fn go(&mut self, cell: &str) {
        self.app.run_command("csv.goToCell", json!({ "cell": cell }));
        self.draw();
    }

    fn cell(&self) -> String {
        let (_, r, _, c) = kalem_core::csv::cell_at(&self.app.doc).unwrap();
        kalem_core::csv_tools::coordinates(r, c).0
    }

    fn text(&self) -> String {
        self.app.doc.text().as_str().to_string()
    }

    fn row(&self, y: u16) -> String {
        let buf = self.term.backend().buffer();
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

const PEOPLE: &str = "name,age,city\nAlexander,36,London\nAl,41,Wilmslow\n\"Grace, H\",85,\"New\nYork\"\n";

#[test]
fn vim_insert_mode_inserts_into_the_cell() {
    // Excel's Ready mode replaced the cell at the first character typed in
    // insert mode: `A` on a record of two lines emptied its value.
    let mut t = open("v.csv", PEOPLE, vim(), (70, 12));
    t.go("B3");
    t.typ("ix");
    t.k(KeyCode::Esc);
    assert!(t.text().contains("\nAl,x41,Wilmslow\n"), "{}", t.text());
    t.typ("A,z");
    t.k(KeyCode::Esc);
    assert!(
        t.text().contains("\nAl,x41,\"Wilmslow,z\"\n"),
        "{}",
        t.text()
    );
    // No Excel mode in the status bar beside Vim's.
    let status = kalem_core::csv::status(&t.app.doc).unwrap_or_default();
    assert!(!status.contains("Ready") && !status.contains("Enter"), "{status}");
}

#[test]
fn vim_j_and_k_keep_the_column() {
    // They went by the text's lines: from B1 to A2, and onto a filter's
    // hidden rows.
    let mut t = open("v.csv", PEOPLE, vim(), (70, 12));
    t.go("B1");
    t.typ("j");
    assert_eq!(t.cell(), "B2");
    t.typ("jj");
    assert_eq!(t.cell(), "B4");
    t.typ("j");
    assert_eq!(t.cell(), "B4", "no row below");
    t.typ("k");
    assert_eq!(t.cell(), "B3");
    let mut t = open(
        "f.csv",
        "id,tag\n1,x\n2,y\n3,x\n",
        vim(),
        (60, 10),
    );
    t.app.run_command("csv.filter", json!({ "text": "x" }));
    t.go("A2");
    t.typ("j");
    assert_eq!(t.cell(), "A4");
}

#[test]
fn up_and_down_go_by_the_rows_shown() {
    let text = "name,age,city\nAda,36,London\nAlan,41,Wilmslow\nGrace,85,NYC\n";
    let mut t = open("w.csv", text, Config::default(), (70, 12));
    // On the last row Down stayed on the text's last line, the row's last
    // cell, where typing replaced NYC.
    t.go("A4");
    t.k(KeyCode::Down);
    assert_eq!(t.cell(), "A4");
    t.go("B1");
    t.k(KeyCode::Up);
    assert_eq!(t.cell(), "B1");
    t.go("A2");
    t.k(KeyCode::Right);
    t.k(KeyCode::Right);
    t.k(KeyCode::Down);
    assert_eq!(t.cell(), "C3");
    // A record of two lines is one row.
    let mut t = open(
        "m.csv",
        "id,note,n\n1,\"two\nlines\",5\n2,plain,6\n",
        Config::default(),
        (60, 10),
    );
    t.go("B2");
    t.k(KeyCode::Down);
    assert_eq!(t.cell(), "B3");
    t.k(KeyCode::Up);
    assert_eq!(t.cell(), "B2");
}

#[test]
fn a_line_break_goes_into_the_cell() {
    // Ctrl+J (Alt+Enter in the graphical editor) split the record.
    let text = "name,age,city\nAda,36,London\nAlan,41,Wilmslow\n";
    let mut t = open("w.csv", text, Config::default(), (70, 12));
    t.go("C2");
    t.key(KeyCode::Char('j'), KeyModifiers::CONTROL);
    t.typ("Town");
    t.k(KeyCode::Enter);
    assert_eq!(
        t.text(),
        "name,age,city\nAda,36,\"London\nTown\"\nAlan,41,Wilmslow\n"
    );
    assert_eq!(t.cell(), "C3");
}

#[test]
fn the_caret_at_a_values_end_is_after_it() {
    // F2 on `Ada` drew the caret at the cell's right edge.
    let text = "name,city\nAda,London\n";
    let mut t = open("c.csv", text, Config::default(), (60, 8));
    t.go("A2");
    t.k(KeyCode::F(2));
    let (x, y) = t.term.get_cursor_position().map(|p| (p.x, p.y)).unwrap();
    let row = t.row(y);
    let ada = row[..row.find("Ada").unwrap()].chars().count() as u16;
    assert_eq!(x, ada + 3, "{row}");
}

#[test]
fn the_header_pinned_after_a_sep_line() {
    // The `sep=` line was pinned in the header's place.
    let mut text = String::from("sep=;\nname;n\n");
    for i in 0..30 {
        text.push_str(&format!("r{i};{i}\n"));
    }
    let mut t = open("s.csv", &text, Config::default(), (40, 10));
    t.go("A20");
    let top: Vec<String> = (0..3).map(|y| t.row(y)).collect();
    assert!(top.iter().any(|r| r.contains("name")), "{top:?}");
    assert!(!top.iter().any(|r| r.contains("sep=")), "{top:?}");
}

#[test]
fn a_tab_in_a_value_keeps_the_bars_in_line() {
    // Drawn to the next tab stop, it pushed the row's bars out of line.
    let text = "id,note,n\n1,\"a\tb\",5\n22,xyz,6\n";
    let mut t = open("t.csv", text, Config::default(), (60, 8));
    t.go("A1");
    let bars = |row: &str| -> Vec<usize> {
        row.chars()
            .enumerate()
            .filter(|(_, c)| *c == '│')
            .map(|(i, _)| i)
            .collect()
    };
    let (r2, r3) = (t.row(2), t.row(3));
    assert!(r2.contains("a b"), "{r2}");
    assert_eq!(bars(&r2), bars(&r3), "{r2}\n{r3}");
}

#[test]
fn a_header_of_two_lines_is_pinned_whole() {
    let mut text = String::from("id,\"first\nsecond\"\n");
    for i in 0..30 {
        text.push_str(&format!("{i},v{i}\n"));
    }
    let mut t = open("h.csv", &text, Config::default(), (40, 12));
    t.go("A25");
    let top: Vec<String> = (0..4).map(|y| t.row(y)).collect();
    assert!(top.iter().any(|r| r.contains("first")), "{top:?}");
    assert!(top.iter().any(|r| r.contains("second")), "{top:?}");
}
