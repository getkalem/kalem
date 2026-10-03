//! Calc's functions beyond arithmetic that tables use: number theory
//! (`prime`, `totient`…), the parts of a float (`mant`, `xpon`), `frac`,
//! the bitwise functions on 32-bit words, and the vector functions
//! (`index`, `sort`, `vunion`, `arrange`…), each as `calc-eval` gives it
//! with Org's Calc modes (`org-table/tests/calc-functions.txt`).

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::algebra::{self, Cmp, add, compare, mul, sub};
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
        // size (32 by default; 0 unbounded, negative for signed words).
        ("and", 2 | 3) | ("or", 2 | 3) | ("xor", 2 | 3) | ("diff", 2 | 3) => {
            let w = word_size(args.get(2))?;
            let (a, b) = (whole(&args[0])?, whole(&args[1])?);
            let r = match f {
                "and" => a & b,
                "or" => a | b,
                "xor" => a ^ b,
                _ => a & !b,
            };
            Some(int(clip(r, w)))
        }
        ("not", 1 | 2) => {
            let w = word_size(args.get(1))?;
            Some(int(not(whole(&args[0])?, w)))
        }
        ("clip", 1 | 2) => {
            let w = word_size(args.get(1))?;
            Some(int(clip(whole(&args[0])?, w)))
        }
        ("lsh", 1..=3) | ("rsh", 1..=3) | ("ash", 1..=3) | ("rash", 1..=3) | ("rot", 1..=3) => {
            let w = word_size(args.get(2))?;
            let a = whole(&args[0])?;
            let mut n = match args.get(1) {
                Some(e) => whole(e)?.to_i64()?,
                None => 1,
            };
            if f == "rsh" || f == "rash" {
                n = -n;
            }
            Some(int(match f {
                "lsh" | "rsh" => lsh(a, n, w),
                "ash" | "rash" => ash(a, n, w),
                _ => rot(a, n, w)?,
            }))
        }
        ("random", 1) => random(&args[0], env),
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
        ("rdup", 1) => rdup(&args[0], env),
        ("vunion", 2) => {
            let mut v = match &args[0] {
                Expr::Vec(v) => v.clone(),
                a if objectp(a) => vec![a.clone()],
                _ => return None,
            };
            match &args[1] {
                Expr::Vec(w) => v.extend(w.iter().cloned()),
                b if objectp(b) => v.push(b.clone()),
                _ => return None,
            }
            rdup(&Expr::Vec(v), env)
        }
        ("vint", 2) | ("vdiff", 2) | ("vxor", 2)
            if simple_set(&args[0]) && simple_set(&args[1]) =>
        {
            let (a, b) = (set(members(&args[0]), env), set(members(&args[1]), env));
            let has = |s: &[Expr], x: &Expr| s.iter().any(|y| same(x, y, env));
            let out: Vec<Expr> = match f {
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
        // Sets with intervals, through complements as Calc computes them.
        ("vint", 2) => {
            let u = union(&vcompl(&args[0], env)?, &vcompl(&args[1], env)?, env)?;
            vcompl(&u, env)
        }
        ("vdiff", 2) => {
            let u = union(&vcompl(&args[0], env)?, &args[1], env)?;
            vcompl(&u, env)
        }
        ("vxor", 2) => {
            let (a, b) = (&args[0], &args[1]);
            let (ca, cb) = (vcompl(a, env)?, vcompl(b, env)?);
            let x = vcompl(&union(&ca, b, env)?, env)?;
            let y = vcompl(&union(a, &cb, env)?, env)?;
            union(&x, &y, env)
        }
        ("vcompl", 1) => vcompl(&args[0], env),
        ("vspan", 1) => {
            let v = prepare_set(&args[0], env)?;
            Some(match (v.first(), v.last()) {
                (Some(first), Some(last)) => {
                    let m = (first.0 & 2) | (last.0 & 1);
                    if m == 3 && cmp_code(&first.1, &last.2, env) == 0 {
                        first.1.clone()
                    } else {
                        Expr::Intv(m, Box::new(first.1.clone()), Box::new(last.2.clone()))
                    }
                }
                _ => Expr::Intv(2, Box::new(Expr::int(0)), Box::new(Expr::int(0))),
            })
        }
        ("vfloor", 1) => Some(clean_set(vfloor(&args[0], env)?, false, env)),
        ("vcard", 1) => {
            let mut count = Expr::int(0);
            for (m, a, b) in vfloor(&args[0], env)? {
                if algebra::infinity(&a).is_some() || algebra::infinity(&b).is_some() {
                    let _ = m;
                    return None;
                }
                count = add(&count, &add(&sub(&b, &a, env), &Expr::int(1), env), env);
            }
            Some(count)
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

fn int(k: BigInt) -> Expr {
    Expr::Num(Num::Int(k))
}

/// The word size of a bitwise function: its argument, or
/// `calc-word-size`, 32.
fn word_size(e: Option<&Expr>) -> Option<i64> {
    match e {
        None => Some(32),
        Some(e) => whole(e)?.to_i64().filter(|w| w.abs() <= 1 << 20),
    }
}

fn pow2(n: i64) -> BigInt {
    BigInt::one() << n.max(0) as usize
}

/// `math-clip`: an integer as a word of `w` bits, unsigned for a positive
/// `w`, two's complement for a negative one, itself for 0.
fn clip(a: BigInt, w: i64) -> BigInt {
    match w {
        0 => a,
        w if w < 0 => {
            let a = clip(a, -w);
            if a < pow2(-1 - w) { a } else { a - pow2(-w) }
        }
        w => a.mod_floor(&pow2(w)),
    }
}

/// `calcFunc-not`.
fn not(a: BigInt, w: i64) -> BigInt {
    if w < 0 {
        return clip(not(a, -w), w);
    }
    clip(!clip(a, w), w)
}

/// `calcFunc-lsh`: a left shift by `n` (right when negative), the bits
/// shifted out of the word lost.
fn lsh(a: BigInt, n: i64, w: i64) -> BigInt {
    if w < 0 {
        return clip(lsh(a, n, -w), w);
    }
    let a = if a.is_negative() { clip(a, w) } else { a };
    if w != 0 && (n < -w || n > w) {
        BigInt::zero()
    } else if n < 0 {
        clip(a, w).div_floor(&pow2(-n))
    } else {
        clip(a * pow2(n), w)
    }
}

/// `calcFunc-ash`: as [`lsh`], a right shift copying the sign bit.
fn ash(a: BigInt, n: i64, w: i64) -> BigInt {
    if n >= 0 {
        return lsh(a, n, w);
    }
    if w < 0 {
        return clip(ash(a, n, -w), w);
    }
    let a = if a.is_negative() { clip(a, w) } else { a };
    let sh = lsh(a.clone(), n, w);
    if w == 0 || (&a & pow2(w - 1)).is_zero() {
        sh
    } else if n < 1 - w {
        pow2(w) - 1
    } else {
        lsh(pow2(-n) - 1, w + n, w) + sh
    }
}

/// `calcFunc-rot`: a rotation within the word; none without a size.
fn rot(a: BigInt, n: i64, w: i64) -> Option<BigInt> {
    if w == 0 {
        algebra::fail();
        return None;
    }
    if w < 0 {
        return Some(clip(rot(a, n, -w)?, w));
    }
    let a = if a.is_negative() { clip(a, w) } else { a };
    if n < 0 || n >= w {
        return rot(a, n.rem_euclid(w), w);
    }
    Some(lsh(a.clone(), n - w, w) + lsh(a, n, w))
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
    if beforep(a, b, env) {
        Ordering::Less
    } else if beforep(b, a, env) {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// `math-beforep` for the infinities and intervals of sets.
fn beforep(a: &Expr, b: &Expr, env: &Env) -> bool {
    let (ninf, inf) = (algebra::neg_inf(), algebra::inf());
    let real = |e: &Expr| matches!(e, Expr::Num(_));
    if !(real(a) && real(b)) {
        if *b == ninf {
            return false;
        }
        if *a == ninf {
            return true;
        }
        if *a == inf {
            return false;
        }
        if *b == inf {
            return true;
        }
        match (a, b) {
            (Expr::Num(_), Expr::Intv(_, lo, _)) if const_intv(b) => {
                return beforep(a, lo, env) || cmp_code(a, lo, env) == 0;
            }
            (Expr::Intv(_, lo, _), Expr::Num(_)) if const_intv(a) => return beforep(lo, b, env),
            (Expr::Intv(ma, la, ha), Expr::Intv(mb, lb, hb)) if const_intv(a) && const_intv(b) => {
                return match cmp_code(la, lb, env) {
                    -1 => true,
                    1 => false,
                    _ if ma & 2 != 0 && mb & 2 == 0 => true,
                    _ if ma & 2 == 0 && mb & 2 != 0 => false,
                    _ => match cmp_code(ha, hb, env) {
                        -1 => true,
                        1 => false,
                        _ => ma & 1 == 0 && mb & 1 != 0,
                    },
                };
            }
            _ => {}
        }
    }
    before_plain(a, b, env) == Ordering::Less
}

fn before_plain(a: &Expr, b: &Expr, env: &Env) -> Ordering {
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

/// `Math-objectp`: a number, a date, an interval or a modulo form.
fn objectp(e: &Expr) -> bool {
    matches!(e, Expr::Num(_) | Expr::Date(_) | Expr::Intv(..)) || algebra::mod_form(e).is_some()
}

/// `math-simple-set`: a set without intervals.
fn simple_set(e: &Expr) -> bool {
    match e {
        Expr::Intv(..) => false,
        Expr::Vec(v) => v.iter().all(|x| !matches!(x, Expr::Intv(..))),
        e => objectp(e),
    }
}

/// `calcFunc-rdup`.
fn rdup(a: &Expr, env: &Env) -> Option<Expr> {
    if simple_set(a) {
        return Some(Expr::Vec(set(members(a), env)));
    }
    Some(clean_set(prepare_set(a, env)?, false, env))
}

fn union(a: &Expr, b: &Expr, env: &Env) -> Option<Expr> {
    apply("vunion", &[a.clone(), b.clone()], env)
}

/// An interval of a set: its mask (bit 1 closes the low end, bit 0 the
/// high end) and its ends.
type Span = (u8, Expr, Expr);

fn cmp_code(a: &Expr, b: &Expr, env: &Env) -> i8 {
    match compare(a, b, env) {
        Cmp::Less => -1,
        Cmp::Equal => 0,
        Cmp::Greater => 1,
        Cmp::Unknown => 2,
    }
}

/// An element of a set: a real number, a date or an infinity.
fn set_end(e: &Expr) -> bool {
    matches!(e, Expr::Num(_) | Expr::Date(_))
        || algebra::infinity(e).is_some_and(|(_, k)| k == "inf")
}

/// `math-intv-constp`: an interval of numbers, from `-inf` or to `inf`.
fn const_intv(e: &Expr) -> bool {
    let num = |e: &Expr| matches!(e, Expr::Num(_) | Expr::Date(_));
    matches!(e, Expr::Intv(_, lo, hi)
        if (num(lo) || **lo == algebra::neg_inf()) && (num(hi) || **hi == algebra::inf()))
}

/// `math-prepare-set`: a set as its intervals, sorted, none empty, none
/// overlapping or touching another.
fn prepare_set(a: &Expr, env: &Env) -> Option<Vec<Span>> {
    let mut v = match a {
        Expr::Vec(v) => v.clone(),
        a if objectp(a) => vec![a.clone()],
        _ => return None,
    };
    v.sort_by(|a, b| before(a, b, env));
    let mut out: Vec<Span> = Vec::new();
    for x in v {
        match x {
            Expr::Intv(m, a, b) => {
                if !const_intv(&Expr::Intv(m, a.clone(), b.clone())) {
                    return None;
                }
                if m != 3 && cmp_code(&a, &b, env) == 0 {
                    continue;
                }
                out.push((m, *a, *b));
            }
            x if set_end(&x) => out.push((3, x.clone(), x)),
            _ => return None,
        }
    }
    let mut i = 0;
    while i + 1 < out.len() {
        let (p, q) = (&out[i], &out[i + 1]);
        let res = cmp_code(&p.2, &q.1, env);
        if res == -1 || res == 2 || (res == 0 && p.0 & 1 == 0 && q.0 & 2 == 0) {
            i += 1;
            continue;
        }
        let res = cmp_code(&p.2, &q.2, env);
        let same_low = cmp_code(&p.1, &q.1, env) == 0;
        let low = (p.0 | if same_low { q.0 } else { 0 }) & 2;
        let high = ((if res != -1 { p.0 } else { 0 }) | (if res != 1 { q.0 } else { 0 })) & 1;
        let hi = if res == 1 { p.2.clone() } else { q.2.clone() };
        out[i] = (low | high, p.1.clone(), hi);
        out.remove(i + 1);
    }
    Some(out)
}

/// `math-clean-set`: intervals of one point as that point, a set of one
/// interval as the interval.
fn clean_set(v: Vec<Span>, always_vec: bool, env: &Env) -> Expr {
    let mut items: Vec<Expr> = v
        .into_iter()
        .map(|(m, a, b)| {
            if cmp_code(&a, &b, env) == 0 {
                a
            } else {
                Expr::Intv(m, Box::new(a), Box::new(b))
            }
        })
        .collect();
    if items.len() == 1 && matches!(items[0], Expr::Intv(..)) && !always_vec {
        return items.remove(0);
    }
    Expr::Vec(items)
}

/// `calcFunc-vcompl`.
fn vcompl(a: &Expr, env: &Env) -> Option<Expr> {
    let set = prepare_set(a, env)?;
    let mut out = Vec::new();
    let mut prev = algebra::neg_inf();
    let mut closed = 2;
    for (m, lo, hi) in set {
        if !(lo == algebra::neg_inf() && m & 2 != 0) {
            out.push((closed + u8::from(m & 2 == 0), prev, lo));
        }
        prev = hi;
        closed = if m & 1 == 0 { 2 } else { 0 };
    }
    if !(prev == algebra::inf() && closed == 0) {
        out.push((closed + 1, prev, algebra::inf()));
    }
    Some(clean_set(out, false, env))
}

/// `calcFunc-vfloor`: the integers of a set, as intervals.
fn vfloor(a: &Expr, env: &Env) -> Option<Vec<Span>> {
    let mut out: Vec<Span> = Vec::new();
    for (mut m, mut a, mut b) in prepare_set(a, env)? {
        let integer = |e: &Expr| matches!(e, Expr::Num(n) if n.is_rational() && num::trunc(n) == *n || n.is_messy_integer());
        if m & 2 == 0 && algebra::infinity(&a).is_none() {
            m |= 2;
            if integer(&a) {
                a = add(&a, &Expr::int(1), env);
            }
        }
        if let Expr::Num(n) = &a {
            a = Expr::Num(num::ceil(n));
        }
        if m & 1 == 0 && algebra::infinity(&b).is_none() {
            m |= 1;
            if integer(&b) {
                b = sub(&b, &Expr::int(1), env);
            }
        }
        if let Expr::Num(n) = &b {
            b = Expr::Num(num::floor(n));
        }
        if let Some(prev) = out.last_mut()
            && cmp_code(&sub(&a, &Expr::int(1), env), &prev.2, env) == 0
        {
            prev.2 = b;
            continue;
        }
        if cmp_code(&b, &a, env) != -1 {
            out.push((m, a, b));
        }
    }
    Some(out)
}

/// A random 64-bit word: xorshift64*, seeded from the clock once per
/// thread.
fn random_word() -> u64 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new({
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64);
            t | 1
        });
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

/// A random integer of `digits` decimal digits.
fn random_digits(digits: usize) -> BigInt {
    let mut r = BigInt::zero();
    for _ in 0..digits.div_ceil(18) {
        r = r * BigInt::from(1_000_000_000_000_000_000u64)
            + BigInt::from(random_word() % 1_000_000_000_000_000_000);
    }
    r
}

/// `math-random-float`: in [0, 1), to the working precision.
fn random_float(env: &Env) -> Num {
    let d = env.prec.digits.max(1) as usize;
    num::make_float(
        random_digits(d) % BigInt::from(10).pow(d as u32),
        -(d as i64),
        &env.prec,
    )
}

/// `calcFunc-random`: an integer below `n` (above it when negative), a
/// real times a random float, a point of an interval, an element of a
/// vector, a Gaussian float for 0.
fn random(max: &Expr, env: &Env) -> Option<Expr> {
    match max {
        Expr::Num(n) if n.is_zero() => {
            // Box–Muller.
            let u = (random_word() >> 11) as f64 / (1u64 << 53) as f64;
            let v = (random_word() >> 11) as f64 / (1u64 << 53) as f64;
            let g = (-2. * (1. - u).ln()).sqrt() * (std::f64::consts::TAU * v).cos();
            super::eval::from_f64(g, env).map(Expr::Num)
        }
        Expr::Num(Num::Int(n)) => {
            let digs = n.abs().to_string().len();
            Some(int(random_digits(digs + 3).mod_floor(n)))
        }
        Expr::Num(x) => Some(Expr::Num(num::mul(&random_float(env), x, &env.prec))),
        Expr::Intv(m, lo, hi) => {
            let (Expr::Num(a), Expr::Num(b)) = (&**lo, &**hi) else {
                return None;
            };
            if cmp_code(lo, hi, env) != -1 {
                return None;
            }
            if a.is_float() || b.is_float() {
                loop {
                    let p = &env.prec;
                    let v = num::add(&num::mul(&random_float(env), &num::sub(b, a, p), p), a, p);
                    let open_low = m & 2 == 0 && num::cmp(&v, a, p).is_eq();
                    let open_high = m & 1 == 0 && num::cmp(&v, b, p).is_eq();
                    if !open_low && !open_high {
                        return Some(Expr::Num(v));
                    }
                }
            }
            let lo = if m & 2 == 0 {
                add(lo, &Expr::int(1), env)
            } else {
                (**lo).clone()
            };
            let hi = if m & 1 != 0 {
                add(hi, &Expr::int(1), env)
            } else {
                (**hi).clone()
            };
            if cmp_code(&lo, &hi, env) != -1 {
                return None;
            }
            Some(add(&random(&sub(&hi, &lo, env), env)?, &lo, env))
        }
        Expr::Vec(v) if !v.is_empty() => {
            let k = (random_word() % v.len() as u64) as usize;
            Some(v[k].clone())
        }
        _ => None,
    }
}
