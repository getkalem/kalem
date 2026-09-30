//! Table formulas (`org-table-recalculate` with a prefix): the `#+TBLFM`
//! of the table at point applied to every row by [`org_table`], then the
//! table aligned, in one transaction.

use org_model::{Document, Inherit};
use org_syntax::{SyntaxKind, SyntaxNode, ast};
use org_table::formula::{DurationCustom, Env, Remote};
use org_table::table::{Row, Table};
use org_table::{recalc, tblfm};

use crate::buffer::EditError;
use crate::transaction::{Selection, Transaction};

/// A recalculation: the edit and what Kalem did not compute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recalculated {
    /// The new fields, aligned.
    pub transaction: Transaction,
    /// Left-hand sides of Emacs Lisp formulas, kept but not evaluated.
    pub lisp: Vec<String>,
    /// The error Emacs gives after changing the table ("No convergence
    /// after 10 iterations"): the transaction still holds the change.
    pub error: Option<String>,
}

fn range(n: &SyntaxNode) -> std::ops::Range<usize> {
    usize::from(n.text_range().start())..usize::from(n.text_range().end())
}

/// The rows of a table node: from its first row to the end of its last.
fn rows_of(table: &SyntaxNode) -> Option<std::ops::Range<usize>> {
    let mut it = table
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_ROW);
    let first = it.next()?;
    let last = it.last().unwrap_or_else(|| first.clone());
    Some(range(&first).start..range(&last).end)
}

/// The table nodes of the document with a `#+NAME`.
struct Named<'a> {
    text: &'a str,
    tables: Vec<(String, std::ops::Range<usize>)>,
}

impl Remote for Named<'_> {
    fn table(&self, name: &str) -> Option<Table> {
        let (_, r) = self
            .tables
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))?;
        Some(Table::parse(&self.text[r.clone()]))
    }
}

/// `#+CONSTANTS` of the document: `name=value` pairs.
fn constants(doc: &Document) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (k, v) in doc.parse().keywords() {
        if k.eq_ignore_ascii_case("CONSTANTS") {
            for pair in v.split_whitespace() {
                if let Some((name, value)) = pair.split_once('=') {
                    out.push((name.to_string(), value.to_string()));
                }
            }
        }
    }
    out
}

/// The table at `point`, its rows' range and the active `#+TBLFM` value.
fn table_at(
    doc: &Document,
    point: usize,
) -> Result<(SyntaxNode, std::ops::Range<usize>, String), EditError> {
    let root = doc.parse().syntax();
    let text = root.to_string();
    let table = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TABLE)
        .find(|n| {
            let r = range(n);
            r.start <= point && point < r.end.max(r.start + 1)
        })
        .ok_or_else(|| EditError::new("Not at a table"))?;
    let rows = rows_of(&table).ok_or_else(|| EditError::new("Not at a table"))?;
    // On a later `#+TBLFM` line, that line's formulas
    // (`org-table-calc-current-TBLFM`); else the first line's.
    let formulas = tblfm_line_at(&text, rows.end, point)
        .or_else(|| tblfm::active_line(&text[rows.end..]).map(|(_, v)| v))
        .unwrap_or_default()
        .to_string();
    Ok((table, rows, formulas))
}

/// The value of the `#+TBLFM` line holding `point`, when it is one of
/// the lines after a table's rows (ending at `rows_end`).
fn tblfm_line_at(text: &str, rows_end: usize, point: usize) -> Option<&str> {
    if point < rows_end {
        return None;
    }
    let bol = text[..point.min(text.len())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let eol = text[bol..].find('\n').map_or(text.len(), |i| bol + i);
    let body = text[bol..eol].trim_start_matches([' ', '\t']);
    let key = body.get(..8)?;
    key.eq_ignore_ascii_case("#+tblfm:")
        .then(|| body[8..].trim_start_matches(' '))
}

/// The table's lines with new fields, indented like its first line; the
/// alignment that follows gives rules and widths.
fn table_text(t: &Table, indent: &str) -> String {
    let mut s = String::new();
    for r in &t.rows {
        s.push_str(indent);
        match r {
            Row::Rule => s.push_str("|-|"),
            Row::Data(f) => {
                s.push('|');
                for x in f {
                    s.push(' ');
                    s.push_str(x);
                    s.push_str(" |");
                }
            }
        }
        s.push('\n');
    }
    s
}

/// Recalculates the table at `point` (on its rows or its `#+TBLFM`
/// lines): once, or until it no longer changes when `iterate` (at most
/// ten times, as `C-u C-u C-c *`).
pub fn recalculate(doc: &Document, point: usize, iterate: bool) -> Result<Recalculated, EditError> {
    let (_, rows, formulas) = table_at(doc, point)?;
    let text = doc.parse().syntax().to_string();
    let equations = tblfm::parse(&formulas);
    if let Some(d) = equations.duplicates.first() {
        return Err(EditError::new(&format!(
            "Double definition `{d}=' in TBLFM line, please fix by hand"
        )));
    }
    let root = doc.parse().syntax();
    let named = Named {
        text: &text,
        tables: root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::TABLE)
            .filter_map(|n| {
                let name = ast::affiliated_keywords(&n)
                    .find(|k| k.key() == "NAME")?
                    .value();
                Some((name, rows_of(&n)?))
            })
            .collect(),
    };
    let consts = constants(doc);
    let entry = doc.outline().entry_at(rows.start);
    let property = |name: &str| doc.entry_get(entry, name, Inherit::Yes, false);
    let env = Env {
        remote: &named,
        constants: &consts,
        property: &property,
        duration_custom: DurationCustom::default(),
    };
    let table = Table::parse(&text[rows.clone()]);
    let result = if iterate {
        recalc::iterate(&table, &equations.equations, &env, 10)
    } else {
        recalc::recalculate(&table, &equations.equations, &env)
    };
    let (new, report) = result.map_err(|e| EditError::new(&e.0))?;
    let first_line = &text[rows.start..];
    let indent_len = first_line.len() - first_line.trim_start_matches([' ', '\t']).len();
    let indent = &first_line[..indent_len];
    let mut replaced = text.clone();
    replaced.replace_range(rows.clone(), &table_text(&new, indent));
    // Aligned as a document of its own, then put in place.
    let new_doc = Document::new(org_syntax::parse_with(&replaced, doc.parse().context()));
    let at = rows.start + indent_len;
    let aligned = crate::table::align_table(&new_doc, at)?.apply(&replaced);
    // The rows after alignment: everything else is unchanged.
    let new_end = aligned.len() - (text.len() - rows.end);
    let new_rows = &aligned[rows.start..new_end];
    let mut tx = Transaction::new("Recalculate table");
    if new_rows != &text[rows.clone()] {
        tx.replace(rows.clone(), new_rows).expect("one edit");
    }
    let caret = if point < rows.end {
        point.min(new_end)
    } else {
        point + new_end - rows.end
    };
    Ok(Recalculated {
        transaction: tx.select(Selection::caret(caret)),
        lisp: report.lisp,
        error: report.error,
    })
}

/// `org-table-convert-region`: the lines from `beg` to `end` (the lines
/// they touch) made into a table by `sep`, then aligned. The region is not
/// limited in size, unlike Emacs's `org-table-convert-region-max-lines`.
pub fn convert_region(
    doc: &Document,
    beg: usize,
    end: usize,
    sep: org_table::csv::Separator,
) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let (beg, end) = (beg.min(end), beg.max(end));
    let start = text[..beg].rfind('\n').map_or(0, |i| i + 1);
    let stop = if end > 0 && (end == text.len() || text.as_bytes()[end - 1] == b'\n') && end > start
    {
        // At the start of a line: the line before ends the region.
        if text.as_bytes().get(end - 1) == Some(&b'\n') {
            end - 1
        } else {
            end
        }
    } else {
        text[end..].find('\n').map_or(text.len(), |i| end + i)
    };
    let comma = match sep {
        org_table::csv::Separator::Comma => true,
        org_table::csv::Separator::Auto => {
            let lines: Vec<&str> = text[start..stop].split('\n').collect();
            let every = |c: char| lines.iter().all(|l| l.is_empty() || l.contains(c));
            !every('\t') && every(',')
        }
        _ => false,
    };
    let converted = org_table::csv::convert(&text[start..stop], sep);
    let mut replaced = text.clone();
    replaced.replace_range(start..stop, &converted);
    let new_doc = Document::new(org_syntax::parse_with(&replaced, doc.parse().context()));
    let aligned = crate::table::align_table(&new_doc, start)?.apply(&replaced);
    let mut tx = Transaction::new("Convert to table");
    let new_stop = aligned.len() - (text.len() - stop);
    tx.replace(start..stop, &aligned[start..new_stop])
        .expect("one edit");
    // Emacs ends before the first bar after converting CSV, in the first
    // field otherwise (its replacements carry the region's start along).
    let spaces = !comma && !matches!(sep, org_table::csv::Separator::Tab) && {
        let lines: Vec<&str> = text[start..stop].split('\n').collect();
        !(matches!(sep, org_table::csv::Separator::Auto)
            && lines.iter().all(|l| l.is_empty() || l.contains('\t')))
    };
    // Leading spaces make Emacs's first replacement non-empty, which keeps
    // the region's start before it.
    let keeps_start = comma || (spaces && text[start..].starts_with(' '));
    let caret = if keeps_start {
        start
    } else {
        (start + 2).min(new_stop)
    };
    Ok(tx.select(Selection::caret(caret)))
}

/// `org-table-import`: `content` (a file's text) inserted at `point`, on a
/// line of its own, and made into a table.
pub fn import(
    doc: &Document,
    point: usize,
    content: &str,
    sep: org_table::csv::Separator,
) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let mut inserted = String::new();
    if point > 0 && text.as_bytes()[point - 1] != b'\n' {
        inserted.push('\n');
    }
    let beg = point + inserted.len();
    inserted.push_str(content);
    let mut new_text = text.clone();
    new_text.insert_str(point, &inserted);
    let new_doc = Document::new(org_syntax::parse_with(&new_text, doc.parse().context()));
    let tx = convert_region(&new_doc, beg, point + inserted.len(), sep)?;
    let final_text = tx.apply(&new_text);
    let mut out = Transaction::new("Import table");
    let tail = text.len() - point;
    out.replace(point..point, &final_text[point..final_text.len() - tail])
        .expect("one edit");
    Ok(out.select(Selection::caret(beg)))
}

/// `org-table-export` to CSV or TSV: the text of the file for the table
/// at `point`, with a final line feed.
pub fn export(
    doc: &Document,
    point: usize,
    format: org_table::csv::Format,
) -> Result<String, EditError> {
    let (_, rows, _) = table_at(doc, point)?;
    let text = doc.parse().syntax().to_string();
    let table = Table::parse(&text[rows]);
    Ok(format!("{}\n", org_table::csv::export(&table, format)))
}

/// The table at `point` as the formula engine sees it, with what formulas
/// need.
struct Context {
    text: String,
    rows: std::ops::Range<usize>,
    formulas: String,
    table: Table,
    /// The line and column of `point` in the table.
    line: usize,
    col: usize,
}

fn context(doc: &Document, point: usize) -> Result<Context, EditError> {
    let (_, rows, formulas) = table_at(doc, point)?;
    let text = doc.parse().syntax().to_string();
    let table = Table::parse(&text[rows.clone()]);
    let p = point.clamp(rows.start, rows.end.saturating_sub(1).max(rows.start));
    let line = text[rows.start..p].matches('\n').count();
    let bol = text[..p].rfind('\n').map_or(0, |i| i + 1);
    let col = text[bol..p].matches('|').count().max(1);
    Ok(Context {
        text,
        rows,
        formulas,
        table,
        line,
        col,
    })
}

/// The text range of field `col` of the `line`th line of `rows`: between
/// its bars.
fn cell_range(
    text: &str,
    rows: &std::ops::Range<usize>,
    line: usize,
    col: usize,
) -> Option<std::ops::Range<usize>> {
    let mut bol = rows.start;
    for _ in 0..line {
        bol += text[bol..rows.end].find('\n')? + 1;
    }
    let eol = text[bol..].find('\n').map_or(text.len(), |i| bol + i);
    let bars: Vec<usize> = text[bol..eol]
        .match_indices('|')
        .map(|(i, _)| bol + i)
        .collect();
    let start = *bars.get(col - 1)? + 1;
    let end = bars.get(col).copied().unwrap_or(eol);
    Some(start..end)
}

/// The formula of the field at point, for a formula bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaInfo {
    /// The left-hand side: `$3`, `@2$4`, a range or a name; for a field no
    /// formula sets, its column (`$3`).
    pub lhs: String,
    /// The formula with its flags; empty when there is none.
    pub rhs: String,
    /// Whether it is a field formula.
    pub field: bool,
    /// Why the field shows `#ERROR`, if it does.
    pub error: Option<String>,
    /// An Emacs Lisp formula, which Kalem does not compute.
    pub lisp: bool,
    /// The field at point.
    pub cell: std::ops::Range<usize>,
    /// The fields the formula refers to.
    pub references: Vec<std::ops::Range<usize>>,
}

/// The formula that sets the field at `point`, if point is in a table row.
pub fn formula_info(doc: &Document, point: usize) -> Option<FormulaInfo> {
    let c = context(doc, point).ok()?;
    if !matches!(c.table.rows.get(c.line), Some(Row::Data(_))) {
        return None;
    }
    let equations = tblfm::parse(&c.formulas).equations;
    let consts = constants(doc);
    let none = |_: &str| None;
    let env = Env {
        remote: &org_table::formula::NoRemote,
        constants: &consts,
        property: &none,
        duration_custom: DurationCustom::default(),
    };
    let applied = recalc::formula_at(&c.table, &equations, &env, c.line, c.col).ok()?;
    let cell = cell_range(&c.text, &c.rows, c.line, c.col)?;
    let (lhs, rhs, field, error) = match applied {
        Some(a) => (a.equation.lhs, a.equation.rhs, a.field, a.error),
        None => (format!("${}", c.col), String::new(), false, None),
    };
    let formula = rhs.rfind(';').map_or(rhs.as_str(), |i| &rhs[..i]);
    let lisp = formula.starts_with("'(");
    let references = if rhs.is_empty() || lisp {
        Vec::new()
    } else {
        recalc::references(&c.table, &env, c.line, c.col, formula)
            .into_iter()
            .filter_map(|(l, col)| cell_range(&c.text, &c.rows, l, col))
            .collect()
    };
    Some(FormulaInfo {
        lhs,
        rhs,
        field,
        error,
        lisp,
        cell,
        references,
    })
}

/// Sets the formula of the field at `point` (`field`) or of its column,
/// as `org-table-get-formula` stores it (an empty formula removes it),
/// then recalculates the table. `=` or `:=` before the formula is
/// dropped.
pub fn set_formula(
    doc: &Document,
    point: usize,
    formula: &str,
    field: bool,
) -> Result<Recalculated, EditError> {
    let c = context(doc, point)?;
    if !matches!(c.table.rows.get(c.line), Some(Row::Data(_))) {
        return Err(EditError::new("Not in a table field"));
    }
    let analysis = org_table::table::Analysis::of(&c.table);
    let name = analysis
        .named_fields
        .iter()
        .find(|(_, l, col)| *l == c.line && *col == c.col)
        .map(|(n, _, _)| n.clone());
    let key = if field {
        name.clone().unwrap_or_else(|| {
            let d = analysis.line_to_dline(c.line, false).unwrap_or(0);
            format!("@{d}${}", c.col)
        })
    } else {
        format!("${}", c.col)
    };
    let eq = formula.trim_start_matches(' ');
    let eq = eq
        .strip_prefix(":=")
        .or_else(|| eq.strip_prefix('='))
        .unwrap_or(eq);
    let eq = eq.trim_matches(' ').to_string();
    let mut stored = tblfm::parse(&c.formulas).equations;
    if !field && let Some(n) = &name {
        stored.retain(|e| &e.lhs != n);
    }
    if eq.is_empty() {
        stored.retain(|e| e.lhs != key);
    } else if let Some(e) = stored.iter_mut().find(|e| e.lhs == key) {
        e.rhs = eq;
    } else {
        stored.insert(0, tblfm::Equation { lhs: key, rhs: eq });
    }
    let sorted = tblfm::store_order(&stored, analysis.ncol);
    let mut text = c.text.clone();
    match tblfm::active_line(&c.text[c.rows.end..]) {
        Some((at, _)) => {
            // After `#+TBLFM:` to the end of the line.
            let value = c.rows.end + at;
            let colon = c.text[..value].rfind(':').map_or(value, |i| i + 1);
            let eol = c.text[value..]
                .find('\n')
                .map_or(c.text.len(), |i| value + i);
            if sorted.is_empty() {
                let bol = c.text[..colon].rfind('\n').map_or(0, |i| i + 1);
                let next = (eol + 1).min(c.text.len());
                text.replace_range(bol..next, "");
            } else {
                text.replace_range(colon..eol, &format!(" {}", tblfm::format(&sorted)));
            }
        }
        None if !sorted.is_empty() => {
            let first = &c.text[c.rows.start..];
            let indent = &first[..first.len() - first.trim_start_matches([' ', '\t']).len()];
            let mut line = format!("{indent}#+TBLFM: {}\n", tblfm::format(&sorted));
            if !c.text[..c.rows.end].ends_with('\n') {
                line.insert(0, '\n');
            }
            text.insert_str(c.rows.end, &line);
        }
        None => {}
    }
    let new_doc = Document::new(org_syntax::parse_with(&text, doc.parse().context()));
    let at = c.rows.start + (point.clamp(c.rows.start, c.rows.end) - c.rows.start);
    let (final_text, lisp) = match recalculate(&new_doc, at, false) {
        Ok(r) => (r.transaction.apply(&text), r.lisp),
        Err(_) => (text, Vec::new()),
    };
    let mut tx = Transaction::new("Set formula");
    let (a, b) = (c.text.as_bytes(), final_text.as_bytes());
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let max_suf = a.len().min(b.len()) - pre;
    let suf = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suf)
        .take_while(|(x, y)| x == y)
        .count();
    let (mut pre, mut suf) = (pre, suf);
    while !c.text.is_char_boundary(pre) || !final_text.is_char_boundary(pre) {
        pre -= 1;
    }
    while !c.text.is_char_boundary(a.len() - suf) || !final_text.is_char_boundary(b.len() - suf) {
        suf -= 1;
    }
    if pre + suf < a.len() || pre + suf < b.len() {
        tx.replace(pre..a.len() - suf, &final_text[pre..b.len() - suf])
            .expect("one edit");
    }
    Ok(Recalculated {
        transaction: tx.select(Selection::caret(point.min(final_text.len()))),
        lisp,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &str, point: usize) -> String {
        let doc = Document::new(org_syntax::parse(t));
        recalculate(&doc, point, false)
            .unwrap()
            .transaction
            .apply(t)
    }

    #[test]
    fn a_later_tblfm_line_applies_its_formulas() {
        let t = "| 1 |   |\n| 2 |   |\n#+TBLFM: $2=$1+1\n#+TBLFM: $2=$1*10\n";
        let tail = "#+TBLFM: $2=$1+1\n#+TBLFM: $2=$1*10\n";
        assert_eq!(
            run(t, t.rfind("#+TBLFM").unwrap() + 2),
            format!("| 1 | 10 |\n| 2 | 20 |\n{tail}")
        );
        assert_eq!(run(t, 2), format!("| 1 | 2 |\n| 2 | 3 |\n{tail}"));
    }

    #[test]
    fn iteration_without_convergence() {
        // Emacs leaves the table at its tenth pass, then gives the error.
        let t = "| 1 |\n| 0 |\n#+TBLFM: @1$1=@1$1+1\n";
        let r = recalculate(&Document::new(org_syntax::parse(t)), 2, true).unwrap();
        assert_eq!(
            r.error.as_deref(),
            Some("No convergence after 10 iterations")
        );
        assert_eq!(
            r.transaction.apply(t),
            "| 11 |\n|  0 |\n#+TBLFM: @1$1=@1$1+1\n"
        );
    }

    #[test]
    fn recalculates_and_aligns() {
        let t = "* Budget\n| Item | Qty | Price | Total |\n|---+---+---+---|\n| Pens | 3 | 1.20 | |\n| Ink | 2 | 12.99 | |\n#+TBLFM: $4=$2*$3\n";
        let out = run(t, 12);
        assert_eq!(
            out,
            "* Budget\n| Item | Qty | Price | Total |\n|------+-----+-------+-------|\n| Pens |   3 |  1.20 |   3.6 |\n| Ink  |   2 | 12.99 | 25.98 |\n#+TBLFM: $4=$2*$3\n"
        );
        // From the formula line too.
        assert_eq!(run(t, t.find("#+TBLFM").unwrap() + 3), out);
        // Formulas see #+CONSTANTS, other tables and properties.
        let t = "#+CONSTANTS: g=9.81\n#+NAME: rates\n| r |\n|---|\n| 2 |\n\n* A\n:PROPERTIES:\n:k: 10\n:END:\n| x | y |\n|---+---|\n| 3 |   |\n#+TBLFM: $2=$1*$g+remote(rates,@2$1)+$PROP_k\n";
        let out = run(t, t.find("| x").unwrap());
        assert!(out.contains("| 3 | 41.43 |"), "{out}");
        // A Lisp formula is reported and left alone.
        let t = "| 1 | x |\n#+TBLFM: $2='(+ 1 2)\n";
        let doc = Document::new(org_syntax::parse(t));
        let r = recalculate(&doc, 0, false).unwrap();
        assert_eq!(r.lisp, vec!["$2".to_string()]);
        assert_eq!(r.transaction.apply(t), t);
        assert!(recalculate(&Document::new(org_syntax::parse("text\n")), 0, false).is_err());
    }

    #[test]
    fn formulas_at_point() {
        let t = "| a | b | c |\n|---+---+---|\n| 1 | 2 | #ERROR |\n#+TBLFM: $3=$1+$2+\n";
        let doc = Document::new(org_syntax::parse(t));
        let at = t.find("#ERROR").unwrap() + 1;
        let f = formula_info(&doc, at).unwrap();
        assert_eq!(
            (f.lhs.as_str(), f.rhs.as_str(), f.field),
            ("$3", "$1+$2+", false)
        );
        assert!(
            f.error.as_deref().unwrap().contains("Expected a number"),
            "{f:?}"
        );
        assert_eq!(&t[f.cell.clone()], " #ERROR ");
        let refs: Vec<&str> = f.references.iter().map(|r| &t[r.clone()]).collect();
        assert_eq!(refs, [" 1 ", " 2 "]);
        // Setting the column formula rewrites the line and recalculates.
        let r = set_formula(&doc, at, "=$1*$2", false).unwrap();
        assert_eq!(
            r.transaction.apply(t),
            "| a | b | c |\n|---+---+---|\n| 1 | 2 | 2 |\n#+TBLFM: $3=$1*$2\n"
        );
        // A field formula goes before column formulas; empty removes.
        let r = set_formula(&doc, at, ":=10", true)
            .unwrap()
            .transaction
            .apply(t);
        assert!(r.ends_with("#+TBLFM: $3=$1+$2+::@2$3=10\n"), "{r}");
        let r = set_formula(&doc, at, "", false)
            .unwrap()
            .transaction
            .apply(t);
        assert!(!r.contains("TBLFM"), "{r}");
        // A table without formulas gets a line.
        let t = "  | 1 | 2 |   |\n";
        let doc = Document::new(org_syntax::parse(t));
        let r = set_formula(&doc, 12, "$1+$2", false)
            .unwrap()
            .transaction
            .apply(t);
        assert_eq!(r, "  | 1 | 2 | 3 |\n  #+TBLFM: $3=$1+$2\n");
    }
}
