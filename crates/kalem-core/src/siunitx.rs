//! siunitx's commands as they typeset in running text (version 3's
//! defaults): numbers with their digits grouped in threes from five
//! digits and exponents as "× 10ⁿ", units from their macros (`\kilo\gram`
//! kg, `\per\second` s⁻¹) or as written, a thin space between number and
//! unit, ranges with "to".

/// The prefixes, and what they print.
const PREFIXES: &[(&str, &str)] = &[
    ("yocto", "y"),
    ("zepto", "z"),
    ("atto", "a"),
    ("femto", "f"),
    ("pico", "p"),
    ("nano", "n"),
    ("micro", "µ"),
    ("milli", "m"),
    ("centi", "c"),
    ("deci", "d"),
    ("deca", "da"),
    ("deka", "da"),
    ("hecto", "h"),
    ("kilo", "k"),
    ("mega", "M"),
    ("giga", "G"),
    ("tera", "T"),
    ("peta", "P"),
    ("exa", "E"),
    ("zetta", "Z"),
    ("yotta", "Y"),
];

/// The units, and what they print.
const UNITS: &[(&str, &str)] = &[
    ("metre", "m"),
    ("meter", "m"),
    ("gram", "g"),
    ("kilogram", "kg"),
    ("second", "s"),
    ("ampere", "A"),
    ("kelvin", "K"),
    ("mole", "mol"),
    ("candela", "cd"),
    ("hertz", "Hz"),
    ("newton", "N"),
    ("pascal", "Pa"),
    ("joule", "J"),
    ("watt", "W"),
    ("coulomb", "C"),
    ("volt", "V"),
    ("farad", "F"),
    ("ohm", "Ω"),
    ("siemens", "S"),
    ("weber", "Wb"),
    ("tesla", "T"),
    ("henry", "H"),
    ("degreeCelsius", "°C"),
    ("lumen", "lm"),
    ("lux", "lx"),
    ("becquerel", "Bq"),
    ("gray", "Gy"),
    ("sievert", "Sv"),
    ("katal", "kat"),
    ("radian", "rad"),
    ("steradian", "sr"),
    ("litre", "L"),
    ("liter", "L"),
    ("hour", "h"),
    ("minute", "min"),
    ("day", "d"),
    ("electronvolt", "eV"),
    ("dalton", "Da"),
    ("bar", "bar"),
    ("percent", "%"),
    ("degree", "°"),
    ("arcminute", "′"),
    ("arcsecond", "″"),
    ("tonne", "t"),
    ("hectare", "ha"),
    ("bel", "B"),
    ("decibel", "dB"),
    ("neper", "Np"),
    ("angstrom", "Å"),
    ("astronomicalunit", "au"),
];

fn superscript(n: i32) -> String {
    n.to_string()
        .chars()
        .map(|c| match c {
            '-' => '⁻',
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            c => c,
        })
        .collect()
}

/// Digits in groups of three with thin spaces, from five digits on
/// (`group-minimum-digits = 5`); from the decimal point outwards.
fn group(digits: &str, from_left: bool) -> String {
    if digits.chars().count() < 5 {
        return digits.to_string();
    }
    let chars: Vec<char> = digits.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        let k = if from_left { i } else { chars.len() - i };
        if i > 0 && k % 3 == 0 {
            out.push('\u{2009}');
        }
        out.push(*c);
    }
    out
}

/// `\num{…}`: `12345.678` "12 345.678", `-1.5e3` "−1.5 × 10³",
/// `3x4` "3 × 4".
pub fn number(src: &str) -> String {
    let s: String = src.split_whitespace().collect();
    let s = s.replace(['{', '}'], "");
    if let Some((a, b)) = s.split_once('x') {
        return format!("{} × {}", number(a), number(b));
    }
    let (mantissa, exponent) = match s.find(['e', 'E', 'd', 'D']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s.as_str(), None),
    };
    let (sign, m) = match mantissa.strip_prefix('-') {
        Some(r) => ("\u{2212}", r),
        None => ("", mantissa.strip_prefix('+').unwrap_or(mantissa)),
    };
    let m = m.replace(',', ".");
    let mut out = String::from(sign);
    match m.split_once('.') {
        Some((i, f)) => {
            out.push_str(&group(i, false));
            out.push('.');
            out.push_str(&group(f, true));
        }
        None => out.push_str(&group(&m, false)),
    }
    if let Some(e) = exponent {
        let e: i32 = e.trim_start_matches('+').parse().unwrap_or(0);
        if m.is_empty() {
            out.push_str(&format!("10{}", superscript(e)));
        } else {
            out.push_str(&format!(" × 10{}", superscript(e)));
        }
    }
    out
}

/// `\unit{…}`: the unit macros (with `\per`, `\square`, `\cubic`,
/// `\squared`, `\cubed`, `\tothe{n}`) as symbols a thin space apart,
/// powers as superscripts; anything else as written, `.` and `~` as
/// thin spaces.
pub fn unit(src: &str) -> String {
    if !src.contains('\\') {
        return src.replace(['.', '~'], "\u{2009}");
    }
    let mut parts: Vec<(String, i32)> = Vec::new();
    let (mut prefix, mut power, mut per) = (String::new(), 1, false);
    let mut rest = src;
    while !rest.is_empty() {
        let r = rest.trim_start();
        let Some(r) = r.strip_prefix('\\') else {
            // Text between macros: kept as a unit of its own.
            let end = r.find('\\').unwrap_or(r.len());
            let text = r[..end].trim().trim_matches(['{', '}']);
            if !text.is_empty() {
                parts.push((text.to_string(), 1));
            }
            rest = &r[end..];
            continue;
        };
        let name: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        rest = &r[name.len()..];
        match name.as_str() {
            "per" => per = true,
            "square" => power = 2,
            "cubic" => power = 3,
            "squared" | "cubed" | "tothe" => {
                let p = match name.as_str() {
                    "squared" => 2,
                    "cubed" => 3,
                    _ => {
                        let t = rest.trim_start();
                        let inner = t.strip_prefix('{').and_then(|t| t.split_once('}'));
                        match inner {
                            Some((n, after)) => {
                                rest = after;
                                n.trim().parse().unwrap_or(1)
                            }
                            None => 1,
                        }
                    }
                };
                if let Some(last) = parts.last_mut() {
                    last.1 *= p;
                }
            }
            n => {
                if let Some((_, p)) = PREFIXES.iter().find(|(k, _)| *k == n) {
                    prefix.push_str(p);
                    continue;
                }
                let symbol = UNITS
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map_or_else(|| n.to_string(), |(_, s)| s.to_string());
                let p = if per { -power } else { power };
                parts.push((format!("{prefix}{symbol}"), p));
                prefix.clear();
                power = 1;
                per = false;
            }
        }
    }
    parts
        .into_iter()
        .map(|(u, p)| {
            if p == 1 {
                u
            } else {
                format!("{u}{}", superscript(p))
            }
        })
        .collect::<Vec<_>>()
        .join("\u{2009}")
}

/// A quantity: `\qty{3}{\metre}` "3 m", `\qty{50}{\percent}` "50 %";
/// the angle units without the space (`30°`).
pub fn quantity(number_src: &str, unit_src: &str) -> String {
    let u = unit(unit_src);
    let n = number(number_src);
    if matches!(u.as_str(), "°" | "′" | "″") {
        format!("{n}{u}")
    } else {
        format!("{n}\u{2009}{u}")
    }
}

/// What a siunitx command typesets for its mandatory arguments, or `None`
/// for another command.
pub fn render(command: &str, args: &[String]) -> Option<String> {
    let a = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    Some(match command {
        "num" => number(a(0)),
        "si" | "unit" => unit(a(0)),
        "SI" | "qty" => quantity(a(0), a(1)),
        "ang" => {
            // `\ang{1;2;3}`: degrees, minutes, seconds.
            let parts: Vec<&str> = a(0).split(';').collect();
            let marks = ["°", "′", "″"];
            parts
                .iter()
                .zip(marks)
                .filter(|(p, _)| !p.trim().is_empty())
                .map(|(p, m)| format!("{}{m}", number(p)))
                .collect::<Vec<_>>()
                .join("")
        }
        "numrange" => format!("{} to {}", number(a(0)), number(a(1))),
        "SIrange" | "qtyrange" => {
            format!("{} to {}", quantity(a(0), a(2)), quantity(a(1), a(2)))
        }
        "numlist" => {
            let items: Vec<String> = a(0).split(';').map(number).collect();
            match items.len() {
                0 => String::new(),
                1 => items[0].clone(),
                2 => format!("{} and {}", items[0], items[1]),
                n => format!("{} and {}", items[..n - 1].join(", "), items[n - 1]),
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typeset() {
        assert_eq!(number("12345.678"), "12\u{2009}345.678");
        assert_eq!(number("1234"), "1234");
        assert_eq!(number("-1.5e3"), "\u{2212}1.5 × 10³");
        assert_eq!(unit("\\kilo\\gram\\per\\second\\squared"), "kg\u{2009}s⁻²");
        assert_eq!(unit("\\square\\metre"), "m²");
        assert_eq!(unit("m/s"), "m/s");
        assert_eq!(
            render("qty", &["3".into(), "\\metre".into()]).unwrap(),
            "3\u{2009}m"
        );
        assert_eq!(
            render("SI", &["50".into(), "\\percent".into()]).unwrap(),
            "50\u{2009}%"
        );
        assert_eq!(
            render("qty", &["30".into(), "\\degree".into()]).unwrap(),
            "30°"
        );
        assert_eq!(render("numlist", &["1;2;3".into()]).unwrap(), "1, 2 and 3");
        assert_eq!(render("ang", &["30".into()]).unwrap(), "30°");
        assert_eq!(
            render(
                "SIrange",
                &["1".into(), "2".into(), "\\centi\\metre".into()]
            )
            .unwrap(),
            "1\u{2009}cm to 2\u{2009}cm"
        );
    }
}
