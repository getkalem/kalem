//! The part of Emacs Calc that table formulas use: its algebraic
//! notation, its numbers and functions, and its display of results.

mod algebra;
pub mod date;
pub mod eval;
pub mod expr;
mod funcs;
pub mod num;
pub mod parse;
pub mod print;

use num::{Display, Prec};

/// The Calc modes a formula runs with (`org-calc-default-modes` and the
/// formula's flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    /// Working precision and fractions.
    pub prec: Prec,
    /// How floats are displayed.
    pub display: Display,
    /// Angles in degrees.
    pub degrees: bool,
}

impl Default for Modes {
    fn default() -> Modes {
        Modes {
            prec: Prec::default(),
            display: Display::default(),
            degrees: true,
        }
    }
}

/// A formula Calc cannot read: where and why.
pub type Error = parse::SyntaxError;

/// `calc-eval`: reads `formula`, evaluates it and formats the result, the
/// values of a comma-separated list joined with `, `.
pub fn eval(formula: &str, modes: &Modes) -> Result<String, Error> {
    eval_checked(formula, modes).map(|(s, _)| s)
}

/// `math-constp`: numbers, modulo forms, and vectors and intervals of them.
fn is_constant(e: &expr::Expr) -> bool {
    match e {
        expr::Expr::Num(_) | expr::Expr::Date(_) => true,
        expr::Expr::Vec(v) => v.iter().all(is_constant),
        expr::Expr::Intv(_, a, b) => is_constant(a) && is_constant(b),
        e => algebra::mod_form(e).is_some(),
    }
}

/// [`eval`], and whether the result is a single constant (what
/// `calc-eval` with `'num` requires).
pub fn eval_checked(formula: &str, modes: &Modes) -> Result<(String, bool), Error> {
    let exprs = parse::parse(formula, &modes.prec)?;
    let env = eval::Env {
        prec: modes.prec,
        degrees: modes.degrees,
    };
    algebra::take_failure();
    let values: Vec<expr::Expr> = exprs.iter().map(|e| eval::normalize(e, &env)).collect();
    let out = values
        .iter()
        .map(|v| print::format(v, &modes.display, &modes.prec))
        .collect::<Vec<_>>()
        .join(", ");
    if algebra::take_failure() {
        return Err(Error {
            pos: 0,
            message: "Calc signalled an error",
        });
    }
    let constant = values.len() == 1 && is_constant(&values[0]);
    Ok((out, constant))
}
