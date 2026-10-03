//! Complex numbers, Calc's `(cplx re im)`: written `(2, 3)`, and what
//! `sqrt`, `ln`, `^` and the inverse sines give outside the reals
//! (`sqrt(-4)` is `(0, 2)`), each as `calc-eval` gives it with Org's Calc
//! modes (`org-table/tests/calc-functions.txt`).

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::algebra;
use super::eval::{self, Env};
use super::expr::Expr;
use super::num::{self, Num, Prec};

/// The name of a complex number's call: the parser gives `(2, 3)` so.
pub(crate) const CPLX: &str = "cplx";

/// The parts of a complex number.
pub(crate) fn parts(e: &Expr) -> Option<(&Num, &Num)> {
    match e {
        Expr::Call(f, xs) if f == CPLX && xs.len() == 2 => match (&xs[0], &xs[1]) {
            (Expr::Num(a), Expr::Num(b)) => Some((a, b)),
            _ => None,
        },
        _ => None,
    }
}

/// `math-complex`: a number as a complex one.
fn complex(e: &Expr) -> Option<(Num, Num)> {
    match e {
        Expr::Num(a) => Some((a.clone(), Num::int(0))),
        e => parts(e).map(|(a, b)| (a.clone(), b.clone())),
    }
}

/// `math-normalize` of a complex number: real when its imaginary part is
/// zero.
pub(crate) fn make(re: Num, im: Num) -> Expr {
    if im.is_zero() {
        Expr::Num(re)
    } else {
        Expr::call(CPLX, vec![Expr::Num(re), Expr::Num(im)])
    }
}

fn wide(env: &Env, extra: i64) -> Env {
    Env {
        prec: Prec {
            digits: env.prec.digits + extra,
            ..env.prec
        },
        ..*env
    }
}

/// A result computed with extra digits, rounded to the working precision.
fn round(e: Expr, env: &Env) -> Expr {
    match e {
        Expr::Num(a) => Expr::Num(num::renormalize(a, &env.prec)),
        e => match parts(&e) {
            Some((a, b)) => make(
                num::renormalize(a.clone(), &env.prec),
                num::renormalize(b.clone(), &env.prec),
            ),
            None => e,
        },
    }
}

/// `+`, `-`, `*` and `/` when a complex number is involved.
pub(crate) fn arith(op: &str, a: &Expr, b: &Expr, env: &Env) -> Option<Expr> {
    if parts(a).is_none() && parts(b).is_none() {
        return None;
    }
    let ((ar, ai), (br, bi)) = (complex(a)?, complex(b)?);
    let p = &env.prec;
    let (add, sub, mul) = (
        |x: &Num, y: &Num| num::add(x, y, p),
        |x: &Num, y: &Num| num::sub(x, y, p),
        |x: &Num, y: &Num| num::mul(x, y, p),
    );
    Some(match op {
        "+" => make(add(&ar, &br), add(&ai, &bi)),
        "-" => make(sub(&ar, &br), sub(&ai, &bi)),
        "*" => make(
            sub(&mul(&ar, &br), &mul(&ai, &bi)),
            add(&mul(&ar, &bi), &mul(&ai, &br)),
        ),
        "/" if parts(b).is_none() => make(num::div(&ar, &br, p).ok()?, num::div(&ai, &br, p).ok()?),
        "/" => {
            let d = add(&mul(&br, &br), &mul(&bi, &bi));
            let re = add(&mul(&ar, &br), &mul(&ai, &bi));
            let im = sub(&mul(&ai, &br), &mul(&ar, &bi));
            make(num::div(&re, &d, p).ok()?, num::div(&im, &d, p).ok()?)
        }
        _ => return None,
    })
}

/// `math-neg` of a complex number.
pub(crate) fn neg(a: &Expr) -> Option<Expr> {
    let (re, im) = parts(a)?;
    Some(make(re.neg(), im.neg()))
}

/// `math-imaginary`: `a` times `(0, 1)`.
fn imaginary(a: &Expr, env: &Env) -> Option<Expr> {
    arith("*", a, &make(Num::int(0), Num::int(1)), env)
}

/// `math-sqrt` of a negative real or a complex number.
pub(crate) fn sqrt(a: &Expr, env: &Env) -> Option<Expr> {
    match a {
        Expr::Num(x) if x.is_negative() => imaginary(&Expr::Num(eval::sqrt(&x.neg(), env)?), env),
        a => {
            let (re, im) = parts(a)?;
            let w = wide(env, 2);
            let p = &w.prec;
            let d = abs_parts(re, im, &w)?;
            let half = num::make_float(BigInt::from(5), -1, p);
            let real = eval::sqrt(&num::mul(&num::add(&d, re, p), &half, p), &w)?;
            let mut imag = eval::sqrt(&num::mul(&num::sub(&d, re, p), &half, p), &w)?;
            if im.is_negative() {
                imag = imag.neg();
            }
            Some(round(make(real, imag), env))
        }
    }
}

/// `math-hypot` of a complex number's parts: its absolute value.
fn abs_parts(re: &Num, im: &Num, env: &Env) -> Option<Num> {
    if re.is_zero() {
        return Some(im.abs());
    }
    if im.is_zero() {
        return Some(re.abs());
    }
    let w = wide(env, 1);
    let p = &w.prec;
    let s = num::add(&num::mul(re, re, p), &num::mul(im, im, p), p);
    eval::sqrt(&s, &w).map(|r| num::renormalize(r, &env.prec))
}

/// `calcFunc-abs` of a complex number.
pub(crate) fn abs(a: &Expr, env: &Env) -> Option<Expr> {
    let (re, im) = parts(a)?;
    abs_parts(re, im, env).map(Expr::Num)
}

/// A complex number in doubles.
fn doubles(a: &Expr) -> Option<(f64, f64)> {
    complex(a).map(|(re, im)| (re.to_f64(), im.to_f64()))
}

/// Doubles as a complex number at the working precision.
fn from_doubles(re: f64, im: f64, env: &Env) -> Option<Expr> {
    Some(make(eval::from_f64(re, env)?, eval::from_f64(im, env)?))
}

/// `math-ln-raw` in doubles.
fn ln_raw(re: f64, im: f64) -> (f64, f64) {
    if im == 0. && re > 0. {
        (re.ln(), 0.)
    } else if im == 0. {
        ((-re).ln(), std::f64::consts::PI)
    } else {
        (0.5 * (re * re + im * im).ln(), im.atan2(re))
    }
}

/// `calcFunc-ln` of a negative real or a complex number.
pub(crate) fn ln(a: &Expr, env: &Env) -> Option<Expr> {
    if !negative_or_complex(a) {
        return None;
    }
    let (re, im) = doubles(a)?;
    let (r, i) = ln_raw(re, im);
    from_doubles(r, i, env)
}

/// `calcFunc-log10` of a negative real or a complex number: its `ln`
/// divided by `ln 10`.
pub(crate) fn log10(a: &Expr, env: &Env) -> Option<Expr> {
    if !negative_or_complex(a) {
        return None;
    }
    let (re, im) = doubles(a)?;
    let (r, i) = ln_raw(re, im);
    let l = std::f64::consts::LN_10;
    from_doubles(r / l, i / l, env)
}

fn negative_or_complex(a: &Expr) -> bool {
    match a {
        Expr::Num(x) => x.is_negative(),
        a => parts(a).is_some(),
    }
}

/// `calcFunc-exp` of a complex number.
pub(crate) fn exp(a: &Expr, env: &Env) -> Option<Expr> {
    parts(a)?;
    let (re, im) = doubles(a)?;
    let m = re.exp();
    from_doubles(m * im.cos(), m * im.sin(), env)
}

/// The inverse sine (`arcsin`) or cosine of a complex number or of a real
/// outside [-1, 1], in the angle mode: `-i ln(i x + sqrt(1 - x^2))`.
pub(crate) fn arcsin(a: &Expr, env: &Env, cosine: bool) -> Option<Expr> {
    let (re, im) = doubles(a)?;
    if parts(a).is_none() && re.abs() <= 1. {
        return None;
    }
    // sqrt(1 - x^2)
    let (sr, si) = csqrt(1. - (re * re - im * im), -2. * re * im);
    let (lr, li) = ln_raw(sr - im, si + re);
    // -i (lr, li) is (li, -lr).
    let (mut r, mut i) = (li, -lr);
    if cosine {
        r = std::f64::consts::FRAC_PI_2 - r;
        i = -i;
    }
    if env.degrees {
        r = r.to_degrees();
        i = i.to_degrees();
    }
    from_doubles(r, i, env)
}

/// The principal square root in doubles, as [`sqrt`] computes it.
fn csqrt(re: f64, im: f64) -> (f64, f64) {
    if im == 0. {
        return if re >= 0. {
            (re.sqrt(), 0.)
        } else {
            (0., (-re).sqrt())
        };
    }
    let d = re.hypot(im);
    let imag = ((d - re) * 0.5).sqrt();
    (
        (((d + re) * 0.5).sqrt()),
        if im < 0. { -imag } else { imag },
    )
}

/// `calcFunc-arg`: the angle of a number, in the angle mode.
pub(crate) fn arg(a: &Expr, env: &Env) -> Option<Expr> {
    match a {
        Expr::Num(x) if x.is_negative() => Some(if env.degrees {
            Expr::int(180)
        } else {
            Expr::Num(eval::from_f64(std::f64::consts::PI, env)?)
        }),
        Expr::Num(_) => Some(Expr::int(0)),
        a => {
            let (re, im) = parts(a)?;
            let r = im.to_f64().atan2(re.to_f64());
            Some(Expr::Num(eval::from_f64(
                if env.degrees { r.to_degrees() } else { r },
                env,
            )?))
        }
    }
}

/// `re`, `im` and `conj`.
pub(crate) fn part(f: &str, a: &Expr) -> Option<Expr> {
    let (re, im) = complex(a)?;
    Some(match f {
        "re" => Expr::Num(re),
        // `calcFunc-im` of a float is a float zero.
        "im" if parts(a).is_none() && re.is_float() => {
            Expr::Num(num::make_float(BigInt::from(0), 0, &Prec::default()))
        }
        "im" => Expr::Num(im),
        _ => make(re, im.neg()),
    })
}

/// `math-pow` when the base is complex or a negative real with an
/// exponent that is not an integer, or the exponent is complex.
pub(crate) fn pow(a: &Expr, b: &Expr, env: &Env) -> Option<Expr> {
    let base_complex = parts(a).is_some();
    let exp_complex = parts(b).is_some();
    let negative_base = matches!(a, Expr::Num(x) if x.is_negative());
    if !base_complex && !exp_complex && !negative_base {
        return None;
    }
    if let Expr::Num(y) = b {
        if let Num::Int(n) = y {
            if !base_complex {
                return None;
            }
            let w = wide(env, 2);
            return Some(round(ipow(a, n, &w)?, env));
        }
        if !base_complex && !negative_base {
            return None;
        }
        // `a^(k/4)`: square roots, then an integer power.
        if let Some(q) = quarter(y)
            && q != 0
        {
            let base = match (a, y.is_float()) {
                (Expr::Num(x), true) => Expr::Num(x.to_float(&env.prec)),
                (a, _) => a.clone(),
            };
            let s = match &base {
                Expr::Num(x) if !x.is_negative() => Expr::Num(eval::sqrt(x, env)?),
                base => sqrt(base, env)?,
            };
            let twice = num::mul(&Num::int(2), y, &env.prec);
            return Some(eval::pow(&s, &Expr::Num(twice), env));
        }
    }
    // e^(b ln a)
    let (ar, ai) = doubles(a)?;
    let (br, bi) = doubles(b)?;
    let (lr, li) = ln_raw(ar, ai);
    let (xr, xi) = (br * lr - bi * li, br * li + bi * lr);
    let m = xr.exp();
    from_doubles(m * xi.cos(), m * xi.sin(), env)
}

/// `math-ipow`: an integer power by repeated squaring.
fn ipow(a: &Expr, n: &BigInt, env: &Env) -> Option<Expr> {
    if n < &BigInt::from(0) {
        let inv = arith("/", &Expr::int(1), a, env)?;
        return ipow(&inv, &-n, env);
    }
    fn go(a: &Expr, n: u64, env: &Env) -> Expr {
        let mul = |x: &Expr, y: &Expr| algebra::mul(x, y, env);
        match n {
            0 => Expr::int(1),
            1 => a.clone(),
            n if n % 2 == 0 => go(&mul(a, a), n / 2, env),
            n => mul(a, &go(&mul(a, a), n / 2, env)),
        }
    }
    Some(go(a, n.to_u64()?, env))
}

/// `math-quarter-integer`: `k` when `x` is an integer plus `k/4`.
fn quarter(x: &Num) -> Option<u8> {
    if x.is_negative() {
        return quarter(&x.neg()).map(|k| (4 - k) % 4);
    }
    match x {
        Num::Int(_) => Some(0),
        Num::Frac(n, d) if *d == BigInt::from(2) => {
            let _ = n;
            Some(2)
        }
        Num::Frac(n, d) if *d == BigInt::from(4) => (n % 4u8).to_u8(),
        Num::Frac(..) => None,
        Num::Float(m, e) => match *e {
            e if e >= 0 => Some(0),
            -1 => ((m % 10u8) == BigInt::from(5)).then_some(2),
            -2 => match (m % 100u8).to_u8()? {
                25 => Some(1),
                75 => Some(3),
                _ => None,
            },
            _ => None,
        },
    }
}
