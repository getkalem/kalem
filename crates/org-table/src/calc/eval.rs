//! Evaluation (`math-normalize`): arguments first, then the operator or
//! function, which computes numbers and leaves what it cannot compute as
//! a formula ([`super::algebra`]).

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::algebra::{
    self, Cmp, add, compare, div, inf, infinity, is_constant, is_nan, is_vec, is_zero, looks_neg,
    mul, nan, neg, neg_inf, sub,
};
use super::expr::Expr;
use super::num::{self, Num, Prec, Reject};

/// The modes of an evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// Precision and fractions.
    pub prec: Prec,
    /// Angles in degrees (Org's default) rather than radians.
    pub degrees: bool,
}

impl Default for Env {
    fn default() -> Env {
        Env {
            prec: Prec::default(),
            degrees: true,
        }
    }
}

impl Env {
    /// Two more digits, as `math-with-extra-prec 2`.
    fn extra(&self) -> Env {
        Env {
            prec: Prec {
                digits: self.prec.digits + 2,
                ..self.prec
            },
            ..*self
        }
    }
}

/// Evaluates `e`.
pub fn normalize(e: &Expr, env: &Env) -> Expr {
    match e {
        Expr::Num(n) => Expr::Num(num::renormalize(n.clone(), &env.prec)),
        Expr::Date(n) => Expr::Date(num::renormalize(n.clone(), &env.prec)),
        Expr::Var(_) => e.clone(),
        Expr::Str(s) => Expr::Vec(
            s.chars()
                .map(|c| Expr::int(i64::from(u32::from(c))))
                .collect(),
        ),
        Expr::Vec(v) => Expr::Vec(v.iter().map(|x| normalize(x, env)).collect()),
        Expr::Intv(m, a, b) => {
            Expr::Intv(*m, Box::new(normalize(a, env)), Box::new(normalize(b, env)))
        }
        Expr::Call(f, args) if f == "if" && args.len() == 3 => normalize_if(args, env),
        Expr::Call(f, args) => {
            let args: Vec<Expr> = args.iter().map(|x| normalize(x, env)).collect();
            apply(f, args, env)
        }
    }
}

/// Normalization without simplification (`calc-simplify-mode` `none`):
/// numbers rounded, nothing computed.
fn normalize_none(e: &Expr, env: &Env) -> Expr {
    match e {
        Expr::Num(n) => Expr::Num(num::renormalize(n.clone(), &env.prec)),
        Expr::Vec(v) => Expr::Vec(v.iter().map(|x| normalize_none(x, env)).collect()),
        Expr::Str(_) => normalize(e, env),
        Expr::Call(f, args) => Expr::Call(
            f.clone(),
            args.iter().map(|x| normalize_none(x, env)).collect(),
        ),
        _ => e.clone(),
    }
}

/// `math-normalize-logical-op`: the condition first; an undecided
/// condition keeps both branches as they are.
fn normalize_if(args: &[Expr], env: &Env) -> Expr {
    let c = normalize(&args[0], env);
    if is_zero(&c) {
        return normalize(&args[2], env);
    }
    if matches!(c, Expr::Num(_)) {
        return normalize(&args[1], env);
    }
    if let Expr::Vec(conds) = &args[0]
        && is_constant(&args[0])
    {
        let e1 = normalize(&args[1], env);
        let e2 = normalize(&args[2], env);
        let pick = |e: &Expr, i: usize| -> Option<Expr> {
            match e {
                Expr::Vec(v) if v.len() == conds.len() => Some(v[i].clone()),
                Expr::Vec(_) => None,
                other => Some(other.clone()),
            }
        };
        let mut out = Vec::new();
        for (i, c) in conds.iter().enumerate() {
            let (Some(a), Some(b)) = (pick(&e1, i), pick(&e2, i)) else {
                return Expr::call("if", vec![args[0].clone(), e1, e2]);
            };
            out.push(if is_zero(c) { b } else { a });
        }
        return Expr::Vec(out);
    }
    Expr::call(
        "if",
        vec![
            c,
            normalize_none(&args[1], env),
            normalize_none(&args[2], env),
        ],
    )
}

fn keep(f: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(f.to_string(), args)
}

/// `math-is-true`, for numbers.
fn is_true(e: &Expr) -> bool {
    matches!(e, Expr::Num(n) if !n.is_zero())
}

/// A float with an integer value as that integer (Calc accepts them where
/// integers are needed).
fn integer(e: &Expr) -> Option<BigInt> {
    match e {
        Expr::Num(Num::Int(n)) => Some(n.clone()),
        Expr::Num(n @ Num::Float(..)) if n.is_messy_integer() => match num::trunc(n) {
            Num::Int(i) => Some(i),
            _ => None,
        },
        _ => None,
    }
}

fn apply(f: &str, args: Vec<Expr>, env: &Env) -> Expr {
    let n = args.len();
    // Functions of `nan` are `nan`.
    if n == 1
        && is_nan(&args[0])
        && matches!(
            f,
            "sqrt" | "exp" | "ln" | "log10" | "floor" | "ceil" | "round" | "trunc" | "abs" | "fact"
        )
    {
        return nan();
    }
    match (f, n) {
        ("+", 2) => add(&args[0], &args[1], env),
        ("-", 2) => sub(&args[0], &args[1], env),
        ("*", 2) => mul(&args[0], &args[1], env),
        ("/", 2) => div(&args[0], &args[1], env),
        // `mod(a, b)` is `a % b`.
        ("%", 2) | ("mod", 2) if is_zero(&args[0]) && !is_vec(&args[1]) => args[0].clone(),
        ("%", 2) | ("mod", 2) => binary_num("%", args, env, num::modulo),
        ("idiv", 2) if is_nan(&args[0]) || is_nan(&args[1]) => nan(),
        ("idiv", 2) => binary_num(f, args, env, num::idiv),
        ("^", 2) | ("pow", 2) => pow(&args[0], &args[1], env),
        ("neg", 1) => neg(&args[0], env),
        ("ident", 1) => args[0].clone(),
        ("add", _) => fold(args, env, add, Expr::int(0)),
        ("mul", _) => fold(args, env, mul, Expr::int(1)),
        ("sub", 2) => sub(&args[0], &args[1], env),
        ("div", 2) => div(&args[0], &args[1], env),
        ("percent", 1) => div(&args[0], &Expr::int(100), env),
        ("abs", 1) if matches!(args[0], Expr::Date(_)) => args[0].clone(),
        ("abs", 1) => match &args[0] {
            Expr::Vec(v) if !v.iter().any(is_vec) => {
                // The length of a vector.
                let sq = v
                    .iter()
                    .fold(Expr::int(0), |acc, x| add(&acc, &mul(x, x, env), env));
                apply("sqrt", vec![sq], env)
            }
            a if infinity(a).is_some_and(|(_, k)| k == "inf") => inf(),
            // `abs(-x)` is `abs(x)`.
            a if !matches!(a, Expr::Num(_)) && looks_neg(a) => {
                let x = neg(a, env);
                if looks_neg(&x) {
                    keep(f, args)
                } else {
                    apply("abs", vec![x], env)
                }
            }
            _ => map1(f, args, env, false, |a, _| Some(a.abs())),
        },
        ("floor", 1) | ("ceil", 1) | ("round", 1) | ("trunc", 1)
            if infinity(&args[0]).is_some_and(|(_, k)| k == "inf") =>
        {
            args[0].clone()
        }
        ("floor", 1) => map1(f, args, env, true, |a, _| Some(num::floor(a))),
        ("ceil", 1) => map1(f, args, env, true, |a, _| Some(num::ceil(a))),
        ("round", 1) => map1(f, args, env, true, |a, e| Some(num::round(a, &e.prec))),
        ("trunc", 1) => map1(f, args, env, true, |a, _| Some(num::trunc(a))),
        ("sign", 1) => match infinity(&args[0]) {
            Some((d, "inf")) => Expr::int(i64::from(d)),
            _ => map1(f, args, env, false, |a, _| {
                Some(Num::int(match (a.is_zero(), a.is_negative()) {
                    (true, _) => 0,
                    (_, true) => -1,
                    _ => 1,
                }))
            }),
        },
        ("fdiv", 2) => match (integer(&args[0]), integer(&args[1])) {
            (Some(a), Some(b)) if !b.is_zero() => Expr::Num(num::make_frac(a, b)),
            _ => binary_num(f, args, env, |a, b, p| {
                num::div(
                    a,
                    b,
                    &Prec {
                        prefer_frac: true,
                        ..*p
                    },
                )
            }),
        },
        ("sqrt", 1) => match (&args[0], infinity(&args[0])) {
            (_, Some((1, "inf"))) => inf(),
            // `sqrt(x / k)` is `sqrt(x) / sqrt(k)`.
            (Expr::Call(g, xs), _)
                if g == "/"
                    && xs.len() == 2
                    && matches!(&xs[1], Expr::Num(k) if !k.is_negative() && !k.is_zero()) =>
            {
                let x = apply("sqrt", vec![xs[0].clone()], env);
                let k = apply("sqrt", vec![xs[1].clone()], env);
                div(&x, &k, env)
            }
            // `sqrt(k x)` is `sqrt(k) sqrt(x)`.
            (Expr::Call(g, xs), _)
                if g == "*"
                    && xs.len() == 2
                    && matches!(&xs[0], Expr::Num(k) if !k.is_negative()) =>
            {
                let k = apply("sqrt", vec![xs[0].clone()], env);
                let x = apply("sqrt", vec![xs[1].clone()], env);
                mul(&k, &x, env)
            }
            _ => map1(f, args, env, false, sqrt),
        },
        ("exp", 1) => match infinity(&args[0]) {
            Some((1, "inf")) => inf(),
            Some((-1, "inf")) => Expr::int(0),
            _ => map1(f, args, env, false, |a, e| {
                if a.is_zero() {
                    return Some(Num::int(1));
                }
                exp(a, e)
            }),
        },
        ("ln", 1) | ("log10", 1) | ("log", 1)
            if infinity(&args[0]).is_some_and(|(_, k)| k == "inf") =>
        {
            inf()
        }
        ("ln", 1) => map1(f, args, env, false, |a, e| {
            if a.is_zero() || a.is_negative() {
                return None;
            }
            if a == &Num::int(1) {
                return Some(Num::int(0));
            }
            real_fn(a, e, f64::ln)
        }),
        ("log10", 1) | ("log", 1) => map1(f, args, env, false, log10),
        ("log", 2) => binary_num(f, args, env, |a, b, p| {
            let e = Env {
                prec: *p,
                degrees: true,
            };
            match (a.to_f64(), b.to_f64()) {
                (x, y) if x > 0. && y > 0. && y != 1. => exact_log(a, b)
                    .map(Ok)
                    .unwrap_or_else(|| real_fn(a, &e, |v| v.ln() / y.ln()).ok_or(Reject::Range)),
                _ => Err(Reject::Range),
            }
        }),
        ("sin", 1) => map1(f, args, env, false, |a, e| trig(a, e, Trig::Sin)),
        ("cos", 1) => map1(f, args, env, false, |a, e| trig(a, e, Trig::Cos)),
        ("tan", 1) => map1(f, args, env, false, |a, e| trig(a, e, Trig::Tan)),
        ("arcsin", 1) => map1(f, args, env, false, |a, e| inverse_trig(a, e, f64::asin)),
        ("arccos", 1) => map1(f, args, env, false, |a, e| inverse_trig(a, e, f64::acos)),
        ("arctan", 1) => map1(f, args, env, false, |a, e| inverse_trig(a, e, f64::atan)),
        ("fact", 1) => match integer(&args[0]).and_then(|n| n.to_i64()) {
            Some(k) if (0..=10_000).contains(&k) => {
                let r = Num::Int((1..=k).fold(BigInt::one(), |acc, i| acc * i));
                Expr::Num(if matches!(args[0], Expr::Num(Num::Float(..))) {
                    r.to_float(&env.prec)
                } else {
                    r
                })
            }
            _ => keep(f, args),
        },
        ("choose", 2) => match (integer(&args[0]), integer(&args[1])) {
            (Some(nn), Some(k)) => {
                if k.is_negative() || k > nn {
                    return Expr::int(0);
                }
                let r = {
                    let (Some(nn), Some(k)) = (nn.to_i64(), k.to_i64()) else {
                        return keep(f, args);
                    };
                    // `choose(n, k)` is `choose(n, n - k)`; beyond this the
                    // product would not finish.
                    let k = k.min(nn - k);
                    if k > 100_000 {
                        return keep(f, args);
                    }
                    let mut r = BigInt::one();
                    for i in 0..k {
                        r = r * (nn - i) / (i + 1);
                    }
                    r
                };
                let float = args.iter().any(|a| matches!(a, Expr::Num(Num::Float(..))));
                Expr::Num(if float {
                    Num::Int(r).to_float(&env.prec)
                } else {
                    Num::Int(r)
                })
            }
            _ => keep(f, args),
        },
        ("lcm", 2) if is_zero(&args[0]) || is_zero(&args[1]) => {
            let float = args.iter().any(|a| matches!(a, Expr::Num(Num::Float(..))));
            Expr::Num(if float {
                Num::Float(BigInt::zero(), 0)
            } else {
                Num::int(0)
            })
        }
        ("gcd", 2) | ("lcm", 2) => match (integer(&args[0]), integer(&args[1])) {
            (Some(a), Some(b)) => {
                let r = if f == "gcd" { a.gcd(&b) } else { a.lcm(&b) };
                // `gcd` gives an integer; `lcm` keeps a float.
                let float =
                    f == "lcm" && args.iter().any(|a| matches!(a, Expr::Num(Num::Float(..))));
                Expr::Num(if float {
                    Num::Int(r).to_float(&env.prec)
                } else {
                    Num::Int(r)
                })
            }
            _ => keep(f, args),
        },
        ("max", _) | ("min", _) if n > 0 => min_max(f, args, env),
        ("lt", 2) | ("gt", 2) | ("leq", 2) | ("geq", 2) => inequality(f, args, env),
        ("eq", 2) | ("neq", 2) => equality(f, args, env),
        ("land", 2) => {
            if is_zero(&args[0]) {
                args[0].clone()
            } else if is_zero(&args[1]) || is_true(&args[0]) {
                args[1].clone()
            } else if is_true(&args[1]) {
                args[0].clone()
            } else {
                keep(f, args)
            }
        }
        ("lor", 2) => {
            if is_zero(&args[0]) {
                args[1].clone()
            } else if is_zero(&args[1]) || is_true(&args[0]) {
                args[0].clone()
            } else if is_true(&args[1]) {
                args[1].clone()
            } else {
                keep(f, args)
            }
        }
        ("lnot", 1) => {
            if is_zero(&args[0]) {
                return Expr::int(1);
            }
            if is_true(&args[0]) {
                return Expr::int(0);
            }
            if let Expr::Call(g, xs) = &args[0]
                && xs.len() == 2
            {
                let flipped = match g.as_str() {
                    "lt" => Some("geq"),
                    "gt" => Some("leq"),
                    "leq" => Some("gt"),
                    "geq" => Some("lt"),
                    "eq" => Some("neq"),
                    "neq" => Some("eq"),
                    _ => None,
                };
                if let Some(h) = flipped {
                    return Expr::call(h, xs.clone());
                }
            }
            keep(f, args)
        }
        ("year", 1) | ("month", 1) | ("day", 1) | ("weekday", 1) => match &args[0] {
            Expr::Date(d) => match super::date::parts(d, &env.prec) {
                Some((y, m, dd, w)) => Expr::int(match f {
                    "year" => y,
                    "month" => m,
                    "day" => dd,
                    _ => w,
                }),
                None => keep(f, args),
            },
            _ => keep(f, args),
        },
        ("vsum", _) => reduce(f, args, env, add, Expr::int(0)),
        ("vprod", _) => reduce(f, args, env, mul, Expr::int(1)),
        ("vmax", _) => reduce(
            f,
            args,
            env,
            |a, b, e| min_max("max", vec![a.clone(), b.clone()], e),
            neg_inf(),
        ),
        ("vmin", _) => reduce(
            f,
            args,
            env,
            |a, b, e| min_max("min", vec![a.clone(), b.clone()], e),
            inf(),
        ),
        ("vcount", _) => match flatten(&args) {
            Some(v) => Expr::int(v.len() as i64),
            None => keep(f, args),
        },
        ("vlen", 1) => match &args[0] {
            Expr::Vec(v) => Expr::int(v.len() as i64),
            Expr::Num(_) => Expr::int(0),
            _ => keep(f, args),
        },
        ("vmean", _) => vmean(args, env),
        ("vmedian", _) => vmedian(args, env),
        ("vvar", _) | ("vsdev", _) | ("vpvar", _) | ("vpsdev", _) => variance(f, args, env),
        _ => keep(f, args),
    }
}

/// `calcFunc-lt` and the others: 1 or 0, or the comparison kept (signs
/// flipped when both sides look negative).
fn inequality(f: &str, args: Vec<Expr>, env: &Env) -> Expr {
    let res = compare(&args[0], &args[1], env);
    let t = match (f, res) {
        (_, Cmp::Unknown) => {
            let negish = |e: &Expr| looks_neg(e) || is_zero(e);
            if negish(&args[0]) && negish(&args[1]) {
                let flipped = match f {
                    "lt" => "gt",
                    "gt" => "lt",
                    "leq" => "geq",
                    _ => "leq",
                };
                return Expr::call(flipped, vec![neg(&args[0], env), neg(&args[1], env)]);
            }
            return keep(f, args);
        }
        ("lt", r) => r == Cmp::Less,
        ("gt", r) => r == Cmp::Greater,
        ("leq", r) => r != Cmp::Greater,
        (_, r) => r != Cmp::Less,
    };
    Expr::int(i64::from(t))
}

/// `math-two-eq`: 1 equal, 0 different, `None` unknown.
fn two_eq(a: &Expr, b: &Expr, env: &Env) -> Option<i64> {
    match (a, b) {
        (Expr::Vec(x), Expr::Vec(y)) => {
            if x.len() != y.len() {
                return Some(0);
            }
            let mut res = Some(1);
            for (p, q) in x.iter().zip(y) {
                match res {
                    Some(_) => res = two_eq(p, q, env),
                    None if two_eq(p, q, env) == Some(0) => res = Some(0),
                    None => {}
                }
                if res == Some(0) {
                    break;
                }
            }
            res
        }
        (Expr::Vec(_), o) | (o, Expr::Vec(_)) => matches!(o, Expr::Num(_)).then_some(0),
        _ => match compare(a, b, env) {
            Cmp::Equal => Some(1),
            Cmp::Unknown if !(matches!(a, Expr::Num(_)) && matches!(b, Expr::Num(_))) => None,
            _ => Some(0),
        },
    }
}

/// `calcFunc-eq` and `calcFunc-neq`.
fn equality(f: &str, args: Vec<Expr>, env: &Env) -> Expr {
    match two_eq(&args[0], &args[1], env) {
        Some(r) => Expr::int(if f == "eq" { r } else { 1 - r }),
        None => {
            let negish = |e: &Expr| looks_neg(e) || is_zero(e);
            if negish(&args[0]) && negish(&args[1]) {
                return Expr::call(f, vec![neg(&args[0], env), neg(&args[1], env)]);
            }
            keep(f, args)
        }
    }
}

/// Folds `args` pairwise with `op` (`calcFunc-add` with many arguments).
fn fold(args: Vec<Expr>, env: &Env, op: fn(&Expr, &Expr, &Env) -> Expr, ident: Expr) -> Expr {
    let mut it = args.into_iter();
    let Some(first) = it.next() else { return ident };
    it.fold(first, |acc, x| op(&acc, &x, env))
}

/// `math-flatten-many-vecs`: the elements of vectors and the scalars
/// among `args`, or `None` if one is neither.
fn flatten(args: &[Expr]) -> Option<Vec<Expr>> {
    let mut out = Vec::new();
    fn walk(e: &Expr, out: &mut Vec<Expr>) {
        match e {
            Expr::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            other => out.push(other.clone()),
        }
    }
    for a in args {
        match a {
            Expr::Vec(_) => walk(a, &mut out),
            Expr::Num(_) | Expr::Date(_) => out.push(a.clone()),
            _ if infinity(a).is_some() => out.push(a.clone()),
            _ => return None,
        }
    }
    Some(out)
}

/// `math-reduce-many-vecs`: vectors flattened and reduced with two more
/// digits, symbolic arguments added at the end.
fn reduce(
    f: &str,
    args: Vec<Expr>,
    env: &Env,
    op: impl Fn(&Expr, &Expr, &Env) -> Expr,
    ident: Expr,
) -> Expr {
    let wide = env.extra();
    let mut constant: Option<Expr> = None;
    let mut symbolic = Vec::new();
    for a in &args {
        match a {
            Expr::Vec(_) => {
                let mut items = Vec::new();
                if let Some(c) = constant.take() {
                    items.push(c);
                }
                let mut flat = flatten(std::slice::from_ref(a)).unwrap_or_default();
                items.append(&mut flat);
                let mut it = items.into_iter();
                constant = Some(match it.next() {
                    Some(first) => it.fold(first, |acc, x| op(&acc, &x, &wide)),
                    None => ident.clone(),
                });
            }
            _ if is_constant(a) || infinity(a).is_some() => {
                constant = Some(match constant.take() {
                    Some(c) => op(&c, a, &wide),
                    None => a.clone(),
                });
            }
            _ => symbolic.push(a.clone()),
        }
    }
    match constant {
        Some(c) => {
            let c = normalize(&c, env);
            if symbolic.is_empty() {
                c
            } else {
                op(&c, &keep(f, symbolic), env)
            }
        }
        None if symbolic.is_empty() => ident,
        None => keep(f, symbolic),
    }
}

/// `calcFunc-vmean`.
fn vmean(args: Vec<Expr>, env: &Env) -> Expr {
    let Some(flat) = flatten(&args) else {
        return keep("vmean", args);
    };
    if flat.is_empty() {
        return keep("vmean", args);
    }
    let wide = env.extra();
    let len = flat.len() as i64;
    let mut it = flat.into_iter();
    let first = it.next().expect("not empty");
    let sum = it.fold(first, |acc, x| add(&acc, &x, &wide));
    normalize(&div(&sum, &Expr::int(len), &wide), env)
}

/// `calcFunc-vmedian`.
fn vmedian(args: Vec<Expr>, env: &Env) -> Expr {
    let Some(mut flat) = flatten(&args) else {
        return keep("vmedian", args);
    };
    if flat.is_empty() || !flat.iter().all(|x| matches!(x, Expr::Num(_))) {
        return keep("vmedian", args);
    }
    let len = flat.len();
    flat.sort_by(|a, b| match (a, b) {
        (Expr::Num(x), Expr::Num(y)) => num::cmp(x, y, &env.prec),
        _ => Ordering::Equal,
    });
    if len % 2 == 0 {
        div(
            &add(&flat[len / 2 - 1], &flat[len / 2], env),
            &Expr::int(2),
            env,
        )
    } else {
        flat.swap_remove(len / 2)
    }
}

/// `math-covariance` of one vector: sample or population variance, and
/// its square root.
fn variance(f: &str, args: Vec<Expr>, env: &Env) -> Expr {
    // The elements of vectors, formulas included (`math-flatten-many-vecs`
    // takes any element of a vector).
    let mut flat = Vec::new();
    fn walk(e: &Expr, out: &mut Vec<Expr>) {
        match e {
            Expr::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            other => out.push(other.clone()),
        }
    }
    for a in &args {
        match a {
            Expr::Vec(_) => walk(a, &mut flat),
            Expr::Num(_) | Expr::Date(_) => flat.push(a.clone()),
            _ if infinity(a).is_some() => flat.push(a.clone()),
            _ => return keep(f, args),
        }
    }
    let pop = f.starts_with("vp");
    let len = flat.len() as i64;
    if pop && len == 1 {
        return Expr::int(0);
    }
    if len < if pop { 1 } else { 2 } {
        return keep(f, args);
    }
    let wide = env.extra();
    let sum = fold(flat.clone(), &wide, add, Expr::int(0));
    // Each value minus the mean, squared.
    let mean = div(&sum, &Expr::int(-len), &wide);
    let squares = flat
        .iter()
        .map(|x| {
            let d = add(x, &mean, &wide);
            mul(&d, &d, &wide)
        })
        .collect();
    let total = fold(squares, &wide, add, Expr::int(0));
    let var = div(&total, &Expr::int(if pop { len } else { len - 1 }), &wide);
    let var = normalize(&var, env);
    if f.ends_with("sdev") {
        apply("sqrt", vec![var], env)
    } else {
        var
    }
}

/// A function of one real argument; vectors are mapped only when `map`.
fn map1(
    f: &str,
    args: Vec<Expr>,
    env: &Env,
    map: bool,
    op: impl Fn(&Num, &Env) -> Option<Num> + Copy,
) -> Expr {
    match &args[0] {
        Expr::Num(a) => match op(a, env) {
            Some(r) => Expr::Num(num::renormalize(r, &env.prec)),
            None => keep(f, args),
        },
        // Mapped over vectors of numbers only: `round([a])` stays.
        Expr::Vec(v) if map && v.iter().all(|x| matches!(x, Expr::Num(_))) => Expr::Vec(
            v.iter()
                .map(|x| map1(f, vec![x.clone()], env, map, op))
                .collect(),
        ),
        _ => keep(f, args),
    }
}

/// A function of two real arguments.
fn binary_num(
    f: &str,
    args: Vec<Expr>,
    env: &Env,
    op: impl Fn(&Num, &Num, &Prec) -> Result<Num, Reject>,
) -> Expr {
    match (&args[0], &args[1]) {
        (Expr::Num(a), Expr::Num(b)) => match op(a, b, &env.prec) {
            Ok(r) => Expr::Num(r),
            Err(_) => keep(f, args),
        },
        _ => keep(f, args),
    }
}

/// A real function computed in double precision and rounded to the
/// working precision.
fn real_fn(a: &Num, env: &Env, f: impl Fn(f64) -> f64) -> Option<Num> {
    let r = f(a.to_f64());
    from_f64(r, env)
}

/// A double as a Calc float at the working precision.
fn from_f64(r: f64, env: &Env) -> Option<Num> {
    if !r.is_finite() {
        return None;
    }
    // Seventeen digits identify the double; rounding them to the working
    // precision gives Calc's digits.
    let s = format!("{r:.16e}");
    let (m, e) = s.split_once('e')?;
    let neg = m.starts_with('-');
    let digits: String = m.chars().filter(char::is_ascii_digit).collect();
    let mut mant: BigInt = digits.parse().ok()?;
    if neg {
        mant = -mant;
    }
    let exp: i64 = e.parse::<i64>().ok()? - 16;
    let prec = Prec {
        digits: env.prec.digits.min(15),
        ..env.prec
    };
    Some(num::make_float(mant, exp, &prec))
}

/// `ln 10` to 40 digits.
const LN10: &str = "2.302585092994045684017960784327646021358";

/// `e^x` for any size of `x`: `10^n e^r` with `x = n ln 10 + r`, the
/// reduction done in decimal so that large arguments keep their digits.
fn exp(a: &Num, env: &Env) -> Option<Num> {
    let x = a.to_f64();
    if x.abs() < 700. {
        return real_fn(a, env, f64::exp);
    }
    let n = (x / std::f64::consts::LN_10).floor();
    if !n.is_finite() || n.abs() > 4_000_000. {
        return None;
    }
    let wide = Prec {
        digits: 40,
        prefer_frac: false,
    };
    let ln10 = num::read(LN10, &wide)?;
    let r = num::sub(
        &a.to_float(&wide),
        &num::mul(&Num::int(n as i64), &ln10, &wide),
        &wide,
    );
    let m = from_f64(r.to_f64().exp(), env)?;
    let Num::Float(mant, e) = m else { return None };
    Some(num::make_float(mant, e + n as i64, &env.prec))
}

/// `calcFunc-sqrt`: exact for perfect squares of integers and fractions.
fn sqrt(a: &Num, env: &Env) -> Option<Num> {
    if a.is_negative() {
        return None;
    }
    match a {
        Num::Int(n) => {
            let r = n.sqrt();
            if &(&r * &r) == n {
                return Some(Num::Int(r));
            }
        }
        Num::Frac(p, q) => {
            let (rp, rq) = (p.sqrt(), q.sqrt());
            if &(&rp * &rp) == p && &(&rq * &rq) == q {
                return Some(num::make_frac(rp, rq));
            }
        }
        Num::Float(..) => {}
    }
    sqrt_float(a, env)
}

/// The square root of a real, correctly rounded at the working precision.
fn sqrt_float(a: &Num, env: &Env) -> Option<Num> {
    let Num::Float(m, e) = a.to_float(&env.prec) else {
        return None;
    };
    // `m × 10^e` scaled to an even exponent with enough digits.
    let digits = env.prec.digits + 4;
    let mut shift = 2 * digits - num::numdigs(&m);
    if (e - shift).rem_euclid(2) != 0 {
        shift += 1;
    }
    let scaled = num::scale_int(&m, shift);
    let root = scaled.sqrt();
    Some(num::make_float(root, (e - shift) / 2, &env.prec))
}

/// `calcFunc-log10`: exact for powers of ten.
fn log10(a: &Num, env: &Env) -> Option<Num> {
    if a.is_zero() || a.is_negative() {
        return None;
    }
    if let Num::Int(n) = a {
        let s = n.to_string();
        if s.starts_with('1') && s[1..].bytes().all(|c| c == b'0') {
            return Some(Num::int(s.len() as i64 - 1));
        }
    }
    real_fn(a, env, f64::log10)
}

/// An exact logarithm of integers, `log(8, 2)` = 3.
fn exact_log(a: &Num, b: &Num) -> Option<Num> {
    let (Num::Int(x), Num::Int(base)) = (a, b) else {
        return None;
    };
    if base <= &BigInt::one() || x <= &BigInt::zero() {
        return None;
    }
    let mut p = BigInt::one();
    let mut k = 0i64;
    while &p < x {
        p *= base;
        k += 1;
    }
    (&p == x).then(|| Num::int(k))
}

#[derive(Clone, Copy)]
enum Trig {
    Sin,
    Cos,
    Tan,
}

/// Sine, cosine and tangent; exact at multiples of 90 degrees for sine and
/// cosine, as Calc is.
fn trig(a: &Num, env: &Env, which: Trig) -> Option<Num> {
    if env.degrees {
        if let Some(k) = exact_quarter(a) {
            let (s, c) = match k.rem_euclid(4) {
                0 => (0, 1),
                1 => (1, 0),
                2 => (0, -1),
                _ => (-1, 0),
            };
            match which {
                Trig::Sin => return Some(Num::int(s)),
                Trig::Cos => return Some(Num::int(c)),
                Trig::Tan if s == 0 => return Some(Num::int(0)),
                Trig::Tan => return None,
            }
        }
    } else if a.is_zero() {
        return Some(match which {
            Trig::Cos => Num::int(1),
            _ => Num::int(0),
        });
    }
    let x = a.to_f64();
    let rad = if env.degrees { x.to_radians() } else { x };
    let r = match which {
        Trig::Sin => rad.sin(),
        Trig::Cos => rad.cos(),
        Trig::Tan => rad.tan(),
    };
    from_f64(r, env)
}

/// `a / 90` if it is an integer.
fn exact_quarter(a: &Num) -> Option<i64> {
    match a {
        Num::Int(n) => {
            let (q, r) = n.div_rem(&BigInt::from(90));
            r.is_zero().then(|| q.to_i64()).flatten()
        }
        Num::Float(m, e) if *e >= 0 => exact_quarter(&Num::Int(num::scale_int(m, *e))),
        _ => None,
    }
}

fn inverse_trig(a: &Num, env: &Env, f: fn(f64) -> f64) -> Option<Num> {
    let r = f(a.to_f64());
    if r.is_nan() {
        return None;
    }
    from_f64(if env.degrees { r.to_degrees() } else { r }, env)
}

/// `calcFunc-max` and `calcFunc-min` of reals and infinities.
fn min_max(f: &str, args: Vec<Expr>, env: &Env) -> Expr {
    if args.iter().any(is_nan) {
        return nan();
    }
    let dates = args.iter().all(|a| matches!(a, Expr::Date(_)));
    if !dates
        && !args
            .iter()
            .all(|a| matches!(a, Expr::Num(_)) || infinity(a).is_some_and(|(s, _)| s != 0))
    {
        return keep(f, args);
    }
    let mut best = args[0].clone();
    for a in &args[1..] {
        let c = compare(a, &best, env);
        if (f == "max" && c == Cmp::Greater) || (f == "min" && c == Cmp::Less) {
            best = a.clone();
        }
    }
    best
}

/// `math-pow`.
fn pow(a: &Expr, b: &Expr, env: &Env) -> Expr {
    if is_nan(b) {
        return b.clone();
    }
    let (Expr::Num(x), Expr::Num(y)) = (a, b) else {
        return algebra::pow(a, b, env);
    };
    // `(or (eq a 1) (eq b 1)) a`.
    if x == &Num::int(1) || y == &Num::int(1) {
        return a.clone();
    }
    if x.is_zero() {
        if y.is_zero() {
            return Expr::int(1);
        }
        if y.is_negative() {
            return Expr::call("^", vec![a.clone(), b.clone()]);
        }
        return Expr::Num(if y.is_float() {
            x.to_float(&env.prec)
        } else {
            x.clone()
        });
    }
    if y.is_zero() {
        return Expr::Num(if x.is_float() || y.is_float() {
            num::make_float(BigInt::one(), 0, &env.prec)
        } else {
            Num::int(1)
        });
    }
    if let Num::Int(n) = y {
        return match num::ipow(x, n, &env.prec) {
            Ok(r) => Expr::Num(r),
            Err(_) => Expr::call("^", vec![a.clone(), b.clone()]),
        };
    }
    if x.is_negative() {
        return Expr::call("^", vec![a.clone(), b.clone()]);
    }
    // `a^(1/2)` is the square root, exact when possible.
    if y == &Num::Frac(BigInt::one(), BigInt::from(2)) {
        return match sqrt(x, env) {
            Some(r) => Expr::Num(r),
            None => Expr::call("^", vec![a.clone(), b.clone()]),
        };
    }
    if matches!(y, Num::Float(m, -1) if *m == BigInt::from(5)) && !x.is_float() {
        // `4^0.5` is `2.`: the square root as a float.
        return match sqrt_float(x, env) {
            Some(r) => Expr::Num(r),
            None => Expr::call("^", vec![a.clone(), b.clone()]),
        };
    }
    match real_fn(x, env, |v| v.powf(y.to_f64())) {
        Some(r) => Expr::Num(r),
        None => Expr::call("^", vec![a.clone(), b.clone()]),
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse::parse;
    use super::super::print::format;
    use super::*;

    fn ev(s: &str) -> String {
        let env = Env::default();
        let e = &parse(s, &env.prec).unwrap()[0];
        format(&normalize(e, &env), &Default::default(), &env.prec)
    }

    #[test]
    fn numbers() {
        for (i, o) in [
            ("1/3", "0.33333333"),
            ("6/3", "2"),
            ("1.5+1.5", "3."),
            ("2^100", "1267650600228229401496703205376"),
            ("2^0.5", "1.4142136"),
            ("4^0.5", "2."),
            ("10^-2", "0.01"),
            ("1/7*7", "0.020408163"),
            ("(1/3)*3", "1.00000000"),
            ("-7%3", "2"),
            ("sin(30)", "0.5"),
            ("sin(90)", "1"),
            ("tan(45)", "1."),
            ("sqrt(16)", "4"),
            ("sqrt(2)", "1.4142136"),
            ("log10(100)", "2"),
            ("log10(2)", "0.30103000"),
            ("exp(1)", "2.7182818"),
            ("ln(10)", "2.3025851"),
            ("round(-2.5)", "-3"),
            ("max(1,5,3)", "5"),
            ("if(1>2,3,4)", "4"),
            ("2 && 3", "3"),
            ("vsum([1,2,3])", "6"),
            ("vmean([1,2,4])", "2.3333333"),
            ("vmedian([3,1,2,4])", "2.5"),
            ("vsdev([1,2,3,4])", "1.2909944"),
            ("vvar([1,2,3,4])", "1.6666667"),
            ("vmax([])", "-inf"),
            ("vsum(1,2,[3,4])", "10"),
            ("[1,2]*[3,4]", "11"),
            ("[1,2]+1", "[2, 3]"),
            ("fdiv(7,2)", "7:2"),
            ("inf-inf", "nan"),
            ("1/inf", "0"),
            ("vsum([1,nan])", "nan"),
            ("50%", "0.5"),
        ] {
            assert_eq!(ev(i), o, "{i}");
        }
    }

    #[test]
    fn formulas() {
        for (i, o) in [
            ("(a)+1", "a + 1"),
            ("1+(a)", "1 + a"),
            ("a+a", "2 a"),
            ("(a)-(a)", "0"),
            ("2*(a)", "2 a"),
            ("(a)*2", "2 a"),
            ("a*b", "a b"),
            ("a/2", "a / 2"),
            ("-a", "-a"),
            ("a*a", "a^2"),
            ("vsum([a,1,2])", "a + 3"),
            ("vsum([1,2,a])", "3 + a"),
            ("vmean([a,1])", "a / 2 + 0.5"),
            ("a+b+1", "a + b + 1"),
            ("3-a", "3 - a"),
            ("a-3", "a - 3"),
            ("2*a+3*a", "5 a"),
            ("(a)*0", "0"),
            ("sqrt(a)", "sqrt(a)"),
            ("max(a,1)", "max(a, 1)"),
            ("a < 2", "a < 2"),
            ("if(a,1,2)", "a ? 1 : 2"),
            ("a/b c", "a / (b c)"),
            ("1/0", "1/0"),
            ("1.5/0", "1.5 / 0"),
            ("ln(0)", "ln(0)"),
        ] {
            assert_eq!(ev(i), o, "{i}");
        }
    }
}
