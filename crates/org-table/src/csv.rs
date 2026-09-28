//! Tables from and to CSV and TSV, as `org-table-convert-region` and
//! `orgtbl-to-csv` / `orgtbl-to-tsv` make them.

use crate::table::{Row, Table};

/// How fields are separated in text made into a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    /// Tabs if every line has one, else commas (CSV) if every line has
    /// one, else runs of spaces.
    Auto,
    /// CSV: commas, with `"quoted, fields"`.
    Comma,
    /// Tabs.
    Tab,
    /// At least this many spaces, or a tab.
    Spaces(usize),
}

/// `org-table-convert-region`: `text` (whole lines) as the lines of an Org
/// table, not yet aligned.
pub fn convert(text: &str, sep: Separator) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let sep = match sep {
        Separator::Auto => {
            let every = |c: char| lines.iter().all(|l| l.is_empty() || l.contains(c));
            if every('\t') {
                Separator::Tab
            } else if every(',') {
                Separator::Comma
            } else {
                Separator::Spaces(1)
            }
        }
        s => s,
    };
    let converted: Vec<String> = lines.iter().map(|l| convert_line(l, sep)).collect();
    converted.join("\n")
}

fn convert_line(line: &str, sep: Separator) -> String {
    match sep {
        Separator::Comma => csv_line(line),
        Separator::Tab => format!("| {}", line.replace('\t', "| ")),
        Separator::Spaces(n) => {
            let n = n.max(1);
            // `^ *\| *\t *\| \{N,\}` replaced by `| `.
            let lead = line.len() - line.trim_start_matches(' ').len();
            let mut out = String::from("| ");
            let rest = &line[lead..];
            let b = rest.as_bytes();
            let mut i = 0;
            while i < b.len() {
                let spaces = b[i..].iter().take_while(|&&c| c == b' ').count();
                if b.get(i + spaces) == Some(&b'\t') {
                    let after = b[i + spaces + 1..]
                        .iter()
                        .take_while(|&&c| c == b' ')
                        .count();
                    out.push_str("| ");
                    i += spaces + 1 + after;
                    continue;
                }
                if spaces >= n {
                    out.push_str("| ");
                    i += spaces;
                    continue;
                }
                let c = rest[i..].chars().next().expect("a character");
                out.push(c);
                i += c.len_utf8();
            }
            out
        }
        Separator::Auto => unreachable!("resolved by `convert`"),
    }
}

/// The CSV loop of `org-table-convert-region` on one line.
fn csv_line(line: &str) -> String {
    let mut out = String::from("| ");
    let b = line.as_bytes();
    let mut i = 0;
    loop {
        let blanks = b[i..]
            .iter()
            .take_while(|&&c| c == b' ' || c == b'\t')
            .count();
        if i + blanks == b.len() {
            // `[ \t]*$` becomes ` |`.
            out.push_str(" |");
            return out;
        }
        if b[i + blanks] == b'"'
            && let Some(close) = line[i + blanks + 1..].find('"')
        {
            // `[ \t]*"\([^"\n]*\)"`: the text inside, and a quote when
            // another follows (a doubled quote).
            let s = i + blanks + 1;
            out.push_str(&line[s..s + close]);
            i = s + close + 1;
            if b.get(i) == Some(&b'"') {
                out.push('"');
            }
            continue;
        }
        if b[i] != b',' {
            // `[^,\n]+`: kept as it is, blanks included.
            let run = line[i..].find(',').unwrap_or(line.len() - i);
            out.push_str(&line[i..i + run]);
            i += run;
            continue;
        }
        // `[ \t]*,` (the blanks went with the text before).
        out.push_str(" | ");
        i += 1;
    }
}

/// The export formats of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Comma-separated, fields with `,` or `"` quoted.
    Csv,
    /// Tab-separated.
    Tsv,
}

/// A special marker in a first field.
fn marker(s: &str) -> bool {
    matches!(s, "/" | "#" | "!" | "$" | "*" | "_" | "^")
}

/// `<r>`, `<l10>`, `<5>`: an alignment or width cookie.
fn cookie(s: &str) -> bool {
    let Some(inner) = s.strip_prefix('<').and_then(|t| t.strip_suffix('>')) else {
        return false;
    };
    let digits = inner.trim_start_matches(['l', 'r', 'c']);
    inner.len() - digits.len() <= 1 && digits.bytes().all(|c| c.is_ascii_digit())
}

/// `orgtbl-to-csv` / `orgtbl-to-tsv` of a table: rules, special rows and
/// the special column left out (`org-export-table-row-is-special-p`), one
/// line per row, without a final line feed.
pub fn export(table: &Table, format: Format) -> String {
    let data: Vec<&Vec<String>> = table
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Data(f) => Some(f),
            Row::Rule => None,
        })
        .collect();
    // A special column: every first field empty or a marker, one a marker.
    let first = |f: &Vec<String>| f.first().map_or("", |s| s.as_str()).to_string();
    let special_column = data.iter().all(|f| {
        let s = first(f);
        s.is_empty() || marker(&s)
    }) && data.iter().any(|f| marker(&first(f)));
    let mut lines = Vec::new();
    for f in data {
        let head = first(f);
        let special_row = head == "/"
            || (special_column && matches!(head.as_str(), "^" | "_" | "$" | "!"))
            || (f.iter().all(|c| c.is_empty() || cookie(c)) && f.iter().any(|c| cookie(c)));
        if special_row {
            continue;
        }
        let cells = if special_column {
            &f[1.min(f.len())..]
        } else {
            &f[..]
        };
        let sep = match format {
            Format::Csv => ",",
            Format::Tsv => "\t",
        };
        let quoted: Vec<String> = cells
            .iter()
            .map(|c| match format {
                Format::Csv if c.contains(['"', ',']) => format!("\"{}\"", c.replace('"', "\"\"")),
                _ => c.clone(),
            })
            .collect();
        lines.push(quoted.join(sep));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converting() {
        assert_eq!(convert("a,b\n1,2", Separator::Auto), "| a | b |\n| 1 | 2 |");
        assert_eq!(convert("a\tb\n1\t2", Separator::Auto), "| a| b\n| 1| 2");
        assert_eq!(convert("a b  c", Separator::Auto), "| a| b| c");
        assert_eq!(
            convert("\"x, y\",\"say \"\"hi\"\"\",3", Separator::Comma),
            "| x, y | say \"hi\" | 3 |"
        );
        assert_eq!(convert("a  b c", Separator::Spaces(2)), "| a| b c");
    }

    #[test]
    fn exporting() {
        let t = Table::parse(
            "| ! | a | b |\n|---+---+---|\n| # | 1, 2 | say \"hi\" |\n|   | x |  |\n| $ | p=1 | |\n|---+---+---|\n| / | <r> | 3 |\n",
        );
        assert_eq!(export(&t, Format::Tsv), "1, 2\tsay \"hi\"\nx\t");
        assert_eq!(export(&t, Format::Csv), "\"1, 2\",\"say \"\"hi\"\"\"\nx,");
    }
}
