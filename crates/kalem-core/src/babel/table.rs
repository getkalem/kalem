//! A program's output read as a table (`org-babel-import-elisp-from-file`
//! over `org-table-convert-region` with `babel-auto`), and a value
//! written as an Org table (`orgtbl-to-orgtbl`, then `org-table-align`).

use super::value::{Value, string_read};

/// How `babel-auto` splits the output's lines into fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Separator {
    Tab,
    Comma,
    Blanks,
    /// One line without a tab or a comma: not split.
    None,
}

/// The separator `org-table-convert-region` picks for `lines`: a tab
/// where every line has one, a comma where every line has one, a single
/// line not split, else runs of blanks.
fn separator(lines: &[&str]) -> Separator {
    if lines.iter().all(|l| l.contains('\t')) {
        Separator::Tab
    } else if lines.iter().all(|l| l.contains(',')) {
        Separator::Comma
    } else if lines.len() == 1 {
        Separator::None
    } else {
        Separator::Blanks
    }
}

/// One CSV line's fields, a field's double quotes taken off.
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if cur.trim().is_empty() && !quoted => {
                cur.clear();
                quoted = true;
            }
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            }
            ',' if !quoted => {
                fields.push(cur.trim().to_string());
                cur.clear();
            }
            o => cur.push(o),
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

/// The rows of `text` as `org-table-convert-region` splits them.
fn rows(text: &str) -> Vec<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let sep = separator(&lines);
    lines
        .iter()
        .map(|l| match sep {
            Separator::Tab => l.split('\t').map(|f| f.trim().to_string()).collect(),
            Separator::Comma => csv_fields(l),
            Separator::None => vec![l.trim().to_string()],
            Separator::Blanks => l.split_whitespace().map(String::from).collect(),
        })
        .collect()
}

/// `org-babel-import-elisp-from-file`: `text` as a table of read cells,
/// a table of one cell its cell and a table of one row of one cell
/// that value; empty text `nil`.
pub fn import(text: &str) -> Value {
    if text.is_empty() {
        return Value::nil();
    }
    let table: Vec<Value> = rows(text)
        .into_iter()
        .filter(|r| !r.is_empty() && !(r.len() == 1 && r[0].is_empty()))
        .map(|r| Value::List(r.iter().map(|c| string_read(c)).collect()))
        .collect();
    match table.as_slice() {
        [Value::List(row)] if row.len() == 1 => row[0].clone(),
        [Value::List(row)] if row.len() > 1 => Value::List(table),
        [one] => one.clone(),
        _ => Value::List(table),
    }
}

/// Whether `v` can be written as a table (`tabulablep`): a list whose
/// elements are atoms or lists.
pub fn tabulable(v: &Value) -> bool {
    matches!(v, Value::List(items) if !items.is_empty())
}

/// `v`, a list of rows (or of atoms, one row), as an aligned Org table,
/// ending with a line feed.
pub fn org_table(v: &Value) -> String {
    let Value::List(items) = v else {
        return String::new();
    };
    let all_rows = items
        .iter()
        .all(|e| e.is_hline() || matches!(e, Value::List(_)));
    let rows: Vec<Value> = if all_rows {
        items.clone()
    } else {
        vec![v.clone()]
    };
    let mut text = String::new();
    for r in &rows {
        match r {
            r if r.is_hline() => text.push_str("|-\n"),
            Value::List(cells) => {
                let cells: Vec<String> = cells.iter().map(Value::princ).collect();
                text.push_str(&format!("| {} |\n", cells.join(" | ")));
            }
            other => text.push_str(&format!("| {} |\n", other.princ())),
        }
    }
    crate::paste::align_tables(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_read_as_emacs_reads_it() {
        assert_eq!(import("hello\n"), Value::Str("hello".into()));
        assert_eq!(import("0\n"), Value::Int(0));
        assert_eq!(
            import("a b\nc d\n"),
            Value::List(vec![
                Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]),
                Value::List(vec![Value::Str("c".into()), Value::Str("d".into())]),
            ])
        );
        // One line: not split, its blanks kept.
        assert_eq!(import("x y\n"), Value::Str("x y".into()));
        // Every line with a tab: tab-separated.
        assert_eq!(
            import("a b\t1\n"),
            Value::List(vec![Value::List(vec![
                Value::Str("a b".into()),
                Value::Int(1)
            ])])
        );
        assert_eq!(import(""), Value::nil());
    }

    #[test]
    fn tables_written_aligned() {
        let t = Value::List(vec![
            Value::List(vec![Value::Str("a".into()), Value::Int(1)]),
            Value::Symbol("hline".into()),
            Value::List(vec![Value::Str("bb".into()), Value::Int(22)]),
        ]);
        assert_eq!(org_table(&t), "| a  |  1 |\n|----+----|\n| bb | 22 |\n");
        let row = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(org_table(&row), "| 1 | 2 |\n");
    }
}
