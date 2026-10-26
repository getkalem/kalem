//! Typing, pasting and deleting in a CSV document's grid, and where the
//! grid's cursor is in Excel's terms (Ready, Enter and Edit modes, cells
//! past a record's end): [`DocumentState`]'s methods for its CSV mode,
//! over [`super::CsvState`].

use std::time::Instant;

use org_edit::{ChangeKind, Selection, Transaction};

use crate::DocumentMode;
use crate::document::{CellEdit, CellMode, DocumentState};

impl DocumentState {
    /// The column of the CSV cell selected past the end of its record,
    /// while the cursor and the text stay as they were (`csv_virtual`).
    pub fn csv_virtual_col(&self) -> Option<usize> {
        let (head, version, col) = self.csv()?.virtual_cell?;
        (self.selection.head == head && self.version() == version).then_some(col)
    }

    /// The column of the selection's anchor when it is a CSV cell past its
    /// record's end (`csv_virtual_anchor`).
    pub fn csv_virtual_anchor_col(&self) -> Option<usize> {
        let (anchor, version, col) = self.csv()?.virtual_anchor?;
        (self.selection.anchor == anchor
            && self.selection.head != anchor
            && self.version() == version)
            .then_some(col)
    }

    /// Selects the CSV cell at column `col` of the record at the cursor,
    /// past its end: the cursor goes to the record's end.
    pub fn select_csv_virtual(&mut self, col: usize) {
        let Some((_, _, rec, _)) = crate::csv::cell_at(self) else {
            return;
        };
        if col < rec.fields.len() {
            return;
        }
        self.selection = Selection::caret(rec.range.end);
        let version = self.version();
        if let Some(c) = self.csv_mut() {
            c.virtual_cell = Some((rec.range.end, version, col));
        }
    }

    /// The CSV grid's cell being typed into or edited, while the cursor
    /// is in it.
    pub fn csv_editing(&self) -> Option<&CellEdit> {
        let e = self.csv()?.edit.as_ref()?;
        let (_, row, _, col) = crate::csv::cell_at(self)?;
        ((row, col) == (e.row, e.col)).then_some(e)
    }

    /// The CSV grid's mode: Ready, Enter or Edit; `None` outside CSV and
    /// with Vim's keys (`vim_keys`), which edit the cells' text.
    pub fn csv_mode(&self) -> Option<CellMode> {
        if self.meta.mode != DocumentMode::Csv || self.vim_keys {
            return None;
        }
        Some(match self.csv_editing() {
            None => CellMode::Ready,
            Some(e) if e.edit => CellMode::Edit,
            Some(_) => CellMode::Enter,
        })
    }

    /// Ends the cell's entry when the cursor has left it (the entry stays
    /// in the file, as Excel enters it).
    pub fn settle_csv_edit(&mut self) {
        if self.csv().is_some_and(|c| c.edit.is_some())
            && self.csv_editing().is_none()
            && let Some(c) = self.csv_mut()
        {
            c.edit = None;
        }
    }

    /// Starts typing into (`edit` false) or editing the CSV cell at the
    /// cursor, its record kept for Escape.
    pub fn begin_csv_edit(&mut self, edit: bool) {
        let Some((_, row, rec, col)) = crate::csv::cell_at(self) else {
            return;
        };
        let record = self.text().as_str()[rec.range.clone()].to_string();
        // The entry's typing a step of its own.
        self.break_undo_group();
        let steps = self.undo_depth();
        if let Some(c) = self.csv_mut() {
            c.edit = Some(CellEdit {
                row,
                col,
                record,
                edit,
                steps,
            });
        }
    }

    /// Escape in a cell being typed into or edited: its record as it was,
    /// the cursor on the cell. Whether there was one.
    pub fn cancel_csv_edit(&mut self, now: Instant) -> bool {
        let Some(e) = self.csv_editing().cloned() else {
            return false;
        };
        // Cells Enter and Tab move within stay selected.
        let rect = self.csv().and_then(|c| c.rect);
        self.end_csv_entry();
        // The entry's own undo steps undone and not kept for Redo (a
        // "Cancel Entry" step of its own let Undo bring the entry back);
        // with history beyond them, the record put back by an edit.
        if self.undo_depth() > e.steps {
            while self.undo_depth() > e.steps && self.undo().is_some() {}
            self.forget_redo();
        }
        let Some((_, _, rec, _)) = crate::csv::cell_at(self) else {
            return true;
        };
        if self.text().as_str()[rec.range.clone()] != e.record {
            let mut tx = Transaction::new("Cancel Entry");
            tx.edit(rec.range.clone(), e.record.clone());
            let start = rec.range.start;
            self.apply(&tx, ChangeKind::Command, now);
            self.selection = Selection::caret(start);
        }
        self.go_to_csv_cell(e.row, e.col);
        if let Some(c) = self.csv_mut() {
            c.rect = rect;
        }
        true
    }

    /// Puts the cursor on the CSV cell at `row` and `col`: at its value's
    /// start, or past its record's end (selected without a change) when the
    /// record is too short to have it.
    pub fn go_to_csv_cell(&mut self, row: usize, col: usize) {
        if let Some(c) = self.csv_mut() {
            c.rect = None;
        }
        let layout = crate::csv::layout(self);
        let rec = layout
            .index
            .borrow_mut()
            .record(self.text().as_str(), row, &layout.dialect);
        let Some(rec) = rec else { return };
        match rec.fields.get(col) {
            Some(f) => {
                let at = crate::csv::value_range(f).start;
                self.selection = Selection::caret(at);
            }
            None => {
                self.selection = Selection::caret(rec.range.end);
                self.select_csv_virtual(col);
            }
        }
        self.settle_csv_edit();
    }

    /// The column of the CSV cell at the cursor, to keep across a move
    /// up or down (`keep_csv_column`).
    pub fn csv_column(&self) -> Option<usize> {
        crate::csv::cell_at(self).map(|(_, _, _, c)| c)
    }

    /// After a move up or down in a CSV grid from column `col`: on a
    /// record too short to have it, its missing cell in that column rather
    /// than its last field, as a spreadsheet keeps the column.
    pub fn keep_csv_column(&mut self, col: usize) {
        let s = self.selection;
        if s.anchor != s.head {
            return;
        }
        let Some((layout, _, rec, c)) = crate::csv::cell_at(self) else {
            return;
        };
        if c != col && col >= rec.fields.len() && col < layout.widths.len() {
            self.select_csv_virtual(col);
        }
    }

    /// Typing in a CSV file's grid (not its source view): a delimiter, a
    /// quote or a line break goes into the field's value, which is quoted
    /// for it. Whether the text was typed so; else `type_text` types it.
    pub fn type_in_grid(&mut self, text: &str, now: Instant) -> bool {
        if self.meta.mode != DocumentMode::Csv || !self.extra.is_empty() {
            return false;
        }
        // Over a selection: within one cell its text goes; over several
        // cells the text goes into the cell the selection started from,
        // as a spreadsheet types into the active cell (it replaced the
        // text between them, merging cells and rows; then it went into
        // the cell the selection was extended to).
        let s = self.selection;
        if s.anchor != s.head {
            let anchor = crate::csv::anchor_cell(self);
            if anchor == crate::csv::cell_at(self).map(|(_, r, _, c)| (r, c)) {
                self.delete_in_grid(false, now);
            } else if let Some((row, col)) = anchor {
                self.selection = Selection::caret(s.anchor);
                self.go_to_csv_cell(row, col);
            }
        }
        let Some((layout, _, rec, col)) = crate::csv::cell_at(self) else {
            return false;
        };
        // Ready mode: the text replaces the cell (past the record's end,
        // the fields up to it first), and typing into it goes on, as in
        // Excel; a new entry, a new undo step (the entries of a row made
        // with Tab were one). Vim's insert mode inserts (it replaced the
        // cell). A cell past the record's end gets the fields up to it in
        // every mode (after F2 the text went into the field before).
        let entry = self.csv_editing().cloned();
        let past = col >= rec.fields.len();
        if (entry.is_none() && !self.vim_keys) || past {
            let record = match &entry {
                Some(e) => e.record.clone(),
                None => self.text().as_str()[rec.range.clone()].to_string(),
            };
            let tx = crate::csv::set_cell(self.text().as_str(), &rec, col, text, &layout.dialect);
            if let Some(c) = self.csv_mut() {
                c.virtual_cell = None;
            }
            if entry.is_none() {
                self.break_undo_group();
            }
            let steps = entry
                .as_ref()
                .map_or_else(|| self.undo_depth(), |e| e.steps);
            self.apply(&tx, ChangeKind::Typing, now);
            if let Some((_, row, _, col)) = crate::csv::cell_at(self)
                && let Some(c) = self.csv_mut()
            {
                c.edit = Some(CellEdit {
                    row,
                    col,
                    record,
                    edit: entry.is_some_and(|e| e.edit),
                    steps,
                });
            }
            return true;
        }
        // Right before or after a quoted field's quotes: inside them, so
        // that the field stays one (`"x"Q` is not CSV).
        let mut pos = self.selection.head;
        if let Some(f) = rec
            .fields
            .get(col)
            .filter(|f| f.quoted && f.range.len() >= 2)
        {
            if pos == f.range.start {
                pos += 1;
            } else if pos == f.range.end {
                pos -= 1;
            }
        }
        let Some(tx) = crate::csv::typed(self.text().as_str(), &rec, pos, text, &layout.dialect)
            .or_else(|| {
                (pos != self.selection.head).then(|| {
                    let mut tx = Transaction::new("Typing");
                    tx.edit(pos..pos, text);
                    tx.select(Selection::caret(pos + text.len()))
                })
            })
        else {
            return false;
        };
        self.apply(&tx, ChangeKind::Typing, now);
        true
    }

    /// A paste into a CSV file's grid (not its source view), as a
    /// spreadsheet pastes: lines or tab-separated values are written over
    /// the cells from the cursor's (the selection's top left) down and to
    /// the right, rows added past the end; a single value goes into the
    /// cell, quoted when it needs it. Whether it was pasted so.
    pub fn paste_in_grid(&mut self, text: &str, now: Instant) -> bool {
        if self.meta.mode != DocumentMode::Csv || !self.extra.is_empty() {
            return false;
        }
        let text = crate::paste::normalize_line_breaks(text);
        let text = text.strip_suffix('\n').unwrap_or(&text);
        if text.is_empty() {
            return false;
        }
        let Some((layout, row, rec, col)) = crate::csv::cell_at(self) else {
            return false;
        };
        // Paste as Block asked for: this is it.
        if let Some(c) = self.csv_mut() {
            c.paste_block = false;
        }
        let s = self.selection;
        let editing = self.csv_editing().is_some();
        // Typing into a cell (Enter or Edit mode): into it at the caret,
        // line breaks and tabs too, as a spreadsheet pastes into a cell
        // being edited (rows of it overwrote the cells below).
        if s.anchor == s.head && editing {
            if !self.type_in_grid(text, now) {
                self.insert_text(text, now);
            }
            return true;
        }
        let (row0, col0) = if s.anchor == s.head {
            (row, col)
        } else {
            let Some(cell) = crate::csv::anchor_cell(self) else {
                return false;
            };
            cell
        };
        // The selection's top row as the grid shows it.
        let top = crate::csv::rows_between(self, row0, row)
            .first()
            .copied()
            .unwrap_or(row0.min(row));
        let left = col0.min(col);
        if text.contains(['\n', '\t']) {
            let block = crate::csv_tools::block_rows(text, &layout.dialect);
            let rows = crate::csv::view_rows_from(self, top, block.len());
            let Some(tx) = crate::csv_tools::paste_block(
                self.text().as_str(),
                &layout.dialect,
                &rows,
                left,
                &block,
            ) else {
                return false;
            };
            self.apply(&tx, ChangeKind::Command, now);
            // The cursor at the block's first cell.
            let layout = crate::csv::layout(self);
            let first = layout
                .index
                .borrow_mut()
                .record(self.text().as_str(), top, &layout.dialect)
                .and_then(|r| r.fields.get(left).map(|f| f.range.start));
            if let Some(at) = first {
                self.selection = Selection::caret(at);
            }
            return true;
        }
        // One value: into the cell, the selection's text in it replaced,
        // a selection over cells collapsed to its first.
        if s.anchor != s.head {
            if (row0, col0) == (row, col) {
                self.delete_in_grid(false, now);
            } else if let Some(f) = crate::csv::layout(self)
                .index
                .borrow_mut()
                .record(self.text().as_str(), top, &layout.dialect)
                .and_then(|r| r.fields.get(left).cloned())
            {
                self.selection = Selection::caret(f.range.end);
            } else {
                self.selection = Selection::caret(rec.range.end);
            }
        }
        if !self.type_in_grid(text, now) {
            self.insert_text(text, now);
        }
        // The cell pasted into stays selected, as in a spreadsheet: Escape
        // no longer takes the paste back, and the next typing replaces it.
        if !editing {
            self.end_csv_entry();
            self.break_undo_group();
        }
        true
    }

    /// Backspace (`forward` false) or Delete in a CSV file's grid (not
    /// its source view): within a cell's value, never its delimiters or
    /// quotes; a selection over more than one cell clears the cells
    /// (rather than merging them, and the rows). Whether the key was
    /// taken so; else `delete_backward` or `delete_forward` deletes.
    pub fn delete_in_grid(&mut self, forward: bool, now: Instant) -> bool {
        if self.meta.mode != DocumentMode::Csv || !self.extra.is_empty() {
            return false;
        }
        let Some((layout, row, rec, col)) = crate::csv::cell_at(self) else {
            return false;
        };
        let s = self.selection;
        if s.anchor != s.head {
            let Some((row0, col0)) = crate::csv::anchor_cell(self) else {
                return false;
            };
            if (row0, col0) == (row, col) {
                // Within one cell: its value only.
                let Some(f) = rec.fields.get(col) else {
                    return false;
                };
                let (a, b) = if f.quoted && f.range.len() >= 2 {
                    (f.range.start + 1, f.range.end - 1)
                } else {
                    (f.range.start, f.range.end)
                };
                let (lo, hi) = (s.anchor.min(s.head).max(a), s.anchor.max(s.head).min(b));
                if lo < hi {
                    let mut tx = Transaction::new("Delete");
                    tx.edit(lo..hi, "");
                    let tx = tx.select(Selection::caret(lo));
                    self.apply(&tx, ChangeKind::Typing, now);
                } else {
                    self.selection = Selection::caret(s.head);
                }
                return true;
            }
            // The cells the grid shows selected (with a filter or a sorted
            // view on, not the file's rows between the corners).
            let rows = crate::csv::rows_between(self, row0, row);
            let cols = (col0.min(col), col0.max(col));
            if let Some(tx) = crate::csv::clear_cells(self.text().as_str(), &layout, &rows, cols) {
                self.apply(&tx, ChangeKind::Command, now);
            }
            return true;
        }
        if let Some(tx) = crate::csv::deleted(
            self.text().as_str(),
            &rec,
            self.selection.head,
            forward,
            &layout.dialect,
        ) {
            self.apply(&tx, ChangeKind::Typing, now);
            self.unquote_entry(now);
        }
        true
    }

    /// After a deletion in the cell being typed into or edited: quotes
    /// the entry put around its value (for a delimiter, a quote or a blank
    /// at an end, typed) go when the value no longer needs them and the
    /// field had none before the entry (a space typed and deleted left
    /// `""` in the file).
    fn unquote_entry(&mut self, now: Instant) {
        let Some(e) = self.csv_editing().cloned() else {
            return;
        };
        let Some((layout, _, rec, col)) = crate::csv::cell_at(self) else {
            return;
        };
        let d = &layout.dialect;
        let Some(f) = rec.fields.get(col).filter(|f| f.quoted) else {
            return;
        };
        if crate::csv::scan(&e.record, 0, d)
            .fields
            .get(col)
            .is_some_and(|f| f.quoted)
        {
            return;
        }
        let text = self.text().as_str();
        let v = crate::csv::value(text, f, d).into_owned();
        if crate::csv::encode(&v, d) != v || text[f.range.clone()].len() != v.len() + 2 {
            return;
        }
        // The caret where it was in the value, the opening quote gone.
        let head = self.selection.head;
        let caret = head.saturating_sub(f.range.start + 1).min(v.len()) + f.range.start;
        let mut tx = Transaction::new("Typing");
        tx.edit(f.range.clone(), v);
        let tx = tx.select(Selection::caret(caret));
        self.apply(&tx, ChangeKind::Typing, now);
    }
}
