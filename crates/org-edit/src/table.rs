//! Org tables: `org-table-align` and the row and column commands of
//! `org-table.el` (insert, kill and move rows, insert, delete and move
//! columns, horizontal rules, and the field motions of TAB and RET that
//! add rows), with `#+TBLFM` references renumbered as
//! `org-table-fix-formulas` does.
//!
//! Column widths are display widths as Emacs measures them, with the
//! hidden parts of bracket links left out (`org-link-descriptive`).

use org_model::Document;
use org_syntax::{NodeOrToken, ParseContext, SyntaxElement, SyntaxKind, SyntaxNode};

use crate::buffer::{Buf, EditError, string_width};
use crate::transaction::Transaction;

fn bol(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn eol(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

fn next_line(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1)
}

/// `org-table-line-regexp` at the line `b`.
fn table_line(text: &str, b: usize) -> bool {
    text[b..eol(text, b)]
        .trim_start_matches([' ', '\t'])
        .starts_with('|')
}

/// `org-table-hline-regexp` at the line `b`.
fn hline(text: &str, b: usize) -> bool {
    text[b..eol(text, b)]
        .trim_start_matches([' ', '\t'])
        .starts_with("|-")
}

/// `org-table-border-regexp` at the line `b`: not a table line (blank lines
/// included).
fn border(text: &str, b: usize) -> bool {
    !text[b..eol(text, b)]
        .trim_start_matches([' ', '\t'])
        .starts_with('|')
}

/// `org-table-begin`.
fn table_begin(text: &str, pos: usize) -> usize {
    let mut b = bol(text, pos);
    loop {
        if b == 0 {
            return 0;
        }
        let prev = bol(text, b - 1);
        if border(text, prev) {
            return b;
        }
        b = prev;
    }
}

/// `org-table-end`.
fn table_end(text: &str, pos: usize) -> usize {
    let mut b = next_line(text, bol(text, pos));
    while b < text.len() {
        if border(text, b) {
            return b;
        }
        b = next_line(text, b);
    }
    // At the end: before trailing blanks of a last line without a line feed.
    let t = text.trim_end_matches([' ', '\t']);
    if t.is_empty() || t.ends_with('\n') {
        t.len()
    } else {
        text.len()
    }
}

/// `org-at-table-p`: a table line inside an Org table.
pub(crate) fn at_table(doc: &Document, text: &str, pos: usize) -> bool {
    let b = bol(text, pos);
    if !table_line(text, b) {
        return false;
    }
    // The table the line belongs to: an ancestor of its first token.
    let root = doc.parse().syntax();
    let Some(tok) = root
        .token_at_offset(org_syntax::TextSize::from(b as u32))
        .right_biased()
    else {
        return false;
    };
    tok.parent_ancestors().any(|n| {
        n.kind() == SyntaxKind::TABLE && {
            let r = n.text_range();
            usize::from(r.start()) <= b
                && b < usize::from(r.end())
                && !n.text().to_string().trim_start().starts_with('+')
        }
    })
}

/// `org-at-table-hline-p` on the line `b`.
pub(crate) fn hline_at(text: &str, b: usize) -> bool {
    hline(text, b)
}

/// `org-table-current-column` at `pos`.
fn current_column(text: &str, pos: usize) -> usize {
    let b = bol(text, pos);
    let before = &text[b..pos];
    let Some(first) = before.find('|') else {
        return 0;
    };
    let sep: &[char] = if hline(text, b) { &['+', '|'] } else { &['|'] };
    1 + before[first + 1..].matches(sep).count()
}

/// `org-table-goto-column` without FORCE: after the Nth bar (and one
/// space) of the line at `b`.
fn goto_column(text: &str, b: usize, n: usize) -> usize {
    let e = eol(text, b);
    let mut p = b;
    let mut k = n;
    while k > 0 {
        match text[p..e].find('|') {
            Some(i) => p = p + i + 1,
            None => break,
        }
        k -= 1;
    }
    if n > 0 && text.as_bytes().get(p) == Some(&b' ') {
        p += 1;
    }
    p
}

/// `org-table-goto-column` with ON-DELIM: before the left bar of the Nth
/// field of the line at `b`, or before the last bar of a shorter line.
fn goto_delim(text: &str, b: usize, n: usize) -> usize {
    let e = eol(text, b);
    let mut p = b;
    for _ in 0..n {
        match text[p..e].find('|') {
            Some(i) => p = p + i + 1,
            None => break,
        }
    }
    if n > 0 && p > b { p - 1 } else { p }
}

/// The display width of a table cell as `org-string-width` measures it:
/// bracket links show their description, or their target.
fn visible_width(el: &SyntaxElement) -> usize {
    match el {
        NodeOrToken::Token(t) => string_width(t.text()),
        NodeOrToken::Node(n) if n.kind() == SyntaxKind::LINK => {
            let children: Vec<SyntaxElement> = n.children_with_tokens().collect();
            let bracket = children
                .first()
                .is_some_and(|c| c.kind() == SyntaxKind::MARKER && c.to_string() == "[[");
            if !bracket {
                return string_width(n.text().to_string().trim_end_matches([' ', '\t']))
                    + trailing_blank(n);
            }
            let desc_start = children
                .iter()
                .position(|c| c.kind() == SyntaxKind::MARKER && c.to_string() == "][");
            let w = match desc_start {
                Some(i) => children[i + 1..]
                    .iter()
                    .filter(|c| !(c.kind() == SyntaxKind::MARKER && c.to_string() == "]]"))
                    .filter(|c| {
                        c.kind() != SyntaxKind::WHITESPACE
                            || c.text_range().end() < n.text_range().end()
                    })
                    .map(visible_width)
                    .sum::<usize>(),
                None => children
                    .iter()
                    .filter(|c| c.kind() == SyntaxKind::CODE_TEXT)
                    .map(visible_width)
                    .sum::<usize>(),
            };
            w + trailing_blank(n)
        }
        NodeOrToken::Node(n) => n.children_with_tokens().map(|c| visible_width(&c)).sum(),
    }
}

fn trailing_blank(n: &SyntaxNode) -> usize {
    match n.last_child_or_token() {
        Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::WHITESPACE => string_width(t.text()),
        _ => 0,
    }
}

/// A row of `org-table-to-lisp` with the widths of its fields.
enum Row {
    Hline,
    Fields(Vec<(String, usize)>),
}

/// `org-table-to-lisp` for the table text, with visible widths.
fn rows(table: &str, ctx: &ParseContext) -> Vec<Row> {
    let parse = org_syntax::parse_with(table, ctx);
    let root = parse.syntax();
    let mut out = Vec::new();
    let Some(t) = root.descendants().find(|n| n.kind() == SyntaxKind::TABLE) else {
        return out;
    };
    for row in t.children().filter(|c| c.kind() == SyntaxKind::TABLE_ROW) {
        if row.children().all(|c| c.kind() != SyntaxKind::TABLE_CELL)
            && row.text().to_string().trim_start().starts_with("|-")
        {
            out.push(Row::Hline);
            continue;
        }
        let mut fields = Vec::new();
        for cell in row
            .children()
            .filter(|c| c.kind() == SyntaxKind::TABLE_CELL)
        {
            let els: Vec<SyntaxElement> = cell.children_with_tokens().collect();
            // Content: without the padding and the closing bar.
            let mut a = 0;
            let mut b = els.len();
            if b > 0 && els[b - 1].kind() == SyntaxKind::MARKER {
                b -= 1;
            }
            while a < b && els[a].kind() == SyntaxKind::WHITESPACE {
                a += 1;
            }
            while b > a && els[b - 1].kind() == SyntaxKind::WHITESPACE {
                b -= 1;
            }
            let text: String = els[a..b].iter().map(|e| e.to_string()).collect();
            let text = text.trim_end_matches([' ', '\t']).to_string();
            let w: usize = els[a..b].iter().map(visible_width).sum::<usize>();
            let w = w
                - (els[a..b]
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<String>()
                    .len()
                    - text.len())
                .min(w);
            fields.push((text, w));
        }
        out.push(Row::Fields(fields));
    }
    out
}

/// The field `i` of a row and its width (empty past the row's end).
fn cell_at(r: &[(String, usize)], i: usize) -> (&str, usize) {
    r.get(i).map_or(("", 0), |(c, w)| (c.as_str(), *w))
}

/// How `org-table-align` aligns a column with these (trimmed) data
/// fields: by the first `<l>`, `<r>` or `<c>` cookie, else right (`'r'`)
/// when at least half of the non-empty fields are numbers, else left.
pub fn column_alignment<'a>(cells: impl IntoIterator<Item = &'a str>) -> char {
    let (mut numbers, mut non_empty) = (0usize, 0usize);
    for cell in cells {
        if cell.is_empty() {
            continue;
        }
        let cookie = cell
            .strip_prefix('<')
            .and_then(|c| c.strip_suffix('>'))
            .filter(|c| {
                let mut ch = c.chars();
                matches!(ch.next(), Some('l' | 'r' | 'c')) && ch.all(|x| x.is_ascii_digit())
            });
        if let Some(c) = cookie {
            return c.chars().next().expect("a cookie letter");
        }
        non_empty += 1;
        if is_number(cell) {
            numbers += 1;
        }
    }
    if numbers as f64 >= 0.5 * non_empty as f64 {
        'r'
    } else {
        'l'
    }
}

/// `org-table-number-regexp`, case-insensitively.
pub fn is_number(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    let t = l.strip_prefix(['<', '>']).unwrap_or(&l);
    // `[-+^.0-9]*[0-9][-+^.0-9eEdDx()%:]*`
    let first = t.find(|c: char| c.is_ascii_digit());
    if let Some(i) = first
        && t[..i].chars().all(|c| matches!(c, '-' | '+' | '^' | '.'))
        && t[i..]
            .chars()
            .all(|c| c.is_ascii_digit() || "-+^.eEdDx()%:".contains(c))
    {
        return true;
    }
    // `[-+]?0[xX][[:xdigit:].]+`
    let u = t.strip_prefix(['-', '+']).unwrap_or(t);
    if let Some(h) = u.strip_prefix("0x")
        && !h.is_empty()
        && h.chars().all(|c| c.is_ascii_hexdigit() || c == '.')
    {
        return true;
    }
    // `[-+]?[0-9]+#[0-9a-zA-Z.]+`
    if let Some((a, b)) = u.split_once('#')
        && !a.is_empty()
        && a.chars().all(|c| c.is_ascii_digit())
        && !b.is_empty()
        && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
    {
        return true;
    }
    l == "nan" || ["inf", "-inf", "+inf", "uinf"].contains(&l.as_str())
}

/// `org-table-align` on the table `begin..end` of `buf`. Returns the end
/// of the table after the change.
fn align(buf: &mut Buf, begin: usize, end: usize, ctx: &ParseContext) -> usize {
    let table = buf.text[begin..end].to_string();
    let rows = rows(&table, ctx);
    let data: Vec<&Vec<(String, usize)>> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Fields(f) => Some(f),
            Row::Hline => None,
        })
        .collect();
    let (widths, aligns): (Vec<usize>, Vec<char>) = if data.is_empty() {
        let first = &table[..table.find('\n').unwrap_or(table.len())];
        let n = 1 + first.matches('+').count();
        (vec![1; n], vec!['l'; n])
    } else {
        let n = data.iter().map(|r| r.len()).max().unwrap_or(0);
        (0..n)
            .map(|i| {
                let max = data
                    .iter()
                    .map(|r| cell_at(r, i).1)
                    .max()
                    .unwrap_or(0)
                    .max(1);
                (max, column_alignment(data.iter().map(|r| cell_at(r, i).0)))
            })
            .unzip()
    };
    let indent = {
        let first = &table[..table.find('|').unwrap_or(0) + 1];
        first.to_string()
    };
    let rule = widths
        .iter()
        .map(|w| "-".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("+");
    let mut p = begin;
    for row in &rows {
        let e = eol(&buf.text, p);
        let new = match row {
            Row::Hline => format!("{indent}{rule}|"),
            Row::Fields(f) => {
                let fields: Vec<String> = (0..widths.len())
                    .map(|i| {
                        let (cell, w) = f.get(i).map_or(("", 0), |(c, w)| (c.as_str(), *w));
                        let spaces = widths[i].saturating_sub(w);
                        let prefix = match aligns[i] {
                            'r' => spaces,
                            'c' => spaces / 2,
                            _ => 0,
                        };
                        format!(
                            " {}{cell}{} ",
                            " ".repeat(prefix),
                            " ".repeat(spaces - prefix)
                        )
                    })
                    .collect();
                format!("{indent}{}|", fields.join("|"))
            }
        };
        if buf.text[p..e] == new {
            p = next_line(&buf.text, p);
        } else {
            // `(insert new "\n")` then delete the old line with its line feed.
            buf.insert_before_point(p, &format!("{new}\n"));
            let s = p + new.len() + 1;
            let old_end = next_line(&buf.text, s);
            buf.delete(s, old_end);
            p = s;
        }
    }
    p
}

/// `org-table-save-field` around `f`: point returns to the start of the
/// same field.
fn with_saved_field(buf: &mut Buf, f: impl FnOnce(&mut Buf)) {
    let line = buf.add_marker(bol(&buf.text, buf.point));
    let column = current_column(&buf.text, buf.point);
    f(buf);
    let b = buf.marker(line);
    buf.point = goto_column(&buf.text, b, column);
}

fn setup(doc: &Document, point: usize) -> Result<(String, &ParseContext), EditError> {
    let text = doc.parse().syntax().to_string();
    if !at_table(doc, &text, point) {
        return Err(EditError::new("Not at a table"));
    }
    Ok((text, doc.parse().context()))
}

/// Aligns the table around `buf.point`.
fn align_here(buf: &mut Buf, ctx: &ParseContext) {
    let begin = table_begin(&buf.text, buf.point);
    let end = table_end(&buf.text, buf.point);
    with_saved_field(buf, |buf| {
        align(buf, begin, end, ctx);
    });
}

/// `org-table-align`.
pub fn align_table(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    align_here(&mut buf, ctx);
    Ok(buf.transaction("Align table"))
}

/// `org-table-create` with a size: a table of `columns` × `rows` empty
/// fields, with a rule after the first row when there are more, on the
/// line at point when only blanks precede point (indented to point's
/// column), and aligned.
///
/// In the middle of a line, Emacs inserts the table after the line break
/// but the rule before the table's first row, and aligns from outside the
/// table; Kalem breaks the line and creates the table at the start of the
/// new line, indented like the broken one (docs/known-differences.org).
pub fn create_table(
    doc: &Document,
    point: usize,
    columns: usize,
    rows: usize,
) -> Result<Transaction, EditError> {
    if columns == 0 || rows == 0 {
        return Err(EditError::new("Invalid table size"));
    }
    let text = doc.parse().syntax().to_string();
    let ctx = doc.parse().context();
    let mut buf = Buf::new(&text, point);
    let bol = buf.bol(point);
    let blank_before = text[bol..point].bytes().all(|b| matches!(b, b' ' | b'\t'));
    let (start, width) = if blank_before {
        (bol, crate::buffer::column_at(&text, point))
    } else {
        let ws = text[bol..]
            .bytes()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
        buf.insert_at_point("\n");
        (buf.point, crate::buffer::column_at(&text, bol + ws))
    };
    let pos = if blank_before { point } else { start };
    let line = format!("{}|{}\n", " ".repeat(width), "  |".repeat(columns));
    buf.replace(start, start, &line.repeat(rows));
    if rows > 1 {
        let eol = buf.eol(pos);
        buf.replace(eol, eol, "\n|-");
    }
    buf.point = pos;
    align_here(&mut buf, ctx);
    Ok(buf.transaction("Create table"))
}

/// `org-table-clean-line`: a table line with only bars and spaces.
fn clean_line(line: &str, ctx: &ParseContext) -> String {
    if line.trim_start_matches([' ', '\t']).starts_with("|-") {
        return line
            .chars()
            .map(|c| if matches!(c, '|' | '+') { '|' } else { ' ' })
            .collect();
    }
    // Fields become as many spaces as they are wide.
    let widths: Vec<usize> = match rows(&format!("{line}\n"), ctx).into_iter().next() {
        Some(Row::Fields(f)) => f.iter().map(|(_, w)| *w).collect(),
        _ => Vec::new(),
    };
    let mut out = String::new();
    let mut rest = line;
    let mut k = 0;
    // Everything up to the first bar stays.
    match rest.find('|') {
        Some(i) => {
            out.push_str(&rest[..=i]);
            rest = &rest[i + 1..];
        }
        None => return line.to_string(),
    }
    while let Some(i) = rest.find('|') {
        let field = &rest[..i];
        if field.trim_matches([' ', '\t']).is_empty() {
            out.push_str(field);
        } else {
            // `|\([ \t]*?[^ \t\r\n|][^\r\n|]*\)|`: from the bar on, the
            // whole field, blanks included, measured.
            out.push_str(&" ".repeat(
                widths.get(k).copied().unwrap_or(0)
                    + (field.len() - field.trim_start_matches([' ', '\t']).len())
                    + (field.len() - field.trim_end_matches([' ', '\t']).len()),
            ));
        }
        out.push('|');
        rest = &rest[i + 1..];
        k += 1;
    }
    out.push_str(rest);
    out
}

/// `org-table-insert-row`: an empty row above the current one, or below
/// with `below`.
pub fn insert_row(doc: &Document, point: usize, below: bool) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    insert_row_in(&mut buf, below, ctx);
    Ok(buf.transaction("Insert row"))
}

fn insert_row_in(buf: &mut Buf, below: bool, ctx: &ParseContext) {
    if buf.point == buf.text.len() {
        let p = buf.point;
        buf.insert_before_point(p, "\n");
    }
    let b = bol(&buf.text, buf.point);
    let line = &buf.text[b..eol(&buf.text, b)];
    if !line.trim_end_matches([' ', '\t']).ends_with('|') {
        align_here(buf, ctx);
    }
    let b = bol(&buf.text, buf.point);
    let line = buf.text[b..eol(&buf.text, b)].to_string();
    let mut new = clean_line(&line, ctx);
    // `^[ \t]*| *[#*$] *|`: a marking column is kept.
    let t = line.trim_start_matches([' ', '\t']);
    if let Some(r) = t.strip_prefix('|') {
        let r2 = r.trim_start_matches(' ');
        if r2.starts_with(['#', '*', '$']) && r2[1..].trim_start_matches(' ').starts_with('|') {
            let m_len = (line.len() - t.len())
                + 1
                + (r.len() - r2.len())
                + 1
                + (r2[1..].len() - r2[1..].trim_start_matches(' ').len())
                + 1;
            new = format!("{}{}", &line[..m_len], &new[m_len.min(new.len())..]);
        }
    }
    let mut p = if below { next_line(&buf.text, b) } else { b };
    if !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
        buf.insert_before_point(p, "\n");
        p += 1;
    }
    buf.replace_before_markers(p, p, &format!("{new}\n"));
    // `(forward-line -1) (re-search-forward "| ?" …)`.
    let nb = p;
    let ne = eol(&buf.text, nb);
    buf.point = match buf.text[nb..ne].find('|') {
        Some(i) => nb + i + 1 + usize::from(buf.text.as_bytes().get(nb + i + 1) == Some(&b' ')),
        None => nb,
    };
    // `org-table-may-need-update` is set by any change, so the table is
    // aligned.
    align_here(buf, ctx);
    let nb = bol(&buf.text, buf.point);
    let dline = data_line_number(&buf.text, nb);
    fix_formulas(buf, nb, '@', &[], Some(dline - 1), 1, None);
}

/// `org-table-current-dline` for the line at `b`.
fn data_line_number(text: &str, b: usize) -> usize {
    let begin = table_begin(text, b);
    let mut c = 0;
    let mut p = begin;
    while p <= b && p < text.len() {
        let l = &text[p..eol(text, p)];
        let t = l.trim_start_matches([' ', '\t']);
        if t.starts_with('|') && !t[1..].starts_with('-') {
            c += 1;
        }
        let n = next_line(text, p);
        if n == p {
            break;
        }
        p = n;
    }
    c
}

/// `org-table-fix-formulas`: in the `#+TBLFM` lines after the table at
/// `pos`, references `KEY N` are renamed by `replace`, or shifted by
/// `delta` above `limit`; with `remove`, formulas for that row or column
/// go.
fn fix_formulas(
    buf: &mut Buf,
    pos: usize,
    key: char,
    replace: &[(usize, Option<usize>)],
    limit: Option<usize>,
    delta: isize,
    remove: Option<usize>,
) {
    let mut l = table_end(&buf.text, pos);
    loop {
        if l >= buf.text.len() {
            break;
        }
        let e = eol(&buf.text, l);
        let line = buf.text[l..e].to_string();
        let t = line.trim_start_matches([' ', '\t']);
        if !(t.len() >= 8 && t.as_bytes()[..8].eq_ignore_ascii_case(b"#+tblfm:")) {
            break;
        }
        let mut new = line.clone();
        if let Some(r) = remove {
            new = remove_formulas(&new, key, r);
        }
        new = renumber(&new, key, replace, limit, delta);
        if new != line {
            buf.replace(l, e, &new);
        }
        l = next_line(&buf.text, l);
    }
}

/// `(org-in-regexp "remote([^)]+?)")` at `pos` of `line`.
fn in_remote(line: &str, pos: usize) -> bool {
    let mut from = 0;
    while let Some(i) = line[from..].find("remote(") {
        let s = from + i;
        let open = s + 7;
        let Some(k) = line[open..].find(')').filter(|k| *k > 0) else {
            from = open;
            continue;
        };
        let e = open + k + 1;
        if s > pos {
            return false;
        }
        if e >= pos {
            return true;
        }
        from = e;
    }
    false
}

/// Removes `(@N)?$R=…` (or `@R$N=…`) terms up to `::` or the end.
fn remove_formulas(line: &str, key: char, r: usize) -> String {
    let mut out = line.to_string();
    let pat = format!("{key}{r}");
    let mut from = 0;
    while let Some(i) = out[from..].find(&pat) {
        let at = from + i;
        // `$R=`, or `@R$N=`; not `$R0`.
        let after = &out[at + pat.len()..];
        let term_len = if key == '$' {
            after.starts_with('=').then_some(pat.len() + 1)
        } else {
            after.strip_prefix('$').and_then(|a| {
                let d = a.bytes().take_while(u8::is_ascii_digit).count();
                (d > 0 && a[d..].starts_with('=')).then_some(pat.len() + 1 + d + 1)
            })
        };
        let Some(_) = term_len else {
            from = at + pat.len();
            continue;
        };
        let mut start = at;
        if key == '$' {
            // An `@N` before it belongs to the term.
            let before = &out[..at];
            let digits = before.len() - before.trim_end_matches(|c: char| c.is_ascii_digit()).len();
            if digits > 0 && before[..before.len() - digits].ends_with('@') {
                start = at - digits - 1;
            }
        } else {
            // `@R$N=`: the pattern includes the column.
        }
        let end = out[at..].find("::").map_or(out.len(), |k| at + k + 2);
        // Emacs checks where the match ends against `remote(...)`.
        if in_remote(&out, end) {
            from = at + pat.len();
            continue;
        }
        out.replace_range(start..end, "");
        from = start;
    }
    out
}

/// Renumbers `KEY N` references, outside `remote(...)`.
fn renumber(
    line: &str,
    key: char,
    replace: &[(usize, Option<usize>)],
    limit: Option<usize>,
    delta: isize,
) -> String {
    let mut out = String::new();
    let mut i = 0;
    let b = line.as_bytes();
    while i < line.len() {
        let c = line[i..].chars().next().expect("char");
        if c == key {
            let d = b[i + 1..].iter().take_while(|x| x.is_ascii_digit()).count();
            if d > 0 {
                let in_remote = in_remote(line, i + 1 + d);
                let n: usize = line[i + 1..i + 1 + d].parse().unwrap_or(0);
                if !in_remote {
                    if let Some((_, to)) = replace.iter().find(|(from, _)| *from == n) {
                        out.push(key);
                        out.push_str(&to.map_or("INVALID".to_string(), |t| t.to_string()));
                        i += 1 + d;
                        continue;
                    }
                    if limit.is_some_and(|l| n > l) {
                        out.push(key);
                        out.push_str(&((n as isize + delta) as usize).to_string());
                        i += 1 + d;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `org-table-kill-row`.
pub fn kill_row(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, _) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    let b = bol(&text, point);
    let col = crate::buffer::column_at(&text, point) - crate::buffer::column_at(&text, b);
    let dline = (!hline(&text, b)).then(|| data_line_number(&text, b));
    let e = (eol(&text, b) + 1).min(text.len());
    buf.delete(b, e);
    let mut b2 = bol(&buf.text, buf.point);
    if !(b2 < buf.text.len() && table_line(&buf.text, b2)) && b2 > 0 {
        b2 = bol(&buf.text, b2 - 1);
    }
    buf.point = crate::buffer::move_to_column(&buf.text, b2, col);
    if let Some(d) = dline {
        fix_formulas(&mut buf, b2, '@', &[(d, None)], Some(d), -1, Some(d));
    }
    Ok(buf.transaction("Kill row"))
}

/// `org-table-move-row` down, or up with `up`.
pub fn move_row(doc: &Document, point: usize, up: bool) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let b = bol(&text, point);
    let col = crate::buffer::column_at(&text, point) - crate::buffer::column_at(&text, b);
    let hline1 = hline(&text, b);
    let dline1 = data_line_number(&text, b);
    let dline2 = if up {
        dline1.wrapping_sub(1)
    } else {
        dline1 + 1
    };
    if up && b == 0 {
        return Err(EditError::new("Cannot move row further"));
    }
    let other = if up {
        bol(&text, b - 1)
    } else {
        next_line(&text, b)
    };
    if (!up && (other >= text.len())) || !(other < text.len() && at_table(doc, &text, other)) {
        return Err(EditError::new("Cannot move row further"));
    }
    let hline2 = hline(&text, other);
    let mut buf = Buf::new(&text, point);
    let row_end = next_line(&text, b);
    let row = text[b..row_end].to_string();
    buf.delete(b, row_end);
    let mut p = if up { other } else { next_line(&buf.text, b) };
    if !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
        buf.insert_before_point(p, "\n");
        p += 1;
    }
    buf.insert_before_point(p, &row);
    let mut after = p + row.len();
    if !row.ends_with('\n') {
        buf.insert_before_point(after, "\n");
        after += 1;
    }
    let _ = after;
    buf.point = crate::buffer::move_to_column(&buf.text, p, col);
    if !(hline1 || hline2) {
        let (a, bb) = (dline1, dline2);
        fix_formulas(
            &mut buf,
            p,
            '@',
            &[(a, Some(bb)), (bb, Some(a))],
            None,
            0,
            None,
        );
    }
    Ok(buf.transaction("Move row"))
}

/// `org-table-insert-hline` below the current line, or above with `above`.
pub fn insert_hline(doc: &Document, point: usize, above: bool) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    if buf.point == buf.text.len() {
        let p = buf.point;
        buf.insert_before_point(p, "\n");
    }
    let b = bol(&buf.text, buf.point);
    if !buf.text[b..eol(&buf.text, b)]
        .trim_end_matches([' ', '\t'])
        .ends_with('|')
    {
        align_here(&mut buf, ctx);
    }
    let b = bol(&buf.text, buf.point);
    let col =
        crate::buffer::column_at(&buf.text, buf.point) - crate::buffer::column_at(&buf.text, b);
    let mut line = clean_line(&buf.text[b..eol(&buf.text, b)], ctx);
    // `|\( +\)|` → `+---|`, then the first `+` becomes `|`.
    while let Some((s, e)) = find_blank_field(&line) {
        line.replace_range(s..=e, &format!("+{}|", "-".repeat(e - s - 1)));
    }
    if let Some(i) = line.find('+') {
        line.replace_range(i..=i, "|");
    }
    let p = if above { b } else { next_line(&buf.text, b) };
    buf.insert_before_point(p, &format!("{line}\n"));
    // `(forward-line (if above 0 -2))`, from the line after the insertion.
    let target = if above {
        p + line.len() + 1
    } else {
        let after = p + line.len() + 1;
        let one = if after == 0 {
            0
        } else {
            bol(&buf.text, after - 1)
        };
        if one == 0 { 0 } else { bol(&buf.text, one - 1) }
    };
    buf.point = crate::buffer::move_to_column(&buf.text, target, col);
    Ok(buf.transaction("Insert horizontal rule"))
}

/// The first `|\( +\)|` in `s`: positions of the two bars.
fn find_blank_field(s: &str) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'|' {
            let n = b[i + 1..].iter().take_while(|c| **c == b' ').count();
            if n > 0 && b.get(i + 1 + n) == Some(&b'|') {
                return Some((i, i + 1 + n));
            }
        }
        i += 1;
    }
    None
}

/// `org-table-find-dataline`: from an hline, the next data line.
fn find_dataline(text: &str, point: usize) -> Result<usize, EditError> {
    let b = bol(text, point);
    if !hline(text, b) {
        return Ok(point);
    }
    let col = crate::buffer::column_at(text, point) - crate::buffer::column_at(text, b);
    let end = table_end(text, point);
    let mut p = next_line(text, b);
    while p < end && hline(text, p) {
        p = next_line(text, p);
    }
    if p >= end {
        return Err(EditError::at(
            "Cannot find data row for column operation",
            p,
        ));
    }
    Ok(crate::buffer::move_to_column(text, p, col))
}

/// `org-table-insert-column`: an empty column before the current one.
pub fn insert_column(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    if buf.point == buf.text.len() {
        let p = buf.point;
        buf.insert_before_point(p, "\n");
    }
    let b = bol(&buf.text, buf.point);
    if !buf.text[b..eol(&buf.text, b)]
        .trim_end_matches([' ', '\t'])
        .ends_with('|')
    {
        align_here(&mut buf, ctx);
    }
    buf.point = find_dataline(&buf.text, buf.point)?;
    let col = current_column(&buf.text, buf.point).max(1);
    let begin = table_begin(&buf.text, buf.point);
    let end_m = {
        let e = table_end(&buf.text, buf.point);
        buf.add_marker(e)
    };
    let mut l = begin;
    while l < buf.marker(end_m) {
        if !hline(&buf.text, l) {
            let p = goto_delim(&buf.text, l, col);
            // `(insert "|")` before the delimiter.
            buf.insert_before_point(p, "|");
        }
        l = next_line(&buf.text, l);
    }
    let b = bol(&buf.text, buf.point);
    buf.point = goto_column(&buf.text, b, col);
    align_here(&mut buf, ctx);
    let b = bol(&buf.text, buf.point);
    fix_formulas(&mut buf, b, '$', &[], Some(col - 1), 1, None);
    Ok(buf.transaction("Insert column"))
}

/// `org-table-delete-column`.
pub fn delete_column(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    buf.point = find_dataline(&text, point)?;
    // At the end of the line: into the last column.
    let e = eol(&buf.text, buf.point);
    if buf.text[buf.point..e].trim_matches([' ', '\t']).is_empty()
        && let Some(i) = buf.text[..buf.point].rfind('|')
    {
        buf.point = i;
    }
    check_data_field(&buf.text, buf.point).map_err(|e| EditError::at(&e.message, buf.point))?;
    let col = current_column(&buf.text, buf.point);
    let begin = table_begin(&buf.text, buf.point);
    let end_m = {
        let e = table_end(&buf.text, buf.point);
        buf.add_marker(e)
    };
    with_saved_field(&mut buf, |buf| {
        let mut l = begin;
        while l < buf.marker(end_m) {
            if !hline(&buf.text, l) {
                let p = goto_delim(&buf.text, l, col);
                // `|[^|\n]+|` → `|`.
                let e = eol(&buf.text, p);
                if let Some(k) = buf.text[p + 1..e].find('|')
                    && k > 0
                {
                    buf.replace(p, p + 1 + k + 1, "|");
                }
            }
            l = next_line(&buf.text, l);
        }
    });
    align_here(&mut buf, ctx);
    let b = bol(&buf.text, buf.point);
    fix_formulas(&mut buf, b, '$', &[(col, None)], Some(col), -1, Some(col));
    Ok(buf.transaction("Delete column"))
}

/// `org-table-check-inside-data-field` in a table.
fn check_data_field(text: &str, p: usize) -> Result<(), EditError> {
    let b = bol(text, p);
    let before_blank = text[b..p].trim_matches([' ', '\t']).is_empty();
    let after_blank = text[p..eol(text, p)].trim_matches([' ', '\t']).is_empty();
    if before_blank || hline(text, b) || after_blank {
        return Err(EditError::new("Not in table data field"));
    }
    Ok(())
}

/// `org-duration-to-minutes` for what `org-duration-p` accepts: `H:MM`,
/// `H:MM:SS`, and amounts with units (`2h`, `1d 3.5h`, `45min`),
/// optionally followed by `H:MM`.
fn duration_minutes(s: &str) -> Option<f64> {
    let s = s.trim();
    let hms = |t: &str| -> Option<f64> {
        let parts: Vec<&str> = t.split(':').collect();
        if !(2..=3).contains(&parts.len())
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()))
            || parts[1].len() != 2
            || parts.get(2).is_some_and(|p| p.len() != 2)
        {
            return None;
        }
        let n = |p: &str| p.parse::<f64>().ok();
        Some(
            n(parts[0])? * 60. + n(parts[1])? + parts.get(2).and_then(|p| n(p)).unwrap_or(0.) / 60.,
        )
    };
    if let Some(m) = hms(s) {
        return Some(m);
    }
    const UNITS: [(&str, f64); 6] = [
        ("min", 1.),
        ("h", 60.),
        ("d", 1440.),
        ("w", 10080.),
        ("m", 43200.),
        ("y", 525_960.),
    ];
    let mut total = 0.;
    let mut rest = s;
    let mut any = false;
    while !rest.is_empty() {
        let num_len = rest
            .bytes()
            .take_while(|c| c.is_ascii_digit() || *c == b'.')
            .count();
        if num_len == 0 {
            // A final `H:MM`.
            return if any {
                hms(rest).map(|m| total + m)
            } else {
                None
            };
        }
        let after = rest[num_len..].trim_start();
        let Some((unit, factor)) = UNITS.iter().find(|(u, _)| {
            after.starts_with(u) && !after[u.len()..].starts_with(|c: char| c.is_alphabetic())
        }) else {
            return if any {
                hms(rest).map(|m| total + m)
            } else {
                None
            };
        };
        total += rest[..num_len].parse::<f64>().ok()? * factor;
        any = true;
        rest = after[unit.len()..].trim_start();
    }
    any.then_some(total)
}

/// A sorting key of `org-table-sort-lines` for a time: seconds for a
/// timestamp, minutes for a duration, else 0.
fn time_key(field: &str) -> f64 {
    let b = field.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if !matches!(c, b'<' | b'[') {
            continue;
        }
        let close = if c == b'<' { '>' } else { ']' };
        let rest = &field[i + 1..];
        let Some(end) = rest.find(['>', ']']) else {
            continue;
        };
        if !rest[end..].starts_with(close) && !rest[end..].starts_with(['>', ']']) {
            continue;
        }
        if let Some(dt) = org_model::time::parse_time_string(&field[i..i + 1 + end + 1]) {
            return dt
                .to_zoned(jiff::tz::TimeZone::UTC)
                .map_or(0.0, |z| z.timestamp().as_second() as f64);
        }
    }
    if let Some(m) = duration_minutes(field) {
        return m;
    }
    // `\<[0-9]+:[0-9]\{2\}\>`
    for (i, _) in field.match_indices(':') {
        let before = field[..i]
            .bytes()
            .rev()
            .take_while(u8::is_ascii_digit)
            .count();
        let after = field[i + 1..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let word_start = i - before == 0
            || !field[..i - before]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        let word_end = !field[i + 1 + after..].starts_with(|c: char| c.is_alphanumeric());
        if before > 0 && after == 2 && word_start && word_end {
            return duration_minutes(&field[i - before..i + 3]).unwrap_or(0.);
        }
    }
    0.
}

/// A sorting key.
#[derive(Debug, Clone, PartialEq)]
enum SortKey {
    Num(f64),
    Text(String),
}

/// `org-table-sort-lines`: the rows between the rules around point sorted
/// by the column at point, `kind` being `a` (alphabetically), `n`
/// (numerically) or `t` (by time or duration), in capitals to reverse;
/// `with_case` keeps letter case apart. Point stays on the same line and
/// column.
pub fn sort_rows(
    doc: &Document,
    point: usize,
    kind: char,
    with_case: bool,
) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let b = bol(&text, point);
    let mut q = point;
    while q > b && matches!(text.as_bytes()[q - 1], b' ' | b'\t') {
        q -= 1;
    }
    if q == b
        && let Some(i) = text[b..eol(&text, b)].find('|')
    {
        q = b + i + 1;
    }
    check_data_field(&text, q)?;
    let column = current_column(&text, point).max(1);
    let reverse = kind.is_ascii_uppercase();
    let kind = kind.to_ascii_lowercase();
    if !matches!(kind, 'a' | 'n' | 't') {
        return Err(EditError::new(&format!("Invalid sorting type `{kind}'")));
    }
    let start = table_begin(&text, point);
    let end = table_end(&text, point);
    // The rows between the nearest rules.
    let mut from = start;
    let mut l = b;
    while l > start {
        let prev = bol(&text, l - 1);
        if hline(&text, prev) {
            from = next_line(&text, prev);
            break;
        }
        l = prev;
    }
    let mut to = end;
    let mut l = next_line(&text, b);
    while l < end {
        if hline(&text, l) {
            to = l;
            break;
        }
        l = next_line(&text, l);
    }
    let mut lines: Vec<&str> = Vec::new();
    let mut l = from;
    while l < to {
        let e = eol(&text, l).min(to);
        lines.push(&text[l..e]);
        l = next_line(&text, l);
    }
    let key = |line: &str| -> SortKey {
        // `org-table-get-field`: after the COLUMNth bar, or after the last
        // one on a shorter line, up to the next bar.
        let bars: Vec<usize> = line.match_indices('|').map(|(i, _)| i).collect();
        let field = match bars.get(column - 1).or(bars.last()) {
            Some(&a) => {
                let rest = &line[a + 1..];
                rest[..rest.find('|').unwrap_or(rest.len())].trim()
            }
            None => "",
        };
        match kind {
            'n' => SortKey::Num(org_table::emacs::string_to_number(field).to_f64()),
            't' => SortKey::Num(time_key(field)),
            _ => {
                let v = crate::sort::remove_invisible(field, ctx);
                SortKey::Text(if with_case { v } else { v.to_lowercase() })
            }
        }
    };
    let mut records: Vec<(SortKey, &str)> = lines.iter().map(|l| (key(l), *l)).collect();
    let order = |a: &SortKey, b: &SortKey| match (a, b) {
        (SortKey::Num(x), SortKey::Num(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (SortKey::Text(x), SortKey::Text(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    };
    if reverse {
        records.sort_by(|a, b| order(&b.0, &a.0));
    } else {
        records.sort_by(|a, b| order(&a.0, &b.0));
    }
    let sorted: Vec<&str> = records.iter().map(|(_, l)| *l).collect();
    let mut buf = Buf::new(&text, point);
    // Line by line from the bottom, so offsets above stay valid.
    let mut starts = Vec::new();
    let mut l = from;
    while l < to {
        starts.push(l);
        l = next_line(&text, l);
    }
    for (i, s) in starts.iter().enumerate().rev() {
        let e = eol(&text, *s).min(to);
        if text[*s..e] != *sorted[i] {
            buf.delete(*s, e);
            buf.insert_before_point(*s, sorted[i]);
        }
    }
    // The same line and display column as before (`forward-line` from
    // the region's start, `move-to-column`; hidden link text takes no
    // columns).
    let line_index = starts.iter().rposition(|&s| s <= point).unwrap_or(0);
    let col = crate::buffer::column_at(&text, point);
    let mut line_start = from;
    for _ in 0..line_index {
        line_start = next_line(&buf.text, line_start);
    }
    buf.point = crate::buffer::move_to_column(&buf.text, line_start, col);
    Ok(buf.transaction("Sort rows"))
}

/// `org-table-move-column` to the right, or left with `left`.
pub fn move_column(doc: &Document, point: usize, left: bool) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    buf.point = find_dataline(&text, point)?;
    check_data_field(&buf.text, buf.point).map_err(|e| EditError::at(&e.message, buf.point))?;
    let col = current_column(&buf.text, buf.point);
    let col1 = if left { col - 1 } else { col };
    let colpos = if left { col - 1 } else { col + 1 };
    if left && col == 1 {
        return Err(EditError::at("Cannot move column further left", buf.point));
    }
    // `[^|\n]*|[^|\n]*$`: the last column.
    let rest = &buf.text[buf.point..eol(&buf.text, buf.point)];
    if !left && rest.matches('|').count() == 1 {
        return Err(EditError::at("Cannot move column further right", buf.point));
    }
    let begin = table_begin(&buf.text, buf.point);
    let end_m = {
        let e = table_end(&buf.text, buf.point);
        buf.add_marker(e)
    };
    with_saved_field(&mut buf, |buf| {
        let mut l = begin;
        while l < buf.marker(end_m) {
            if !hline(&buf.text, l) {
                let p = goto_delim(&buf.text, l, col1);
                // `|\([^|\n]+\)|\([^|\n]+\)|`: swap the two fields.
                let e = eol(&buf.text, p);
                let s = &buf.text[p..e];
                let parts: Vec<&str> = s.splitn(4, '|').collect();
                if parts.len() >= 4
                    && parts[0].is_empty()
                    && !parts[1].is_empty()
                    && !parts[2].is_empty()
                {
                    let a = parts[1].to_string();
                    let bb = parts[2].to_string();
                    let a_start = p + 1;
                    let b_start = a_start + a.len() + 1;
                    // `transpose-regions` keeps markers with their text.
                    let pt = buf.point;
                    buf.replace(a_start, b_start + bb.len(), &format!("{bb}|{a}"));
                    if pt >= a_start && pt < a_start + a.len() {
                        buf.point = pt + bb.len() + 1;
                    } else if pt >= b_start && pt < b_start + bb.len() {
                        buf.point = pt - a.len() - 1;
                    } else {
                        buf.point = pt;
                    }
                }
            }
            l = next_line(&buf.text, l);
        }
    });
    let b = bol(&buf.text, buf.point);
    buf.point = goto_column(&buf.text, b, colpos);
    align_here(&mut buf, ctx);
    let b = bol(&buf.text, buf.point);
    fix_formulas(
        &mut buf,
        b,
        '$',
        &[(col, Some(colpos)), (colpos, Some(col))],
        None,
        0,
        None,
    );
    Ok(buf.transaction("Move column"))
}

/// The motion of `org-table-maybe-eval-formula`, which reads the field
/// with `org-table-get-field`: back to the start of the field, then one
/// character in, unless the field is empty or point is before the table.
fn get_field_motion(buf: &mut Buf) {
    let t = &buf.text;
    let b = bol(t, buf.point);
    let q = t[b..buf.point].rfind('|').map_or(b, |i| b + i + 1);
    let e = eol(t, q);
    buf.point = if q == b || t[q..e].trim_matches([' ', '\t']).is_empty() {
        q
    } else {
        (q + 1).min(e)
    };
}

/// `org-table-next-field` (TAB): aligns, then goes to the next field,
/// adding a row at the end of the table.
pub fn next_field(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    get_field_motion(&mut buf);
    align_here(&mut buf, ctx);
    let end = table_end(&buf.text, buf.point);
    let mut p = buf.point;
    if hline(&buf.text, bol(&buf.text, p)) {
        p = eol(&buf.text, p);
    }
    let search = |t: &str, from: usize| t[from..end.min(t.len())].find('|').map(|i| from + i + 1);
    let r = (|| {
        let mut q = search(&buf.text, p)?;
        if buf.text[q..eol(&buf.text, q)]
            .trim_matches([' ', '\t'])
            .is_empty()
        {
            q = search(&buf.text, q)?;
        }
        Some(q)
    })();
    match r {
        Some(q) => {
            if buf.text.as_bytes().get(q) == Some(&b'-') {
                // At an hline: jump over it to the next data line, if any.
                let mut l = next_line(&buf.text, bol(&buf.text, q));
                let mut target = None;
                while l < end {
                    let t = &buf.text[l..eol(&buf.text, l)];
                    let tt = t.trim_start_matches([' ', '\t']);
                    if tt.starts_with('|') && !tt[1..].starts_with('-') && !tt[1..].is_empty() {
                        target = Some(l + (t.len() - tt.len()) + 1);
                        break;
                    }
                    l = next_line(&buf.text, l);
                }
                match target {
                    Some(t) => {
                        buf.point = t;
                        if buf.text.as_bytes().get(t) == Some(&b' ') {
                            buf.point += 1;
                        }
                    }
                    None => {
                        let b = bol(&buf.text, q);
                        buf.point = if b == 0 { 0 } else { bol(&buf.text, b - 1) };
                        insert_row_in(&mut buf, true, ctx);
                    }
                }
            } else {
                buf.point = q + usize::from(buf.text.as_bytes().get(q) == Some(&b' '));
            }
        }
        None => insert_row_in(&mut buf, true, ctx),
    }
    Ok(buf.transaction("Next field"))
}

/// `org-table-previous-field` (S-TAB).
pub fn previous_field(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    align_here(&mut buf, ctx);
    if hline(&buf.text, bol(&buf.text, buf.point)) {
        buf.point = eol(&buf.text, buf.point);
    }
    let start = table_begin(&buf.text, buf.point);
    // `(search-backward "|" start nil 2)`: the second bar before point.
    let t = &buf.text;
    let find_back = |from: usize| t[start..from].rfind('|').map(|i| start + i);
    let r = (|| {
        let mut q = find_back(buf.point)?;
        q = find_back(q)?;
        loop {
            let rest = &t[q + 1..eol(t, q)];
            if rest.starts_with('-') || rest.trim_matches([' ', '\t']).is_empty() {
                q = find_back(q)?;
            } else {
                return Some(q);
            }
        }
    })();
    // Emacs returns to where it was after aligning.
    let Some(q) = r else {
        return Err(EditError::at(
            "Cannot move to previous table field",
            buf.point,
        ));
    };
    buf.point = q + 1 + usize::from(buf.text.as_bytes().get(q + 1) == Some(&b' '));
    Ok(buf.transaction("Previous field"))
}

/// `org-table-next-row` (RET): aligns, then goes to the same column on the
/// next row, adding a row when there is none or an hline follows.
pub fn next_row(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc, point)?;
    let mut buf = Buf::new(&text, point);
    get_field_motion(&mut buf);
    align_here(&mut buf, ctx);
    let col = current_column(&buf.text, buf.point);
    let mut n = next_line(&buf.text, bol(&buf.text, buf.point));
    if !(n == 0 || buf.text.as_bytes()[n - 1] == b'\n') {
        buf.insert_before_point(n, "\n");
        n += 1;
    }
    let text_now = buf.text.clone();
    let doc2 = Document::new(org_syntax::parse_with(&text_now, ctx));
    let ok = n < text_now.len() && at_table(&doc2, &text_now, n) && !hline(&text_now, n);
    if ok {
        buf.point = n;
    } else {
        buf.point = bol(&buf.text, n.saturating_sub(1));
        insert_row_in(&mut buf, true, ctx);
        n = bol(&buf.text, buf.point);
    }
    let p = goto_column(&buf.text, n, col);
    // `(skip-chars-backward "^|\n\r")`, then one space.
    let q = buf.text[..p].rfind(['|', '\n', '\r']).map_or(0, |i| i + 1);
    buf.point = q + usize::from(buf.text.as_bytes().get(q) == Some(&b' '));
    Ok(buf.transaction("Next row"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        text: &str,
        p: usize,
        f: fn(&Document, usize) -> Result<Transaction, EditError>,
    ) -> String {
        let doc = Document::new(org_syntax::parse(text));
        f(&doc, p).unwrap().apply(text)
    }

    #[test]
    fn aligns() {
        assert_eq!(
            run("| a | bb |\n|-\n| 10 | x |\n", 2, align_table),
            "|  a | bb |\n|----+----|\n| 10 | x  |\n"
        );
        assert_eq!(
            run("|1|2|\n|333|x|\n", 1, align_table),
            "|   1 | 2 |\n| 333 | x |\n"
        );
        assert_eq!(
            run("| [[https://x][ab]] | c |\n| d | e |\n", 2, align_table),
            "| [[https://x][ab]] | c |\n| d  | e |\n"
        );
        assert!(
            is_number("-1.5e3") && is_number("12:30") && is_number("0x1F") && !is_number("abc")
        );
        // Characters of more than one byte before the digits.
        assert!(!is_number("±0.5") && !is_number("≈ 12"));
        assert_eq!(
            run("| ±0.5 | a |\n| 1 | bb |\n", 2, align_table),
            "| ±0.5 | a  |\n|    1 | bb |\n"
        );
    }

    #[test]
    fn create_mid_line() {
        let create = |t: &str, p: usize, c: usize, r: usize| {
            let doc = Document::new(org_syntax::parse(t));
            let tx = create_table(&doc, p, c, r).unwrap();
            (tx.apply(t), tx.selection_after.map(|s| s.head))
        };
        assert_eq!(
            create("Some text\n", 4, 2, 2),
            (
                "Some\n|   |   |\n|---+---|\n|   |   |\n text\n".into(),
                Some(5)
            )
        );
        assert_eq!(
            create("  - item", 8, 1, 1),
            ("  - item\n  |   |\n".into(), Some(9))
        );
        let doc = Document::new(org_syntax::parse("x"));
        assert!(create_table(&doc, 0, 0, 2).is_err());
    }
}
