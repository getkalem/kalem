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

/// Sets field `col` of `rec` to `v` in `tx`, padding a short record.
fn set(tx: &mut Transaction, rec: &Record, col: usize, v: &str, d: &Dialect) {
    let enc = encode(v, d);
    match rec.fields.get(col) {
        Some(f) => {
            let _ = tx.replace(f.range.clone(), enc);
        }
        None => {
            let pad = d
                .delimiter_char()
                .to_string()
                .repeat(col + 1 - rec.fields.len());
            let _ = tx.replace(rec.range.end..rec.range.end, format!("{pad}{enc}"));
        }
    }
}

/// The value after `v` in a series stepping by `step`: a number (decimals
/// kept as the first value writes them), or text ending in a number
/// (`Item 9` → `Item 10`).
pub fn next_in_series(v: &str, step: f64, comma_decimal: bool) -> Option<String> {
    // Leading zeros make an identifier, continued as text below.
    let t = v.trim();
    let zero_padded = t.len() > 1 && t.starts_with('0') && t.bytes().all(|b| b.is_ascii_digit());
    if let Some(x) = number(v, comma_decimal).filter(|_| !zero_padded) {
        let decimals = v
            .trim()
            .rsplit_once(['.', ','])
            .filter(|(_, frac)| frac.len() != 3 || !frac.bytes().all(|b| b.is_ascii_digit()))
            .map_or(0, |(_, frac)| frac.len());
        let y = x + step;
        let s = format!("{y:.decimals$}");
        return Some(if comma_decimal && decimals > 0 {
            s.replace('.', ",")
        } else {
            s
        });
    }
    let digits = v.len() - v.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || step.fract() != 0.0 {
        return None;
    }
    let (head, tail) = v.split_at(v.len() - digits);
    let n: i64 = tail.parse().ok()?;
    let m = n + step as i64;
    if m < 0 {
        return None;
    }
    // Leading zeros kept: `007` → `008`.
    Some(format!("{head}{m:0digits$}"))
}

/// Fills column `col` of the records `first + 1 ..= last` from record
/// `first`: its value copied (fill down), or a series continuing it by
/// `step` (fill series). `None` when a series cannot continue the value.
pub fn fill(
    text: &str,
    d: &Dialect,
    col: usize,
    first: usize,
    last: usize,
    series: Option<f64>,
) -> Option<Transaction> {
    let recs = records(text, d);
    let top = recs.get(first)?;
    let start = top
        .fields
        .get(col)
        .map(|f| value(text, f, d).into_owned())?;
    let comma = d.delimiter == b';';
    let mut tx = Transaction::new(if series.is_some() {
        "Fill Series"
    } else {
        "Fill Down"
    });
    let mut v = start;
    for rec in recs.iter().take(last + 1).skip(first + 1) {
        if let Some(step) = series {
            v = next_in_series(&v, step, comma)?;
        }
        set(&mut tx, rec, col, &v, d);
    }
    Some(tx)
}

/// The step of a series from the two values above a cell: their
/// difference when both are numbers, else 1.
pub fn series_step(above2: Option<&str>, above: &str, comma_decimal: bool) -> f64 {
    match (
        above2.and_then(|s| number(s, comma_decimal)),
        number(above, comma_decimal),
    ) {
        (Some(a), Some(b)) => b - a,
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
            // The record with the line ending before it.
            let _ = tx.replace(recs[i - 1].range.end..r.range.end, "");
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

/// Sort keys from text: columns by letter (`B`) or number (`2`), a `-`
/// before one for descending: `B, -A` or `2 -1`.
pub fn parse_sort_keys(s: &str) -> Option<Vec<(usize, bool)>> {
    let keys: Vec<(usize, bool)> = s
        .split([',', ' ', ';'])
        .filter(|t| !t.is_empty())
        .map(|t| {
            let (rev, t) = match t.strip_prefix('-') {
                Some(t) => (true, t),
                None => (false, t),
            };
            Some((column_index(t)?, rev))
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
    String::from_utf8(s).expect("ASCII")
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
    let rows: Vec<Vec<String>> = recs.iter().map(|r| values(text, r, d)).collect();
    let first = usize::from(d.header);
    let (mut order, blanks): (Vec<usize>, Vec<usize>) =
        (first..recs.len()).partition(|&i| !blank(&recs[i]));
    let comma = d.delimiter == b';';
    order.sort_by(|&a, &b| {
        for &(col, rev) in keys {
            let key = |i: usize| rows[i].get(col).map_or("", String::as_str);
            let o = crate::csv::compare(key(a), key(b), comma);
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
    let _ = tx.replace(start..end, body.join(d.line_ending()));
    tx
}

/// The sum of column `col`'s numbers, the header and the record `skip`
/// left out.
pub fn column_sum(text: &str, d: &Dialect, col: usize, skip: Option<usize>) -> Option<f64> {
    let recs = records(text, d);
    let comma = d.delimiter == b';';
    let nums: Vec<f64> = recs
        .iter()
        .enumerate()
        .skip(usize::from(d.header))
        .filter(|(i, _)| Some(*i) != skip)
        .filter_map(|(_, r)| number(&value(text, r.fields.get(col)?, d), comma))
        .collect();
    (!nums.is_empty()).then(|| nums.iter().sum())
}

/// The rows of a block from the clipboard: lines of fields separated by
/// tabs (what spreadsheets copy), or by the dialect's delimiter when no
/// line has a tab.
pub fn block_rows(block: &str, d: &Dialect) -> Vec<Vec<String>> {
    let block = block.strip_suffix('\n').unwrap_or(block);
    let block = block.strip_suffix('\r').unwrap_or(block);
    if block.contains('\t') {
        block
            .split('\n')
            .map(|l| {
                l.strip_suffix('\r')
                    .unwrap_or(l)
                    .split('\t')
                    .map(str::to_string)
                    .collect()
            })
            .collect()
    } else {
        crate::csv::rows(block, d)
    }
}

/// Paste as Block: the clipboard's rows written over the cells from row
/// `row`, column `col` down and to the right, as a spreadsheet pastes a
/// range; rows past the end are added. Only the cells the block covers
/// change.
pub fn paste_block(
    text: &str,
    d: &Dialect,
    row: usize,
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
        match index.record(text, row + i, d) {
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
        let end = text.trim_end_matches(['\n', '\r']).len();
        tx.replace(end..end, added).ok()?;
    }
    Some(tx)
}

#[cfg(test)]
mod tests {

    #[test]
    fn paste_block_writes_over_cells() {
        let d = Dialect::default();
        let t = "a,b,c\n1,2,3\n4,5,6\n";
        let block = block_rows("x\ty\nz\tw\n", &d);
        assert_eq!(block, vec![vec!["x", "y"], vec!["z", "w"]]);
        let tx = paste_block(t, &d, 1, 1, &block).unwrap();
        assert_eq!(tx.apply(t), "a,b,c\n1,x,y\n4,z,w\n");
        // Past the last column and the last row.
        let tx = paste_block(t, &d, 2, 2, &block).unwrap();
        assert_eq!(tx.apply(t), "a,b,c\n1,2,3\n4,5,x,y\n,,z,w\n");
        // A value that needs quotes gets them.
        let tx = paste_block(t, &d, 0, 0, &[vec!["p, q".into()]]).unwrap();
        assert_eq!(tx.apply(t), "\"p, q\",b,c\n1,2,3\n4,5,6\n");
        // CSV on the clipboard, without tabs.
        assert_eq!(block_rows("1,\"2,3\"\n", &d), vec![vec!["1", "2,3"]]);
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
        let tx = fill(t, &d, 1, 1, 3, None).unwrap();
        assert_eq!(run(t, &tx), "name,n\na,1\nb,1\nc,1\n");
        let tx = fill(t, &d, 1, 1, 3, Some(1.0)).unwrap();
        assert_eq!(run(t, &tx), "name,n\na,1\nb,2\nc,3\n");
        assert_eq!(only_cells(&tx), ["2", "3"]);
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
        let keys = parse_sort_keys("B, -A").unwrap();
        assert_eq!(keys, [(1, false), (0, true)]);
        assert_eq!(run(t, &sort_by(t, &d, &keys)), "k,v\nc,1\nb,2\na,2\n");
        assert_eq!(parse_sort_keys("2 1").unwrap(), [(1, false), (0, false)]);
        assert!(parse_sort_keys("?").is_none());
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
        assert_eq!(column_sum(t, &d, 0, None), Some(3.5));
        assert_eq!(column_sum(t, &d, 0, Some(1)), Some(2.5));
    }
}
