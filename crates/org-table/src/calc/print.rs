//! Calc's display of results in its normal language
//! (`math-compose-expr` in `calccomp.el`, flat): operators by precedence,
//! products written as `2 a`, vectors as `[1, 2]`.

use super::expr::Expr;
use super::num::{self, Display, Num, Prec};

/// How an operator is written, from Calc's table: its text, function and
/// precedences.
struct Op {
    text: &'static str,
    left: i32,
    right: i32,
    /// The precedence for parentheses when it differs (`mod`).
    paren: Option<i32>,
}

/// `math-assq2`: the first operator of the table computing `func`.
fn op_for(func: &str) -> Option<Op> {
    let (text, left, right, paren) = match func {
        "*" => ("*", 196, 195, None),
        "subscr" => ("_", 1200, 1201, None),
        "percent" => ("%", 1100, -1, None),
        "lnot" => ("u!", -1, 1000, None),
        "mod" | "mod-form" => ("mod", 400, 400, Some(185)),
        "sdev" => ("+/-", 300, 300, Some(185)),
        "dfact" => ("!!", 210, -1, None),
        "fact" => ("!", 210, -1, None),
        "^" => ("^", 201, 200, None),
        "ident" => ("u+", -1, 197, None),
        "neg" => ("u-", -1, 197, None),
        "/" => ("/", 190, 191, None),
        "%" => ("%", 190, 191, None),
        "idiv" => ("\\", 190, 191, None),
        "+" => ("+", 180, 181, None),
        "-" => ("-", 180, 181, None),
        "|" => ("|", 170, 171, None),
        "lt" => ("<", 160, 161, None),
        "gt" => (">", 160, 161, None),
        "leq" => ("<=", 160, 161, None),
        "geq" => (">=", 160, 161, None),
        "eq" => ("=", 160, 161, None),
        "neq" => ("!=", 160, 161, None),
        "land" => ("&&", 110, 111, None),
        "lor" => ("||", 100, 101, None),
        "if" => ("?", 91, 90, None),
        "pnot" => ("!!!", -1, 85, None),
        "pand" => ("&&&", 80, 81, None),
        "por" => ("|||", 75, 76, None),
        "assign" => (":=", 51, 50, None),
        "condition" => ("::", 45, 46, None),
        "evalto" => ("=>", 40, 41, None),
        _ => return None,
    };
    Some(Op {
        text,
        left,
        right,
        paren,
    })
}

struct Printer<'a> {
    display: &'a Display,
    prec: &'a Prec,
}

/// Formats `e` as Calc shows it.
pub fn format(e: &Expr, display: &Display, prec: &Prec) -> String {
    Printer { display, prec }.compose(e, 0, false)
}

fn round_bracket(s: String) -> String {
    format!("({s})")
}

/// The first term of a product (`math-prod-first-term`).
fn first_term(e: &Expr) -> &Expr {
    match e {
        Expr::Call(f, a) if f == "*" && a.len() == 2 => first_term(&a[0]),
        _ => e,
    }
}

/// The last term of a product (`math-prod-last-term`).
fn last_term(e: &Expr) -> &Expr {
    match e {
        Expr::Call(f, a) if f == "*" && a.len() == 2 => last_term(&a[1]),
        _ => e,
    }
}

fn is_integer_valued(e: &Expr) -> bool {
    match e {
        Expr::Num(Num::Int(_)) => true,
        Expr::Num(n) => n.is_messy_integer(),
        _ => false,
    }
}

impl Printer<'_> {
    fn number(&self, n: &Num) -> String {
        num::format(n, *self.display, self.prec)
    }

    fn compose(&self, e: &Expr, prec: i32, div: bool) -> String {
        match e {
            Expr::Num(n) => self.number(n),
            Expr::Date(n) => super::date::format(n, self.prec),
            Expr::Var(v) => v.clone(),
            Expr::Str(s) => {
                let codes: Vec<String> = s.chars().map(|c| (c as u32).to_string()).collect();
                format!("[{}]", codes.join(", "))
            }
            Expr::Vec(v) => {
                let items: Vec<String> = v.iter().map(|x| self.compose(x, 0, false)).collect();
                format!("[{}]", items.join(", "))
            }
            Expr::Intv(m, a, b) => format!(
                "{}{} .. {}{}",
                if m & 2 != 0 { "[" } else { "(" },
                self.compose(a, 0, false),
                self.compose(b, 0, false),
                if m & 1 != 0 { "]" } else { ")" }
            ),
            Expr::Call(f, args) => self.call(f, args, prec, div),
        }
    }

    fn call(&self, f: &str, args: &[Expr], prec: i32, div: bool) -> String {
        if f == "cplx" && args.len() == 2 {
            return round_bracket(format!(
                "{}, {}",
                self.compose(&args[0], 0, false),
                self.compose(&args[1], 0, false)
            ));
        }
        let op = op_for(f);
        match op {
            Some(o) if (args.len() == 2 || (f == "if" && args.len() == 3)) && o.right != -1 => {
                if prec > o.paren.unwrap_or(o.left.min(o.right)) || (div && f == "*") {
                    return round_bracket(self.compose(
                        &Expr::Call(f.into(), args.to_vec()),
                        0,
                        false,
                    ));
                }
                if f == "if" {
                    return format!(
                        "{} ? {} : {}",
                        self.compose(&args[0], o.left, false),
                        self.compose(&args[1], 0, false),
                        self.compose(&args[2], o.right, false)
                    );
                }
                let mut lhs = self.compose(&args[0], o.left, false);
                let rhs = self.compose(&args[1], o.right, f == "/");
                if o.text == "^" && lhs.starts_with('-') {
                    lhs = round_bracket(lhs);
                }
                if f == "*" {
                    let nextc = rhs.chars().next();
                    let juxtapose = nextc.is_some_and(|c| {
                        c.is_ascii_alphanumeric()
                            || ('α'..='ω').contains(&c)
                            || ('Α'..='Ω').contains(&c)
                            || matches!(c, '.' | '_' | '#' | '(' | '[' | '{')
                    }) && !(matches!(last_term(&args[0]), Expr::Var(_))
                        && nextc == Some('('));
                    let _ = first_term(&args[1]);
                    if juxtapose && !lhs.is_empty() {
                        return format!("{lhs} {rhs}");
                    }
                    return format!("{lhs}*{rhs}");
                }
                let tight = o.text == "^"
                    || o.text == "_"
                    || (o.text == "/"
                        && is_integer_valued(&args[0])
                        && matches!(args[1], Expr::Num(Num::Int(_))));
                if tight {
                    format!("{lhs}{}{rhs}", o.text)
                } else {
                    format!("{lhs} {} {rhs}", o.text)
                }
            }
            Some(o) if args.len() == 1 && o.right == -1 => {
                // Postfix: `50%`, `5!`.
                if prec > o.paren.unwrap_or(o.left) {
                    return round_bracket(self.compose(
                        &Expr::Call(f.into(), args.to_vec()),
                        0,
                        false,
                    ));
                }
                let lhs = self.compose(&args[0], o.left, false);
                if o.text.len() > 1 {
                    format!("{lhs} {}", o.text)
                } else {
                    format!("{lhs}{}", o.text)
                }
            }
            Some(o) if args.len() == 1 && o.left == -1 => {
                // Prefix: `-a`, `!a`.
                if prec > o.paren.unwrap_or(o.right) {
                    return round_bracket(self.compose(
                        &Expr::Call(f.into(), args.to_vec()),
                        0,
                        false,
                    ));
                }
                let rhs = self.compose(&args[0], o.right, false);
                let text = o
                    .text
                    .strip_prefix('u')
                    .filter(|s| s.len() == 1 && !s.chars().all(char::is_alphabetic))
                    .unwrap_or(o.text);
                if text.len() > 1 {
                    format!("{text} {rhs}")
                } else {
                    format!("{text}{rhs}")
                }
            }
            _ => {
                let items: Vec<String> = args.iter().map(|x| self.compose(x, 0, false)).collect();
                format!("{f}({})", items.join(", "))
            }
        }
    }
}
