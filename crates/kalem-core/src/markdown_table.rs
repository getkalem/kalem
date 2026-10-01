//! GitHub's tables in Markdown edited as Org tables are (T2.7c.4): Tab
//! aligns the table and goes to the next cell (a new row past the last),
//! Shift+Tab to the one before, and Align Table pads the columns. The
//! delimiter row keeps each column's alignment (`:---`, `:---:`, `---:`),
//! and cells are padded as their column is aligned.

use std::ops::Range;

use org_edit::{Selection, Transaction};
use unicode_width::UnicodeWidthStr;

use crate::markdown::{Md, MdKind};

/// How a column is aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    None,
    Left,
    Center,
    Right,
}

/// The table holding `pos`: its lines, from the start of the first to the
/// end of the last (without its line feed).
pub fn table_at(md: &Md, text: &str, pos: usize) -> Option<Range<usize>> {
    let n = md
        .nodes
        .iter()
        .find(|n| matches!(n.kind, MdKind::Table) && n.range.start <= pos && pos <= n.range.end)?;
    let start = text[..n.range.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[n.range.end..]
        .find('\n')
        .map_or(text.len(), |i| n.range.end + i);
    Some(start..end)
}

/// The cells of a table line: split at the pipes that are not escaped, the
/// leading and trailing pipe dropped, each cell trimmed.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for c in t.chars() {
        if c == '|' && !escaped {
            out.push(cell.trim().to_string());
            cell.clear();
            continue;
        }
        escaped = c == '\\' && !escaped;
        cell.push(c);
    }
    // After the trailing pipe nothing is left; without one, the last cell.
    if !cell.trim().is_empty() || !t.ends_with('|') {
        out.push(cell.trim().to_string());
    }
    out
}

fn delimiter_cell(c: &str) -> Option<Align> {
    let c = c.trim();
    let (l, r) = (c.starts_with(':'), c.ends_with(':'));
    let dashes = c.trim_matches(':');
    if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
        return None;
    }
    Some(match (l, r) {
        (true, true) => Align::Center,
        (true, false) => Align::Left,
        (false, true) => Align::Right,
        _ => Align::None,
    })
}

/// The table's text aligned: the columns padded to their widest cell, the
/// delimiter row's dashes to the width, short rows given empty cells, the
/// first line's indentation kept.
pub fn align(table: &str) -> String {
    let lines: Vec<&str> = table.split('\n').collect();
    let indent: String = lines
        .first()
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect())
        .unwrap_or_default();
    let rows: Vec<Vec<String>> = lines.iter().map(|l| cells(l)).collect();
    let aligns: Vec<Align> = rows
        .get(1)
        .map(|r| {
            r.iter()
                .map(|c| delimiter_cell(c).unwrap_or(Align::None))
                .collect()
        })
        .unwrap_or_default();
    let ncols = rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(aligns.len());
    let mut widths = vec![3usize; ncols];
    for (i, r) in rows.iter().enumerate() {
        if i == 1 {
            continue;
        }
        for (j, c) in r.iter().enumerate() {
            widths[j] = widths[j].max(c.width());
        }
    }
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let mut parts = Vec::new();
        for (j, &w) in widths.iter().enumerate() {
            let a = aligns.get(j).copied().unwrap_or(Align::None);
            if i == 1 {
                let (l, r) = match a {
                    Align::Left => (":", ""),
                    Align::Right => ("", ":"),
                    Align::Center => (":", ":"),
                    Align::None => ("", ""),
                };
                parts.push(format!("{l}{}{r}", "-".repeat(w - l.len() - r.len())));
                continue;
            }
            let c = r.get(j).map_or("", String::as_str);
            let pad = w - c.width();
            parts.push(match a {
                Align::Right => format!("{}{c}", " ".repeat(pad)),
                Align::Center => format!("{}{c}{}", " ".repeat(pad / 2), " ".repeat(pad - pad / 2)),
                _ => format!("{c}{}", " ".repeat(pad)),
            });
        }
        out.push(format!("{indent}| {} |", parts.join(" | ")));
    }
    out.join("\n")
}

/// The row and column of `pos` in the table `range` of `text`.
fn cell_of(text: &str, range: &Range<usize>, pos: usize) -> (usize, usize) {
    let before = &text[range.start..pos.clamp(range.start, range.end)];
    let row = before.matches('\n').count();
    let line = &before[before.rfind('\n').map_or(0, |i| i + 1)..];
    // The unescaped pipes before it; a leading pipe opens the first cell.
    let mut pipes = 0usize;
    let mut escaped = false;
    for c in line.chars() {
        if c == '|' && !escaped {
            pipes += 1;
        }
        escaped = c == '\\' && !escaped;
    }
    let lead = line.trim_start().starts_with('|');
    (row, if lead { pipes.saturating_sub(1) } else { pipes })
}

/// Where the text of cell (`row`, `col`) starts in an aligned table.
fn cell_start(aligned: &str, row: usize, col: usize) -> usize {
    let mut at = 0;
    for (i, line) in aligned.split('\n').enumerate() {
        if i == row {
            // After the `col + 1`-th pipe and its space.
            let mut seen = 0;
            for (k, c) in line.char_indices() {
                if c == '|' {
                    if seen == col {
                        let rest = &line[k + 1..];
                        let skip = rest.len() - rest.trim_start().len();
                        return at + k + 1 + skip.min(1);
                    }
                    seen += 1;
                }
            }
            return at + line.len();
        }
        at += line.len() + 1;
    }
    at
}

/// The table aligned and the cursor moved one cell on (`forward`) or back:
/// rows wrap, the delimiter row is skipped, and past the last cell of the
/// last row a new empty row is added.
pub fn next_field(md: &Md, text: &str, pos: usize, forward: bool) -> Option<Transaction> {
    let range = table_at(md, text, pos)?;
    let (row, col) = cell_of(text, &range, pos);
    let mut aligned = align(&text[range.clone()]);
    let rows = aligned.split('\n').count();
    let ncols = cells(aligned.split('\n').next().unwrap_or("")).len().max(1);
    let col = col.min(ncols - 1);
    let (mut r, mut c) = (row, col);
    if forward {
        c += 1;
        if c >= ncols {
            c = 0;
            r += 1;
        }
        if r == 1 {
            r = 2;
        }
        if r >= rows {
            // A new row, as Tab does in an Org table.
            let indent: String = aligned
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let empty = vec!["   "; ncols].join(" | ");
            aligned.push_str(&format!("\n{indent}| {empty} |"));
            aligned = align(&aligned);
        }
    } else if c > 0 {
        c -= 1;
    } else if r > 0 {
        r -= 1;
        if r == 1 {
            r = 0;
        }
        c = ncols - 1;
    }
    let caret = range.start + cell_start(&aligned, r, c);
    let mut new = String::with_capacity(text.len() + 16);
    new.push_str(&text[..range.start]);
    new.push_str(&aligned);
    new.push_str(&text[range.end..]);
    let tx = crate::lines::replace_differing(text, &new, "Next Field")
        .unwrap_or_else(|| Transaction::new("Next Field"));
    Some(tx.select(Selection::caret(caret)))
}

/// A change of a table's rows or columns, as Org's table keys make them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableEdit {
    /// The row at the cursor up (`true`) or down, among the body rows.
    MoveRow(bool),
    /// The column at the cursor left (`true`) or right.
    MoveColumn(bool),
    /// An empty row above the cursor's (the first body row from the
    /// header).
    InsertRow,
    /// The cursor's body row deleted.
    DeleteRow,
    /// An empty column left of the cursor's.
    InsertColumn,
    /// The cursor's column deleted (not the last one).
    DeleteColumn,
    /// The body rows sorted by the cursor's column: as numbers when they
    /// all are, else as text ignoring case; descending when `true`.
    Sort(bool),
}

/// The table at `pos` changed by `e` and aligned, the cursor in the cell
/// it went to; `None` when `e` cannot apply there (the header row does
/// not move, a body row does not go above the delimiter row).
pub fn edit_at(md: &Md, text: &str, pos: usize, e: TableEdit) -> Option<Transaction> {
    let range = table_at(md, text, pos)?;
    let (row, col) = cell_of(text, &range, pos);
    let table = &text[range.clone()];
    let indent: String = table
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let mut rows: Vec<Vec<String>> = table.split('\n').map(cells).collect();
    if rows.len() < 2 {
        return None;
    }
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    for (i, r) in rows.iter_mut().enumerate() {
        let fill = if i == 1 { "---" } else { "" };
        r.resize(ncols, fill.to_string());
    }
    let col = col.min(ncols - 1);
    let n = rows.len();
    let (r, c) = match e {
        TableEdit::MoveRow(true) if row >= 3 && row < n => {
            rows.swap(row, row - 1);
            (row - 1, col)
        }
        TableEdit::MoveRow(false) if row >= 2 && row + 1 < n => {
            rows.swap(row, row + 1);
            (row + 1, col)
        }
        TableEdit::MoveColumn(true) if col > 0 => {
            rows.iter_mut().for_each(|r| r.swap(col, col - 1));
            (row, col - 1)
        }
        TableEdit::MoveColumn(false) if col + 1 < ncols => {
            rows.iter_mut().for_each(|r| r.swap(col, col + 1));
            (row, col + 1)
        }
        TableEdit::InsertRow => {
            let at = row.max(2);
            rows.insert(at, vec![String::new(); ncols]);
            (at, col)
        }
        TableEdit::DeleteRow if row >= 2 && row < n => {
            rows.remove(row);
            let last = rows.len() - 1;
            (row.min(last).max(if last >= 2 { 2 } else { 0 }), col)
        }
        TableEdit::InsertColumn => {
            for (i, r) in rows.iter_mut().enumerate() {
                r.insert(col, if i == 1 { "---".into() } else { String::new() });
            }
            (row, col)
        }
        TableEdit::DeleteColumn if ncols > 1 => {
            rows.iter_mut().for_each(|r| {
                r.remove(col);
            });
            (row, col.min(ncols - 2))
        }
        TableEdit::Sort(reverse) if n > 3 => {
            let mut body = rows.split_off(2);
            let numbers: Option<Vec<f64>> = body
                .iter()
                .map(|r| r[col].trim().parse::<f64>().ok())
                .collect();
            match numbers {
                Some(_) => body.sort_by(|a, b| {
                    let (x, y) = (a[col].trim().parse::<f64>(), b[col].trim().parse::<f64>());
                    x.unwrap_or(0.0).total_cmp(&y.unwrap_or(0.0))
                }),
                None => body.sort_by_key(|r| r[col].to_lowercase()),
            }
            if reverse {
                body.reverse();
            }
            rows.extend(body);
            (row, col)
        }
        _ => return None,
    };
    let lines: Vec<String> = rows
        .iter()
        .map(|r| format!("{indent}| {} |", r.join(" | ")))
        .collect();
    let aligned = align(&lines.join("\n"));
    let caret = range.start + cell_start(&aligned, r, c);
    let mut new = String::with_capacity(text.len() + 16);
    new.push_str(&text[..range.start]);
    new.push_str(&aligned);
    new.push_str(&text[range.end..]);
    let tx = crate::lines::replace_differing(text, &new, "Edit Table")
        .unwrap_or_else(|| Transaction::new("Edit Table"));
    Some(tx.select(Selection::caret(caret)))
}

/// The formulas of the table `range` of `text`: the `<!-- TBLFM: … -->`
/// lines right after it, as Obsidian's Advanced Tables writes them (each
/// line one `#+TBLFM` of Org).
fn formulas_after(text: &str, range: &Range<usize>) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = (range.end + 1).min(text.len());
    while at < text.len() {
        let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
        let line = text[at..end].trim();
        let Some(f) = line
            .strip_prefix("<!--")
            .and_then(|l| l.strip_suffix("-->"))
            .map(str::trim)
            .and_then(|l| l.strip_prefix("TBLFM:"))
        else {
            break;
        };
        out.push(f.trim().to_string());
        at = end + 1;
    }
    out
}

/// The table at `pos` computed with its formulas (T2.7c.4): the
/// `<!-- TBLFM: … -->` lines after it, in Org's formula language, the
/// delimiter row standing for Org's first rule (so column formulas skip
/// the header). `Ok(None)` without formulas.
pub fn recalculate_at(md: &Md, text: &str, pos: usize) -> Result<Option<Transaction>, String> {
    use org_table::table::{Row, Table};
    let Some(range) = table_at(md, text, pos) else {
        return Ok(None);
    };
    let lines = formulas_after(text, &range);
    if lines.is_empty() {
        return Ok(None);
    }
    let mut equations = Vec::new();
    for l in &lines {
        let t = org_table::tblfm::parse(l);
        if let Some(d) = t.duplicates.first() {
            return Err(format!(
                "Double definition `{d}=' in TBLFM line, please fix by hand"
            ));
        }
        equations.extend(t.equations);
    }
    let table_text = &text[range.clone()];
    let rows: Vec<Row> = table_text
        .split('\n')
        .enumerate()
        .map(|(i, l)| {
            if i == 1 {
                Row::Rule
            } else {
                Row::Data(cells(l))
            }
        })
        .collect();
    let table = Table { rows };
    let env = org_table::formula::Env {
        remote: &org_table::formula::NoRemote,
        constants: &[],
        property: &|_: &str| None,
        duration_custom: Default::default(),
    };
    let (new, _) = org_table::recalc::recalculate(&table, &equations, &env).map_err(|e| e.0)?;
    let old_lines: Vec<&str> = table_text.split('\n').collect();
    let indent: String = table_text
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let out: Vec<String> = new
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| match r {
            Row::Rule => old_lines.get(i).copied().unwrap_or("|---|").to_string(),
            Row::Data(f) => {
                let f: Vec<String> = f.iter().map(|c| c.replace('|', "\\|")).collect();
                format!("{indent}| {} |", f.join(" | "))
            }
        })
        .collect();
    let aligned = align(&out.join("\n"));
    let mut whole = String::with_capacity(text.len() + 16);
    whole.push_str(&text[..range.start]);
    whole.push_str(&aligned);
    whole.push_str(&text[range.end..]);
    let caret = pos.min(range.start + aligned.len());
    Ok(Some(
        crate::lines::replace_differing(text, &whole, "Recalculate Table")
            .unwrap_or_else(|| Transaction::new("Recalculate Table"))
            .select(Selection::caret(caret)),
    ))
}

/// The table at `pos` aligned, `None` when it already is.
pub fn align_at(md: &Md, text: &str, pos: usize) -> Option<Transaction> {
    let range = table_at(md, text, pos)?;
    let aligned = align(&text[range.clone()]);
    let mut new = String::with_capacity(text.len());
    new.push_str(&text[..range.start]);
    new.push_str(&aligned);
    new.push_str(&text[range.end..]);
    crate::lines::replace_differing(text, &new, "Align Table")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, tx: &Transaction) -> String {
        let mut s = text.to_string();
        for e in tx.edits.iter().rev() {
            s.replace_range(e.range.clone(), &e.insert);
        }
        s
    }

    #[test]
    fn formulas_as_obsidian_writes_them() {
        let text = "| item | n | price | total |\n|---|--:|--:|--:|\n| a | 2 | 3 | |\n| b | 4 | 1.5 | |\n| sum | | | |\n<!-- TBLFM: $4=$2*$3 -->\n<!-- TBLFM: @>$4=vsum(@I..@II) -->\n\nAfter.\n";
        let md = Md::parse(text);
        let tx = recalculate_at(&md, text, 3).unwrap().unwrap();
        let t = apply(text, &tx);
        let rows: Vec<&str> = t.lines().collect();
        assert!(rows[2].ends_with("|     6 |"), "{t}");
        // As Emacs's Calc writes a float.
        assert!(rows[3].ends_with("|    6. |"), "{t}");
        assert!(
            rows[4].contains("| sum ") && rows[4].ends_with("|   12. |"),
            "{t}"
        );
        assert!(
            t.contains("<!-- TBLFM: $4=$2*$3 -->\n<!-- TBLFM: @>$4=vsum(@I..@II) -->\n\nAfter.\n")
        );
        // Without formulas, nothing.
        let plain = "| a |\n|---|\n| 1 |\n";
        assert!(
            recalculate_at(&Md::parse(plain), plain, 2)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rows_and_columns_as_in_org() {
        let text = "Intro\n\n| n | name |\n|--:|:--|\n| 2 | beta |\n| 10 | Alpha |\n| 1 | gamma |\n\nAfter\n";
        let md = Md::parse(text);
        let at = |cell: &str| text.find(cell).unwrap();
        let run = |pos: usize, e: TableEdit| {
            let tx = edit_at(&md, text, pos, e).expect("applies");
            (apply(text, &tx), tx.selection_after.unwrap().head)
        };
        let table = |s: &str| s.split("\n\n").nth(1).unwrap().to_string();
        // A body row up, and the cursor with it.
        let (t, c) = run(at("10"), TableEdit::MoveRow(true));
        assert_eq!(
            table(&t),
            "|   n | name  |\n| --: | :---- |\n|  10 | Alpha |\n|   2 | beta  |\n|   1 | gamma |"
        );
        assert!(t[c..].trim_start().starts_with("10 |"));
        // The first body row does not go above the delimiter row, nor the
        // header down.
        assert!(edit_at(&md, text, at("beta"), TableEdit::MoveRow(true)).is_none());
        assert!(edit_at(&md, text, at("name"), TableEdit::MoveRow(false)).is_none());
        // Columns move with their alignment.
        let (t, _) = run(at("beta"), TableEdit::MoveColumn(true));
        assert!(
            table(&t).starts_with("| name  |   n |\n| :---- | --: |"),
            "{t}"
        );
        // Insert and delete.
        let (t, _) = run(at("beta"), TableEdit::InsertRow);
        assert_eq!(table(&t).lines().nth(2), Some("|     |       |"));
        let (t, _) = run(at("beta"), TableEdit::DeleteRow);
        assert!(!t.contains("beta"));
        let (t, _) = run(at("beta"), TableEdit::InsertColumn);
        assert!(
            table(&t).starts_with("|   n |     | name  |\n| --: | --- | :---- |"),
            "{t}"
        );
        let (t, _) = run(at("beta"), TableEdit::DeleteColumn);
        assert!(!t.contains("beta") && t.contains("|  10 |"));
        // Sorting: numbers as numbers, text ignoring case.
        let (t, _) = run(at("10"), TableEdit::Sort(false));
        let tt = table(&t);
        let firsts: Vec<&str> = tt
            .lines()
            .skip(2)
            .map(|l| l.split('|').nth(1).unwrap().trim())
            .collect();
        assert_eq!(firsts, ["1", "2", "10"]);
        let (t, _) = run(at("10"), TableEdit::Sort(true));
        assert!(table(&t).lines().nth(2).unwrap().contains("10"));
        let tx = edit_at(&md, text, at("name |"), TableEdit::Sort(false)).unwrap();
        let t = apply(text, &tx);
        let names: Vec<String> = table(&t)
            .lines()
            .skip(2)
            .map(|l| l.split('|').nth(2).unwrap().trim().to_string())
            .collect();
        assert_eq!(names, ["Alpha", "beta", "gamma"]);
        // Outside the text around the table, nothing changes.
        assert!(t.starts_with("Intro\n\n") && t.ends_with("\n\nAfter\n"));
    }

    #[test]
    fn aligning_keeps_the_alignments() {
        let t = "| a | long header |\n|:-|--:|\n| wide cell | 1 |\n| x |";
        assert_eq!(
            align(t),
            "| a         | long header |\n| :-------- | ----------: |\n| wide cell |           1 |\n| x         |             |"
        );
        // Escaped pipes stay inside their cell; rows without outer pipes.
        assert_eq!(
            align("a | b\n---|---\nc \\| d | e"),
            "| a      | b   |\n| ------ | --- |\n| c \\| d | e   |"
        );
        assert_eq!(
            align("| ç | ü |\n|:-:|-|\n| İİ | x |"),
            "|  ç  | ü   |\n| :-: | --- |\n| İİ  | x   |"
        );
    }

    #[test]
    fn tab_moves_through_the_cells() {
        let t = "Intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nAfter\n";
        let md = Md::parse(t);
        let at = t.find("| a").unwrap() + 2;
        let tx = next_field(&md, t, at, true).unwrap();
        let s = apply(t, &tx);
        assert_eq!(
            s,
            "Intro\n\n| a   | b   |\n| --- | --- |\n| 1   | 2   |\n\nAfter\n"
        );
        let caret = tx.selection_after.unwrap().head;
        assert_eq!(&s[caret..caret + 1], "b");
        // From the last cell of the header to the first of the next row.
        let md = Md::parse(&s);
        let tx = next_field(&md, &s, caret, true).unwrap();
        let caret = tx.selection_after.unwrap().head;
        assert_eq!(&s[caret..caret + 1], "1");
        // Past the last cell: a new row.
        let at = s.find("2  ").unwrap();
        let tx = next_field(&md, &s, at, true).unwrap();
        let s2 = apply(&s, &tx);
        assert_eq!(
            s2,
            "Intro\n\n| a   | b   |\n| --- | --- |\n| 1   | 2   |\n|     |     |\n\nAfter\n"
        );
        // Back again.
        let md = Md::parse(&s);
        let tx = next_field(&md, &s, at, false).unwrap();
        let caret = tx.selection_after.unwrap().head;
        assert_eq!(&s[caret..caret + 1], "1");
        assert!(next_field(&md, &s, 0, true).is_none());
        assert!(align_at(&md, &s, at).is_none());
    }
}
