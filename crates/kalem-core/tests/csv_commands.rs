//! The CSV grid's commands on the rows and cells the user sees: with a
//! filter or a sorted view on, on a short record's missing cells, in a
//! file of one column or of a `sep=` line alone; each changes what it
//! names and nothing else (found by trying the commands as a user would,
//! 2026-10-07).

use std::sync::Arc;
use std::time::Instant;

use kalem_core::command::Clipboard;
use kalem_core::csv;
use kalem_core::{
    CommandRegistry, Config, DocumentMode, DocumentState, EditorContext, LineEnding, Metadata,
};
use serde_json::{Value, json};

struct Sheet {
    d: DocumentState,
    clip: Clipboard,
    reg: CommandRegistry,
    config: Config,
    messages: Vec<String>,
}

impl Sheet {
    fn new(text: &str) -> Sheet {
        let meta = Metadata {
            path: None,
            mode: DocumentMode::Csv,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: kalem_core::encoding_rs::UTF_8,
            lossy: false,
        };
        let config = Config::default();
        let mut d = DocumentState::new(text, meta, Arc::new(org_model::Settings::default()));
        d.set_mode(DocumentMode::Csv, &config.parse_base());
        Sheet {
            d,
            clip: Clipboard::default(),
            reg: CommandRegistry::with_builtins(),
            config,
            messages: Vec::new(),
        }
    }

    /// Runs command `id`; its error's text when it fails.
    fn run(&mut self, id: &str, args: Value) -> Result<(), String> {
        let mut ctx = EditorContext {
            document: Some(&mut self.d),
            clipboard: &mut self.clip,
            config: &self.config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 7).at(10, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let r = self.reg.execute(id, &mut ctx, &args);
        self.messages = ctx.messages;
        r.map_err(|e| e.to_string())
    }

    fn ok(&mut self, id: &str, args: Value) -> &mut Sheet {
        if let Err(e) = self.run(id, args) {
            panic!("{id}: {e}");
        }
        self
    }

    fn at(&mut self, cell: &str) -> &mut Sheet {
        self.ok("csv.goToCell", json!({ "cell": cell }))
    }

    fn text(&self) -> &str {
        self.d.text().as_str()
    }

    fn cell(&self) -> String {
        let (_, r, _, c) = csv::cell_at(&self.d).unwrap();
        csv_cell_name(r, c)
    }

    /// Types `text` as the editors do: into the grid, else as text.
    fn typ(&mut self, text: &str) {
        for c in text.chars() {
            let c = c.to_string();
            let now = Instant::now();
            if !self.d.type_in_grid(&c, now) {
                self.d.type_text(&c, false, now);
            }
        }
    }

    fn key(&mut self, id: &str) -> &mut Sheet {
        self.ok(id, Value::Null)
    }
}

fn csv_cell_name(row: usize, col: usize) -> String {
    kalem_core::csv_tools::coordinates(row, col).0
}

#[test]
fn a_short_records_missing_cell_is_that_cell_alone() {
    // Delete Column on C3, a cell "Alan" lacks, took every column from A
    // (the selection's anchor read as the record's last field).
    let mut s = Sheet::new("name,age,city\nAda,36,London\nAlan\nGrace,85,NY\n");
    s.at("C2").key("csv.cellBelow");
    assert_eq!(s.cell(), "C3");
    s.key("csv.deleteColumn");
    assert_eq!(s.text(), "name,age\nAda,36\nAlan\nGrace,85\n");
    // Cut on such a cell copies one cell and pads nothing.
    let mut s = Sheet::new("name,age,city\nAda,36,London\nAlan\nGrace,85,NY\n");
    s.at("C2").key("csv.cellBelow").key("csv.cutCells");
    assert_eq!(s.text(), "name,age,city\nAda,36,London\nAlan\nGrace,85,NY\n");
    assert_eq!(s.clip.text, "\n");
}

#[test]
fn a_selection_into_short_records_keeps_its_column() {
    // Fill Down over C2:C4 where rows 3 and 4 lack C filled A to C.
    let mut s = Sheet::new("k,v,w\na,1,x\nb\nc\n");
    s.at("C2").key("csv.extendDown").key("csv.extendDown");
    assert_eq!(s.cell(), "C4");
    s.key("csv.fillDown");
    assert_eq!(s.text(), "k,v,w\na,1,x\nb,,x\nc,,x\n");
    // From a missing cell down: the rectangle starts in its column.
    let mut s = Sheet::new("k,v,w\na\nb,2,y\n");
    s.at("C2").key("csv.extendDown").key("csv.clearCells");
    assert_eq!(s.text(), "k,v,w\na\nb,2,\n");
}

const TAGS: &str = "id,name,tag\n1,Ada,x\n2,Bob,y\n3,Cem,x\n4,Dan,y\n5,Eve,x\n";

#[test]
fn a_filter_keeps_its_hidden_rows_out_of_a_selection() {
    // Deleting the rows selected over a filter deleted the hidden ones
    // between them too.
    let mut s = Sheet::new(TAGS);
    s.ok("csv.filter", json!({"text": "x"}));
    s.at("B2").key("csv.extendDown").key("csv.deleteRow");
    assert_eq!(s.text(), "id,name,tag\n2,Bob,y\n4,Dan,y\n5,Eve,x\n");
    assert_eq!(s.cell(), "B4", "the row shown after them");
    // Fill Down over the rows shown.
    let mut s = Sheet::new(TAGS);
    s.ok("csv.filter", json!({"text": "x"}));
    s.at("A2").key("csv.extendDown").key("csv.extendDown");
    assert_eq!(s.cell(), "A6");
    s.key("csv.fillDown");
    assert_eq!(
        s.text(),
        "id,name,tag\n1,Ada,x\n2,Bob,y\n1,Cem,x\n4,Dan,y\n1,Eve,x\n"
    );
    // Delete over cells, and Copy.
    let mut s = Sheet::new(TAGS);
    s.ok("csv.filter", json!({"text": "x"}));
    s.at("B2").key("csv.extendDown").key("csv.copyCells");
    assert_eq!(s.clip.text, "Ada\nCem\n");
    s.key("csv.clearCells");
    assert_eq!(
        s.text(),
        "id,name,tag\n1,,x\n2,Bob,y\n3,,x\n4,Dan,y\n5,Eve,x\n"
    );
}

const SCORES: &str = "id,name,score\n1,Dan,40\n2,Bob,20\n3,Cem,30\n4,Ada,10\n";

#[test]
fn a_sorted_view_selects_the_rows_it_shows_together() {
    // Sorted by name: Ada, Bob, Cem, Dan. C5 (Ada) and the row below it
    // (Bob, C3): Fill Down wrote Cem's score too, between them in the file.
    let mut s = Sheet::new(SCORES);
    s.at("B2").key("csv.sortView");
    s.at("C5").key("csv.extendDown");
    assert_eq!(s.cell(), "C3");
    s.key("csv.fillDown");
    assert_eq!(
        s.text(),
        "id,name,score\n1,Dan,40\n2,Bob,10\n3,Cem,30\n4,Ada,10\n"
    );
    let mut s = Sheet::new(SCORES);
    s.at("B2").key("csv.sortView");
    s.at("B5").key("csv.extendDown").key("csv.deleteRow");
    assert_eq!(s.text(), "id,name,score\n1,Dan,40\n3,Cem,30\n");
    // Without a selection, Fill Down takes the cell shown above.
    let mut s = Sheet::new(SCORES);
    s.at("B2").key("csv.sortView");
    s.at("C3").key("csv.fillDown");
    assert_eq!(
        s.text(),
        "id,name,score\n1,Dan,40\n2,Bob,10\n3,Cem,30\n4,Ada,10\n"
    );
    // Move Row has no file order to move in.
    assert!(s.run("csv.moveRowDown", Value::Null).is_err());
}

#[test]
fn a_filter_moves_rows_past_the_hidden_ones() {
    let mut s = Sheet::new(TAGS);
    s.ok("csv.filter", json!({"text": "x"}));
    s.at("B2").key("csv.moveRowDown");
    assert_eq!(
        s.text(),
        "id,name,tag\n3,Cem,x\n2,Bob,y\n1,Ada,x\n4,Dan,y\n5,Eve,x\n"
    );
    assert_eq!(s.cell(), "B4");
    s.key("csv.moveRowUp");
    assert_eq!(s.text(), TAGS);
}

#[test]
fn rows_added_to_a_file_of_one_column() {
    // A final line feed alone is no record: the row was not added, and
    // typing went into the last value.
    let mut s = Sheet::new("name\nAda");
    s.at("A2").key("csv.insertRow");
    assert_eq!(s.cell(), "A3");
    assert_eq!(s.text(), "name\nAda\n\n");
    let mut s = Sheet::new("name\nAda");
    s.at("A2").key("csv.cellBelow");
    assert_eq!(s.cell(), "A3");
    let mut s = Sheet::new("");
    s.key("csv.insertRow");
    assert_eq!(s.cell(), "A2");
    // Insert Column in an empty file.
    let mut s = Sheet::new("");
    s.key("csv.insertColumn");
    assert_eq!(s.text(), ",");
}

#[test]
fn a_sep_line_is_not_a_record() {
    // Replace in Column replaced in the `sep=` line and in the header.
    let mut s = Sheet::new("sep=;\nname;x\nname;1\n");
    s.at("A2")
        .ok("csv.replaceInColumn", json!({"find": "name", "replace": "N"}));
    assert_eq!(s.text(), "sep=;\nname;x\nN;1\n");
    let mut s = Sheet::new("sep=;\n1;2\n3;4\n");
    s.ok("csv.replaceInColumn", json!({"find": "sep", "replace": "X"}));
    assert_eq!(s.text(), "sep=;\n1;2\n3;4\n");
    // A `sep=` line alone: one empty record the commands work on.
    let mut s = Sheet::new("sep=;\n");
    s.ok("csv.setField", json!({"value": "x"}));
    assert_eq!(s.text(), "sep=;\nx");
}

#[test]
fn fill_series_continues_as_a_spreadsheet_does() {
    let series = |text: &str, cell: &str| {
        let mut s = Sheet::new(text);
        s.at(cell).key("csv.fillSeries");
        s.text().to_string()
    };
    assert_eq!(series("v\n0.125\n0.250\n\n", "A4"), "v\n0.125\n0.250\n0.375\n");
    assert_eq!(series("v\n1.500\n\n", "A3"), "v\n1.500\n2.500\n");
    assert_eq!(series("v;w\n1,5;a\n2;b\n;c\n", "A4"), "v;w\n1,5;a\n2;b\n2,5;c\n");
    assert_eq!(series("v;w\n1,25;a\n1,5;b\n;c\n", "A4"), "v;w\n1,25;a\n1,5;b\n1,75;c\n");
    assert_eq!(series("d\n2026-01-31\n\n", "A3"), "d\n2026-01-31\n2026-02-01\n");
    assert_eq!(series("d\n28.02.2026\n\n", "A3"), "d\n28.02.2026\n01.03.2026\n");
    assert_eq!(
        series("d\n1/1/2026\n1/8/2026\n\n", "A4"),
        "d\n1/1/2026\n1/8/2026\n1/15/2026\n"
    );
    assert_eq!(series("v\nItem 2\nItem 1\n\n", "A4"), "v\nItem 2\nItem 1\nItem 0\n");
    assert_eq!(
        series("v\n9007199254740993\n\n", "A3"),
        "v\n9007199254740993\n9007199254740994\n"
    );
    // On the first data row there is nothing above to fill from.
    let mut s = Sheet::new("v\n1\n");
    s.at("A2");
    let e = s.run("csv.fillDown", Value::Null).unwrap_err();
    assert!(e.contains("above"), "{e}");
}

#[test]
fn sum_column() {
    let mut s = Sheet::new("item;price\na;1,5\nb;2,25\n;\n");
    s.at("B4").ok("csv.sumColumn", json!({"insert": true}));
    assert_eq!(s.text(), "item;price\na;1,5\nb;2,25\n;3,75\n");
    // Not over the header.
    let mut s = Sheet::new("item,price\na,1\nb,2\n");
    s.at("B1");
    assert!(s.run("csv.sumColumn", json!({"insert": true})).is_err());
    assert_eq!(s.text(), "item,price\na,1\nb,2\n");
    // With a filter on, the rows it shows.
    let mut s = Sheet::new("k,n\nx,1\ny,2\nx,4\n");
    s.ok("csv.filter", json!({"text": "x"}));
    s.at("B2").key("csv.sumColumn");
    assert!(s.messages.iter().any(|m| m.contains('5')), "{:?}", s.messages);
    let status = csv::status(&s.d).unwrap();
    assert!(status.contains("Count: 2"), "{status}");
}

#[test]
fn the_views_columns_follow_theirs() {
    let mut s = Sheet::new("a,b,c,d\n1,2,3,4\n");
    s.at("C1").key("csv.hideColumn");
    s.at("A1").key("csv.insertColumn");
    assert_eq!(s.d.csv_columns.hidden.iter().copied().collect::<Vec<_>>(), [3]);
    s.at("A1").key("csv.deleteColumn");
    assert_eq!(s.d.csv_columns.hidden.iter().copied().collect::<Vec<_>>(), [2]);
    s.at("B1").key("csv.moveColumnRight");
    assert_eq!(s.d.csv_columns.hidden.iter().copied().collect::<Vec<_>>(), [1]);
    assert_eq!(s.cell(), "C1");
    let mut s = Sheet::new("a,b,c\n1,2,3\n");
    s.at("C2").key("csv.sortView");
    s.at("B1").key("csv.deleteColumn");
    assert_eq!(s.d.csv_sort, Some((1, false)));
}

#[test]
fn commands_that_change_nothing_leave_the_document_unmodified() {
    let mut s = Sheet::new("n\n1\n2\n");
    s.at("A2").key("csv.sortFile");
    assert!(!s.d.is_modified());
    s.ok("csv.setField", json!({"value": "1"}));
    assert!(!s.d.is_modified());
}

#[test]
fn blank_lines_before_a_last_record_stay() {
    let mut s = Sheet::new("k\nb\n\na");
    s.at("A2").key("csv.sortFile");
    assert_eq!(s.text(), "k\na\nb\n\n");
    let mut s = Sheet::new("k\na\n\na");
    s.key("csv.removeDuplicates");
    assert_eq!(s.text(), "k\na\n\n");
}

#[test]
fn delete_row_leaves_the_cursor_on_a_row() {
    let mut s = Sheet::new("a,b,c\n1,2,3\n4,5,6\n");
    s.at("C3").key("csv.deleteRow");
    assert_eq!(s.text(), "a,b,c\n1,2,3\n");
    assert_eq!(s.cell(), "C2");
    let mut s = Sheet::new("a,b\r\n1,2\r\n3,4");
    s.at("A3").key("csv.deleteRow");
    assert_eq!(s.text(), "a,b\r\n1,2");
    assert_eq!(s.cell(), "A2");
}

#[test]
fn field_commands() {
    // Kill Field on a missing cell pads nothing; Yank with nothing to
    // yank leaves the cell.
    let mut s = Sheet::new("a,b\n1\n");
    s.at("B2").key("csv.killField");
    assert_eq!(s.text(), "a,b\n1\n");
    s.clip.text.clear();
    s.at("A2");
    assert!(s.run("csv.yankField", Value::Null).is_err());
    assert_eq!(s.text(), "a,b\n1\n");
    // Go to Cell past the last column is no cell.
    assert!(s.run("csv.goToCell", json!({"cell": "Z1"})).is_err());
    // Sort File by Columns by a header's name, and not by a word read as
    // letters.
    let mut s = Sheet::new("name,age\nAda,36\nBob,7\n");
    s.ok("csv.sortFileBy", json!({"columns": "age"}));
    assert_eq!(s.text(), "name,age\nBob,7\nAda,36\n");
    assert!(s.run("csv.sortFileBy", json!({"columns": "city"})).is_err());
}

#[test]
fn sorting_dates_and_amounts() {
    let mut s = Sheet::new("d,p\n12/31/2025,10%\n2/1/2025,9%\n1/15/2026,100%\n");
    s.at("A2").key("csv.sortFile");
    assert_eq!(s.text(), "d,p\n2/1/2025,9%\n12/31/2025,10%\n1/15/2026,100%\n");
    s.at("B2").ok("csv.sortFile", json!({"reverse": true}));
    assert_eq!(s.text(), "d,p\n1/15/2026,100%\n12/31/2025,10%\n2/1/2025,9%\n");
    let mut s = Sheet::new("d\n31.12.2025\n01.02.2026\n15.01.2026\n");
    s.at("A2").key("csv.sortFile");
    assert_eq!(s.text(), "d\n31.12.2025\n15.01.2026\n01.02.2026\n");
}

/// The rows of the view in its order, by their first field.
fn view_ids(s: &Sheet) -> Vec<String> {
    let t = s.d.text();
    csv::shown_lines(&s.d)
        .unwrap()
        .iter()
        .map(|&l| {
            let line = &t.as_str()[t.line_range(l)];
            line.split(',').next().unwrap_or("").to_string()
        })
        .filter(|f| !f.is_empty())
        .collect()
}

#[test]
fn a_sorted_view_keeps_its_order_through_edits() {
    // Sorted by name: Ada, Bob, Cem, Dan. Bob renamed Zed stays where it
    // shows (it jumped to the end at the keystroke); sorting again moves
    // it.
    let mut s = Sheet::new(SCORES);
    s.at("B2").key("csv.sortView");
    assert_eq!(view_ids(&s), ["id", "4", "2", "3", "1"]);
    s.at("B3").ok("csv.setField", json!({"value": "Zed"}));
    assert_eq!(view_ids(&s), ["id", "4", "2", "3", "1"]);
    // A row inserted shows below the one it was inserted after.
    s.at("B3").key("csv.insertRow");
    assert_eq!(s.cell(), "B4");
    s.ok("csv.setField", json!({"value": "Abe"}));
    s.ok("csv.setField", json!({"value": "5", "column": 0}));
    let ids = view_ids(&s);
    assert_eq!(ids.len(), 6);
    assert_eq!(&ids[..3], ["id", "4", "2"], "{ids:?}");
    // Sorted again: the edits count.
    s.at("B2").ok("csv.sortView", json!({"reverse": false}));
    s.at("B2").ok("csv.sortView", json!({"reverse": false}));
    let t = s.text().to_string();
    let names: Vec<&str> = view_ids(&s)
        .iter()
        .filter_map(|id| {
            t.lines()
                .find(|l| l.starts_with(&format!("{id},")))
                .and_then(|l| l.split(',').nth(1))
        })
        .collect();
    assert_eq!(names, ["name", "Abe", "Ada", "Cem", "Dan", "Zed"]);
}

#[test]
fn a_filter_keeps_a_row_edited_out_of_it() {
    let mut s = Sheet::new(TAGS);
    s.ok("csv.filter", json!({"text": "x"}));
    assert_eq!(view_ids(&s), ["id", "1", "3", "5"]);
    // Cem's tag made `y`: still shown, counted, until filtered again.
    s.at("C4").ok("csv.setField", json!({"value": "y"}));
    s.at("A2");
    assert_eq!(view_ids(&s), ["id", "1", "3", "5"]);
    let status = csv::status(&s.d).unwrap();
    assert!(status.contains("3 of 5"), "{status}");
    s.ok("csv.filter", json!({"text": "x"}));
    assert_eq!(view_ids(&s), ["id", "1", "5"]);
    // A row inserted under a filter stays shown when the cursor leaves.
    s.at("A2").key("csv.insertRow");
    s.ok("csv.setField", json!({"value": "9"}));
    s.at("A1");
    assert_eq!(view_ids(&s), ["id", "1", "9", "5"]);
}

#[test]
fn undo_after_escape_does_not_bring_the_entry_back() {
    let mut s = Sheet::new("a,b,c\n1,2,3\n");
    s.at("A2");
    s.typ("xy");
    assert_eq!(s.text(), "a,b,c\nxy,2,3\n");
    s.key("csv.cancelEdit");
    assert_eq!(s.text(), "a,b,c\n1,2,3\n");
    // Undo has nothing of the entry: neither it nor its cancel.
    let _ = s.d.undo();
    assert_eq!(s.text(), "a,b,c\n1,2,3\n");
    let _ = s.d.redo();
    assert_eq!(s.text(), "a,b,c\n1,2,3\n");
    // Backspace's clearing goes with the entry it starts.
    s.at("B2").key("csv.backspaceCell");
    s.typ("9");
    assert_eq!(s.text(), "a,b,c\n1,9,3\n");
    s.key("csv.cancelEdit");
    assert_eq!(s.text(), "a,b,c\n1,2,3\n");
    let _ = s.d.undo();
    assert_eq!(s.text(), "a,b,c\n1,2,3\n");
    // An entry made before stays undoable.
    s.at("C2");
    s.typ("7");
    s.at("A2");
    s.typ("5");
    s.key("csv.cancelEdit");
    assert_eq!(s.text(), "a,b,c\n1,2,7\n");
    let _ = s.d.undo();
    assert_eq!(s.text(), "a,b,c\n1,2,3\n");
}

#[test]
fn enter_after_tabs_goes_back_to_the_column_they_started_from() {
    // As in Excel: B2, Tab, Tab, Enter is B3 (it was D3).
    let mut s = Sheet::new("a,b,c,d\n1,2,3,4\n5,6,7,8\n");
    s.at("B2");
    s.key("csv.nextField");
    s.typ("x");
    s.key("csv.nextField");
    assert_eq!(s.cell(), "D2");
    s.key("csv.cellBelow");
    assert_eq!(s.cell(), "B3");
    // Another move ends the run: Enter keeps the column.
    s.at("A2").key("csv.nextField").key("csv.cellRight").key("csv.cellBelow");
    assert_eq!(s.cell(), "C3");
}
