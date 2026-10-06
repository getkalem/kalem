//! Workbooks through the grid contract: any grid document (a legacy
//! `.xls`, an OpenDocument spreadsheet) copied into a new workbook, a
//! sheet copied from one workbook into another, and a text file's rows
//! read in as entries for a new workbook. The formats themselves are the
//! workbook plugin's: it makes a new workbook from entries and writes its
//! other kinds and `.ods` (the plugin API's `formats` interface).
//!
//! What crosses: values, formulas, dates, number formats, bold, italic,
//! underline, colors, fills, alignment, merged cells, column widths and
//! frozen panes. Charts, pictures, conditional formats and the like stay
//! behind.

use std::collections::BTreeMap;

use kalem_viewer::{FileHandle, GridCell, NewSheet, StyleChange, Viewer, ViewerDocument};

/// A number written with a point for decimals and commas between groups
/// of three digits (`-1,234.5`).
fn grouped_number(t: &str) -> Option<f64> {
    let body = t.strip_prefix('-').unwrap_or(t);
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let groups: Vec<&str> = int.split(',').collect();
    let digits = |g: &str| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit());
    let ok = digits(groups[0])
        && (groups.len() == 1 || groups[0].len() <= 3)
        && groups[1..].iter().all(|g| g.len() == 3 && digits(g))
        && (frac.is_empty() || digits(frac));
    if !ok {
        return None;
    }
    let n: f64 = format!(
        "{}.{}",
        groups.concat(),
        if frac.is_empty() { "0" } else { frac }
    )
    .parse()
    .ok()?;
    Some(if t.starts_with('-') { -n } else { n })
}

/// `v` as a number written with a comma for decimals (`1.234,56`,
/// `12,5%`, `-0,5`, `12`) rewritten with a point (`1,234.56`, `12.5%`), as
/// the sheet reads an entry; `None` for anything else.
pub fn decimal_comma_to_point(v: &str) -> Option<String> {
    let t = v.trim();
    let (t, percent) = match t.strip_suffix('%') {
        Some(p) => (p.trim_end(), "%"),
        None => (t, ""),
    };
    let (sign, body) = match t.strip_prefix('-') {
        Some(b) => ("-", b),
        None => ("", t),
    };
    let (int, frac) = body.split_once(',').unwrap_or((body, ""));
    let groups: Vec<&str> = int.split('.').collect();
    let digits = |g: &str| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit());
    let ok = digits(groups[0])
        && (groups.len() == 1 || groups[0].len() <= 3)
        && groups[1..].iter().all(|g| g.len() == 3 && digits(g))
        && (frac.is_empty() || digits(frac));
    if !ok {
        return None;
    }
    let int = groups.join(",");
    Some(if frac.is_empty() {
        format!("{sign}{int}{percent}")
    } else {
        format!("{sign}{int}.{frac}{percent}")
    })
}

/// Whether the numbers of `rows` are written with a comma for decimals
/// (`1.234,56`): what most of the values that can be read only one way
/// say; with none, whether the fields are apart by `;`, as in the
/// countries that write so.
pub fn guess_decimal_comma(rows: &[Vec<String>], delimiter: u8) -> bool {
    let (mut comma, mut point) = (0usize, 0usize);
    for v in rows.iter().flatten() {
        let t = v.trim().trim_end_matches('%');
        let last_comma = t.rfind(',');
        let last_point = t.rfind('.');
        let tail = |i: usize| t.len() - i - 1;
        match (last_comma, last_point) {
            // `1.234,5`: the comma after the point is the decimal one.
            (Some(c), Some(p)) if c > p && decimal_comma_to_point(t).is_some() => comma += 1,
            (Some(c), Some(p)) if p > c && grouped_number(t).is_some() => point += 1,
            // `1,5` or `12,25`: not groups of three.
            (Some(c), None) if tail(c) != 3 && decimal_comma_to_point(t).is_some() => comma += 1,
            (None, Some(p)) if tail(p) != 3 && t.parse::<f64>().is_ok() => point += 1,
            _ => {}
        }
    }
    if comma == point {
        return delimiter == b';';
    }
    comma > point
}

/// A workbook's bytes opened by `viewer`, as the file `name` (an `.xlsx`).
pub fn open_bytes(
    viewer: &dyn Viewer,
    name: &std::path::Path,
    bytes: Vec<u8>,
) -> Result<Box<dyn ViewerDocument>, String> {
    let bytes = std::sync::Arc::new(bytes);
    let len = bytes.len() as u64;
    let file = name
        .file_name()
        .map_or("book.xlsx".into(), |n| n.to_string_lossy().into_owned());
    let handle = FileHandle::from_reader(file, len, move |at: u64, n: usize| {
        let a = (at as usize).min(bytes.len());
        let b = (a + n).min(bytes.len());
        bytes[a..b].to_vec()
    });
    viewer.open(handle).map_err(|e| e.to_string())
}

/// A sheet of a grid document as read: its name, and each cell's entry
/// as typed and how it looks, by row and column.
struct SheetData {
    name: String,
    hidden: bool,
    cells: BTreeMap<(u32, u32), (String, GridCell, Option<String>)>,
    layout: kalem_viewer::GridLayout,
}

/// The grid sheets of `doc`.
fn sheets(doc: &mut dyn ViewerDocument) -> Result<Vec<(usize, SheetData)>, String> {
    let units = doc.structure().units;
    let mut out = Vec::new();
    for (u, unit) in units.iter().enumerate() {
        let Some(layout) = doc.grid(u) else { continue };
        let (rows, cols) = (layout.rows, layout.cols);
        if u64::from(rows) * u64::from(cols) > 5_000_000 {
            return Err(format!("{} is too large to convert", unit.label));
        }
        let mut cells = BTreeMap::new();
        // A band of rows at a time.
        let mut r = 0;
        while r < rows {
            let to = (r + 500).min(rows);
            for (row, col, cell) in doc.grid_cells(u, r..to, 0..cols) {
                let input = doc.cell_input(u, row, col);
                if input.is_empty() && cell == GridCell::default() {
                    continue;
                }
                let fmt = doc.cell_format(u, row, col).filter(|f| f != "General");
                cells.insert((row, col), (input, cell, fmt));
            }
            r = to;
        }
        let hidden = unit.label.ends_with(" (hidden)");
        let name = unit.label.trim_end_matches(" (hidden)").to_owned();
        out.push((
            u,
            SheetData {
                name,
                hidden,
                cells,
                layout,
            },
        ));
    }
    if out.is_empty() {
        return Err("No sheet to convert".into());
    }
    Ok(out)
}

/// Whether an entry, typed again, would be read as something other than
/// text (a number, a date, a formula, a logical value).
fn reads_as_value(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    t.starts_with('=')
        || t.starts_with('\'')
        || t.eq_ignore_ascii_case("true")
        || t.eq_ignore_ascii_case("false")
        || t.starts_with('#')
        || t.replace([',', '%', '$', ' '], "").parse::<f64>().is_ok()
        || (t.chars().next().is_some_and(|c| c.is_ascii_digit())
            && t.chars().all(|c| c.is_ascii_digit() || "-/.: ".contains(c)))
}

/// A cell's look as a change to make.
fn style_of(cell: &GridCell, fmt: &Option<String>) -> StyleChange {
    StyleChange {
        bold: cell.bold.then_some(true),
        italic: cell.italic.then_some(true),
        underline: cell.underline.then_some(true),
        strike: cell.strike.then_some(true),
        color: cell.color.map(Some),
        fill: cell.fill.map(Some),
        align: (cell.align != kalem_viewer::Align::General).then_some(cell.align),
        number_format: fmt.clone(),
        ..StyleChange::default()
    }
}

/// `src`'s sheets copied into `dst`, whose sheets match them in order.
fn fill(src: &[(usize, SheetData)], dst: &mut dyn ViewerDocument) -> Result<(), String> {
    let targets: Vec<usize> = (0..src.len()).collect();
    fill_into(src, &targets, dst)
}

/// `src`'s sheets' looks copied onto `dst`'s sheets `targets` (in order),
/// as one batch: a viewer keeps one copy of the workbook for undo rather
/// than one for each run of styled cells (thousands for a formatted
/// `.ods`, publish_todo 3.7).
fn fill_into(
    src: &[(usize, SheetData)],
    targets: &[usize],
    dst: &mut dyn ViewerDocument,
) -> Result<(), String> {
    dst.begin_batch();
    let filled = fill_looks(src, targets, dst);
    dst.end_batch();
    filled
}

fn fill_looks(
    src: &[(usize, SheetData)],
    targets: &[usize],
    dst: &mut dyn ViewerDocument,
) -> Result<(), String> {
    let e = |e: kalem_viewer::ViewerError| e.to_string();
    for ((_, s), &k) in src.iter().zip(targets) {
        let (Some(&(r1, _)), Some(c1)) = (
            s.cells.keys().next_back(),
            s.cells.keys().map(|k| k.1).max(),
        ) else {
            continue;
        };
        // Each run of like cells along a row, and the same run on the rows
        // below it (a column of amounts), in one change.
        let mut open: Vec<(u32, u32, u32, StyleChange)> = Vec::new();
        for r in 0..=r1 + 1 {
            let mut runs: Vec<(u32, u32, StyleChange)> = Vec::new();
            let mut c = 0;
            while r <= r1 && c <= c1 {
                let Some((_, cell, fmt)) = s.cells.get(&(r, c)) else {
                    c += 1;
                    continue;
                };
                let st = style_of(cell, fmt);
                let mut end = c;
                while end < c1
                    && s.cells
                        .get(&(r, end + 1))
                        .is_some_and(|(_, x, f)| style_of(x, f) == st)
                {
                    end += 1;
                }
                if st != StyleChange::default() {
                    runs.push((c, end, st));
                }
                c = end + 1;
            }
            // A run that goes on from the row above extends its rectangle;
            // the others end there.
            let mut still = Vec::new();
            for (top, c0, c1r, st) in open.drain(..) {
                if let Some(i) = runs
                    .iter()
                    .position(|(a, b, x)| *a == c0 && *b == c1r && *x == st)
                {
                    runs.remove(i);
                    still.push((top, c0, c1r, st));
                } else {
                    dst.change_style(k, [top, c0, r - 1, c1r], st).map_err(e)?;
                }
            }
            still.extend(runs.into_iter().map(|(a, b, st)| (r, a, b, st)));
            open = still;
        }
        for m in &s.layout.merged {
            dst.merge_cells(k, *m, false).map_err(e)?;
        }
        for (c, w) in s.layout.widths.iter().enumerate() {
            if (w - s.layout.default_width).abs() > 0.01 {
                dst.set_col_width(k, c as u32, *w).map_err(e)?;
            }
        }
        let (fr, fc) = s.layout.frozen;
        if fr > 0 || fc > 0 {
            dst.set_frozen(k, fr, fc).map_err(e)?;
        }
    }
    for ((_, s), &k) in src.iter().zip(targets) {
        if s.hidden {
            dst.edit_sheets(kalem_viewer::SheetEdit::Hide(k, true))
                .map_err(e)?;
        }
    }
    Ok(())
}

/// The viewer that makes workbooks: the workbook plugin's.
pub fn workbook_viewer() -> Result<std::sync::Arc<dyn Viewer>, String> {
    crate::viewer::find("book.xlsx", b"")
        .ok_or_else(|| "No workbook viewer: the workbook plugin is not installed".to_string())
}

/// `src` (any grid document) as a new workbook of `extension` `viewer`
/// makes: its entries written at once (`Viewer::new_file`), then their
/// looks through the viewer.
pub fn to_xlsx(
    viewer: &dyn Viewer,
    src: &mut dyn ViewerDocument,
    extension: &str,
) -> Result<Vec<u8>, String> {
    let data = sheets(src)?;
    let built: Vec<NewSheet> = data
        .iter()
        .map(|(_, s)| {
            let (Some(&(r1, _)), Some(c1)) = (
                s.cells.keys().next_back(),
                s.cells.keys().map(|k| k.1).max(),
            ) else {
                return NewSheet {
                    name: s.name.clone(),
                    rows: Vec::new(),
                };
            };
            let mut rows = vec![vec![String::new(); c1 as usize + 1]; r1 as usize + 1];
            for (&(r, c), (input, cell, _)) in &s.cells {
                // Text that would read as a number stays text.
                let text_cell = !cell.numeric && !cell.formula;
                rows[r as usize][c as usize] = if text_cell && reads_as_value(input) {
                    format!("'{input}")
                } else {
                    input.clone()
                };
            }
            NewSheet {
                name: s.name.clone(),
                rows,
            }
        })
        .collect();
    let bytes = viewer
        .new_file(extension, &built)
        .map_err(|e| e.to_string())?;
    let mut dst = open_bytes(
        viewer,
        std::path::Path::new(&format!("book.{extension}")),
        bytes,
    )?;
    fill(&data, dst.as_mut())?;
    whole_colors(src, &data, dst.as_mut())?;
    Ok(dst.save().map_err(|e| e.to_string())?.bytes)
}

/// Columns and rows colored whole in `src` (an OpenDocument column's or
/// row's cell style) colored whole in `dst`: what a cell past the used
/// range shows in each column, and past the last column in each row.
fn whole_colors(
    src: &mut dyn ViewerDocument,
    data: &[(usize, SheetData)],
    dst: &mut dyn ViewerDocument,
) -> Result<(), String> {
    let e = |e: kalem_viewer::ViewerError| e.to_string();
    for (k, (u, s)) in data.iter().enumerate() {
        let (rows, cols) = (s.layout.rows, s.layout.cols);
        let (max_r, max_c) = (s.layout.max_rows, s.layout.max_cols);
        if rows + 1 >= max_r || cols + 1 >= max_c {
            continue;
        }
        let fill_at = |src: &mut dyn ViewerDocument, r: u32, c: u32| {
            src.grid_cells(*u, r..r + 1, c..c + 1)
                .first()
                .and_then(|x| x.2.fill)
        };
        for c in 0..cols {
            if let Some(f) = fill_at(src, rows + 1, c) {
                let change = StyleChange {
                    fill: Some(Some(f)),
                    ..StyleChange::default()
                };
                dst.change_style(k, [0, c, 1_048_575, c], change)
                    .map_err(e)?;
            }
        }
        for r in 0..rows {
            if let Some(f) = fill_at(src, r, cols + 1) {
                let change = StyleChange {
                    fill: Some(Some(f)),
                    ..StyleChange::default()
                };
                dst.change_style(k, [r, 0, r, 16_383], change).map_err(e)?;
            }
        }
    }
    Ok(())
}

/// Sheet `unit` of `src` copied into workbook `dst` as its last sheet,
/// named as it was (or as a copy is named when the name is taken): its
/// entries, looks, merged cells, widths and frozen panes. The new sheet's
/// place.
pub fn copy_sheet_into(
    src: &mut dyn ViewerDocument,
    unit: usize,
    dst: &mut dyn ViewerDocument,
) -> Result<usize, String> {
    let e = |e: kalem_viewer::ViewerError| e.to_string();
    let data: Vec<(usize, SheetData)> = sheets(src)?
        .into_iter()
        .filter(|(u, _)| *u == unit)
        .collect();
    let Some((_, sheet)) = data.first() else {
        return Err("Only a sheet of cells is copied to another workbook".into());
    };
    let names: Vec<String> = dst
        .structure()
        .units
        .iter()
        .map(|u| u.label.trim_end_matches(" (hidden)").to_lowercase())
        .collect();
    let name = std::iter::once(sheet.name.clone())
        .chain((2..).map(|n| {
            let suffix = format!(" ({n})");
            let keep: String = sheet
                .name
                .chars()
                .take(31 - suffix.chars().count())
                .collect();
            format!("{keep}{suffix}")
        }))
        .find(|n| !names.contains(&n.to_lowercase()))
        .unwrap_or_default();
    let n = dst.structure().units.len();
    let at = dst
        .edit_sheets(kalem_viewer::SheetEdit::Insert(n))
        .map_err(e)?;
    dst.edit_sheets(kalem_viewer::SheetEdit::Rename(at, name))
        .map_err(e)?;
    let (Some(&(r1, _)), Some(c1)) = (
        sheet.cells.keys().next_back(),
        sheet.cells.keys().map(|k| k.1).max(),
    ) else {
        return Ok(at);
    };
    let mut rows = vec![vec![String::new(); c1 as usize + 1]; r1 as usize + 1];
    for (&(r, c), (input, cell, _)) in &sheet.cells {
        let text_cell = !cell.numeric && !cell.formula;
        rows[r as usize][c as usize] = if text_cell && reads_as_value(input) {
            format!("'{input}")
        } else {
            input.clone()
        };
    }
    dst.set_cells(at, 0, 0, &rows).map_err(e)?;
    fill_into(&data, &[at], dst)?;
    Ok(at)
}

/// How a column of a text file is read in (Excel's Text Import Wizard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    /// Numbers and dates read as such, the rest as text.
    General,
    /// Kept as text.
    Text,
    /// Dates in this order of day, month and year (`dmy`, `mdy`, `ymd`).
    Date([u8; 3]),
    /// Left out.
    Skip,
}

/// Column types as written: `A=text, C=date dmy, D=skip` (columns by
/// letter or number); the others General.
pub fn parse_types(s: &str) -> Result<BTreeMap<usize, ColumnType>, String> {
    let mut out = BTreeMap::new();
    for part in s.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (col, ty) = part
            .split_once('=')
            .ok_or_else(|| format!("{part}: write a column and its type, as C=date"))?;
        let col = col.trim();
        let c = match col.parse::<usize>() {
            Ok(n) if n > 0 => n - 1,
            _ => crate::csv_tools::column_index(col)
                .filter(|_| col.chars().all(|ch| ch.is_ascii_alphabetic()))
                .ok_or_else(|| format!("{col} is not a column"))?,
        };
        let mut words = ty.split_whitespace();
        let t = match words.next().map(str::to_ascii_lowercase).as_deref() {
            Some("general") => ColumnType::General,
            Some("text") => ColumnType::Text,
            Some("skip") => ColumnType::Skip,
            Some("date") => {
                let order = words.next().unwrap_or("dmy").to_ascii_lowercase();
                let b = order.as_bytes();
                if b.len() != 3 || !(b.contains(&b'd') && b.contains(&b'm') && b.contains(&b'y')) {
                    return Err(format!("{order}: a date's order is dmy, mdy or ymd"));
                }
                ColumnType::Date([b[0], b[1], b[2]])
            }
            _ => {
                return Err(format!(
                    "{part}: the types are general, text, date and skip"
                ));
            }
        };
        out.insert(c, t);
    }
    Ok(out)
}

/// A date as written in `order` (any of `/`, `.`, `-` between) as ISO.
fn iso_date(s: &str, order: [u8; 3]) -> Option<String> {
    let parts: Vec<&str> = s.trim().split(['/', '.', '-']).collect();
    if parts.len() != 3 {
        return None;
    }
    let (mut d, mut m, mut y) = (0u32, 0u32, 0i32);
    for (k, p) in order.iter().zip(&parts) {
        let n: i64 = p.trim().parse().ok()?;
        match k {
            b'd' => d = n as u32,
            b'm' => m = n as u32,
            _ => y = n as i32,
        }
    }
    if y < 100 {
        y += if y < 30 { 2000 } else { 1900 };
    }
    jiff::civil::Date::new(y as i16, m as i8, d as i8).ok()?;
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// A text file's rows read in by column types: entries as typed into a
/// sheet; with `decimal_comma`, numbers written `1.234,56` read as
/// numbers too.
pub fn import_rows(
    rows: &[Vec<String>],
    types: &BTreeMap<usize, ColumnType>,
    decimal_comma: bool,
) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .filter(|(c, _)| types.get(c) != Some(&ColumnType::Skip))
                .map(
                    |(c, v)| match types.get(&c).copied().unwrap_or(ColumnType::General) {
                        _ if v.is_empty() => String::new(),
                        ColumnType::Text => format!("'{v}"),
                        ColumnType::Date(order) => {
                            iso_date(v, order).unwrap_or_else(|| format!("'{v}"))
                        }
                        // A formula in a text file is text.
                        _ if v.starts_with('=') => format!("'{v}"),
                        _ if decimal_comma => {
                            decimal_comma_to_point(v).unwrap_or_else(|| point_as_text(v))
                        }
                        _ => v.clone(),
                    },
                )
                .collect()
        })
        .collect()
}

/// A value of a file that writes decimals with a comma: one that would
/// read as a number written with a point (`1.5`, a date's `1.12`) is
/// text there.
fn point_as_text(v: &str) -> String {
    let t = v.trim();
    let p = t.strip_suffix('%').map_or(t, str::trim_end);
    if p.parse::<f64>().is_ok_and(f64::is_finite) || grouped_number(p).is_some() {
        format!("'{v}")
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_commas_read_as_numbers() {
        // publish_todo 3.7: `1,5`, `1.234,56`, `50%` and `12,5%` were text.
        assert_eq!(decimal_comma_to_point("1,5").as_deref(), Some("1.5"));
        assert_eq!(
            decimal_comma_to_point("1.234,56").as_deref(),
            Some("1,234.56")
        );
        assert_eq!(decimal_comma_to_point("-12,5 %").as_deref(), Some("-12.5%"));
        assert_eq!(decimal_comma_to_point("1.234").as_deref(), Some("1,234"));
        assert_eq!(decimal_comma_to_point("12").as_deref(), Some("12"));
        assert_eq!(decimal_comma_to_point("1,2,3"), None);
        assert_eq!(decimal_comma_to_point("abc"), None);
        let rows = |v: &[&str]| vec![v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()];
        assert!(guess_decimal_comma(&rows(&["1,5", "2.000,25", "x"]), b','));
        assert!(!guess_decimal_comma(&rows(&["1.5", "2,000.25"]), b';'));
        assert!(guess_decimal_comma(&rows(&["1.234"]), b';'));
        let types = BTreeMap::new();
        let read = import_rows(
            &rows(&["1.234,5", "50%", "12,5%", "1.5", "x"]),
            &types,
            true,
        );
        assert_eq!(read[0], ["1,234.5", "50%", "12.5%", "'1.5", "x"]);
        // In a file of decimal commas, what reads as a number written
        // with a point is text.
        assert_eq!(point_as_text("1.5"), "'1.5");
        assert_eq!(point_as_text("1,234.5"), "'1,234.5");
        assert_eq!(point_as_text("12.5%"), "'12.5%");
        assert_eq!(point_as_text("1,23"), "1,23");
        assert_eq!(point_as_text("inf"), "inf");
    }

    #[test]
    fn text_read_in() {
        let types = parse_types("A=text, 3=date mdy, D=skip").unwrap();
        let rows = vec![vec![
            "007".to_string(),
            "1.5".into(),
            "12/31/2025".into(),
            "x".into(),
            "=1+1".into(),
        ]];
        assert_eq!(
            import_rows(&rows, &types, false),
            vec![vec![
                "'007".to_string(),
                "1.5".into(),
                "2025-12-31".into(),
                "'=1+1".into()
            ]]
        );
        assert!(parse_types("A=money").is_err());
        assert_eq!(iso_date("31.12.25", *b"dmy").as_deref(), Some("2025-12-31"));
        assert_eq!(iso_date("31.02.2025", *b"dmy"), None);
    }
}
