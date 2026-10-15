//! Values as Babel passes them between a block's language and Org, as
//! Emacs Lisp holds them (`org-babel-read`, `org-babel-script-escape`):
//! strings, integers, floats, symbols (`hline`, Python's `True`) and
//! lists, a table being a list of rows; printed as `princ` and `prin1`
//! print them.

/// A Lisp value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Symbol(String),
    /// A list; `nil` is the empty one.
    List(Vec<Value>),
}

impl Value {
    /// `nil`.
    pub fn nil() -> Value {
        Value::List(Vec::new())
    }

    /// Whether it is `nil`.
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::List(v) if v.is_empty())
    }

    /// Whether it is the symbol `hline`, a table's rule.
    pub fn is_hline(&self) -> bool {
        matches!(self, Value::Symbol(s) if s == "hline")
    }

    /// `princ`'s text (`%s`): a string as it is.
    pub fn princ(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::List(v) => list_text(v, Value::princ),
            other => other.prin1(),
        }
    }

    /// `prin1`'s text (`%S`): a string quoted.
    pub fn prin1(&self) -> String {
        match self {
            Value::Str(s) => {
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
            Value::Int(i) => i.to_string(),
            Value::Float(f) => float_text(*f),
            Value::Symbol(s) => s.clone(),
            Value::List(v) => list_text(v, Value::prin1),
        }
    }
}

fn list_text(v: &[Value], each: fn(&Value) -> String) -> String {
    if v.is_empty() {
        return "nil".into();
    }
    let items: Vec<String> = v.iter().map(each).collect();
    format!("({})", items.join(" "))
}

/// Emacs's text of a float (`float_to_string`): the shortest of `%.15g`,
/// `%.16g` and `%.17g` that reads back as it, `.0` added where it has
/// neither a point nor an exponent.
pub fn float_text(f: f64) -> String {
    if f.is_nan() {
        return if f.is_sign_negative() {
            "-0.0e+NaN".into()
        } else {
            "0.0e+NaN".into()
        };
    }
    if f.is_infinite() {
        return if f < 0.0 {
            "-1.0e+INF".into()
        } else {
            "1.0e+INF".into()
        };
    }
    let mut s = String::new();
    for p in 15..=17 {
        s = format_g(f, p);
        if s.parse::<f64>().ok() == Some(f) {
            break;
        }
    }
    if !s.contains(['.', 'e']) {
        s.push_str(".0");
    }
    s
}

/// C's `%.{p}g`.
fn format_g(f: f64, p: usize) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let sci = format!("{:.*e}", p - 1, f);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < -4 || exp >= p as i32 {
        format!(
            "{}e{}{:02}",
            strip_zeros(mantissa),
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip_zeros(&format!("{f:.decimals$}"))
    }
}

fn strip_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// The Lisp reader's number in `t`, whole: `12`, `-3.` (an integer),
/// `1.5`, `.5`, `1e3`.
fn lisp_number(t: &str) -> Option<Value> {
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let negative = t.starts_with('-');
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // An integer, a trailing point allowed.
    let int_part = body.strip_suffix('.').unwrap_or(body);
    if digits(int_part) {
        return Some(match int_part.parse::<i64>() {
            Ok(i) => Value::Int(if negative { -i } else { i }),
            Err(_) => Value::Float(t.trim_end_matches('.').parse().ok()?),
        });
    }
    // A float: digits, a point and digits, or an exponent.
    let (mantissa, exponent) = match body.split_once('e') {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['+', '-']).unwrap_or(e)));
    let mantissa_ok = match mantissa.split_once('.') {
        Some((a, b)) => (a.is_empty() || digits(a)) && digits(b),
        None => digits(mantissa) && exponent.is_some(),
    };
    if exponent_ok && mantissa_ok {
        return t.parse::<f64>().ok().map(Value::Float);
    }
    None
}

/// `org-babel--string-to-number`: the number `s` writes, if it is one.
pub fn string_to_number(s: &str) -> Option<Value> {
    let t = s.trim();
    if t.chars().any(char::is_whitespace)
        || s.is_empty()
        || !s
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 'e' | '.' | '+' | ' ' | '-'))
    {
        return None;
    }
    lisp_number(t)
}

/// A Lisp string literal `s` (from its opening quote) read: its value
/// and where it ends.
fn lisp_string(s: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, i + 1)),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\n' => {}
                o => out.push(o),
            },
            o => out.push(o),
        }
    }
    None
}

/// `org-babel-read` with Lisp evaluation inhibited: a number, a string in
/// double quotes read, anything else as it is.
pub fn read(cell: &str) -> Value {
    if cell.trim().is_empty() {
        return Value::Str(cell.to_string());
    }
    if let Some(n) = string_to_number(cell) {
        return n;
    }
    let t = cell.trim_start_matches([' ', '\t', '\n']);
    if t.starts_with('"')
        && let Some((value, end)) = lisp_string(t)
        && t[end..].trim().is_empty()
    {
        return Value::Str(value);
    }
    Value::Str(cell.to_string())
}

/// `org-babel-string-read`: one pair of double quotes around `cell` taken
/// off, then [`read`].
pub fn string_read(cell: &str) -> Value {
    let t = cell.trim();
    if t.len() > 2 && t.starts_with('"') && t.ends_with('"') {
        return read(&t[1..t.len() - 1]);
    }
    read(cell)
}

/// A Python (or JavaScript, Ruby) literal as `org-babel-script-escape`
/// and `org-babel-read` take it: a list or tuple a list of its items, a
/// quoted string its text, a number a number, a bare word a symbol, and
/// anything else the text as it is.
pub fn read_script(s: &str) -> Value {
    let t = s.trim();
    let bracketed = t.len() >= 2
        && matches!(
            (t.as_bytes()[0], t.as_bytes()[t.len() - 1]),
            (b'[', b']') | (b'(', b')') | (b'{', b'}')
        );
    let quoted = t.len() > 2
        && matches!(
            (t.as_bytes()[0], t.as_bytes()[t.len() - 1]),
            (b'\'', b'\'') | (b'"', b'"')
        );
    if bracketed || quoted {
        let mut p = Script { s: t, at: 0 };
        if let Some(v) = p.value()
            && p.rest_is_blank()
        {
            return v;
        }
        return Value::Str(s.to_string());
    }
    // Not escaped: `org-babel-read`, which reads a number.
    match string_to_number(s) {
        Some(n) => n,
        None => Value::Str(s.to_string()),
    }
}

/// A reader of script literals.
struct Script<'a> {
    s: &'a str,
    at: usize,
}

impl Script<'_> {
    fn blank(&mut self) {
        while let Some(c) = self.s[self.at..].chars().next() {
            if c.is_whitespace() || c == ',' {
                self.at += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn rest_is_blank(&mut self) -> bool {
        self.blank();
        self.at == self.s.len()
    }

    fn value(&mut self) -> Option<Value> {
        self.blank();
        let c = self.s[self.at..].chars().next()?;
        match c {
            '[' | '(' | '{' => {
                let close = match c {
                    '[' => ']',
                    '(' => ')',
                    _ => '}',
                };
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.blank();
                    let next = self.s[self.at..].chars().next()?;
                    if next == close {
                        self.at += 1;
                        return Some(Value::List(items));
                    }
                    // A dictionary's colon separates as a comma does.
                    if next == ':' {
                        self.at += 1;
                        items.push(Value::Symbol(":".into()));
                        continue;
                    }
                    items.push(self.value()?);
                }
            }
            '\'' | '"' => {
                let mut out = String::new();
                let mut chars = self.s[self.at..].char_indices().skip(1);
                while let Some((i, ch)) = chars.next() {
                    if ch == c {
                        self.at += i + 1;
                        return Some(Value::Str(out));
                    }
                    if ch == '\\' {
                        match chars.next()?.1 {
                            'n' => out.push('\n'),
                            't' => out.push('\t'),
                            o => out.push(o),
                        }
                    } else {
                        out.push(ch);
                    }
                }
                None
            }
            _ => {
                let start = self.at;
                while let Some(ch) = self.s[self.at..].chars().next() {
                    if ch.is_whitespace()
                        || matches!(ch, ',' | ']' | ')' | '}' | '[' | '(' | '{' | ':')
                    {
                        break;
                    }
                    self.at += ch.len_utf8();
                }
                let word = &self.s[start..self.at];
                if word.is_empty() {
                    return None;
                }
                Some(lisp_number(word).unwrap_or_else(|| Value::Symbol(word.to_string())))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_emacs_reads_and_prints_them() {
        assert_eq!(string_to_number("42"), Some(Value::Int(42)));
        assert_eq!(string_to_number(" -7 "), Some(Value::Int(-7)));
        assert_eq!(string_to_number("3."), Some(Value::Int(3)));
        assert_eq!(string_to_number("1.5"), Some(Value::Float(1.5)));
        assert_eq!(string_to_number(".5"), Some(Value::Float(0.5)));
        assert_eq!(string_to_number("1e3"), Some(Value::Float(1000.0)));
        assert_eq!(string_to_number("1 2"), None);
        assert_eq!(string_to_number("e"), None);
        assert_eq!(string_to_number("1x"), None);
        assert_eq!(float_text(1.0), "1.0");
        assert_eq!(float_text(3.5), "3.5");
        assert_eq!(float_text(0.1), "0.1");
        assert_eq!(float_text(1e21), "1e+21");
        assert_eq!(float_text(1e-5), "1e-05");
        assert_eq!(float_text(123456.0), "123456.0");
    }

    #[test]
    fn cells_and_strings() {
        assert_eq!(read("hello"), Value::Str("hello".into()));
        assert_eq!(read("\"a \\\"b\\\"\""), Value::Str("a \"b\"".into()));
        assert_eq!(string_read("\"5\""), Value::Int(5));
        assert_eq!(Value::Str("a\"b".into()).prin1(), "\"a\\\"b\"");
        assert_eq!(
            Value::List(vec![Value::Int(1), Value::Str("x".into())]).princ(),
            "(1 x)"
        );
    }

    #[test]
    fn python_literals() {
        assert_eq!(
            read_script("[[1, 2], [3, 4]]"),
            Value::List(vec![
                Value::List(vec![Value::Int(1), Value::Int(2)]),
                Value::List(vec![Value::Int(3), Value::Int(4)]),
            ])
        );
        assert_eq!(
            read_script("[1, 'two', 3.5, None]"),
            Value::List(vec![
                Value::Int(1),
                Value::Str("two".into()),
                Value::Float(3.5),
                Value::Symbol("None".into()),
            ])
        );
        assert_eq!(read_script("'hi'"), Value::Str("hi".into()));
        assert_eq!(read_script("2"), Value::Int(2));
        assert_eq!(read_script("hi"), Value::Str("hi".into()));
        assert_eq!(read_script("[1, 2"), Value::Str("[1, 2".into()));
    }
}
