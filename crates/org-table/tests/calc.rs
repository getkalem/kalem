//! Calc formulas evaluated here and by Emacs (`calc-cases.txt`, made by
//! `tools/calc-cases.py` with Org's Calc modes).
//!
//! The formulas are random and many are far from what tables compute:
//! symbolic statistics, complex numbers, factorials of fractions, sines
//! of twenty-digit numbers. Those differences are known
//! (`book/part-2/org-known-differences.org`); the count may only go down.

#![allow(clippy::print_stderr)]

use org_table::calc;

/// Differences known in `calc-cases.txt`.
const KNOWN: usize = 192;

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

/// Calc functions tables use (`calc-functions.txt`, from
/// `tests/emacs/calc.el` with Org's Calc modes): every one as Emacs gives
/// it.
#[test]
fn functions_agree_with_emacs() {
    let mut wrong = Vec::new();
    for line in include_str!("calc-functions.txt").lines() {
        let Some((formula, expected)) = line.split_once('\t') else {
            continue;
        };
        let got = calc::eval(formula, &calc::Modes::default()).unwrap_or_else(|_| "ERROR".into());
        if got != expected {
            wrong.push(format!("{formula}: emacs {expected}, kalem {got}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `random`: never the same twice, always in its range.
#[test]
fn random_stays_in_range() {
    let m = calc::Modes::default();
    let ev = |f: &str| calc::eval(f, &m).unwrap();
    for _ in 0..200 {
        let k: i64 = ev("random(10)").parse().unwrap();
        assert!((0..10).contains(&k));
        let k: i64 = ev("random(-5)").parse().unwrap();
        assert!((-4..=0).contains(&k), "{k}");
        let k: i64 = ev("random([3 .. 5])").parse().unwrap();
        assert!((3..=5).contains(&k));
        let k: i64 = ev("random((3 .. 5))").parse().unwrap();
        assert_eq!(k, 4);
        let x: f64 = ev("random(2.5)").parse().unwrap();
        assert!((0.0..2.5).contains(&x));
        let x: f64 = ev("random([1. .. 2.))").parse().unwrap();
        assert!((1.0..2.0).contains(&x));
        assert!(["a", "b", "c"].contains(&ev("random([a, b, c])").as_str()));
        let g: f64 = ev("random(0)").parse().unwrap();
        assert!(g.abs() < 10.);
    }
    let draws: std::collections::HashSet<String> = (0..50).map(|_| ev("random(1000000)")).collect();
    assert!(draws.len() > 40);
}

/// A vector and a multiple of it after a term: Calc takes them for like
/// terms and the sum's last term, put back with them uncombined, came back
/// for ever, out of stack. Emacs spreads the modulo form over the vector
/// first (`x + [4 mod 5, 3 mod 5]`); here the two are combined as like
/// terms.
#[test]
fn like_terms_that_do_not_add_up() {
    let m = calc::Modes::default();
    let ev = |f: &str| calc::eval(f, &m).unwrap();
    assert_eq!(ev("x + [1, 2] + (3 mod 5) [1, 2]"), "x + (4 mod 5) [1, 2]");
    assert_eq!(ev("a + [1, 2] - (2 mod 5) [1, 2]"), "a + (4 mod 5) [1, 2]");
    assert_eq!(
        ev("-a + [a, 1] + (2 mod 5) * ([a, 1]) + 2"),
        "(3 mod 5) [a, 1] - a + 2"
    );
}

/// `gcd` with a zero is the other's absolute value, whatever it is, as in
/// Emacs; integers otherwise.
#[test]
fn gcd_with_a_zero() {
    let m = calc::Modes::default();
    let ev = |f: &str| calc::eval(f, &m).unwrap();
    assert_eq!(ev("gcd(857.17, 0)"), "857.17");
    assert_eq!(ev("gcd(0, a)"), "abs(a)");
    assert_eq!(ev("gcd(1:2, 0)"), "1:2");
    assert_eq!(ev("gcd(-6, 0)"), "6");
    assert_eq!(ev("gcd(1.5, 3)"), "gcd(1.5, 3)");
}

/// An empty vector by a formula stays a formula, as in Emacs: it took
/// anything for spread over its no elements.
#[test]
fn empty_vectors_by_formulas() {
    let m = calc::Modes::default();
    let ev = |f: &str| calc::eval(f, &m).unwrap();
    assert_eq!(ev("(8) + a*[]"), "8 + a []");
    assert_eq!(ev("892.48 / Price + []"), "892.48 / Price + []");
    assert_eq!(ev("[] + 5"), "[]");
    assert_eq!(ev("nan + []"), "[]");
}

/// A term that looks negative before one that does not: the second less
/// the first's negation, as Emacs writes them.
#[test]
fn negative_looking_terms_go_after() {
    let m = calc::Modes::default();
    let ev = |f: &str| calc::eval(f, &m).unwrap();
    assert_eq!(ev("-2 a + 3"), "3 - 2 a");
    assert_eq!(ev("a / -2 + 3.5"), "3.5 - a / 2");
    assert_eq!(ev("-3 + a"), "a - 3");
    assert_eq!(ev("-1:2 + a"), "a - 1:2");
    assert_eq!(ev("-a b + c"), "c - a b");
    assert_eq!(ev("-a - 3 + b"), "-a - 3 + b");
}
