//! Arithmetic on Calc formulas: numbers computed, vectors mapped or
//! multiplied, infinities followed, and formulas simplified the little
//! Calc simplifies them by default: a term combines with its neighbour
//! (`a - b + b` is `a`, `x + 1.5 + 2` is `x + 3.5`), numbers go first in
//! products (`a 3 b` is `3 a b`) and spread over sums (`2 (a + 1)` is
//! `2 a + 2`).

use std::cell::Cell;
use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::One;

use super::eval::Env;
use super::expr::Expr;
use super::num::{self, Num};

thread_local! {
    static FAILED: Cell<bool> = const { Cell::new(false) };
}

/// Marks the evaluation as failed: Emacs signals an error there (a
/// number divided by a vector), which Org shows as `#ERROR`.
pub(crate) fn fail() {
    FAILED.with(|f| f.set(true));
}

/// Whether the evaluation failed since the last call.
pub(crate) fn take_failure() -> bool {
    FAILED.with(|f| f.replace(false))
}

pub(crate) fn var(name: &str) -> Expr {
    Expr::Var(name.to_string())
}

pub(crate) fn inf() -> Expr {
    var("inf")
}

pub(crate) fn neg_inf() -> Expr {
    Expr::call("neg", vec![inf()])
}

pub(crate) fn nan() -> Expr {
    var("nan")
}

/// `math-infinitep`: the direction of `inf` (1), `-inf` (-1), `uinf` and
/// `nan` (0), with the name.
pub(crate) fn infinity(e: &Expr) -> Option<(i8, &'static str)> {
    match e {
        Expr::Var(v) if v == "inf" => Some((1, "inf")),
        Expr::Var(v) if v == "uinf" => Some((0, "uinf")),
        Expr::Var(v) if v == "nan" => Some((0, "nan")),
        Expr::Call(f, a) if f == "neg" && a.len() == 1 && a[0] == inf() => Some((-1, "inf")),
        _ => None,
    }
}

/// `math-infinitep` through products, quotients and negations: whether
/// `e` is infinite, like `x inf`.
pub(crate) fn infinitep(e: &Expr) -> bool {
    let mut a = e;
    loop {
        match a {
            Expr::Call(f, xs) if (f == "neg" || f == "/") && !xs.is_empty() => a = &xs[0],
            Expr::Call(f, xs) if f == "*" && xs.len() == 2 => {
                a = if infinitep(&xs[0]) { &xs[0] } else { &xs[1] };
            }
            _ => return infinity(a).is_some(),
        }
    }
}

pub(crate) fn is_nan(e: &Expr) -> bool {
    matches!(e, Expr::Var(v) if v == "nan")
}

pub(crate) fn num_of(e: &Expr) -> Option<&Num> {
    match e {
        Expr::Num(n) => Some(n),
        _ => None,
    }
}

pub(crate) fn is_zero(e: &Expr) -> bool {
    num_of(e).is_some_and(Num::is_zero)
}

/// Numerically one (`1` or `1.`).
fn is_one(e: &Expr) -> bool {
    match e {
        Expr::Num(Num::Int(n)) => n.is_one(),
        Expr::Num(Num::Float(m, 0)) => m.is_one(),
        _ => false,
    }
}

fn is_minus_one(e: &Expr) -> bool {
    match e {
        Expr::Num(Num::Int(n)) => *n == BigInt::from(-1),
        Expr::Num(Num::Float(m, 0)) => *m == BigInt::from(-1),
        _ => false,
    }
}

/// A number or a vector of them (`Math-objvecp`, roughly).
pub(crate) fn is_constant(e: &Expr) -> bool {
    match e {
        Expr::Num(_) | Expr::Date(_) => true,
        Expr::Vec(v) => v.iter().all(is_constant),
        e => mod_form(e).is_some(),
    }
}

pub(crate) fn is_vec(e: &Expr) -> bool {
    matches!(e, Expr::Vec(_))
}

/// `math-looks-negp`.
pub(crate) fn looks_neg(e: &Expr) -> bool {
    match e {
        Expr::Num(n) => n.is_negative(),
        Expr::Call(f, _) if f == "neg" => true,
        Expr::Call(f, a) if (f == "*" || f == "/") && a.len() == 2 => {
            looks_neg(&a[0]) || looks_neg(&a[1])
        }
        Expr::Call(f, a) if f == "-" && a.len() == 2 => looks_neg(&a[0]) && is_zero(&a[1]),
        _ => false,
    }
}

/// The result of `math-compare`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cmp {
    Less,
    Equal,
    Greater,
    /// Not comparable (formulas, `nan`).
    Unknown,
}

/// `math-compare`.
pub(crate) fn compare(a: &Expr, b: &Expr, env: &Env) -> Cmp {
    if a == b {
        return if infinity(a).is_some() {
            Cmp::Unknown
        } else {
            Cmp::Equal
        };
    }
    if let (Expr::Num(x), Expr::Num(y)) | (Expr::Date(x), Expr::Date(y)) = (a, b) {
        return match num::cmp(x, y, &env.prec) {
            Ordering::Less => Cmp::Less,
            Ordering::Equal => Cmp::Equal,
            Ordering::Greater => Cmp::Greater,
        };
    }
    let dir = |e: &Expr| infinity(e).map(|(d, k)| if k == "inf" { d } else { 0 });
    match (dir(a), dir(b)) {
        (Some(0), _) | (_, Some(0)) => Cmp::Unknown,
        (Some(da), Some(db)) => match (da, db) {
            (1, -1) => Cmp::Greater,
            (-1, 1) => Cmp::Less,
            _ => Cmp::Unknown,
        },
        (Some(da), None) => {
            if da == 1 {
                Cmp::Greater
            } else {
                Cmp::Less
            }
        }
        (None, Some(db)) => {
            if db == 1 {
                Cmp::Less
            } else {
                Cmp::Greater
            }
        }
        _ => {
            if let (Expr::Vec(x), Expr::Vec(y)) = (a, b)
                && x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|(p, q)| compare(p, q, env) == Cmp::Equal)
            {
                return Cmp::Equal;
            }
            Cmp::Unknown
        }
    }
}

/// A term as coefficient and base: `3 a` is (3, a), `-a` is (-1, a), a
/// number `k` is (k, 1).
fn term(e: &Expr) -> (Num, Expr) {
    match e {
        Expr::Num(n) => (n.clone(), Expr::int(1)),
        Expr::Call(f, xs) if f == "*" && xs.len() == 2 => match num_of(&xs[0]) {
            Some(c) => (c.clone(), xs[1].clone()),
            None => (Num::int(1), e.clone()),
        },
        Expr::Call(f, xs) if f == "neg" => {
            let (c, x) = term(&xs[0]);
            (c.neg(), x)
        }
        _ => (Num::int(1), e.clone()),
    }
}

/// `k x`, simplified.
fn scaled(k: Num, x: Expr, env: &Env) -> Expr {
    // A coefficient that is a modulo form takes the number in.
    if let Expr::Call(f, xs) = &x
        && f == "*"
        && xs.len() == 2
        && mod_form(&xs[0]).is_some()
    {
        return mul(&mul(&Expr::Num(k), &xs[0], env), &xs[1], env);
    }
    if x == Expr::int(1) {
        return Expr::Num(k);
    }
    if k.is_zero() {
        return Expr::Num(k);
    }
    if k == Num::int(1) {
        return x;
    }
    if k == Num::int(-1) {
        return neg(&x, env);
    }
    Expr::call("*", vec![Expr::Num(k), x])
}

/// `a + b` for like terms: `(k + m) x`.
fn combine(a: &Expr, b: &Expr, sign: i8, env: &Env) -> Option<Expr> {
    // Terms whose coefficients are modulo forms or numbers, one of them a
    // modulo form: `(5 mod 7) y + 4 y` is `(2 mod 7) y`.
    let modterm = |e: &Expr| match e {
        Expr::Call(f, xs) if f == "*" && xs.len() == 2 && mod_form(&xs[0]).is_some() => {
            Some((xs[0].clone(), xs[1].clone()))
        }
        _ => None,
    };
    if let Some(((ka, xa), (kb, xb))) = match (modterm(a), modterm(b)) {
        (Some(p), Some(q)) => Some((p, q)),
        (Some(p), None) => {
            let (k, x) = term(b);
            Some((p, (Expr::Num(k), x)))
        }
        (None, Some(q)) => {
            let (k, x) = term(a);
            Some(((Expr::Num(k), x), q))
        }
        _ => None,
    } {
        if xa != xb {
            return None;
        }
        let k = if sign > 0 {
            add(&ka, &kb, env)
        } else {
            sub(&ka, &kb, env)
        };
        return Some(mul(&k, &xa, env));
    }
    let (ka, xa) = term(a);
    let (kb, xb) = term(b);
    if xa != xb {
        return None;
    }
    let k = if sign > 0 {
        num::add(&ka, &kb, &env.prec)
    } else {
        num::sub(&ka, &kb, &env.prec)
    };
    Some(scaled(k, xa, env))
}

/// A modulo form `a mod m` (Calc's `(mod a m)`): the parser cannot
/// produce the name, so a form is never evaluated again as `%`.
pub(crate) const MOD_FORM: &str = "mod-form";

/// The value and modulus of a modulo form.
pub(crate) fn mod_form(e: &Expr) -> Option<(&Num, &Num)> {
    match e {
        Expr::Call(f, xs) if f == MOD_FORM && xs.len() == 2 => match (&xs[0], &xs[1]) {
            (Expr::Num(a), Expr::Num(m)) => Some((a, m)),
            _ => None,
        },
        _ => None,
    }
}

/// `math-make-mod`: `a` reduced into `[0, m)`.
pub(crate) fn make_mod(a: &Num, m: &Num, env: &Env) -> Option<Expr> {
    if m.is_zero() || m.is_negative() {
        return None;
    }
    let r = num::modulo(a, m, &env.prec).ok()?;
    Some(Expr::call(
        MOD_FORM,
        vec![Expr::Num(r), Expr::Num(m.clone())],
    ))
}

/// The inverse of `a` modulo the integer `m`, when there is one.
fn mod_inverse(a: &BigInt, m: &BigInt) -> Option<BigInt> {
    let e = a.extended_gcd(m);
    e.gcd.is_one().then(|| e.x.mod_floor(m))
}

/// Arithmetic on modulo forms (`math-add`, `math-mul`… on `(mod a m)`):
/// with a number or a form of the same modulus the result is reduced; two
/// moduli differ, or a formula is involved, and the caller keeps it.
pub(crate) fn modular(op: &str, a: &Expr, b: &Expr, env: &Env) -> Option<Expr> {
    let (x, y, m) = match (mod_form(a), mod_form(b), a, b) {
        (Some((x, m)), Some((y, n)), ..) if m == n => (x, y, m),
        (Some((x, m)), None, _, Expr::Num(y)) => (x, y, m),
        (None, Some((y, m)), Expr::Num(x), _) => (x, y, m),
        _ => return None,
    };
    let p = &env.prec;
    let r = match op {
        "+" => num::add(x, y, p),
        "-" => num::sub(x, y, p),
        "*" => num::mul(x, y, p),
        "/" => match (x, y, m) {
            (Num::Int(x), Num::Int(y), Num::Int(mi)) => {
                Num::Int(x * mod_inverse(&y.mod_floor(mi), mi)?)
            }
            _ => num::div(x, y, p).ok()?,
        },
        _ => return None,
    };
    make_mod(&r, m, env)
}

/// `math-add`.
pub(crate) fn add(a: &Expr, b: &Expr, env: &Env) -> Expr {
    if let Some(r) = modular("+", a, b, env) {
        return r;
    }
    if let Some(r) = super::cplx::arith("+", a, b, env) {
        return r;
    }
    match (a, b) {
        (Expr::Date(d), Expr::Num(n)) | (Expr::Num(n), Expr::Date(d)) => {
            return Expr::Date(num::add(d, n, &env.prec));
        }
        // Two dates add up to a plain number.
        (Expr::Date(x), Expr::Date(y)) => return Expr::Num(num::add(x, y, &env.prec)),
        (Expr::Num(x), Expr::Num(y)) => return Expr::Num(num::add(x, y, &env.prec)),
        (Expr::Vec(x), Expr::Vec(y)) => {
            return if x.len() == y.len() {
                Expr::Vec(x.iter().zip(y).map(|(p, q)| add(p, q, env)).collect())
            } else {
                Expr::call("+", vec![a.clone(), b.clone()])
            };
        }
        (Expr::Vec(x), y) if distributes(y) || x.is_empty() => {
            return Expr::Vec(x.iter().map(|p| add(p, y, env)).collect());
        }
        (x, Expr::Vec(y)) if distributes(x) || y.is_empty() => {
            return Expr::Vec(y.iter().map(|q| add(x, q, env)).collect());
        }
        (Expr::Vec(_), _) | (_, Expr::Vec(_)) => {
            return Expr::call("+", vec![a.clone(), b.clone()]);
        }
        _ => {}
    }
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    // An infinite term absorbs finite ones: `528.2 + x inf` is `x inf`.
    match (infinitep(a), infinitep(b)) {
        (true, false) if infinity(a).is_none() => return a.clone(),
        (false, true) if infinity(b).is_none() => return b.clone(),
        _ => {}
    }
    match (infinity(a), infinity(b)) {
        (Some((sa, ka)), Some((sb, kb))) => {
            return if ka == "uinf" || kb == "uinf" || sa != sb {
                nan()
            } else {
                a.clone()
            };
        }
        (Some(_), None) => return a.clone(),
        (None, Some(_)) => return b.clone(),
        _ => {}
    }
    if is_zero(a) {
        return b.clone();
    }
    if is_zero(b) {
        return a.clone();
    }
    if looks_neg(b) {
        let nb = neg(b, env);
        if !looks_neg(&nb) {
            return sub(a, &nb, env);
        }
    }
    if let Expr::Call(f, xs) = a
        && f == "neg"
        && !looks_neg(b)
    {
        return sub(b, &xs[0], env);
    }
    if let Some(r) = combine(a, b, 1, env) {
        return r;
    }
    if let Expr::Call(f, xs) = a
        && xs.len() == 2
    {
        if f == "+" && combine(&xs[1], b, 1, env).is_some() {
            return add(&xs[0], &add(&xs[1], b, env), env);
        }
        if f == "-" && combine(b, &xs[1], -1, env).is_some() {
            return add(&xs[0], &sub(b, &xs[1], env), env);
        }
    }
    // Sums are kept left-nested: `a + (p - q)` is `(a + p) - q`.
    if let Expr::Call(f, xs) = b
        && xs.len() == 2
    {
        if f == "+" {
            return add(&add(a, &xs[0], env), &xs[1], env);
        }
        if f == "-" {
            return sub(&add(a, &xs[0], env), &xs[1], env);
        }
    }
    Expr::call("+", vec![a.clone(), b.clone()])
}

/// Whether a scalar is spread over a vector's elements: numbers and
/// infinities are, formulas are not (`[1] + x` stays).
fn distributes(e: &Expr) -> bool {
    matches!(e, Expr::Num(_)) || infinity(e).is_some() || super::cplx::parts(e).is_some()
}

/// `math-sub`.
pub(crate) fn sub(a: &Expr, b: &Expr, env: &Env) -> Expr {
    if let Some(r) = modular("-", a, b, env) {
        return r;
    }
    if let Some(r) = super::cplx::arith("-", a, b, env) {
        return r;
    }
    // Less a modulo form, or a term with one as its coefficient, is plus
    // its negation (`- 1 mod 3` is `+ 2 mod 3`).
    if mod_form(b).is_some()
        || matches!(b, Expr::Call(f, xs) if f == "*" && xs.len() == 2 && mod_form(&xs[0]).is_some())
    {
        return add(a, &neg(b, env), env);
    }
    match (a, b) {
        (Expr::Date(x), Expr::Date(y)) => return Expr::Num(num::sub(x, y, &env.prec)),
        (Expr::Date(x), Expr::Num(n)) => return Expr::Date(num::sub(x, n, &env.prec)),
        (Expr::Num(x), Expr::Num(y)) => return Expr::Num(num::sub(x, y, &env.prec)),
        (Expr::Vec(_), _) | (_, Expr::Vec(_)) => return add(a, &neg(b, env), env),
        _ => {}
    }
    if is_nan(a) || is_nan(b) || infinity(a).is_some() || infinity(b).is_some() {
        return add(a, &neg(b, env), env);
    }
    if is_zero(b) {
        return a.clone();
    }
    if is_zero(a) {
        return neg(b, env);
    }
    if a == b {
        return Expr::int(0);
    }
    if looks_neg(b) {
        let nb = neg(b, env);
        if !looks_neg(&nb) {
            return add(a, &nb, env);
        }
    }
    if let Some(r) = combine(a, b, -1, env) {
        return r;
    }
    if let Expr::Call(f, xs) = a
        && xs.len() == 2
    {
        if f == "+" && combine(&xs[1], b, -1, env).is_some() {
            return add(&xs[0], &sub(&xs[1], b, env), env);
        }
        if f == "-" && combine(&xs[1], b, 1, env).is_some() {
            return sub(&xs[0], &add(&xs[1], b, env), env);
        }
    }
    // `a - (p + q)` is `(a - p) - q`, `a - (p - q)` is `(a - p) + q`.
    if let Expr::Call(f, xs) = b
        && xs.len() == 2
    {
        if f == "+" {
            return sub(&sub(a, &xs[0], env), &xs[1], env);
        }
        if f == "-" {
            return add(&sub(a, &xs[0], env), &xs[1], env);
        }
    }
    Expr::call("-", vec![a.clone(), b.clone()])
}

/// `math-neg`.
pub(crate) fn neg(a: &Expr, env: &Env) -> Expr {
    if let Some(r) = super::cplx::neg(a) {
        return r;
    }
    if let Some((x, m)) = mod_form(a)
        && let Some(r) = make_mod(&x.neg(), m, env)
    {
        return r;
    }
    match a {
        Expr::Num(n) => Expr::Num(n.neg()),
        Expr::Vec(v) => Expr::Vec(v.iter().map(|x| neg(x, env)).collect()),
        Expr::Var(v) if v == "nan" || v == "uinf" => a.clone(),
        Expr::Call(f, xs) if f == "neg" => xs[0].clone(),
        Expr::Call(f, xs) if f == "*" && xs.len() == 2 && mod_form(&xs[0]).is_some() => {
            mul(&neg(&xs[0], env), &xs[1], env)
        }
        Expr::Call(f, xs) if f == "*" && num_of(&xs[0]).is_some() => {
            let k = num_of(&xs[0]).expect("a number").neg();
            scaled(k, xs[1].clone(), env)
        }
        Expr::Call(f, xs) if f == "+" && xs.len() == 2 => sub(&neg(&xs[0], env), &xs[1], env),
        Expr::Call(f, xs) if f == "-" && xs.len() == 2 => sub(&xs[1], &xs[0], env),
        // The sign goes to a factor that has one.
        Expr::Call(f, xs) if (f == "*" || f == "/") && xs.len() == 2 && looks_neg(&xs[0]) => {
            Expr::call(f, vec![neg(&xs[0], env), xs[1].clone()])
        }
        Expr::Call(f, xs) if (f == "*" || f == "/") && xs.len() == 2 && looks_neg(&xs[1]) => {
            Expr::call(f, vec![xs[0].clone(), neg(&xs[1], env)])
        }
        _ => Expr::call("neg", vec![a.clone()]),
    }
}

/// The base and exponent of a power: `a^3` is (a, 3), `a` is (a, 1).
fn power(e: &Expr) -> (Expr, Expr) {
    match e {
        Expr::Call(f, xs) if f == "^" && xs.len() == 2 => (xs[0].clone(), xs[1].clone()),
        _ => (e.clone(), Expr::int(1)),
    }
}

/// `math-mul`.
pub(crate) fn mul(a: &Expr, b: &Expr, env: &Env) -> Expr {
    if let Some(r) = modular("*", a, b, env) {
        return r;
    }
    if let Some(r) = super::cplx::arith("*", a, b, env) {
        return r;
    }
    // `math-mul-zero`: a zero modulo form times a formula is 0.
    let zero_mod = |e: &Expr| mod_form(e).is_some_and(|(k, _)| k.is_zero());
    let formula = |e: &Expr| {
        !matches!(
            e,
            Expr::Num(_) | Expr::Vec(_) | Expr::Date(_) | Expr::Intv(..)
        ) && mod_form(e).is_none()
            && super::cplx::parts(e).is_none()
            && infinity(e).is_none()
    };
    if (zero_mod(a) && formula(b)) || (zero_mod(b) && formula(a)) {
        return Expr::int(0);
    }
    // A number or modulo form times `(k mod m) x`: one coefficient.
    let modcoef = |e: &Expr| match e {
        Expr::Call(f, xs) if f == "*" && xs.len() == 2 && mod_form(&xs[0]).is_some() => {
            Some((xs[0].clone(), xs[1].clone()))
        }
        _ => None,
    };
    let scalar = |e: &Expr| matches!(e, Expr::Num(_)) || mod_form(e).is_some();
    if scalar(a)
        && let Some((c, x)) = modcoef(b)
    {
        return mul(&mul(a, &c, env), &x, env);
    }
    if scalar(b)
        && let Some((c, x)) = modcoef(a)
    {
        return mul(&mul(b, &c, env), &x, env);
    }
    match (a, b) {
        (Expr::Num(x), Expr::Num(y)) => return Expr::Num(num::mul(x, y, &env.prec)),
        (Expr::Vec(x), Expr::Vec(y)) => {
            // The dot product of two vectors.
            return if x.len() == y.len() && !x.iter().chain(y).any(is_vec) {
                let mut terms = x.iter().zip(y).map(|(p, q)| mul(p, q, env));
                let first = terms.next().unwrap_or(Expr::int(0));
                terms.fold(first, |acc, t| add(&acc, &t, env))
            } else {
                Expr::call("*", vec![a.clone(), b.clone()])
            };
        }
        (Expr::Vec(x), y) if distributes(y) || x.is_empty() => {
            return Expr::Vec(x.iter().map(|p| mul(p, y, env)).collect());
        }
        (x, Expr::Vec(y)) if distributes(x) || y.is_empty() => {
            return Expr::Vec(y.iter().map(|q| mul(x, q, env)).collect());
        }
        (Expr::Vec(_), _) | (_, Expr::Vec(_)) => {
            if is_zero(a) || is_zero(b) {
                return if is_zero(a) { a.clone() } else { b.clone() };
            }
            return Expr::call("*", vec![a.clone(), b.clone()]);
        }
        _ => {}
    }
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    if let Some(r) = mul_infinite(a, b) {
        return r;
    }
    if is_zero(a) {
        return a.clone();
    }
    if is_zero(b) {
        return b.clone();
    }
    if is_one(a) {
        return b.clone();
    }
    if is_one(b) {
        return a.clone();
    }
    if is_minus_one(a) {
        return neg(b, env);
    }
    if is_minus_one(b) {
        return neg(a, env);
    }
    if num_of(b).is_some() && num_of(a).is_none() {
        return mul(b, a, env);
    }
    if infinity(a).is_some() && num_of(b).is_none() {
        return Expr::call("*", vec![b.clone(), a.clone()]);
    }
    if num_of(a).is_none()
        && let Expr::Call(f, xs) = b
        && f == "*"
        && xs.len() == 2
        && num_of(&xs[0]).is_some()
    {
        return mul(&xs[0], &mul(a, &xs[1], env), env);
    }
    // Quotients: `k (x / y)` is `k x / y`.
    if let Expr::Call(f, xs) = b
        && f == "/"
        && xs.len() == 2
    {
        return div(&mul(a, &xs[0], env), &xs[1], env);
    }
    if let Expr::Call(f, xs) = a
        && f == "/"
        && xs.len() == 2
    {
        return div(&mul(&xs[0], b, env), &xs[1], env);
    }
    if let Some(k) = num_of(a) {
        return match b {
            Expr::Call(f, xs) if f == "*" && num_of(&xs[0]).is_some() => {
                let k2 = num_of(&xs[0]).expect("a number");
                scaled(num::mul(k, k2, &env.prec), xs[1].clone(), env)
            }
            Expr::Call(f, xs) if f == "neg" => scaled(k.neg(), xs[0].clone(), env),
            Expr::Call(f, xs) if f == "+" && xs.len() == 2 => {
                add(&mul(a, &xs[0], env), &mul(a, &xs[1], env), env)
            }
            Expr::Call(f, xs) if f == "-" && xs.len() == 2 => {
                sub(&mul(a, &xs[0], env), &mul(a, &xs[1], env), env)
            }
            _ => Expr::call("*", vec![a.clone(), b.clone()]),
        };
    }
    match a {
        Expr::Call(f, xs) if f == "*" && xs.len() == 2 => {
            if num_of(&xs[0]).is_some() {
                return mul(&xs[0], &mul(&xs[1], b, env), env);
            }
            return Expr::call("*", vec![xs[0].clone(), mul(&xs[1], b, env)]);
        }
        Expr::Call(f, xs) if f == "neg" => return neg(&mul(&xs[0], b, env), env),
        _ => {}
    }
    if let Expr::Call(f, xs) = b
        && f == "neg"
    {
        return neg(&mul(a, &xs[0], env), env);
    }
    let (ba, ea) = power(a);
    let (bb, eb) = power(b);
    // Formulas combine into powers; dates do not.
    if ba == bb && infinity(&ba).is_none() && !matches!(ba, Expr::Date(_)) {
        return pow(&ba, &add(&ea, &eb, env), env);
    }
    Expr::call("*", vec![a.clone(), b.clone()])
}

fn mul_infinite(a: &Expr, b: &Expr) -> Option<Expr> {
    let (ia, ib) = (infinity(a), infinity(b));
    if ia.is_none() && ib.is_none() {
        return None;
    }
    let sign = |e: &Expr| -> Option<i8> {
        match e {
            Expr::Num(n) if n.is_zero() => Some(0),
            Expr::Num(n) if n.is_negative() => Some(-1),
            Expr::Num(_) => Some(1),
            _ => infinity(e).map(|(s, _)| s),
        }
    };
    let (sa, sb) = (sign(a)?, sign(b)?);
    if (sa == 0 && ia.is_none()) || (sb == 0 && ib.is_none()) {
        return Some(nan());
    }
    let uinf = ia.is_some_and(|(_, k)| k == "uinf") || ib.is_some_and(|(_, k)| k == "uinf");
    Some(if uinf {
        var("uinf")
    } else if sa * sb > 0 {
        inf()
    } else {
        neg_inf()
    })
}

/// `math-div`.
pub(crate) fn div(a: &Expr, b: &Expr, env: &Env) -> Expr {
    if let Some(r) = modular("/", a, b, env) {
        return r;
    }
    if let Some(r) = super::cplx::arith("/", a, b, env) {
        return r;
    }
    match (a, b) {
        (Expr::Num(x), Expr::Num(y)) => {
            return match num::div(x, y, &env.prec) {
                Ok(r) => Expr::Num(r),
                Err(_) => Expr::call("/", vec![a.clone(), b.clone()]),
            };
        }
        (Expr::Vec(x), y) if !is_vec(y) => {
            return Expr::Vec(x.iter().map(|p| div(p, y, env)).collect());
        }
        (_, Expr::Vec(_)) => {
            // Emacs recurses without end dividing by a vector.
            fail();
            return Expr::call("/", vec![a.clone(), b.clone()]);
        }
        _ => {}
    }
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    if infinity(a).is_some() && matches!(b, Expr::Num(_)) {
        return mul(a, &div(&Expr::int(1), b, env), env);
    }
    if infinity(b).is_some() && infinity(a).is_none() {
        // A float stays a float: `628.07 / inf` is `0.`.
        return match a {
            Expr::Num(n) if n.is_float() => Expr::Num(Num::Float(BigInt::from(0), 0)),
            _ => Expr::int(0),
        };
    }
    if is_one(b) {
        return a.clone();
    }
    if is_zero(a) && !is_zero(b) {
        return a.clone();
    }
    if a == b && infinity(a).is_none() {
        return Expr::int(1);
    }
    if let Some(k) = num_of(b) {
        match a {
            Expr::Call(f, xs) if f == "*" && num_of(&xs[0]).is_some() => {
                let c = num_of(&xs[0]).expect("a number");
                if let Ok(q) = num::div(c, k, &env.prec) {
                    return scaled(q, xs[1].clone(), env);
                }
            }
            // `(x / k1) / k2` is `x / (k1 k2)`.
            Expr::Call(f, xs) if f == "/" && num_of(&xs[1]).is_some_and(|k1| !k1.is_zero()) => {
                let k1 = num_of(&xs[1]).expect("a number");
                return div(&xs[0], &Expr::Num(num::mul(k1, k, &env.prec)), env);
            }
            Expr::Call(f, xs)
                if (f == "+" || f == "-")
                    && (num_of(&xs[0]).is_some() || num_of(&xs[1]).is_some()) =>
            {
                let x = div(&xs[0], b, env);
                let c = div(&xs[1], b, env);
                return if f == "+" {
                    add(&x, &c, env)
                } else {
                    sub(&x, &c, env)
                };
            }
            _ => {}
        }
    }
    // `(x / y) / z` is `x / (y z)`, unless `y` is a zero Calc refused.
    if let Expr::Call(f, xs) = a
        && f == "/"
        && xs.len() == 2
        && !is_zero(&xs[1])
        && !(matches!(xs[0], Expr::Num(_)) && matches!(xs[1], Expr::Num(_)))
    {
        return div(&xs[0], &mul(&xs[1], b, env), env);
    }
    // `x / (y / z)` is `x z / y`.
    if let Expr::Call(f, xs) = b
        && f == "/"
        && xs.len() == 2
        && !is_zero(&xs[1])
    {
        return div(&mul(a, &xs[1], env), &xs[0], env);
    }
    // `(k x) / (m y)` is `(k / m) x / y`.
    if let (Expr::Call(f, xs), Expr::Call(g, ys)) = (a, b)
        && f == "*"
        && g == "*"
        && let (Some(k), Some(m)) = (num_of(&xs[0]), num_of(&ys[0]))
        && let Ok(q) = num::div(k, m, &env.prec)
    {
        return div(&scaled(q, xs[1].clone(), env), &ys[1], env);
    }
    // `k1 / (k2 x)` is `(k1 / k2) / x`.
    if let (Some(k1), Expr::Call(f, xs)) = (num_of(a), b)
        && f == "*"
        && xs.len() == 2
        && let Some(k2) = num_of(&xs[0])
        && let Ok(q) = num::div(k1, k2, &env.prec)
    {
        return div(&Expr::Num(q), &xs[1], env);
    }
    Expr::call("/", vec![a.clone(), b.clone()])
}

/// `math-pow` on formulas; numbers are left to the caller.
pub(crate) fn pow(a: &Expr, b: &Expr, env: &Env) -> Expr {
    if is_nan(b) {
        return b.clone();
    }
    if is_nan(a) {
        return a.clone();
    }
    // A modulo form to a natural power, by squaring.
    if let Some((_, m)) = mod_form(a)
        && let Some(n) = num_of(b).and_then(Num::to_i64)
        && n >= 0
    {
        let mut r = make_mod(&Num::int(1), m, env).expect("a positive modulus");
        let (mut base, mut n) = (a.clone(), n);
        while n > 0 {
            if n & 1 == 1 {
                r = mul(&r, &base, env);
            }
            base = mul(&base, &base, env);
            n >>= 1;
        }
        return r;
    }
    if let Expr::Vec(_) = a
        && let Some(n) = num_of(b).and_then(Num::to_i64)
    {
        if n < 1 {
            fail();
            return Expr::call("^", vec![a.clone(), b.clone()]);
        }
        let mut r = a.clone();
        for _ in 1..n.min(64) {
            r = mul(&r, a, env);
        }
        return r;
    }
    if let (Some((s, "inf")), Some(k)) = (infinity(a), num_of(b)) {
        if k.is_zero() {
            return nan();
        }
        if k.is_negative() {
            return Expr::int(0);
        }
        let odd = matches!(k, Num::Int(n) if n.is_odd());
        return if s < 0 && odd { neg_inf() } else { inf() };
    }
    if is_one(b) {
        return a.clone();
    }
    if is_zero(b) && !is_vec(a) {
        return Expr::int(1);
    }
    Expr::call("^", vec![a.clone(), b.clone()])
}
