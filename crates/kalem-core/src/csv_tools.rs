//! The CSV grid's spreadsheet habits as plain edits (T2.7d.9): fill down
//! and fill series, duplicate rows removed, transpose, a column split and
//! two joined, sorting by several columns, a column's sum, and cell
//! coordinates as spreadsheets (`B3`) and Org tables (`@3$2`) write them.
//! Each edit changes the cells it names and nothing else: only the user's
//! cell text is ever written into the file.

use org_edit::Transaction;

use crate::csv::{Dialect, Index, Record, encode, number, value};

fn records(text: &str, d: &Dialect) -> Vec<Record> {
    let mut idx = Index::new(text);
    let n = idx.count(text, d);
    (0..n).filter_map(|i| idx.record(text, i, d)).collect()
}

fn values(text: &str, r: &Record, d: &Dialect) -> Vec<String> {
    r.fields
        .iter()
        .map(|f| value(text, f, d).into_owned())
        .collect()
}

/// A blank line: one empty field.
fn blank(r: &Record) -> bool {
    r.range.is_empty()
}

/// How `v` writes a number's fraction: its decimal separator and the
/// digits after it; `None` for an integer, or a separator that groups
/// thousands (`1,234`; `1.234` in a file `;` delimits), as [`number`]
/// reads them (three digits after it were always taken for a group:
/// `0.125` went on as `0`).
fn fraction(v: &str, comma_decimal: bool) -> Option<(char, usize)> {
    let t = v.trim();
    let i = t.rfind(['.', ','])?;
    let sep = if t.as_bytes()[i] == b',' { ',' } else { '.' };
    let digits = &t[i + 1..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let x = number(t, comma_decimal)?;
    // A separator that groups thousands reads the same without it.
    let joined = format!("{}{digits}", &t[..i]);
    (number(&joined, comma_decimal) != Some(x)).then_some((sep, digits.len()))
}

/// An integer written as digits alone, a sign before them (without f64's
/// rounding: `9007199254740993` went on as `…992`).
fn integer(v: &str) -> Option<i128> {
    let t = v.trim();
    let digits = t.strip_prefix(['-', '+']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    t.parse().ok()
}

/// How a date is written, to write the next one so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DateForm {
    /// `2026-09-29`.
    Iso,
    /// `29.09.2026`, day first.
    Dotted,
    /// `9/29/2026`, month first.
    MonthFirst,
    /// `29/9/2026`, day first (the day past 12).
    DayFirst,
}

/// A date as spreadsheets write one in a CSV file: `2026-09-29`,
/// `29.09.2026`, `9/29/2026` (`29/9/2026`, day first, when the first part
/// is past 12), with whether its day and month have two digits.
pub(crate) fn date(v: &str) -> Option<(jiff::civil::Date, DateForm, bool)> {
    let t = v.trim();
    let (sep, form) = if t.contains('-') {
        ('-', DateForm::Iso)
    } else if t.contains('.') {
        ('.', DateForm::Dotted)
    } else {
        ('/', DateForm::MonthFirst)
    };
    let parts: Vec<&str> = t.split(sep).collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || p.len() > 4 || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let n = |i: usize| parts[i].parse::<i16>().ok();
    let (year, month, day, form) = match form {
        DateForm::Iso if parts[0].len() == 4 => (n(0)?, n(1)?, n(2)?, form),
        DateForm::Dotted if parts[2].len() == 4 => (n(2)?, n(1)?, n(0)?, form),
        DateForm::MonthFirst if parts[2].len() == 4 && n(0)? > 12 => {
            (n(2)?, n(1)?, n(0)?, DateForm::DayFirst)
        }
        DateForm::MonthFirst if parts[2].len() == 4 => (n(2)?, n(0)?, n(1)?, form),
        _ => return None,
    };
    let padded = match form {
        DateForm::Iso => parts[1].len() == 2 && parts[2].len() == 2,
        _ => parts[0].len() == 2 && parts[1].len() == 2,
    };
    let d =
        jiff::civil::Date::new(year, i8::try_from(month).ok()?, i8::try_from(day).ok()?).ok()?;
    Some((d, form, padded))
}

fn write_date(d: jiff::civil::Date, form: DateForm, padded: bool) -> String {
    let (y, m, day) = (d.year(), d.month(), d.day());
    let two = |x: i8| {
        if padded {
            format!("{x:02}")
        } else {
            x.to_string()
        }
    };
    match form {
        DateForm::Iso => format!("{y:04}-{}-{}", two(m), two(day)),
        DateForm::Dotted => format!("{}.{}.{y:04}", two(day), two(m)),
        DateForm::MonthFirst => format!("{}/{}/{y:04}", two(m), two(day)),
        DateForm::DayFirst => format!("{}/{}/{y:04}", two(day), two(m)),
    }
}

/// Text ending in a number, as its text before the number and the number
/// (`Item 9`, `007`).
fn counted(v: &str) -> Option<(&str, i64, usize)> {
    let digits = v.len() - v.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let (head, tail) = v.split_at(v.len() - digits);
    Some((head, tail.parse().ok()?, digits))
}

/// The value after `v` in a series stepping by `step`: a number (as many
/// decimals as it or the step has, its decimal separator kept), a date
/// (`step` days on), or text ending in a number (`Item 9` → `Item 10`).
pub fn next_in_series(v: &str, step: f64, comma_decimal: bool) -> Option<String> {
    // Leading zeros make an identifier, continued as text below.
    let t = v.trim();
    let zero_padded = t.len() > 1 && t.starts_with('0') && t.bytes().all(|b| b.is_ascii_digit());
    let whole = step.fract() == 0.0 && step.abs() < 9e15;
    if let (Some(n), true) = (integer(v), whole) {
        return Some((n + step as i128).to_string());
    }
    if let Some(x) = number(v, comma_decimal).filter(|_| !zero_padded) {
        let own = fraction(v, comma_decimal);
        // The step's decimals, as its shortest form writes them.
        let step_decimals = format!("{step}")
            .split_once('.')
            .map_or(0, |(_, f)| f.len());
        let decimals = own.map_or(0, |(_, n)| n).max(step_decimals);
        let sep = own.map_or(if comma_decimal { ',' } else { '.' }, |(c, _)| c);
        let s = format!("{:.decimals$}", x + step);
        return Some(if sep == ',' { s.replace('.', ",") } else { s });
    }
    if let Some((d, form, padded)) = date(v) {
        if !whole {
            return None;
        }
        let next = d.checked_add(jiff::Span::new().days(step as i64)).ok()?;
        return Some(write_date(next, form, padded));
    }
    let (head, n, digits) = counted(v)?;
    if !whole {
        return None;
    }
    let m = n.checked_add(step as i64)?;
    if m < 0 {
        return None;
    }
    // Leading zeros kept: `007` → `008`.
    Some(format!("{head}{m:0digits$}"))
}

/// Fills columns `cols` (first and last, inclusive) of the records
/// `rows[1..]` from record `rows[0]` (the rows the grid shows, in its
/// order): each column's value copied (fill down; a cell the first record
/// lacks empties them), or a series continuing it by `step` (fill
/// series). `None` when a series cannot continue a value, or record
/// `rows[0]` has none of the columns.
pub fn fill(
    text: &str,
    d: &Dialect,
    cols: (usize, usize),
    rows: &[usize],
    series: Option<f64>,
) -> Option<Transaction> {
    let recs = records(text, d);
    let top = recs.get(*rows.first()?)?;
    // Each column's first value; a column the top record lacks stays in a
    // series, and empties the cells below in a fill.
    let starts: Vec<Option<String>> = (cols.0..=cols.1)
        .map(|c| {
            top.fields
                .get(c)
                .map(|f| value(text, f, d).into_owned())
                .or_else(|| series.is_none().then(String::new))
        })
        .collect();
    if starts.iter().all(Option::is_none) {
        return None;
    }
    let comma = d.delimiter == b';';
    let mut tx = Transaction::new(if series.is_some() {
        "Fill Series"
    } else {
        "Fill Down"
    });
    let mut values = starts;
    for rec in rows[1..].iter().filter_map(|&r| recs.get(r)) {
        if let Some(step) = series {
            for v in values.iter_mut().flatten() {
                *v = next_in_series(v, step, comma)?;
            }
        }
        // The fields the record has, then the ones it lacks in one insert
        // (two inserts at its end would overlap).
        let mut tail = String::new();
        let mut added = 0;
        let delim = d.delimiter_char();
        for (k, v) in values.iter().enumerate() {
            let col = cols.0 + k;
            match (rec.fields.get(col), v) {
                (Some(f), Some(v)) => {
                    let _ = tx.replace(f.range.clone(), encode(v, d));
                }
                (None, Some(v)) if v.is_empty() => {}
                (None, v) => {
                    for _ in added..col + 1 - rec.fields.len() {
                        tail.push(delim);
                    }
                    added = col + 1 - rec.fields.len();
                    if let Some(v) = v {
                        tail.push_str(&encode(v, d));
                    }
                }
                (Some(_), None) => {}
            }
        }
        if !tail.is_empty() {
            let _ = tx.replace(rec.range.end..rec.range.end, tail);
        }
    }
    Some(tx)
}

/// The step of a series from the two values above a cell: their
/// difference when both are numbers (rounded to their decimals), days
/// when both are dates, the difference of their numbers when both are the
/// same text ending in one (`Item 2`, `Item 1` count down); else 1.
pub fn series_step(above2: Option<&str>, above: &str, comma_decimal: bool) -> f64 {
    let Some(above2) = above2 else {
        return 1.0;
    };
    if let (Some(a), Some(b)) = (integer(above2), integer(above)) {
        return (b - a) as f64;
    }
    if let (Some(a), Some(b)) = (number(above2, comma_decimal), number(above, comma_decimal)) {
        let decimals = fraction(above2, comma_decimal)
            .map_or(0, |(_, n)| n)
            .max(fraction(above, comma_decimal).map_or(0, |(_, n)| n));
        let scale = 10f64.powi(i32::try_from(decimals).unwrap_or(15).min(15));
        return ((b - a) * scale).round() / scale;
    }
    if let (Some((a, ..)), Some((b, ..))) = (date(above2), date(above)) {
        return (b - a).get_days() as f64;
    }
    match (counted(above2), counted(above)) {
        (Some((h1, a, _)), Some((h2, b, _))) if h1 == h2 => (b - a) as f64,
        _ => 1.0,
    }
}

/// Removes the data records equal, field by field, to an earlier one
/// (blank lines and the header stay), and how many went.
pub fn remove_duplicates(text: &str, d: &Dialect) -> (Transaction, usize) {
    let recs = records(text, d);
    let mut seen = std::collections::HashSet::new();
    let mut tx = Transaction::new("Remove Duplicate Rows");
    let mut removed = 0;
    for (i, r) in recs.iter().enumerate().skip(usize::from(d.header)) {
        if blank(r) {
            continue;
        }
        if !seen.insert(values(text, r, d)) && i > 0 {
            // The record with the line ending before it; the last record,
            // after a blank line, without (that line ending is the blank
            // line's, which went with it).
            let start = if blank(&recs[i - 1]) && r.next <= r.range.end {
                r.range.start
            } else {
                recs[i - 1].range.end
            };
            let _ = tx.replace(start..r.range.end, "");
            removed += 1;
        }
    }
    (tx, removed)
}

/// Rewrites the records `recs` as `rows`, from the first record to the
/// last (a `sep=` line before them stays).
fn rewrite(recs: &[Record], rows: &[Vec<String>], d: &Dialect, label: &str) -> Transaction {
    let delim = d.delimiter_char().to_string();
    let body: Vec<String> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| encode(v, d))
                .collect::<Vec<_>>()
                .join(&delim)
        })
        .collect();
    let start = recs.first().map_or(0, |r| r.range.start);
    let end = recs.last().map_or(0, |r| r.range.end);
    let mut tx = Transaction::new(label);
    let _ = tx.replace(start..end, body.join(d.line_ending()));
    tx
}

/// Rows become columns; short rows are padded with empty fields.
pub fn transpose(text: &str, d: &Dialect) -> Transaction {
    let recs: Vec<Record> = records(text, d).into_iter().filter(|r| !blank(r)).collect();
    let rows: Vec<Vec<String>> = recs.iter().map(|r| values(text, r, d)).collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let cols: Vec<Vec<String>> = (0..width)
        .map(|j| {
            rows.iter()
                .map(|r| r.get(j).cloned().unwrap_or_default())
                .collect()
        })
        .collect();
    let all = records(text, d);
    rewrite(&all, &cols, d, "Transpose")
}

/// Splits column `col` on `sep` into as many columns as the most parts a
/// record has; records with fewer parts get empty fields.
pub fn split_column(text: &str, d: &Dialect, col: usize, sep: &str) -> Option<Transaction> {
    if sep.is_empty() {
        return None;
    }
    let recs = records(text, d);
    let parts = |r: &Record| -> Option<Vec<String>> {
        let f = r.fields.get(col)?;
        Some(value(text, f, d).split(sep).map(str::to_string).collect())
    };
    let n = recs.iter().filter_map(parts).map(|p| p.len()).max()?;
    if n < 2 {
        return None;
    }
    let delim = d.delimiter_char().to_string();
    let mut tx = Transaction::new("Split Column");
    for r in &recs {
        if blank(r) {
            continue;
        }
        let Some(mut p) = parts(r) else { continue };
        p.resize(n, String::new());
        let enc: Vec<String> = p.iter().map(|v| encode(v, d)).collect();
        let _ = tx.replace(r.fields[col].range.clone(), enc.join(&delim));
    }
    Some(tx)
}

/// Joins column `col` and the one after it with `sep` between their
/// values.
pub fn join_columns(text: &str, d: &Dialect, col: usize, sep: &str) -> Option<Transaction> {
    let recs = records(text, d);
    let mut tx = Transaction::new("Join Columns");
    let mut any = false;
    for r in &recs {
        let (Some(a), Some(b)) = (r.fields.get(col), r.fields.get(col + 1)) else {
            continue;
        };
        let (va, vb) = (value(text, a, d), value(text, b, d));
        let joined = match (va.is_empty(), vb.is_empty()) {
            (_, true) => va.into_owned(),
            (true, _) => vb.into_owned(),
            _ => format!("{va}{sep}{vb}"),
        };
        let _ = tx.replace(a.range.start..b.range.end, encode(&joined, d));
        any = true;
    }
    any.then_some(tx)
}

/// Sort keys from text: columns by a header's name (`names`, without
/// case), by letter (`B`) or by number (`2`), a `-` before one for
/// descending: `B, -A`, `2 -1` or `city -age`. `None` when one names no
/// column of the `columns` there are (a word was read as letters far
/// past the last column, and the file sorted by nothing).
pub fn parse_sort_keys(s: &str, names: &[String], columns: usize) -> Option<Vec<(usize, bool)>> {
    let keys: Vec<(usize, bool)> = s
        .split([',', ' ', ';'])
        .filter(|t| !t.is_empty())
        .map(|t| {
            let (rev, t) = match t.strip_prefix('-') {
                Some(t) => (true, t),
                None => (false, t),
            };
            let named = names
                .iter()
                .position(|n| crate::csv::fold(n.trim()) == crate::csv::fold(t));
            let col = named.or_else(|| column_index(t))?;
            (col < columns).then_some((col, rev))
        })
        .collect::<Option<_>>()?;
    (!keys.is_empty()).then_some(keys)
}

/// A column from its letters (`A`, `AB`) or its number from 1.
pub fn column_index(s: &str) -> Option<usize> {
    let s = s.trim();
    if let Ok(n) = s.parse::<usize>() {
        return n.checked_sub(1);
    }
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut n = 0usize;
    for b in s.to_ascii_uppercase().bytes() {
        n = n.checked_mul(26)?.checked_add(usize::from(b - b'A') + 1)?;
    }
    Some(n - 1)
}

/// A column's letters: 0 is `A`, 26 is `AA`.
pub fn column_letters(mut col: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    s.reverse();
    s.into_iter().map(char::from).collect()
}

/// A cell as a spreadsheet and an Org table name it: row and column from
/// 0, `B3` and `@3$2`.
pub fn coordinates(row: usize, col: usize) -> (String, String) {
    (
        format!("{}{}", column_letters(col), row + 1),
        format!("@{}${}", row + 1, col + 1),
    )
}

/// A cell named as `B3`, `@3$2`, `3,2` or `3 2` (row, column), as the row
/// and column from 0.
pub fn parse_cell(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('@') {
        let (r, c) = rest.split_once('$')?;
        return Some((
            r.trim().parse::<usize>().ok()?.checked_sub(1)?,
            c.trim().parse::<usize>().ok()?.checked_sub(1)?,
        ));
    }
    if let Some((r, c)) = s.split_once([',', ' ']) {
        return Some((
            r.trim().parse::<usize>().ok()?.checked_sub(1)?,
            c.trim().parse::<usize>().ok()?.checked_sub(1)?,
        ));
    }
    let digits = s.len()
        - s.trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .len();
    let (letters, row) = s.split_at(digits);
    let col = column_index(letters).filter(|_| !letters.is_empty())?;
    let row = row.parse::<usize>().ok()?.checked_sub(1)?;
    Some((row, col))
}

/// Sorts the data records by several columns in order, each ascending or
/// descending (the header stays first, blank lines last), each record's
/// bytes kept.
pub fn sort_by(text: &str, d: &Dialect, keys: &[(usize, bool)]) -> Transaction {
    let recs = records(text, d);
    let first = usize::from(d.header).min(recs.len());
    let (mut order, blanks): (Vec<usize>, Vec<usize>) =
        (first..recs.len()).partition(|&i| !blank(&recs[i]));
    let comma = d.delimiter == b';';
    let turkish = crate::l10n::language() == "tr";
    // Each record's keys, worked out once.
    let sort_keys: Vec<Vec<crate::csv::SortKey>> = recs
        .iter()
        .map(|r| {
            keys.iter()
                .map(|&(col, _)| {
                    let v = r
                        .fields
                        .get(col)
                        .map_or(Default::default(), |f| value(text, f, d));
                    crate::csv::sort_key(&v, comma, turkish)
                })
                .collect()
        })
        .collect();
    order.sort_by(|&a, &b| {
        for (k, &(_, rev)) in keys.iter().enumerate() {
            let o = sort_keys[a][k].cmp(&sort_keys[b][k]);
            let o = if rev { o.reverse() } else { o };
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        std::cmp::Ordering::Equal
    });
    let mut all: Vec<usize> = (0..first).collect();
    all.extend(order);
    all.extend(blanks);
    let body: Vec<&str> = all.iter().map(|&i| &text[recs[i].range.clone()]).collect();
    let start = recs.first().map_or(0, |r| r.range.start);
    let end = recs.last().map_or(0, |r| r.range.end);
    let mut tx = Transaction::new("Sort File");
    let _ = tx.replace(
        start..end,
        crate::csv::records_text(&body, d.line_ending(), end >= text.len()),
    );
    tx
}

/// The sum of column `col`'s numbers, the header and the record `skip`
/// left out, and with `kept` (a filter's) the records it leaves out.
pub fn column_sum(
    text: &str,
    d: &Dialect,
    col: usize,
    skip: Option<usize>,
    kept: Option<&[bool]>,
) -> Option<f64> {
    let recs = records(text, d);
    let comma = d.delimiter == b';';
    let nums: Vec<f64> = recs
        .iter()
        .enumerate()
        .skip(usize::from(d.header))
        .filter(|(i, _)| Some(*i) != skip)
        .filter(|(i, _)| kept.is_none_or(|k| k.get(*i).copied().unwrap_or(true)))
        .filter_map(|(_, r)| crate::csv::quantity(&value(text, r.fields.get(col)?, d), comma))
        .collect();
    (!nums.is_empty()).then(|| nums.iter().sum())
}

/// The rows of a block from the clipboard: what spreadsheets copy, fields
/// separated by tabs and a value with a line break, a tab or a quote in
/// double quotes (read as TSV, quotes and all); one line without a tab, a
/// single value (`Smith, John`); lines without tabs, CSV in the
/// document's dialect.
pub fn block_rows(block: &str, d: &Dialect) -> Vec<Vec<String>> {
    let block = block.strip_suffix('\n').unwrap_or(block);
    let block = block.strip_suffix('\r').unwrap_or(block);
    if block.contains('\t') {
        let tsv = Dialect {
            delimiter: b'\t',
            quote: b'"',
            header: false,
            crlf: block.contains("\r\n"),
        };
        crate::csv::rows(block, &tsv)
    } else if !block.contains('\n') {
        // A cell copied alone, in quotes for a leading quote of its own.
        let tsv = Dialect {
            delimiter: b'\t',
            ..Dialect::default()
        };
        let quoted = block.len() > 1 && block.starts_with('"') && block.ends_with('"');
        match crate::csv::rows(block, &tsv).as_slice() {
            [one] if quoted && one.len() == 1 && crate::csv::encode(&one[0], &tsv) == block => {
                vec![one.clone()]
            }
            _ => vec![vec![block.to_string()]],
        }
    } else {
        crate::csv::rows(block, d)
    }
}

/// The cells of rows `rows` (in that order) and columns `cols` (both
/// inclusive, in any order) as TSV, as spreadsheets copy a range: a tab
/// between fields, a line feed after each row, a value with a tab or a
/// line break, or starting with a quote, in quotes (they went as spaces,
/// and a leading quote was lost on the way back). Cells past a short row
/// are empty.
pub fn rectangle_tsv(text: &str, d: &Dialect, rows: &[usize], cols: (usize, usize)) -> String {
    let (c0, c1) = (cols.0.min(cols.1), cols.0.max(cols.1));
    let mut idx = Index::new(text);
    let mut out = String::new();
    for &r in rows {
        let row = idx
            .record(text, r, d)
            .map(|rec| values(text, &rec, d))
            .unwrap_or_default();
        let cells: Vec<String> = (c0..=c1)
            .map(|c| {
                row.get(c).map_or(String::new(), |v| {
                    if v.contains(['\t', '\n', '\r']) || v.starts_with('"') {
                        format!("\"{}\"", v.replace('"', "\"\""))
                    } else {
                        v.clone()
                    }
                })
            })
            .collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// Paste as Block: the clipboard's rows written over the cells of rows
/// `rows` (the rows the grid shows from the first down,
/// `csv::view_rows_from`) from column `col` to the right, as a
/// spreadsheet pastes a range; rows past the last are added. Only the
/// cells the block covers change.
pub fn paste_block(
    text: &str,
    d: &Dialect,
    rows: &[usize],
    col: usize,
    block: &[Vec<String>],
) -> Option<Transaction> {
    if block.is_empty() {
        return None;
    }
    let mut tx = Transaction::new("Paste as Block");
    let mut index = Index::new(text);
    let delim = d.delimiter_char().to_string();
    let mut added = String::new();
    let nl = if d.crlf { "\r\n" } else { "\n" };
    for (i, cells) in block.iter().enumerate() {
        match rows.get(i).and_then(|&r| index.record(text, r, d)) {
            Some(rec) => {
                // Each cell of the block, then any missing fields before it.
                let mut extra = String::new();
                for (k, v) in cells.iter().enumerate() {
                    let c = col + k;
                    match rec.fields.get(c) {
                        Some(f) => {
                            tx.replace(f.range.clone(), encode(v, d)).ok()?;
                        }
                        None => {
                            let have = rec.fields.len() + extra.matches(&delim).count();
                            extra.push_str(&delim.repeat(c + 1 - have));
                            extra.push_str(&encode(v, d));
                        }
                    }
                }
                if !extra.is_empty() {
                    let at = rec.range.end;
                    tx.replace(at..at, extra).ok()?;
                }
            }
            None => {
                added.push_str(nl);
                added.push_str(&delim.repeat(col));
                added.push_str(
                    &cells
                        .iter()
                        .map(|v| encode(v, d))
                        .collect::<Vec<_>>()
                        .join(&delim),
                );
            }
        }
    }
    if !added.is_empty() {
        // After the last record, before a Local Variables block.
        let end = text[..crate::csv::body_end(text)]
            .trim_end_matches(['\n', '\r'])
            .len();
        tx.replace(end..end, added).ok()?;
    }
    Some(tx)
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_rectangle_of_cells_as_tsv() {
        let d = Dialect::default();
        let text = "a,b,c\n1,\"x\ty\",3\n4,5\n";
        // A tab in a value: in quotes, as a spreadsheet copies it.
        assert_eq!(
            rectangle_tsv(text, &d, &[1, 2], (1, 2)),
            "\"x\ty\"\t3\n5\t\n"
        );
        assert_eq!(rectangle_tsv(text, &d, &[0], (0, 0)), "a\n");
        // Copied and pasted as a block elsewhere, the same cells.
        let block = block_rows(&rectangle_tsv(text, &d, &[0, 1, 2], (0, 1)), &d);
        assert_eq!(block, [["a", "b"], ["1", "x\ty"], ["4", "5"]]);
        // Line breaks and a leading quote come back as they were (they
        // came back as spaces, and without the quote).
        let text = "a,b\n\"\"\"hi\"\" there\",1\n\"x\ny\",2\n";
        let tsv = rectangle_tsv(text, &d, &[1, 2], (0, 1));
        assert_eq!(tsv, "\"\"\"hi\"\" there\"\t1\n\"x\ny\"\t2\n");
        assert_eq!(block_rows(&tsv, &d), [["\"hi\" there", "1"], ["x\ny", "2"]]);
        // One cell alone.
        let one = rectangle_tsv(text, &d, &[1], (0, 0));
        assert_eq!(block_rows(&one, &d), [["\"hi\" there"]]);
        assert_eq!(block_rows("\"Hello\"", &d), [["\"Hello\""]]);
    }

    #[test]
    fn paste_block_writes_over_cells() {
        let d = Dialect::default();
        let t = "a,b,c\n1,2,3\n4,5,6\n";
        let block = block_rows("x\ty\nz\tw\n", &d);
        assert_eq!(block, vec![vec!["x", "y"], vec!["z", "w"]]);
        let tx = paste_block(t, &d, &[1, 2], 1, &block).unwrap();
        assert_eq!(tx.apply(t), "a,b,c\n1,x,y\n4,z,w\n");
        // Past the last column and the last row.
        let tx = paste_block(t, &d, &[2], 2, &block).unwrap();
        assert_eq!(tx.apply(t), "a,b,c\n1,2,3\n4,5,x,y\n,,z,w\n");
        // A value that needs quotes gets them.
        let tx = paste_block(t, &d, &[0], 0, &[vec!["p, q".into()]]).unwrap();
        assert_eq!(tx.apply(t), "\"p, q\",b,c\n1,2,3\n4,5,6\n");
        // CSV on the clipboard, lines without tabs.
        assert_eq!(
            block_rows("1,\"2,3\"\n4,5\n", &d),
            vec![vec!["1", "2,3"], vec!["4", "5"]]
        );
        // One line without a tab: one value.
        assert_eq!(block_rows("Smith, John\n", &d), vec![vec!["Smith, John"]]);
        // A spreadsheet's cell with a line break and quotes, in quotes.
        assert_eq!(
            block_rows("\"two\nlines\"\t\"say \"\"hi\"\"\"\nx\ty\n", &d),
            vec![vec!["two\nlines", "say \"hi\""], vec!["x", "y"]]
        );
    }
    use super::*;
    use crate::csv::detect;

    fn run(text: &str, tx: &Transaction) -> String {
        let mut s = text.to_string();
        for e in tx.edits.iter().rev() {
            s.replace_range(e.range.clone(), &e.insert);
        }
        s
    }

    /// The bytes outside the edited ranges are those of the file.
    fn only_cells(tx: &Transaction) -> Vec<String> {
        tx.edits.iter().map(|e| e.insert.clone()).collect()
    }

    #[test]
    fn fill_down_and_series() {
        let t = "name,n\na,1\nb,\nc,\n";
        let d = detect(t);
        let tx = fill(t, &d, (1, 1), &[1, 2, 3], None).unwrap();
        assert_eq!(run(t, &tx), "name,n\na,1\nb,1\nc,1\n");
        let tx = fill(t, &d, (1, 1), &[1, 2, 3], Some(1.0)).unwrap();
        assert_eq!(run(t, &tx), "name,n\na,1\nb,2\nc,3\n");
        assert_eq!(only_cells(&tx), ["2", "3"]);
        // Several columns: each from its own first value, short records
        // padded (it filled only the cursor's column).
        let t = "a,b,c\nx,\"1,5\",z\n,,\n\n";
        let d = detect(t);
        let tx = fill(t, &d, (0, 2), &[1, 2, 3], None).unwrap();
        assert_eq!(
            run(t, &tx),
            "a,b,c\nx,\"1,5\",z\nx,\"1,5\",z\nx,\"1,5\",z\n"
        );
        // Text ending in a number, decimals, leading zeros.
        assert_eq!(next_in_series("Item 9", 1.0, false).unwrap(), "Item 10");
        assert_eq!(next_in_series("1.50", 0.25, false).unwrap(), "1.75");
        assert_eq!(next_in_series("1,5", 1.0, true).unwrap(), "2,5");
        assert_eq!(next_in_series("007", 1.0, false).unwrap(), "008");
        assert_eq!(next_in_series("abc", 1.0, false), None);
        assert_eq!(series_step(Some("2"), "5", false), 3.0);
        assert_eq!(series_step(None, "5", false), 1.0);
    }

    #[test]
    fn duplicates_go_and_nothing_else_changes() {
        let t = "a,b\n1,2\n3,4\n1,2\n\n3,4\n1,2\n";
        let d = detect(t);
        let (tx, n) = remove_duplicates(t, &d);
        assert_eq!(n, 3);
        assert_eq!(run(t, &tx), "a,b\n1,2\n3,4\n\n");
        let t = "a,b\r\n1,\"x\"\r\n1,x\r\n";
        let (tx, n) = remove_duplicates(t, &detect(t));
        assert_eq!((run(t, &tx).as_str(), n), ("a,b\r\n1,\"x\"\r\n", 1));
    }

    #[test]
    fn transpose_split_join() {
        let t = "a,b,c\n1,2\n";
        let d = detect(t);
        assert_eq!(run(t, &transpose(t, &d)), "a,1\nb,2\nc,\n");
        let t = "name,age\nAda Lovelace,36\nPlato,80\n";
        let d = detect(t);
        let tx = split_column(t, &d, 0, " ").unwrap();
        assert_eq!(run(t, &tx), "name,,age\nAda,Lovelace,36\nPlato,,80\n");
        let s = run(t, &tx);
        let tx = join_columns(&s, &d, 0, " ").unwrap();
        assert_eq!(run(&s, &tx), t);
        assert!(split_column(t, &d, 1, " ").is_none());
    }

    #[test]
    fn sorting_by_several_columns() {
        let t = "k,v\nb,2\na,2\nc,1\n";
        let d = detect(t);
        let keys = parse_sort_keys("B, -A", &[], 2).unwrap();
        assert_eq!(keys, [(1, false), (0, true)]);
        assert_eq!(run(t, &sort_by(t, &d, &keys)), "k,v\nc,1\nb,2\na,2\n");
        assert_eq!(
            parse_sort_keys("2 1", &[], 2).unwrap(),
            [(1, false), (0, false)]
        );
        // By a header's name, without case; a column past the last is none.
        let names = ["Name".to_string(), "Age".to_string()];
        assert_eq!(
            parse_sort_keys("-age name", &names, 2).unwrap(),
            [(1, true), (0, false)]
        );
        assert!(parse_sort_keys("city", &names, 2).is_none());
        assert!(parse_sort_keys("C", &names, 2).is_none());
        assert!(parse_sort_keys("?", &[], 2).is_none());
    }

    #[test]
    fn cells_by_name() {
        assert_eq!(coordinates(2, 1), ("B3".into(), "@3$2".into()));
        assert_eq!(column_letters(25), "Z");
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_index("AA"), Some(26));
        assert_eq!(parse_cell("B3"), Some((2, 1)));
        assert_eq!(parse_cell("@3$2"), Some((2, 1)));
        assert_eq!(parse_cell("3,2"), Some((2, 1)));
        assert_eq!(parse_cell("aa10"), Some((9, 26)));
        assert_eq!(parse_cell("B0"), None);
        assert_eq!(parse_cell("12"), None);
    }

    #[test]
    fn sums() {
        let t = "n\n1\n2.5\nx\n";
        let d = detect(t);
        assert_eq!(column_sum(t, &d, 0, None, None), Some(3.5));
        assert_eq!(column_sum(t, &d, 0, Some(1), None), Some(2.5));
    }
}
