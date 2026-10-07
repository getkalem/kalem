//! One formula applied to one field, as `org-table-eval-formula` applies
//! it: references in the formula's text replaced by the fields they name
//! (`(5)`, vectors `[1,2,3]`), the text handed to Calc, the result
//! formatted.

use crate::calc::num::{Display, Prec};
use crate::calc::{self, Modes};
use crate::emacs;
use crate::table::{Analysis, Row, Table};
use crate::tblfm::{DurationOutput, Flags, FloatFormat};

/// A formula Org refuses: the recalculation stops with this message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err<T>(msg: impl Into<String>) -> Result<T, Error> {
    Err(Error(msg.into()))
}

/// Other tables, for `remote(NAME, REF)`.
pub trait Remote {
    /// The table after `#+NAME: name` (or in the entry with that ID).
    fn table(&self, name_or_id: &str) -> Option<Table>;
}

/// No other tables.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRemote;

impl Remote for NoRemote {
    fn table(&self, _: &str) -> Option<Table> {
        None
    }
}

/// What a formula sees besides its table.
pub struct Env<'a> {
    /// Other tables.
    pub remote: &'a dyn Remote,
    /// `#+CONSTANTS` of the document and user constants: name and value.
    pub constants: &'a [(String, String)],
    /// Properties of the entry holding the table, for `$PROP_name`.
    pub property: &'a dyn Fn(&str) -> Option<String>,
    /// `org-table-duration-custom-format`, for the `t` flag.
    pub duration_custom: DurationCustom,
}

impl std::fmt::Debug for Env<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("constants", &self.constants)
            .finish_non_exhaustive()
    }
}

/// `org-table-duration-custom-format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DurationCustom {
    /// Hours with two decimals (the default).
    #[default]
    Hours,
    /// Minutes with one decimal.
    Minutes,
    /// Seconds.
    Seconds,
    /// Days with three decimals.
    Days,
}

/// The result of a formula in a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The new text of the field.
    Value(String),
    /// An Emacs Lisp formula: Kalem leaves the field as it is.
    Lisp,
}

/// `org-table-formula-handle-first/last-rc`: `@<`, `@>`, `$<`, `$>` (and
/// `@>>`…) replaced by row and column numbers, outside `remote(…)`.
pub fn first_last(s: &str, a: &Analysis) -> Result<String, Error> {
    let mut s = s.to_string();
    let mut start = 0;
    loop {
        let b = s.as_bytes();
        let Some(i) = (start..b.len()).find(|&i| {
            (matches!(b[i], b'@' | b'$') && matches!(b.get(i + 1), Some(b'<' | b'>')))
                || b[i..].starts_with(b"remote(")
        }) else {
            return Ok(s);
        };
        if s[i..].starts_with("remote(") {
            // `remote([^)]+)`: skipped.
            match s[i + 7..].find(')') {
                Some(j) if j > 0 => {
                    start = i + 7 + j + 1;
                    continue;
                }
                _ => {
                    start = i + 1;
                    continue;
                }
            }
        }
        let kind = b[i];
        let arrow = b[i + 1];
        let len = b[i + 1..].iter().take_while(|&&c| c == arrow).count();
        let nmax = if kind == b'@' {
            a.dlines.len() as i64 - 1
        } else {
            a.ncol as i64
        };
        let n = if arrow == b'<' {
            len as i64
        } else {
            nmax - len as i64 + 1
        };
        if n < 1 || n > nmax {
            return err(format!(
                "Reference \"{}\" in expression \"{s}\" points outside table",
                &s[i..i + 1 + len]
            ));
        }
        s.replace_range(i..i + 1 + len, &format!("{}{n}", kind as char));
        start = i;
    }
}

/// `org-table-time-string-to-seconds`.
pub fn time_to_seconds(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if let Some(v) = hms(s, true) {
        return v.to_string();
    }
    if !has_timestamp(s)
        && let Some(v) = hms(s, false)
    {
        return v.to_string();
    }
    emacs::number_to_string(emacs::string_to_number(s))
}

/// `-?H+:M+:S+` (or `-?H+:M+` when not `with_seconds`) anywhere in `s`,
/// in seconds.
fn hms(s: &str, with_seconds: bool) -> Option<i128> {
    let b = s.as_bytes();
    for start in 0..b.len() {
        let neg = b[start] == b'-';
        let mut i = start + usize::from(neg);
        let mut parts = Vec::new();
        let count = if with_seconds { 3 } else { 2 };
        for k in 0..count {
            let d = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
            if d == 0 {
                break;
            }
            parts.push(s[i..i + d].parse::<i128>().ok()?);
            i += d;
            if k + 1 < count {
                if b.get(i) != Some(&b':') {
                    break;
                }
                i += 1;
            }
        }
        if parts.len() == count {
            let secs = parts[0] * 3600 + parts[1] * 60 + parts.get(2).copied().unwrap_or(0);
            return Some(if neg { -secs } else { secs });
        }
    }
    None
}

/// Whether `s` holds an Org timestamp (`<2026-01-01 Thu>`,
/// `[2026-01-01]`).
fn has_timestamp(s: &str) -> bool {
    timestamp_at(s, 0).is_some()
}

/// The first timestamp in `s` from `from`: its range and whether active.
fn timestamp_at(s: &str, from: usize) -> Option<(usize, usize, bool)> {
    let b = s.as_bytes();
    let mut i = from;
    while i < b.len() {
        if matches!(b[i], b'<' | b'[') {
            let close = if b[i] == b'<' { b'>' } else { b']' };
            let rest = &s[i + 1..];
            let date = rest.len() >= 10
                && rest.as_bytes()[..10].iter().enumerate().all(|(k, c)| {
                    if k == 4 || k == 7 {
                        *c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                });
            if date
                && let Some(j) = rest[10..].find(close as char)
                && (j == 0 || rest.as_bytes()[10] == b' ')
                && !rest[10..10 + j].contains('\n')
            {
                return Some((i, i + 1 + 10 + j + 1, b[i] == b'<'));
            }
        }
        i += 1;
    }
    None
}

/// `org-table-time-seconds-to-string`.
fn seconds_to_time(secs: f64, output: DurationOutput, custom: DurationCustom) -> String {
    let s0 = secs.abs();
    let res = match output {
        DurationOutput::Custom => match custom {
            DurationCustom::Days => format!("{:.3}", s0 / 86400.),
            DurationCustom::Hours => format!("{:.2}", s0 / 3600.),
            DurationCustom::Minutes => format!("{:.1}", s0 / 60.),
            DurationCustom::Seconds => format!("{}", s0 as i128),
        },
        DurationOutput::HhMm => {
            let t = emacs::format_hms(s0, true);
            t[..t.len() - 3].to_string()
        }
        DurationOutput::HhMmSs => emacs::format_hms(s0, true),
    };
    if secs < 0. { format!("-{res}") } else { res }
}

/// A reference's value: one field or the fields of a range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A field.
    Field(String),
    /// Fields, row by row.
    Range(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lisp {
    No,
    Yes,
    Literal,
}

/// Emacs's `prin1-to-string` of a string.
fn prin1(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// `org-table-make-reference`.
fn make_reference(v: &Value, keep_empty: bool, numbers: bool, lisp: Lisp) -> String {
    let blank = |x: &str| x.chars().all(char::is_whitespace);
    let as_number = |x: &str| emacs::number_to_string(emacs::string_to_number(x));
    match v {
        Value::Field(x) => match lisp {
            Lisp::Literal => x.clone(),
            Lisp::Yes => {
                if numbers {
                    as_number(x)
                } else {
                    prin1(x)
                }
            }
            Lisp::No => {
                if !blank(x) {
                    let x = if numbers { as_number(x) } else { x.clone() };
                    format!("({x})")
                } else if !keep_empty || numbers {
                    "(0)".into()
                } else {
                    "nan".into()
                }
            }
        },
        Value::Range(items) => {
            let items: Vec<&String> = items.iter().filter(|x| keep_empty || !blank(x)).collect();
            match lisp {
                Lisp::Literal => items
                    .iter()
                    .map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                Lisp::Yes => items
                    .iter()
                    .map(|x| if numbers { as_number(x) } else { prin1(x) })
                    .collect::<Vec<_>>()
                    .join(" "),
                Lisp::No => {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|x| {
                            if !blank(x) {
                                if numbers { as_number(x) } else { (*x).clone() }
                            } else if !keep_empty || numbers {
                                "0".into()
                            } else {
                                "nan".into()
                            }
                        })
                        .collect();
                    format!("[{}]", parts.join(","))
                }
            }
        }
    }
}

/// `org-table-range-regexp`
/// (`@\([-+]?I*[-+]?[0-9]*\)\(\$[-+]?[0-9]+\)?\(\.\.@?…\)?`) at `@` in
/// `s[i..]`: the length and the groups row1, col1, row2, col2.
#[derive(Debug, Clone, Default)]
struct RangeMatch {
    len: usize,
    row1: String,
    col1: Option<String>,
    range: bool,
    row2: String,
    col2: Option<String>,
}

fn row_part(b: &[u8], mut i: usize) -> usize {
    if matches!(b.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    while b.get(i) == Some(&b'I') {
        i += 1;
    }
    if matches!(b.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    i
}

/// `\$[-+]?[0-9]+` at `i`: its end.
fn col_part(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'$') {
        return None;
    }
    let mut j = i + 1;
    if matches!(b.get(j), Some(b'-' | b'+')) {
        j += 1;
    }
    let d = b[j.min(b.len())..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    (d > 0).then_some(j + d)
}

fn match_range(s: &str, at: usize) -> RangeMatch {
    let b = s.as_bytes();
    let mut m = RangeMatch::default();
    let r1 = row_part(b, at + 1);
    m.row1 = s[at + 1..r1].to_string();
    let mut i = r1;
    if let Some(e) = col_part(b, i) {
        m.col1 = Some(s[i..e].to_string());
        i = e;
    }
    if s[i..].starts_with("..") {
        let mut j = i + 2;
        if b.get(j) == Some(&b'@') {
            j += 1;
        }
        let r2 = row_part(b, j);
        m.row2 = s[j..r2].to_string();
        let mut k = r2;
        if let Some(e) = col_part(b, k) {
            m.col2 = Some(s[k..e].to_string());
            k = e;
        }
        m.range = true;
        i = k;
    }
    m.len = i - at;
    m
}

/// The evaluation of formulas in one table.
pub struct Evaluator<'a> {
    /// The table, as the formulas change it.
    pub table: Table,
    /// Its analysis.
    pub analysis: Analysis,
    /// What else formulas see.
    pub env: &'a Env<'a>,
}

impl std::fmt::Debug for Evaluator<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Evaluator")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}

impl Evaluator<'_> {
    /// `org-table--descriptor-line`: the line a row descriptor (`3`,
    /// `-1`, `I`, `II+2`) names, from line `cline`.
    fn descriptor_line(&self, desc: &str, cline: usize) -> Result<Option<usize>, Error> {
        let a = &self.analysis;
        if !desc.is_empty() && desc.bytes().all(|c| c.is_ascii_digit()) {
            let n: usize = desc
                .parse()
                .map_err(|_| Error(format!("Invalid row descriptor `{desc}'")))?;
            if n == 0 {
                return Ok(None);
            }
            return match a.dlines.get(n) {
                Some(&l) => Ok(Some(l)),
                None => err(format!("Invalid row descriptor `{desc}'")),
            };
        }
        // `^\(\([-+]\)?\(I+\)\)?\(\([-+]\)?\([0-9]+\)\)?`
        let b = desc.as_bytes();
        let mut i = 0;
        let mut hdir = None;
        let mut hn = 0;
        if matches!(b.first(), Some(b'-' | b'+')) && b.get(1) == Some(&b'I') {
            hdir = Some(b[0]);
            i = 1;
        }
        while b.get(i) == Some(&b'I') {
            hn += 1;
            i += 1;
        }
        if hn == 0 {
            hdir = None;
            i = 0;
        }
        let mut odir = None;
        let mut j = i;
        if matches!(b.get(j), Some(b'-' | b'+')) {
            odir = Some(b[j]);
            j += 1;
        }
        let digits = b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        let on: Option<usize> =
            (digits > 0).then(|| desc[j..j + digits].parse().unwrap_or(usize::MAX));
        if digits == 0 {
            odir = None;
        }
        if (hn == 0 && on.is_none()) || (hn > 0 && on.is_some() && odir.is_none()) {
            return err(format!("Invalid row descriptor `{desc}'"));
        }
        let rel = on.is_some() && odir.is_some();
        let mut cline = cline as i64;
        if hn > 0 && hdir.is_none() {
            cline = 0;
            hdir = Some(b'+');
            if a.is_rule[0] {
                hn -= 1;
            }
        }
        if on.is_some() && odir.is_none() && hn == 0 {
            // A plain number is handled above; `+N`/`-N` need a sign.
            return err("Should never happen");
        }
        if hn > 0 || hdir.is_some() {
            cline = self.row_type(true, hn, cline, hdir == Some(b'-'), false, desc)?;
        }
        if let Some(on) = on {
            cline = self.row_type(false, on, cline, odir == Some(b'-'), rel, desc)?;
        }
        Ok(Some(cline as usize))
    }

    /// `org-table--row-type`: the line `n` rules (`rule`) or data lines
    /// away from `i`.
    fn row_type(
        &self,
        rule: bool,
        n: usize,
        mut i: i64,
        back: bool,
        _rel: bool,
        desc: &str,
    ) -> Result<i64, Error> {
        let types = &self.analysis.is_rule;
        let l = types.len() as i64;
        for _ in 0..n {
            loop {
                i += if back { -1 } else { 1 };
                if i < 0 || i >= l || types[i as usize] == rule {
                    break;
                }
            }
        }
        if i < 0 || i >= l {
            return err(format!("Row descriptor {desc} leads outside table"));
        }
        // The imaginary last rule means the last line.
        Ok(if i == l - 1 { i - 1 } else { i })
    }

    fn is_data(&self, line: usize) -> bool {
        matches!(self.table.rows.get(line), Some(Row::Data(_)))
    }

    /// `org-table-get-range`: the value of a reference like `@2$3` or
    /// `@2$1..@>$3`, from line `this` and column `col`.
    pub fn get_range(&self, desc: &str, this: usize, col: usize) -> Result<Value, Error> {
        let (r1, c1, r2, c2, rangep) = self.corners(desc, this, col)?;
        let rows = self.table.rows.len();
        if !rangep || (r1 == r2 && c1 == c2) {
            let mut l = r1;
            while l < rows && !self.is_data(l) {
                l += 1;
            }
            return Ok(Value::Field(self.table.field(l, c1).trim().to_string()));
        }
        let (mut first, mut last) = (r1.min(r2), r1.max(r2));
        let (fc, lc) = (c1.min(c2), c1.max(c2));
        while first < rows && !self.is_data(first) {
            first += 1;
        }
        while last > 0 && !self.is_data(last) {
            last -= 1;
        }
        let mut out = Vec::new();
        for l in first..=last.min(rows.saturating_sub(1)) {
            if self.is_data(l) {
                for c in fc..=lc {
                    out.push(self.table.field(l, c).trim().to_string());
                }
            }
        }
        Ok(Value::Range(out))
    }

    /// The lines and columns of a reference's corners, and whether it is a
    /// range.
    fn corners(
        &self,
        desc: &str,
        this: usize,
        col: usize,
    ) -> Result<(usize, usize, usize, usize, bool), Error> {
        let desc = if desc.starts_with('$') && desc.contains("..$") {
            desc.replace('$', "@0$")
        } else {
            desc.to_string()
        };
        let at = desc
            .find('@')
            .ok_or_else(|| Error(format!("Invalid table range specifier `{desc}'")))?;
        let m = match_range(&desc, at);
        let line = |r: &str| -> Result<usize, Error> {
            if r.is_empty() {
                return Ok(this);
            }
            Ok(self.descriptor_line(r, this)?.unwrap_or(this))
        };
        let column = |c: &Option<String>| -> usize {
            let Some(c) = c else { return col };
            let t = &c[1..];
            let n: i64 = t.parse().unwrap_or(0);
            if n == 0 {
                col
            } else if t.starts_with(['-', '+']) {
                (col as i64 + n).max(0) as usize
            } else {
                n as usize
            }
        };
        let r1 = line(&m.row1)?;
        let r2 = if m.range { line(&m.row2)? } else { r1 };
        let c1 = column(&m.col1);
        let c2 = if m.range { column(&m.col2) } else { c1 };
        let r2 = if m.range && m.row2.is_empty() {
            this
        } else {
            r2
        };
        Ok((r1, c1, r2, c2, m.range))
    }

    /// The expansion of a range on the left-hand side: its fields as
    /// `@R$C`.
    pub fn lhs_fields(&self, lhs: &str) -> Result<Vec<String>, Error> {
        let (r1, c1, r2, c2, _) = self.corners(lhs, 0, 1)?;
        let (first, last) = (r1.min(r2), r1.max(r2));
        let (fc, lc) = (c1.min(c2), c1.max(c2));
        let a = &self.analysis;
        let (Some(d1), Some(d2)) = (a.line_to_dline(first, false), a.line_to_dline(last, true))
        else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for r in d1..=d2 {
            for c in fc..=lc {
                out.push(format!("@{r}${c}"));
            }
        }
        Ok(out)
    }

    /// The value of a constant or parameter (`org-table-get-constant`).
    fn constant(&self, name: &str) -> String {
        if let Some(v) = self.analysis.parameter(name) {
            return v.to_string();
        }
        if let Some((_, v)) = self.env.constants.iter().find(|(n, _)| n == name) {
            return v.clone();
        }
        if let Some(p) = name.strip_prefix("PROP_")
            && let Some(v) = (self.env.property)(p)
        {
            return v;
        }
        "#UNDEFINED_NAME".into()
    }

    /// `org-table-formula-substitute-names`: column names, parameters and
    /// constants replaced by their values.
    pub fn substitute_names(&self, f: &str) -> String {
        let pp = !f.starts_with('\'');
        let duration = f.rfind(';').is_some_and(|i| f[i..].contains(['t', 'T']));
        // Column names: `$name` as a whole word.
        let mut new = String::with_capacity(f.len());
        let b = f.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'$' {
                let word = f[i + 1..]
                    .bytes()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    .count();
                if word > 0
                    && let Some(c) = self.analysis.column(&f[i + 1..i + 1 + word])
                {
                    new.push_str(&format!("${c}"));
                    i += 1 + word;
                    continue;
                }
            }
            new.push(b[i] as char);
            let ch = f[i..].chars().next().expect("a character");
            if ch.len_utf8() > 1 {
                new.pop();
                new.push(ch);
            }
            i += ch.len_utf8();
        }
        // Parameters and constants: `$name`, outside `remote(…)`.
        let mut start = 0;
        loop {
            let b = new.as_bytes();
            let found = (start..b.len()).find(|&i| {
                (b[i] == b'$' && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic))
                    || (b[i..].starts_with(b"remote(")
                        && (i == 0
                            || !new[..i]
                                .chars()
                                .next_back()
                                .is_some_and(char::is_alphanumeric)))
            });
            let Some(i) = found else { break };
            if new[i..].starts_with("remote(") {
                start = new[i..].find(')').map_or(new.len(), |j| i + j + 1);
                continue;
            }
            let len = new[i + 1..]
                .bytes()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
                .count();
            let name = new[i + 1..i + 1 + len].to_string();
            let mut value = self.constant(&name);
            if duration && !value.chars().all(char::is_whitespace) {
                value = time_to_seconds(&value);
            }
            let rep = if pp { format!("({value})") } else { value };
            new.replace_range(i..i + 1 + len, &rep);
            start = i + 1;
        }
        new
    }

    /// `org-table-eval-formula` for field `col` of line `line`: the
    /// formula (with its flags) after `substitute_names`.
    pub fn eval(&self, line: usize, col: usize, formula: &str) -> Result<Outcome, Error> {
        self.eval_explained(line, col, formula).map(|(o, _)| o)
    }

    /// [`Evaluator::eval`], and when the field gets `#ERROR`, why: Calc's
    /// message and the text Calc was given.
    pub fn eval_explained(
        &self,
        line: usize,
        col: usize,
        formula: &str,
    ) -> Result<(Outcome, Option<String>), Error> {
        let a = &self.analysis;
        let (formula, flags) = match formula.rfind(';') {
            Some(i) => (&formula[..i], Some(&formula[i + 1..])),
            None => (formula, None),
        };
        let flags = flags.map(|f| Flags::parse(f, a.parameter("%")));
        let flags = flags.unwrap_or_default();
        let formula = first_last(formula, a)?;
        let mut fields: Vec<String> = self
            .table
            .fields(line)
            .map(<[String]>::to_vec)
            .unwrap_or_default();
        if flags.duration.is_some() {
            fields = fields.iter().map(|x| time_to_seconds(x)).collect();
        }
        if flags.numbers {
            fields = fields
                .iter()
                .map(|x| {
                    if x.chars().any(|c| !c.is_whitespace()) {
                        emacs::number_to_string(emacs::string_to_number(x))
                    } else {
                        x.clone()
                    }
                })
                .collect();
        }
        let mut form = formula.clone();
        let lisp = if form.len() > 2 && form.starts_with("'(") {
            if flags.literal {
                Lisp::Literal
            } else {
                Lisp::Yes
            }
        } else {
            Lisp::No
        };
        if lisp != Lisp::No {
            return Ok((Outcome::Lisp, None));
        }
        // `@#` and `$#`: this row and column.
        let dline = a.line_to_dline(line, false).unwrap_or(0);
        while let Some(j) = [form.find("@#"), form.find("$#")]
            .into_iter()
            .flatten()
            .min()
        {
            let v = if form.as_bytes()[j] == b'@' {
                dline
            } else {
                col
            };
            form.replace_range(j..j + 2, &v.to_string());
        }
        if let Some(i) = form.find('&')
            && form[i + 1..]
                .starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | 'I'))
        {
            return err("Formula contains old &row reference, please rewrite using @-syntax");
        }
        let keep_empty = flags.keep_empty;
        let numbers = flags.numbers;
        let convert = |v: Value| -> Value {
            if flags.duration.is_none() {
                return v;
            }
            match v {
                Value::Field(x) => Value::Field(time_to_seconds(&x)),
                Value::Range(xs) => Value::Range(xs.iter().map(|x| time_to_seconds(x)).collect()),
            }
        };
        form = self.remote_indirection(&form, line, col)?;
        form = self.remotes(&form, keep_empty, numbers, &convert)?;
        // Ranges and fields with `@`.
        while let Some(at) = form.find('@') {
            let m = match_range(&form, at);
            if m.len <= 1 {
                break;
            }
            let desc = form[at..at + m.len].to_string();
            let v = convert(self.get_range(&desc, line, col)?);
            let rep = make_reference(&v, keep_empty, numbers, lisp);
            if rep.contains(&form) {
                return err(format!("Spreadsheet error: invalid reference \"{form}\""));
            }
            form.replace_range(at..at + m.len, &rep);
        }
        // Ranges in this row: `$1..$3`.
        while let Some((s, e, n1, n2)) = simple_range(&form) {
            let start = n1.value + if n1.relative { col as i64 } else { 0 } - 1;
            let end = n2.value + if n2.relative { col as i64 } else { 0 };
            if start < 0 || end < start || end as usize > fields.len() {
                return err(format!("Invalid range \"{}\"", &form[s..e]));
            }
            let v = Value::Range(fields[start as usize..end as usize].to_vec());
            let rep = make_reference(&v, keep_empty, numbers, lisp);
            form.replace_range(s..e, &rep);
        }
        // Fields in this row: `$3`, `$-1`.
        while let Some((s, e, r)) = field_ref(&form) {
            let n = r.value + if r.relative { col as i64 } else { 0 };
            let idx = if n == 0 { col as i64 } else { n.max(1) } - 1;
            let Some(x) = fields.get(idx as usize) else {
                return err(format!("Invalid field specifier \"{}\"", &form[s..e]));
            };
            let rep = make_reference(&Value::Field(x.clone()), keep_empty, numbers, lisp);
            if rep.contains(&formula) && !formula.is_empty() {
                return err(format!("Invalid field specifier \"{}\"", &form[s..e]));
            }
            form.replace_range(s..e, &rep);
        }
        // Inactive timestamps are made active for Calc.
        let form = activate_timestamps(&form);
        let modes = modes(&flags);
        let ev: Result<String, String> = if flags.duration.is_some() && is_hhmm(&form) {
            Ok(form.clone())
        } else {
            match calc::eval_checked(&form, &modes) {
                Ok((v, false)) if numbers && !keep_empty => {
                    Err(format!("Number expected, not {v} (flag N)"))
                }
                Ok((v, _)) => Ok(v),
                Err(e) => Err(format!("{} in {form}", e.message)),
            }
        };
        let ev = match (ev, flags.duration) {
            (Ok(v), Some(out)) if !v.is_empty() => {
                let secs = if is_hhmm(&v) {
                    emacs::string_to_number(&time_to_seconds(&v)).to_f64()
                } else {
                    emacs::string_to_number(&v).to_f64()
                };
                Ok(seconds_to_time(secs, out, self.env.duration_custom))
            }
            (other, _) => other,
        };
        let (value, why) = match ev {
            Err(why) => ("#ERROR".to_string(), Some(why)),
            Ok(v) => (
                match &flags.printf {
                    Some(fmt) => emacs::format(fmt, emacs::string_to_number(&v))
                        .ok_or_else(|| Error(format!("Invalid format \"{fmt}\"")))?,
                    None => deactivate_timestamps(&v),
                },
                None,
            ),
        };
        Ok((Outcome::Value(value), why))
    }

    /// The fields `form` (names substituted) refers to from field `col`
    /// of line `line`: `@` references and ranges, `$N` and `$N..$M` in
    /// the row; references to other tables are left out.
    pub fn references(&self, form: &str, line: usize, col: usize) -> Vec<(usize, usize)> {
        let mut cells = Vec::new();
        let mut add = |l: usize, c: usize| {
            if !cells.contains(&(l, c)) {
                cells.push((l, c));
            }
        };
        let b = form.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if form[i..].starts_with("remote(") {
                i = form[i..].find(')').map_or(b.len(), |j| i + j + 1);
                continue;
            }
            if b[i] == b'@' {
                let m = match_range(form, i);
                if m.len > 1 {
                    if let Ok((r1, c1, r2, c2, range)) =
                        self.corners(&form[i..i + m.len], line, col)
                    {
                        let rows = self.table.rows.len();
                        if !range || (r1 == r2 && c1 == c2) {
                            let mut l = r1;
                            while l < rows && !self.is_data(l) {
                                l += 1;
                            }
                            add(l, c1);
                        } else {
                            let (first, last) = (r1.min(r2), r1.max(r2));
                            for l in first..=last.min(rows.saturating_sub(1)) {
                                if self.is_data(l) {
                                    for c in c1.min(c2)..=c1.max(c2) {
                                        add(l, c);
                                    }
                                }
                            }
                        }
                    }
                    i += m.len;
                    continue;
                }
            }
            if b[i] == b'$' {
                if let Some((s, e, n1, n2)) =
                    simple_range(&form[i..]).filter(|(s, _, _, _)| *s == 0)
                {
                    let at = |r: ColRef| r.value + if r.relative { col as i64 } else { 0 };
                    for c in at(n1)..=at(n2) {
                        if c > 0 {
                            add(line, c as usize);
                        }
                    }
                    i += e - s;
                    continue;
                }
                if let Some((e, r)) = col_ref_at(form, i) {
                    let n = r.value + if r.relative { col as i64 } else { 0 };
                    let c = if n == 0 { col as i64 } else { n.max(1) };
                    add(line, c as usize);
                    i = e;
                    continue;
                }
            }
            i += form[i..].chars().next().map_or(1, char::len_utf8);
        }
        cells
    }

    /// `org-table-remote-reference-indirection`: `remote($1, …)` takes the
    /// table name from a field.
    fn remote_indirection(&self, form: &str, line: usize, col: usize) -> Result<String, Error> {
        let mut out = form.to_string();
        let mut start = 0;
        while let Some(i) = find_remote(&out, start) {
            let open = i + "remote(".len();
            let rest = &out[open..];
            let lead = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            let arg = &rest[lead..];
            if arg.starts_with(['@', '$']) {
                let len = arg.find([' ', '\t', ',']).unwrap_or(arg.len());
                let r = &arg[..len];
                if arg[len..].trim_start_matches([' ', '\t']).starts_with(',') {
                    let eq = first_last(r, &self.analysis)?;
                    let eq = if eq.starts_with('$') && eq[1..].bytes().all(|c| c.is_ascii_digit()) {
                        format!("@0{eq}")
                    } else {
                        eq
                    };
                    let v = match self.get_range(&eq, line, col)? {
                        Value::Field(x) => x,
                        Value::Range(xs) => xs.join(" "),
                    };
                    let s = open + lead;
                    out.replace_range(s..s + len, &v);
                }
            }
            start = open;
        }
        Ok(out)
    }

    /// `remote(NAME, REF)` replaced by the referenced fields.
    fn remotes(
        &self,
        form: &str,
        keep_empty: bool,
        numbers: bool,
        convert: &dyn Fn(Value) -> Value,
    ) -> Result<String, Error> {
        let mut out = form.to_string();
        while let Some(i) = find_remote(&out, 0) {
            let open = i + "remote(".len();
            let Some(close) = out[open..].find(')').map(|j| open + j) else {
                break;
            };
            let inner = &out[open..close];
            let Some((name, reference)) = inner.split_once(',') else {
                break;
            };
            let name = name.trim_matches([' ', '\t']).to_string();
            let reference = reference.trim_matches([' ', '\t']).to_string();
            if name.is_empty() || reference.is_empty() || reference.contains('\n') {
                break;
            }
            let Some(table) = self.env.remote.table(&name) else {
                return err(format!("Can't find remote table \"{name}\""));
            };
            let other = Evaluator {
                analysis: Analysis::of(&table),
                table,
                env: self.env,
            };
            let r = other.substitute_names(&first_last(&reference, &other.analysis)?);
            let v = if r.find('@').is_some_and(|at| match_range(&r, at).len > 1) {
                let at = r.find('@').expect("an @");
                let m = match_range(&r, at);
                other.get_range(&r[at..at + m.len], 0, 1)?
            } else {
                Value::Field(r)
            };
            let rep = make_reference(&convert(v), keep_empty, numbers, Lisp::No);
            out.replace_range(i..=close, &rep);
        }
        Ok(out)
    }
}

/// `\<remote(` from `start`.
fn find_remote(s: &str, start: usize) -> Option<usize> {
    let mut from = start;
    while let Some(j) = s[from..].find("remote(") {
        let i = from + j;
        if i == 0
            || !s[..i]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            return Some(i);
        }
        from = i + 1;
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct ColRef {
    value: i64,
    relative: bool,
}

/// `\$\(\([-+]\)?[0-9]+\)` at `i`: end and the reference.
fn col_ref_at(s: &str, i: usize) -> Option<(usize, ColRef)> {
    let b = s.as_bytes();
    if b.get(i) != Some(&b'$') {
        return None;
    }
    let mut j = i + 1;
    let relative = matches!(b.get(j), Some(b'-' | b'+'));
    if relative {
        j += 1;
    }
    let d = b[j.min(b.len())..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if d == 0 {
        return None;
    }
    let value: i64 = s[i + 1..j + d].parse().unwrap_or(i64::MAX);
    Some((j + d, ColRef { value, relative }))
}

/// The first `$N..$M` in `s`.
fn simple_range(s: &str) -> Option<(usize, usize, ColRef, ColRef)> {
    let b = s.as_bytes();
    (0..b.len()).find_map(|i| {
        let (e1, r1) = col_ref_at(s, i)?;
        if !s[e1..].starts_with("..") {
            return None;
        }
        let (e2, r2) = col_ref_at(s, e1 + 2)?;
        Some((i, e2, r1, r2))
    })
}

/// The first `$N` in `s`.
fn field_ref(s: &str) -> Option<(usize, usize, ColRef)> {
    (0..s.len()).find_map(|i| col_ref_at(s, i).map(|(e, r)| (i, e, r)))
}

/// `^[0-9]+:[0-9]+\(?::[0-9]+\)?$`
fn is_hhmm(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    (parts.len() == 2 || parts.len() == 3)
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

/// Timestamps as Calc reads them: inactive ones made active, and each
/// rewritten `<%Y-%m-%d %a>` (with ` %H:%M` if it has a time) in the C
/// locale, as `org-table-eval-formula` does.
fn activate_timestamps(s: &str) -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let mut out = s.to_string();
    let mut from = 0;
    while let Some((a, b, _)) = timestamp_at(&out, from) {
        let inner = &out[a + 1..b - 1];
        let (y, m, d) = (&inner[..4], &inner[5..7], &inner[8..10]);
        let day = match (y.parse(), m.parse(), d.parse()) {
            (Ok(y), Ok(m), Ok(d)) => crate::calc::date::day_number(y, m, d),
            _ => {
                from = b;
                continue;
            }
        };
        let wd = WEEKDAYS[day.rem_euclid(7) as usize];
        // The first `H:MM` or `HH:MM`.
        let time = inner[10..].char_indices().find_map(|(i, _)| {
            let t = &inner[10 + i..];
            let hd = t.bytes().take_while(u8::is_ascii_digit).count();
            let before = inner[..10 + i].chars().next_back();
            ((1..=2).contains(&hd)
                && !before.is_some_and(|c| c.is_ascii_digit())
                && t.as_bytes().get(hd) == Some(&b':')
                && t.as_bytes()
                    .get(hd + 1..hd + 3)
                    .is_some_and(|m| m.iter().all(u8::is_ascii_digit)))
            .then(|| {
                (
                    t[..hd].parse::<u32>().unwrap_or(0),
                    t[hd + 1..hd + 3].to_string(),
                )
            })
        });
        let rep = match time {
            Some((h, mi)) => format!("<{y}-{m}-{d} {wd} {h:02}:{mi}>"),
            None => format!("<{y}-{m}-{d} {wd}>"),
        };
        let len = rep.len();
        out.replace_range(a..b, &rep);
        from = a + len;
    }
    out
}

/// `<2026-01-01 Thu>` as `[2026-01-01 Thu]`: dates in tables are data,
/// not appointments.
fn deactivate_timestamps(s: &str) -> String {
    let mut out = s.to_string();
    let mut from = 0;
    while let Some((a, b, active)) = timestamp_at(&out, from) {
        if active {
            out.replace_range(a..a + 1, "[");
            out.replace_range(b - 1..b, "]");
        }
        from = b;
    }
    out
}

/// The Calc modes of a formula's flags.
fn modes(f: &Flags) -> Modes {
    let mut m = Modes::default();
    if let Some(p) = f.precision {
        m.prec = Prec {
            digits: p.clamp(3, 1000),
            ..m.prec
        };
    }
    if f.prefer_frac {
        m.prec.prefer_frac = true;
    }
    if let Some(ff) = f.float_format {
        m.display = match ff {
            FloatFormat::Float(n) => Display::Float(n),
            FloatFormat::Fix(n) => Display::Fix(n),
            FloatFormat::Sci(n) => Display::Sci(n),
            FloatFormat::Eng(n) => Display::Eng(n),
        };
    }
    if let Some(a) = f.angle {
        m.degrees = matches!(a, crate::tblfm::Angle::Degrees);
    }
    m
}
