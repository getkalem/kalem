//! What Calc can tell of a formula's sign and type without declarations
//! (`math-possible-signs`, `math-possible-types` and the `math-known-`
//! tests built on them), and the rules of `math-pow-fancy` that ask them:
//! `(-x)^2` is `x^2`, `(a b)^2` is `a^2 b^2`, `(x^2)^3` is `x^6`.

use num_integer::Integer;

use super::algebra::{
    div, infinity, is_one, is_zero, looks_neg, mod_form, mul, nan, neg, num_of, refuse, refused,
};
use super::cplx;
use super::eval::{self, Env};
use super::expr::Expr;
use super::num::{self, Num};

/// `math-possible-signs`' bits: may be negative, zero, positive, not real.
const NEG: u8 = 1;
const ZERO: u8 = 2;
const POS: u8 = 4;
const NONREAL: u8 = 8;

/// The signs `a - origin` may have. Long sums nest to the left and long
/// products to the right, a level a term (`vsum` and `vprod` over a
/// column of words): down their spines in a loop, the signs worked out on
/// the way back up, where Calc recursed a level a term.
fn signs(a: &Expr, origin: Option<Num>, env: &Env) -> u8 {
    enum Up {
        /// A sum's term (its signs, negated for a difference).
        Term(u8),
        /// A product's first factor, and the origin it is compared with.
        Factor(u8, Option<Num>),
        /// A product by a negative number.
        Negated,
    }
    let p = &env.prec;
    let (mut a, mut origin) = (a, origin);
    let mut ups = Vec::new();
    let mut s = loop {
        let Expr::Call(f, xs) = a else {
            break signs_at(a, origin, env);
        };
        match f.as_str() {
            "+" | "-" if xs.len() == 2 && num_of(&xs[0]).is_none() => {
                let minus = f == "-";
                if let Some(k) = num_of(&xs[1]) {
                    let org = if minus { k.clone() } else { k.neg() };
                    origin = Some(origin.map_or_else(|| org.clone(), |o| num::add(&o, &org, p)));
                } else {
                    let s2 = signs(&xs[1], None, env);
                    ups.push(Up::Term(if minus { neg_signs(s2) } else { s2 }));
                }
                a = &xs[0];
            }
            "*" if xs.len() == 2 && num_of(&xs[1]).is_none() => {
                let o = origin.filter(|o| !o.is_zero());
                match num_of(&xs[0]) {
                    Some(k) if k.is_zero() => break signs_at(&Expr::int(0), o, env),
                    Some(k) => {
                        if k.is_negative() {
                            ups.push(Up::Negated);
                        }
                        origin = num::div(o.as_ref().unwrap_or(&Num::int(0)), k, p).ok();
                    }
                    None => {
                        ups.push(Up::Factor(signs(&xs[0], None, env), o));
                        origin = None;
                    }
                }
                a = &xs[1];
            }
            _ => break signs_at(a, origin, env),
        }
    };
    for up in ups.into_iter().rev() {
        s = match up {
            Up::Term(s2) => sum_signs(s, s2),
            Up::Factor(s1, o) => against(product_signs(s1, s), o.as_ref()),
            Up::Negated => neg_signs(s),
        };
    }
    s
}

/// [`signs`] of anything but the spines of sums and products.
fn signs_at(a: &Expr, origin: Option<Num>, env: &Env) -> u8 {
    let p = &env.prec;
    match a {
        Expr::Num(n) => {
            let d = match &origin {
                Some(o) => num::sub(n, o, p),
                None => n.clone(),
            };
            sign_of(&d)
        }
        Expr::Date(n) => match sign_of(n) {
            ZERO => NONREAL,
            s => s,
        },
        Expr::Intv(..) => NEG | ZERO | POS,
        _ if cplx::parts(a).is_some() => NONREAL,
        _ if mod_form(a).is_some() => {
            let (x, _) = mod_form(a).expect("a modulo form");
            if x.is_zero() { ZERO } else { POS }
        }
        // A number plus or less something (the rest of a sum is [`signs`]').
        Expr::Call(f, xs)
            if (f == "+" || f == "-") && xs.len() == 2 && num_of(&xs[0]).is_some() =>
        {
            let k = num_of(&xs[0]).expect("a number");
            if f == "-" {
                let o = origin.map_or_else(|| k.clone(), |o| num::add(&o, k, p));
                neg_signs(signs(&xs[1], Some(o), env))
            } else {
                let o = origin.map_or_else(|| k.neg(), |o| num::sub(&o, k, p));
                signs(&xs[1], Some(o), env)
            }
        }
        Expr::Call(f, xs) if f == "neg" && xs.len() == 1 => {
            neg_signs(signs(&xs[0], origin.map(|o| o.neg()), env))
        }
        Expr::Vec(items) => {
            let mut s = 0;
            for x in items {
                if s >= 15 {
                    break;
                }
                s |= signs(x, origin.clone(), env);
            }
            s
        }
        _ => {
            let origin = origin.filter(|o| !o.is_zero());
            if let Expr::Call(f, xs) = a
                && xs.len() == 2
                && let Some(s) = scaled_signs(f, xs, origin.as_ref(), env)
            {
                return s;
            }
            let (sign, origin) = match call_signs(a, origin.as_ref(), env) {
                Some(r) => r,
                None => {
                    return match a {
                        Expr::Var(v) => const_signs(v, origin.as_ref()),
                        _ => 15,
                    };
                }
            };
            against(sign, origin.as_ref())
        }
    }
}

/// The signs of a sum from those of its two sides.
fn sum_signs(s1: u8, s2: u8) -> u8 {
    match (s1, s2) {
        _ if s1 == s2 => s1,
        (ZERO, _) => s2,
        (_, ZERO) => s1,
        _ if s1 >= NONREAL || s2 >= NONREAL => 15,
        (4, 6) | (6, 4) => POS,
        (1, 3) | (3, 1) => NEG,
        _ => 7,
    }
}

/// The signs of a product from those of its two factors.
fn product_signs(s1: u8, s2: u8) -> u8 {
    if s1 >= NONREAL || s2 >= NONREAL {
        return 15;
    }
    (if [4, 5, 6, 7].contains(&s1) { s2 } else { 0 })
        | (if [2, 3, 6, 7].contains(&s1) { ZERO } else { 0 })
        | (if [1, 3, 5, 7].contains(&s1) {
            neg_signs(s2)
        } else {
            0
        })
}

/// Signs about zero as signs about an origin other than zero.
fn against(sign: u8, origin: Option<&Num>) -> u8 {
    let Some(o) = origin else {
        return sign;
    };
    (sign & NONREAL)
        + if sign_of(o) == POS {
            if [1, 2, 3, 8, 9, 10, 11].contains(&sign) {
                NEG
            } else {
                7
            }
        } else if [2, 4, 6, 8, 10, 12, 14].contains(&sign) {
            POS
        } else {
            7
        }
}

/// The signs of `k x` or `x / k` from those of `x` against an origin
/// moved by `k`, a real number.
fn scaled_signs(f: &str, xs: &[Expr], origin: Option<&Num>, env: &Env) -> Option<u8> {
    let p = &env.prec;
    let zero = Num::int(0);
    if (f == "*" || (f == "/" && origin.is_some()))
        && let Some(k) = num_of(&xs[0])
    {
        let s = if f == "*" {
            if k.is_zero() {
                signs(&Expr::int(0), origin.cloned(), env)
            } else {
                signs(&xs[1], num::div(origin.unwrap_or(&zero), k, p).ok(), env)
            }
        } else {
            let o = origin.expect("an origin");
            neg_signs(signs(&xs[1], num::div(k, o, p).ok(), env))
        };
        return Some(if k.is_negative() { neg_signs(s) } else { s });
    }
    if (f == "*" || f == "/")
        && let Some(k) = num_of(&xs[1])
    {
        // Calc divides the origin by it: a division by zero, refused.
        if f == "/" && k.is_zero() {
            refuse();
            return Some(15);
        }
        let o = origin.unwrap_or(&zero);
        let o = if f == "*" {
            Some(num::mul(o, k, p))
        } else {
            num::div(o, k, p).ok()
        };
        let s = signs(&xs[0], o, env);
        return Some(if k.is_negative() { neg_signs(s) } else { s });
    }
    None
}

/// The signs of an operator or a function Calc knows, and the origin
/// they are then compared with (`exp` moves it); `None` for others.
fn call_signs(a: &Expr, origin: Option<&Num>, env: &Env) -> Option<(u8, Option<Num>)> {
    let Expr::Call(f, xs) = a else {
        return None;
    };
    let s = |e: &Expr| signs(e, None, env);
    let sign = match (f.as_str(), xs.len()) {
        ("*" | "/", 2) => {
            let (s1, s2) = (s(&xs[0]), s(&xs[1]));
            if f == "/" && s1 < NONREAL && [2, 3, 6, 7].contains(&s2) {
                15
            } else {
                product_signs(s1, s2)
            }
        }
        ("^", 2) => {
            let (s1, s2) = (s(&xs[0]), s(&xs[1]));
            match (s1, s2) {
                _ if s1 >= NONREAL || s2 >= NONREAL => 15,
                (POS, _) => POS,
                (ZERO, _) => {
                    if s2 == POS {
                        ZERO
                    } else {
                        15
                    }
                }
                (_, ZERO) => {
                    if s1 == 1 || s1 == 5 {
                        ZERO
                    } else {
                        15
                    }
                }
                _ => match &xs[1] {
                    Expr::Num(Num::Int(m)) => {
                        if m.is_even() {
                            if [3, 6, 7].contains(&s1) { 6 } else { POS }
                        } else {
                            s1
                        }
                    }
                    _ if s1 == 6 => {
                        if s2 == POS {
                            6
                        } else {
                            15
                        }
                    }
                    _ => 7,
                },
            }
        }
        ("%", 2) => match s(&xs[1]) {
            s2 if s2 >= NONREAL => 7,
            ZERO => ZERO,
            4 | 6 => 6,
            1 | 3 => 3,
            _ => 7,
        },
        ("abs" | "abssqr", 1) => match s(&xs[0]) {
            ZERO => ZERO,
            1 | 4 | 5 => POS,
            _ => 6,
        },
        ("exp", 1) => {
            let s1 = s(&xs[0]);
            if s1 >= NONREAL {
                15
            } else if origin.is_none_or(Num::is_negative) {
                POS
            } else {
                let o = num::sub(origin.expect("an origin"), &Num::int(1), &env.prec);
                return Some((s1, (!o.is_zero()).then_some(o)));
            }
        }
        ("ln" | "log10", 1) => {
            if known_nonneg(&xs[0], env) {
                signs(&xs[0], Some(Num::int(1)), env)
            } else {
                15
            }
        }
        ("log", 2) if signs(&xs[1], None, env) == POS => {
            if known_nonneg(&xs[0], env) {
                signs(&xs[0], Some(Num::int(1)), env)
            } else {
                15
            }
        }
        ("sqrt", 1) => match s(&xs[0]) {
            s1 @ (2 | 4 | 6) => s1,
            _ => 15,
        },
        ("cnorm" | "rnorm" | "vlen" | "vcount", _) => 6,
        (
            "arg" | "re" | "im" | "floor" | "ceil" | "trunc" | "round" | "rounde" | "roundu"
            | "ffloor" | "fceil" | "ftrunc" | "fround" | "frounde" | "froundu",
            _,
        ) => 7,
        (
            "%" | "idiv" | "abs" | "choose" | "perm" | "eq" | "neq" | "lt" | "gt" | "leq" | "geq"
            | "lnot" | "max" | "min",
            _,
        ) => 7,
        (
            "sin" | "cos" | "tan" | "sec" | "csc" | "cot" | "arctan" | "sinh" | "cosh" | "tanh"
            | "sech" | "csch" | "coth" | "gamma" | "fact",
            1,
        ) => {
            if known_real(&xs[0], env) {
                7
            } else {
                15
            }
        }
        _ => return None,
    };
    Some((sign, origin.cloned()))
}

/// The signs of Calc's constants against an origin; any other variable
/// may be anything.
fn const_signs(v: &str, origin: Option<&Num>) -> u8 {
    let value = match v {
        "pi" => std::f64::consts::PI,
        "e" => std::f64::consts::E,
        "inf" => return POS,
        "uinf" => return 13,
        "i" => return NONREAL,
        _ => return 15,
    };
    let o = origin.map_or(0.0, Num::to_f64);
    if value > o {
        POS
    } else if value < o {
        NEG
    } else {
        ZERO
    }
}

fn sign_of(n: &Num) -> u8 {
    if n.is_negative() {
        NEG
    } else if n.is_zero() {
        ZERO
    } else {
        POS
    }
}

/// `math-neg-signs`.
fn neg_signs(s: u8) -> u8 {
    if s >= NONREAL {
        return NONREAL + neg_signs(s - NONREAL);
    }
    (if s & NEG != 0 { POS } else { 0 }) | (s & ZERO) | (if s & POS != 0 { NEG } else { 0 })
}

/// `math-known-realp`.
fn known_real(a: &Expr, env: &Env) -> bool {
    signs(a, None, env) < NONREAL
}

/// `math-known-nonnegp`.
fn known_nonneg(a: &Expr, env: &Env) -> bool {
    matches!(signs(a, None, env), 2 | 4 | 6)
}

/// `math-possible-types`' bits: may be an integer, a float with an integer
/// value, a fraction, another float, imaginary, otherwise not real. Sums,
/// differences, products and quotients by a stack of their own: a long
/// sum or product nests a level a term.
fn types(a: &Expr, num: bool, env: &Env) -> u8 {
    enum Task<'a> {
        Visit(&'a Expr),
        Combine(&'a str),
    }
    let mut tasks = vec![Task::Visit(a)];
    let mut found: Vec<u8> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(Expr::Call(f, xs))
                if xs.len() == 2 && matches!(f.as_str(), "+" | "-" | "*" | "%" | "/") =>
            {
                tasks.push(Task::Combine(f));
                tasks.push(Task::Visit(&xs[1]));
                tasks.push(Task::Visit(&xs[0]));
            }
            Task::Visit(e) => found.push(types_at(e, num, env)),
            Task::Combine(f) => {
                let t2 = found.pop().expect("a type");
                let t1 = found.pop().expect("a type");
                found.push(combine_types(f, t1, t2, num, env));
            }
        }
    }
    found.pop().expect("a type")
}

/// [`types`] of anything but a sum, a difference, a product or a quotient.
fn types_at(a: &Expr, num: bool, env: &Env) -> u8 {
    match a {
        Expr::Num(Num::Int(_)) => {
            if num {
                3
            } else {
                1
            }
        }
        Expr::Num(n @ Num::Float(..)) if n.is_messy_integer() => {
            if num {
                3
            } else {
                2
            }
        }
        Expr::Num(Num::Frac(..)) => {
            if num {
                12
            } else {
                4
            }
        }
        Expr::Num(Num::Float(..)) => {
            if num {
                12
            } else {
                8
            }
        }
        Expr::Intv(_, lo, hi) => {
            if lo == hi {
                types(lo, false, env)
            } else {
                15
            }
        }
        _ if cplx::parts(a).is_some() => {
            if cplx::parts(a).is_some_and(|(re, _)| re.is_zero()) {
                16
            } else {
                32
            }
        }
        Expr::Vec(items) => {
            let mut t = 0;
            for x in items {
                if t >= 63 {
                    break;
                }
                t |= types(x, true, env);
            }
            t
        }
        Expr::Var(v) => match v.as_str() {
            "e" | "pi" | "phi" | "gamma" => 8,
            "inf" => 1,
            "i" => 16,
            _ => 63,
        },
        Expr::Call(f, xs) => call_types(f, xs, num, env),
        _ => 63,
    }
}

/// The type of `t1 f t2`, `f` an arithmetic operator.
fn combine_types(f: &str, mut t1: u8, mut t2: u8, num: bool, env: &Env) -> u8 {
    if f == "/" {
        let t12 = t1 | t2;
        return if t12 < 16 {
            if t12 & 10 > 0 {
                10
            } else if t1 == 4 || t2 == 4 || env.prec.prefer_frac {
                5
            } else {
                15
            }
        } else if t12 < 32 {
            mixed_product(t1, t2)
        } else {
            63
        };
    }
    if f == "%" {
        t1 &= 15;
        t2 &= 15;
    }
    let t12 = t1 | t2;
    if t12 < 16 {
        let mask = if t12 <= 3 {
            1
        } else if ((t1 <= 3 && t2 & 3 == 0) || (t2 <= 3 && t1 & 3 == 0)) && (f == "+" || f == "-") {
            4
        } else {
            5
        };
        if num {
            mask * 3
        } else {
            (if t1 & 5 > 0 && t2 & 5 > 0 { mask } else { 0 })
                | (if t12 & 10 > 0 { mask * 2 } else { 0 })
        }
    } else if t12 < 32 {
        if f == "*" {
            mixed_product(t1, t2)
        } else if t12 == 16 {
            16
        } else if (t1 == 16 && t2 < 16) || (t2 == 16 && t1 < 16) {
            32
        } else {
            63
        }
    } else {
        63
    }
}

fn call_types(f: &str, xs: &[Expr], num: bool, env: &Env) -> u8 {
    let both = |num| (types(&xs[0], num, env), types(&xs[1], num, env));
    match (f, xs.len()) {
        ("neg", 1) => types(&xs[0], false, env),
        ("^", 2) => {
            let (t1, t2) = both(num);
            let t12 = t1 | t2;
            if t2 <= 3 && known_nonneg(&xs[1], env) && t1 < 16 {
                let mask =
                    (if t1 & 3 > 0 { 1 } else { 0 }) | (t1 & 4) | (if t1 & 12 > 0 { 5 } else { 0 });
                if num {
                    mask * 3
                } else {
                    (if t1 & 5 > 0 && t2 & 5 > 0 { mask } else { 0 })
                        | (if t12 & 10 > 0 { mask * 2 } else { 0 })
                }
            } else if known_nonneg(&xs[0], env) && signs(&xs[1], None, env) == POS {
                15
            } else {
                63
            }
        }
        ("sqrt", 1) => {
            let s = signs(&xs[0], None, env);
            (if s & ZERO > 0 { 3 } else { 0 })
                | (if s & NEG > 0 { 16 } else { 0 })
                | (if s & POS > 0 { 15 } else { 0 })
                | (if s & NONREAL > 0 { 32 } else { 0 })
        }
        ("idiv" | "isqrt" | "ilog" | "vlen" | "vcount", _) => 1,
        ("floor" | "ceil" | "round" | "trunc" | "rounde" | "roundu", _)
            if xs.get(1).is_none_or(|d| known_nonneg(d, env)) =>
        {
            1
        }
        ("ffloor" | "fceil" | "fround" | "ftrunc" | "frounde" | "froundu", _)
            if xs.get(1).is_none_or(|d| known_nonneg(d, env)) =>
        {
            2
        }
        ("frac", _) => 5,
        ("float", 1) => {
            let t1 = types(&xs[0], false, env);
            (if t1 & 3 > 0 { 2 } else { 0 }) | (if t1 & 12 > 0 { 8 } else { 0 }) | (t1 & 48)
        }
        ("abs" | "abssqr", 1) => {
            let t1 = types(&xs[0], false, env);
            if t1 >= 16 { 15 } else { t1 }
        }
        _ => 63,
    }
}

/// The type of a product or quotient of which one side may be imaginary.
fn mixed_product(t1: u8, t2: u8) -> u8 {
    match (t1, t2) {
        (16, 16) => 15,
        (16, t) | (t, 16) if t < 16 => 16,
        _ => 31,
    }
}

/// `math-known-integerp`.
fn known_integer(a: &Expr, env: &Env) -> bool {
    types(a, false, env) == 1
}

/// `math-known-num-integerp`.
fn known_num_integer(a: &Expr, env: &Env) -> bool {
    types(a, true, env) <= 3
}

/// What `math-known-evenp` and `math-known-oddp` can tell: `Some(true)`
/// for even, `Some(false)` for odd. A sum is even when it has an even
/// number of odd terms (all of them known), a product when a factor is
/// even and those before it integers (it is never known odd); a long sum
/// or product in a loop.
fn parity(a: &Expr, env: &Env) -> Option<bool> {
    let mut a = a;
    let mut odd_terms = false;
    while let Expr::Call(f, xs) = a
        && (f == "+" || f == "-")
        && xs.len() == 2
    {
        odd_terms ^= !parity(&xs[1], env)?;
        a = &xs[0];
    }
    let even = match a {
        Expr::Num(Num::Int(n)) => n.is_even(),
        Expr::Num(n @ Num::Float(_, e)) if n.is_messy_integer() => {
            *e > 0 || matches!(num::trunc(n), Num::Int(t) if t.is_even())
        }
        Expr::Call(f, xs) if f == "neg" && xs.len() == 1 => parity(&xs[0], env)?,
        Expr::Call(f, xs) if f == "*" && xs.len() == 2 => {
            let mut a = a;
            loop {
                match a {
                    Expr::Call(g, ys) if g == "*" && ys.len() == 2 => {
                        if parity(&ys[0], env) == Some(true) {
                            if known_num_integer(&ys[1], env) {
                                break true;
                            }
                            return None;
                        }
                        if !known_num_integer(&ys[0], env) {
                            return None;
                        }
                        a = &ys[1];
                    }
                    _ if parity(a, env) == Some(true) => break true,
                    _ => return None,
                }
            }
        }
        _ => return None,
    };
    Some(even ^ odd_terms)
}

/// `math-quarter-integer`: 1, 2 or 3 for a number a quarter, a half or
/// three quarters past an integer, 0 for an integer.
fn quarter_integer(x: &Num) -> Option<u8> {
    match x {
        Num::Int(_) => Some(0),
        _ if x.is_negative() => quarter_integer(&x.neg()).map(|q| 4 - q),
        Num::Frac(n, d) => {
            if *d == 2.into() {
                Some(2)
            } else if *d == 4.into() {
                n.mod_floor(&4.into()).try_into().ok()
            } else {
                None
            }
        }
        Num::Float(_, e) if *e >= 0 => Some(0),
        Num::Float(m, -1) => (m.mod_floor(&10.into()) == 5.into()).then_some(2),
        Num::Float(m, -2) => match u8::try_from(m.mod_floor(&100.into())) {
            Ok(25) => Some(1),
            Ok(75) => Some(3),
            _ => None,
        },
        Num::Float(..) => None,
    }
}

/// `math-looks-negp`, the function: a difference whose first term looks
/// negative too.
fn looks_negative(e: &Expr) -> bool {
    match e {
        Expr::Call(f, xs) if f == "-" && xs.len() == 2 => looks_negative(&xs[0]),
        Expr::Call(f, xs) if (f == "*" || f == "/") && xs.len() == 2 => {
            looks_negative(&xs[0]) || looks_negative(&xs[1])
        }
        _ => looks_neg(e),
    }
}

/// Whether `math-pow-fancy` takes `a^b` as a formula: `a` is not a
/// number, a vector or another of Calc's objects, or `b` is not an object.
pub(crate) fn formula_power(a: &Expr, b: &Expr) -> bool {
    let object = |e: &Expr| {
        matches!(e, Expr::Num(_) | Expr::Date(_) | Expr::Intv(..))
            || cplx::parts(e).is_some()
            || mod_form(e).is_some()
    };
    !(object(a) || matches!(a, Expr::Vec(_) | Expr::Str(_))) || !object(b)
}

/// The rules of `math-pow-fancy` for a formula to a power, in its order:
/// a product or a quotient to an integer power (or of a factor known not
/// to be negative) as the power of each, a power or a square root to an
/// integer power as one power, an even power's power of something real
/// to a quarter power as its absolute value, a base that looks negative
/// to an integer power as its negation's (negated for an odd power), the
/// absolute value of something real to an even power as its own. `None`
/// when no rule applies, or when a step was refused (Calc then leaves the
/// whole power as it was).
pub(crate) fn pow_fancy(a: &Expr, b: &Expr, env: &Env) -> Option<Expr> {
    if !formula_power(a, b) {
        return None;
    }
    let done = |r: Expr| (!refused()).then_some(r);
    let pow = |x: &Expr, y: &Expr| done(eval::pow(x, y, env));
    let integer_power = known_num_integer(b, env);
    if let Expr::Call(f, xs) = a
        && xs.len() == 2
    {
        let (x, y) = (&xs[0], &xs[1]);
        match f.as_str() {
            "*" if integer_power || known_nonneg(x, env) || known_nonneg(y, env) => {
                // Down a product nested to the right (`vprod`'s) as a loop:
                // Calc's recursion went a level deeper for each factor.
                let mut factors = vec![x];
                let mut rest = y;
                while let Expr::Call(g, ys) = rest
                    && g == "*"
                    && ys.len() == 2
                    && (integer_power || known_nonneg(&ys[0], env) || known_nonneg(&ys[1], env))
                {
                    factors.push(&ys[0]);
                    rest = &ys[1];
                }
                let powers = factors
                    .into_iter()
                    .map(|f| pow(f, b))
                    .collect::<Option<Vec<_>>>()?;
                let mut acc = pow(rest, b)?;
                for p in powers.into_iter().rev() {
                    acc = done(mul(&p, &acc, env))?;
                }
                return Some(acc);
            }
            "/" if integer_power || known_nonneg(y, env) => {
                let (p, q) = (pow(x, b)?, pow(y, b)?);
                return done(div(&p, &q, env));
            }
            "/" if known_nonneg(x, env) && !is_one(x) => {
                let inverse = done(div(&Expr::int(1), y, env))?;
                let (p, q) = (pow(x, b)?, pow(&inverse, b)?);
                return done(mul(&p, &q, env));
            }
            "^" if integer_power || known_nonneg(x, env) => {
                let m = done(mul(y, b, env))?;
                return pow(x, &m);
            }
            _ => {}
        }
    }
    if let Expr::Call(f, xs) = a
        && f == "sqrt"
        && xs.len() == 1
        && (integer_power || known_nonneg(&xs[0], env))
    {
        let half = done(div(b, &Expr::int(2), env))?;
        return pow(&xs[0], &half);
    }
    if let Expr::Call(f, xs) = a
        && f == "^"
        && xs.len() == 2
        && parity(&xs[1], env) == Some(true)
        && num_of(b)
            .and_then(quarter_integer)
            .is_some_and(|q| (1..=3).contains(&q))
        && known_real(&xs[0], env)
    {
        let m = done(mul(&xs[1], b, env))?;
        let p = pow(&xs[0], &m)?;
        return done(eval::apply("abs", vec![p], env));
    }
    if looks_negative(a)
        && known_integer(b, env)
        && let Some(even) = parity(b, env)
    {
        // A negation that still looks negative would go round for ever
        // (Calc's own negation leaves none).
        let n = neg(a, env);
        if !looks_negative(&n) {
            let p = pow(&n, b)?;
            return done(if even { p } else { neg(&p, env) });
        }
    }
    if let Expr::Call(f, xs) = a
        && f == "abs"
        && xs.len() == 1
        && known_real(&xs[0], env)
        && parity(b, env) == Some(true)
    {
        return pow(&xs[0], b);
    }
    None
}

/// `math-pow-of-zero` for a power not a number: zero to a power known
/// positive is zero, to an unsigned infinity `nan`; to a zero (an
/// indeterminate form) or a power known negative (a division by zero) it
/// is refused. `None` otherwise.
pub(crate) fn pow_of_zero(a: &Expr, b: &Expr, env: &Env) -> Option<Expr> {
    if !is_zero(a) || matches!(b, Expr::Num(_)) {
        return None;
    }
    if mod_form(b).is_some_and(|(k, _)| k.is_zero()) {
        refuse();
        return None;
    }
    match signs(b, None, env) {
        POS => Some(a.clone()),
        NEG => {
            refuse();
            None
        }
        _ if infinity(b).is_some() => Some(nan()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_of_formulas() {
        let env = Env::default();
        let s = |f: &str| {
            let e = &super::super::parse::parse(f, &env.prec).unwrap()[0];
            signs(&eval::normalize(e, &env), None, &env)
        };
        assert_eq!(s("3"), POS);
        assert_eq!(s("x"), 15);
        assert_eq!(s("abs(x)"), 6);
        assert_eq!(s("abs(x) + 1"), POS);
        assert_eq!(s("-abs(x) - 1"), NEG);
        assert_eq!(s("floor(x)"), 7);
        assert_eq!(s("floor(x)^2"), 6);
        assert_eq!(s("sqrt(abs(x))"), 6);
        assert_eq!(s("exp(floor(x))"), POS);
        assert_eq!(s("pi - 3"), POS);
    }

    #[test]
    fn quarter_integers() {
        let q = |n: Num| quarter_integer(&n);
        assert_eq!(q(Num::int(3)), Some(0));
        assert_eq!(q(num::make_frac(1.into(), 2.into())), Some(2));
        assert_eq!(q(num::make_frac((-1).into(), 4.into())), Some(3));
        assert_eq!(q(Num::Float(25.into(), -1)), Some(2));
        assert_eq!(q(Num::Float(225.into(), -2)), Some(1));
        assert_eq!(q(Num::Float(1.into(), -1)), None);
    }
}
