//! Workbooks in other formats, through the grid contract: any grid
//! document (a legacy `.xls`, an OpenDocument spreadsheet, a CSV file's
//! rows) copied into a new Excel workbook the workbook viewer writes, and
//! a workbook written as an OpenDocument spreadsheet (`.ods`).
//!
//! What crosses: values, formulas (OpenFormula in `.ods`), dates, number
//! formats, bold, italic, underline, colors, fills, alignment, merged
//! cells, column widths and frozen panes (into `.xlsx`). Charts, pictures,
//! conditional formats and the like stay behind.

use std::collections::BTreeMap;
use std::io::Write as _;

use kalem_viewer::{FileHandle, GridCell, StyleChange, Viewer, ViewerDocument};

/// A zip file of `entries` (name, bytes, deflated), as Office files are.
pub fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, deflate) in entries {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let body = if *deflate {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            let _ = e.write_all(data);
            e.finish().unwrap_or_default()
        } else {
            data.to_vec()
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let offset = out.len() as u32;
        let header = |sig: u32, central: bool| -> Vec<u8> {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(method.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(0x21u16.to_le_bytes());
            h.extend(crc.sum().to_le_bytes());
            h.extend((body.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central {
                // Comment length, disk, internal and external attributes.
                h.extend([0u8; 6]);
                h.extend(0u32.to_le_bytes());
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(&body);
        central.extend(header(0x0201_4b50, true));
    }
    let at = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A sheet's name and its entries as typed, by row and column.
type SheetEntries = (String, BTreeMap<(u32, u32), String>);

/// An entry as a cell holds it.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Number(f64),
    Text(String),
    Bool(bool),
    /// A serial date or time and its built-in format (14, 22, 21).
    Date(f64, u32),
    Formula(String),
}

/// An entry as typed (as a cell's entry reads back: plain numbers, ISO
/// dates and times, `=` formulas, `TRUE`, `'` before text that would
/// read otherwise).
fn entry(input: &str) -> Option<Entry> {
    if input.is_empty() {
        return None;
    }
    if let Some(t) = input.strip_prefix('\'') {
        return Some(Entry::Text(t.to_owned()));
    }
    if let Some(f) = input.strip_prefix('=') {
        return Some(Entry::Formula(f.to_owned()));
    }
    let t = input.trim();
    if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") {
        return Some(Entry::Bool(t.eq_ignore_ascii_case("true")));
    }
    if let Ok(n) = t.parse::<f64>()
        && n.is_finite()
    {
        return Some(Entry::Number(n));
    }
    if let Some((kind, v)) = date_value(t) {
        return serial(kind, &v).map(|(n, f)| Entry::Date(n, f));
    }
    Some(Entry::Text(input.to_owned()))
}

/// An OpenDocument date or time value as a serial number and the format
/// that shows it.
fn serial(kind: &str, v: &str) -> Option<(f64, u32)> {
    let time = |t: &str| -> Option<f64> {
        let p: Vec<f64> = t
            .split(':')
            .map(|x| x.parse().ok())
            .collect::<Option<_>>()?;
        Some((p[0] * 3600.0 + p.get(1).unwrap_or(&0.0) * 60.0 + p.get(2).unwrap_or(&0.0)) / 86400.0)
    };
    if kind == "time" {
        // `PT10H30M00S`.
        let t = v
            .trim_start_matches("PT")
            .replace(['H', 'M'], ":")
            .replace('S', "");
        return Some((time(&t)?, 21));
    }
    let (d, t) = match v.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (v, None),
    };
    let p: Vec<i32> = d
        .split('-')
        .map(|x| x.parse().ok())
        .collect::<Option<_>>()?;
    let date = jiff::civil::Date::new(p[0] as i16, p[1] as i8, p[2] as i8).ok()?;
    let base = jiff::civil::date(1899, 12, 30);
    let days = (date - base).get_days() as f64;
    match t {
        Some(t) => Some((days + time(t)?, 22)),
        None => Some((days, 14)),
    }
}

/// An Excel workbook of sheets (name, entries by row and column as typed),
/// written straight, every cell at once.
fn build_xlsx(sheets: &[SheetEntries]) -> Vec<u8> {
    let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    let mut list = String::new();
    let mut rels = String::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, (name, cells)) in sheets.iter().enumerate() {
        let k = i + 1;
        types.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{k}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
        list.push_str(&format!(
            r#"<sheet name="{}" sheetId="{k}" r:id="rId{k}"/>"#,
            esc(name)
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{k}" Type="{rel}/worksheet" Target="worksheets/sheet{k}.xml"/>"#
        ));
        let mut data = String::new();
        let mut row: Option<u32> = None;
        for (&(r, c), input) in cells {
            let Some(e) = entry(input) else { continue };
            if row != Some(r) {
                if row.is_some() {
                    data.push_str("</row>");
                }
                data.push_str(&format!(r#"<row r="{}">"#, r + 1));
                row = Some(r);
            }
            let at = format!("{}{}", crate::csv_tools::column_letters(c as usize), r + 1);
            data.push_str(&match e {
                Entry::Number(n) => format!(r#"<c r="{at}"><v>{n}</v></c>"#),
                Entry::Bool(b) => format!(r#"<c r="{at}" t="b"><v>{}</v></c>"#, u8::from(b)),
                Entry::Date(n, f) => {
                    let s = match f {
                        14 => 1,
                        22 => 2,
                        _ => 3,
                    };
                    format!(r#"<c r="{at}" s="{s}"><v>{n}</v></c>"#)
                }
                Entry::Formula(f) => format!(r#"<c r="{at}"><f>{}</f></c>"#, esc(&f)),
                Entry::Text(t) => format!(
                    r#"<c r="{at}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                    esc(&t)
                ),
            });
        }
        if row.is_some() {
            data.push_str("</row>");
        }
        parts.push((
            format!("xl/worksheets/sheet{k}.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{main}" xmlns:r="{rel}"><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><sheetData>{data}</sheetData></worksheet>"#
            ),
        ));
    }
    let n = sheets.len() + 1;
    rels.push_str(&format!(
        r#"<Relationship Id="rId{n}" Type="{rel}/styles" Target="styles.xml"/>"#
    ));
    types.push_str("</Types>");
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{main}" xmlns:r="{rel}"><bookViews><workbookView/></bookViews><sheets>{list}</sheets><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#
    );
    // The formats dates and times show with: Excel's built-in 14, 22, 21.
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="{main}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="22" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="21" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
    );
    let root = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{rel}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let wb_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let mut entries: Vec<(&str, &[u8], bool)> = vec![
        ("[Content_Types].xml", types.as_bytes(), true),
        ("_rels/.rels", root.as_bytes(), true),
        ("xl/workbook.xml", workbook.as_bytes(), true),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes(), true),
        ("xl/styles.xml", styles.as_bytes(), true),
    ];
    for (name, text) in &parts {
        entries.push((name.as_str(), text.as_bytes(), true));
    }
    zip(&entries)
}

/// An Excel workbook of empty sheets named `names`.
pub fn blank_xlsx(names: &[String]) -> Vec<u8> {
    let sheets: Vec<SheetEntries> = names.iter().map(|n| (n.clone(), BTreeMap::new())).collect();
    build_xlsx(&sheets)
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

/// `src`'s sheets' looks copied onto `dst`'s sheets `targets` (in order).
fn fill_into(
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
        // Each run of like cells along a row in one change.
        for r in 0..=r1 {
            let mut c = 0;
            while c <= c1 {
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
                    dst.change_style(k, [r, c, r, end], st.clone()).map_err(e)?;
                }
                c = end + 1;
            }
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

/// `src` (any grid document) as a new Excel workbook `viewer` writes:
/// its entries written at once, then their looks through the viewer.
pub fn to_xlsx(viewer: &dyn Viewer, src: &mut dyn ViewerDocument) -> Result<Vec<u8>, String> {
    let data = sheets(src)?;
    let built: Vec<SheetEntries> = data
        .iter()
        .map(|(_, s)| {
            let cells = s
                .cells
                .iter()
                .map(|(&k, (input, cell, _))| {
                    // Text that would read as a number stays text.
                    let text_cell = !cell.numeric && !cell.formula;
                    let v = if text_cell && reads_as_value(input) {
                        format!("'{input}")
                    } else {
                        input.clone()
                    };
                    (k, v)
                })
                .collect();
            (s.name.clone(), cells)
        })
        .collect();
    let mut dst = open_bytes(
        viewer,
        std::path::Path::new("book.xlsx"),
        build_xlsx(&built),
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

/// The entries of a zip file: name and bytes (stored or deflated).
pub fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::Read as _;
    let u16_at = |i: usize| -> Option<usize> {
        Some(u16::from_le_bytes(bytes.get(i..i + 2)?.try_into().ok()?) as usize)
    };
    let u32_at = |i: usize| -> Option<usize> {
        Some(u32::from_le_bytes(bytes.get(i..i + 4)?.try_into().ok()?) as usize)
    };
    let bad = || "Not a zip file".to_string();
    // The end of the central directory, searched from the end.
    let end = (0..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(bad)?;
    let count = u16_at(end + 10).ok_or_else(bad)?;
    let mut at = u32_at(end + 16).ok_or_else(bad)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(bad());
        }
        let method = u16_at(at + 10).ok_or_else(bad)?;
        let size = u32_at(at + 20).ok_or_else(bad)?;
        let (nlen, xlen, clen) = (
            u16_at(at + 28).ok_or_else(bad)?,
            u16_at(at + 30).ok_or_else(bad)?,
            u16_at(at + 32).ok_or_else(bad)?,
        );
        let local = u32_at(at + 42).ok_or_else(bad)?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + nlen).ok_or_else(bad)?)
            .into_owned();
        at += 46 + nlen + xlen + clen;
        let lname = u16_at(local + 26).ok_or_else(bad)?;
        let lextra = u16_at(local + 28).ok_or_else(bad)?;
        let start = local + 30 + lname + lextra;
        let data = bytes.get(start..start + size).ok_or_else(bad)?;
        let body = match method {
            0 => data.to_vec(),
            8 => {
                let mut v = Vec::new();
                flate2::read::DeflateDecoder::new(data)
                    .read_to_end(&mut v)
                    .map_err(|e| e.to_string())?;
                v
            }
            _ => return Err(format!("{name}: a compression Kalem does not read")),
        };
        out.push((name, body));
    }
    Ok(out)
}

/// A template (`.xltx`, `.xltm`) made a workbook (`.xlsx`, `.xlsm`): the
/// same parts, its main part's type a workbook's.
pub fn template_to_workbook(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut entries = unzip(bytes)?;
    let mut found = false;
    for (name, body) in &mut entries {
        if name == "[Content_Types].xml" {
            let text = String::from_utf8_lossy(body)
                .replace(
                    "spreadsheetml.template.main+xml",
                    "spreadsheetml.sheet.main+xml",
                )
                .replace(
                    "application/vnd.ms-excel.template.macroEnabled.main+xml",
                    "application/vnd.ms-excel.sheet.macroEnabled.main+xml",
                );
            *body = text.into_bytes();
            found = true;
        }
    }
    if !found {
        return Err("Not an Excel template".into());
    }
    let list: Vec<(&str, &[u8], bool)> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice(), true))
        .collect();
    Ok(zip(&list))
}

/// Rows of entries (as typed) as a new Excel workbook of one sheet.
pub fn rows_to_xlsx(sheet: &str, rows: &[Vec<String>]) -> Vec<u8> {
    let mut cells = BTreeMap::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            if !v.is_empty() {
                cells.insert((r as u32, c as u32), v.clone());
            }
        }
    }
    build_xlsx(&[(sheet.to_owned(), cells)])
}

/// An Excel formula (without `=`) in OpenFormula: references in brackets
/// (`[.A1:.B2]`, `['Other sheet'.A1]`), arguments apart by `;`, array
/// rows by `|`.
pub fn open_formula(f: &str) -> String {
    let chars: Vec<char> = f.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_array = false;
    let cell_at = |s: &[char], mut j: usize| -> Option<usize> {
        // `$A$1`, `A1`, `A:A`'s halves, `1:1`'s halves: a cell here.
        let start = j;
        if j < s.len() && s[j] == '$' {
            j += 1;
        }
        let letters = j;
        while j < s.len() && s[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j == letters || j - letters > 3 {
            return None;
        }
        if j < s.len() && s[j] == '$' {
            j += 1;
        }
        let digits = j;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        (j > digits && j > start).then_some(j)
    };
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '"' => {
                // A string, as it is.
                out.push(ch);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == '"' {
                        if chars.get(i + 1) == Some(&'"') {
                            out.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            '{' => in_array = true,
            '}' => in_array = false,
            ',' => {
                out.push(';');
                i += 1;
                continue;
            }
            ';' if in_array => {
                out.push('|');
                i += 1;
                continue;
            }
            _ => {}
        }
        let boundary = i == 0
            || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '.');
        if boundary && (ch.is_ascii_alphabetic() || ch == '$' || ch == '\'' || ch == '_') {
            // A sheet's name, then `!`.
            let mut j = i;
            let mut sheet: Option<String> = None;
            if ch == '\'' {
                let mut k = i + 1;
                let mut name = String::new();
                while k < chars.len() {
                    if chars[k] == '\'' {
                        if chars.get(k + 1) == Some(&'\'') {
                            name.push('\'');
                            k += 2;
                            continue;
                        }
                        break;
                    }
                    name.push(chars[k]);
                    k += 1;
                }
                if chars.get(k + 1) == Some(&'!') {
                    sheet = Some(format!("'{}'", name.replace('\'', "''")));
                    j = k + 2;
                }
            } else {
                let mut k = i;
                while k < chars.len()
                    && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '.')
                {
                    k += 1;
                }
                if chars.get(k) == Some(&'!') {
                    sheet = Some(chars[i..k].iter().collect());
                    j = k + 1;
                }
            }
            // Whole columns: `A:C`.
            let col_end = |mut k: usize| -> Option<usize> {
                let st = k;
                if chars.get(k) == Some(&'$') {
                    k += 1;
                }
                let l = k;
                while k < chars.len() && chars[k].is_ascii_alphabetic() {
                    k += 1;
                }
                (k > l && k - l <= 3 && k > st).then_some(k)
            };
            if let Some(e1) = col_end(j)
                && chars.get(e1) == Some(&':')
                && let Some(e2) = col_end(e1 + 1)
                && !chars
                    .get(e2)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '(')
            {
                let a: String = chars[j..e1].iter().collect();
                let b: String = chars[e1 + 1..e2].iter().collect();
                out.push_str(&format!("[{}.{a}:.{b}]", sheet.clone().unwrap_or_default()));
                i = e2;
                continue;
            }
            if let Some(end) = cell_at(&chars, j)
                && chars.get(end) != Some(&'(')
                && !chars
                    .get(end)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                let first: String = chars[j..end].iter().collect();
                let (second, next) = match chars.get(end) {
                    Some(':') => match cell_at(&chars, end + 1) {
                        Some(e2) => (Some(chars[end + 1..e2].iter().collect::<String>()), e2),
                        None => (None, end),
                    },
                    _ => (None, end),
                };
                let sh = sheet.unwrap_or_default();
                out.push('[');
                out.push_str(&sh);
                out.push('.');
                out.push_str(&first);
                if let Some(s2) = second {
                    out.push_str(":.");
                    out.push_str(&s2);
                }
                out.push(']');
                i = next;
                continue;
            }
            // A function or a name: as it is, Excel's future prefixes out.
            let mut k = i;
            while k < chars.len()
                && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '.')
            {
                k += 1;
            }
            let word: String = chars[i..k.max(i + 1)].iter().collect();
            let word = word
                .strip_prefix("_xlfn._xlws.")
                .or_else(|| word.strip_prefix("_xlfn."))
                .or_else(|| word.strip_prefix("_xlws."))
                .unwrap_or(&word)
                .to_owned();
            out.push_str(&word);
            i = k.max(i + 1);
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// A width in characters as centimeters.
fn cm(chars: f32) -> f32 {
    (chars * 7.0 + 5.0) / 96.0 * 2.54
}

/// An ISO date or date and time (`2026-10-04`, `2026-10-04 10:30:00`) as
/// an OpenDocument date value; a time (`10:30:00`) as a duration.
fn date_value(s: &str) -> Option<(&'static str, String)> {
    let t = s.trim();
    let date_ok = |d: &str| {
        let p: Vec<&str> = d.split('-').collect();
        p.len() == 3
            && p[0].len() == 4
            && p.iter()
                .all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()))
    };
    let time_ok = |x: &str| {
        let p: Vec<&str> = x.split(':').collect();
        (2..=3).contains(&p.len())
            && p.iter()
                .all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit() || c == '.'))
    };
    if let Some((d, tm)) = t.split_once(' ')
        && date_ok(d)
        && time_ok(tm)
    {
        let tm = if tm.matches(':').count() == 1 {
            format!("{tm}:00")
        } else {
            tm.to_owned()
        };
        return Some(("date", format!("{d}T{tm}")));
    }
    if date_ok(t) {
        return Some(("date", t.to_owned()));
    }
    if time_ok(t) {
        let p: Vec<&str> = t.split(':').collect();
        return Some((
            "time",
            format!("PT{}H{}M{}S", p[0], p[1], p.get(2).unwrap_or(&"0")),
        ));
    }
    None
}

/// `src` written as an OpenDocument spreadsheet.
pub fn to_ods(src: &mut dyn ViewerDocument) -> Result<Vec<u8>, String> {
    let data = sheets(src)?;
    let mut styles: Vec<String> = Vec::new();
    let mut style_names: BTreeMap<String, String> = BTreeMap::new();
    let mut cell_style = |cell: &GridCell, date: Option<&str>| -> Option<String> {
        let mut text = String::new();
        let mut props = String::new();
        if cell.bold {
            text.push_str(r#" fo:font-weight="bold""#);
        }
        if cell.italic {
            text.push_str(r#" fo:font-style="italic""#);
        }
        if cell.underline {
            text.push_str(r#" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color""#);
        }
        if let Some([r, g, b]) = cell.color {
            text.push_str(&format!(r##" fo:color="#{r:02x}{g:02x}{b:02x}""##));
        }
        if let Some([r, g, b]) = cell.fill {
            props.push_str(&format!(
                r##" fo:background-color="#{r:02x}{g:02x}{b:02x}""##
            ));
        }
        let align = match cell.align {
            kalem_viewer::Align::Left => Some("start"),
            kalem_viewer::Align::Center => Some("center"),
            kalem_viewer::Align::Right => Some("end"),
            kalem_viewer::Align::General => None,
        };
        let data_style = match date {
            Some("date") => r#" style:data-style-name="Ndate""#,
            Some("time") => r#" style:data-style-name="Ntime""#,
            _ => "",
        };
        if text.is_empty() && props.is_empty() && align.is_none() && data_style.is_empty() {
            return None;
        }
        let mut body = String::new();
        if !props.is_empty() {
            body.push_str(&format!("<style:table-cell-properties{props}/>"));
        }
        if let Some(a) = align {
            body.push_str(&format!(
                r#"<style:paragraph-properties fo:text-align="{a}"/>"#
            ));
        }
        if !text.is_empty() {
            body.push_str(&format!("<style:text-properties{text}/>"));
        }
        let key = format!("{data_style}|{body}");
        if let Some(n) = style_names.get(&key) {
            return Some(n.clone());
        }
        let name = format!("ce{}", style_names.len() + 1);
        styles.push(format!(
            r#"<style:style style:name="{name}" style:family="table-cell" style:parent-style-name="Default"{data_style}>{body}</style:style>"#
        ));
        style_names.insert(key, name.clone());
        Some(name)
    };
    let mut col_styles: BTreeMap<String, String> = BTreeMap::new();
    let mut tables = String::new();
    for (unit, s) in &data {
        // Formulas' results, computed together.
        let formulas: Vec<((u32, u32), String)> = s
            .cells
            .iter()
            .filter(|(_, (input, _, _))| input.starts_with('='))
            .map(|(k, (input, _, _))| (*k, input.clone()))
            .collect();
        let texts: Vec<String> = formulas.iter().map(|(_, f)| f.clone()).collect();
        let results: BTreeMap<(u32, u32), Option<String>> = formulas
            .iter()
            .map(|(k, _)| *k)
            .zip(src.evaluate_formulas(*unit, &texts))
            .collect();
        let max_c = s.cells.keys().map(|k| k.1).max().unwrap_or(0);
        let max_r = s.cells.keys().map(|k| k.0).max().unwrap_or(0);
        let mut t = format!(
            r#"<table:table table:name="{}"{}>"#,
            esc(&s.name),
            if s.hidden {
                r#" table:display="false""#
            } else {
                ""
            }
        );
        for c in 0..=max_c {
            let w = s
                .layout
                .widths
                .get(c as usize)
                .copied()
                .unwrap_or(s.layout.default_width);
            let key = format!("{:.3}cm", cm(w));
            let n = col_styles.len() + 1;
            let name = col_styles
                .entry(key)
                .or_insert_with(|| format!("co{n}"))
                .clone();
            t.push_str(&format!(
                r#"<table:table-column table:style-name="{name}"/>"#
            ));
        }
        let covered = |r: u32, c: u32| {
            s.layout.merged.iter().any(|m| {
                (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c) && (m[0], m[1]) != (r, c)
            })
        };
        for r in 0..=max_r {
            t.push_str("<table:table-row>");
            let mut blank = 0;
            for c in 0..=max_c {
                if covered(r, c) {
                    if blank > 0 {
                        t.push_str(&format!(
                            r#"<table:table-cell table:number-columns-repeated="{blank}"/>"#
                        ));
                        blank = 0;
                    }
                    t.push_str("<table:covered-table-cell/>");
                    continue;
                }
                let Some((input, cell, _)) = s.cells.get(&(r, c)) else {
                    blank += 1;
                    continue;
                };
                if blank > 0 {
                    t.push_str(&format!(
                        r#"<table:table-cell table:number-columns-repeated="{blank}"/>"#
                    ));
                    blank = 0;
                }
                let mut attrs = String::new();
                if let Some(m) = s.layout.merged.iter().find(|m| (m[0], m[1]) == (r, c)) {
                    attrs.push_str(&format!(
                        r#" table:number-columns-spanned="{}" table:number-rows-spanned="{}""#,
                        m[3] - m[1] + 1,
                        m[2] - m[0] + 1
                    ));
                }
                let value = |v: &str| -> String {
                    let v = v.trim();
                    if let Ok(n) = v.parse::<f64>() {
                        format!(r#" office:value-type="float" office:value="{n}""#)
                    } else if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
                        format!(
                            r#" office:value-type="boolean" office:boolean-value="{}""#,
                            v.eq_ignore_ascii_case("true")
                        )
                    } else {
                        r#" office:value-type="string""#.to_owned()
                    }
                };
                let mut kind = None;
                if let Some(f) = input.strip_prefix('=') {
                    attrs.push_str(&format!(
                        r#" table:formula="of:={}""#,
                        esc(&open_formula(f))
                    ));
                    match results.get(&(r, c)).cloned().flatten() {
                        Some(v) if v.starts_with('"') => {
                            attrs.push_str(r#" office:value-type="string""#)
                        }
                        Some(v) => attrs.push_str(&value(&v)),
                        None => {}
                    }
                } else if cell.numeric
                    && let Some((k, v)) = date_value(input)
                {
                    kind = Some(k);
                    if k == "date" {
                        attrs.push_str(&format!(
                            r#" office:value-type="date" office:date-value="{v}""#
                        ));
                    } else {
                        attrs.push_str(&format!(
                            r#" office:value-type="time" office:time-value="{v}""#
                        ));
                    }
                } else if cell.numeric || !input.starts_with('\'') && input.parse::<f64>().is_ok() {
                    attrs.push_str(&value(input));
                } else {
                    attrs.push_str(r#" office:value-type="string""#);
                }
                if let Some(st) = cell_style(cell, kind) {
                    attrs.push_str(&format!(r#" table:style-name="{st}""#));
                }
                let shown = if cell.text.is_empty() && !input.starts_with('=') {
                    input.trim_start_matches('\'').to_owned()
                } else {
                    cell.text.clone()
                };
                t.push_str(&format!(
                    "<table:table-cell{attrs}><text:p>{}</text:p></table:table-cell>",
                    esc(&shown)
                ));
            }
            t.push_str("</table:table-row>");
        }
        t.push_str("</table:table>");
        tables.push_str(&t);
    }
    let cols: String = col_styles
        .iter()
        .map(|(w, n)| {
            format!(
                r#"<style:style style:name="{n}" style:family="table-column"><style:table-column-properties style:column-width="{w}"/></style:style>"#
            )
        })
        .collect();
    let ns = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2""#;
    let data_styles = r#"<number:date-style style:name="Ndate"><number:year number:style="long"/><number:text>-</number:text><number:month number:style="long"/><number:text>-</number:text><number:day number:style="long"/></number:date-style><number:time-style style:name="Ntime"><number:hours number:style="long"/><number:text>:</number:text><number:minutes number:style="long"/><number:text>:</number:text><number:seconds number:style="long"/></number:time-style>"#;
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {ns} office:version="1.2"><office:automatic-styles>{data_styles}{cols}{}</office:automatic-styles><office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body></office:document-content>"#,
        styles.concat()
    );
    let styles_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {ns} office:version="1.2"><office:styles><style:style style:name="Default" style:family="table-cell"/></office:styles></office:document-styles>"#
    );
    let meta = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" office:version="1.2"><office:meta><meta:generator>Kalem</meta:generator></office:meta></office:document-meta>"#;
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    Ok(zip(&[
        (
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
            false,
        ),
        ("content.xml", content.as_bytes(), true),
        ("styles.xml", styles_xml.as_bytes(), true),
        ("meta.xml", meta.as_bytes(), true),
        ("META-INF/manifest.xml", manifest.as_bytes(), true),
    ]))
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
/// sheet.
pub fn import_rows(rows: &[Vec<String>], types: &BTreeMap<usize, ColumnType>) -> Vec<Vec<String>> {
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
                        _ => v.clone(),
                    },
                )
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_in_open_formula() {
        assert_eq!(open_formula("SUM(A1:B2,C3)"), "SUM([.A1:.B2];[.C3])");
        assert_eq!(
            open_formula("'My Sheet'!$A$1*Data!B2"),
            "['My Sheet'.$A$1]*[Data.B2]"
        );
        assert_eq!(
            open_formula("IF(A1=\"a,b\",LOG10(2),1)"),
            "IF([.A1]=\"a,b\";LOG10(2);1)"
        );
        assert_eq!(open_formula("_xlfn.CONCAT(A:A)"), "CONCAT([.A:.A])");
        assert_eq!(open_formula("SUM({1,2;3,4})"), "SUM({1;2|3;4})");
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
            import_rows(&rows, &types),
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

    #[test]
    fn dates_and_zip() {
        assert_eq!(
            date_value("2026-10-04"),
            Some(("date", "2026-10-04".into()))
        );
        assert_eq!(
            date_value("2026-10-04 10:30"),
            Some(("date", "2026-10-04T10:30:00".into()))
        );
        assert_eq!(date_value("10:30:00"), Some(("time", "PT10H30M00S".into())));
        assert_eq!(date_value("1200"), None);
        let z = zip(&[("a.txt", b"hello", false), ("b.txt", b"world world", true)]);
        assert_eq!(&z[..4], b"PK\x03\x04");
        assert!(z.windows(5).any(|w| w == b"hello"));
        let back = unzip(&z).unwrap();
        assert_eq!(back[1], ("b.txt".to_string(), b"world world".to_vec()));
        let tpl = zip(&[("[Content_Types].xml", br#"<Override ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml"/>"#, true)]);
        let wb = unzip(&template_to_workbook(&tpl).unwrap()).unwrap();
        assert!(String::from_utf8_lossy(&wb[0].1).contains("spreadsheetml.sheet.main+xml"));
        assert_eq!(entry("2026-10-04"), Some(Entry::Date(46299.0, 14)));
        assert_eq!(entry("'007"), Some(Entry::Text("007".into())));
        assert_eq!(entry("=A1"), Some(Entry::Formula("A1".into())));
        assert_eq!(entry("1.5"), Some(Entry::Number(1.5)));
    }
}
