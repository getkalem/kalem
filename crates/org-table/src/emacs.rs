//! The Emacs Lisp primitives Org's table formulas go through:
//! `string-to-number`, `number-to-string`, `format` and `format-seconds`.

/// An Emacs number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    /// A fixnum (or small bignum).
    Int(i128),
    /// A float.
    Float(f64),
}

impl Number {
    /// As a float.
    pub fn to_f64(self) -> f64 {
        match self {
            Number::Int(i) => i as f64,
            Number::Float(f) => f,
        }
    }
}

/// `string-to-number` in base 10: leading blanks skipped, then a sign,
/// digits, a fraction and an exponent as far as they go; 0 if none.
pub fn string_to_number(s: &str) -> Number {
    let s = s.trim_start_matches([' ', '\t', '\n', '\r']);
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    let mut float = false;
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        frac_digits = j - i - 1;
        if frac_digits > 0 {
            float = true;
            i = j;
        } else if int_digits > 0 {
            // `1.` is the integer 1.
            i += 1;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return Number::Int(0);
    }
    let mantissa_end = i;
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let d = b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        if d > 0 {
            float = true;
            i = j + d;
        }
    }
    let text = &s[..i];
    if float {
        let t = if mantissa_end == i {
            &s[..mantissa_end]
        } else {
            text
        };
        Number::Float(t.trim_end_matches('.').parse().unwrap_or(0.))
    } else {
        let t = s[..mantissa_end].trim_end_matches('.');
        match t.parse::<i128>() {
            Ok(n) => Number::Int(n),
            Err(_) => Number::Float(t.parse().unwrap_or(0.)),
        }
    }
}

/// C's `%.{p}g`.
fn format_g(x: f64, p: usize, alt: bool) -> String {
    if x == 0. {
        return if x.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let p = p.max(1);
    let sci = format!("{:.*e}", p - 1, x);
    let (mant, exp) = sci.split_once('e').expect("an exponent");
    let exp: i32 = exp.parse().expect("a number");
    if exp < -4 || exp >= p as i32 {
        let mant = if alt {
            mant.to_string()
        } else {
            strip_zeros(mant)
        };
        format!("{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        let s = format!("{:.*}", decimals, x);
        if alt { s } else { strip_zeros(&s) }
    }
}

fn strip_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// `number-to-string`.
pub fn number_to_string(n: Number) -> String {
    match n {
        Number::Int(i) => i.to_string(),
        Number::Float(f) => float_to_string(f),
    }
}

/// How Emacs prints a float: the shortest `%.15g` to `%.17g` that reads
/// back, with a point.
pub fn float_to_string(f: f64) -> String {
    if f.is_nan() {
        return if f.is_sign_negative() {
            "-0.0e+NaN".into()
        } else {
            "0.0e+NaN".into()
        };
    }
    if f.is_infinite() {
        return if f < 0. {
            "-1.0e+INF".into()
        } else {
            "1.0e+INF".into()
        };
    }
    let mut s = String::new();
    for p in 15..=17 {
        s = format_g(f, p, false);
        if s.parse::<f64>().ok() == Some(f) {
            break;
        }
    }
    if !s.contains(['.', 'e']) {
        s.push_str(".0");
    }
    s
}

/// Emacs's `format` with one number for every specification (`%.2f`,
/// `%d`, `%5.1f`, `%%`…); `None` if the format asks for something else.
pub fn format(fmt: &str, n: Number) -> Option<String> {
    let mut out = String::new();
    let mut it = fmt.char_indices().peekable();
    while let Some((_, c)) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut flags = String::new();
        while let Some(&(_, f)) = it.peek() {
            if matches!(f, '-' | '+' | ' ' | '0' | '#') {
                flags.push(f);
                it.next();
            } else {
                break;
            }
        }
        let mut width = String::new();
        while let Some(&(_, d)) = it.peek() {
            if d.is_ascii_digit() {
                width.push(d);
                it.next();
            } else {
                break;
            }
        }
        let mut prec: Option<usize> = None;
        if let Some(&(_, '.')) = it.peek() {
            it.next();
            let mut p = String::new();
            while let Some(&(_, d)) = it.peek() {
                if d.is_ascii_digit() {
                    p.push(d);
                    it.next();
                } else {
                    break;
                }
            }
            prec = Some(p.parse().unwrap_or(0));
        }
        let (_, conv) = it.next()?;
        let body = match conv {
            '%' => {
                out.push('%');
                continue;
            }
            'd' | 'i' => {
                let v = match n {
                    Number::Int(i) => i,
                    Number::Float(f) => f.trunc() as i128,
                };
                let mut s = v.abs().to_string();
                if let Some(p) = prec
                    && s.len() < p
                {
                    s = format!("{}{s}", "0".repeat(p - s.len()));
                }
                sign(v < 0, &flags) + &s
            }
            'f' | 'e' | 'g' | 'E' | 'G' => {
                let x = n.to_f64();
                let p = prec.unwrap_or(6);
                let alt = flags.contains('#');
                let s = match conv {
                    'f' => format!("{:.*}", p, x.abs()),
                    'e' | 'E' => {
                        let t = format!("{:.*e}", p, x.abs());
                        let (m, e) = t.split_once('e').expect("an exponent");
                        let e: i32 = e.parse().expect("a number");
                        let t = format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
                        if conv == 'E' { t.to_uppercase() } else { t }
                    }
                    _ => {
                        let t = format_g(x.abs(), if p == 0 { 1 } else { p }, alt);
                        if conv == 'G' { t.to_uppercase() } else { t }
                    }
                };
                sign(x.is_sign_negative() && x != 0., &flags) + &s
            }
            's' | 'S' => number_to_string(n),
            'x' | 'X' | 'o' => {
                let v = match n {
                    Number::Int(i) => i,
                    Number::Float(f) => f.trunc() as i128,
                };
                let s = match conv {
                    'x' => format!("{:x}", v.abs()),
                    'X' => format!("{:X}", v.abs()),
                    _ => format!("{:o}", v.abs()),
                };
                sign(v < 0, &flags) + &s
            }
            'c' => char::from_u32(match n {
                Number::Int(i) => u32::try_from(i).ok()?,
                Number::Float(_) => return None,
            })?
            .to_string(),
            _ => return None,
        };
        let w: usize = width.parse().unwrap_or(0);
        if body.chars().count() >= w {
            out.push_str(&body);
        } else if flags.contains('-') {
            out.push_str(&body);
            out.push_str(&" ".repeat(w - body.chars().count()));
        } else if flags.contains('0') && !matches!(conv, 's' | 'S' | 'c') {
            let (sgn, digits) = match body.chars().next() {
                Some(c @ ('-' | '+' | ' ')) => (c.to_string(), body[1..].to_string()),
                _ => (String::new(), body.clone()),
            };
            out.push_str(&sgn);
            out.push_str(&"0".repeat(w - body.chars().count()));
            out.push_str(&digits);
        } else {
            out.push_str(&" ".repeat(w - body.chars().count()));
            out.push_str(&body);
        }
    }
    Some(out)
}

fn sign(negative: bool, flags: &str) -> String {
    if negative {
        "-".into()
    } else if flags.contains('+') {
        "+".into()
    } else if flags.contains(' ') {
        " ".into()
    } else {
        String::new()
    }
}

/// `format-seconds` with `%.2h:%.2m:%.2s` (`pad`) or `%h:%.2m:%.2s`;
/// seconds are rounded as Emacs rounds them (to the nearest, halves
/// up).
pub fn format_hms(secs: f64, pad: bool) -> String {
    let total = (secs + 0.5).floor() as i128;
    let (h, rest) = (total / 3600, total % 3600);
    let (m, s) = (rest / 60, rest % 60);
    if pad {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{h}:{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        for (x, s) in [
            (1000.0, "1000.0"),
            (0.1, "0.1"),
            (1.5e-7, "1.5e-07"),
            (1e21, "1e+21"),
            (1e16, "1e+16"),
            (1e15, "1e+15"),
            (123456789012345.0, "123456789012345.0"),
            (0.0001, "0.0001"),
            (1e-5, "1e-05"),
            (0.1 + 0.2, "0.30000000000000004"),
        ] {
            assert_eq!(float_to_string(x), s, "{x}");
        }
        for (s, n) in [
            ("12", Number::Int(12)),
            ("12.0", Number::Float(12.)),
            (" 12", Number::Int(12)),
            ("12abc", Number::Int(12)),
            (".5", Number::Float(0.5)),
            ("1.", Number::Int(1)),
            ("1e3", Number::Float(1000.)),
            ("-3", Number::Int(-3)),
            ("abc", Number::Int(0)),
            ("", Number::Int(0)),
            ("1.5e", Number::Float(1.5)),
            ("12:30", Number::Int(12)),
        ] {
            assert_eq!(string_to_number(s), n, "{s}");
        }
    }

    #[test]
    fn formats() {
        let f = |fmt: &str, n: Number| format(fmt, n).unwrap();
        assert_eq!(f("%.2f", Number::Float(2.675)), "2.67");
        assert_eq!(f("%.2f", Number::Float(0.125)), "0.12");
        assert_eq!(f("%.0f", Number::Float(2.5)), "2");
        assert_eq!(f("%d", Number::Float(-2.5)), "-2");
        assert_eq!(f("%.2f", Number::Int(3)), "3.00");
        assert_eq!(f("%5.1f", Number::Float(4.25)), "  4.2");
        assert_eq!(f("%s", Number::Float(3.5)), "3.5");
        assert_eq!(f("%e", Number::Float(12345.678)), "1.234568e+04");
        assert_eq!(f("%g", Number::Float(0.0001)), "0.0001");
        assert_eq!(f("%x", Number::Int(255)), "ff");
        assert_eq!(f("%+.1f", Number::Float(2.)), "+2.0");
        assert_eq!(f("%.2f%%", Number::Float(12.345)), "12.35%");
        assert_eq!(format_hms(3661., true), "01:01:01");
        assert_eq!(format_hms(3599.9, false), "1:00:00");
        assert_eq!(format_hms(1.5, true), "00:00:02");
    }
}
