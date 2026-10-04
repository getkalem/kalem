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

#[test]
fn sorting_and_filtering() {
    let mut t = T::open("sort");
    let name = |t: &mut T, row: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, 0);
        v.cell_input()
    };
    // A1:D4 selected from A1: sorted Z to A by Item, the header kept.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(0, 0);
        v.grid_extend_to(3, 3);
    }
    t.key(KeyCode::Char('s'));
    t.key(KeyCode::Char('d'));
    let names: Vec<String> = (0..4).map(|r| name(&mut t, r)).collect();
    assert_eq!(names, ["Item", "Travel", "Rent", "Food"]);
    // Rent's total moved with it, reading its own row.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 3);
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().cell_input(),
        "=B3+C3"
    );
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(name(&mut t, 1), "Rent");

    // The filter on the table at the cursor, a column filtered to Food.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    t.key(KeyCode::Char('f'));
    let f = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_layout()
        .unwrap()
        .filter;
    assert_eq!(f, Some([0, 0, 4, 3]));
    assert!(t.screen().contains('▾'), "{}", t.screen());
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().filter_values(0),
        ["Food", "Rent", "Sum", "Travel"]
    );
    t.app.run_command(
        "viewer.grid.setColumnFilter",
        json!({ "col": 0, "value": "Food" }),
    );
    let l = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_layout()
        .unwrap();
    assert_eq!(l.hidden_rows, vec![1, 3, 4]);
    let s = t.screen();
    assert!(
        s.contains('▼') && s.contains("Food") && !s.contains("Rent"),
        "{s}"
    );
    // Two values at once: Food and Travel.
    t.app.run_command(
        "viewer.grid.setColumnFilter",
        json!({ "col": 0, "values": ["Food", "Travel"] }),
    );
    let l = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_layout()
        .unwrap();
    assert_eq!(l.hidden_rows, vec![1, 4]);
    // The checklist in the palette: the values shown checked.
    t.app
        .run_command("viewer.grid.filterColumn", json!({ "col": 0 }));
    let s = t.screen();
    assert!(
        s.contains("☑ Food") && s.contains("☐ Rent") && s.contains("☑ Travel"),
        "{s}"
    );
    // Rent checked from the palette, then applied.
    for ch in "Rent".chars() {
        t.key(KeyCode::Char(ch));
    }
    t.key(KeyCode::Enter);
    let s = t.screen();
    assert!(s.contains("☑ Rent"), "{s}");
    for ch in "apply".chars() {
        t.key(KeyCode::Char(ch));
    }
    t.key(KeyCode::Enter);
    let l = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_layout()
        .unwrap();
    assert_eq!(l.hidden_rows, vec![4], "only Sum is hidden");
    t.app.run_command("viewer.grid.clearFilters", json!({}));
    assert!(
        t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .hidden_rows
            .is_empty()
    );
    t.key(KeyCode::Char('f'));
    assert!(
        t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .filter
            .is_none()
    );
}

#[test]
fn conditional_formatting() {
    let mut t = T::open("cf");
    let number = |t: &mut T, row: u32| -> f64 {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, 1);
        v.cell_input().parse().unwrap()
    };
    let values: Vec<f64> = (1..4).map(|r| number(&mut t, r)).collect();
    let least = values.iter().copied().fold(f64::MAX, f64::min);
    // B2:B4 selected; the menu, then Greater Than asking for its value.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(3, 1);
    }
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )));
    let s = t.screen();
    assert!(s.contains("Greater Than") && s.contains("Less Than"), "{s}");
    t.key(KeyCode::Esc);
    t.app.run_command("viewer.grid.highlightGreater", json!({}));
    let s = t.screen();
    assert!(s.contains("Greater Than: value"), "{s}");
    for ch in least.to_string().chars() {
        t.key(KeyCode::Char(ch));
    }
    t.key(KeyCode::Enter);
    let cells = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_cells(1..4, 1..2);
    for (r, _, cell) in &cells {
        let v = values[(*r - 1) as usize];
        assert_eq!(cell.fill.is_some(), v > least, "row {r}: {cell:?}");
    }
    // Icons on C2:C4 and bars on D2:D4, drawn.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 2);
        v.grid_extend_to(3, 2);
    }
    t.app
        .run_command("viewer.grid.iconSet", json!({ "name": "3Arrows" }));
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 3);
        v.grid_extend_to(3, 3);
    }
    t.app
        .run_command("viewer.grid.dataBars", json!({ "color": "#638EC6" }));
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(6, 0);
    let s = t.screen();
    assert!(s.contains('▲') && s.contains('▼'), "{s}");
    let buf = t.term.backend().buffer().clone();
    let bar = ratatui::style::Color::Rgb(0x63, 0x8E, 0xC6);
    assert!(buf.content().iter().any(|c| c.bg == bar), "{s}");
    // Cleared from the sheet, then back with undo.
    t.app
        .run_command("viewer.grid.clearSheetConditionalFormats", json!({}));
    let s = t.screen();
    assert!(!s.contains('▲'), "{s}");
    t.app.run_command("edit.undo", json!({}));
    assert!(t.screen().contains('▲'));
    // Saved: Excel's markup in the sheet.
    t.app.run_command("app.save", json!({}));
    let bytes = std::fs::read(t.dir.join("budget.xlsx")).unwrap();
    let mut wb = kalem_plugin_xlsx::Workbook::open(bytes).unwrap();
    assert_eq!(wb.conditional_formats(0).unwrap().len(), 3);
}

#[test]
fn data_validation() {
    let mut t = T::open("dv");
    let input = |t: &mut T, row: u32, col: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, col);
        v.cell_input()
    };
    // A list on A2:A5 that leaves Sum out, with an input message.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.grid_extend_to(4, 0);
    }
    t.app.run_command(
        "viewer.grid.validateList",
        json!({ "value": "Food, Rent, Travel" }),
    );
    t.app.run_command(
        "viewer.grid.validationMessage",
        json!({ "value": "Pick an item" }),
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    t.screen();
    assert!(t.status().contains("Pick an item"), "{}", t.status());
    assert!(t.screen().contains('▾'), "{}", t.screen());
    // Sum breaks it: circled.
    t.app.run_command("viewer.grid.circleInvalid", json!({}));
    assert!(t.screen().contains('⦅'), "{}", t.screen());
    // Chosen from the list.
    t.app
        .event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)));
    let s = t.screen();
    assert!(s.contains("Travel") && s.contains("Rent"), "{s}");
    for ch in "Trav".chars() {
        t.key(KeyCode::Char(ch));
    }
    t.key(KeyCode::Enter);
    assert_eq!(input(&mut t, 1, 0), "Travel");
    // A value outside the list: refused and asked again.
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 2, "col": 0, "value": "Cinema" }),
    );
    let s = t.screen();
    assert!(s.contains("Set Cell"), "{s}");
    t.key(KeyCode::Esc);
    assert_eq!(input(&mut t, 2, 0), "Food");
    // A warning instead: kept when the user says so.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 0);
    t.app.run_command(
        "viewer.grid.validationAlert",
        json!({ "style": "warning", "value": "Unusual item" }),
    );
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 2, "col": 0, "value": "Cinema" }),
    );
    let s = t.screen();
    assert!(
        s.contains("Unusual item") && s.contains("keep the value"),
        "{s}"
    );
    t.key(KeyCode::Enter);
    assert_eq!(input(&mut t, 2, 0), "Cinema");
    // Whole numbers over zero in B2:B5.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(4, 1);
    }
    t.app.run_command(
        "viewer.grid.validateNumber",
        json!({ "kind": "whole", "op": "greaterThan", "value": "0" }),
    );
    let check = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .check_input(3, 1, "-5");
    assert!(check.is_some());
    // Saved: the list (without A3), A3's warning list and the numbers.
    t.app.run_command("app.save", json!({}));
    let bytes = std::fs::read(t.dir.join("budget.xlsx")).unwrap();
    let mut wb = kalem_plugin_xlsx::Workbook::open(bytes).unwrap();
    assert_eq!(wb.validations(0).unwrap().len(), 3);
    t.app.run_command("viewer.grid.clearValidation", json!({}));
    assert!(
        t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .cursor_validation()
            .is_none()
    );
}

#[test]
fn pivot_tables() {
    let mut t = T::open("pivot");
    let units = |t: &mut T| {
        t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .structure()
            .units
            .len()
    };
    let before = units(&mut t);
    // From inside the table: Item for rows, no columns, Q1 summed.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('T'),
        KeyModifiers::SHIFT,
    )));
    let pick = |t: &mut T, what: &str| {
        let s = t.screen();
        assert!(s.contains(what), "{what}: {s}");
        for ch in what.chars() {
            t.key(KeyCode::Char(ch));
        }
        t.key(KeyCode::Enter);
    };
    pick(&mut t, "Item");
    pick(&mut t, "(No Column Field)");
    pick(&mut t, "Q1");
    pick(&mut t, "Sum of Q1");
    pick(&mut t, "Create PivotTable");
    assert_eq!(units(&mut t), before + 1);
    let s = t.screen();
    assert!(
        s.contains("Row Labels") && s.contains("Sum of Q1") && s.contains("Grand Total"),
        "{s}"
    );
    assert!(t.status().starts_with("Pivot1"), "{}", t.status());
    // Refresh All, then undo takes the sheet back.
    t.app.run_command("viewer.grid.refreshPivots", json!({}));
    assert!(t.screen().contains("Grand Total"));
    t.app.run_command("edit.undo", json!({}));
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(units(&mut t), before);
}

#[test]
fn charts() {
    let mut t = T::open("chart");
    // Q1 and Q2 by item, from the table at the cursor: the kinds offered.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1);
    t.app
        .event(Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::ALT)));
    let s = t.screen();
    assert!(s.contains("Column Chart") && s.contains("Pie Chart"), "{s}");
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.insertChart",
        json!({ "kind": "column", "title": "Spending" }),
    );
    let charts = t.app.doc.viewer.as_deref_mut().unwrap().charts();
    let c = charts.last().unwrap().clone();
    assert_eq!(c.series.len(), 3, "Q1, Q2 and Total");
    // Drawn over the cells it covers, with its title.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    let s = t.screen();
    assert!(s.contains("Spending"), "{s}");
    // Deleted from a cell it covers; undo brings it back.
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(c.anchor[0] + 1, c.anchor[1] + 1);
    t.app.run_command("viewer.grid.deleteChart", json!({}));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts().len(),
        charts.len() - 1
    );
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts().len(),
        charts.len()
    );
    // Moved down two rows and made a column wider with the keys, the
    // cursor going along.
    let i = charts.len() - 1;
    let anchor = |t: &mut T| t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].anchor;
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(c.anchor[0], c.anchor[1]);
    for _ in 0..2 {
        t.key(KeyCode::Char('p'));
        t.key(KeyCode::Down);
    }
    let a = anchor(&mut t);
    assert_eq!(
        a,
        [c.anchor[0] + 2, c.anchor[1], c.anchor[2] + 2, c.anchor[3]]
    );
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().grid_pos().row,
        a[0]
    );
    t.app.run_command("viewer.grid.chartWider", json!({}));
    assert_eq!(anchor(&mut t)[3], a[3] + 1);
    t.app.run_command("viewer.grid.chartShorter", json!({}));
    assert_eq!(anchor(&mut t)[2], a[2] - 1);
    // Retitled through the prompt, starting from the title it has.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('t'));
    let s = t.screen();
    assert!(s.contains("Chart Title") && s.contains("Spending"), "{s}");
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.chartTitle", json!({ "value": "Costs" }));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i]
            .title
            .as_deref(),
        Some("Costs")
    );
    t.app
        .run_command("viewer.grid.chartTitle", json!({ "value": "" }));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].title,
        None
    );
    // The axes titled: on the chart's bottom edge.
    t.app.run_command(
        "viewer.grid.horizontalAxisTitle",
        json!({ "value": "Item" }),
    );
    t.app
        .run_command("viewer.grid.verticalAxisTitle", json!({ "value": "TRY" }));
    let c = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert_eq!(
        (c.horizontal_title.as_deref(), c.vertical_title.as_deref()),
        (Some("Item"), Some("TRY"))
    );
    let s = t.screen();
    assert!(s.contains("↑ TRY") && s.contains("Item"), "{s}");
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('y'));
    let s = t.screen();
    assert!(s.contains("Vertical Axis Title"), "{s}");
    t.key(KeyCode::Esc);
    // The legend: offered where it may go, then moved to the right and
    // taken away.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('l'));
    let s = t.screen();
    assert!(s.contains("Top Right") && s.contains("None"), "{s}");
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.chartLegend", json!({ "position": "right" }));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].legend,
        Some(kalem_viewer::LegendPosition::Right)
    );
    let s = t.screen();
    let lines: Vec<&str> = s.lines().collect();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("■ Q1") && !l.contains("■ Q2")),
        "a column of entries: {s}"
    );
    t.app
        .run_command("viewer.grid.chartLegend", json!({ "position": "none" }));
    assert!(!t.screen().contains("■ Q1"));
    // Data labels: the checklist, Value checked from the palette and
    // applied; the bars show their values.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('d'));
    let s = t.screen();
    assert!(s.contains("☐ Value") && s.contains("Apply"), "{s}");
    for ch in "Value".chars() {
        t.key(KeyCode::Char(ch));
    }
    t.key(KeyCode::Enter);
    assert!(t.screen().contains("☑ Value"));
    for ch in "apply".chars() {
        t.key(KeyCode::Char(ch));
    }
    t.key(KeyCode::Enter);
    assert!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i]
            .labels
            .value
    );
    let s = t.screen();
    assert!(s.contains("1200"), "{s}");
    t.app.run_command(
        "viewer.grid.dataLabels",
        json!({ "value": false, "category": false, "series": false, "percent": false, "apply": true }),
    );
    assert!(
        !t.app.doc.viewer.as_deref_mut().unwrap().charts()[i]
            .labels
            .any()
    );
    // The value axis scaled: the menu, then a maximum typed with a decimal
    // comma, a major unit, and back to automatic.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('s'));
    let s = t.screen();
    assert!(
        s.contains("Maximum") && s.contains("Logarithmic Scale"),
        "{s}"
    );
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.axisScale",
        json!({ "field": "max", "value": "5000,5" }),
    );
    t.app.run_command(
        "viewer.grid.axisScale",
        json!({ "field": "major", "value": "1000" }),
    );
    let sc = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].scale;
    assert_eq!(
        (sc.max, sc.major, sc.min),
        (Some(5000.5), Some(1000.0), None)
    );
    t.app.run_command(
        "viewer.grid.axisScale",
        json!({ "field": "min", "value": "9999" }),
    );
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i]
            .scale
            .min,
        None,
        "a minimum over the maximum is refused"
    );
    t.app
        .run_command("viewer.grid.axisScale", json!({ "field": "auto" }));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].scale,
        kalem_viewer::AxisScale::default()
    );
    // Its kind: the menu marks the one it is; made a line chart, drawn in
    // braille, then back to columns.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('k'));
    let s = t.screen();
    assert!(s.contains("● Column") && s.contains("○ Line"), "{s}");
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.chartKind", json!({ "kind": "line" }));
    let c2 = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert_eq!(c2.kind, kalem_viewer::ChartKind::Line);
    assert_eq!(c2.series.len(), 3);
    let s = t.screen();
    assert!(
        s.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)),
        "{s}"
    );
    t.app
        .run_command("viewer.grid.chartKind", json!({ "kind": "column" }));
    // Series colors: the series offered, then the colors; Q2 made red,
    // drawn so; a typed color for Total; Q2 automatic again.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('c'));
    let s = t.screen();
    assert!(s.contains("Q1 (automatic)") && s.contains("Total"), "{s}");
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.seriesColor", json!({ "series": 1 }));
    let s = t.screen();
    assert!(
        s.contains("Red #FF0000") && s.contains("Color of Q2"),
        "{s}"
    );
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.seriesColor",
        json!({ "series": 1, "color": "#FF0000" }),
    );
    t.app.run_command(
        "viewer.grid.seriesColor",
        json!({ "series": 2, "value": "#00aa00" }),
    );
    let c3 = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert_eq!(
        (c3.series[0].color, c3.series[1].color, c3.series[2].color),
        (None, Some([0xFF, 0, 0]), Some([0, 0xAA, 0]))
    );
    t.screen();
    let red = ratatui::style::Color::Rgb(0xFF, 0, 0);
    assert!(
        t.term
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg == red)
    );
    t.app.run_command(
        "viewer.grid.seriesColor",
        json!({ "series": 1, "color": "auto" }),
    );
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].series[1].color,
        None
    );
    // A point of its own color: Q1's Food bar green, drawn so, then Q1's
    // color again.
    t.app
        .run_command("viewer.grid.pointColor", json!({ "series": 0 }));
    let s = t.screen();
    assert!(
        s.contains("Food (series)") && s.contains("Point Color"),
        "{s}"
    );
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.pointColor",
        json!({ "series": 0, "point": 1, "color": "#00FF00" }),
    );
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].series[0].point_colors,
        vec![(1, [0, 0xFF, 0])]
    );
    t.screen();
    let green = ratatui::style::Color::Rgb(0, 0xFF, 0);
    assert!(
        t.term
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg == green)
    );
    t.app.run_command(
        "viewer.grid.pointColor",
        json!({ "series": 0, "point": 1, "color": "auto" }),
    );
    assert!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].series[0]
            .point_colors
            .is_empty()
    );
    // The chart area: the menu; a dark border drawn so, no border, then
    // the style's again with a light background.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('b'));
    let s = t.screen();
    assert!(
        s.contains("Background… (automatic)") && s.contains("Border…"),
        "{s}"
    );
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.chartArea", json!({ "part": "border" }));
    assert!(t.screen().contains("No Border"));
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.chartArea",
        json!({ "part": "border", "color": "#264478" }),
    );
    t.screen();
    let navy = ratatui::style::Color::Rgb(0x26, 0x44, 0x78);
    assert!(
        t.term
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg == navy && c.symbol() == "│")
    );
    t.app.run_command(
        "viewer.grid.chartArea",
        json!({ "part": "border", "color": "none" }),
    );
    t.app.run_command(
        "viewer.grid.chartArea",
        json!({ "part": "background", "value": "#FFF2CC" }),
    );
    let c4 = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert_eq!(
        (c4.background, c4.border),
        (
            kalem_viewer::Paint::Color([0xFF, 0xF2, 0xCC]),
            kalem_viewer::Paint::None
        )
    );
    t.app.run_command(
        "viewer.grid.chartArea",
        json!({ "part": "background", "color": "auto" }),
    );
    t.app.run_command(
        "viewer.grid.chartArea",
        json!({ "part": "border", "color": "auto" }),
    );
    // The plot area: offered with the chart area, painted light gray.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('b'));
    assert!(
        t.screen().contains("Plot Area Background"),
        "{}",
        t.screen()
    );
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.chartArea",
        json!({ "part": "plotBackground", "color": "#F2F2F2" }),
    );
    let c5 = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert_eq!(c5.plot_background, kalem_viewer::Paint::Color([0xF2; 3]));
    assert_eq!(
        c5.background,
        kalem_viewer::Paint::Automatic,
        "the chart area's untouched"
    );
    t.screen();
    let gray = ratatui::style::Color::Rgb(0xF2, 0xF2, 0xF2);
    assert!(
        t.term
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.bg == gray)
    );
    t.app.run_command(
        "viewer.grid.chartArea",
        json!({ "part": "plotBackground", "color": "auto" }),
    );
    // Gridlines: the checklist, its horizontal ones on as made; vertical
    // ones shown from the palette, the list offered again.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('g'));
    let s = t.screen();
    assert!(
        s.contains("☑ Major Horizontal") && s.contains("☐ Major Vertical"),
        "{s}"
    );
    for ch in "Major Vert".chars() {
        t.key(KeyCode::Char(ch));
    }
    t.key(KeyCode::Enter);
    assert!(t.screen().contains("☑ Major Vertical"), "{}", t.screen());
    t.key(KeyCode::Esc);
    let g = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].gridlines;
    assert!(g.horizontal_major && g.vertical_major && !g.horizontal_minor);
    t.app.run_command(
        "viewer.grid.gridlines",
        json!({ "toggle": "horizontalMajor" }),
    );
    t.key(KeyCode::Esc);
    assert!(
        !t.app.doc.viewer.as_deref_mut().unwrap().charts()[i]
            .gridlines
            .horizontal_major
    );
    // The value axis's number format: the presets with an example, then
    // a percent; the cells' own again.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('n'));
    let s = t.screen();
    assert!(
        s.contains("#,##0.00   1,234.50") && s.contains("The Cells' Own"),
        "{s}"
    );
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.axisFormat",
        json!({ "format": "#,##0 \"TL\"" }),
    );
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i]
            .axis_format
            .as_deref(),
        Some("#,##0 \"TL\"")
    );
    t.app
        .run_command("viewer.grid.axisFormat", json!({ "format": "auto" }));
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].axis_format,
        None
    );
    // The axes' font: the axes offered; the categories (horizontal) bold
    // and red, drawn so; a size typed for the values; back to the style's.
    t.key(KeyCode::Char('p'));
    t.key(KeyCode::Char('f'));
    assert!(t.screen().contains("Both Axes"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.axisFont",
        json!({ "axis": "horizontal", "op": "bold" }),
    );
    assert!(t.screen().contains("☑ Bold"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.axisFont",
        json!({ "axis": "horizontal", "op": "color", "color": "#FF0000" }),
    );
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.axisFont",
        json!({ "axis": "vertical", "op": "size", "value": "12" }),
    );
    t.key(KeyCode::Esc);
    let c6 = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert!(c6.horizontal_font.bold);
    assert_eq!(c6.horizontal_font.color, Some([0xFF, 0, 0]));
    assert_eq!(c6.vertical_font.size, Some(12.0));
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    t.screen();
    let red = ratatui::style::Color::Rgb(0xFF, 0, 0);
    let buf = t.term.backend().buffer().clone();
    assert!(
        buf.content()
            .iter()
            .any(|c| c.fg == red && c.symbol() == "R"),
        "Rent in red"
    );
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(c.anchor[0] + 2, c.anchor[1]);
    t.app.run_command(
        "viewer.grid.axisFont",
        json!({ "axis": "both", "op": "reset" }),
    );
    t.key(KeyCode::Esc);
    let c7 = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert!(c7.horizontal_font.is_default() && c7.vertical_font.is_default());
    // The title's font: refused without a title; with one, red and bold,
    // drawn so, and kept when the title's words change.
    t.app
        .run_command("viewer.grid.chartTitle", json!({ "value": "" }));
    t.app.run_command("viewer.grid.titleFont", json!({}));
    assert!(
        t.screen().contains("Give the chart a title first"),
        "{}",
        t.screen()
    );
    t.app
        .run_command("viewer.grid.chartTitle", json!({ "value": "Spending" }));
    t.app.run_command(
        "viewer.grid.titleFont",
        json!({ "op": "color", "color": "#FF0000" }),
    );
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.chartTitle", json!({ "value": "Costs" }));
    let c8 = t.app.doc.viewer.as_deref_mut().unwrap().charts()[i].clone();
    assert_eq!(
        (c8.title.as_deref(), c8.title_font.color),
        (Some("Costs"), Some([0xFF, 0, 0]))
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    t.screen();
    let red = ratatui::style::Color::Rgb(0xFF, 0, 0);
    assert!(
        t.term
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg == red && c.symbol() == "C")
    );
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(c.anchor[0] + 2, c.anchor[1]);
    // The legend's font: refused without a legend; with one at the
    // bottom, green names in its line.
    t.app.run_command("viewer.grid.legendFont", json!({}));
    assert!(t.screen().contains("has no legend"), "{}", t.screen());
    t.app
        .run_command("viewer.grid.chartLegend", json!({ "position": "bottom" }));
    t.app.run_command(
        "viewer.grid.legendFont",
        json!({ "op": "color", "color": "#00B050" }),
    );
    t.key(KeyCode::Esc);
    assert_eq!(
        t.app.doc.viewer.as_deref_mut().unwrap().charts()[i]
            .legend_font
            .color,
        Some([0, 0xB0, 0x50])
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    t.screen();
    let green = ratatui::style::Color::Rgb(0, 0xB0, 0x50);
    assert!(
        t.term
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg == green && c.symbol() == "Q")
    );
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(c.anchor[0] + 2, c.anchor[1]);
    // A column chart's slices do not stand out.
    t.app.run_command("viewer.grid.explodeSlice", json!({}));
    assert!(t.screen().contains("Only a pie's"), "{}", t.screen());
    // A line chart in braille.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1);
    t.app
        .run_command("viewer.grid.insertChart", json!({ "kind": "line" }));
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    let s = t.screen();
    assert!(
        s.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)),
        "{s}"
    );
    // A pie on top, its first slice pulled out: marked in the terminal.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1);
    t.app
        .run_command("viewer.grid.insertChart", json!({ "kind": "pie" }));
    let a = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .charts()
        .last()
        .unwrap()
        .anchor;
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(a[0], a[1]);
    t.app.run_command(
        "viewer.grid.explodeSlice",
        json!({ "point": 0, "percent": 25 }),
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    let s = t.screen();
    assert!(s.contains("» Rent"), "{s}");
    // The line chart's axis in percent, written on its labels.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1);
    t.app
        .run_command("viewer.grid.insertChart", json!({ "kind": "line" }));
    let a = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .charts()
        .last()
        .unwrap()
        .anchor;
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(a[0], a[1]);
    t.app
        .run_command("viewer.grid.axisFormat", json!({ "format": "#,##0,\"K\"" }));
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    let s = t.screen();
    assert!(s.contains("0K"), "{s}");
}

#[test]
fn fill_handle() {
    let mut t = T::open("fill");
    let input = |t: &mut T, row: u32, col: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, col);
        v.cell_input()
    };
    // Fill Down (Ctrl+D): D2's formula copied to D3:D5, each reading its row.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 3);
        v.grid_extend_to(4, 3);
    }
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(input(&mut t, 4, 3), "=B5+C5");
    // A series from the keyboard: "Week 1" in F2, F2:F5 selected.
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 1, "col": 5, "value": "Week 1" }),
    );
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 5);
        v.grid_extend_to(4, 5);
    }
    t.app.run_command("viewer.grid.fillSeries", json!({}));
    assert_eq!(input(&mut t, 4, 5), "Week 4");
    // The fill handle's drag, as a command: Ocak across three columns.
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 7, "col": 0, "value": "Ocak" }),
    );
    t.app.run_command(
        "viewer.grid.fillSeries",
        json!({ "source": [7, 0, 7, 0], "target": [7, 0, 7, 2] }),
    );
    assert_eq!(input(&mut t, 7, 2), "Mart");
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 7, 2), "");
    // Down along the data: E2 "=D2*2", A2:A5 filled beside it.
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 1, "col": 4, "value": "=D2*2" }),
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 4);
    t.app.run_command("viewer.grid.fillToEnd", json!({}));
    assert_eq!(input(&mut t, 4, 4), "=D5*2");
    assert_eq!(input(&mut t, 5, 4), "", "it stops where the data does");
}

#[test]
fn flash_fill() {
    let mut t = T::open("flash");
    let input = |t: &mut T, row: u32, col: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, col);
        v.cell_input()
    };
    // Beside the budget's table, E2 "RENT:1200": the item in capitals, a
    // colon and Q1's figure; Ctrl+E fills E3:E5 so.
    let q1 = input(&mut t, 1, 1);
    let shown = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_cells(1..2, 1..2)[0]
        .2
        .text
        .clone();
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 1, "col": 4, "value": format!("RENT:{shown}") }),
    );
    let _ = q1;
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 4);
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    )));
    let food_shown = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_cells(2..3, 1..2)[0]
        .2
        .text
        .clone();
    assert_eq!(input(&mut t, 2, 4), format!("FOOD:{food_shown}"));
    assert!(input(&mut t, 4, 4).starts_with("SUM:"));
    assert!(t.screen().contains("Flash Fill: 3 cells"), "{}", t.screen());
    // One undo step takes them all back.
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 2, 4), "");
    assert_eq!(input(&mut t, 1, 4), "RENT:1,200.00");
}

#[test]
fn font_formatting() {
    let mut t = T::open("font");
    let cell = |t: &mut T, row: u32, col: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, col);
        v.cursor_cell()
    };
    // A2:B3 bold with Ctrl+B, then not, with it again.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.grid_extend_to(2, 1);
    }
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    )));
    assert!(cell(&mut t, 2, 1).bold);
    let s = t.screen();
    let buf = t.term.backend().buffer().clone();
    assert!(
        buf.content()
            .iter()
            .any(|c| c.symbol() == "R" && c.modifier.contains(ratatui::style::Modifier::BOLD)),
        "{s}"
    );
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.grid_extend_to(2, 1);
    }
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    )));
    assert!(!cell(&mut t, 1, 0).bold);
    // Italic, a red font, a yellow fill, 14 point Arial on A2.
    // Ctrl+I is Tab to a terminal: t i.
    t.key(KeyCode::Char('t'));
    t.key(KeyCode::Char('i'));
    t.app
        .run_command("viewer.grid.fontColor", json!({ "color": "#FF0000" }));
    t.app.run_command("viewer.grid.fillColor", json!({}));
    assert!(t.screen().contains("No Fill"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.fillColor", json!({ "color": "#FFFF00" }));
    t.app
        .run_command("viewer.grid.fontSize", json!({ "value": "14" }));
    t.app
        .run_command("viewer.grid.fontFace", json!({ "value": "Arial" }));
    let c = cell(&mut t, 1, 0);
    assert!(c.italic);
    assert_eq!(
        (c.color, c.fill),
        (Some([0xFF, 0, 0]), Some([0xFF, 0xFF, 0]))
    );
    assert_eq!((c.font_size, c.face.as_deref()), (Some(140), Some("Arial")));
    // Saved as Excel reads it; undone step by step.
    t.app.run_command("app.save", json!({}));
    let bytes = std::fs::read(t.dir.join("budget.xlsx")).unwrap();
    let mut wb = kalem_plugin_xlsx::Workbook::open(bytes).unwrap();
    let s = wb.sheet(0).unwrap().cells[&kalem_plugin_xlsx::CellRef::new(1, 0)].style;
    assert!(wb.style(s).italic && wb.style(s).font.as_deref() == Some("Arial"));
    for _ in 0..5 {
        t.app.run_command("edit.undo", json!({}));
    }
    let c = cell(&mut t, 1, 0);
    // Back to the workbook's own black.
    assert!(!c.italic && c.fill.is_none() && c.color == Some([0, 0, 0]));
}

#[test]
fn alignment() {
    let mut t = T::open("align");
    let row2 = |t: &mut T| {
        t.screen()
            .lines()
            .find(|l| l.contains("Ren") && l.trim_start().starts_with('2'))
            .unwrap_or_else(|| panic!("{}", t.screen()))
            .to_owned()
    };
    // A2 to the right with t r: "Rent" ends at the column's edge (its
    // last letter under the note's mark).
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    let left = row2(&mut t).find("Ren").unwrap();
    t.key(KeyCode::Char('t'));
    t.key(KeyCode::Char('r'));
    let c = t.app.doc.viewer.as_deref_mut().unwrap().cursor_cell();
    assert_eq!(c.align, kalem_viewer::Align::Right);
    let right = row2(&mut t).find("Ren").unwrap();
    assert!(right > left + 5, "{}", t.screen());
    // Centered, then t e again back to General, as Excel's button.
    t.key(KeyCode::Char('t'));
    t.key(KeyCode::Char('e'));
    let center = row2(&mut t).find("Ren").unwrap();
    assert!(left < center && center < right, "{}", t.screen());
    t.key(KeyCode::Char('t'));
    t.key(KeyCode::Char('e'));
    let c = t.app.doc.viewer.as_deref_mut().unwrap().cursor_cell();
    assert_eq!(c.align, kalem_viewer::Align::General);
    // Top and middle, kept in the cell.
    t.key(KeyCode::Char('t'));
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('T'),
        KeyModifiers::SHIFT,
    )));
    let c = t.app.doc.viewer.as_deref_mut().unwrap().cursor_cell();
    assert_eq!(c.valign, kalem_viewer::VAlign::Top);
    t.app.run_command("viewer.grid.alignMiddle", json!({}));
    let c = t.app.doc.viewer.as_deref_mut().unwrap().cursor_cell();
    assert_eq!(c.valign, kalem_viewer::VAlign::Middle);
    // Saved as Excel reads it; undone a step at a time.
    t.app.run_command("app.save", json!({}));
    let bytes = std::fs::read(t.dir.join("budget.xlsx")).unwrap();
    let mut wb = kalem_plugin_xlsx::Workbook::open(bytes).unwrap();
    let s = wb.sheet(0).unwrap().cells[&kalem_plugin_xlsx::CellRef::new(1, 0)].style;
    assert_eq!(wb.style(s).valign.as_deref(), Some("center"));
    for _ in 0..5 {
        t.app.run_command("edit.undo", json!({}));
    }
    let c = t.app.doc.viewer.as_deref_mut().unwrap().cursor_cell();
    assert_eq!(
        (c.align, c.valign),
        (kalem_viewer::Align::General, kalem_viewer::VAlign::Bottom)
    );
    assert_eq!(row2(&mut t).find("Ren"), Some(left));
}

#[test]
fn borders() {
    let mut t = T::open("borders");
    // A red line color, then outside borders round B2:C3 from the menu.
    t.key(KeyCode::Char('t'));
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('D'),
        KeyModifiers::SHIFT,
    )));
    assert!(t.screen().contains("Line Color: Red"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.borderColor", json!({ "color": "#FF0000" }));
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(2, 2);
    }
    t.key(KeyCode::Char('t'));
    t.key(KeyCode::Char('d'));
    assert!(
        t.screen().contains("Thick Outside Borders"),
        "{}",
        t.screen()
    );
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.borders", json!({ "set": "outside" }));
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    v.grid_move_to(2, 2);
    let c3 = v.cursor_cell();
    let red = Some([0xFF, 0, 0]);
    assert_eq!(c3.borders, [None, red, red, None]);
    // Drawn: C3's right side a red line, row 3's cells underlined.
    v.grid_move_to(6, 0);
    let s = t.screen();
    let buf = t.term.backend().buffer().clone();
    let y = s
        .lines()
        .position(|l| l.trim_start().starts_with("3 Food"))
        .unwrap() as u16;
    let reds: Vec<u16> = (0..buf.area.width)
        .filter(|&x| {
            buf[(x, y)].symbol() == "│" && buf[(x, y)].fg == ratatui::style::Color::Rgb(0xFF, 0, 0)
        })
        .collect();
    assert_eq!(reds.len(), 2, "B3's left and C3's right: {s}");
    let under = (0..buf.area.width)
        .filter(|&x| {
            buf[(x, y)]
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        })
        .count();
    assert!(under >= 16, "{under} {s}");
    // Taken away, one undo step.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(2, 2);
    }
    t.app
        .run_command("viewer.grid.borders", json!({ "set": "none" }));
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    v.grid_move_to(2, 2);
    assert_eq!(v.cursor_cell().borders, [None; 4]);
    t.app.run_command("edit.undo", json!({}));
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    assert_eq!(v.cursor_cell().borders, [None, red, red, None]);
}

#[test]
fn number_formats() {
    let mut t = T::open("numfmt");
    // B2:C2 from the menu (t 1), as a percent.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(1, 2);
    }
    t.key(KeyCode::Char('t'));
    t.key(KeyCode::Char('1'));
    assert!(t.screen().contains("Scientific"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.numberFormat", json!({ "code": "0%" }));
    assert!(t.screen().contains("120000%"), "{}", t.screen());
    // A decimal more with t ., two fewer with t , (the second does nothing).
    t.key(KeyCode::Char('t'));
    t.key(KeyCode::Char('.'));
    assert!(t.screen().contains("120000.0%"), "{}", t.screen());
    for _ in 0..2 {
        t.key(KeyCode::Char('t'));
        t.key(KeyCode::Char(','));
    }
    let s = t.screen();
    assert!(s.contains("120000%") && !s.contains("120000.0%"), "{s}");
    // General goes by what the cell shows: 431.5 gets 431.50.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(2, 1);
    }
    t.app
        .run_command("viewer.grid.numberFormat", json!({ "code": "General" }));
    assert!(t.screen().contains("431.5 "), "{}", t.screen());
    t.app.run_command("viewer.grid.increaseDecimal", json!({}));
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    assert_eq!(v.cursor_format().as_deref(), Some("0.00"));
    // A typed code, kept through saving.
    t.app
        .run_command("viewer.grid.numberFormat", json!({ "code": "0.000" }));
    assert!(t.screen().contains("431.500"), "{}", t.screen());
    t.app.run_command("app.save", json!({}));
    let bytes = std::fs::read(t.dir.join("budget.xlsx")).unwrap();
    let mut wb = kalem_plugin_xlsx::Workbook::open(bytes).unwrap();
    let s = wb.sheet(0).unwrap().cells[&kalem_plugin_xlsx::CellRef::new(2, 1)].style;
    assert_eq!(wb.style(s).num_fmt, "0.000");
    for _ in 0..6 {
        t.app.run_command("edit.undo", json!({}));
    }
    let s = t.screen();
    assert!(s.contains("1,200.00") && s.contains("431.50"), "{s}");
}

#[test]
fn moving_and_selecting() {
    let mut t = T::open("moving");
    let ctrl = |t: &mut T, k: KeyCode, shift: bool| {
        let m = if shift {
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        } else {
            KeyModifiers::CONTROL
        };
        t.app.event(Event::Key(KeyEvent::new(k, m)));
    };
    let pos = |t: &mut T| {
        let p = t.app.doc.viewer.as_deref_mut().unwrap().grid_pos();
        (p.row, p.col)
    };
    let sel = |t: &mut T| t.app.doc.viewer.as_deref_mut().unwrap().selection();
    // Ctrl+Down along the data to its end, then on to the sheet's edge,
    // and back up.
    ctrl(&mut t, KeyCode::Down, false);
    assert_eq!(pos(&mut t), (4, 0));
    ctrl(&mut t, KeyCode::Down, false);
    let max = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_layout()
        .unwrap();
    assert_eq!(pos(&mut t), (max.max_rows - 1, 0));
    ctrl(&mut t, KeyCode::Up, false);
    assert_eq!(pos(&mut t), (4, 0));
    ctrl(&mut t, KeyCode::Home, false);
    // Right to the table's last column, then over the gap to the next.
    ctrl(&mut t, KeyCode::Right, false);
    assert_eq!(pos(&mut t), (0, 3));
    ctrl(&mut t, KeyCode::Right, false);
    assert_eq!(pos(&mut t), (0, 5));
    // Ctrl+Shift+Down from B1 selects B1:B5.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 1);
    ctrl(&mut t, KeyCode::Down, true);
    assert_eq!(sel(&mut t), [0, 1, 4, 1]);
    // The row (g r; Shift+Space in the graphical editor), the column.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 1);
    t.key(KeyCode::Char('g'));
    t.key(KeyCode::Char('r'));
    assert_eq!(sel(&mut t), [2, 0, 2, max.max_cols - 1]);
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 1);
    ctrl(&mut t, KeyCode::Char(' '), false);
    assert_eq!(sel(&mut t), [0, 1, max.max_rows - 1, 1]);
    // Ctrl+A: the table around the cursor, then the sheet.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 1);
    ctrl(&mut t, KeyCode::Char('a'), false);
    assert_eq!(sel(&mut t), [0, 0, 4, 3]);
    ctrl(&mut t, KeyCode::Char('a'), false);
    assert_eq!(sel(&mut t), [0, 0, max.max_rows - 1, max.max_cols - 1]);
    // Go To (F5): asked, then a cell, a range, another sheet's cell.
    t.key(KeyCode::F(5));
    assert!(t.screen().contains("Go To"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.goTo", json!({ "value": "C4" }));
    assert_eq!(pos(&mut t), (3, 2));
    t.app
        .run_command("viewer.grid.goTo", json!({ "value": "$B$2:C3" }));
    assert_eq!((sel(&mut t), pos(&mut t)), ([1, 1, 2, 2], (1, 1)));
    t.app
        .run_command("viewer.grid.goTo", json!({ "value": "Dates!B2" }));
    assert!(t.screen().contains("Dates · B2"), "{}", t.screen());
}

#[test]
fn selection_sums() {
    let mut t = T::open("sums");
    // One cell: nothing summed.
    assert!(!t.screen().contains("Sum:"), "{}", t.screen());
    // B2:D5's twelve numbers, in the cells' format.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(4, 3);
    }
    let s = t.screen();
    assert!(
        s.contains("Average: 1,431.25 · Count: 12 · Sum: 17,175.00"),
        "{s}"
    );
    // Text only: the count.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.grid_extend_to(3, 0);
    }
    let s = t.screen();
    assert!(s.contains("Count: 3") && !s.contains("Sum:"), "{s}");
    // After an edit, summed again.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(2, 1);
    }
    assert!(t.screen().contains("Sum: 1,631.50"), "{}", t.screen());
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 1, "col": 1, "value": "1000" }),
    );
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(2, 1);
    }
    assert!(t.screen().contains("Sum: 1,431.50"), "{}", t.screen());
}

#[test]
fn find_and_replace() {
    let mut t = T::open("find");
    let pos = |t: &mut T| {
        let p = t.app.doc.viewer.as_deref_mut().unwrap().grid_pos();
        (p.row, p.col)
    };
    // Ctrl+F asks; found from the cursor on.
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    )));
    let asked = t.screen();
    t.key(KeyCode::Esc);
    assert!(asked.contains("Find"), "{asked}");
    t.app
        .run_command("viewer.grid.find", json!({ "value": "food" }));
    assert_eq!(pos(&mut t), (2, 0));
    assert!(t.screen().contains("food 1 of 1"), "{}", t.screen());
    // Part of what cells show: 950.00 twice, then round the sheet.
    t.app
        .run_command("viewer.grid.find", json!({ "value": "950" }));
    assert_eq!(pos(&mut t), (3, 2));
    t.key(KeyCode::F(3));
    assert_eq!(pos(&mut t), (3, 3));
    t.key(KeyCode::F(3));
    assert_eq!(pos(&mut t), (3, 2));
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::F(4),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    )));
    assert_eq!(pos(&mut t), (3, 3));
    // Match Case, then the whole cell, then in formulas.
    t.app.run_command("viewer.grid.findMatchCase", json!({}));
    t.app
        .run_command("viewer.grid.find", json!({ "value": "food" }));
    assert!(t.screen().contains("Cannot find food"), "{}", t.screen());
    t.app.run_command("viewer.grid.findMatchCase", json!({}));
    t.app.run_command("viewer.grid.findWholeCell", json!({}));
    t.app
        .run_command("viewer.grid.find", json!({ "value": "foo" }));
    assert!(t.screen().contains("Cannot find foo"), "{}", t.screen());
    t.app.run_command("viewer.grid.findWholeCell", json!({}));
    t.app.run_command("viewer.grid.findInFormulas", json!({}));
    t.app
        .run_command("viewer.grid.find", json!({ "value": "sum(" }));
    let (r, c) = pos(&mut t);
    let input = t.app.doc.viewer.as_deref_mut().unwrap().cell_input();
    assert!(input.to_lowercase().contains("sum("), "{r},{c}: {input}");
    t.app.run_command("viewer.grid.findInFormulas", json!({}));
    // Replace: what with asked, then Replace All or one at a time.
    t.app
        .run_command("viewer.grid.replace", json!({ "value": "o" }));
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.replace", json!({ "value": "o", "with": "0" }));
    assert!(t.screen().contains("Replace All"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.replace",
        json!({ "value": "o", "with": "0", "how": "all" }),
    );
    let s = t.screen();
    assert!(
        s.contains("F00d") && s.contains("Tr") && s.contains("cells replaced"),
        "{s}"
    );
    t.app.run_command("edit.undo", json!({}));
    assert!(t.screen().contains("Food"), "{}", t.screen());
    // One at a time: the cursor's cell if it matches, then the next.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    t.app.run_command(
        "viewer.grid.replace",
        json!({ "value": "rent", "with": "Kira", "how": "one" }),
    );
    let s = t.screen();
    assert!(s.contains("Kira") && s.contains("Food"), "{s}");
}

#[test]
fn sheets() {
    let mut t = T::open("sheets");
    let shift_s = |t: &mut T, k: char| {
        t.app.event(Event::Key(KeyEvent::new(
            KeyCode::Char('S'),
            KeyModifiers::SHIFT,
        )));
        t.key(KeyCode::Char(k));
    };
    let status = |t: &mut T| {
        let s = t.screen();
        s.lines().rev().nth(1).unwrap_or_default().to_owned()
    };
    // Shift+F11: a new sheet before this one.
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::F(11),
        KeyModifiers::SHIFT,
    )));
    assert!(
        status(&mut t).starts_with("Sheet1 · A1 · 1/4"),
        "{}",
        t.screen()
    );
    // Renamed; moved right and back.
    shift_s(&mut t, 'r');
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.renameSheet", json!({ "value": "Gelir" }));
    assert!(
        status(&mut t).starts_with("Gelir · A1 · 1/4"),
        "{}",
        t.screen()
    );
    t.app
        .run_command("viewer.grid.renameSheet", json!({ "value": "Budget" }));
    assert!(
        t.screen().contains("There is a sheet named Budget"),
        "{}",
        t.screen()
    );
    shift_s(&mut t, 'l');
    assert!(
        status(&mut t).starts_with("Gelir · A1 · 2/4"),
        "{}",
        t.screen()
    );
    shift_s(&mut t, 'h');
    assert!(
        status(&mut t).starts_with("Gelir · A1 · 1/4"),
        "{}",
        t.screen()
    );
    // Hidden: the next visible sheet shows; unhidden from the list.
    shift_s(&mut t, 'x');
    assert!(status(&mut t).starts_with("Budget"), "{}", t.screen());
    shift_s(&mut t, 'u');
    assert!(t.screen().contains("Gelir"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.unhideSheet", json!({ "unit": 0 }));
    assert!(
        status(&mut t).starts_with("Gelir · A1 · 1/4"),
        "{}",
        t.screen()
    );
    // Deleted after asking.
    shift_s(&mut t, 'd');
    assert!(
        t.screen().contains("Delete the sheet Gelir"),
        "{}",
        t.screen()
    );
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.deleteSheet", json!({ "confirmed": true }));
    assert!(
        status(&mut t).starts_with("Budget · A1 · 1/3"),
        "{}",
        t.screen()
    );
    // All undone.
    for _ in 0..7 {
        t.app.run_command("edit.undo", json!({}));
    }
    let n = t
        .app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .structure()
        .units
        .len();
    assert_eq!(n, 3);
}

#[test]
fn hidden_rows_and_columns() {
    let mut t = T::open("hide");
    let z = |t: &mut T, k: char| {
        t.key(KeyCode::Char('z'));
        let m = if k.is_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        t.app.event(Event::Key(KeyEvent::new(KeyCode::Char(k), m)));
    };
    // Rows 2 and 3 hidden: the cursor goes on to row 4.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.grid_extend_to(2, 0);
    }
    z(&mut t, 'r');
    let s = t.screen();
    assert!(
        !s.contains("Rent") && !s.contains("Food") && s.contains("Travel"),
        "{s}"
    );
    assert!(s.contains("A4 "), "{s}");
    // Column B hidden: Q1 goes, Q2 shows.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 1);
    z(&mut t, 'c');
    let s = t.screen();
    let head = s.lines().nth(2).unwrap_or_default();
    assert!(!head.contains("Q1") && head.contains("Q2"), "{s}");
    // Shown again from a selection over them.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(0, 0);
        v.grid_extend_to(4, 2);
    }
    z(&mut t, 'R');
    z(&mut t, 'C');
    let s = t.screen();
    assert!(
        s.contains("Rent") && s.contains("Food") && s.contains("Q1"),
        "{s}"
    );
    // Undone step by step.
    t.app.run_command("edit.undo", json!({}));
    let s = t.screen();
    assert!(!s.lines().nth(2).unwrap_or_default().contains("Q1"), "{s}");
    for _ in 0..3 {
        t.app.run_command("edit.undo", json!({}));
    }
    let s = t.screen();
    assert!(s.contains("Rent") && s.contains("Q1"), "{s}");
}

#[test]
fn frozen_panes() {
    let mut t = T::open("freeze");
    let frozen = |t: &mut T| {
        t.app
            .doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .frozen
    };
    let z = |t: &mut T, k: char| {
        t.key(KeyCode::Char('z'));
        let m = if k.is_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        t.app.event(Event::Key(KeyEvent::new(KeyCode::Char(k), m)));
    };
    // Nothing to freeze above and left of A1.
    assert_eq!(frozen(&mut t), (0, 0));
    z(&mut t, 'f');
    assert!(
        t.screen().contains("Put the cursor below"),
        "{}",
        t.screen()
    );
    // At C3: two rows and two columns; they stay when the view moves on.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 2);
    z(&mut t, 'f');
    assert_eq!(frozen(&mut t), (2, 2));
    t.app
        .doc
        .viewer
        .as_deref_mut()
        .unwrap()
        .grid_move_to(200, 30);
    let s = t.screen();
    assert!(
        s.contains("Item") && s.contains("Rent") && s.contains("201"),
        "{s}"
    );
    // The top row, the first column, none.
    z(&mut t, 't');
    assert_eq!(frozen(&mut t), (1, 0));
    z(&mut t, 'F');
    assert_eq!(frozen(&mut t), (0, 1));
    // z f again unfreezes, as Excel's Unfreeze Panes; z u likewise.
    z(&mut t, 'f');
    assert_eq!(frozen(&mut t), (0, 0));
    z(&mut t, 't');
    z(&mut t, 'u');
    assert_eq!(frozen(&mut t), (0, 0));
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(frozen(&mut t), (1, 0));
}

#[test]
fn notes() {
    let mut t = T::open("notes");
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    // Shift+F2 asks with the note A2 has.
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::F(2),
        KeyModifiers::SHIFT,
    )));
    let asked = t.screen();
    t.key(KeyCode::Esc);
    assert!(
        asked.contains("Edit Note") && asked.contains("Paid on the first"),
        "{asked}"
    );
    t.app
        .run_command("viewer.grid.editNote", json!({ "value": "Kira ödendi" }));
    assert!(
        t.screen().contains("A2 · 1/3 · Kira ödendi"),
        "{}",
        t.screen()
    );
    // A new note on B3: its mark in the corner.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 1);
    t.app
        .run_command("viewer.grid.editNote", json!({ "value": "Kontrol et" }));
    let s = t.screen();
    let row3 = s
        .lines()
        .find(|l| l.trim_start().starts_with("3 Food"))
        .unwrap();
    assert!(row3.contains("431.5◥"), "{s}");
    // Left empty, the note goes; Delete Note likewise.
    t.app
        .run_command("viewer.grid.editNote", json!({ "value": "" }));
    assert!(!t.screen().contains("Kontrol et"), "{}", t.screen());
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    t.app.run_command("viewer.grid.deleteNote", json!({}));
    assert!(!t.screen().contains("Kira"), "{}", t.screen());
    // Saved as Excel reads it, then undone.
    t.app.run_command("app.save", json!({}));
    let bytes = std::fs::read(t.dir.join("budget.xlsx")).unwrap();
    let wb = kalem_plugin_xlsx::Workbook::open(bytes).unwrap();
    assert!(wb.comments(0).unwrap().is_empty());
    for _ in 0..4 {
        t.app.run_command("edit.undo", json!({}));
    }
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    assert!(t.screen().contains("Paid on the first"), "{}", t.screen());
}

#[test]
fn auto_sum_and_functions() {
    let mut t = T::open("autosum");
    let alt_eq = |t: &mut T| {
        t.app.event(Event::Key(KeyEvent::new(
            KeyCode::Char('='),
            KeyModifiers::ALT,
        )));
        t.screen()
    };
    // Under a column of numbers: their sum, proposed to be entered.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(5, 1);
    let s = alt_eq(&mut t);
    assert!(s.contains("=SUM(B2:B5)"), "{s}");
    t.key(KeyCode::Enter);
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    v.grid_move_to(5, 1);
    assert_eq!(v.cell_input(), "=SUM(B2:B5)");
    assert!(
        t.screen().contains("3,263.00") || t.screen().contains("3263"),
        "{}",
        t.screen()
    );
    t.app.run_command("edit.undo", json!({}));
    // Right of a row of numbers: theirs.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 4);
    let s = alt_eq(&mut t);
    assert!(s.contains("=SUM(B2:D2)"), "{s}");
    t.key(KeyCode::Esc);
    // A selection ending in an empty row: a sum under each column at once.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(5, 2);
    }
    alt_eq(&mut t);
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    v.grid_move_to(5, 2);
    assert_eq!(v.cell_input(), "=SUM(C2:C5)");
    t.app.run_command("edit.undo", json!({}));
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    v.grid_move_to(5, 1);
    assert_eq!(v.cell_input(), "");
    // Insert Function: the list with arguments, the chosen one begun.
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::F(3),
        KeyModifiers::SHIFT,
    )));
    for c in "vlookup".chars() {
        t.key(KeyCode::Char(c));
    }
    let s = t.screen();
    assert!(s.contains("VLOOKUP(lookup_value, table_array"), "{s}");
    t.key(KeyCode::Esc);
    t.app
        .run_command("viewer.grid.insertFunction", json!({ "name": "AVERAGE" }));
    assert!(t.screen().contains("=AVERAGE("), "{}", t.screen());
    t.key(KeyCode::Esc);
}

#[test]
fn paste_special() {
    let mut t = T::open("pastespecial");
    let input = |t: &mut T, r: u32, c: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(r, c);
        v.cell_input()
    };
    // Nothing copied yet.
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    )));
    assert!(t.screen().contains("Values"), "{}", t.screen());
    t.key(KeyCode::Esc);
    t.app.run_command(
        "viewer.grid.pasteSpecial",
        json!({ "what": "values", "transpose": false }),
    );
    assert!(
        t.screen().contains("Copy cells of the workbook first"),
        "{}",
        t.screen()
    );
    // D2:D3 copied (y): their values pasted at F2.
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 3);
        v.grid_extend_to(2, 3);
    }
    t.key(KeyCode::Char('y'));
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 5);
    t.app.run_command(
        "viewer.grid.pasteSpecial",
        json!({ "what": "values", "transpose": false }),
    );
    assert_eq!(input(&mut t, 1, 5), "2400");
    assert_eq!(input(&mut t, 2, 5), "943.75");
    // Transposed: across row 7 from B7.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(6, 1);
    t.app.run_command(
        "viewer.grid.pasteSpecial",
        json!({ "what": "all", "transpose": true }),
    );
    assert!(
        input(&mut t, 6, 1).starts_with('='),
        "{}",
        input(&mut t, 6, 1)
    );
    assert!(input(&mut t, 6, 2).starts_with('='));
    t.app.run_command("edit.undo", json!({}));
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 1, 5), "");
}

#[test]
fn inserting_and_deleting_cells() {
    let mut t = T::open("cells");
    let input = |t: &mut T, r: u32, c: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(r, c);
        v.cell_input()
    };
    let select = |t: &mut T, a: (u32, u32), b: (u32, u32)| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(a.0, a.1);
        v.grid_extend_to(b.0, b.1);
    };
    // Two rows selected: two rows inserted above them.
    select(&mut t, (1, 0), (2, 0));
    t.app.event(Event::Key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::SHIFT,
    )));
    assert_eq!(input(&mut t, 3, 0), "Rent");
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 1, 0), "Rent");
    // Cells shifted down: B2:B3 empty, their numbers two rows lower, and
    // the totals following them.
    select(&mut t, (1, 1), (2, 1));
    t.key(KeyCode::Char('g'));
    t.key(KeyCode::Char('i'));
    assert!(t.screen().contains("Shift Cells Down"), "{}", t.screen());
    t.key(KeyCode::Esc);
    select(&mut t, (1, 1), (2, 1));
    t.app
        .run_command("viewer.grid.insertCells", json!({ "how": "down" }));
    assert_eq!(input(&mut t, 1, 1), "");
    assert_eq!(input(&mut t, 3, 1), "1200");
    assert_eq!(input(&mut t, 2, 2), "512.25");
    assert!(
        input(&mut t, 1, 3).contains("B4"),
        "{}",
        input(&mut t, 1, 3)
    );
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 1, 1), "1200");
    // Cells deleted, those below moving up.
    select(&mut t, (1, 1), (1, 1));
    t.app
        .run_command("viewer.grid.deleteCells", json!({ "how": "up" }));
    assert_eq!(input(&mut t, 1, 1), "431.5");
    assert_eq!(input(&mut t, 1, 2), "1200");
    t.app.run_command("edit.undo", json!({}));
    // And from the right, moving left.
    select(&mut t, (1, 1), (1, 1));
    t.app
        .run_command("viewer.grid.deleteCells", json!({ "how": "left" }));
    assert_eq!(input(&mut t, 1, 1), "1200");
    assert!(
        input(&mut t, 1, 2).starts_with('='),
        "{}",
        input(&mut t, 1, 2)
    );
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 1, 1), "1200");
    assert_eq!(input(&mut t, 2, 1), "431.5");
}

#[test]
fn clear_formats_and_painter() {
    let mut t = T::open("painter");
    let tp = |t: &mut T, k: char| {
        t.key(KeyCode::Char('t'));
        let m = if k.is_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        t.app.event(Event::Key(KeyEvent::new(KeyCode::Char(k), m)));
    };
    let row = |t: &mut T, start: &str| {
        let s = t.screen();
        s.lines()
            .find(|l| l.trim_start().starts_with(start))
            .unwrap_or_default()
            .to_owned()
    };
    // B2's format cleared: 1200 as General.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1);
    tp(&mut t, 'x');
    assert!(row(&mut t, "2 Rent").contains(" 1200"), "{}", t.screen());
    // Painted from B3: t p takes it, t p again paints B2:C2.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(2, 1);
    tp(&mut t, 'p');
    assert!(
        t.screen().contains("Format Painter: select"),
        "{}",
        t.screen()
    );
    {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(1, 2);
    }
    tp(&mut t, 'p');
    assert!(row(&mut t, "2 Rent").contains("1,200.00"), "{}", t.screen());
    // Clear All: the value, the format and the note of A2.
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    tp(&mut t, 'X');
    let s = t.screen();
    assert!(
        !s.contains("Rent") && !s.contains("Paid on the first"),
        "{s}"
    );
    for _ in 0..3 {
        t.app.run_command("edit.undo", json!({}));
    }
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    let s = t.screen();
    assert!(
        s.contains("Rent") && s.contains("1,200.00") && s.contains("Paid on the first"),
        "{s}"
    );
}

#[test]
fn duplicates_and_text_to_columns() {
    let mut t = T::open("dupes");
    let input = |t: &mut T, r: u32, c: u32| {
        let v = t.app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(r, c);
        v.cell_input()
    };
    // A small table at F8 with a header and a repeated row.
    for (r, row) in [["Ad", "Yaş"], ["Ali", "30"], ["Ayşe", "25"], ["ali", "30"]]
        .iter()
        .enumerate()
    {
        for (c, v) in row.iter().enumerate() {
            t.app.run_command(
                "viewer.grid.setCell",
                json!({ "row": 7 + r, "col": 5 + c, "value": v }),
            );
        }
    }
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(8, 5);
    t.app.run_command("viewer.grid.removeDuplicates", json!({}));
    let s = t.screen();
    assert!(s.contains("All Columns") && s.contains("Yaş (G)"), "{s}");
    t.key(KeyCode::Esc);
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(8, 5);
    t.app
        .run_command("viewer.grid.removeDuplicates", json!({ "columns": [] }));
    assert!(
        t.screen()
            .contains("1 duplicate row removed; 2 unique rows"),
        "{}",
        t.screen()
    );
    assert_eq!(input(&mut t, 10, 5), "");
    assert_eq!(input(&mut t, 9, 6), "25");
    // Text to Columns at a comma.
    t.app.run_command(
        "viewer.grid.setCell",
        json!({ "row": 14, "col": 5, "value": "Kaya, Zeynep, 42" }),
    );
    t.app.doc.viewer.as_deref_mut().unwrap().grid_move_to(14, 5);
    t.app
        .run_command("viewer.grid.textToColumns", json!({ "delimiter": "," }));
    assert_eq!(input(&mut t, 14, 5), "Kaya");
    assert_eq!(input(&mut t, 14, 6), "Zeynep");
    assert_eq!(input(&mut t, 14, 7), "42");
    let v = t.app.doc.viewer.as_deref_mut().unwrap();
    v.grid_move_to(14, 7);
    assert!(v.cursor_cell().numeric);
    t.app.run_command("edit.undo", json!({}));
    assert_eq!(input(&mut t, 14, 5), "Kaya, Zeynep, 42");
}
