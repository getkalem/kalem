//! Calc's numbers: integers of any size, fractions and decimal floats,
//! with the arithmetic and rounding of `calc.el` (`math-add`, `math-mul`,
//! `math-div`, `math-make-float`…), so that results agree to the digit.

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// A Calc number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Num {
    /// An integer.
    Int(BigInt),
    /// A fraction in lowest terms, the denominator above one.
    Frac(BigInt, BigInt),
    /// `mant × 10^exp`, the mantissa without trailing zeros and with at
    /// most the working precision's digits.
    Float(BigInt, i64),
}

/// An arithmetic failure; Calc then leaves the expression unevaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// Division by zero.
    DivisionByZero,
    /// A result Calc cannot represent (overflow, a huge power).
    Range,
}

/// The modes that change arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prec {
    /// `calc-internal-prec`: significant digits of floats.
    pub digits: i64,
    /// `calc-prefer-frac`: inexact integer division gives a fraction.
    pub prefer_frac: bool,
}

impl Default for Prec {
    /// Org's defaults (`org-calc-default-modes`).
    fn default() -> Prec {
        Prec {
            digits: 12,
            prefer_frac: false,
        }
    }
}

fn big(n: i64) -> BigInt {
    BigInt::from(n)
}

fn pow10(n: u64) -> BigInt {
    num_traits::pow(big(10), n as usize)
}

/// `math-numdigs`: the number of decimal digits of `a` (0 for zero).
pub fn numdigs(a: &BigInt) -> i64 {
    if a.is_zero() {
        return 0;
    }
    a.abs().to_str_radix(10).len() as i64
}

/// `math-scale-right`: `a / 10^n` truncated toward zero.
fn scale_right(a: &BigInt, n: i64) -> BigInt {
    if n <= 0 {
        return a.clone();
    }
    // Truncating division of the magnitude, sign kept.
    let q = a.abs() / pow10(n as u64);
    if a.is_negative() { -q } else { q }
}

/// `math-scale-int`: `a × 10^n`, truncated toward zero when `n < 0`.
pub fn scale_int(a: &BigInt, n: i64) -> BigInt {
    match n.cmp(&0) {
        Ordering::Equal => a.clone(),
        Ordering::Greater => a * pow10(n as u64),
        Ordering::Less => scale_right(a, -n),
    }
}

/// `math-scale-rounding`: `a × 10^n` rounded half away from zero, after
/// truncating to one extra digit.
fn scale_rounding(a: &BigInt, n: i64) -> BigInt {
    if n >= 0 {
        return scale_int(a, n);
    }
    if a.is_negative() {
        return -scale_rounding(&-a, n);
    }
    let t = if n == -1 {
        a.clone()
    } else {
        scale_right(a, -1 - n)
    };
    (t + 5) / 10
}

/// `math-make-float`: a float rounded to the working precision, without
/// trailing zeros.
pub fn make_float(mant: BigInt, exp: i64, prec: &Prec) -> Num {
    if mant.is_zero() {
        return Num::Float(BigInt::zero(), 0);
    }
    let mut mant = mant;
    let mut exp = exp;
    let ldiff = prec.digits - numdigs(&mant);
    if ldiff < 0 {
        mant = scale_rounding(&mant, ldiff);
        exp -= ldiff;
    }
    let ten = big(10);
    while !mant.is_zero() && (&mant % &ten).is_zero() {
        mant /= &ten;
        exp += 1;
    }
    Num::Float(mant, exp)
}

/// `math-make-frac`: `num/den` in lowest terms, an integer if exact.
pub fn make_frac(num: BigInt, den: BigInt) -> Num {
    let (num, den) = if den.is_negative() {
        (-num, -den)
    } else {
        (num, den)
    };
    let g = num.gcd(&den);
    let (n, d) = if g.is_one() || g.is_zero() {
        (num, den)
    } else {
        (&num / &g, &den / &g)
    };
    if d.is_one() {
        Num::Int(n)
    } else {
        Num::Frac(n, d)
    }
}

impl Num {
    /// An integer.
    pub fn int(n: i64) -> Num {
        Num::Int(big(n))
    }

    /// Whether this is zero (of any type).
    pub fn is_zero(&self) -> bool {
        match self {
            Num::Int(a) | Num::Float(a, _) => a.is_zero(),
            Num::Frac(..) => false,
        }
    }

    /// Whether this is below zero.
    pub fn is_negative(&self) -> bool {
        match self {
            Num::Int(a) | Num::Frac(a, _) | Num::Float(a, _) => a.is_negative(),
        }
    }

    /// Whether this is a float.
    pub fn is_float(&self) -> bool {
        matches!(self, Num::Float(..))
    }

    /// Whether this is an integer or a fraction.
    pub fn is_rational(&self) -> bool {
        !self.is_float()
    }

    /// `Math-messy-integerp`: a float with an integer value.
    pub fn is_messy_integer(&self) -> bool {
        matches!(self, Num::Float(_, e) if *e >= 0)
    }

    /// `math-float`.
    pub fn to_float(&self, prec: &Prec) -> Num {
        match self {
            Num::Int(a) => make_float(a.clone(), 0, prec),
            Num::Frac(n, d) => div(&make_float(n.clone(), 0, prec), &Num::Int(d.clone()), prec)
                .unwrap_or_else(|_| make_float(BigInt::zero(), 0, prec)),
            Num::Float(..) => self.clone(),
        }
    }

    /// `math-neg`.
    pub fn neg(&self) -> Num {
        match self {
            Num::Int(a) => Num::Int(-a),
            Num::Frac(n, d) => Num::Frac(-n, d.clone()),
            Num::Float(m, e) => Num::Float(-m, *e),
        }
    }

    /// `math-abs`.
    pub fn abs(&self) -> Num {
        if self.is_negative() {
            self.neg()
        } else {
            self.clone()
        }
    }

    /// The value as an `f64`, for functions computed in floating point.
    pub fn to_f64(&self) -> f64 {
        match self {
            Num::Int(a) => a.to_f64().unwrap_or(f64::NAN),
            Num::Frac(n, d) => n.to_f64().unwrap_or(f64::NAN) / d.to_f64().unwrap_or(f64::NAN),
            Num::Float(m, e) => {
                // Through the decimal text, which rounds correctly.
                format!("{m}e{e}").parse().unwrap_or(f64::NAN)
            }
        }
    }

    /// The integer value, if this is an integer that fits.
    pub fn to_i64(&self) -> Option<i64> {
        match self {
            Num::Int(a) => a.to_i64(),
            _ => None,
        }
    }
}

/// Converts two reals to floats for float arithmetic.
fn floats(a: &Num, b: &Num, prec: &Prec) -> ((BigInt, i64), (BigInt, i64)) {
    let f = |x: &Num| match x.to_float(prec) {
        Num::Float(m, e) => (m, e),
        _ => unreachable!("math-float gives a float"),
    };
    (f(a), f(b))
}

/// `math-add`.
pub fn add(a: &Num, b: &Num, prec: &Prec) -> Num {
    if let (Num::Int(x), Num::Int(y)) = (a, b) {
        return Num::Int(x + y);
    }
    if a.is_zero() {
        return if a.is_float() && b.is_rational() {
            b.to_float(prec)
        } else {
            b.clone()
        };
    }
    if b.is_zero() {
        return if b.is_float() && a.is_rational() {
            a.to_float(prec)
        } else {
            a.clone()
        };
    }
    if a.is_rational() && b.is_rational() {
        let (an, ad) = rational(a);
        let (bn, bd) = rational(b);
        return make_frac(&an * &bd + &ad * &bn, ad * bd);
    }
    let ((am, ae), (bm, be)) = floats(a, b, prec);
    add_float(am, ae, bm, be, prec)
}

/// `math-add-float`.
fn add_float(am: BigInt, ae: i64, bm: BigInt, be: i64, prec: &Prec) -> Num {
    let ediff = ae - be;
    let limit = prec.digits + prec.digits;
    if ediff >= 0 {
        if ediff >= limit {
            return Num::Float(am, ae);
        }
        make_float(bm + scale_int(&am, ediff), be, prec)
    } else {
        if -ediff >= limit {
            return Num::Float(bm, be);
        }
        make_float(am + scale_int(&bm, -ediff), ae, prec)
    }
}

/// The numerator and denominator of an integer or fraction.
fn rational(a: &Num) -> (BigInt, BigInt) {
    match a {
        Num::Int(n) => (n.clone(), BigInt::one()),
        Num::Frac(n, d) => (n.clone(), d.clone()),
        Num::Float(..) => unreachable!("a rational"),
    }
}

/// `math-sub`.
pub fn sub(a: &Num, b: &Num, prec: &Prec) -> Num {
    add(a, &b.neg(), prec)
}

/// `math-mul`.
pub fn mul(a: &Num, b: &Num, prec: &Prec) -> Num {
    if let (Num::Int(x), Num::Int(y)) = (a, b) {
        return Num::Int(x * y);
    }
    if a.is_zero() {
        return if b.is_float() && a.is_rational() {
            a.to_float(prec)
        } else {
            a.clone()
        };
    }
    if b.is_zero() {
        return if a.is_float() && b.is_rational() {
            b.to_float(prec)
        } else {
            b.clone()
        };
    }
    if a.is_rational() && b.is_rational() {
        let (an, ad) = rational(a);
        let (bn, bd) = rational(b);
        return make_frac(an * bn, ad * bd);
    }
    let ((am, ae), (bm, be)) = floats(a, b, prec);
    make_float(am * bm, ae + be, prec)
}

/// `math-div`.
pub fn div(a: &Num, b: &Num, prec: &Prec) -> Result<Num, Reject> {
    if b.is_zero() {
        return Err(Reject::DivisionByZero);
    }
    if a.is_zero() {
        return Ok(if b.is_float() && a.is_rational() {
            a.to_float(prec)
        } else {
            a.clone()
        });
    }
    if let (Num::Int(x), Num::Int(y)) = (a, b) {
        let (q, r) = (x / y, x % y);
        if r.is_zero() {
            return Ok(Num::Int(q));
        }
        if prec.prefer_frac {
            return Ok(make_frac(x.clone(), y.clone()));
        }
        let fa = make_float(x.clone(), 0, prec);
        let fb = make_float(y.clone(), 0, prec);
        return Ok(div_float(&fa, &fb, prec));
    }
    if a.is_rational() && b.is_rational() {
        let (an, ad) = rational(a);
        let (bn, bd) = rational(b);
        return Ok(make_frac(an * bd, ad * bn));
    }
    let fa = a.to_float(prec);
    let fb = b.to_float(prec);
    Ok(div_float(&fa, &fb, prec))
}

/// `math-div-float`.
fn div_float(a: &Num, b: &Num, prec: &Prec) -> Num {
    let (Num::Float(am, ae), Num::Float(bm, be)) = (a, b) else {
        unreachable!("floats")
    };
    let ldiff = ((1 + prec.digits) - (numdigs(am) - numdigs(bm))).max(0);
    // `math-quotient`: truncation toward zero.
    let q = scale_int(am, ldiff) / bm;
    make_float(q, ae - be - ldiff, prec)
}

/// Compares two numbers (`math-compare` on reals).
pub fn cmp(a: &Num, b: &Num, prec: &Prec) -> Ordering {
    match (a, b) {
        (Num::Int(x), Num::Int(y)) => x.cmp(y),
        _ if a.is_rational() && b.is_rational() => {
            let (an, ad) = rational(a);
            let (bn, bd) = rational(b);
            (an * bd).cmp(&(bn * ad))
        }
        (Num::Float(am, ae), Num::Float(bm, be)) => {
            if a == b {
                return Ordering::Equal;
            }
            // `math-lessp-float`: exact, by signs when the exponents are
            // far apart.
            let limit = prec.digits + prec.digits;
            let ediff = ae - be;
            let less = if ediff >= 0 {
                if ediff >= limit {
                    if am.is_zero() {
                        bm.is_positive()
                    } else {
                        am.is_negative()
                    }
                } else {
                    &scale_int(am, ediff) < bm
                }
            } else if -ediff >= limit {
                if bm.is_zero() {
                    am.is_negative()
                } else {
                    bm.is_positive()
                }
            } else {
                am < &scale_int(bm, -ediff)
            };
            if less {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        _ => {
            // A float and a rational: the sign of their difference at the
            // working precision.
            let d = sub(a, b, prec);
            if d.is_zero() {
                Ordering::Equal
            } else if d.is_negative() {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
    }
}

/// `math-trunc`.
pub fn trunc(a: &Num) -> Num {
    match a {
        Num::Int(_) => a.clone(),
        Num::Frac(n, d) => Num::Int(n / d),
        Num::Float(m, e) => Num::Int(scale_int(m, *e)),
    }
}

/// `math-floor`.
pub fn floor(a: &Num) -> Num {
    match a {
        Num::Int(_) => a.clone(),
        _ if a.is_messy_integer() => trunc(a),
        _ => {
            // Not an integer value here: below zero, one less.
            let Num::Int(t) = trunc(a) else {
                unreachable!("math-trunc gives an integer")
            };
            if a.is_negative() {
                Num::Int(t - 1)
            } else {
                Num::Int(t)
            }
        }
    }
}

/// `math-ceiling`.
pub fn ceil(a: &Num) -> Num {
    floor(&a.neg()).neg()
}

/// `math-round`: half away from zero.
pub fn round(a: &Num, prec: &Prec) -> Num {
    match a {
        Num::Int(_) => a.clone(),
        _ if a.is_negative() => round(&a.neg(), prec).neg(),
        Num::Float(m, e) => {
            if *e >= 0 {
                return Num::Int(scale_int(m, *e));
            }
            // `(math-add a '(float 5 -1))` then truncate.
            let half = Num::Float(big(5), -1);
            trunc(&add(a, &half, prec))
        }
        Num::Frac(n, d) => {
            let half = Num::Frac(big(1), big(2));
            let s = add(&Num::Frac(n.clone(), d.clone()), &half, prec);
            trunc(&s)
        }
    }
}

/// `math-mod`: `a - floor(a/b)·b`.
pub fn modulo(a: &Num, b: &Num, prec: &Prec) -> Result<Num, Reject> {
    if a.is_zero() {
        return Ok(a.clone());
    }
    if b.is_zero() {
        return Err(Reject::DivisionByZero);
    }
    if let (Num::Int(x), Num::Int(y)) = (a, b)
        && !x.is_negative()
        && !y.is_negative()
    {
        return Ok(Num::Int(x % y));
    }
    let q = floor(&div(a, b, prec)?);
    Ok(sub(a, &mul(&q, b, prec), prec))
}

/// `calcFunc-idiv`: the quotient rounded toward minus infinity.
pub fn idiv(a: &Num, b: &Num, prec: &Prec) -> Result<Num, Reject> {
    if b.is_zero() {
        return Err(Reject::DivisionByZero);
    }
    if let (Num::Int(x), Num::Int(y)) = (a, b) {
        return Ok(Num::Int(x.div_floor(y)));
    }
    Ok(floor(&div(a, b, prec)?))
}

/// `math-ipow`: `a^n` for an integer `n`, with two extra digits.
pub fn ipow(a: &Num, n: &BigInt, prec: &Prec) -> Result<Num, Reject> {
    let extra = Prec {
        digits: prec.digits + 2,
        ..*prec
    };
    let r = if n.is_negative() {
        let inv = div(&Num::int(1), a, &extra)?;
        iipow(&inv, &-n, &extra)?
    } else {
        iipow(a, n, &extra)?
    };
    Ok(renormalize(r, prec))
}

fn iipow(a: &Num, n: &BigInt, prec: &Prec) -> Result<Num, Reject> {
    if n.is_zero() {
        return Ok(Num::int(1));
    }
    if n.is_one() {
        return Ok(a.clone());
    }
    // Exponents this large would not finish in Calc either.
    if n.bits() > 32 && !matches!(a, Num::Int(x) if x.abs().is_one() || x.is_zero()) {
        return Err(Reject::Range);
    }
    let sq = mul(a, a, prec);
    let half = n / 2;
    if n.is_even() {
        iipow(&sq, &half, prec)
    } else {
        Ok(mul(a, &iipow(&sq, &half, prec)?, prec))
    }
}

/// `math-normalize` at `prec`: floats rounded again.
pub fn renormalize(a: Num, prec: &Prec) -> Num {
    match a {
        Num::Float(m, e) => make_float(m, e, prec),
        other => other,
    }
}

/// `math-read-number` on a token of the Calc reader: `123`, `1.5`,
/// `.5`, `1e3`, `1.5e-7`, `2:3` (a fraction), `1:2:3`.
pub fn read(s: &str, prec: &Prec) -> Option<Num> {
    let s = s.trim_matches(' ');
    if s.is_empty() {
        return None;
    }
    if s.bytes().all(|c| c.is_ascii_digit()) {
        return s.parse().ok().map(Num::Int);
    }
    if let Some(rest) = s.strip_prefix(['-', '_', '+']) {
        let v = read(rest, prec)?;
        return Some(if s.starts_with('+') { v } else { v.neg() });
    }
    if s.bytes()
        .any(|c| !matches!(c, b'-' | b'+' | b'0'..=b'9' | b'e' | b'E' | b'.'))
    {
        return read_fancy(s);
    }
    // Decimal point.
    if let Some((int, frac)) = s.split_once('.')
        && int.bytes().all(|c| c.is_ascii_digit())
        && frac.bytes().all(|c| c.is_ascii_digit())
    {
        if int.is_empty() && frac.is_empty() {
            return None;
        }
        let digits = format!("{int}{frac}");
        let mant: BigInt = digits.parse().ok()?;
        return Some(make_float(mant, -(frac.len() as i64), prec));
    }
    // `e` notation.
    let i = s.rfind(['e', 'E'])?;
    let (mant, exp) = (&s[..i], &s[i + 1..]);
    let digits = exp.trim_start_matches(['+', '-']);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let limit = if exp.starts_with(['+', '-']) { 8 } else { 7 };
    if exp.len() > limit {
        return None;
    }
    let exp: i64 = exp.parse().ok()?;
    let mant = if mant.is_empty() {
        Num::int(1)
    } else {
        read(mant, prec)?
    };
    if !(-4_000_000 < exp && exp < 4_000_000) {
        return None;
    }
    let Num::Float(m, e) = mant.to_float(prec) else {
        return None;
    };
    Some(Num::Float(m, e + exp))
}

/// `math-read-number-fancy`, for fractions.
fn read_fancy(s: &str) -> Option<Num> {
    let parts: Vec<&str> = s.split([':', '/']).collect();
    if !parts.iter().all(|p| p.bytes().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let num = |p: &str| -> BigInt { p.parse().unwrap_or_else(|_| BigInt::one()) };
    match parts.as_slice() {
        [n, d] => {
            let numer = if n.is_empty() { BigInt::one() } else { num(n) };
            let denom = if n.is_empty() { BigInt::one() } else { num(d) };
            if d.is_empty() && !n.is_empty() {
                return None;
            }
            (!denom.is_zero()).then(|| make_frac(numer, denom))
        }
        [i, n, d] => {
            let int = if i.is_empty() { BigInt::zero() } else { num(i) };
            let numer = if n.is_empty() { BigInt::one() } else { num(n) };
            let denom = if n.is_empty() { BigInt::one() } else { num(d) };
            if d.is_empty() && !n.is_empty() {
                return None;
            }
            (!denom.is_zero()).then(|| make_frac(&numer + &int * &denom, denom))
        }
        _ => None,
    }
}

/// Calc's float display format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// `(float N)`: `N` significant digits, 0 for all.
    Float(i64),
    /// `(fix N)`: `N` digits after the point.
    Fix(i64),
    /// `(sci N)`.
    Sci(i64),
    /// `(eng N)`.
    Eng(i64),
}

impl Default for Display {
    /// Org's `(float 8)`.
    fn default() -> Display {
        Display::Float(8)
    }
}

/// `math-format-number` for the normal language in radix 10.
pub fn format(a: &Num, display: Display, prec: &Prec) -> String {
    match a {
        Num::Int(n) => n.to_string(),
        Num::Frac(n, d) => format!("{n}:{d}"),
        Num::Float(m, e) => {
            if m.is_negative() {
                return format!("-{}", format(&Num::Float(-m, *e), display, prec));
            }
            format_float(m.clone(), *e, display, prec)
        }
    }
}

/// `calc-display-sci-high` and `calc-display-sci-low`.
const SCI_HIGH: i64 = 0;
const SCI_LOW: i64 = -3;

fn format_float(mut mant: BigInt, mut exp: i64, display: Display, prec: &Prec) -> String {
    let (kind, mut figs) = match display {
        Display::Float(n) => ('f', n),
        Display::Fix(n) => ('x', n),
        Display::Sci(n) => ('s', n),
        Display::Eng(n) => ('e', n),
    };
    let point = ".";
    if kind == 'x' {
        let neg_figs = figs < 0;
        if neg_figs {
            figs = -figs;
        }
        if neg_figs || exp + numdigs(&mant) > -figs {
            let m = scale_rounding(&mant, exp + figs);
            let mut s = m.to_string();
            if s.len() as i64 <= figs {
                s = format!("{}{s}", "0".repeat((1 + figs - s.len() as i64) as usize));
            }
            return if figs > 0 {
                let cut = s.len() - figs as usize;
                format!("{}{point}{}", &s[..cut], &s[cut..])
            } else {
                format!("{s}{point}")
            };
        }
    }
    if figs < 0 {
        figs += prec.digits;
    }
    if figs > 0 {
        let adj = figs - numdigs(&mant);
        if adj < 0 {
            mant = scale_rounding(&mant, adj);
            exp -= adj;
        }
    }
    let mut s = mant.to_string();
    let len = s.len() as i64;
    let dpos = exp + len;
    if kind == 'f' && dpos <= prec.digits + SCI_HIGH && dpos >= SCI_LOW + 2 {
        if dpos == 0 {
            s = format!("0{point}{s}");
        } else if exp <= 0 && dpos > 0 {
            let d = dpos as usize;
            s = format!("{}{point}{}", &s[..d], &s[d..]);
        } else if exp > 0 {
            s = format!("{s}{}{point}", "0".repeat(exp as usize));
        } else {
            s = format!("0{point}{}{s}", "0".repeat((-dpos) as usize));
        }
        return s;
    }
    let eadj = exp + len;
    let scale = if kind == 'e' {
        1 + (eadj + 300_002).rem_euclid(3)
    } else {
        1
    };
    if scale > s.len() as i64 {
        s.push_str(&"0".repeat((scale - s.len() as i64) as usize));
    }
    if scale < s.len() as i64 {
        s.insert_str(scale as usize, point);
    }
    format!("{s}e{}", eadj - scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Prec {
        Prec::default()
    }

    fn n(s: &str) -> Num {
        read(s, &p()).unwrap()
    }

    fn show(a: &Num) -> String {
        format(a, Display::default(), &p())
    }

    #[test]
    fn reading_and_display() {
        for (i, o) in [
            ("1", "1"),
            ("1.5", "1.5"),
            ("1e3", "1000."),
            ("1e20", "1e20"),
            ("1.5e-7", "1.5e-7"),
            ("123456789.123", "123456790."),
            ("0.000012345", "1.2345e-5"),
            ("0.0012345", "1.2345e-3"),
            ("0.012345", "0.012345"),
            ("99999999.5", "100000000."),
            ("2:6", "1:3"),
            ("1:2:3", "5:3"),
            ("12345678901234", "12345678901234"),
            (".5", "0.5"),
            ("100000000000", "100000000000"),
            ("100000000000.", "100000000000."),
            ("1000000000000.", "1e12"),
        ] {
            assert_eq!(show(&n(i)), o, "{i}");
        }
    }

    #[test]
    fn arithmetic() {
        let pr = p();
        let d = |a: &str, b: &str| show(&div(&n(a), &n(b), &pr).unwrap());
        assert_eq!(d("1", "3"), "0.33333333");
        assert_eq!(d("2", "3"), "0.66666667");
        assert_eq!(d("6", "3"), "2");
        assert_eq!(d("10", "4"), "2.5");
        assert_eq!(show(&add(&n("0.1"), &n("0.2"), &pr)), "0.3");
        assert_eq!(show(&add(&n("1.5"), &n("1.5"), &pr)), "3.");
        assert_eq!(show(&mul(&n("100000000"), &n("1.5"), &pr)), "150000000.");
        assert_eq!(show(&add(&n("1e11"), &n("0.5"), &pr)), "100000000000.");
        assert_eq!(
            show(&ipow(&n("2"), &big(100), &pr).unwrap()),
            "1267650600228229401496703205376"
        );
        assert_eq!(show(&modulo(&n("-7"), &n("3"), &pr).unwrap()), "2");
        assert_eq!(show(&modulo(&n("7.5"), &n("2"), &pr).unwrap()), "1.5");
        assert_eq!(show(&round(&n("2.5"), &pr)), "3");
        assert_eq!(show(&round(&n("-2.5"), &pr)), "-3");
        assert_eq!(show(&floor(&n("-2.5"))), "-3");
        assert_eq!(show(&trunc(&n("-2.7"))), "-2");
        assert_eq!(show(&ceil(&n("2.1"))), "3");
        assert!(div(&n("1"), &n("0"), &pr).is_err());
        let frac = Prec {
            prefer_frac: true,
            ..pr
        };
        assert_eq!(show(&div(&n("1"), &n("3"), &frac).unwrap()), "1:3");
    }

    #[test]
    fn display_formats() {
        let pr = p();
        let f = |s: &str, d: Display| format(&n(s), d, &pr);
        assert_eq!(f("3.14159", Display::Fix(2)), "3.14");
        assert_eq!(f("2.5", Display::Fix(0)), "3.");
        assert_eq!(f("0.001", Display::Fix(2)), "1e-3");
        assert_eq!(f("-0.5", Display::Fix(0)), "-5e-1");
        assert_eq!(f("123.456", Display::Fix(1)), "123.5");
        assert_eq!(f("12345678901234.5", Display::Fix(2)), "12345678901200.00");
        assert_eq!(f("1000000000000", Display::Float(8)), "1000000000000");
        assert_eq!(f("1234.5", Display::Sci(3)), "1.23e3");
        assert_eq!(f("12345.6", Display::Eng(3)), "12.3e3");
        assert_eq!(f("3.14159", Display::Float(3)), "3.14");
    }
}
