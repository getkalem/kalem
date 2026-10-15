//! The part of Emacs Calc that table formulas use: its algebraic
//! notation, its numbers and functions, and its display of results.

mod algebra;
mod cplx;
pub mod date;
pub mod eval;
pub mod expr;
mod funcs;
mod gamma;
pub mod num;
pub mod parse;
pub mod print;
mod signs;

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
        e => algebra::mod_form(e).is_some() || cplx::parts(e).is_some(),
    }
}

/// [`eval`], and whether the result is a single constant (what
/// `calc-eval` with `'num` requires).
pub fn eval_checked(formula: &str, modes: &Modes) -> Result<(String, bool), Error> {
    // A formula nested past what Calc takes (a field of an iterated table
    // whose sum grew at each pass): refused before it is parsed, as Emacs
    // refuses it (Lisp's `max-lisp-eval-depth`).
    if depth_bound(formula) > 2 * eval::MAX_TERMS {
        return Err(Error {
            pos: 0,
            message: "The formula nests too deeply for Calc",
        });
    }
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

/// A bound on how deep `formula` parses: an element of a list or of a
/// vector (commas part them) as deep as its tokens, each operator and
/// operand a level at most, and its deepest bracket. A long sum is as deep
/// as it is long; a vector of many numbers is not deep.
fn depth_bound(formula: &str) -> usize {
    // For each bracket open: its element's tokens so far, the deepest
    // bracket closed in it, and its deepest element before.
    let mut open = vec![(0usize, 0usize, 0usize)];
    let close = |open: &mut Vec<(usize, usize, usize)>| {
        let (tokens, inner, best) = open.pop().expect("a bracket");
        let outer = open.last_mut().expect("the formula");
        outer.1 = outer.1.max(best.max(tokens + inner) + 1);
    };
    let mut in_word = false;
    for b in formula.bytes() {
        let word = b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_') || !b.is_ascii();
        let top = open.len() - 1;
        match b {
            b'(' | b'[' | b'{' => open.push((0, 0, 0)),
            b')' | b']' | b'}' if top > 0 => close(&mut open),
            b',' => {
                let (tokens, inner, best) = open[top];
                open[top] = (0, 0, best.max(tokens + inner));
            }
            _ if word => open[top].0 += usize::from(!in_word),
            _ if b.is_ascii_whitespace() => {}
            _ => open[top].0 += 1,
        }
        in_word = word;
    }
    // Brackets left open nest as closed ones do.
    while open.len() > 1 {
        close(&mut open);
    }
    let (tokens, inner, best) = open[0];
    best.max(tokens + inner)
}
