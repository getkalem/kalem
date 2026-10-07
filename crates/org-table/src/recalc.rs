//! Recalculating a table (`org-table-recalculate` with a prefix: every
//! row). Column formulas run row after row, then field formulas, each in
//! the order of their left-hand sides; a formula sees what the formulas
//! before it wrote, exactly as in Emacs, so that the result is the same.
//! `iterate` repeats until the table no longer changes
//! (`org-table-iterate`).

use std::collections::HashSet;

use crate::formula::{Env, Error, Evaluator, Outcome, first_last};
use crate::table::{Analysis, Row, Table, is_name};
use crate::tblfm::{Equation, recalc_order};

/// What a recalculation did besides changing fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Left-hand sides of Emacs Lisp formulas, which Kalem keeps but does
    /// not evaluate.
    pub lisp: Vec<String>,
    /// Passes made (1 unless iterating).
    pub passes: usize,
    /// "No convergence after N iterations": the table is the last pass's,
    /// as Emacs leaves it before its error.
    pub error: Option<String>,
}

fn err<T>(msg: impl Into<String>) -> Result<T, Error> {
    Err(Error(msg.into()))
}

/// `\`@-?I+`: a left-hand side relative to a rule.
fn hline_relative(lhs: &str) -> bool {
    let rest = lhs.strip_prefix('@').unwrap_or("");
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    rest.starts_with('I')
}

/// `\`\$[0-9]+\'`: a column number.
fn column_number(lhs: &str) -> Option<usize> {
    let d = lhs.strip_prefix('$')?;
    (!d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
        .then(|| d.parse().ok())
        .flatten()
}

/// `\`@[-+0-9]+\$-?[0-9]+\'`: one field.
fn single_field(lhs: &str) -> bool {
    let Some(rest) = lhs.strip_prefix('@') else {
        return false;
    };
    let Some((r, c)) = rest.split_once('$') else {
        return false;
    };
    let c = c.strip_prefix('-').unwrap_or(c);
    !r.is_empty()
        && r.bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'+')
        && !c.is_empty()
        && c.bytes().all(|b| b.is_ascii_digit())
}

/// `\`@[0-9]+\'`: a whole row.
fn whole_row(lhs: &str) -> Option<&str> {
    let d = lhs.strip_prefix('@')?;
    (!d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())).then_some(d)
}

/// The first field of a row, without blanks.
fn first_field(t: &Table, line: usize) -> &str {
    t.field(line, 1).trim_matches(' ')
}

/// What a recalculation will do: the formulas in order, and where
/// column formulas apply.
#[derive(Debug, Clone, Default)]
struct Plan {
    /// Column formulas: column, right-hand side ready to evaluate, the
    /// equation's left-hand side as written.
    columns: Vec<(usize, String, String)>,
    /// Field formulas, one per field: `@R$C` or a name, right-hand side,
    /// left-hand side as written.
    fields: Vec<(String, String, String)>,
    /// The first line column formulas apply to.
    start: usize,
    /// Only rows marked `#` or `*` are computed.
    marked: bool,
    /// Fields set by field formulas, which column formulas leave alone.
    untouchable: HashSet<(usize, usize)>,
}

/// `org-table-recalculate` up to the evaluation: the equations sorted,
/// their names and ranges expanded, the rows chosen.
fn plan(ev: &Evaluator<'_>, equations: &[Equation]) -> Result<Plan, Error> {
    let mut p = Plan::default();
    let eqlist = recalc_order(equations);
    let mut fields: Vec<(String, String, String)> = Vec::new();
    for eq in &eqlist {
        let rhs = ev.substitute_names(&first_last(&eq.rhs, &ev.analysis)?);
        let old = eq.lhs.as_str();
        let lhs = if hline_relative(old) {
            return err("Can't assign to hline relative reference");
        } else if old.starts_with("$<") || old.starts_with("$>") {
            let new = first_last(old, &ev.analysis)?;
            if eqlist.iter().any(|e| e.lhs == new) {
                return err(format!(
                    "\"{old}=\" formula tries to overwrite existing formula for column {new}"
                ));
            }
            new
        } else {
            first_last(old, &ev.analysis)?
        };
        match column_number(&lhs) {
            Some(0) => return err(format!("Invalid column number in {}", eq.lhs)),
            Some(c) => p.columns.push((c, rhs, eq.lhs.clone())),
            None => fields.push((lhs, rhs, eq.lhs.clone())),
        }
    }
    // Left-hand sides that are rows or ranges become one field each.
    for (lhs, rhs, orig) in fields {
        if single_field(&lhs) || is_name(&lhs) || column_number(&lhs).is_some() {
            p.fields.push((lhs, rhs, orig));
        } else if let Some(r) = whole_row(&lhs) {
            for c in 1..=ev.analysis.ncol {
                p.fields
                    .push((format!("@{r}${c}"), rhs.clone(), orig.clone()));
            }
        } else {
            for f in ev.lhs_fields(&lhs)? {
                p.fields.push((f, rhs.clone(), orig.clone()));
            }
        }
    }
    // The rows column formulas apply to.
    let rows = ev.table.rows.len();
    p.marked = (0..rows).any(|l| mark(&ev.table, l, &["!", "$", "^", "_", "#", "*"]));
    p.start = if p.marked {
        0
    } else {
        let data = |l: usize| matches!(ev.table.rows[l], Row::Data(_));
        let first = (0..rows).find(|&l| data(l));
        let rule = first.and_then(|f| (f + 1..rows).find(|&l| !data(l)));
        rule.and_then(|r| (r + 1..rows).find(|&l| data(l)))
            .unwrap_or(0)
    };
    let mut seen = std::collections::HashSet::new();
    for (name, _, _) in &p.fields {
        let location = ev.analysis.named_fields.iter().find(|(n, _, _)| n == name);
        let reference = match location {
            Some((_, line, col)) => {
                let d = ev.analysis.line_to_dline(*line, false).unwrap_or(0);
                format!("@{d}${col}")
            }
            None => name.clone(),
        };
        if !seen.insert(reference.clone()) {
            return err(format!(
                "Several field/range formulas try to set {reference}"
            ));
        }
        p.untouchable.insert(ev.goto_field(name)?);
    }
    Ok(p)
}

/// A row whose first field is one of `set`.
fn mark(t: &Table, l: usize, set: &[&str]) -> bool {
    t.fields(l).is_some() && set.contains(&first_field(t, l))
}

/// Whether column formulas compute line `line`.
fn column_row(t: &Table, p: &Plan, line: usize) -> bool {
    line >= p.start
        && t.fields(line).is_some()
        && (!p.marked || mark(t, line, &["#", "*"]))
        && !["_", "^", "!", "$", "/"].contains(&first_field(t, line))
}

/// Recalculates every row of `table` once with `equations` (the table's
/// `#+TBLFM`).
pub fn recalculate(
    table: &Table,
    equations: &[Equation],
    env: &Env<'_>,
) -> Result<(Table, Report), Error> {
    let mut ev = Evaluator {
        analysis: Analysis::of(table),
        table: table.clone(),
        env,
    };
    let mut report = Report {
        passes: 1,
        ..Report::default()
    };
    let p = plan(&ev, equations)?;
    for line in 0..ev.table.rows.len() {
        if !column_row(&ev.table, &p, line) {
            continue;
        }
        for (c, rhs, _) in &p.columns {
            // `org-table-goto-column` with FORCE adds the columns needed.
            if ev.table.fields(line).is_some_and(|f| f.len() < *c) {
                let v = ev.table.field(line, *c).to_string();
                ev.table.set_field(line, *c, v);
            }
            if p.untouchable.contains(&(line, *c)) {
                continue;
            }
            match ev.eval(line, *c, rhs)? {
                Outcome::Value(v) => ev.table.set_field(line, *c, v),
                Outcome::Lisp => {
                    let l = format!("${c}");
                    if !report.lisp.contains(&l) {
                        report.lisp.push(l);
                    }
                }
            }
        }
    }
    // Setting a field never widens the table past its widest row.
    let width = ev
        .table
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Data(f) => Some(f.len()),
            Row::Rule => None,
        })
        .max()
        .unwrap_or(0);
    for (reference, rhs, _) in &p.fields {
        let (line, col) = ev.goto_field(reference)?;
        if col > width {
            return err("Missing columns in the table.  Aborting");
        }
        match ev.eval(line, col, rhs)? {
            Outcome::Value(v) => ev.table.set_field(line, col, v),
            Outcome::Lisp => {
                if !report.lisp.contains(reference) {
                    report.lisp.push(reference.clone());
                }
            }
        }
    }
    Ok((ev.table, report))
}

/// The formula that sets a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The equation as written in the `#+TBLFM` line.
    pub equation: Equation,
    /// Whether it is a field formula (else a column formula).
    pub field: bool,
    /// Why the field shows `#ERROR`, if it does now.
    pub error: Option<String>,
}

/// The formula that sets field `col` of line `line` in a recalculation,
/// if one does: a field formula for it, else its column's formula.
pub fn formula_at(
    table: &Table,
    equations: &[Equation],
    env: &Env<'_>,
    line: usize,
    col: usize,
) -> Result<Option<Applied>, Error> {
    let ev = Evaluator {
        analysis: Analysis::of(table),
        table: table.clone(),
        env,
    };
    let p = plan(&ev, equations)?;
    let find = |lhs: &str| equations.iter().find(|e| e.lhs == lhs).cloned();
    let explain = |rhs: &str| -> Option<String> {
        if table.field(line, col).trim() != "#ERROR" {
            return None;
        }
        ev.eval_explained(line, col, rhs)
            .ok()
            .and_then(|(_, why)| why)
    };
    for (reference, rhs, orig) in &p.fields {
        if ev.goto_field(reference).ok() == Some((line, col)) {
            return Ok(find(orig).map(|equation| Applied {
                equation,
                field: true,
                error: explain(rhs),
            }));
        }
    }
    if column_row(table, &p, line)
        && let Some((_, rhs, orig)) = p.columns.iter().find(|(c, _, _)| *c == col)
    {
        return Ok(find(orig).map(|equation| Applied {
            equation,
            field: false,
            error: explain(rhs),
        }));
    }
    Ok(None)
}

/// The fields a formula for field `col` of line `line` refers to: line
/// and column of each, for highlighting.
pub fn references(
    table: &Table,
    env: &Env<'_>,
    line: usize,
    col: usize,
    formula: &str,
) -> Vec<(usize, usize)> {
    let ev = Evaluator {
        analysis: Analysis::of(table),
        table: table.clone(),
        env,
    };
    let Ok(f) = first_last(formula, &ev.analysis) else {
        return Vec::new();
    };
    ev.references(&ev.substitute_names(&f), line, col)
}

/// Recalculates until the table no longer changes, at most `max` times
/// (`org-table-iterate`, 10 in Emacs); without convergence, the last
/// pass's table with the report's error.
pub fn iterate(
    table: &Table,
    equations: &[Equation],
    env: &Env<'_>,
    max: usize,
) -> Result<(Table, Report), Error> {
    let mut last = table.clone();
    let mut report = Report::default();
    for i in 1..=max {
        let (next, r) = recalculate(&last, equations, env)?;
        report.lisp = r.lisp;
        report.passes = i;
        if next == last {
            return Ok((next, report));
        }
        last = next;
    }
    report.error = Some(format!("No convergence after {max} iterations"));
    Ok((last, report))
}

impl Evaluator<'_> {
    /// `org-table-goto-field`: the line and column of a named field or of
    /// `@R$C`.
    pub fn goto_field(&self, reference: &str) -> Result<(usize, usize), Error> {
        if let Some((_, line, col)) = self
            .analysis
            .named_fields
            .iter()
            .find(|(n, _, _)| n == reference)
        {
            return Ok((*line, *col));
        }
        let parsed = reference
            .strip_prefix('@')
            .and_then(|r| r.split_once('$'))
            .and_then(|(r, c)| {
                let ok = |s: &str| {
                    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
                };
                (ok(r) && ok(c)).then(|| (r.parse::<usize>().ok(), c.parse::<usize>().ok()))
            });
        match parsed {
            Some((Some(r), Some(c))) => match self.analysis.dlines.get(r) {
                Some(&line) if r > 0 => Ok((line, c)),
                _ => err(format!("Invalid row number in {reference}")),
            },
            _ => err(format!("Unknown field: {reference}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::{DurationCustom, NoRemote};
    use crate::tblfm;

    fn env<'a>(p: &'a dyn Fn(&str) -> Option<String>) -> Env<'a> {
        Env {
            remote: &NoRemote,
            constants: &[],
            property: p,
            duration_custom: DurationCustom::default(),
        }
    }

    #[test]
    fn inspection() {
        let t = Table::parse(
            "| a | b | c |\n|---+---+---|\n| 1 | 2 | 3 |\n| 4 | x |   |\n|---+---+---|\n|   |   |   |\n",
        );
        let eqs = tblfm::parse("$3=$1+$2::@>$1=vsum(@I..@II)::@3$3=$1/0*[1]").equations;
        let none = |_: &str| None;
        let env = env(&none);
        let at = |l, c| formula_at(&t, &eqs, &env, l, c).unwrap();
        assert_eq!(at(2, 3).unwrap().equation.lhs, "$3");
        assert!(!at(2, 3).unwrap().field);
        assert_eq!(at(5, 1).unwrap().equation.lhs, "@>$1");
        assert!(at(3, 3).unwrap().field);
        // The header is not computed; rows after a second rule are.
        assert!(at(0, 3).is_none());
        assert_eq!(at(5, 3).unwrap().equation.lhs, "$3");
        assert!(at(2, 1).is_none());
        assert_eq!(references(&t, &env, 2, 3, "$1+$2"), vec![(2, 1), (2, 2)]);
        assert_eq!(
            references(&t, &env, 5, 1, "vsum(@I..@II)"),
            vec![(2, 1), (3, 1)]
        );
        assert_eq!(
            references(&t, &env, 3, 2, "@-1$-1+$1..$3"),
            vec![(2, 1), (3, 1), (3, 2), (3, 3)]
        );
        // Why a field shows #ERROR.
        let (t2, _) = recalculate(&t, &eqs, &env).unwrap();
        let a = formula_at(&t2, &eqs, &env, 3, 3).unwrap().unwrap();
        assert_eq!(t2.field(3, 3), "#ERROR");
        assert!(a.error.is_some(), "{a:?}");
    }

    #[test]
    fn letters_beyond_ascii_and_column_zero() {
        let t = Table::parse("| a | b |\n|---+---|\n| 1 |   |\n");
        let none = |_: &str| None;
        let env = env(&none);
        // Byte offsets inside `ş` and `€` were sliced.
        for f in ["$2=\"ş\"", "$2=$1*2;%.1f €", "@>$2=\"ğ\"+remote(x,@1$1)"] {
            let eqs = tblfm::parse(f).equations;
            let _ = recalculate(&t, &eqs, &env);
            let _ = formula_at(&t, &eqs, &env, 2, 2);
        }
        let eqs = tblfm::parse("$2=$1*2;%.1f €").equations;
        let (t2, _) = recalculate(&t, &eqs, &env).unwrap();
        assert_eq!(t2.field(2, 2), "2.0 €");
        // `$0` is no column to set.
        let eqs = tblfm::parse("$0=1").equations;
        assert!(recalculate(&t, &eqs, &env).is_err());
    }
}
