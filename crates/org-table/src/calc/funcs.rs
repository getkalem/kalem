//! Calc's functions beyond arithmetic that tables use: number theory
//! (`prime`, `totient`…), the parts of a float (`mant`, `xpon`), `frac`,
//! the bitwise functions on 32-bit words, and the vector functions
//! (`index`, `sort`, `vunion`, `arrange`…), each as `calc-eval` gives it
//! with Org's Calc modes (`org-table/tests/calc-functions.txt`).

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::algebra::{Cmp, add, compare, mul, sub};
use super::eval::Env;
use super::expr::Expr;
use super::num::{self, Num};

/// The functions of this module; `None` leaves the call as it is.
pub(crate) fn apply(f: &str, args: &[Expr], env: &Env) -> Option<Expr> {
    let n = args.len();
    match (f, n) {
        ("prime", 1) => {
            let k = whole(&args[0])?;
            Some(Expr::int(i64::from(is_prime(&k))))
        }
        ("nextprime", 1) => {
            let mut k = floor(&args[0])?.max(BigInt::one());
            loop {
                k += 1;
                if is_prime(&k) {
                    return Some(Expr::Num(Num::Int(k)));
                }
            }
        }
        ("prevprime", 1) => {
            let mut k = ceil(&args[0])?;
            loop {
                k -= 1;
                if k <= BigInt::from(2) {
                    return Some(Expr::int(2));
                }
                if is_prime(&k) {
                    return Some(Expr::Num(Num::Int(k)));
                }
            }
        }
        ("totient", 1) => {
            let k = whole(&args[0])?;
            if k.is_negative() {
                return None;
            }
            Some(Expr::Num(Num::Int(totient(k))))
        }
        ("isqrt", 1) => {
            let k = floor(&args[0])?;
            (!k.is_negative()).then(|| Expr::Num(Num::Int(k.sqrt())))
        }
        ("ilog", 2) => ilog(&args[0], &args[1]),
        ("perm", 2) => perm(&args[0], &args[1], env),
        ("mant", 1) | ("xpon", 1) => match &args[0] {
            Expr::Num(Num::Float(m, e)) if !m.is_zero() => {
                let d = num::numdigs(m);
                Some(if f == "mant" {
                    Expr::Num(Num::Float(m.clone(), -(d - 1)))
                } else {
                    Expr::int(e + d - 1)
                })
            }
            Expr::Num(a) => Some(if f == "mant" {
                Expr::Num(a.clone())
            } else {
                Expr::int(0)
            }),
            _ => None,
        },
        ("frac", 1) | ("frac", 2) => frac(&args[0], args.get(1), env),
        // The bitwise functions, an optional last argument being the word
        // size (32 by default).
        ("and", 2 | 3) | ("or", 2 | 3) | ("xor", 2 | 3) | ("diff", 2 | 3) => {
            let w = word_size(args.get(2))?;
            let (a, b) = (word(&args[0], w)?, word(&args[1], w)?);
            let r = match f {
                "and" => a & b,
                "or" => a | b,
                "xor" => a ^ b,
                _ => a & !b,
            };
            Some(Expr::Num(Num::Int(BigInt::from(r))))
        }
        ("not", 1 | 2) => {
            let w = word_size(args.get(1))?;
            let a = word(&args[0], w)?;
            Some(Expr::Num(Num::Int(BigInt::from(!a & mask(w)))))
        }
        ("lsh", 1..=3) | ("rsh", 1..=3) => {
            let w = word_size(args.get(2))?;
            let a = word(&args[0], w)?;
            let k = match args.get(1) {
                Some(e) => whole(e)?.to_i64()?,
                None => 1,
            };
            let k = if f == "rsh" { -k } else { k };
            let r = match k {
                k if k >= 64 || k <= -64 => 0,
                k if k >= 0 => (a << k) & mask(w),
                k => a >> -k,
            };
            Some(Expr::Num(Num::Int(BigInt::from(r))))
        }
        ("vec", _) => Some(Expr::Vec(args.to_vec())),
        ("cvec", 2) | ("cvec", 3) => {
            let mut v = args[0].clone();
            for d in args[1..].iter().rev() {
                let k = whole(d)?.to_usize()?;
                v = Expr::Vec(vec![v; k]);
            }
            Some(v)
        }
        ("index", 1..=3) => {
            let k = whole(&args[0])?.to_usize()?;
            let start = args.get(1).cloned().unwrap_or(Expr::int(1));
            let step = args.get(2).cloned().unwrap_or(Expr::int(1));
            let items = (0..k)
                .map(|i| add(&start, &mul(&Expr::int(i as i64), &step, env), env))
                .collect();
            Some(Expr::Vec(items))
        }
        ("rev", 1) => {
            let mut v = items(&args[0])?.to_vec();
            v.reverse();
            Some(Expr::Vec(v))
        }
        ("sort", 1) | ("rsort", 1) => {
            let mut v = items(&args[0])?.to_vec();
            v.sort_by(|a, b| before(a, b, env));
            if f == "rsort" {
                v.reverse();
            }
            Some(Expr::Vec(v))
        }
        ("grade", 1) | ("rgrade", 1) => {
            let v = items(&args[0])?;
            let mut idx: Vec<usize> = (0..v.len()).collect();
            idx.sort_by(|&i, &j| before(&v[i], &v[j], env));
            if f == "rgrade" {
                idx.reverse();
            }
            Some(Expr::Vec(
                idx.into_iter().map(|i| Expr::int(i as i64 + 1)).collect(),
            ))
        }
        ("rdup", 1) => Some(Expr::Vec(set(items(&args[0])?.to_vec(), env))),
        ("vunion", 2) | ("vint", 2) | ("vdiff", 2) | ("vxor", 2) => {
            let (a, b) = (set(members(&args[0]), env), set(members(&args[1]), env));
            let has = |s: &[Expr], x: &Expr| s.iter().any(|y| same(x, y, env));
            let out: Vec<Expr> = match f {
                "vunion" => a.iter().chain(&b).cloned().collect(),
                "vint" => a.iter().filter(|x| has(&b, x)).cloned().collect(),
                "vdiff" => a.iter().filter(|x| !has(&b, x)).cloned().collect(),
                _ => a
                    .iter()
                    .filter(|x| !has(&b, x))
                    .chain(b.iter().filter(|x| !has(&a, x)))
                    .cloned()
                    .collect(),
            };
            Some(Expr::Vec(set(out, env)))
        }
        ("head", 1) => items(&args[0])?.first().cloned(),
        ("rtail", 1) => items(&args[0])?.last().cloned(),
        ("tail", 1) => {
            let v = items(&args[0])?;
            (!v.is_empty()).then(|| Expr::Vec(v[1..].to_vec()))
        }
        ("rhead", 1) => {
            let v = items(&args[0])?;
            (!v.is_empty()).then(|| Expr::Vec(v[..v.len() - 1].to_vec()))
        }
        ("cons", 2) => {
            let mut v = vec![args[0].clone()];
            v.extend(items(&args[1])?.iter().cloned());
            Some(Expr::Vec(v))
        }
        ("rcons", 2) => {
            let mut v = items(&args[0])?.to_vec();
            v.push(args[1].clone());
            Some(Expr::Vec(v))
        }
        ("vconcat", 2) => Some(Expr::Vec(
            members(&args[0])
                .into_iter()
                .chain(members(&args[1]))
                .collect(),
        )),
        ("find", 2) => {
            let v = items(&args[0])?;
            let i = v.iter().position(|x| same(x, &args[1], env));
            Some(Expr::int(i.map_or(0, |i| i as i64 + 1)))
        }
        ("vmask", 2) => {
            let (m, v) = (items(&args[0])?, items(&args[1])?);
            if m.len() != v.len() {
                return None;
            }
            let keep = |x: &Expr| matches!(x, Expr::Num(k) if !k.is_zero());
            Some(Expr::Vec(
                m.iter()
                    .zip(v)
                    .filter(|(k, _)| keep(k))
                    .map(|(_, x)| x.clone())
                    .collect(),
            ))
        }
        ("subvec", 2) | ("subvec", 3) => {
            let v = items(&args[0])?;
            let len = v.len() as i64;
            let from = whole(&args[1])?.to_i64()?;
            let to = match args.get(2) {
                Some(e) => whole(e)?.to_i64()?,
                None => len + 1,
            };
            if from < 1 || to < from || to > len + 1 {
                return None;
            }
            Some(Expr::Vec(v[from as usize - 1..to as usize - 1].to_vec()))
        }
        ("mrow", 2) => {
            let v = items(&args[0])?;
            let i = whole(&args[1])?.to_usize()?;
            v.get(i.checked_sub(1)?).cloned()
        }
        ("mcol", 2) => {
            let rows = matrix(&args[0])?;
            let j = whole(&args[1])?.to_usize()?.checked_sub(1)?;
            let col = rows
                .iter()
                .map(|r| r.get(j).cloned())
                .collect::<Option<_>>()?;
            Some(Expr::Vec(col))
        }
        ("trn", 1) => {
            let rows = matrix(&args[0])?;
            let w = rows.first()?.len();
            let cols = (0..w)
                .map(|j| Expr::Vec(rows.iter().map(|r| r[j].clone()).collect()))
                .collect();
            Some(Expr::Vec(cols))
        }
        ("histogram", 2) => {
            let v = items(&args[0])?;
            let bins = whole(&args[1])?.to_usize()?;
            let mut counts = vec![0i64; bins];
            for x in v {
                if let Some(k) = floor(x).and_then(|k| k.to_usize())
                    && k < bins
                {
                    counts[k] += 1;
                }
            }
            Some(Expr::Vec(counts.into_iter().map(Expr::int).collect()))
        }
        ("arrange", 2) => {
            let flat = flatten(items(&args[0])?);
            let k = whole(&args[1])?.to_usize()?;
            if k == 0 {
                return Some(Expr::Vec(flat));
            }
            Some(Expr::Vec(
                flat.chunks(k).map(|c| Expr::Vec(c.to_vec())).collect(),
            ))
        }
        ("string", 1) => {
            let s = items(&args[0])?
                .iter()
                .map(|c| whole(c)?.to_u32().and_then(char::from_u32))
                .collect::<Option<String>>()?;
            Some(Expr::Var(s))
        }
        _ => None,
    }
}

/// An integer, or a float with an integer value.
fn whole(e: &Expr) -> Option<BigInt> {
    match e {
        Expr::Num(Num::Int(k)) => Some(k.clone()),
        Expr::Num(a @ Num::Float(..)) if a.is_messy_integer() => int_of(&num::trunc(a)),
        _ => None,
    }
}

fn int_of(a: &Num) -> Option<BigInt> {
    match a {
        Num::Int(k) => Some(k.clone()),
        _ => None,
    }
}

fn floor(e: &Expr) -> Option<BigInt> {
    match e {
        Expr::Num(a) => int_of(&num::floor(a)),
        _ => None,
    }
}

fn ceil(e: &Expr) -> Option<BigInt> {
    match e {
        Expr::Num(a) => int_of(&num::ceil(a)),
        _ => None,
    }
}

/// The word size of a bitwise function: its argument, or
/// `calc-word-size`, 32; up to 63 bits here.
fn word_size(e: Option<&Expr>) -> Option<u32> {
    match e {
        None => Some(32),
        Some(e) => whole(e)?.to_u32().filter(|w| (1..64).contains(w)),
    }
}

fn mask(w: u32) -> u64 {
    (1u64 << w) - 1
}

/// An integer clipped to a word of `w` bits, as Calc clips the arguments
/// of its bitwise functions.
fn word(e: &Expr, w: u32) -> Option<u64> {
    let k = whole(e)?;
    k.mod_floor(&(BigInt::one() << w)).to_u64()
}

/// Primality: trial division for small numbers, Miller–Rabin with the
/// bases that are exact below 3.3 × 10^24 for larger ones.
fn is_prime(k: &BigInt) -> bool {
    if *k < BigInt::from(2) {
        return false;
    }
    for p in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41] {
        let p = BigInt::from(p);
        if *k == p {
            return true;
        }
        if (k % &p).is_zero() {
            return false;
        }
    }
    let one = BigInt::one();
    let m = k - &one;
    let s = m.trailing_zeros().unwrap_or(0);
    let d = &m >> s;
    'base: for a in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41] {
        let mut x = BigInt::from(a).modpow(&d, k);
        if x == one || x == m {
            continue;
        }
        for _ in 1..s {
            x = x.modpow(&BigInt::from(2), k);
            if x == m {
                continue 'base;
            }
        }
        return false;
    }
    true
}

/// Euler's totient, by trial division.
fn totient(mut k: BigInt) -> BigInt {
    if k.is_zero() {
        return k;
    }
    let mut r = k.clone();
    let mut p = BigInt::from(2);
    while &p * &p <= k {
        if (&k % &p).is_zero() {
            while (&k % &p).is_zero() {
                k /= &p;
            }
            r -= &r / &p;
        }
        p += 1;
    }
    if k > BigInt::one() {
        r -= &r / &k;
    }
    r
}

/// `calcFunc-ilog`: the integer part of the logarithm, exact for
/// integers.
fn ilog(x: &Expr, b: &Expr) -> Option<Expr> {
    let (Expr::Num(xv), Some(bv)) = (x, whole(b)) else {
        return None;
    };
    if xv.is_negative() || xv.is_zero() || bv <= BigInt::one() {
        return None;
    }
    if let Some(xi) = whole(x) {
        let (mut k, mut p) = (-1i64, BigInt::one());
        while p <= xi {
            p *= &bv;
            k += 1;
        }
        return Some(Expr::int(k));
    }
    let r = (xv.to_f64().ln() / bv.to_f64()?.ln()).floor();
    r.is_finite().then(|| Expr::int(r as i64))
}

/// `calcFunc-perm`: `n (n - 1) … (n - k + 1)`.
fn perm(n: &Expr, k: &Expr, env: &Env) -> Option<Expr> {
    let k = whole(k)?.to_i64()?;
    if !(0..=100_000).contains(&k) {
        return None;
    }
    if let Expr::Num(Num::Int(nn)) = n
        && BigInt::from(k) > *nn
    {
        return None;
    }
    let Expr::Num(_) = n else {
        return None;
    };
    let mut r = Expr::int(1);
    for i in 0..k {
        r = mul(&r, &sub(n, &Expr::int(i), env), env);
    }
    Some(match (n, r) {
        (Expr::Num(Num::Float(..)), Expr::Num(v)) => Expr::Num(v.to_float(&env.prec)),
        (_, r) => r,
    })
}

/// `calcFunc-frac`: a float as a fraction, within a tolerance (`tol`
/// digits when an integer, the default being the working precision) by
/// its continued fraction.
fn frac(a: &Expr, tol: Option<&Expr>, env: &Env) -> Option<Expr> {
    let Expr::Num(x) = a else {
        return None;
    };
    let Num::Float(m, e) = x else {
        return Some(a.clone());
    };
    if x.is_messy_integer() {
        return Some(Expr::Num(num::trunc(x)));
    }
    if x.is_negative() {
        let r = frac(&Expr::Num(x.neg()), tol, env)?;
        return Some(super::algebra::neg(&r, env));
    }
    // The tolerance as a rational `tn / td`.
    let (tn, td) = match tol.map(|t| match t {
        Expr::Num(n) => Some(n.abs()),
        _ => None,
    }) {
        Some(None) => return None,
        None | Some(Some(Num::Int(_))) => {
            let digits = match tol {
                Some(Expr::Num(Num::Int(k))) if k.is_positive() => k.to_i64()?,
                Some(Expr::Num(Num::Int(k))) => k.to_i64()? + env.prec.digits,
                _ => env.prec.digits,
            };
            // `5e(numdigs + exp - (tol + 1))`.
            pow10_frac(BigInt::from(5), num::numdigs(m) + e - (digits + 1))
        }
        Some(Some(Num::Float(tm, te))) => pow10_frac(tm, te),
        Some(Some(t)) => num::rational(&t),
    };
    if tn.is_zero() {
        return frac(a, None, env);
    }
    if tn >= td {
        return Some(Expr::Num(num::trunc(x)));
    }
    // The float exactly, as `xn / xd`.
    let (xn, xd) = pow10_frac(m.clone(), *e);
    // Convergents `p / q` of the continued fraction of `xn / xd`, until
    // `|x - p/q| < tol`.
    let (mut p0, mut q0, mut p1, mut q1) =
        (BigInt::zero(), BigInt::one(), BigInt::one(), BigInt::zero());
    let (mut num_, mut den) = (xn.clone(), xd.clone());
    loop {
        let (t, r) = num_.div_rem(&den);
        let p2 = &t * &p1 + &p0;
        let q2 = &t * &q1 + &q0;
        (p0, q0, p1, q1) = (p1, q1, p2, q2);
        // |xn/xd - p1/q1| < tn/td  ⇔  |xn q1 - p1 xd| td < tn xd q1.
        let err = (&xn * &q1 - &p1 * &xd).abs() * &td;
        if r.is_zero() || err < &tn * &xd * &q1 {
            return Some(Expr::Num(num::make_frac(p1, q1)));
        }
        (num_, den) = (den, r);
    }
}

/// `m × 10^e` as a fraction.
fn pow10_frac(m: BigInt, e: i64) -> (BigInt, BigInt) {
    let p = BigInt::from(10).pow(e.unsigned_abs() as u32);
    if e >= 0 {
        (m * p, BigInt::one())
    } else {
        (m, p)
    }
}

/// The elements of a vector.
fn items(e: &Expr) -> Option<&[Expr]> {
    match e {
        Expr::Vec(v) => Some(v),
        _ => None,
    }
}

/// A set's members: a vector's elements, or a lone value.
fn members(e: &Expr) -> Vec<Expr> {
    match e {
        Expr::Vec(v) => v.clone(),
        x => vec![x.clone()],
    }
}

/// The rows of a matrix, all of the same length.
fn matrix(e: &Expr) -> Option<Vec<&[Expr]>> {
    let rows = items(e)?.iter().map(items).collect::<Option<Vec<_>>>()?;
    let w = rows.first()?.len();
    rows.iter().all(|r| r.len() == w).then_some(rows)
}

/// All the elements of nested vectors, in order.
fn flatten(v: &[Expr]) -> Vec<Expr> {
    let mut out = Vec::new();
    for x in v {
        match x {
            Expr::Vec(w) => out.extend(flatten(w)),
            x => out.push(x.clone()),
        }
    }
    out
}

/// `math-beforep`: numbers by value first, then the rest by their
/// written form.
fn before(a: &Expr, b: &Expr, env: &Env) -> Ordering {
    match (a, b) {
        (Expr::Num(_), Expr::Num(_)) => match compare(a, b, env) {
            Cmp::Less => Ordering::Less,
            Cmp::Greater => Ordering::Greater,
            _ => Ordering::Equal,
        },
        (Expr::Num(_), _) => Ordering::Less,
        (_, Expr::Num(_)) => Ordering::Greater,
        _ => a.to_lisp().cmp(&b.to_lisp()),
    }
}

fn same(a: &Expr, b: &Expr, env: &Env) -> bool {
    a == b || matches!(compare(a, b, env), Cmp::Equal)
}

/// Sorted, without duplicates, as Calc keeps its sets.
fn set(mut v: Vec<Expr>, env: &Env) -> Vec<Expr> {
    v.sort_by(|a, b| before(a, b, env));
    v.dedup_by(|a, b| same(a, b, env));
    v
}
