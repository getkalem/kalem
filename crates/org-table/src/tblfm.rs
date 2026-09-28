//! `#+TBLFM` lines: the formulas stored under a table, read as
//! `org-table-get-stored-formulas` reads them, with their flags read as
//! `org-table-eval-formula` reads them.

/// One `LHS=RHS` of a `#+TBLFM` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Equation {
    /// The left-hand side as Org keeps it: `$3`, `$>`, `@2$4`, `@>$<`,
    /// `@2$1..@3$2`, `@3`, or a field name without its `$`.
    pub lhs: String,
    /// The right-hand side, with its flags after the last `;`.
    pub rhs: String,
}

impl Equation {
    /// The formula and its flags: `vsum($1..$3);%.2f` gives
    /// `vsum($1..$3)` and `%.2f`. The flags follow the last `;`.
    pub fn formula(&self) -> (&str, Option<&str>) {
        match self.rhs.rfind(';') {
            Some(i) => (&self.rhs[..i], Some(&self.rhs[i + 1..])),
            None => (&self.rhs, None),
        }
    }

    /// Whether the formula is Emacs Lisp, `'(...)`, which Kalem keeps but
    /// does not evaluate.
    pub fn is_lisp(&self) -> bool {
        let f = self.formula().0;
        f.len() > 2 && f.starts_with("'(")
    }
}

/// The equations of a `#+TBLFM` line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tblfm {
    /// The equations, in the order of the line.
    pub equations: Vec<Equation>,
    /// Left-hand sides defined more than once (Org refuses to compute
    /// such a table).
    pub duplicates: Vec<String>,
}

/// `org-split-string`: `s` split at the matches of `sep`, ignoring a
/// separator at the start or the end.
fn split_org(s: &str, sep: impl Fn(&str) -> Option<(usize, usize)>) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut first = true;
    while let Some((a, b)) = sep(&s[i..]).map(|(a, b)| (i + a, i + b)) {
        if !(first && a == 0) {
            out.push(&s[i..a]);
        }
        first = false;
        // An empty match would not advance.
        i = if b > i { b } else { b + 1 };
        if i > s.len() {
            return out;
        }
    }
    if i < s.len() {
        out.push(&s[i..]);
    }
    out
}

/// The separator ` *:: *` in `s`: its start and end.
fn double_colon(s: &str) -> Option<(usize, usize)> {
    let i = s.find("::")?;
    let start = s[..i].trim_end_matches(' ').len();
    let end = i + 2 + (s[i + 2..].len() - s[i + 2..].trim_start_matches(' ').len());
    Some((start, end))
}

/// `LHS *= *RHS` with the left-hand side
/// `@[-+I<>0-9.$@]+` or `$([_a-zA-Z0-9]+|[<>]+)`: the left-hand side as
/// Org keeps it and the right-hand side without trailing blanks.
fn split_equation(s: &str) -> Option<(String, &str)> {
    let b = s.as_bytes();
    let (lhs_end, group) = match b.first()? {
        b'@' => {
            let n = b[1..]
                .iter()
                .take_while(|c| {
                    matches!(c, b'-' | b'+' | b'I' | b'<' | b'>' | b'.' | b'$' | b'@')
                        || c.is_ascii_digit()
                })
                .count();
            (n > 0).then_some((1 + n, None))?
        }
        b'$' => {
            let word = b[1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
                .count();
            let n = if word > 0 {
                word
            } else {
                b[1..]
                    .iter()
                    .take_while(|c| matches!(c, b'<' | b'>'))
                    .count()
            };
            (n > 0).then_some((1 + n, Some(1..1 + n)))?
        }
        _ => return None,
    };
    let rest = s[lhs_end..].trim_start_matches(' ');
    let rest = rest.strip_prefix('=')?.trim_start_matches(' ');
    let rhs = rest.trim_end_matches([' ', '\t']);
    if rhs.is_empty() || rhs.contains('\n') {
        return None;
    }
    let m = &s[..lhs_end];
    let lhs = match group {
        None => m.to_string(),
        // A column reference, or else (named columns cannot be assigned)
        // a named field.
        Some(g) => {
            let name = &s[g];
            if name.bytes().all(|c| c.is_ascii_digit())
                || name.bytes().all(|c| matches!(c, b'<' | b'>'))
            {
                m.to_string()
            } else {
                name.to_string()
            }
        }
    };
    Some((lhs, rhs))
}

/// Reads the value of a `#+TBLFM` line, the text after `#+TBLFM:`.
pub fn parse(value: &str) -> Tblfm {
    let value = value.trim_start_matches(' ');
    let mut out = Tblfm::default();
    let mut seen: Vec<String> = Vec::new();
    for part in split_org(value, double_colon) {
        let Some((lhs, rhs)) = split_equation(part) else {
            continue;
        };
        if seen.contains(&lhs) {
            if !out.duplicates.contains(&lhs) {
                out.duplicates.push(lhs.clone());
            }
        } else {
            seen.push(lhs.clone());
        }
        out.equations.push(Equation {
            lhs,
            rhs: rhs.to_string(),
        });
    }
    out
}

/// The formulas in force for a table, `text` being what follows it: the
/// value of the first `#+TBLFM` line, which blank lines may precede, and
/// its offset in `text`. Later `#+TBLFM` lines are alternatives Org does
/// not apply.
pub fn active_line(text: &str) -> Option<(usize, &str)> {
    let mut at = 0;
    loop {
        let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
        let line = &text[at..end];
        let body = line.trim_start_matches([' ', '\t']);
        if body.is_empty() && end < text.len() {
            at = end + 1;
            continue;
        }
        let key = body.get(..8)?;
        if !key.eq_ignore_ascii_case("#+tblfm:") {
            return None;
        }
        let value = body[8..].trim_start_matches(' ');
        return Some((end - value.len(), value));
    }
}

/// Calc's float display format (`nN`, `fN`, `sN`, `eN`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatFormat {
    /// `nN`: `N` significant digits.
    Float(i64),
    /// `fN`: `N` digits after the point.
    Fix(i64),
    /// `sN`: scientific notation.
    Sci(i64),
    /// `eN`: engineering notation.
    Eng(i64),
}

/// How durations are written back (`t`, `T`, `U`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationOutput {
    /// `t`: as `org-table-duration-custom-format` says (hours by default).
    Custom,
    /// `T`: `HH:MM:SS`.
    HhMmSs,
    /// `U`: `HH:MM`.
    HhMm,
}

/// Calc's angle unit (`D`, `R`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Angle {
    /// Degrees, the default.
    Degrees,
    /// Radians.
    Radians,
}

/// The flags of a formula, after its last `;`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    /// `pN`: Calc's working precision in digits.
    pub precision: Option<i64>,
    /// The display format of floats.
    pub float_format: Option<FloatFormat>,
    /// `t`, `T`, `U`: fields are durations, and so is the result.
    pub duration: Option<DurationOutput>,
    /// `N`: fields are numbers, an empty or non-numeric one zero.
    pub numbers: bool,
    /// `L`: fields are inserted literally (Lisp formulas).
    pub literal: bool,
    /// `E`: empty fields are kept in ranges.
    pub keep_empty: bool,
    /// `D`, `R`: the angle unit.
    pub angle: Option<Angle>,
    /// `F`: fractions rather than floats.
    pub prefer_frac: bool,
    /// `S`: symbolic results.
    pub symbolic: bool,
    /// `u`: units are simplified.
    pub units: bool,
    /// What remains: a format for Emacs's `format`, such as `%.2f`.
    pub printf: Option<String>,
}

impl Flags {
    /// Reads `flags` as `org-table-eval-formula` does, after the value of
    /// the table parameter `%` if there is one.
    pub fn parse(flags: &str, percent: Option<&str>) -> Flags {
        let mut f = Flags::default();
        let mut s = format!("{}{flags}", percent.unwrap_or(""));
        // `\([pnfse]\)\(-?[0-9]+\)`, leftmost first, each match removed
        // (removing one may make another).
        while let Some((start, end, c, n)) = numbered_flag(&s) {
            match c {
                b'p' => f.precision = Some(n),
                b'n' => f.float_format = Some(FloatFormat::Float(n)),
                b'f' => f.float_format = Some(FloatFormat::Fix(n)),
                b's' => f.float_format = Some(FloatFormat::Sci(n)),
                _ => f.float_format = Some(FloatFormat::Eng(n)),
            }
            s.replace_range(start..end, "");
        }
        while let Some(i) = s.find(['t', 'T', 'U', 'N', 'L', 'E', 'D', 'R', 'F', 'S', 'u']) {
            match s.as_bytes()[i] {
                b't' => {
                    f.duration = Some(DurationOutput::Custom);
                    f.numbers = true;
                }
                b'T' => {
                    f.duration = Some(DurationOutput::HhMmSs);
                    f.numbers = true;
                }
                b'U' => {
                    f.duration = Some(DurationOutput::HhMm);
                    f.numbers = true;
                }
                b'N' => f.numbers = true,
                b'L' => f.literal = true,
                b'E' => f.keep_empty = true,
                b'D' => f.angle = Some(Angle::Degrees),
                b'R' => f.angle = Some(Angle::Radians),
                b'F' => f.prefer_frac = true,
                b'S' => f.symbolic = true,
                _ => f.units = true,
            }
            s.remove(i);
        }
        if s.chars().any(|c| !c.is_whitespace()) {
            f.printf = Some(s);
        }
        f
    }
}

/// The leftmost `[pnfse]-?[0-9]+` in `s`: start, end, letter and number.
fn numbered_flag(s: &str) -> Option<(usize, usize, u8, i64)> {
    let b = s.as_bytes();
    (0..b.len()).find_map(|i| {
        if !matches!(b[i], b'p' | b'n' | b'f' | b's' | b'e') {
            return None;
        }
        let sign = usize::from(b.get(i + 1) == Some(&b'-'));
        let digits = b[i + 1 + sign..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        let end = i + 1 + sign + digits;
        // `string-to-number` of a long run of digits gives a large number.
        let n = s[i + 1..end]
            .parse::<i64>()
            .unwrap_or(if sign == 1 { i64::MIN } else { i64::MAX });
        Some((i, end, b[i], n))
    })
}

/// The equations in the order `org-table-recalculate` applies them:
/// sorted by left-hand side, as text.
pub fn recalc_order(equations: &[Equation]) -> Vec<Equation> {
    let mut v = equations.to_vec();
    v.sort_by(|a, b| a.lhs.cmp(&b.lhs));
    v
}

/// `org-table-formula-make-cmp-string`: the key that orders stored
/// formulas; `ncol` is the number of columns of the table.
fn store_key(lhs: &str, ncol: usize) -> String {
    let mut a = lhs.to_string();
    if let Some(rest) = a.strip_prefix('$')
        && let Some(arrow) = rest.chars().next().filter(|c| matches!(c, '<' | '>'))
    {
        // `$<`, `$>`… sort last, `$<` before `$>`.
        let len = rest.chars().take_while(|&c| c == arrow).count();
        let n = if arrow == '<' {
            len as i64
        } else {
            ncol as i64 - len as i64 + 1
        };
        a = format!("${}", 10000 + if arrow == '<' { -1000 } else { 0 } + n);
    }
    // `^\(@\([0-9]+\)\)?\(\$?\([0-9]+\)\)?\(\$?[a-zA-Z0-9]+\)?`
    let b = a.as_bytes();
    let mut i = 0;
    let mut key = String::new();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    if b.first() == Some(&b'@') && digits(1) > 0 {
        let n = digits(1);
        key.push_str(&format!(
            "@{:05}",
            a[1..1 + n].parse::<u64>().unwrap_or(u64::MAX)
        ));
        i = 1 + n;
    }
    let dollar = usize::from(b.get(i) == Some(&b'$'));
    if digits(i + dollar) > 0 {
        let n = digits(i + dollar);
        key.push_str(&format!(
            "${:05}",
            a[i + dollar..i + dollar + n]
                .parse::<u64>()
                .unwrap_or(u64::MAX)
        ));
        i += dollar + n;
    }
    let dollar = usize::from(b.get(i) == Some(&b'$'));
    let word = b[i + dollar..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    if word > 0 {
        key.push_str("@@");
        key.push_str(&a[i + dollar..i + dollar + word]);
    }
    key
}

/// The equations in the order Org stores them (`org-table-store-formulas`).
pub fn store_order(equations: &[Equation], ncol: usize) -> Vec<Equation> {
    let mut v = equations.to_vec();
    v.sort_by_cached_key(|e| store_key(&e.lhs, ncol));
    v
}

/// The value of a `#+TBLFM` line holding `equations`, as Org writes it.
pub fn format(equations: &[Equation]) -> String {
    equations
        .iter()
        .map(|e| format!("{}={}", e.lhs, e.rhs))
        .collect::<Vec<_>>()
        .join("::")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eqs(value: &str) -> Vec<(String, String)> {
        parse(value)
            .equations
            .into_iter()
            .map(|e| (e.lhs, e.rhs))
            .collect()
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.into(), b.into())
    }

    #[test]
    fn equations() {
        assert_eq!(
            eqs("$3=$1*$2::@2$4=vsum(@2$1..@2$3);%.2f :: $>=1"),
            vec![
                pair("$3", "$1*$2"),
                pair("@2$4", "vsum(@2$1..@2$3);%.2f"),
                pair("$>", "1")
            ]
        );
        // Named fields lose their `$`; ranges and rows are kept.
        assert_eq!(
            eqs("$total = vsum($2..$4)::@2$1..@4$2=0::@3=2::$x1 = 3  "),
            vec![
                pair("total", "vsum($2..$4)"),
                pair("@2$1..@4$2", "0"),
                pair("@3", "2"),
                pair("x1", "3")
            ]
        );
        // Not equations: no `=`, an empty right-hand side, other text.
        assert_eq!(eqs("::$1::$2= ::x=1::$3=a=b"), vec![pair("$3", "a=b")]);
        let t = parse("$1=1::$1=2::$2=3");
        assert_eq!(t.duplicates, vec!["$1".to_string()]);
        assert_eq!(t.equations.len(), 3);
    }

    #[test]
    fn formulas_and_flags() {
        let e = Equation {
            lhs: "$2".into(),
            rhs: "vsum($1);p20;%.3f".into(),
        };
        assert_eq!(e.formula(), ("vsum($1);p20", Some("%.3f")));
        assert!(
            Equation {
                lhs: "$1".into(),
                rhs: "'(concat $2 $3);L".into()
            }
            .is_lisp()
        );
        let f = Flags::parse("%.2f", None);
        assert_eq!(f.printf.as_deref(), Some("%.2f"));
        let f = Flags::parse("NE", None);
        assert!(f.numbers && f.keep_empty && f.printf.is_none());
        let f = Flags::parse("p20n3f2", None);
        assert_eq!(f.precision, Some(20));
        assert_eq!(f.float_format, Some(FloatFormat::Fix(2)));
        let f = Flags::parse("T", None);
        assert_eq!(f.duration, Some(DurationOutput::HhMmSs));
        assert!(f.numbers);
        // Removing a flag can make another: `s3` goes, then `f-2`.
        let f = Flags::parse("fs3-2", None);
        assert_eq!(f.float_format, Some(FloatFormat::Fix(-2)));
        // The table's `%` parameter comes first.
        let f = Flags::parse("N", Some("%.1f"));
        assert!(f.numbers);
        assert_eq!(f.printf.as_deref(), Some("%.1f"));
    }

    #[test]
    fn lines() {
        let after = "\n  \n  #+TBLFM: $2=1\n#+TBLFM: $2=2\n";
        let (at, v) = active_line(after).unwrap();
        assert_eq!(v, "$2=1");
        assert_eq!(&after[at..at + 4], "$2=1");
        assert_eq!(active_line("#+tblfm:$1=1").unwrap().1, "$1=1");
        assert!(active_line("text\n#+TBLFM: $1=1").is_none());
        assert!(active_line("").is_none());
    }

    #[test]
    fn orders() {
        let e = |l: &str| Equation {
            lhs: l.into(),
            rhs: "1".into(),
        };
        let v = vec![
            e("$10"),
            e("@2$1"),
            e("$2"),
            e("$>"),
            e("$<"),
            e("name"),
            e("@10$1"),
        ];
        let lhs = |v: Vec<Equation>| v.into_iter().map(|e| e.lhs).collect::<Vec<_>>();
        assert_eq!(
            lhs(recalc_order(&v)),
            ["$10", "$2", "$<", "$>", "@10$1", "@2$1", "name"]
        );
        assert_eq!(
            lhs(store_order(&v, 5)),
            ["$2", "$10", "$<", "$>", "@2$1", "@10$1", "name"]
        );
        assert_eq!(format(&v[..2]), "$10=1::@2$1=1");
        // As Emacs orders them (`org-table-formula-less-p`).
        let t = parse("$3a=1::$<<=2::@>$<=3::  $4 =  x  ");
        assert_eq!(
            lhs(store_order(&t.equations, 5)),
            ["@>$<", "3a", "$4", "$<<"]
        );
    }
}
