//! LaTeX tables (T2.7h.9): a `tabular` whose column specification and
//! rows are simple, drawn as the shared grid; Tab between its cells.

use std::ops::Range;

use latex_syntax::{SyntaxKind as K, SyntaxNode};
use org_edit::{Selection, Transaction};

use crate::view::{TableCell, TableRow, TableView};

/// The environments drawn as tables.
const TABLES: &[&str] = &[
    "tabular",
    "tabular*",
    "tabularx",
    "tabulary",
    "array",
    "longtable",
    // aastex's, its rows one by one (its caption and head come before them).
    "deluxetable",
    "deluxetable*",
];

/// Commands that may stand on a row's line besides its cells.
const RULES: &[&str] = &[
    "hline",
    "toprule",
    "midrule",
    "bottomrule",
    "cline",
    "cmidrule",
    "addlinespace",
];

/// Whether `name` is a table environment.
pub fn is_table(name: &str) -> bool {
    TABLES.contains(&name)
}

fn span(n: &SyntaxNode) -> Range<usize> {
    usize::from(n.text_range().start())..usize::from(n.text_range().end())
}

/// A table row: a rule line, or a line of cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    Rule(Range<usize>),
    /// The line, its cells, its spans (the column, the columns covered and
    /// the span's alignment), and whether a rule follows its `\\\\`.
    Data(Range<usize>, Vec<Range<usize>>, Vec<Span>, bool),
}

/// A span in a row: its first column, the columns it covers (a
/// `\\multicolumn`) or, for a `\\multirow`, 1, its alignment, and the
/// rows it covers (1 but for a `\\multirow`).
pub(crate) type Span = (usize, usize, char, usize);

/// A simple table: one row a line, the rule lines between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Simple {
    /// From the first row's line to the `\end` line.
    pub body: Range<usize>,
    pub align: Vec<char>,
    pub rows: Vec<Row>,
}

/// The alignment of each column of `spec`, if it has only `l`, `c`, `r`,
/// `p{}`, `m{}`, `b{}`, `X` and `|`.
fn spec(text: &str) -> Option<Vec<char>> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    // The text of the braced group next (blanks before it skipped).
    fn group(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.next() != Some('{') {
            return None;
        }
        let mut depth = 1;
        let mut text = String::new();
        for c in chars.by_ref() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                return Some(text);
            }
            text.push(c);
        }
        None
    }
    while let Some(c) = chars.next() {
        match c {
            'l' | 'c' | 'r' => out.push(c),
            'X' | 'L' | 'C' | 'R' | 'J' => out.push('l'),
            'p' | 'm' | 'b' => {
                group(&mut chars)?;
                out.push('l');
            }
            // siunitx's and dcolumn's numbers, aligned on their point.
            'S' => out.push('r'),
            'D' => {
                for _ in 0..3 {
                    group(&mut chars)?;
                }
                out.push('r');
            }
            // array's `w{align}{width}`.
            'w' | 'W' => {
                let align = group(&mut chars)?;
                group(&mut chars)?;
                out.push(
                    align
                        .trim()
                        .chars()
                        .next()
                        .filter(|a| "lcr".contains(*a))
                        .unwrap_or('l'),
                );
            }
            // Between columns (`@{}`, `!{}`), before or after a column's
            // cells (`>{\bfseries}`): not columns.
            '@' | '!' | '>' | '<' => {
                group(&mut chars)?;
            }
            // `*{3}{c}`: the columns repeated.
            '*' => {
                let n: usize = group(&mut chars)?.trim().parse().ok()?;
                let inner = spec(&group(&mut chars)?)?;
                for _ in 0..n.min(64) {
                    out.extend(inner.iter().copied());
                }
            }
            '|' | ' ' | '\t' | '\n' => {}
            _ => return None,
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Past a bracketed group `open`…`close` at `i` within `..end`, if one
/// starts there.
fn skip_group(b: &[u8], i: usize, end: usize, open: u8, close: u8) -> Option<usize> {
    if i >= end || b[i] != open {
        return Some(i);
    }
    let j = b[i..end].iter().position(|&x| x == close)?;
    Some(i + j + 1)
}

fn skip_blanks(b: &[u8], mut i: usize, end: usize) -> usize {
    while i < end && matches!(b[i], b' ' | b'\t' | b'\r') {
        i += 1;
    }
    i
}

/// Past the rule commands and blanks from `i` within `..end`, and whether
/// there was one.
fn skip_rules(text: &str, mut i: usize, end: usize) -> Option<(usize, bool)> {
    let b = text.as_bytes();
    let mut any = false;
    loop {
        i = skip_blanks(b, i, end);
        if i >= end || b[i] != b'\\' {
            return Some((i, any));
        }
        let n = b[i + 1..end]
            .iter()
            .take_while(|x| x.is_ascii_alphabetic())
            .count();
        if !RULES.contains(&&text[i + 1..i + 1 + n]) {
            return Some((i, any));
        }
        i = skip_blanks(b, i + 1 + n, end);
        i = skip_group(b, i, end, b'(', b')')?;
        i = skip_group(b, i, end, b'[', b']')?;
        i = skip_group(b, i, end, b'{', b'}')?;
        any = true;
    }
}

/// The row on `line`, if simple; `open` set when it has no `\\`.
fn row(text: &str, line: Range<usize>, columns: usize, open: &mut bool) -> Option<Row> {
    let s = &text[line.clone()];
    if s.trim().is_empty() {
        return None;
    }
    let (e, any) = skip_rules(text, line.start, line.end)?;
    if any && e == line.end {
        return Some(Row::Rule(line));
    }
    if ["\\verb", "\\lstinline"].iter().any(|c| s.contains(c)) {
        return None;
    }
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut i = line.start;
    let mut cell = line.start;
    let mut raw = Vec::new();
    let mut ended = false;
    let mut rule_after = false;
    while i < line.end {
        match b[i] {
            b'\\' if b.get(i + 1) == Some(&b'\\') && depth == 0 => {
                raw.push(cell..i);
                let j = skip_blanks(b, i + 2, line.end);
                let j = skip_group(b, j, line.end, b'[', b']')?;
                let (k, any) = skip_rules(text, j, line.end)?;
                if k != line.end {
                    return None;
                }
                rule_after = any;
                ended = true;
                break;
            }
            b'\\' => {
                // An escaped character is not a delimiter.
                i += if b.get(i + 1).is_some_and(|x| x.is_ascii()) {
                    2
                } else {
                    1
                };
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            b'%' => return None,
            b'&' if depth == 0 => {
                raw.push(cell..i);
                cell = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return None;
    }
    if !ended {
        raw.push(cell..line.end);
        *open = true;
    }
    let mut cells = Vec::new();
    let mut spans = Vec::new();
    for r in raw {
        let t = &text[r.clone()];
        let lead = t.len() - t.trim_start().len();
        if lead == t.len() {
            // An empty cell: after one blank.
            let at = r.start + t.len().min(1);
            cells.push(at..at);
            continue;
        }
        let trail = t.len() - t.trim_end().len();
        let r = r.start + lead..r.end - trail;
        // A span: its text in its first column, the columns it covers
        // after it empty (the grid has no spans).
        match span_cell(text, r.clone()) {
            Some((content, columns, rows, align)) => {
                if columns > 1 || rows > 1 {
                    spans.push((cells.len(), columns, align, rows));
                }
                cells.push(content);
                for _ in 1..columns {
                    cells.push(r.end..r.end);
                }
            }
            None if text[r.clone()].contains("\\multicolumn")
                || text[r.clone()].contains("\\multirow") =>
            {
                return None;
            }
            None => cells.push(r),
        }
    }
    if cells.len() > columns {
        return None;
    }
    Some(Row::Data(line, cells, spans, rule_after))
}

/// A cell that is all a `\\multicolumn{n}{spec}{text}` or a
/// `\\multirow{n}[…]{width}{text}`: its text's range, the columns and the
/// rows it covers, and the alignment its specification gives (`l` for a
/// `\\multirow`).
fn span_cell(text: &str, cell: Range<usize>) -> Option<(Range<usize>, usize, usize, char)> {
    let b = text.as_bytes();
    let s = &text[cell.clone()];
    // The groups `{…}` from `i`, balanced, with their inner ranges.
    let group = |i: usize| -> Option<(Range<usize>, usize)> {
        let i = skip_blanks(b, i, cell.end);
        if b.get(i) != Some(&b'{') {
            return None;
        }
        let mut depth = 0;
        for (k, &c) in b[i..cell.end].iter().enumerate() {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((i + 1..i + k, i + k + 1));
                    }
                }
                _ => {}
            }
        }
        None
    };
    let (name, multi) = if s.starts_with("\\multicolumn") {
        ("\\multicolumn", true)
    } else if s.starts_with("\\multirow") {
        ("\\multirow", false)
    } else {
        return None;
    };
    let (n, at) = group(cell.start + name.len())?;
    let count: usize = text[n].trim().parse().ok()?;
    let at = if multi {
        at
    } else {
        // `\\multirow{n}[bigstruts]{width}`.
        let at = skip_blanks(b, at, cell.end);
        skip_group(b, at, cell.end, b'[', b']')?
    };
    // The specification or width: a group, or `*` for `\\multirow`.
    let w = skip_blanks(b, at, cell.end);
    let mut align = 'l';
    let at = if !multi && b.get(w) == Some(&b'*') {
        w + 1
    } else {
        let (inner, after) = group(at)?;
        if multi {
            align = spec(&text[inner])
                .and_then(|a| a.first().copied())
                .unwrap_or('l');
        }
        after
    };
    let (content, end) = group(at)?;
    if end != cell.end {
        return None;
    }
    let (columns, rows) = if multi {
        (count.max(1), 1)
    } else {
        (1, count.max(1))
    };
    Some((content, columns, rows, align))
}

/// `env` as a simple table: its specification plain and every line of
/// its body a rule or one row.
pub(crate) fn simple(text: &str, env: &SyntaxNode) -> Option<Simple> {
    let name = latex_syntax::name(env)?;
    if !is_table(&name) || name.starts_with("deluxetable") {
        return None;
    }
    let begin = env.children().find(|c| c.kind() == K::BEGIN)?;
    let end = env.children().find(|c| c.kind() == K::END)?;
    let groups: Vec<SyntaxNode> = begin.children().filter(|c| c.kind() == K::GROUP).collect();
    if groups.len() < 2 {
        return None;
    }
    let g = span(groups.last()?);
    let align = spec(text.get(g.start + 1..g.end - 1)?)?;
    let b_end = span(&begin).end;
    let nl = text[b_end..].find('\n')?;
    if !text[b_end..b_end + nl].trim().is_empty() {
        return None;
    }
    let body = b_end + nl + 1;
    let e = span(&end).start;
    let last = text[..e].rfind('\n').map_or(0, |i| i + 1);
    if last <= body || !text[last..e].trim().is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    let mut pos = body;
    let mut open = false;
    while pos < last {
        if open {
            return None;
        }
        let le = text[pos..last].find('\n').map_or(last, |i| pos + i);
        let line_end = if text[pos..le].ends_with('\r') {
            le - 1
        } else {
            le
        };
        rows.push(row(text, pos..line_end, align.len(), &mut open)?);
        pos = le + 1;
    }
    Some(Simple {
        body: body..last,
        align,
        rows,
    })
}

/// The table environment around `pos`.
fn table_at(root: &SyntaxNode, pos: usize) -> Option<SyntaxNode> {
    latex_syntax::token_at(root, pos)
        .or_else(|| latex_syntax::token_before(root, pos))?
        .parent_ancestors()
        .find(|a| a.kind() == K::ENVIRONMENT && latex_syntax::name(a).is_some_and(|n| is_table(&n)))
}

/// The simple table whose rows start at `start`, as a grid.
pub fn table_view(doc: &crate::DocumentState, start: usize) -> Option<TableView> {
    let state = doc.latex()?;
    let root = state.parse().syntax();
    let text = doc.text().as_str();
    let t = simple(text, &table_at(&root, start)?)?;
    if t.body.start != start {
        return None;
    }
    let mut spans = Vec::new();
    let mut ruled = Vec::new();
    let mut multirows = Vec::new();
    for (i, r) in t.rows.iter().enumerate() {
        if let Row::Data(_, _, sp, rule) = r {
            for &(c, n, a, down) in sp {
                if n > 1 {
                    spans.push((i, c, n, a));
                }
                if down > 1 {
                    multirows.push((i, c, down));
                }
            }
            if *rule {
                ruled.push(i);
            }
        }
    }
    let mut rows: Vec<TableRow> = t
        .rows
        .into_iter()
        .map(|r| match r {
            Row::Rule(line) => TableRow::Rule { line },
            Row::Data(line, cells, ..) => {
                let view = crate::latex_view::line_view(doc, line.clone(), None);
                let cells = cells
                    .into_iter()
                    .map(|range| TableCell {
                        runs: crate::view::runs_within(&view, &range),
                        range,
                    })
                    .collect();
                TableRow::Data { line, cells }
            }
        })
        .collect();
    for (i, c, down) in multirows {
        center_multirow(&mut rows, i, c, down);
    }
    Some(TableView {
        range: t.body,
        rows,
        align: t.align,
        spans,
        ruled,
    })
}

/// A `\\multirow` at row `i`, column `c`, over `down` rows, drawn in the
/// middle of them as LaTeX sets it: its text moved to the middle row's
/// cell when that one is empty (the runs keep pointing at that cell, so
/// the line still maps to its source).
fn center_multirow(rows: &mut [TableRow], i: usize, c: usize, down: usize) {
    let data: Vec<usize> = (i..rows.len())
        .filter(|&r| matches!(rows[r], TableRow::Data { .. }))
        .take(down)
        .collect();
    let Some(&target) = data.get((down - 1) / 2) else {
        return;
    };
    if target == i {
        return;
    }
    let empty = match &rows[target] {
        TableRow::Data { cells, .. } => cells.get(c).is_some_and(|cell| {
            cell.range.is_empty() && cell.runs.iter().all(|r| r.text.trim().is_empty())
        }),
        TableRow::Rule { .. } => false,
    };
    if !empty {
        return;
    }
    let TableRow::Data { cells, .. } = &mut rows[i] else {
        return;
    };
    let Some(from) = cells.get_mut(c) else {
        return;
    };
    let moved = std::mem::take(&mut from.runs);
    let TableRow::Data { cells, .. } = &mut rows[target] else {
        return;
    };
    let cell = &mut cells[c];
    let at = cell.range.start;
    cell.runs = moved
        .into_iter()
        .map(|r| crate::view::Run {
            src: at..at,
            verbatim: false,
            widget: None,
            ..r
        })
        .collect();
}

/// The cursor moved to the next cell of the simple table around `pos`
/// (the previous one when `back`).
pub fn next_cell(text: &str, pos: usize, root: &SyntaxNode, back: bool) -> Option<Transaction> {
    let t = simple(text, &table_at(root, pos)?)?;
    if !(t.body.start <= pos && pos <= t.body.end) {
        return None;
    }
    let starts: Vec<usize> = t
        .rows
        .iter()
        .flat_map(|r| match r {
            Row::Data(_, cells, ..) => cells.iter().map(|c| c.start).collect(),
            Row::Rule(_) => Vec::new(),
        })
        .collect();
    let cur = starts.iter().rposition(|&s| s <= pos);
    let to = if back {
        starts.get(cur?.checked_sub(1)?)
    } else {
        starts.get(cur.map_or(0, |c| c + 1))
    }?;
    let mut tx = Transaction::new(if back { "Previous Field" } else { "Next Field" });
    tx.replace(pos..pos, "").ok()?;
    Some(tx.select(Selection::caret(*to)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(text: &str) -> (latex_syntax::Parse, SyntaxNode) {
        let p = latex_syntax::parse(text);
        let root = p.syntax();
        let e = table_at(&root, text.find("\\begin").unwrap() + 1).unwrap();
        (p, e)
    }

    #[test]
    fn simple_tables() {
        let text = "\\begin{tabular}{|l|c|p{2cm}|}\n\\toprule\n  A & B & C \\\\\n\\midrule\n 1 &  & \\textbf{3} \\\\ \\hline\n x & {a & b} \\\\[2pt]\n\\bottomrule\n\\end{tabular}\n";
        let (_p, e) = env(text);
        let t = simple(text, &e).unwrap();
        assert_eq!(t.align, vec!['l', 'c', 'l']);
        assert_eq!(t.rows.len(), 6);
        assert!(matches!(t.rows[0], Row::Rule(_)));
        let Row::Data(_, cells, ..) = &t.rows[1] else {
            panic!()
        };
        let got: Vec<&str> = cells.iter().map(|c| &text[c.clone()]).collect();
        assert_eq!(got, ["A", "B", "C"]);
        let Row::Data(_, cells, ..) = &t.rows[3] else {
            panic!()
        };
        let got: Vec<&str> = cells.iter().map(|c| &text[c.clone()]).collect();
        assert_eq!(got, ["1", "", "\\textbf{3}"]);
        let Row::Data(_, cells, ..) = &t.rows[4] else {
            panic!()
        };
        assert_eq!(cells.len(), 2);
        assert_eq!(&text[cells[1].clone()], "{a & b}");
        // tabularx: the specification is the last argument.
        let text = "\\begin{tabularx}{\\linewidth}{lX}\na & b\n\\end{tabularx}\n";
        let (_p, e) = env(text);
        assert_eq!(simple(text, &e).unwrap().align, vec!['l', 'l']);
        // Material between columns is not a column.
        let text = "\\begin{tabular}{@{}l@{\\quad}r@{}}\na & b \\\\\n\\end{tabular}\n";
        let (_p, e) = env(text);
        assert_eq!(simple(text, &e).unwrap().align, vec!['l', 'r']);
    }

    #[test]
    fn column_specifications() {
        assert_eq!(spec("|l|c|p{2cm}|"), Some(vec!['l', 'c', 'l']));
        // Material between columns, `*{n}{…}`, siunitx's and dcolumn's
        // numbers, array's `w`.
        assert_eq!(spec("@{}l@{\\quad}c@{}"), Some(vec!['l', 'c']));
        assert_eq!(spec(">{\\bfseries}l<{x}r!{\\vrule}"), Some(vec!['l', 'r']));
        assert_eq!(spec("l*{3}{c}"), Some(vec!['l', 'c', 'c', 'c']));
        assert_eq!(spec("lSD{.}{.}{2}"), Some(vec!['l', 'r', 'r']));
        assert_eq!(spec("w{c}{1cm}"), Some(vec!['c']));
        assert_eq!(spec("l*{x}{c}"), None);
        assert_eq!(spec("lq"), None);
    }

    #[test]
    fn spans_in_their_first_column() {
        let text = "\\begin{tabular}{lll}\n\\multicolumn{2}{c}{Head} & c \\\\\n\\multirow{2}*{A} & b & c \\\\\n\\multirow{2}{3cm}{B} & e & f \\\\\n\\end{tabular}\n";
        let (_p, e) = env(text);
        let t = simple(text, &e).expect("simple with spans");
        let Row::Data(_, cells, ..) = &t.rows[0] else {
            panic!()
        };
        assert_eq!(cells.len(), 3);
        assert_eq!(&text[cells[0].clone()], "Head");
        assert!(cells[1].is_empty());
        assert_eq!(&text[cells[2].clone()], "c");
        let Row::Data(_, cells, ..) = &t.rows[2] else {
            panic!()
        };
        assert_eq!(&text[cells[0].clone()], "B");
    }

    #[test]
    fn spans_and_rules_after_rows() {
        let text = "\\begin{tabular}{lll}\n\\multicolumn{2}{c}{Head} & c \\\\ \\hline\na & b & c \\\\\n\\end{tabular}\n";
        let meta = crate::Metadata {
            path: None,
            mode: crate::DocumentMode::Latex,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let d = crate::DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        );
        let tv = table_view(&d, text.find("\\multicolumn").unwrap()).unwrap();
        assert_eq!(tv.spans, vec![(0, 0, 2, 'c')]);
        assert_eq!(tv.ruled, vec![0]);
    }

    #[test]
    fn multirow_drawn_across_its_rows() {
        // Three rows: the text in the middle one, as LaTeX sets it; the
        // runs of every line stay within it.
        let text = "\\begin{tabular}{ll}\n\\multirow{3}*{Group} & a \\\\\n & b \\\\\n & c \\\\\n\\end{tabular}\n";
        let meta = crate::Metadata {
            path: None,
            mode: crate::DocumentMode::Latex,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let d = crate::DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        );
        let tv = table_view(&d, text.find("\\multirow").unwrap()).unwrap();
        let shown = |r: usize| match &tv.rows[r] {
            TableRow::Data { line, cells } => {
                for c in cells {
                    for run in &c.runs {
                        assert!(line.start <= run.src.start && run.src.end <= line.end);
                    }
                }
                cells[0]
                    .runs
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<String>()
            }
            TableRow::Rule { .. } => panic!(),
        };
        assert_eq!(shown(0), "");
        assert_eq!(shown(1), "Group");
        assert_eq!(shown(2), "");
        assert!(tv.spans.is_empty());
    }

    #[test]
    fn complex_tables_stay_source() {
        for text in [
            // A column type Kalem does not know.
            "\\begin{tabular}{lq}\na & b \\\\\n\\end{tabular}\n",
            "\\begin{tabular}{ll}\n\\multicolumn{3}{c}{x} \\\\\n\\end{tabular}\n",
            "\\begin{tabular}{ll}\na \\multicolumn{2}{c}{x} \\\\\n\\end{tabular}\n",
            "\\begin{tabular}{ll}\na & b \\\\ c & d \\\\\n\\end{tabular}\n",
            "\\begin{tabular}{ll}\na & b & c \\\\\n\\end{tabular}\n",
            "\\begin{tabular}{ll} a & b \\\\\n\\end{tabular}\n",
            "\\begin{tabular}{ll}\na & b % note\n\\end{tabular}\n",
            "\\begin{tabular}{ll}\na & b\nc & d\n\\end{tabular}\n",
            "\\begin{tabular}{ll}\n\\end{tabular}\n",
        ] {
            let (_p, e) = env(text);
            assert_eq!(simple(text, &e), None, "{text}");
        }
    }

    #[test]
    fn tab_moves_between_cells() {
        let text = "\\begin{tabular}{ll}\na & b \\\\\n\\hline\nc & d\n\\end{tabular}\n";
        let p = latex_syntax::parse(text);
        let root = p.syntax();
        let a = text.find("a &").unwrap();
        let go = |pos: usize, back: bool| {
            next_cell(text, pos, &root, back).and_then(|tx| tx.selection_after.map(|s| s.head))
        };
        let b = text.find("b \\").unwrap();
        let c = text.find("c &").unwrap();
        let d = text.find("d\n").unwrap();
        assert_eq!(go(a, false), Some(b));
        assert_eq!(go(b + 1, false), Some(c));
        assert_eq!(go(c, false), Some(d));
        assert_eq!(go(d, false), None);
        assert_eq!(go(c, true), Some(b));
        assert_eq!(go(a, true), None);
        assert_eq!(go(0, false), None);
    }
}
