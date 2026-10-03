//! An Excel workbook in the terminal editor through the xlsx plugin
//! (T3.7.4): the sheet as a grid, the cursor's keys, a cell edited, a row
//! inserted, undo, the next sheet, and the file saved as itself.

use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;

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

impl T {
    fn open(name: &str) -> T {
        kalem_core::viewer::register(Arc::new(kalem_plugin_xlsx::XlsxViewer));
        let dir =
            std::env::temp_dir().join(format!("kalem-tui-xlsx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/budget.xlsx");
        std::fs::copy(data, dir.join("budget.xlsx")).unwrap();
        let app = App::with_keymap(
            Some(&dir.join("budget.xlsx")),
            Config::default(),
            Caps::full(),
            &[],
            Vec::new(),
        )
        .unwrap();
        let mut t = T {
            app,
            term: Terminal::new(TestBackend::new(80, 12)).unwrap(),
            dir,
        };
        t.screen();
        t
    }

    fn screen(&mut self) -> String {
        let app = &mut self.app;
        self.term.draw(|f| app.draw(f)).unwrap();
        let buf = self.term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(&mut self, code: KeyCode) {
        self.app
            .event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        self.screen();
    }

    fn status(&mut self) -> String {
        self.app
            .doc
            .viewer
            .as_deref_mut()
            .map(|v| v.status())
            .unwrap_or_default()
    }
}

#[test]
fn a_workbook_opens_as_a_grid() {
    let mut t = T::open("grid");
    assert_eq!(t.app.doc.meta.mode, DocumentMode::Viewer);
    let s = t.screen();
    assert!(
        s.contains("Item") && s.contains("1,200.00") && s.contains("4,293.75"),
        "{s}"
    );
    assert!(s.contains(" A ") && s.contains(" D "), "{s}");
    assert!(
        t.status().starts_with("Budget · A1 · 1/3"),
        "{}",
        t.status()
    );
    // The note on A2 shows in the status line.
    t.key(KeyCode::Down);
    assert!(
        t.status().contains("A2") && t.status().contains("Paid on the first"),
        "{}",
        t.status()
    );
    t.key(KeyCode::Right);
    assert!(t.status().contains("B2"), "{}", t.status());
    // The formula bar shows the cell in full: a long name, a formula.
    t.key(KeyCode::Right);
    t.key(KeyCode::Right);
    let s = t.screen();
    assert!(s.contains("D2    │ =B2+C2"), "{s}");
    // Column A (18 wide) fitted to its widest text, "Travel".
    t.key(KeyCode::Home);
    t.app
        .run_command("viewer.grid.autofitColumn", serde_json::json!({}));
    let w = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_layout()
        .unwrap()
        .widths[0];
    assert_eq!(w, 7.0);
    assert!(t.app.doc.viewer.as_deref().unwrap().modified());
    t.app.run_command("edit.undo", serde_json::json!({}));
    let w = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_layout()
        .unwrap()
        .widths[0];
    assert_eq!(w, 18.0);
    // `c +` and `c -`: a digit wider, narrower.
    t.key(KeyCode::Char('c'));
    t.key(KeyCode::Char('+'));
    assert_eq!(t.app.doc.viewer.as_deref_mut().unwrap().col_width(0), 19.0);
    t.key(KeyCode::Char('c'));
    t.key(KeyCode::Char('-'));
    t.key(KeyCode::Char('c'));
    t.key(KeyCode::Char('-'));
    assert_eq!(t.app.doc.viewer.as_deref_mut().unwrap().col_width(0), 17.0);
    // `r +` and `r -`: three points taller, shorter.
    let h = t.app.doc.viewer.as_deref_mut().unwrap().row_height(1);
    t.key(KeyCode::Char('r'));
    t.key(KeyCode::Char('+'));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().row_height(1),
        h + 3.0
    );
    t.key(KeyCode::Char('r'));
    t.key(KeyCode::Char('-'));
    assert_eq!(t.app.doc.viewer.as_deref_mut().unwrap().row_height(1), h);
}

#[test]
fn cells_rows_undo_and_save() {
    let mut t = T::open("edit");
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 1, "col": 1, "value": "1300" }),
    );
    let s = t.screen();
    assert!(
        s.contains("1,300.00") && s.contains("2,500.00") && s.contains("4,393.75"),
        "{s}"
    );
    // Enter moved the cursor down.
    assert!(t.status().contains("B3"), "{}", t.status());
    t.app.run_command("viewer.grid.insertRow", json!({}));
    let s = t.screen();
    assert!(s.contains("Food"), "{s}");
    assert_eq!(t.app.doc.viewer.as_deref_mut().unwrap().cell_input(), "");
    let input = |t: &mut T, row: u32, col: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, col);
        v.cell_input()
    };
    assert_eq!(input(&mut t, 3, 0), "Food");
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 2, 0), "Food");
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 1, 1), "1200");
    assert!(!t.app.doc.viewer.as_deref().unwrap().modified());
    t.app.run_command("edit.redo", json!({}));
    assert_eq!(input(&mut t, 1, 1), "1300");
    t.app.run_command("app.save", json!({}));
    let saved = std::fs::read(t.dir.join("budget.xlsx")).unwrap();
    let mut wb = kalem_plugin_xlsx::Workbook::open(saved).unwrap();
    assert_eq!(
        wb.display(0, kalem_plugin_xlsx::CellRef::new(1, 1))
            .unwrap(),
        "1,300.00"
    );
    // B2:C3 selected with Shift and the arrows, merged and centered after
    // the question (its other cells hold values), stepped over, unmerged.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
    }
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::SHIFT,
    )));
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::SHIFT,
    )));
    let s = t.screen();
    assert!(s.contains("B2:C3"), "{s}");
    t.key(KeyCode::Char('m'));
    assert!(
        t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .merged
            .iter()
            .all(|m| *m != [1, 1, 2, 2])
    );
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.mergeCenter", json!({ "confirmed": true }));
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    assert!(v.grid_layout().unwrap().merged.contains(&[1, 1, 2, 2]));
    assert_eq!((v.grid_pos().row, v.grid_pos().col), (1, 1));
    v.grid_move_by(0, 1);
    assert_eq!(v.grid_pos().col, 3, "the merged cell is stepped over");
    v.grid_move_to(2, 2);
    assert_eq!(
        (v.grid_pos().row, v.grid_pos().col),
        (1, 1),
        "inside it is its first cell"
    );
    t.key(KeyCode::Char('M'));
    assert!(
        !t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .merged
            .contains(&[1, 1, 2, 2])
    );
    t.app.run_command("edit.undo", json!({}));
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().cell_input(),
        "1300"
    );

    // B3:C4 selected and deleted: one step, undone as one.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 1);
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::SHIFT,
    )));
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::SHIFT,
    )));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().selection_tsv(),
        "431.50\t512.25\n0.00\t950.00"
    );
    t.key(KeyCode::Delete);
    assert_eq!(input(&mut t, 2, 1), "");
    assert_eq!(input(&mut t, 3, 2), "");
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 2, 1), "431.5");
    assert_eq!(input(&mut t, 3, 2), "950");

    // B3:C4 cut and pasted at H10: moved, the formulas that read them follow.
    let text = {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(2, 1);
        v.grid_extend_to(3, 2);
        v.selection_tsv()
    };
    t.app.run_command("edit.cut", json!({}));
    assert_eq!(
        t.app.doc.viewer.as_deref().unwrap().cut_range(),
        Some([2, 1, 3, 2])
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(9, 7);
    t.app.paste(&text, false);
    assert_eq!(input(&mut t, 2, 1), "");
    assert_eq!(input(&mut t, 9, 7), "431.5");
    assert_eq!(input(&mut t, 2, 3), "=H10+I10");
    assert_eq!(t.app.doc.viewer.as_deref().unwrap().cut_range(), None);
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 2, 1), "431.5");
    assert_eq!(input(&mut t, 2, 3), "=B3+C3");

    // Budget!B3:D3 cut and pasted on Dates at C10: moved across sheets.
    let text = {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(2, 1);
        v.grid_extend_to(2, 3);
        v.selection_tsv()
    };
    t.app.run_command("edit.cut", json!({}));
    t.app.run_command("viewer.grid.nextSheet", json!({}));
    assert!(t.status().starts_with("Dates"), "{}", t.status());
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(9, 2);
    t.app.paste(&text, false);
    assert_eq!(input(&mut t, 9, 2), "431.5");
    assert_eq!(input(&mut t, 9, 4), "=Dates!C10+Dates!D10");
    t.app.run_command("viewer.grid.previousSheet", json!({}));
    assert_eq!(input(&mut t, 2, 1), "");
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 2, 3), "=B3+C3");

    // Pasted text: rows into cells in one step; one value fills a selection.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(9, 0);
    t.app.paste("Books\t1,000.50\nTea\t=B10*2\n", false);
    assert_eq!(input(&mut t, 9, 0), "Books");
    assert_eq!(input(&mut t, 9, 1), "1000.5");
    assert_eq!(input(&mut t, 10, 1), "=B10*2");
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 9, 0), "");
    assert_eq!(input(&mut t, 10, 1), "");
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(11, 2);
        v.grid_extend_to(12, 3);
    }
    t.app.paste("7", false);
    assert_eq!(input(&mut t, 12, 3), "7");
    assert_eq!(input(&mut t, 11, 2), "7");

    // Wrap Text on a long text: the row grows to its lines.
    let long = "Paid on the first of every month, by bank transfer";
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 8, "col": 0, "value": long }),
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(8, 0);
    t.app.run_command("viewer.grid.wrapText", json!({}));
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    assert!(v.grid_cells(8..9, 0..1)[0].2.wrap);
    // 50 characters in a column 18 wide, 17 of them for text: three lines.
    assert_eq!(v.row_height(8), 45.0);
    // The key, `w`, turns it off again.
    t.key(KeyCode::Char('w'));
    assert!(
        !t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_cells(8..9, 0..1)[0]
            .2
            .wrap
    );

    // The next sheet.
    t.app.run_command("viewer.grid.nextSheet", json!({}));
    assert!(t.status().starts_with("Dates"), "{}", t.status());
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    t.screen();
    // A date wider than its column shows as #, as in a spreadsheet.
    assert!(t.screen().contains("########"));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().cell_input(),
        "2026-10-03"
    );
}
