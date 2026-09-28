//! A table as the formula engine sees it: its lines, each a horizontal
//! rule or a row of fields, and what `org-table-analyze` finds in them
//! (data lines, rules, column names, parameters, named fields).

/// A line of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A horizontal rule, `|---+---|`.
    Rule,
    /// A row of fields, without the blanks around them.
    Data(Vec<String>),
}

/// The lines of a table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// One per line, from the top.
    pub rows: Vec<Row>,
}

/// `org-split-string` of a table line at ` *| *`, after `org-trim`.
pub fn split_fields(line: &str) -> Vec<String> {
    let t = line.trim_matches(|c: char| c.is_whitespace());
    let mut out: Vec<String> = t
        .split('|')
        .map(|f| f.trim_matches(' ').to_string())
        .collect();
    // Separators at the start and the end are ignored.
    if t.starts_with('|') {
        out.remove(0);
    }
    if t.ends_with('|') && !out.is_empty() {
        out.pop();
    }
    out
}

impl Table {
    /// Reads the lines of an Org table, the lines that start (after
    /// blanks) with `|`; it stops at the first other line.
    pub fn parse(text: &str) -> Table {
        let mut rows = Vec::new();
        for line in text.lines() {
            let t = line.trim_start_matches([' ', '\t']);
            if !t.starts_with('|') {
                break;
            }
            if t.starts_with("|-") {
                rows.push(Row::Rule);
            } else {
                rows.push(Row::Data(split_fields(t)));
            }
        }
        Table { rows }
    }

    /// The fields of line `line`, if it is a row.
    pub fn fields(&self, line: usize) -> Option<&[String]> {
        match self.rows.get(line) {
            Some(Row::Data(f)) => Some(f),
            _ => None,
        }
    }

    /// The text of field `col` (1-based) of line `line`; empty beyond the
    /// last field.
    pub fn field(&self, line: usize, col: usize) -> &str {
        self.fields(line)
            .and_then(|f| f.get(col.wrapping_sub(1)))
            .map_or("", String::as_str)
    }

    /// Sets field `col` of line `line`, adding empty fields before it if
    /// needed.
    pub fn set_field(&mut self, line: usize, col: usize, value: String) {
        if let Some(Row::Data(f)) = self.rows.get_mut(line) {
            while f.len() < col {
                f.push(String::new());
            }
            f[col - 1] = value;
        }
    }

    /// The table in Org syntax, one line per row, not aligned.
    pub fn to_org(&self) -> String {
        let mut s = String::new();
        for r in &self.rows {
            match r {
                Row::Rule => s.push_str("|-|\n"),
                Row::Data(f) => {
                    s.push('|');
                    for x in f {
                        s.push(' ');
                        s.push_str(x);
                        s.push_str(" |");
                    }
                    s.push('\n');
                }
            }
        }
        s
    }
}

/// What `org-table-analyze` computes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Analysis {
    /// For each line, whether it is a rule, with an imaginary rule after
    /// the last line.
    pub is_rule: Vec<bool>,
    /// The lines of data lines: `dlines[n]` is the line of `@n` (index 0
    /// unused).
    pub dlines: Vec<usize>,
    /// The lines of rules: `hlines[n]` is the line of the `n`th rule
    /// (index 0 unused).
    pub hlines: Vec<usize>,
    /// The number of fields of the first data line.
    pub ncol: usize,
    /// Column names from the first `!` row: name and column.
    pub column_names: Vec<(String, usize)>,
    /// Parameters from `$` rows and named fields, the most recent
    /// definition first (as `assoc` finds them).
    pub parameters: Vec<(String, String)>,
    /// Fields named in `_` and `^` rows: name, line and column.
    pub named_fields: Vec<(String, usize, usize)>,
}

/// A name for columns, parameters and fields: `[a-zA-Z][_a-zA-Z0-9]*`.
pub(crate) fn is_name(s: &str) -> bool {
    let mut it = s.chars();
    it.next().is_some_and(|c| c.is_ascii_alphabetic())
        && it.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Analysis {
    /// `org-table-analyze`.
    pub fn of(t: &Table) -> Analysis {
        let mut a = Analysis::default();
        // Column names: the first row whose first field is `!`.
        if let Some(f) = t.rows.iter().find_map(|r| match r {
            Row::Data(f) if f.first().is_some_and(|x| x == "!") => Some(f),
            _ => None,
        }) {
            for (i, name) in f.iter().enumerate().skip(1) {
                if is_name(name) {
                    a.column_names.push((name.clone(), i + 1));
                }
            }
        }
        // Parameters: `$` rows, `name=value`.
        let mut params: Vec<(String, String)> = Vec::new();
        for r in &t.rows {
            if let Row::Data(f) = r
                && f.first().is_some_and(|x| x == "$")
            {
                for field in &f[1..] {
                    if let Some((name, value)) = parameter(field) {
                        params.push((name, value));
                    }
                }
            }
        }
        // Named fields: `_` names the row below, `^` the row above.
        for (i, r) in t.rows.iter().enumerate() {
            let Row::Data(f) = r else { continue };
            let Some(mark) = f.first().filter(|x| *x == "_" || *x == "^") else {
                continue;
            };
            let target = if mark == "_" {
                i + 1
            } else {
                i.wrapping_sub(1)
            };
            let values: &[String] = match t.rows.get(target) {
                Some(Row::Data(v)) if !v.is_empty() => &v[1..],
                _ => &[],
            };
            for (k, (name, v)) in f[1..].iter().zip(values).enumerate() {
                if is_name(name) {
                    params.push((name.clone(), v.clone()));
                    a.named_fields.push((name.clone(), target, k + 2));
                }
            }
        }
        params.reverse();
        a.parameters = params;
        a.named_fields.reverse();
        a.dlines.push(usize::MAX);
        a.hlines.push(usize::MAX);
        for (i, r) in t.rows.iter().enumerate() {
            let rule = matches!(r, Row::Rule);
            a.is_rule.push(rule);
            if rule {
                a.hlines.push(i);
            } else {
                a.dlines.push(i);
            }
        }
        a.is_rule.push(true);
        a.ncol = a
            .dlines
            .get(1)
            .and_then(|&l| t.fields(l))
            .map_or(0, <[String]>::len);
        a
    }

    /// The value of parameter `name`.
    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The column called `name`.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.column_names
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| *c)
    }

    /// The data line number (`@n`) of line `line`: its own if it is one,
    /// else the one below it (`above`: above it), as
    /// `org-table-line-to-dline`.
    pub fn line_to_dline(&self, line: usize, above: bool) -> Option<usize> {
        let (min, max) = (1, self.dlines.len().checked_sub(1)?);
        if max < min || self.dlines[min] > line || self.dlines[max] < line {
            return None;
        }
        match self.dlines[1..].binary_search(&line) {
            Ok(i) => Some(i + 1),
            Err(i) => Some(if above { i } else { i + 1 }),
        }
    }
}

/// `name=value` in a `$` row.
fn parameter(field: &str) -> Option<(String, String)> {
    let (name, value) = field.split_once('=')?;
    let name = name.trim_end_matches(' ');
    if !(is_name(name) || name == "%") {
        return None;
    }
    Some((name.to_string(), value.trim_start_matches(' ').to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis() {
        let t = Table::parse(
            "| ! | a | b |  |\n|---+---+---|\n|   | 1 | 2 |\n| $ | x=3 | y = 4 |\n| _ | n1 | 7x |\n|   | 5 | 6 |\n|---|\n",
        );
        let a = Analysis::of(&t);
        assert_eq!(a.column_names, vec![("a".into(), 2), ("b".into(), 3)]);
        assert_eq!(a.parameter("x"), Some("3"));
        assert_eq!(a.parameter("y"), Some("4"));
        assert_eq!(a.parameter("n1"), Some("5"));
        assert_eq!(a.named_fields, vec![("n1".into(), 5, 2)]);
        assert_eq!(a.dlines, vec![usize::MAX, 0, 2, 3, 4, 5]);
        assert_eq!(a.hlines, vec![usize::MAX, 1, 6]);
        assert_eq!(a.ncol, 4);
        assert_eq!(a.line_to_dline(1, false), Some(2));
        assert_eq!(a.line_to_dline(1, true), Some(1));
        assert_eq!(split_fields("| a |  | b c |"), ["a", "", "b c"]);
        assert_eq!(split_fields("|a|b"), ["a", "b"]);
    }
}
