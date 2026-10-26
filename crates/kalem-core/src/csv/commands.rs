//! CSV mode's commands (T2.7d): the grid's keys as a spreadsheet's (Ready,
//! Enter and Edit modes, Tab runs, cells Enter and Tab move within), its
//! view (filter, sort, columns, the spreadsheet look), its dialect, and
//! the edits of rows, columns and cells. Registered with the other
//! built-in commands (`crate::builtin::commands`).

use serde_json::Value;

use crate::command::{
    Command, CommandError, CommandResult, EditorContext, Handler, Request, arg_bool, arg_str, cmd,
    request, scoped,
};

/// Where the cursor goes in field `f`: its first character, inside the
/// quotes of a quoted one.
fn csv_caret(f: &crate::csv::Field) -> usize {
    f.range.start + usize::from(f.quoted)
}

/// Runs a CSV edit on the cell at the cursor: `f` gets the text, the
/// layout, the row, its record and the column, and gives the change and
/// the cell (row, column) the cursor goes to after it.
/// The CSV cell at the cursor, or the not-CSV error.
fn csv_cell(
    d: &crate::DocumentState,
) -> Result<
    (
        std::rc::Rc<crate::csv::Layout>,
        usize,
        crate::csv::Record,
        usize,
    ),
    CommandError,
> {
    crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))
}

/// In Edit mode, the cursor moved within the cell's value to the stop
/// `to` picks (given the value's stops and the cursor); whether it was.
fn csv_in_cell(
    d: &mut crate::DocumentState,
    to: impl FnOnce(&[usize], usize) -> Option<usize>,
) -> bool {
    csv_in_cell_extending(d, false, to)
}

/// [`csv_in_cell`], with `extend` the selection's end moved (Shift with
/// Left and Right selected the whole cell in Edit mode).
fn csv_in_cell_extending(
    d: &mut crate::DocumentState,
    extend: bool,
    to: impl FnOnce(&[usize], usize) -> Option<usize>,
) -> bool {
    if !d.csv_editing().is_some_and(|e| e.edit) {
        return false;
    }
    let Some((layout, _, rec, col)) = crate::csv::cell_at(d) else {
        return false;
    };
    let Some(f) = rec.fields.get(col) else {
        return true;
    };
    let stops = crate::csv::value_stops(d.text().as_str(), f, &layout.dialect);
    let at = d.selection.head;
    if let Some(p) = to(&stops, at) {
        d.selection = if extend {
            org_edit::Selection {
                anchor: d.selection.anchor,
                head: p,
            }
        } else {
            org_edit::Selection::caret(p)
        };
    }
    true
}

/// Enter, Shift+Enter, Tab or Shift+Tab with cells of several rows or
/// columns selected: the active cell (the cursor's; the anchor's for a
/// selection just made) goes on within them, down each column for Enter
/// (`along_rows` false) or along each row for Tab, back with `back`,
/// wrapping at the ends, as Excel moves in a selected range; the cells
/// stay selected (`DocumentState::csv_rect`) and typing goes into the
/// active cell. `None` without such cells (they left the selection).
/// The state of the CSV document a command runs on ([`crate::csv::CsvState`]).
fn csv_state(d: &mut crate::DocumentState) -> Result<&mut crate::csv::CsvState, CommandError> {
    d.csv_mut()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))
}

/// The columns a CSV document's grid hides; none outside CSV.
fn csv_hidden(d: &crate::DocumentState) -> std::collections::BTreeSet<usize> {
    d.csv()
        .map(|s| s.columns.hidden.clone())
        .unwrap_or_default()
}

fn csv_within(ctx: &mut EditorContext<'_>, along_rows: bool, back: bool) -> Option<CommandResult> {
    let d = ctx.doc().ok()?;
    let rect = crate::csv::cell_rectangle(d)?;
    let (row, col) = if d.selection.anchor != d.selection.head {
        crate::csv::anchor_cell(d)?
    } else {
        crate::csv::cell_at(d).map(|(_, r, _, c)| (r, c))?
    };
    let cols: Vec<usize> = (rect.cols.0..=rect.cols.1)
        .filter(|c| !d.csv().is_some_and(|s| s.columns.hidden.contains(c)))
        .collect();
    let (nr, nc) = (rect.rows.len(), cols.len());
    if nr == 0 || nc == 0 {
        return None;
    }
    let ri = rect.rows.iter().position(|&r| r == row).unwrap_or(0);
    let ci = cols.iter().position(|&c| c == col).unwrap_or(0);
    let total = nr * nc;
    let at = if along_rows {
        ri * nc + ci
    } else {
        ci * nr + ri
    };
    let to = if back {
        (at + total - 1) % total
    } else {
        (at + 1) % total
    };
    let (r, c) = if along_rows {
        (rect.rows[to / nc], cols[to % nc])
    } else {
        (rect.rows[to % nr], cols[to / nr])
    };
    let corners = (
        (rect.rows[0], rect.cols.0),
        (rect.rows[nr - 1], rect.cols.1),
    );
    // The entry in the active cell stays, as Enter enters it.
    if let Some(s) = d.csv_mut() {
        s.edit = None;
        s.tab_run = None;
    }
    d.go_to_csv_cell(r, c);
    if let Some(s) = d.csv_mut() {
        s.rect = Some(corners);
    }
    Some(Ok(()))
}

/// Tab or Shift+Tab (`step`) as part of a run along a row: the column the
/// run started from is kept for Enter to go back to on the row below
/// (`DocumentState::csv_tab_run`); a step to another row ends it.
fn csv_tab(
    ctx: &mut EditorContext<'_>,
    step: impl FnOnce(&mut EditorContext<'_>) -> CommandResult,
) -> CommandResult {
    let d = ctx.doc()?;
    let here = crate::csv::cell_at(d).map(|(_, r, _, c)| (r, c));
    let start = match (d.csv().and_then(|s| s.tab_run), here) {
        (Some((r, c, s)), Some(h)) if (r, c) == h => Some(s),
        _ => here.map(|(_, c)| c),
    };
    step(ctx)?;
    let d = ctx.doc()?;
    let there = crate::csv::cell_at(d).map(|(_, r, _, c)| (r, c));
    csv_state(d)?.tab_run = match (here, there, start) {
        (Some((r0, _)), Some((r1, c1)), Some(s)) if r0 == r1 => Some((r1, c1, s)),
        _ => None,
    };
    Ok(())
}

/// An arrow in the grid: in Edit mode Left and Right within the cell;
/// else the cell `rows` down and `cols` right (the entry, in Enter mode,
/// ended), or with `extend` the selection's corner moved there.
fn csv_step(ctx: &mut EditorContext<'_>, rows: isize, cols: isize, extend: bool) -> CommandResult {
    let d = ctx.doc()?;
    if rows == 0
        && csv_in_cell_extending(d, extend, |stops, at| {
            let i = stops.iter().position(|s| *s >= at).unwrap_or(stops.len());
            if cols < 0 {
                i.checked_sub(1).and_then(|i| stops.get(i)).copied()
            } else {
                stops.iter().copied().find(|s| *s > at)
            }
        })
    {
        return Ok(());
    }
    let (layout, row, rec, col) = csv_cell(d)?;
    csv_state(d)?.tab_run = None;
    // Up and down in the order the view shows the records (its filter and
    // sort); at either end, where it is.
    let to_row = if rows == 0 {
        row
    } else {
        crate::csv::view_step(d, row, rows).unwrap_or(row)
    };
    let to_col = if cols == 0 {
        col
    } else {
        csv_shown_column(d, col, cols, layout.widths.len().max(rec.fields.len()))
    };
    d.end_csv_entry();
    if extend {
        // The selection's corner on the target cell; past a short
        // record's end, that column's missing cell (it was the record's
        // last field, and the rectangle reached column A). An anchor on
        // such a cell keeps its column too.
        let anchor = d.selection.anchor;
        if anchor == d.selection.head {
            let version = d.version();
            let kept = d.csv_virtual_col().map(|c| (anchor, version, c));
            csv_state(d)?.virtual_anchor = kept;
        }
        let rec = layout
            .index
            .borrow_mut()
            .record(d.text().as_str(), to_row, &layout.dialect);
        let Some(rec) = rec else {
            return Ok(());
        };
        match rec.fields.get(to_col) {
            Some(f) => {
                let at = crate::csv::value_range(f).start;
                d.selection = org_edit::Selection { anchor, head: at };
            }
            None => {
                let at = rec.range.end;
                d.selection = org_edit::Selection { anchor, head: at };
                let version = d.version();
                csv_state(d)?.virtual_cell = Some((at, version, to_col));
            }
        }
        return Ok(());
    }
    d.go_to_csv_cell(to_row, to_col);
    Ok(())
}

/// The column `step` columns (one, either way) from `col` that the grid
/// shows: hidden ones skipped (typing went into a cell out of sight), at
/// most `last`, the first empty column right of the data (past it the
/// terminal drew no cell); `col` when there is none.
fn csv_shown_column(d: &crate::DocumentState, col: usize, step: isize, last: usize) -> usize {
    let hidden = csv_hidden(d);
    let mut to = col as isize;
    loop {
        to += step.signum();
        if to < 0 || to as usize > last {
            return col;
        }
        if !hidden.contains(&(to as usize)) {
            return to as usize;
        }
    }
}

/// Ctrl with an arrow: to the edge of the data, as Excel goes: from a
/// filled cell with a filled one next, to the last filled one before a
/// blank; else to the next filled one; else to the sheet's edge.
fn csv_edge(ctx: &mut EditorContext<'_>, rows: isize, cols: isize) -> CommandResult {
    let d = ctx.doc()?;
    if csv_in_cell(d, |stops, _| {
        if cols < 0 || rows < 0 {
            stops.first().copied()
        } else {
            stops.last().copied()
        }
    }) {
        return Ok(());
    }
    let (layout, row, _, col) = csv_cell(d)?;
    let text = d.text().as_str();
    let dl = &layout.dialect;
    let n = layout.index.borrow_mut().count(text, dl);
    let last_col = layout.widths.len().saturating_sub(1).max(col);
    let filled = |r: usize, c: usize| -> bool {
        layout
            .index
            .borrow_mut()
            .record(text, r, dl)
            .and_then(|rec| {
                rec.fields
                    .get(c)
                    .map(|f| !crate::csv::value(text, f, dl).is_empty())
            })
            .unwrap_or(false)
    };
    let next = |(r, c): (usize, usize)| -> Option<(usize, usize)> {
        let r2 = r as isize + rows;
        let c2 = c as isize + cols;
        (r2 >= 0 && c2 >= 0 && (r2 as usize) < n && (c2 as usize) <= last_col)
            .then_some((r2 as usize, c2 as usize))
    };
    let mut at = (row, col);
    match next(at) {
        None => {}
        Some(first) if filled(at.0, at.1) && filled(first.0, first.1) => {
            at = first;
            while let Some(p) = next(at).filter(|p| filled(p.0, p.1)) {
                at = p;
            }
        }
        Some(first) => {
            at = first;
            while !filled(at.0, at.1) {
                match next(at) {
                    Some(p) => at = p,
                    None => break,
                }
            }
        }
    }
    d.end_csv_entry();
    d.go_to_csv_cell(at.0, at.1);
    Ok(())
}

/// Empties the selected cells, or the cursor's (Delete in Ready mode).
fn csv_clear_selection(d: &mut crate::DocumentState, now: std::time::Instant) -> CommandResult {
    let s = d.selection;
    if s.anchor != s.head {
        d.delete_in_grid(true, now);
        return Ok(());
    }
    // Cells Enter and Tab move within: all of them cleared, and kept.
    if let Some(rect) = crate::csv::cell_rectangle(d) {
        let kept = d.csv().and_then(|s| s.rect);
        let (layout, row, _, col) = csv_cell(d)?;
        if let Some(tx) = crate::csv::clear_cells(d.text().as_str(), &layout, &rect.rows, rect.cols)
            .filter(|tx| csv_changes(d.text().as_str(), tx))
        {
            d.apply(&tx, org_edit::ChangeKind::Command, now);
        }
        d.go_to_csv_cell(row, col);
        csv_state(d)?.rect = kept;
        return Ok(());
    }
    let (layout, row, rec, col) = csv_cell(d)?;
    if rec.fields.get(col).is_some_and(|f| !f.range.is_empty()) {
        let tx = crate::csv::set_cell(d.text().as_str(), &rec, col, "", &layout.dialect);
        d.apply(&tx, org_edit::ChangeKind::Command, now);
    }
    d.go_to_csv_cell(row, col);
    Ok(())
}

/// The records before and after the cursor's in the order the view shows
/// them (its filter and sort), which Enter, Tab and the arrows step to.
fn csv_neighbours(ctx: &mut EditorContext<'_>) -> (Option<usize>, Option<usize>) {
    let Ok(d) = ctx.doc() else {
        return (None, None);
    };
    match crate::csv::cell_at(d) {
        Some((_, row, _, _)) => (
            crate::csv::view_step(d, row, -1),
            crate::csv::view_step(d, row, 1),
        ),
        None => (None, None),
    }
}

/// The rows shown above and below the cursor's, for Move Row Up and Down
/// (they swapped with the file's next record, hidden by a filter);
/// refused in a sorted view, which has no order of the file's to move in.
fn csv_row_neighbours(
    ctx: &mut EditorContext<'_>,
) -> Result<(Option<usize>, Option<usize>), CommandError> {
    let d = ctx.doc()?;
    if d.csv().is_some_and(|s| s.sort.is_some()) {
        return Err(CommandError::new(crate::tr!("msg-csv-sorted-view")));
    }
    Ok(csv_neighbours(ctx))
}

/// Where Enter or Tab past a record's end goes: the next record the view
/// shows; past the last, a new record when the cursor's is the file's
/// last (as Tab in an Org table), else nowhere (the ones after are
/// hidden).
fn csv_down(
    text: &str,
    l: &crate::csv::Layout,
    row: usize,
    rec: &crate::csv::Record,
    next: Option<usize>,
    col: usize,
) -> (Option<org_edit::Transaction>, Option<(usize, usize)>) {
    if let Some(r) = next {
        return (None, Some((r, col)));
    }
    let n = l.index.borrow_mut().count(text, &l.dialect);
    if row + 1 < n {
        return (None, None);
    }
    let columns = l.widths.len().max(rec.fields.len());
    (
        Some(crate::csv::insert_row(text, rec, columns, &l.dialect)),
        Some((row + 1, col)),
    )
}

fn csv_edit(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(
        &str,
        &crate::csv::Layout,
        usize,
        &crate::csv::Record,
        usize,
    ) -> Result<(Option<org_edit::Transaction>, Option<(usize, usize)>), CommandError>,
) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    let (layout, row, rec, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let (tx, to) = f(d.text().as_str(), &layout, row, &rec, col)?;
    if let Some(tx) = tx.filter(|tx| csv_changes(d.text().as_str(), tx)) {
        d.apply(&tx, org_edit::ChangeKind::Command, now);
    }
    if let Some((row, col)) = to {
        let layout = crate::csv::layout(d);
        let text = d.text().as_str();
        let r = layout.index.borrow_mut().record(text, row, &layout.dialect);
        if let Some(r) = r {
            // A column past the record's end (and on the grid): its
            // missing cell, selected without a change to the file.
            let columns = layout.widths.len();
            if col >= r.fields.len() && col < columns {
                d.selection = org_edit::Selection::caret(r.range.end);
                d.select_csv_virtual(col);
                d.settle_csv_edit();
                return Ok(());
            }
            let at = r
                .fields
                .get(col)
                .or(r.fields.last())
                .map_or(r.range.start, csv_caret);
            d.selection = org_edit::Selection::caret(at);
            d.settle_csv_edit();
        }
    }
    Ok(())
}

/// A preview of a CSV edit before it is made: the first rows as they
/// would be, each a choice that makes the edit (`command` again with
/// `confirmed`).
fn csv_preview(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Value,
    edit: impl FnOnce(&str, &crate::csv::Layout, usize) -> Result<org_edit::Transaction, CommandError>,
) -> CommandResult {
    let d = ctx.doc()?;
    let (layout, _, _, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let text = d.text().as_str();
    let tx = edit(text, &layout, col)?;
    let after = tx.apply(text);
    let rows = crate::csv::rows(&after, &layout.dialect);
    let mut confirmed = args.clone();
    confirmed["confirmed"] = Value::Bool(true);
    let id = crate::palette::invocation(command, &confirmed);
    let category = crate::tr!("category-preview");
    let items = rows
        .iter()
        .take(8)
        .map(|r| crate::palette::PaletteItem {
            id: id.clone(),
            title: r.join("  │  "),
            category: category.clone(),
            detail: String::new(),
            keys: String::new(),
            also: String::new(),
        })
        .collect();
    request(ctx, Request::Choose(items))
}

/// The optional `column` argument (counted from 0) of the commands on a
/// column's width.
fn arg_column(args: &Value) -> Option<usize> {
    args.get("column")
        .and_then(Value::as_u64)
        .and_then(|c| usize::try_from(c).ok())
}

/// Changes the columns the CSV grid shows and their widths, given the
/// layout and the column at the cursor.
fn csv_columns(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(&mut crate::csv::Columns, &crate::csv::Layout, usize) -> CommandResult,
) -> CommandResult {
    let d = ctx.doc()?;
    let (layout, _, _, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let mut cols = csv_state(d)?.columns.clone();
    f(&mut cols, &layout, col)?;
    csv_state(d)?.columns = cols;
    Ok(())
}

/// The width column `col` is drawn at: in the spreadsheet look at least
/// its letters and [`crate::csv::SHEET_MIN_WIDTH`] (Widen Column made a
/// narrow column narrower there, from its values' width).
fn csv_drawn_width(l: &crate::csv::Layout, col: usize) -> usize {
    if l.view.sheet {
        crate::csv::sheet_widths(l).get(col).copied().unwrap_or(1)
    } else {
        l.widths.get(col).copied().unwrap_or(1)
    }
}

/// The cursor out of a hidden column: to the next one that shows, else
/// the one before.
fn csv_to_shown_column(ctx: &mut EditorContext<'_>) -> CommandResult {
    let d = ctx.doc()?;
    let Some((_, _, rec, col)) = crate::csv::cell_at(d) else {
        return Ok(());
    };
    let hidden = csv_hidden(d);
    let to = (col..rec.fields.len())
        .find(|j| !hidden.contains(j))
        .or_else(|| (0..col).rev().find(|j| !hidden.contains(j)));
    if let Some(f) = to.and_then(|j| rec.fields.get(j)) {
        d.selection = org_edit::Selection::caret(csv_caret(f));
    }
    Ok(())
}

/// Changes how the CSV grid shows the document.
fn csv_view(ctx: &mut EditorContext<'_>, f: impl FnOnce(&mut crate::csv::View)) -> CommandResult {
    let d = ctx.doc()?;
    if d.meta.mode != crate::DocumentMode::Csv {
        return Err(CommandError::new(crate::tr!("msg-not-csv")));
    }
    f(&mut csv_state(d)?.view);
    Ok(())
}

/// Column `j` after columns `a` and `a + 1` swap places.
fn swapped(j: usize, a: usize) -> usize {
    match j {
        j if j == a => a + 1,
        j if j == a + 1 => a,
        j => j,
    }
}

/// Whether transaction `tx` changes `text` (a command that changes
/// nothing, such as sorting a sorted file, left the document modified
/// with an undo step that did nothing).
fn csv_changes(text: &str, tx: &org_edit::Transaction) -> bool {
    tx.edits
        .iter()
        .any(|e| text.get(e.range.clone()) != Some(e.insert.as_str()))
}

/// The rows and columns of the selection's rectangle of cells, or the
/// cursor's cell.
fn rectangle_or_cell(d: &crate::DocumentState) -> Result<crate::csv::Rectangle, CommandError> {
    if let Some(r) = crate::csv::cell_rectangle(d) {
        return Ok(r);
    }
    // The cursor's cell, past its record's end too (the anchor's offset
    // was that record's last field: Delete Column there took every
    // column from A).
    let (_, row, _, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    Ok(crate::csv::Rectangle {
        rows: vec![row],
        cols: (col, col),
    })
}

/// The selection's cells as TSV, recorded on the clipboard, with a message
/// of how many.
fn csv_copy_cells(ctx: &mut EditorContext<'_>) -> Result<String, CommandError> {
    let d = ctx.doc()?;
    let crate::csv::Rectangle {
        rows,
        cols: (c0, c1),
    } = rectangle_or_cell(d)?;
    let layout = crate::csv::layout(d);
    let tsv = crate::csv_tools::rectangle_tsv(d.text().as_str(), &layout.dialect, &rows, (c0, c1));
    ctx.messages.push(crate::tr!(
        "msg-csv-copied-cells",
        rows = rows.len(),
        columns = c1 - c0 + 1
    ));
    ctx.clipboard.record(tsv.clone());
    Ok(tsv)
}

/// Fill Down and Fill Series in the column at the cursor: the rows the
/// selection covers from its first one, or without a selection the cell
/// from the one above (a series stepping as the two above do); rows as
/// the grid shows them, a filter's and a sorted view's (it filled the
/// file's rows between, hidden ones too, and took the file's row above).
fn csv_fill(ctx: &mut EditorContext<'_>, series: bool) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    let (layout, row, _, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let tx = {
        let text = d.text().as_str();
        let dl = &layout.dialect;
        let top = usize::from(dl.header);
        let cell = |r: usize| -> Option<String> {
            let rec = layout.index.borrow_mut().record(text, r, dl)?;
            rec.fields
                .get(col)
                .map(|f| crate::csv::value(text, f, dl).into_owned())
        };
        let selected = crate::csv::cell_rectangle(d);
        // The columns the selection covers, else the cursor's; its rows,
        // else the cursor's and the one shown above.
        let (cols, rows) = match &selected {
            Some(r) => (r.cols, r.rows.clone()),
            None => {
                let above = crate::csv::view_step(d, row, -1)
                    .filter(|&r| r >= top)
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-nothing-above")))?;
                ((col, col), vec![above, row])
            }
        };
        if rows.len() < 2 || rows[0] < top {
            return Err(CommandError::new(crate::tr!("msg-csv-nothing-above")));
        }
        let step = series.then(|| {
            if selected.is_some() {
                return 1.0;
            }
            let above2 = crate::csv::view_step(d, rows[0], -1)
                .filter(|&r| r >= top)
                .and_then(cell);
            let above = cell(rows[0]).unwrap_or_default();
            crate::csv_tools::series_step(above2.as_deref(), &above, dl.delimiter == b';')
        });
        crate::csv_tools::fill(text, dl, cols, &rows, step)
            .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-series")))?
    };
    if csv_changes(d.text().as_str(), &tx) {
        d.apply(&tx, org_edit::ChangeKind::Command, now);
    }
    Ok(())
}

pub(crate) fn csv_commands() -> Vec<Command> {
    use crate::command::Scope;
    let csv = || Scope::only(&["csv"]);
    let c = |id: &str, title: &str, keys: &[&str], h: Handler| {
        scoped(cmd(id, title, "CSV", keys, None, h), csv())
    };
    vec![
        c("csv.nextField", "Next Field", &["tab"], |ctx, _| {
            if let Some(r) = csv_within(ctx, true, false) {
                return r;
            }
            csv_tab(ctx, |ctx| {
                let (_, next) = csv_neighbours(ctx);
                let hidden = csv_hidden(ctx.doc()?);
                csv_edit(ctx, |text, l, row, rec, col| {
                    // The next column that shows (Tab went into hidden
                    // ones).
                    let last = rec.fields.len().max(l.widths.len());
                    if let Some(c) = (col + 1..last).find(|c| !hidden.contains(c)) {
                        return Ok((None, Some((row, c))));
                    }
                    // Past the last field: the next record shown, or a new
                    // one, as Tab in an Org table.
                    let first = (0..last).find(|c| !hidden.contains(c)).unwrap_or(0);
                    Ok(csv_down(text, l, row, rec, next, first))
                })
            })
        }),
        // Excel's keys in the grid (bound with `!sourceView` in the
        // keymap): in Ready mode the arrows go from cell to cell, Ctrl
        // with them to the data's edge and Shift extends the selection; in
        // Edit mode (F2) Left, Right, Home and End move in the cell; in
        // Enter mode (typing into a cell) the arrows end it and move.
        c("csv.cellLeft", "Cell Left", &[], |ctx, _| {
            csv_step(ctx, 0, -1, false)
        }),
        c("csv.cellRight", "Cell Right", &[], |ctx, _| {
            csv_step(ctx, 0, 1, false)
        }),
        c("csv.extendLeft", "Extend Selection Left", &[], |ctx, _| {
            csv_step(ctx, 0, -1, true)
        }),
        c(
            "csv.extendRight",
            "Extend Selection Right",
            &[],
            |ctx, _| csv_step(ctx, 0, 1, true),
        ),
        c("csv.extendUp", "Extend Selection Up", &[], |ctx, _| {
            csv_step(ctx, -1, 0, true)
        }),
        c("csv.extendDown", "Extend Selection Down", &[], |ctx, _| {
            csv_step(ctx, 1, 0, true)
        }),
        c("csv.edgeLeft", "Data Edge Left", &[], |ctx, _| {
            csv_edge(ctx, 0, -1)
        }),
        c("csv.edgeRight", "Data Edge Right", &[], |ctx, _| {
            csv_edge(ctx, 0, 1)
        }),
        c("csv.edgeUp", "Data Edge Up", &[], |ctx, _| {
            csv_edge(ctx, -1, 0)
        }),
        c("csv.edgeDown", "Data Edge Down", &[], |ctx, _| {
            csv_edge(ctx, 1, 0)
        }),
        c("csv.rowStart", "Row Start", &[], |ctx, _| {
            let d = ctx.doc()?;
            if csv_in_cell(d, |stops, _| stops.first().copied()) {
                return Ok(());
            }
            let (_, row, _, _) = csv_cell(d)?;
            d.end_csv_entry();
            d.go_to_csv_cell(row, 0);
            Ok(())
        }),
        c("csv.rowEnd", "Row End", &[], |ctx, _| {
            let d = ctx.doc()?;
            if csv_in_cell(d, |stops, _| stops.last().copied()) {
                return Ok(());
            }
            let (_, row, rec, _) = csv_cell(d)?;
            d.end_csv_entry();
            d.go_to_csv_cell(row, rec.fields.len().saturating_sub(1));
            Ok(())
        }),
        c("csv.firstCell", "First Cell", &[], |ctx, _| {
            let d = ctx.doc()?;
            // In Edit mode, the start of the cell's text, as in Excel.
            if csv_in_cell(d, |stops, _| stops.first().copied()) {
                return Ok(());
            }
            d.end_csv_entry();
            d.go_to_csv_cell(0, 0);
            Ok(())
        }),
        c("csv.lastCell", "Last Cell", &[], |ctx, _| {
            // The last row's cell in the last column, as Excel's Ctrl+End;
            // in Edit mode, the end of the cell's text.
            let d = ctx.doc()?;
            if csv_in_cell(d, |stops, _| stops.last().copied()) {
                return Ok(());
            }
            let (layout, _, _, _) = csv_cell(d)?;
            let text = d.text().as_str();
            let n = layout.index.borrow_mut().count(text, &layout.dialect);
            let col = layout.widths.len().saturating_sub(1);
            d.end_csv_entry();
            d.go_to_csv_cell(n.saturating_sub(1), col);
            Ok(())
        }),
        c("csv.editCell", "Edit Cell", &[], |ctx, args| {
            // F2: Edit mode, the cursor at the value's end (where it is,
            // from a double click, with `here`); in Enter mode, Edit mode.
            let d = ctx.doc()?;
            let here = args.get("here").and_then(Value::as_bool).unwrap_or(false);
            let (layout, _, rec, col) = csv_cell(d)?;
            if d.csv_editing().is_some() {
                if let Some(e) = csv_state(d)?.edit.as_mut() {
                    e.edit = true;
                }
                return Ok(());
            }
            if let Some(f) = rec.fields.get(col) {
                let stops = crate::csv::value_stops(d.text().as_str(), f, &layout.dialect);
                let at = d.selection.head;
                let to = if here && stops.contains(&at) {
                    at
                } else if here {
                    stops
                        .iter()
                        .copied()
                        .min_by_key(|s| s.abs_diff(at))
                        .unwrap_or(at)
                } else {
                    stops.last().copied().unwrap_or(at)
                };
                d.selection = org_edit::Selection::caret(to);
            }
            d.begin_csv_edit(true);
            Ok(())
        }),
        c("csv.lineBreak", "Line Break in Cell", &[], |ctx, _| {
            // Alt+Enter, as in Excel: a line break into the cell's value at
            // the caret (from Ready mode, after its text); no key put one
            // there, and Enter's line break split the record.
            let now = ctx.now;
            let d = ctx.doc()?;
            let (layout, _, rec, col) = csv_cell(d)?;
            if d.csv_editing().is_none() {
                if let Some(f) = rec.fields.get(col) {
                    let stops = crate::csv::value_stops(d.text().as_str(), f, &layout.dialect);
                    if let Some(&end) = stops.last() {
                        d.selection = org_edit::Selection::caret(end);
                    }
                }
                d.begin_csv_edit(true);
            }
            if !d.type_in_grid("\n", now) {
                d.insert_text("\n", now);
            }
            Ok(())
        }),
        c("csv.cancelEdit", "Cancel Entry", &[], |ctx, _| {
            // Escape: the cell as it was; else the selection collapsed.
            let now = ctx.now;
            let d = ctx.doc()?;
            if !d.cancel_csv_edit(now) {
                let head = d.selection.head;
                d.selection = org_edit::Selection::caret(head);
                d.clear_extra();
            }
            Ok(())
        }),
        c("csv.clearCells", "Clear Contents", &[], |ctx, _| {
            // Delete: in Ready mode the selected cells emptied; typing
            // into a cell, the character after the cursor.
            let now = ctx.now;
            let d = ctx.doc()?;
            if d.csv_editing().is_some() {
                d.delete_in_grid(true, now);
                return Ok(());
            }
            csv_clear_selection(d, now)
        }),
        c("csv.backspaceCell", "Clear and Type", &[], |ctx, _| {
            // Backspace: in Ready mode the cell emptied and typed into, as
            // in Excel; typing into a cell, the character before.
            let now = ctx.now;
            let d = ctx.doc()?;
            if d.csv_editing().is_some() {
                d.delete_in_grid(false, now);
                return Ok(());
            }
            // Over several cells, the one the selection started from, as
            // Excel clears the active cell alone (they were all cleared,
            // and Escape put one record back over another).
            let s = d.selection;
            if s.anchor != s.head
                && let Some((row, col)) = crate::csv::anchor_cell(d)
                && Some((row, col)) != crate::csv::cell_at(d).map(|(_, r, _, c)| (r, c))
            {
                d.selection = org_edit::Selection::caret(s.anchor);
                d.go_to_csv_cell(row, col);
            }
            let (_, _, rec, _) = csv_cell(d)?;
            let record = d.text().as_str()[rec.range.clone()].to_string();
            let steps = d.undo_depth();
            csv_clear_selection(d, now)?;
            d.begin_csv_edit(false);
            if let Some(e) = csv_state(d)?.edit.as_mut() {
                e.record = record;
                e.steps = steps;
            }
            Ok(())
        }),
        // Enter and Shift+Enter: the cell below or above, as in a
        // spreadsheet (Enter broke the record in two). Past the last row
        // Enter adds one, as Tab does.
        c("csv.cellBelow", "Cell Below", &[], |ctx, _| {
            if let Some(r) = csv_within(ctx, false, false) {
                return r;
            }
            let (_, next) = csv_neighbours(ctx);
            // After a run of Tabs along the row, the column it started
            // from, as in Excel.
            let d = ctx.doc()?;
            let run = csv_state(d)?.tab_run.take();
            let start = run
                .filter(|&(r, c, _)| csv_cell(d).is_ok_and(|(_, row, _, col)| (row, col) == (r, c)))
                .map(|(_, _, s)| s);
            csv_edit(ctx, |text, l, row, rec, col| {
                Ok(csv_down(text, l, row, rec, next, start.unwrap_or(col)))
            })
        }),
        c("csv.cellAbove", "Cell Above", &[], |ctx, _| {
            if let Some(r) = csv_within(ctx, false, true) {
                return r;
            }
            let (previous, _) = csv_neighbours(ctx);
            csv_edit(ctx, |_, _, _, _, col| {
                Ok((None, previous.map(|r| (r, col))))
            })
        }),
        c(
            "csv.previousField",
            "Previous Field",
            &["shift+tab"],
            |ctx, _| {
                if let Some(r) = csv_within(ctx, true, true) {
                    return r;
                }
                csv_tab(ctx, |ctx| {
                    let (previous, _) = csv_neighbours(ctx);
                    let hidden = csv_hidden(ctx.doc()?);
                    csv_edit(ctx, |text, l, row, _, col| {
                        if let Some(c) = (0..col).rev().find(|c| !hidden.contains(c)) {
                            return Ok((None, Some((row, c))));
                        }
                        // The previous record shown, at its last field that
                        // shows.
                        Ok((
                            None,
                            previous.map(|r| {
                                let n = l
                                    .index
                                    .borrow_mut()
                                    .record(text, r, &l.dialect)
                                    .map_or(1, |rec| rec.fields.len());
                                let c = (0..n).rev().find(|c| !hidden.contains(c));
                                (r, c.unwrap_or(usize::MAX))
                            }),
                        ))
                    })
                })
            },
        ),
        c(
            "csv.insertRow",
            "Insert Row",
            &["alt+shift+down"],
            |ctx, _| {
                // As many rows as the selection covers, after its last (as
                // the grid shows them), as Excel inserts; else one after the
                // cursor's.
                let rect = rectangle_or_cell(ctx.doc()?)?;
                let (rows, first_col) = (rect.rows, rect.cols.0);
                let after = rows.last().copied();
                csv_edit(ctx, |text, l, row, rec, _| {
                    let after = after.unwrap_or(row);
                    let rec = if after == row {
                        rec.clone()
                    } else {
                        l.index
                            .borrow_mut()
                            .record(text, after, &l.dialect)
                            .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?
                    };
                    let columns = l.widths.len().max(rec.fields.len());
                    Ok((
                        Some(crate::csv::insert_rows(
                            text,
                            &rec,
                            columns,
                            &l.dialect,
                            rows.len(),
                        )),
                        Some((after + 1, first_col)),
                    ))
                })
            },
        ),
        c(
            "csv.deleteRow",
            "Delete Row",
            &["alt+shift+up"],
            |ctx, _| {
                // The rows the selection covers as the grid shows them (a
                // filter's hidden rows between stay), else the cursor's;
                // the cursor to the row shown after them, else before.
                let rows = rectangle_or_cell(ctx.doc()?)?.rows;
                let d = ctx.doc()?;
                let to = rows
                    .last()
                    .and_then(|&r| crate::csv::view_step(d, r, 1))
                    .or_else(|| rows.first().and_then(|&r| crate::csv::view_step(d, r, -1)));
                csv_edit(ctx, |text, l, _, _, col| {
                    let tx = crate::csv::delete_row_list(text, &l.dialect, &rows);
                    let to = to.map_or(0, |r| r - rows.iter().filter(|&&x| x < r).count());
                    Ok((Some(tx), Some((to, col))))
                })
            },
        ),
        c("csv.moveRowUp", "Move Row Up", &["alt+up"], |ctx, _| {
            let (previous, _) = csv_row_neighbours(ctx)?;
            csv_edit(ctx, |text, l, row, rec, col| {
                let top = usize::from(l.dialect.header);
                // The row shown above (a filter's hidden rows stay).
                let prev = previous.filter(|&p| p >= top && row > top).and_then(|p| {
                    l.index
                        .borrow_mut()
                        .record(text, p, &l.dialect)
                        .map(|r| (p, r))
                });
                let (p, prev) =
                    prev.ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?;
                Ok((
                    Some(crate::csv::swap_rows(text, &prev, rec)),
                    Some((p, col)),
                ))
            })
        }),
        c(
            "csv.moveRowDown",
            "Move Row Down",
            &["alt+down"],
            |ctx, _| {
                let (_, next) = csv_row_neighbours(ctx)?;
                csv_edit(ctx, |text, l, row, rec, col| {
                    if l.dialect.header && row == 0 {
                        return Err(CommandError::new(crate::tr!("msg-csv-no-row")));
                    }
                    // The row shown below (a filter's hidden rows stay).
                    let next = next.and_then(|n| {
                        l.index
                            .borrow_mut()
                            .record(text, n, &l.dialect)
                            .map(|r| (n, r))
                    });
                    let (n, next) =
                        next.ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?;
                    Ok((
                        Some(crate::csv::swap_rows(text, rec, &next)),
                        Some((n, col)),
                    ))
                })
            },
        ),
        c(
            "csv.insertColumn",
            "Insert Column",
            &["alt+shift+right"],
            |ctx, _| {
                // As many columns as the selection covers, before its first,
                // as Excel inserts; else one at the cursor's.
                let rect = rectangle_or_cell(ctx.doc()?)?;
                let (col, last) = rect.cols;
                let n = last - col + 1;
                let top = rect.rows.first().copied();
                csv_edit(ctx, |text, l, row, _, _| {
                    Ok((
                        Some(crate::csv::insert_columns(text, &l.dialect, col, n)),
                        Some((top.unwrap_or(row), col)),
                    ))
                })?;
                // Hidden columns, widths and the sorted view's column follow
                // theirs.
                crate::csv::remap_columns(ctx.doc()?, |j| Some(if j >= col { j + n } else { j }));
                Ok(())
            },
        ),
        c(
            "csv.deleteColumn",
            "Delete Column",
            &["alt+shift+left"],
            |ctx, _| {
                // The columns the selection covers, else the cursor's.
                let (c0, c1) = rectangle_or_cell(ctx.doc()?)?.cols;
                csv_edit(ctx, |text, l, row, _, _| {
                    let left = l.widths.len().saturating_sub(c1 - c0 + 1);
                    Ok((
                        Some(crate::csv::delete_columns(text, &l.dialect, c0, c1)),
                        Some((row, c0.min(left.saturating_sub(1)))),
                    ))
                })?;
                let n = c1 - c0 + 1;
                crate::csv::remap_columns(ctx.doc()?, |j| {
                    if j < c0 {
                        Some(j)
                    } else if j > c1 {
                        Some(j - n)
                    } else {
                        None
                    }
                });
                Ok(())
            },
        ),
        c(
            "csv.moveColumnLeft",
            "Move Column Left",
            &["alt+left"],
            |ctx, _| {
                let col = csv_cell(ctx.doc()?)?.3;
                csv_edit(ctx, |text, l, row, _, col| {
                    if col == 0 {
                        return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
                    }
                    Ok((
                        Some(crate::csv::swap_columns(text, &l.dialect, col - 1)),
                        Some((row, col - 1)),
                    ))
                })?;
                crate::csv::remap_columns(ctx.doc()?, |j| Some(swapped(j, col - 1)));
                Ok(())
            },
        ),
        c(
            "csv.moveColumnRight",
            "Move Column Right",
            &["alt+right"],
            |ctx, _| {
                let col = csv_cell(ctx.doc()?)?.3;
                csv_edit(ctx, |text, l, row, rec, col| {
                    if col + 1 >= rec.fields.len() {
                        return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
                    }
                    Ok((
                        Some(crate::csv::swap_columns(text, &l.dialect, col)),
                        Some((row, col + 1)),
                    ))
                })?;
                crate::csv::remap_columns(ctx.doc()?, |j| Some(swapped(j, col)));
                Ok(())
            },
        ),
        c("csv.sortFile", "Sort File by Column", &[], |ctx, args| {
            let reverse = arg_bool(args, "reverse");
            csv_edit(ctx, |text, l, _, _, col| {
                let top = usize::from(l.dialect.header);
                Ok((
                    Some(crate::csv::sort_file(text, &l.dialect, col, reverse)),
                    Some((top, col)),
                ))
            })
        }),
        // A record as a form, a long field in a line of its own, find and
        // replace in a column, the frequency table, fields killed and
        // yanked (T2.7d.9).
        c("csv.recordView", "Record View", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, row, rec, _) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let text = d.text().as_str();
            let dl = &layout.dialect;
            let header = if dl.header {
                layout.index.borrow_mut().record(text, 0, dl)
            } else {
                None
            };
            let n = rec
                .fields
                .len()
                .max(header.as_ref().map_or(0, |h| h.fields.len()));
            let category = crate::tr!("category-record", row = row + 1);
            let items = (0..n)
                .map(|col| {
                    let name = header
                        .as_ref()
                        .and_then(|h| h.fields.get(col))
                        .map(|f| crate::csv::value(text, f, dl).into_owned())
                        .unwrap_or_else(|| crate::tr!("csv-column", n = col + 1));
                    let v = rec
                        .fields
                        .get(col)
                        .map(|f| crate::csv::value(text, f, dl).into_owned())
                        .unwrap_or_default();
                    crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "csv.setField",
                            &serde_json::json!({ "row": row, "column": col, "value_default": v }),
                        ),
                        // A line break shown as one (the lines ran together).
                        title: format!("{name}: {}", v.replace("\r\n", "\n").replace('\n', " ⏎ ")),
                        category: category.clone(),
                        detail: String::new(),
                        keys: String::new(),
                        also: String::new(),
                    }
                })
                .collect();
            request(ctx, Request::Choose(items))
        }),
        c("csv.editField", "Edit Field", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, row, rec, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let v = rec
                .fields
                .get(col)
                .map(|f| crate::csv::value(d.text().as_str(), f, &layout.dialect).into_owned())
                .unwrap_or_default();
            request(
                ctx,
                Request::Ask {
                    command: "csv.setField".into(),
                    args: serde_json::json!({ "row": row, "column": col, "value_default": v }),
                    arg: "value".into(),
                },
            )
        }),
        c("csv.setField", "Set Field", &[], |ctx, args| {
            let row = args.get("row").and_then(Value::as_u64).map(|r| r as usize);
            let col = args
                .get("column")
                .and_then(Value::as_u64)
                .map(|c| c as usize);
            let value = arg_str(args, "value")?.to_string();
            csv_edit(ctx, |text, l, here, rec, here_col| {
                let row = row.unwrap_or(here);
                let col = col.unwrap_or(here_col);
                let rec = if row == here {
                    rec.clone()
                } else {
                    l.index
                        .borrow_mut()
                        .record(text, row, &l.dialect)
                        .ok_or_else(|| {
                            let (cell, _) = crate::csv_tools::coordinates(row, col);
                            CommandError::new(crate::tr!("msg-csv-bad-cell", value = cell.as_str()))
                        })?
                };
                Ok((
                    Some(crate::csv::set_cell(text, &rec, col, &value, &l.dialect)),
                    Some((row, col)),
                ))
            })
        }),
        c(
            "csv.replaceInColumn",
            "Replace in Column",
            &[],
            |ctx, args| {
                let find = arg_str(args, "find")?.to_string();
                let with = args
                    .get("replace")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let mut count = 0;
                csv_edit(ctx, |text, l, row, _, col| {
                    let (tx, n) =
                        crate::csv::replace_in_column(text, &l.dialect, col, &find, &with);
                    count = n;
                    Ok(((n > 0).then_some(tx), Some((row, col))))
                })?;
                ctx.messages
                    .push(crate::tr!("msg-replaced-count", count = count));
                Ok(())
            },
        ),
        c("csv.frequencies", "Frequency Table", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, _, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let freq = crate::csv::frequencies(d.text().as_str(), &layout.dialect, col);
            let max = freq.first().map_or(0, |f| f.1);
            let category = crate::tr!("category-frequencies");
            let items = freq
                .into_iter()
                .take(500)
                .map(|(v, n)| crate::palette::PaletteItem {
                    id: crate::palette::invocation(
                        "csv.filter",
                        &serde_json::json!({ "text": v, "column": col }),
                    ),
                    title: format!("{n:>6}  {}  {v}", crate::csv::bar(n, max, 20)),
                    category: category.clone(),
                    detail: String::new(),
                    keys: String::new(),
                    also: String::new(),
                })
                .collect();
            request(ctx, Request::Choose(items))
        }),
        c("csv.histogram", "Histogram of Column", &[], |ctx, _| {
            // The column's numbers in ranges; choosing one goes to its
            // first row.
            let d = ctx.doc()?;
            let (layout, _, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let bins = crate::csv::histogram(d.text().as_str(), &layout.dialect, col);
            if bins.is_empty() {
                return Err(CommandError::new(crate::tr!("msg-csv-no-numbers")));
            }
            let max = bins.iter().map(|b| b.count).max().unwrap_or(0);
            let width = bins
                .iter()
                .map(|b| crate::csv::bin_label(b).chars().count())
                .max()
                .unwrap_or(0);
            let category = crate::tr!("category-histogram");
            let items = bins
                .iter()
                .map(|b| {
                    let label = crate::csv::bin_label(b);
                    let pad = width - label.chars().count();
                    crate::palette::PaletteItem {
                        id: if b.count == 0 {
                            crate::palette::invocation("csv.histogram", &serde_json::json!({}))
                        } else {
                            crate::palette::invocation(
                                "csv.goToCell",
                                &serde_json::json!({ "cell": format!("@{}${}", b.first + 1, col + 1) }),
                            )
                        },
                        title: format!(
                            "{label}{}  {:>6}  {}",
                            " ".repeat(pad),
                            b.count,
                            crate::csv::bar(b.count, max, 20)
                        ),
                        category: category.clone(),
                        detail: String::new(),
                        keys: String::new(),
                        also: String::new(),
                    }
                })
                .collect();
            request(ctx, Request::Choose(items))
        }),
        c("csv.killField", "Kill Field", &[], |ctx, _| {
            let mut killed = String::new();
            csv_edit(ctx, |text, l, row, rec, col| {
                // A cell past the record's end is empty already (it padded
                // the record).
                let Some(f) = rec.fields.get(col) else {
                    return Ok((None, Some((row, col))));
                };
                killed = crate::csv::value(text, f, &l.dialect).into_owned();
                Ok((
                    Some(crate::csv::set_cell(text, rec, col, "", &l.dialect)),
                    Some((row, col)),
                ))
            })?;
            ctx.clipboard.record(killed.clone());
            request(ctx, Request::CopyText(killed))
        }),
        c("csv.yankField", "Yank Field", &[], |ctx, _| {
            let v = ctx.clipboard.text.clone();
            // Nothing killed or copied: the cell stays (it was emptied).
            if v.is_empty() {
                return Err(CommandError::new(crate::tr!("msg-csv-nothing-to-yank")));
            }
            csv_edit(ctx, |text, l, row, rec, col| {
                Ok((
                    Some(crate::csv::set_cell(text, rec, col, &v, &l.dialect)),
                    Some((row, col)),
                ))
            })
        }),
        c("csv.filter", "Filter Rows", &[], |ctx, args| {
            // Only the rows with a field holding the text show; the file
            // stays as it is. An empty text shows them all.
            let text = arg_str(args, "text")?.trim().to_string();
            // With `column`, the records whose value there is the text (a
            // Frequency Table's choice), an empty one too.
            let column = arg_column(args);
            let d = ctx.doc()?;
            if d.meta.mode != crate::DocumentMode::Csv {
                return Err(CommandError::new(crate::tr!("msg-not-csv")));
            }
            let s = csv_state(d)?;
            s.filter = (!text.is_empty() || column.is_some()).then_some(text);
            s.filter_column = column;
            // Filtered again, the edits since too (the view keeps rows
            // edited out of the filter until then).
            crate::csv::view_again(d);
            Ok(())
        }),
        c("csv.sortView", "Sort View by Column", &[], |ctx, args| {
            // The rows shown in the order of the column at the cursor (again:
            // descending); the file keeps its order.
            let reverse = arg_bool(args, "reverse");
            let d = ctx.doc()?;
            let (_, _, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let s = csv_state(d)?;
            let reverse = reverse || s.sort == Some((col, false));
            s.sort = Some((col, reverse));
            // Sorted again, the edits since too.
            crate::csv::view_again(d);
            Ok(())
        }),
        scoped(
            cmd(
                "csv.openAsWorkbook",
                "Open as Workbook",
                "CSV",
                &[],
                None,
                crate::plugin_store::commands::open_as_workbook,
            ),
            Scope::only(&["csv", "text"]),
        ),
        // The dialect by hand: it is detected once and kept.
        c("csv.setDelimiter", "Set Delimiter", &[], |ctx, args| {
            let v = arg_str(args, "delimiter")?.to_string();
            // A space as itself or by name (it was trimmed away).
            let b = match v.trim() {
                "tab" | "\\t" => b'\t',
                "space" => b' ',
                "" if v == " " => b' ',
                s if s.len() == 1 && !matches!(s, "\r" | "\n") => s.as_bytes()[0],
                _ => {
                    return Err(CommandError::new(crate::tr!(
                        "msg-csv-bad-delimiter",
                        value = v.as_str()
                    )));
                }
            };
            let d = ctx.doc()?;
            let mut dialect = crate::csv::layout(d).dialect;
            if b == dialect.quote {
                return Err(CommandError::new(crate::tr!("msg-csv-quote-delimiter")));
            }
            dialect.delimiter = b;
            csv_state(d)?.set_dialect(dialect, false);
            Ok(())
        }),
        c("csv.setQuote", "Set Quote Character", &[], |ctx, args| {
            let v = arg_str(args, "quote")?.to_string();
            let Some(&q) = v.trim().as_bytes().first().filter(|_| v.trim().len() == 1) else {
                return Err(CommandError::new(crate::tr!(
                    "msg-csv-bad-delimiter",
                    value = v.as_str()
                )));
            };
            let d = ctx.doc()?;
            let mut dialect = crate::csv::layout(d).dialect;
            if q == dialect.delimiter {
                return Err(CommandError::new(crate::tr!("msg-csv-quote-delimiter")));
            }
            dialect.quote = q;
            csv_state(d)?.set_dialect(dialect, false);
            Ok(())
        }),
        c(
            "csv.toggleHeader",
            "First Row Is a Header",
            &[],
            |ctx, _| {
                let d = ctx.doc()?;
                let mut dialect = crate::csv::layout(d).dialect;
                dialect.header = !dialect.header;
                csv_state(d)?.set_dialect(dialect, false);
                Ok(())
            },
        ),
        c(
            "csv.detectDialect",
            "Detect Delimiter and Header Again",
            &[],
            |ctx, _| {
                let d = ctx.doc()?;
                let text = d.text().as_str();
                let dialect = crate::csv::detect_for(text, d.meta.path.as_deref());
                let provisional = !crate::csv::settled(text);
                csv_state(d)?.set_dialect(dialect, provisional);
                Ok(())
            },
        ),
        c("csv.unsortView", "File Order", &[], |ctx, _| {
            csv_state(ctx.doc()?)?.sort = None;
            Ok(())
        }),
        c("csv.clearFilter", "Show All Rows", &[], |ctx, _| {
            let d = ctx.doc()?;
            let s = csv_state(d)?;
            s.filter = None;
            s.filter_column = None;
            Ok(())
        }),
        c(
            "csv.copyAsTsv",
            "Copy as Tab-Separated Values",
            &[],
            |ctx, _| {
                let d = ctx.doc()?;
                let layout = crate::csv::layout(d);
                let text = d.text().as_str();
                let sel = d.selection;
                // The records the selection covers (all of them without one).
                let part = if sel.anchor == sel.head {
                    text.to_string()
                } else {
                    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    let mut idx = layout.index.borrow_mut();
                    let first = idx.row_at(text, a, &layout.dialect);
                    let last = idx.row_at(text, b, &layout.dialect);
                    let s = idx
                        .record(text, first, &layout.dialect)
                        .map_or(a, |r| r.range.start);
                    let e = idx
                        .record(text, last, &layout.dialect)
                        .map_or(b, |r| r.range.end);
                    text[s..e].to_string()
                };
                let tsv = crate::csv::to_tsv(&crate::csv::rows(&part, &layout.dialect));
                request(ctx, Request::CopyText(tsv))
            },
        ),
        // The grid's view (never written to the file).
        c(
            "csv.toggleAlignment",
            "Align Numbers Right",
            &[],
            |ctx, _| csv_view(ctx, |v| v.align_numbers = !v.align_numbers),
        ),
        c("csv.toggleRainbow", "Rainbow Columns", &[], |ctx, _| {
            csv_view(ctx, |v| v.rainbow = !v.rainbow)
        }),
        c("csv.toggleCoordinates", "Coordinate Grid", &[], |ctx, _| {
            csv_view(ctx, |v| v.coordinates = !v.coordinates)
        }),
        // Columns as the grid shows them (never written to the file).
        c("csv.hideColumn", "Hide Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, l, col| {
                let shown = (0..l.widths.len())
                    .filter(|j| !cols.hidden.contains(j))
                    .count();
                if shown <= 1 {
                    return Err(CommandError::new(crate::tr!("msg-csv-last-column")));
                }
                cols.hidden.insert(col);
                Ok(())
            })?;
            // The cursor to a column that shows.
            csv_to_shown_column(ctx)
        }),
        c("csv.showColumns", "Show All Columns", &[], |ctx, _| {
            csv_columns(ctx, |cols, _, _| {
                cols.hidden.clear();
                Ok(())
            })
        }),
        c("csv.widenColumn", "Widen Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, l, col| {
                cols.widths.insert(col, csv_drawn_width(l, col) + 2);
                Ok(())
            })
        }),
        c("csv.narrowColumn", "Narrow Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, l, col| {
                cols.widths
                    .insert(col, csv_drawn_width(l, col).saturating_sub(2).max(2));
                Ok(())
            })
        }),
        c("csv.setColumnWidth", "Column Width", &[], |ctx, args| {
            let w: usize = arg_str(args, "width")?
                .trim()
                .parse()
                .ok()
                .filter(|w| (2..=500).contains(w))
                .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-bad-width")))?;
            // The cursor's column, or the one given (a column's edge
            // dragged in the letters bar).
            let given = arg_column(args);
            csv_columns(ctx, |cols, _, col| {
                cols.widths.insert(given.unwrap_or(col), w);
                Ok(())
            })
        }),
        c("csv.autosizeColumn", "Autosize Column", &[], |ctx, args| {
            let text = ctx.doc()?.text().as_str().to_string();
            let given = arg_column(args);
            csv_columns(ctx, |cols, l, col| {
                let col = given.unwrap_or(col);
                let w = crate::csv::natural_widths(&text, &l.dialect)
                    .get(col)
                    .copied()
                    .unwrap_or(1);
                cols.widths.insert(col, w.clamp(2, 500));
                Ok(())
            })
        }),
        c(
            "csv.autosizeColumns",
            "Autosize All Columns",
            &[],
            |ctx, _| {
                let text = ctx.doc()?.text().as_str().to_string();
                csv_columns(ctx, |cols, l, _| {
                    for (j, w) in crate::csv::natural_widths(&text, &l.dialect)
                        .into_iter()
                        .enumerate()
                    {
                        cols.widths.insert(j, w.clamp(2, 500));
                    }
                    Ok(())
                })
            },
        ),
        c("csv.resetWidths", "Reset Column Widths", &[], |ctx, _| {
            csv_columns(ctx, |cols, _, _| {
                cols.widths.clear();
                Ok(())
            })
        }),
        c("csv.toggleFrozen", "Freeze First Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, _, _| {
                cols.frozen = !cols.frozen;
                Ok(())
            })
        }),
        c("csv.copyCells", "Copy Cells", &[], |ctx, _| {
            // The rectangle from the selection's anchor cell to the
            // cursor's, as TSV; Paste as Block writes it back as cells.
            let tsv = csv_copy_cells(ctx)?;
            request(ctx, Request::CopyText(tsv))
        }),
        c("csv.cutCells", "Cut Cells", &[], |ctx, _| {
            // Copied as Copy Cells does, then emptied: the rows and columns
            // stay.
            let tsv = csv_copy_cells(ctx)?;
            let now = ctx.now;
            let d = ctx.doc()?;
            let rect = rectangle_or_cell(d)?;
            let layout = crate::csv::layout(d);
            // The cells the records have emptied (a short record is not
            // padded for its missing ones).
            if let Some(tx) =
                crate::csv::clear_cells(d.text().as_str(), &layout, &rect.rows, rect.cols)
                    .filter(|tx| csv_changes(d.text().as_str(), tx))
            {
                d.apply(&tx, org_edit::ChangeKind::Command, now);
            }
            request(ctx, Request::CopyText(tsv))
        }),
        c("csv.pasteBlock", "Paste as Block", &[], |ctx, _| {
            let d = ctx.doc()?;
            if d.meta.mode != crate::DocumentMode::Csv {
                return Err(CommandError::new(crate::tr!("msg-not-csv")));
            }
            csv_state(d)?.paste_block = true;
            request(ctx, Request::Paste { plain: false })
        }),
        c("csv.fillDown", "Fill Down", &[], |ctx, _| {
            csv_fill(ctx, false)
        }),
        c("csv.fillSeries", "Fill Series", &[], |ctx, _| {
            csv_fill(ctx, true)
        }),
        c(
            "csv.removeDuplicates",
            "Remove Duplicate Rows",
            &[],
            |ctx, _| {
                let now = ctx.now;
                let d = ctx.doc()?;
                let (layout, row, _, col) = csv_cell(d)?;
                let (tx, n) =
                    crate::csv_tools::remove_duplicates(d.text().as_str(), &layout.dialect);
                if n > 0 {
                    // The cursor on its record, or where it was when that
                    // went (it went to the first row).
                    let at = layout
                        .index
                        .borrow_mut()
                        .record(d.text().as_str(), row, &layout.dialect)
                        .map_or(0, |r| r.range.start);
                    let at = tx.map(at, org_edit::Assoc::After);
                    d.apply(&tx, org_edit::ChangeKind::Command, now);
                    d.selection = org_edit::Selection::caret(at);
                    let row = csv_cell(d)?.1;
                    d.go_to_csv_cell(row, col);
                }
                ctx.messages
                    .push(crate::tr!("msg-csv-duplicates", count = n));
                Ok(())
            },
        ),
        c("csv.transpose", "Transpose", &[], |ctx, _| {
            let now = ctx.now;
            let d = ctx.doc()?;
            let (layout, row, _, col) = csv_cell(d)?;
            let tx = crate::csv_tools::transpose(d.text().as_str(), &layout.dialect);
            if csv_changes(d.text().as_str(), &tx) {
                d.apply(&tx, org_edit::ChangeKind::Command, now);
                // Rows are columns now: the view's columns start again, and
                // the cursor goes with its cell.
                let s = csv_state(d)?;
                s.columns = crate::csv::Columns::default();
                s.sort = None;
                d.go_to_csv_cell(col, row);
            }
            Ok(())
        }),
        c("csv.splitColumn", "Split Column", &[], |ctx, args| {
            let sep = arg_str(args, "separator")?.to_string();
            let sep = if sep.is_empty() { " ".to_string() } else { sep };
            if !arg_bool(args, "confirmed") {
                return csv_preview(ctx, "csv.splitColumn", args, |text, l, col| {
                    crate::csv_tools::split_column(text, &l.dialect, col, &sep).ok_or_else(|| {
                        CommandError::new(crate::tr!(
                            "msg-csv-nothing-to-split",
                            separator = sep.as_str()
                        ))
                    })
                });
            }
            csv_edit(ctx, |text, l, row, _, col| {
                let tx = crate::csv_tools::split_column(text, &l.dialect, col, &sep).ok_or_else(
                    || {
                        CommandError::new(crate::tr!(
                            "msg-csv-nothing-to-split",
                            separator = sep.as_str()
                        ))
                    },
                )?;
                Ok((Some(tx), Some((row, col))))
            })
        }),
        c(
            "csv.joinColumns",
            "Join with Next Column",
            &[],
            |ctx, args| {
                let sep = arg_str(args, "separator")?.to_string();
                if !arg_bool(args, "confirmed") {
                    return csv_preview(ctx, "csv.joinColumns", args, |text, l, col| {
                        crate::csv_tools::join_columns(text, &l.dialect, col, &sep)
                            .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-next-column")))
                    });
                }
                csv_edit(ctx, |text, l, row, _, col| {
                    let tx = crate::csv_tools::join_columns(text, &l.dialect, col, &sep)
                        .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-next-column")))?;
                    Ok((Some(tx), Some((row, col))))
                })
            },
        ),
        c(
            "csv.sortFileBy",
            "Sort File by Columns",
            &[],
            |ctx, args| {
                // Columns by letter or number in order, `-` for descending:
                // `B, -A`.
                let spec = arg_str(args, "columns")?.to_string();
                let d = ctx.doc()?;
                let (layout, ..) = csv_cell(d)?;
                let text = d.text().as_str();
                // The header's names name the columns too.
                let names: Vec<String> = layout
                    .dialect
                    .header
                    .then(|| layout.index.borrow_mut().record(text, 0, &layout.dialect))
                    .flatten()
                    .map(|r| {
                        r.fields
                            .iter()
                            .map(|f| crate::csv::value(text, f, &layout.dialect).into_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                let columns = layout.widths.len().max(names.len());
                let keys =
                    crate::csv_tools::parse_sort_keys(&spec, &names, columns).ok_or_else(|| {
                        CommandError::new(crate::tr!("msg-csv-bad-columns", value = spec.as_str()))
                    })?;
                csv_edit(ctx, |text, l, _, _, col| {
                    let top = usize::from(l.dialect.header);
                    Ok((
                        Some(crate::csv_tools::sort_by(text, &l.dialect, &keys)),
                        Some((top, col)),
                    ))
                })
            },
        ),
        c("csv.sumColumn", "Sum Column", &[], |ctx, args| {
            // As `C-c +` in an Org table: the sum shown and copied; with
            // `insert`, written into the cell at the cursor as a value.
            let insert = arg_bool(args, "insert");
            let mut sum = String::new();
            // With a filter on, the rows it shows, as a spreadsheet's
            // status bar sums a filtered range.
            let kept = crate::csv::kept_rows(ctx.doc()?);
            csv_edit(ctx, |text, l, row, rec, col| {
                if insert && l.dialect.header && row == 0 {
                    return Err(CommandError::new(crate::tr!("msg-csv-in-header")));
                }
                let skip = insert.then_some(row);
                let total =
                    crate::csv_tools::column_sum(text, &l.dialect, col, skip, kept.as_deref())
                        .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-numbers")))?;
                sum = crate::formulas::number(total);
                // A file `;` delimits writes decimal commas.
                if l.dialect.delimiter == b';' {
                    sum = sum.replace('.', ",");
                }
                Ok((
                    insert.then(|| crate::csv::set_cell(text, rec, col, &sum, &l.dialect)),
                    None,
                ))
            })?;
            ctx.messages
                .push(crate::tr!("msg-csv-sum", sum = sum.as_str()));
            if insert {
                Ok(())
            } else {
                request(ctx, Request::CopyText(sum))
            }
        }),
        c("csv.cellCoordinates", "Cell Coordinates", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, row, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let (cell, org) = crate::csv_tools::coordinates(row, col);
            let text = d.text().as_str();
            let header = layout
                .dialect
                .header
                .then(|| layout.index.borrow_mut().record(text, 0, &layout.dialect))
                .flatten()
                .and_then(|r| {
                    r.fields
                        .get(col)
                        .map(|f| crate::csv::value(text, f, &layout.dialect).into_owned())
                })
                .filter(|h| !h.is_empty());
            ctx.messages.push(match header {
                Some(h) => crate::tr!(
                    "msg-csv-cell-named",
                    cell = cell.as_str(),
                    org = org.as_str(),
                    column = h.as_str()
                ),
                None => crate::tr!("msg-csv-cell", cell = cell.as_str(), org = org.as_str()),
            });
            Ok(())
        }),
        // A click on a column's letter or a row's number, as in a
        // spreadsheet: the whole column or row selected, a rectangle of
        // cells that Copy, Cut and Delete take.
        c("csv.selectColumn", "Select Column", &[], |ctx, args| {
            let d = ctx.doc()?;
            let (layout, _, _, here) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let col = arg_column(args).unwrap_or(here);
            let text = d.text().as_str();
            let mut idx = layout.index.borrow_mut();
            let n = idx.count(text, &layout.dialect);
            // From the first record with the column to the last one.
            let ends: Vec<usize> = (0..n)
                .filter_map(|r| idx.record(text, r, &layout.dialect))
                .filter_map(|r| r.fields.get(col).map(|f| f.range.start))
                .collect();
            let (Some(&first), Some(&last)) = (ends.first(), ends.last()) else {
                return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
            };
            drop(idx);
            d.selection = org_edit::Selection {
                anchor: first,
                head: last,
            };
            Ok(())
        }),
        c("csv.selectRow", "Select Row", &[], |ctx, args| {
            let d = ctx.doc()?;
            let (layout, here, _, _) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let row = args
                .get("row")
                .and_then(Value::as_u64)
                .and_then(|r| usize::try_from(r).ok())
                .unwrap_or(here);
            let text = d.text().as_str();
            let rec = layout
                .index
                .borrow_mut()
                .record(text, row, &layout.dialect)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?;
            d.selection = org_edit::Selection {
                anchor: rec.range.start,
                head: rec.range.end,
            };
            Ok(())
        }),
        c("csv.goToCell", "Go to Cell", &[], |ctx, args| {
            // `B3`, `@3$2` or `3,2`.
            let name = arg_str(args, "cell")?.to_string();
            let (row, col) = crate::csv_tools::parse_cell(&name).ok_or_else(|| {
                CommandError::new(crate::tr!("msg-csv-bad-cell", value = name.as_str()))
            })?;
            csv_edit(ctx, |text, l, _, _, _| {
                let n = l.index.borrow_mut().count(text, &l.dialect);
                // Past the last row or column there is no cell (it went to
                // the row's last one without a word).
                if row >= n || col >= l.widths.len().max(1) {
                    return Err(CommandError::new(crate::tr!(
                        "msg-csv-bad-cell",
                        value = name.as_str()
                    )));
                }
                Ok((None, Some((row, col))))
            })
        }),
        c("csv.openAsText", "Open as Plain Text", &[], |ctx, _| {
            let base = ctx.config.parse_base();
            let doc = ctx.doc()?;
            doc.set_mode(crate::DocumentMode::Text { language: None }, &base);
            ctx.messages.push(crate::tr!(
                "msg-mode-set",
                mode = crate::DocumentMode::Text { language: None }.title()
            ));
            request(ctx, Request::ModeChanged)
        }),
        c("csv.convertToOrg", "Convert to Org Table", &[], |ctx, _| {
            let d = ctx.doc()?;
            let layout = crate::csv::layout(d);
            let table = crate::csv::to_org_table(d.text().as_str(), &layout.dialect);
            let path = d
                .meta
                .path
                .clone()
                .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-save-first")))?;
            let target = path.with_extension("org");
            if target.exists() {
                return Err(CommandError::new(crate::tr!(
                    "msg-import-exists",
                    path = target.display().to_string()
                )));
            }
            std::fs::write(&target, table).map_err(|e| CommandError::new(e.to_string()))?;
            ctx.messages.push(crate::tr!(
                "msg-csv-converted",
                path = target.display().to_string()
            ));
            request(
                ctx,
                Request::Open {
                    path: Some(target.display().to_string()),
                },
            )
        }),
    ]
}
