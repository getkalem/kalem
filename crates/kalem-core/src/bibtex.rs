//! BibTeX files as a grid (T2.7h.19): each entry one row (key, type,
//! authors, title, year) away from the cursor, its source where the cursor
//! is; the rows sorted in the view without touching the file; a field set
//! by the smallest edit. The scanner returns ranges into the file, so what
//! the grid does not touch stays byte for byte as written.

use std::ops::Range;

use org_edit::Transaction;

/// A field of an entry: `name = value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The name.
    pub name: Range<usize>,
    /// The value as written: braces, quotes, `#` concatenations.
    pub value: Range<usize>,
}

/// An entry: `@type{key, fields}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// From the `@` to the closing brace.
    pub range: Range<usize>,
    /// The type, as written (`article`, `Book`).
    pub kind: Range<usize>,
    /// The citation key.
    pub key: Range<usize>,
    /// The fields in order.
    pub fields: Vec<Field>,
    /// The closing brace or parenthesis.
    pub close: usize,
}

impl Entry {
    /// The field named `name` (case-insensitive).
    pub fn field<'a>(&'a self, text: &str, name: &str) -> Option<&'a Field> {
        self.fields
            .iter()
            .find(|f| text[f.name.clone()].eq_ignore_ascii_case(name))
    }
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Whether an entry starts at `j`: `@` at the start of a line (where an
/// unbalanced value is taken to end).
fn entry_start(b: &[u8], j: usize) -> bool {
    b[j] == b'@' && j > 0 && b[j - 1] == b'\n'
}

/// Past a braced group opening at `i`.
fn skip_braces(b: &[u8], i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        if j > i && entry_start(b, j) {
            return j;
        }
        match b[j] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return j + 1;
                }
            }
            b'\\' => j += 1,
            _ => {}
        }
        j += 1;
    }
    b.len()
}

/// Past a quoted string opening at `i` (braces inside protect quotes).
fn skip_quoted(b: &[u8], i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i + 1;
    while j < b.len() {
        if entry_start(b, j) {
            return j;
        }
        match b[j] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'"' if depth == 0 => return j + 1,
            b'\\' => j += 1,
            _ => {}
        }
        j += 1;
    }
    b.len()
}

fn ident_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len()
        && !b[i].is_ascii_whitespace()
        && !matches!(b[i], b'{' | b'}' | b'(' | b')' | b',' | b'=' | b'#' | b'"')
    {
        i += 1;
    }
    i
}

/// The entries of `text`; `@string`, `@preamble` and `@comment` are skipped,
/// as is text outside entries (BibTeX's comments).
pub fn entries(text: &str) -> Vec<Entry> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = text[i..].find('@').map(|k| i + k) {
        let kind = at + 1..ident_end(b, at + 1);
        let mut j = skip_ws(b, kind.end);
        let (open, close) = match b.get(j) {
            Some(b'{') => (b'{', b'}'),
            Some(b'(') => (b'(', b')'),
            _ => {
                i = at + 1;
                continue;
            }
        };
        let name = text[kind.clone()].to_ascii_lowercase();
        if matches!(name.as_str(), "string" | "preamble" | "comment") {
            i = if open == b'{' {
                skip_braces(b, j)
            } else {
                text[j..].find(')').map_or(b.len(), |k| j + k + 1)
            };
            continue;
        }
        j = skip_ws(b, j + 1);
        let key_start = j;
        while j < b.len() && b[j] != b',' && b[j] != close && !b[j].is_ascii_whitespace() {
            j += 1;
        }
        let key = key_start..j;
        let mut fields = Vec::new();
        let mut end = None;
        // Past what cannot be read: to the next comma or the entry's end
        // at the top level.
        let recover = |mut k: usize| -> usize {
            let mut depth = 0usize;
            while k < b.len() {
                match b[k] {
                    b'{' => depth += 1,
                    b'}' if depth > 0 => depth -= 1,
                    c if depth == 0 && (c == b',' || c == close) => return k,
                    // A new entry at the start of a line.
                    b'@' if depth == 0 && (k == 0 || b[k - 1] == b'\n') => return k,
                    _ => {}
                }
                k += 1;
            }
            k
        };
        let mut next_entry = None;
        loop {
            j = skip_ws(b, j);
            while j < b.len() && b[j] == b',' {
                j = skip_ws(b, j + 1);
            }
            if j >= b.len() {
                break;
            }
            if b[j] == close {
                end = Some(j);
                break;
            }
            // An entry left open: it ends where the next one starts.
            if b[j] == b'@' && (j == 0 || b[j - 1] == b'\n') {
                next_entry = Some(j);
                break;
            }
            let n = j..ident_end(b, j);
            if n.is_empty() {
                // Not a field: skipped.
                j = recover(j + 1);
                continue;
            }
            j = skip_ws(b, n.end);
            if b.get(j) != Some(&b'=') {
                j = recover(j);
                continue;
            }
            j = skip_ws(b, j + 1);
            let v_start = j;
            let mut v_end = j;
            loop {
                let part_end = match b.get(j) {
                    Some(b'{') => skip_braces(b, j),
                    Some(b'"') => skip_quoted(b, j),
                    Some(_) => ident_end(b, j),
                    None => j,
                };
                if part_end == j {
                    break;
                }
                v_end = part_end;
                j = skip_ws(b, part_end);
                if b.get(j) == Some(&b'#') {
                    j = skip_ws(b, j + 1);
                } else {
                    break;
                }
            }
            fields.push(Field {
                name: n,
                value: v_start..v_end,
            });
            j = v_end;
        }
        let Some(close_at) = end else {
            // An entry left open: what was read is kept, to the next entry
            // or the end.
            let stop = next_entry.unwrap_or(b.len());
            let last = text[..stop].trim_end().len().max(at + 1);
            out.push(Entry {
                range: at..last,
                kind,
                key,
                fields,
                close: last,
            });
            match next_entry {
                Some(n) => {
                    i = n;
                    continue;
                }
                None => break,
            }
        };
        out.push(Entry {
            range: at..close_at + 1,
            kind,
            key,
            fields,
            close: close_at,
        });
        i = close_at + 1;
    }
    out
}

/// A value as a reader reads it: outer braces or quotes off, TeX's
/// accents and escapes as characters, blanks collapsed.
pub fn plain(value: &str) -> String {
    let v = value.trim();
    let v = if (v.starts_with('{') && v.ends_with('}')) || (v.starts_with('"') && v.ends_with('"'))
    {
        &v[1..v.len() - 1]
    } else {
        v
    };
    let mut out = String::new();
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '~' => out.push(' '),
            '\\' => {
                let Some(&n) = chars.peek() else { break };
                if matches!(
                    n,
                    '"' | '\'' | '`' | '^' | '~' | 'c' | '=' | '.' | 'u' | 'v' | 'H'
                ) {
                    chars.next();
                    // `\c c`, `\c{c}`: the letter after blanks or a brace.
                    while chars
                        .peek()
                        .is_some_and(|x| *x == '{' || (n.is_alphabetic() && *x == ' '))
                    {
                        chars.next();
                    }
                    let Some(l) = chars.next() else { break };
                    out.push(accent(n, l).unwrap_or(l));
                } else if n.is_ascii_alphabetic() {
                    // A command: its name is dropped (`\textit`), the
                    // special letters kept.
                    let mut name = String::new();
                    while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                        name.push(chars.next().unwrap_or(' '));
                    }
                    match name.as_str() {
                        "ss" => out.push('ß'),
                        "i" => out.push('ı'),
                        "o" => out.push('ø'),
                        "O" => out.push('Ø'),
                        "ae" => out.push('æ'),
                        "aa" => out.push('å'),
                        "l" => out.push('ł'),
                        "L" => out.push('Ł'),
                        "TeX" | "LaTeX" | "BibTeX" | "LaTeXe" => out.push_str(&name),
                        _ => {}
                    }
                } else {
                    chars.next();
                    out.push(n);
                }
            }
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn accent(mark: char, letter: char) -> Option<char> {
    let table: &[(char, &str, &str)] = &[
        ('"', "aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
        ('\'', "aeiouycnszAEIOUCNSZ", "áéíóúýćńśźÁÉÍÓÚĆŃŚŹ"),
        ('`', "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
        ('^', "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
        ('~', "anoANO", "ãñõÃÑÕ"),
        ('c', "csCS", "çşÇŞ"),
        ('u', "gG", "ğĞ"),
        ('v', "csznrCSZNR", "čšžňřČŠŽŇŘ"),
        ('.', "zIZ", "żİŻ"),
        ('H', "ouOU", "őűŐŰ"),
    ];
    let (_, from, to) = table.iter().find(|(m, _, _)| *m == mark)?;
    let i = from.chars().position(|c| c == letter)?;
    to.chars().nth(i)
}

/// The authors as a grid shows them: last names, "and" between two, "et
/// al." after the first of more.
pub fn short_authors(authors: &str) -> String {
    let names: Vec<String> = authors
        .split(" and ")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| match n.split_once(',') {
            Some((last, _)) => last.trim().to_string(),
            None => n.split_whitespace().last().unwrap_or(n).to_string(),
        })
        .collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{} and {}", names[0], names[1]),
        _ => format!("{} et al.", names[0]),
    }
}

/// Whether `doc` is a BibTeX file.
pub fn is_bib(doc: &crate::DocumentState) -> bool {
    matches!(&doc.meta.mode, crate::DocumentMode::Text { language: Some(l) } if l.eq_ignore_ascii_case("bib"))
}

/// The grid's columns.
pub const COLUMNS: [&str; 5] = ["key", "type", "author", "title", "year"];

/// The widest each column is laid out, in characters.
const MAX: [usize; 5] = [24, 13, 24, 48, 4];

/// An entry's cells, as the grid shows them.
pub fn cells(text: &str, e: &Entry) -> [String; 5] {
    let get = |n: &str| {
        e.field(text, n)
            .map(|f| plain(&text[f.value.clone()]))
            .unwrap_or_default()
    };
    let author = {
        let a = get("author");
        if a.is_empty() { get("editor") } else { a }
    };
    let year = {
        let y = get("year");
        if y.is_empty() {
            get("date").chars().take(4).collect()
        } else {
            y
        }
    };
    [
        text[e.key.clone()].to_string(),
        text[e.kind.clone()].to_ascii_lowercase(),
        short_authors(&author),
        get("title"),
        year,
    ]
}

/// The grid of a BibTeX document: its entries, their cells, the column
/// widths.
#[derive(Debug)]
pub struct Grid {
    /// The entries in file order.
    pub entries: Vec<Entry>,
    /// Their cells.
    pub cells: Vec<[String; 5]>,
    /// The columns' widths, in characters.
    pub widths: [usize; 5],
}

thread_local! {
    static GRID: std::cell::RefCell<Option<(u64, std::rc::Rc<Grid>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The grid of the BibTeX document `doc`, for its text version.
pub fn grid(doc: &crate::DocumentState) -> std::rc::Rc<Grid> {
    use unicode_width::UnicodeWidthStr;
    let key = doc.version();
    GRID.with(|g| {
        if let Some((k, v)) = &*g.borrow()
            && *k == key
        {
            return v.clone();
        }
        let text = doc.text().as_str();
        let entries = entries(text);
        let cells: Vec<[String; 5]> = entries.iter().map(|e| cells(text, e)).collect();
        let mut widths = [0; 5];
        for c in &cells {
            for (i, s) in c.iter().enumerate() {
                widths[i] = widths[i].max(s.width().min(MAX[i]));
            }
        }
        let v = std::rc::Rc::new(Grid {
            entries,
            cells,
            widths,
        });
        *g.borrow_mut() = Some((key, v.clone()));
        v
    })
}

/// `s` cut to `width` columns (with `…`) and padded to it.
fn fit(s: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if s.width() <= width {
        return format!("{s}{}", " ".repeat(width - s.width()));
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    w += 1;
    format!("{out}{}", " ".repeat(width.saturating_sub(w)))
}

/// The entry holding byte `pos`.
fn entry_at(grid: &Grid, pos: usize) -> Option<usize> {
    let i = grid.entries.partition_point(|e| e.range.end <= pos);
    grid.entries
        .get(i)
        .filter(|e| e.range.start <= pos && pos <= e.range.end)
        .map(|_| i)
}

/// A line of a BibTeX document: an entry's first line away from the cursor
/// as its row of the grid; other lines as they are.
pub fn line_view(
    doc: &crate::DocumentState,
    line: Range<usize>,
    cursor: Option<usize>,
) -> crate::view::LineView {
    use crate::view::{LineView, Run, Style};
    let text = doc.text().as_str();
    let g = grid(doc);
    let row = g
        .entries
        .iter()
        .position(|e| line.start <= e.range.start && e.range.start < line.end.max(line.start + 1));
    let Some(i) = row.filter(|&i| {
        let e = &g.entries[i];
        cursor.is_none_or(|c| !(e.range.start <= c && c <= e.range.end))
            && text[line.start..e.range.start].trim().is_empty()
    }) else {
        let mut v = crate::view::plain_line_view(text, line, cursor);
        v.mono = true;
        return v;
    };
    let e = &g.entries[i];
    let mut runs = Vec::new();
    let bar = |at: usize| Run {
        src: at..at,
        text: " │ ".into(),
        verbatim: false,
        style: Style {
            dim: true,
            ..Style::default()
        },
        widget: None,
    };
    for (c, s) in g.cells[i].iter().enumerate() {
        if c > 0 {
            runs.push(bar(e.key.end.min(line.end)));
        }
        let (src, style) = if c == 0 {
            // The key stands for the line: a click there opens the entry.
            (
                line.start..line.end,
                Style {
                    bold: true,
                    ..Style::default()
                },
            )
        } else {
            (
                line.end..line.end,
                Style {
                    italic: c == 3,
                    dim: c == 1,
                    ..Style::default()
                },
            )
        };
        let shown = if c + 1 == COLUMNS.len() {
            s.clone()
        } else {
            fit(s, g.widths[c])
        };
        runs.push(Run {
            src,
            text: shown,
            verbatim: false,
            style,
            widget: None,
        });
    }
    LineView {
        runs,
        range: line,
        mono: true,
        ..LineView::default()
    }
}

/// The lines a BibTeX document shows as its grid: each entry's first line,
/// all the lines of the entry holding the cursor, and the text between
/// entries that is not blank (comments, `@string`s), in the order of the
/// view's sort (`DocumentState::bib_sort`).
pub fn shown_lines(doc: &crate::DocumentState) -> Option<std::rc::Rc<Vec<usize>>> {
    if !is_bib(doc) {
        return None;
    }
    let g = grid(doc);
    let t = doc.text();
    let text = t.as_str();
    let cursor = doc.selection.head.min(text.len());
    let open = entry_at(&g, cursor);
    // The lines outside entries: shown unless blank.
    let mut in_entry = vec![false; t.line_count()];
    for e in &g.entries {
        let first = t.line_of(e.range.start);
        let last = t.line_of(e.range.end.saturating_sub(1).max(e.range.start));
        for l in first..=last.min(in_entry.len().saturating_sub(1)) {
            in_entry[l] = true;
        }
    }
    let mut order: Vec<usize> = (0..g.entries.len()).collect();
    if let Some((col, reverse)) = doc.bib_sort {
        let key = |i: &usize| {
            g.cells[*i]
                .get(col)
                .cloned()
                .unwrap_or_default()
                .to_lowercase()
        };
        order.sort_by_key(key);
        if reverse {
            order.reverse();
        }
    }
    let mut out: Vec<usize> = (0..in_entry.len())
        .filter(|&l| !in_entry[l] && !text[t.line_range(l)].trim().is_empty())
        .collect();
    for i in order {
        let e = &g.entries[i];
        let first = t.line_of(e.range.start);
        if open == Some(i) {
            let last = t.line_of(e.range.end.saturating_sub(1).max(e.range.start));
            out.extend(first..=last);
        } else {
            out.push(first);
        }
    }
    if doc.bib_sort.is_none() {
        out.sort_unstable();
        out.dedup();
    }
    if out.is_empty() {
        out.push(t.line_of(cursor));
    }
    Some(std::rc::Rc::new(out))
}

/// One replacement as a transaction.
fn one(range: Range<usize>, insert: String) -> Transaction {
    let mut tx = Transaction::new("Set Field");
    let _ = tx.replace(range, insert);
    tx
}

/// The edit that sets field `name` of entry `e` to `value` (braced), the
/// smallest one: the value replaced where the field is, else a line added
/// before the closing brace in the indentation of the other fields; an
/// empty value removes the field.
pub fn set_field(text: &str, e: &Entry, name: &str, value: &str) -> Transaction {
    let braced = format!("{{{value}}}");
    if let Some(f) = e.field(text, name) {
        if value.is_empty() {
            // The field's line, or the field and its comma.
            let start = text[..f.name.start]
                .rfind('\n')
                .map_or(f.name.start, |n| n + 1);
            let mut end = f.value.end;
            let rest = &text[end..e.close];
            let comma = rest.find(',').filter(|k| rest[..*k].trim().is_empty());
            if let Some(k) = comma {
                end += k + 1;
            }
            let line_start_blank = text[start..f.name.start].trim().is_empty();
            let (from, to) = if line_start_blank && text[end..].starts_with('\n') {
                (start, end + 1)
            } else {
                (f.name.start, end)
            };
            return one(from..to, String::new());
        }
        return one(f.value.clone(), braced);
    }
    if value.is_empty() {
        return Transaction::new("Set Field");
    }
    let indent = e
        .fields
        .first()
        .map(|f| {
            let ls = text[..f.name.start].rfind('\n').map_or(0, |n| n + 1);
            let lead = &text[ls..f.name.start];
            if lead.trim().is_empty() {
                lead.to_string()
            } else {
                "  ".into()
            }
        })
        .unwrap_or_else(|| "  ".into());
    // After the last field (its comma added when it has none), on a line of
    // its own.
    let after = e.fields.last().map_or(e.key.end, |f| f.value.end);
    let rest = &text[after..e.close];
    let has_comma = rest.trim_start().starts_with(',');
    let at = if has_comma {
        after + rest.find(',').map_or(0, |k| k + 1)
    } else {
        after
    };
    let comma = if has_comma { "" } else { "," };
    one(at..at, format!("{comma}\n{indent}{name} = {braced}"))
}

/// The entry at `pos` of the BibTeX document `doc`.
pub fn entry_at_cursor(doc: &crate::DocumentState) -> Option<Entry> {
    let g = grid(doc);
    let pos = doc.selection.head.min(doc.text().len());
    entry_at(&g, pos).map(|i| g.entries[i].clone())
}

/// The column named `name`, or the one whose field the cursor is in.
pub fn column(doc: &crate::DocumentState, name: Option<&str>) -> usize {
    if let Some(n) = name {
        return COLUMNS
            .iter()
            .position(|c| c.eq_ignore_ascii_case(n))
            .unwrap_or(0);
    }
    let text = doc.text().as_str();
    let pos = doc.selection.head;
    entry_at_cursor(doc)
        .and_then(|e| {
            if e.key.start <= pos && pos <= e.key.end {
                return Some(0);
            }
            let f = e
                .fields
                .iter()
                .find(|f| f.name.start <= pos && pos <= f.value.end)?;
            let n = text[f.name.clone()].to_ascii_lowercase();
            COLUMNS
                .iter()
                .position(|c| *c == n || (*c == "author" && n == "editor"))
        })
        .unwrap_or(0)
}

/// The status bar's words for a BibTeX document: how many entries, and
/// the sort.
pub fn status(doc: &crate::DocumentState) -> Option<String> {
    if !is_bib(doc) {
        return None;
    }
    let g = grid(doc);
    let n = g.entries.len();
    Some(match doc.bib_sort {
        Some((c, rev)) => crate::tr!(
            "status-bib-sorted",
            count = n as i64,
            column = COLUMNS[c.min(4)],
            order = if rev { "desc" } else { "asc" }
        ),
        None => crate::tr!("status-bib", count = n as i64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIB: &str = "% My references\n@string{tug = \"TUGboat\"}\n\n@book{knuth84,\n  author = {Donald E. Knuth},\n  title = {The {\\TeX}book},\n  year = 1984,\n}\n\n@Article{lamport,\n  author = \"Lamport, Leslie and Mittelbach, Frank and Goossens, Michel\",\n  title = {G{\\\"o}del and {\\c C}ay},\n  journal = tug # { 1},\n  year = {1994}\n}\n";

    fn doc(text: &str) -> crate::DocumentState {
        let meta = crate::Metadata {
            path: Some("refs.bib".into()),
            mode: crate::DocumentMode::Text {
                language: Some("bib".into()),
            },
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
        };
        crate::DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        )
    }

    #[test]
    fn scans_entries() {
        let es = entries(BIB);
        assert_eq!(es.len(), 2);
        assert_eq!(&BIB[es[0].key.clone()], "knuth84");
        assert_eq!(&BIB[es[1].kind.clone()], "Article");
        let j = es[1].field(BIB, "JOURNAL").unwrap();
        assert_eq!(&BIB[j.value.clone()], "tug # { 1}");
        assert_eq!(
            cells(BIB, &es[1]),
            [
                "lamport".to_string(),
                "article".into(),
                "Lamport et al.".into(),
                "Gödel and Çay".into(),
                "1994".into()
            ]
        );
        assert_eq!(cells(BIB, &es[0])[3], "The TeXbook");
        assert_eq!(cells(BIB, &es[0])[2], "Knuth");
    }

    #[test]
    fn grid_rows_and_sorting() {
        let mut d = doc(BIB);
        d.selection = org_edit::Selection::caret(0);
        let lines = shown_lines(&d).unwrap();
        // The comment, the `@string`, and each entry's first line.
        assert_eq!(*lines, vec![0, 1, 3, 9]);
        let row = line_view(&d, d.text().line_range(3), Some(0)).display();
        assert!(row.starts_with("knuth84 │ book    │ Knuth "), "{row}");
        assert!(row.ends_with("│ 1984"), "{row}");
        // The cursor in an entry: all its lines, as source.
        d.selection = org_edit::Selection::caret(BIB.find("Lamport,").unwrap());
        let lines = shown_lines(&d).unwrap();
        assert_eq!(*lines, vec![0, 1, 3, 9, 10, 11, 12, 13, 14]);
        let src = line_view(&d, d.text().line_range(9), Some(d.selection.head)).display();
        assert_eq!(src, "@Article{lamport,");
        // Sorted by year, descending.
        d.selection = org_edit::Selection::caret(0);
        d.bib_sort = Some((4, true));
        let lines = shown_lines(&d).unwrap();
        assert_eq!(*lines, vec![0, 1, 9, 3]);
    }

    #[test]
    fn malformed_entries_do_not_swallow_the_rest() {
        let bib = "@book{a,\n  author = {A},\n  junk here,\n  year = 2000,\n}\n\n@article{b,\n  title = {Open\n\n@misc{c,\n  title = {C}\n}\n";
        let es = entries(bib);
        let keys: Vec<&str> = es.iter().map(|e| &bib[e.key.clone()]).collect();
        assert_eq!(keys, ["a", "b", "c"]);
        // The bad field skipped, the good ones read.
        assert!(es[0].field(bib, "year").is_some());
        assert_eq!(cells(bib, &es[2])[3], "C");
    }

    #[test]
    fn sets_fields() {
        let es = entries(BIB);
        let apply = |tx: Transaction| {
            let mut s = BIB.to_string();
            for c in tx.edits.iter().rev() {
                s.replace_range(c.range.clone(), &c.insert);
            }
            s
        };
        // A value replaced in place.
        let s = apply(set_field(BIB, &es[0], "year", "1986"));
        assert!(s.contains("  year = {1986},\n}"), "{s}");
        // A new field after the last, the comma added.
        let s = apply(set_field(BIB, &es[1], "doi", "10.1/x"));
        assert!(s.contains("  year = {1994},\n  doi = {10.1/x}\n}"), "{s}");
        // An empty value removes the field's line.
        let s = apply(set_field(BIB, &es[0], "year", ""));
        assert!(s.contains("  title = {The {\\TeX}book},\n}"), "{s}");
    }
}
