//! Calc formulas evaluated here and by Emacs (`calc-cases.txt`, made by
//! `tools/calc-cases.py` with Org's Calc modes).
//!
//! The formulas are random and many are far from what tables compute:
//! symbolic statistics, complex numbers, factorials of fractions, sines
//! of twenty-digit numbers. Those differences are known
//! (`docs/known-differences.org`); the count may only go down.

#![allow(clippy::print_stderr)]

use org_table::calc;

/// Differences known in `calc-cases.txt`.
const KNOWN: usize = 311;

#[test]
fn agrees_with_emacs() {
    let cases = include_str!("calc-cases.txt");
    let mut total = 0;
    let mut wrong = Vec::new();
    for line in cases.lines() {
        let Some((formula, expected)) = line.split_once('\t') else {
            continue;
        };
        total += 1;
        let got = match calc::eval(formula, &calc::Modes::default()) {
            Ok(s) => s,
            Err(_) => "ERROR".to_string(),
        };
        if got != expected {
            wrong.push(format!("{formula}\n    emacs {expected}\n    kalem {got}"));
        }
    }
    let limit: usize = std::env::var("CALC_SHOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(25);
    for w in wrong.iter().take(limit) {
        eprintln!("{w}");
    }
    eprintln!("{} of {total} differ", wrong.len());
    assert!(
        wrong.len() <= KNOWN,
        "{} of {total} differ, more than the {KNOWN} known",
        wrong.len()
    );
}
