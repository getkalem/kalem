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
        t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
    t.key(KeyCode::Char('y'));
    let s = t.screen();
    assert!(s.contains("Vertical Axis Title"), "{s}");
    t.key(KeyCode::Esc);
    // The legend: offered where it may go, then moved to the right and
    // taken away.
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
    t.key(KeyCode::Char('h'));
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
