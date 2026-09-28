//! Calc's formulas: numbers, variables, vectors and calls, the operators
//! being calls too (`(+ a b)`), as Calc itself represents them.

use super::num::{self, Num, Prec};

/// A Calc formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A number.
    Num(Num),
    /// A date: its day number, 1 for January 1 of year 1, with the time
    /// as a fraction.
    Date(Num),
    /// A variable: a name Calc does not know, or `nan`, `inf`, `pi`…
    Var(String),
    /// A string (`"abc"`).
    Str(String),
    /// A vector.
    Vec(Vec<Expr>),
    /// An interval; bit 1 closes the low end, bit 0 the high end.
    Intv(u8, Box<Expr>, Box<Expr>),
    /// An operator (`+`, `neg`, `^`…) or a function (`vsum`), with its
    /// arguments.
    Call(String, Vec<Expr>),
}

impl Expr {
    /// A call of `f`.
    pub fn call(f: &str, args: Vec<Expr>) -> Expr {
        Expr::Call(f.to_string(), args)
    }

    /// An integer.
    pub fn int(n: i64) -> Expr {
        Expr::Num(Num::int(n))
    }

    /// The formula in Lisp notation, for tests: `(+ 1 (var x))`.
    pub fn to_lisp(&self) -> String {
        match self {
            Expr::Num(n) => num::format(n, num::Display::Float(0), &Prec::default()),
            Expr::Date(n) => super::date::format(n, &Prec::default()),
            Expr::Var(v) => format!("(var {v})"),
            Expr::Str(s) => format!("{s:?}"),
            Expr::Vec(v) => {
                let mut s = String::from("(vec");
                for e in v {
                    s.push(' ');
                    s.push_str(&e.to_lisp());
                }
                s.push(')');
                s
            }
            Expr::Intv(m, a, b) => format!("(intv {m} {} {})", a.to_lisp(), b.to_lisp()),
            Expr::Call(f, args) => {
                let mut s = format!("({f}");
                for e in args {
                    s.push(' ');
                    s.push_str(&e.to_lisp());
                }
                s.push(')');
                s
            }
        }
    }
}
