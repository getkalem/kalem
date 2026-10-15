//! Calc's factorial and binomial coefficient of numbers that are not
//! integers (`calcFunc-fact`, `calcFunc-choose`, `math-gammap1-raw`): the
//! gamma function, `fact(x)` being `gamma(x + 1)`.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::eval::{self, Env};
use super::expr::Expr;
use super::num::{self, Num, Prec};

/// `calcFunc-fact` of a number: an integer's exactly (to 10,000), a
/// float's with an integer value as that integer's float, anything else
/// by the gamma function. `None` where Calc refuses: a negative integer,
/// a result past its floats' exponents.
pub(crate) fn fact(n: &Num, env: &Env) -> Option<Num> {
    match n {
        Num::Int(k) => Some(Num::Int(factorial(k.to_i64()?)?)),
        Num::Float(..) if n.is_messy_integer() => {
            let Num::Int(k) = num::trunc(n) else {
                return None;
            };
            let k = k.to_i64()?;
            match factorial(k) {
                Some(f) => Some(Num::Int(f).to_float(&env.prec)),
                None if k > 0 => gamma1p(n, env),
                None => None,
            }
        }
        _ => gamma1p(n, env),
    }
}

/// `k!` for `0 <= k <= 10000`.
fn factorial(k: i64) -> Option<BigInt> {
    (0..=10_000)
        .contains(&k)
        .then(|| (1..=k).fold(BigInt::one(), |acc, i| acc * i))
}

/// Γ(1 + x) at the working precision; `None` at a pole and past Calc's
/// floats (a magnitude of 10^4000000 or more, or of 10^-4000000 or less).
fn gamma1p(x: &Num, env: &Env) -> Option<Num> {
    let ((hi, lo), negative) = ln_gamma1p(x)?;
    let log10 = hi / std::f64::consts::LN_10;
    if !(-4_000_000.0 < log10 && log10 < 4_000_000.0) {
        return None;
    }
    let wide = Prec {
        digits: 40,
        prefer_frac: false,
    };
    let r = eval::exp(&num::add(&exact(hi), &exact(lo), &wide), env)?;
    Some(if negative { r.neg() } else { r })
}

/// `ln |Γ(1 + x)|` as the sum of two doubles (a large one keeps more
/// digits than one double holds), and whether Γ(1 + x) is negative, for a
/// real `x` not a negative integer: Stirling's series from 15 up, Γ's
/// recurrence below that, its reflection below -15, as `math-gammap1-raw`
/// goes. Near a negative integer the distance to it is taken in decimal,
/// where `x` is exact, before it is a double.
fn ln_gamma1p(x: &Num) -> Option<((f64, f64), bool)> {
    let exact = Prec {
        digits: 40,
        prefer_frac: false,
    };
    let plus = |k: i64| num::add(x, &Num::int(k), &exact);
    let xf = x.to_f64();
    if !xf.is_finite() {
        return None;
    }
    if xf < -15.0 {
        // Γ(1 + x) = -π / (Γ(1 + (-x - 1)) sin(π x)), `sin(π x)` that of
        // `x` less its nearest even integer.
        let ((hi, lo), negative) = ln_gamma1p(&plus(1).neg())?;
        let even = 2 * (xf / 2.0).round() as i64;
        let s = (std::f64::consts::PI * plus(-even).to_f64()).sin();
        if s == 0.0 {
            return None;
        }
        let rest = std::f64::consts::PI.ln() - s.abs().ln();
        return Some((add((-hi, -lo), (rest, 0.0)), negative == (s < 0.0)));
    }
    // Γ(1 + x) = Γ(1 + y) / ((x + 1) (x + 2) ... y), y = x + k >= 15.
    let mut k = 0;
    let mut below = 0.0;
    let mut negative = false;
    while xf + (k as f64) < 15.0 {
        k += 1;
        let y = plus(k);
        if y.is_zero() {
            return None;
        }
        below += y.to_f64().abs().ln();
        negative ^= y.is_negative();
    }
    Some((add(stirling(plus(k).to_f64()), (-below, 0.0)), negative))
}

/// `ln Γ(1 + y)` for `y >= 15`: `(y + 1/2) ln y - y + ln √(2π)` and the
/// series of Bernoulli numbers in `1 / y`; the first two terms in two
/// doubles each, `ln y` as `k ln 2 + ln f` with `f` in [1, 2).
fn stirling(y: f64) -> (f64, f64) {
    // B(2k) / (2k (2k - 1)), k = 1 to 7.
    const TERMS: [f64; 7] = [
        1.0 / 12.0,
        -1.0 / 360.0,
        1.0 / 1260.0,
        -1.0 / 1680.0,
        1.0 / 1188.0,
        -691.0 / 360_360.0,
        1.0 / 156.0,
    ];
    let inv = 1.0 / y;
    let inv2 = inv * inv;
    let mut series = 0.0;
    let mut power = inv;
    for t in TERMS {
        series += t * power;
        power *= inv2;
    }
    let ln_sqrt_2pi = 0.918_938_533_204_672_8;
    // ln 2 in two doubles.
    const LN2: (f64, f64) = (std::f64::consts::LN_2, 2.319_046_813_846_299_6e-17);
    let k = y.log2().floor();
    let f = y / k.exp2();
    let ln_y = add(scale(LN2, k), (f.ln(), 0.0));
    let main = scale(ln_y, y + 0.5);
    add(add(main, (-y, 0.0)), (ln_sqrt_2pi + series, 0.0))
}

/// The sum of two numbers each held as two doubles (Dekker, Knuth).
fn add(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let s = a.0 + b.0;
    let v = s - a.0;
    let e = (a.0 - (s - v)) + (b.0 - v) + a.1 + b.1;
    let hi = s + e;
    (hi, e - (hi - s))
}

/// A number held as two doubles times a double.
fn scale(a: (f64, f64), b: f64) -> (f64, f64) {
    let p = a.0 * b;
    let e = a.0.mul_add(b, -p) + a.1 * b;
    let hi = p + e;
    (hi, e - (hi - p))
}

/// A finite double exactly, as a Calc float: `m 2^e` is `m 5^-e 10^e`.
fn exact(r: f64) -> Num {
    if r == 0.0 {
        return Num::Float(BigInt::zero(), 0);
    }
    let bits = r.to_bits();
    let biased = i64::try_from((bits >> 52) & 0x7ff).expect("eleven bits");
    let fraction = bits & ((1 << 52) - 1);
    let (mut m, e) = if biased == 0 {
        (BigInt::from(fraction), -1074)
    } else {
        (BigInt::from(fraction | (1 << 52)), biased - 1075)
    };
    if r < 0.0 {
        m = -m;
    }
    // Every digit kept (a double has at most 767), trailing zeros dropped.
    let all = Prec {
        digits: 800,
        prefer_frac: false,
    };
    if e >= 0 {
        num::make_float(m << usize::try_from(e).expect("a small exponent"), 0, &all)
    } else {
        let k = u32::try_from(-e).expect("a small exponent");
        num::make_float(m * BigInt::from(5).pow(k), e, &all)
    }
}

/// `calcFunc-choose`: integers exactly; a lower index that is not an
/// integer by factorials; negative integers as Kronenburg extends them
/// (`choose(-3, 2)` is 6); a lower index up to 100 as a product, one past
/// that by factorials. `None` where Calc leaves it or refuses.
pub(crate) fn choose(n: &Expr, m: &Expr, env: &Env) -> Option<Expr> {
    let (Expr::Num(n), Expr::Num(m)) = (n, m) else {
        return None;
    };
    choose_num(n, m, env).map(Expr::Num)
}

fn choose_num(n: &Num, m: &Num, env: &Env) -> Option<Num> {
    let p = &env.prec;
    let integer = |x: &Num| matches!(x, Num::Int(_)) || x.is_messy_integer();
    if let (Num::Int(a), Num::Int(b)) = (n, m)
        && !b.is_negative()
        && b <= a
    {
        let (a, b) = (a.to_i64()?, b.to_i64()?);
        // `choose(n, k)` is `choose(n, n - k)`; beyond this the product
        // would not finish.
        let k = b.min(a - b);
        if k > 100_000 {
            return None;
        }
        let mut r = BigInt::one();
        for i in 0..k {
            r = r * (a - i) / (i + 1);
        }
        return Some(Num::Int(r));
    }
    if !integer(m) {
        if integer(n) && n.is_negative() {
            return None;
        }
        let top = fact(&n.to_float(p), env)?;
        let bottom = num::mul(&fact(m, env)?, &fact(&num::sub(n, m, p), env)?, p);
        return num::div(&top, &bottom, p).ok();
    }
    let parity = |x: &Num| match num::trunc(x) {
        Num::Int(t) => t.is_even(),
        _ => true,
    };
    if n.is_negative() && !m.is_negative() {
        // n < 0 <= m: (-1)^m choose(m - n - 1, m).
        let v = choose_num(&num::sub(&num::sub(m, n, p), &Num::int(1), p), m, env)?;
        return Some(if parity(m) { v } else { v.neg() });
    }
    if n.is_negative() && integer(n) {
        if num::cmp(n, m, p).is_lt() {
            return Some(Num::int(0));
        }
        // m <= n < 0: (-1)^(n - m) choose(-m - 1, n - m).
        let d = num::sub(n, m, p);
        let v = choose_num(&num::sub(&m.neg(), &Num::int(1), p), &d, env)?;
        return Some(if parity(&d) { v } else { v.neg() });
    }
    if m.is_negative() || (integer(n) && num::cmp(n, m, p).is_lt()) {
        return Some(Num::int(0));
    }
    let Num::Int(tm) = num::trunc(m) else {
        return None;
    };
    let tm = tm.to_i64()?;
    if tm > 100 {
        let top = fact(&n.to_float(p), env)?;
        let d = num::sub(n, m, p);
        let bottom = num::mul(&fact(&m.to_float(p), env)?, &fact(&d.to_float(p), env)?, p);
        return num::div(&top, &bottom, p).ok();
    }
    // `math-choose-float-iter` with a digit more.
    let wide = Prec {
        digits: p.digits + 1,
        ..*p
    };
    let (mut c, mut k) = (Num::int(1), n.clone());
    for i in 1..=tm {
        c = num::div(&num::mul(&c, &k, &wide), &Num::int(i), &wide).ok()?;
        k = num::sub(&k, &Num::int(1), &wide);
    }
    Some(num::renormalize(c, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_near_its_poles_and_far() {
        let env = Env::default();
        let g = |x: f64| gamma1p(&exact(x), &env).map(|n| n.to_f64());
        let close = |a: f64, b: f64| ((a - b) / b).abs() < 1e-7;
        assert!(close(g(0.5).unwrap(), 0.886_226_93));
        assert!(close(g(-0.5).unwrap(), 1.772_453_9));
        assert!(close(g(-3.3).unwrap(), -1.447_107_4));
        assert!(close(g(-20.7).unwrap(), 3.941_385_0e-18));
        assert_eq!(g(-3.0), None);
        // A hair from a pole: the hair is measured in decimal.
        let near = gamma1p(&num::read("-7476.9999477", &env.prec).unwrap(), &env).unwrap();
        assert_eq!(
            num::format(&near, num::Display::Float(8), &env.prec),
            "1.5124670e-25711"
        );
        assert_eq!(g(800_000.0), None);
        // Large: the logarithm's digits past one double's count. 200000!
        // is 1.42022534547e973350 (Emacs says 1.4202254e973350, its 15
        // digits short of the logarithm's 19).
        let big = gamma1p(&Num::int(200_000), &env).unwrap();
        assert_eq!(
            num::format(&big, num::Display::Float(8), &env.prec),
            "1.4202253e973350"
        );
    }

    #[test]
    fn doubles_exactly() {
        let wide = Prec {
            digits: 60,
            prefer_frac: false,
        };
        let f = |r: f64| num::format(&exact(r), num::Display::Float(0), &wide);
        assert_eq!(f(0.5), "0.5");
        assert_eq!(f(-3.0), "-3.");
        assert_eq!(
            f(0.1),
            "0.1000000000000000055511151231257827021181583404541015625"
        );
    }
}
