//! Calc's algebraic notation (`math-read-exprs` in `calc-aent.el`): a
//! precedence-climbing (Pratt) parser over Calc's operator table, with
//! Calc's quirks: `*` and implicit multiplication bind tighter than `/`,
//! `%` is a percentage or a remainder depending on what follows, `(a, b)`
//! is a complex number, `[1 2]` is a vector.

use super::expr::Expr;
use super::num::{self, Prec};

/// A syntax error: the position in the text and Calc's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    /// Byte offset of the token Calc stopped at.
    pub pos: usize,
    /// The message.
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    Number,
    Symbol,
    Dollar,
    Hash,
    Str,
    Punc,
    Space,
    End,
}

/// An operator of `math-standard-opers`, with multiplication binding
/// tighter than division (`calc-multiplication-has-precedence`).
#[derive(Debug, Clone, Copy)]
struct Op {
    key: &'static str,
    func: &'static str,
    left: i32,
    right: i32,
}

const fn op(key: &'static str, func: &'static str, left: i32, right: i32) -> Op {
    Op {
        key,
        func,
        left,
        right,
    }
}

const OPS: &[Op] = &[
    op("*", "*", 196, 195),
    op("2x", "*", 196, 195),
    op("_", "subscr", 1200, 1201),
    op("%", "percent", 1100, -1),
    op("u!", "lnot", -1, 1000),
    op("mod", "makemod", 400, 400),
    op("+/-", "sdev", 300, 300),
    op("!!", "dfact", 210, -1),
    op("!", "fact", 210, -1),
    op("^", "^", 201, 200),
    op("**", "^", 201, 200),
    op("u+", "ident", -1, 197),
    op("u-", "neg", -1, 197),
    op("/", "/", 190, 191),
    op("%", "%", 190, 191),
    op("\\", "idiv", 190, 191),
    op("+", "+", 180, 181),
    op("-", "-", 180, 181),
    op("|", "|", 170, 171),
    op("<", "lt", 160, 161),
    op(">", "gt", 160, 161),
    op("<=", "leq", 160, 161),
    op(">=", "geq", 160, 161),
    op("=", "eq", 160, 161),
    op("==", "eq", 160, 161),
    op("!=", "neq", 160, 161),
    op("&&", "land", 110, 111),
    op("||", "lor", 100, 101),
    op("?", "if", 91, 90),
    op("!!!", "pnot", -1, 85),
    op("&&&", "pand", 80, 81),
    op("|||", "por", 75, 76),
    op(":=", "assign", 51, 50),
    op("::", "condition", 45, 46),
    op("=>", "evalto", 40, 41),
    op("=>", "evalto", 40, -1),
];

/// The first operator with `key` at or after index `from`.
fn find_op(key: &str, from: usize) -> Option<(usize, Op)> {
    OPS.iter()
        .enumerate()
        .skip(from)
        .find(|(_, o)| o.key == key)
        .map(|(i, o)| (i, *o))
}

/// Multi-character punctuation, in the order Calc tries it.
const PUNCS: &[&str] = &[
    "~=", "<=", ">=", "<>", "/=", "+/-", "\\dots", "\\ldots", "**", "<<", ">>", "==", "!=", "&&&",
    "|||", "!!!", "&&", "||", "!!", ":=", "::", "=>",
];

fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic() || ('α'..='ω').contains(&c) || ('Α'..='Ω').contains(&c)
}

#[derive(Debug, Clone)]
struct State {
    pos: usize,
    old_pos: usize,
    tok: Tok,
    data: String,
}

struct Parser<'a> {
    s: &'a str,
    st: State,
    keep_spaces: bool,
    prec: Prec,
}

type Res<T> = Result<T, &'static str>;

impl Parser<'_> {
    fn ch(&self, at: usize) -> Option<char> {
        self.s[at..].chars().next()
    }

    /// `math-read-token`.
    fn next(&mut self) {
        let s = self.s;
        loop {
            let pos = self.st.pos;
            self.st.old_pos = pos;
            let Some(ch) = self.ch(pos) else {
                self.st.tok = Tok::End;
                self.st.data = "\0".into();
                return;
            };
            if matches!(ch, ' ' | '\n' | '\t') {
                self.st.pos += 1;
                if self.keep_spaces {
                    self.st.tok = Tok::Space;
                    self.st.data = " ".into();
                    return;
                }
                continue;
            }
            if is_letter(ch) {
                let len: usize = s[pos..]
                    .char_indices()
                    .take_while(|&(i, c)| {
                        if i == 0 {
                            true
                        } else {
                            is_letter(c) || c.is_ascii_digit() || c == '\'' || c == '#'
                        }
                    })
                    .map(|(_, c)| c.len_utf8())
                    .sum();
                self.st.tok = Tok::Symbol;
                // `math-restore-dashes`.
                self.st.data = s[pos..pos + len].replace(['#', '_'], "-");
                self.st.pos = pos + len;
                return;
            }
            let rest = &s[pos..];
            let digit_after = |i: usize| rest.as_bytes().get(i).is_some_and(u8::is_ascii_digit);
            let number_start = ch.is_ascii_digit()
                || (ch == '.' && digit_after(1))
                || (ch == '_'
                    && (digit_after(1)
                        || (rest.as_bytes().get(1) == Some(&b'.') && digit_after(2)))
                    && (pos == 0
                        || !s[..pos].chars().next_back().is_some_and(|p| {
                            matches!(p, ']' | ')' | '}' | '"' | '\'' | '$')
                                || is_letter(p)
                                || p.is_ascii_digit()
                            // `[^])}"a-zA-Z0-9'$]_`
                        })));
            if number_start {
                let len = number_len(rest);
                self.st.tok = Tok::Number;
                self.st.data = rest[..len].to_string();
                self.st.pos = pos + len;
                return;
            }
            if ch == '$' {
                let digits = rest[1..].bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 && rest.as_bytes()[1] != b'0' {
                    self.st.pos = pos + 1 + digits;
                } else {
                    self.st.pos = pos + rest.bytes().take_while(|&c| c == b'$').count();
                }
                self.st.tok = Tok::Dollar;
                self.st.data = "$".into();
                return;
            }
            if ch == '#' {
                let digits = rest[1..].bytes().take_while(u8::is_ascii_digit).count();
                self.st.pos = if digits > 0 && rest.as_bytes()[1] != b'0' {
                    pos + 1 + digits
                } else {
                    pos + 1
                };
                self.st.tok = Tok::Hash;
                self.st.data = "#".into();
                return;
            }
            if let Some(p) = PUNCS.iter().find(|p| rest.starts_with(**p)) {
                self.st.tok = Tok::Punc;
                self.st.data = (*p).to_string();
                self.st.pos = pos + p.len();
                return;
            }
            if ch == '"' {
                // `"\\([^"\\]\\|\\\\.\\)*\\(\"\\|\\'\\)`
                let mut i = 1;
                let b = rest.as_bytes();
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' && i + 1 < b.len() {
                        2
                    } else {
                        1
                    };
                }
                let body_end = i.min(b.len());
                self.st.tok = Tok::Str;
                self.st.data = rest[1..body_end].to_string();
                self.st.pos = pos + (i + 1).min(b.len());
                return;
            }
            if rest.starts_with("%%") {
                self.st.pos = pos + rest.find('\n').unwrap_or(rest.len());
                continue;
            }
            self.st.tok = Tok::Punc;
            self.st.data = ch.to_string();
            self.st.pos = pos + ch.len_utf8();
            return;
        }
    }

    fn data_is(&self, s: &str) -> bool {
        self.st.data == s
    }

    /// `math-read-expr-list`.
    fn expr_list(&mut self) -> Res<Vec<Expr>> {
        let saved = self.keep_spaces;
        self.keep_spaces = false;
        let mut v = vec![self.level(0, None)?];
        while self.data_is(",") {
            self.next();
            v.push(self.level(0, None)?);
        }
        self.keep_spaces = saved;
        Ok(v)
    }

    /// `math-factor-after`: whether a factor follows the current token.
    fn factor_after(&mut self) -> bool {
        let saved = self.st.clone();
        self.next();
        let t = self.st.tok;
        let d = self.st.data.clone();
        self.st = saved;
        matches!(
            t,
            Tok::Number | Tok::Symbol | Tok::Dollar | Tok::Hash | Tok::Str
        ) || (matches!(d.as_str(), "-" | "+" | "!" | "|" | "/")
            && find_op(&format!("u{d}"), 0).is_some())
            || find_op(&d, 0).is_some_and(|(_, o)| o.left == -1)
            || matches!(d.as_str(), "(" | "[" | "{")
    }

    /// `math-read-expr-level`.
    fn level(&mut self, prec: i32, term: Option<&str>) -> Res<Expr> {
        let mut x = self.factor()?;
        let mut first = true;
        loop {
            let found = find_op(&self.st.data, 0);
            let mut op = None;
            if let Some((i, o)) = found
                && o.left != -1
            {
                let mut chosen = o;
                if let Some((_, o2)) = find_op(&self.st.data, i + 1)
                    && (o.right == -1) == (o2.right != -1)
                    && (o2.right == -1) == !self.factor_after()
                {
                    chosen = o2;
                }
                op = Some(chosen);
            } else if (found.is_some_and(|(_, o)| o.left == -1)
                || matches!(
                    self.st.tok,
                    Tok::Symbol | Tok::Number | Tok::Dollar | Tok::Hash
                )
                || self.data_is("(")
                || (self.data_is("[") && !(self.keep_spaces && matches!(x, Expr::Vec(_)))))
                && found.is_none_or(|(_, o)| o.left != -1)
            {
                op = find_op("2x", 0).map(|(_, o)| o);
            }
            let Some(o) = op else { break };
            if term.is_some_and(|t| self.data_is(t)) || o.left < prec {
                break;
            }
            if o.key != "2x" {
                self.next();
            }
            x = if o.func == "if" {
                // `math-read-if`.
                let then = self.level(0, None)?;
                if !self.data_is(":") {
                    return Err("Expected `:'");
                }
                self.next();
                let otherwise = self.level(o.right, None)?;
                Expr::call("if", vec![x, then, otherwise])
            } else if o.right == -1 {
                if o.func == "ident" {
                    x
                } else {
                    Expr::call(o.func, vec![x])
                }
            } else if !first && is_inequality(o.func) && is_inequality_expr(&x) {
                // `math-composite-inequalities`: `a < b < c`.
                let y = self.level(o.right, term)?;
                composite(x, o.func, y)
            } else {
                let y = self.level(o.right, term)?;
                Expr::call(o.func, vec![x, y])
            };
            first = false;
        }
        Ok(x)
    }

    /// `math-read-factor`.
    fn factor(&mut self) -> Res<Expr> {
        match self.st.tok {
            Tok::Number => {
                let Some(n) = num::read(&self.st.data, &self.prec) else {
                    self.st.old_pos = self.st.pos;
                    return Err("Bad format");
                };
                self.next();
                return Ok(Expr::Num(n));
            }
            Tok::Symbol => return self.symbol(),
            Tok::Dollar => {
                return Err("$'s not allowed in this context");
            }
            Tok::Hash => return Err("#'s not allowed in this context"),
            Tok::Str => {
                let s = std::mem::take(&mut self.st.data);
                self.next();
                return Ok(Expr::Str(unescape(&s)));
            }
            _ => {}
        }
        if matches!(self.st.data.as_str(), "-" | "+" | "!" | "|" | "/") {
            self.st.data = format!("u{}", self.st.data);
            return self.factor();
        }
        if let Some((_, o)) = find_op(&self.st.data, 0)
            && o.left == -1
        {
            self.next();
            let val = self.level(o.right, None)?;
            return Ok(match o.func {
                "ident" => val,
                "neg" => match val {
                    Expr::Num(n) => Expr::Num(n.neg()),
                    v => Expr::call("neg", vec![v]),
                },
                f => Expr::call(f, vec![val]),
            });
        }
        if self.data_is("(") {
            let saved = self.keep_spaces;
            self.keep_spaces = false;
            self.next();
            let mut exp = if self.data_is("\\dots") || self.data_is("\\ldots") {
                Expr::call("neg", vec![Expr::Var("inf".into())])
            } else {
                self.level(0, None)?
            };
            if self.data_is(",") {
                self.next();
                let im = self.level(0, None)?;
                exp = match (&exp, &im) {
                    (Expr::Num(_), Expr::Num(_)) => Expr::call("cplx", vec![exp, im]),
                    _ => Expr::call(
                        "+",
                        vec![exp, Expr::call("*", vec![im, Expr::Var("i".into())])],
                    ),
                };
            } else if self.data_is(";") {
                self.next();
                let arg = self.level(0, None)?;
                exp = Expr::call("polar", vec![exp, arg]);
            } else if self.data_is("\\dots") || self.data_is("\\ldots") {
                self.next();
                let hi = if self.data_is(")") || self.data_is("]") || self.st.tok == Tok::End {
                    Expr::Var("inf".into())
                } else {
                    self.level(0, None)?
                };
                let closed = if self.data_is(")") { 0 } else { 1 };
                exp = Expr::Intv(closed, Box::new(exp), Box::new(hi));
            }
            self.keep_spaces = saved;
            if !(self.data_is(")")
                || (self.data_is("]") && matches!(exp, Expr::Intv(..)))
                || self.st.tok == Tok::End)
            {
                return Err("Expected `)'");
            }
            self.next();
            return Ok(exp);
        }
        if self.data_is("[") {
            return self.brackets(true, "]");
        }
        if self.data_is("{") {
            return self.brackets(false, "}");
        }
        if self.data_is("<") {
            // `math-read-angle-brackets`, for the timestamps Org writes.
            return match super::date::read(&self.s[self.st.pos..], &self.prec) {
                Some((d, len)) => {
                    self.st.pos += len;
                    self.next();
                    Ok(Expr::Date(d))
                }
                None => Err("Bad format"),
            };
        }
        Err("Expected a number")
    }

    fn symbol(&mut self) -> Res<Expr> {
        let name = std::mem::take(&mut self.st.data);
        self.next();
        if self.data_is("(") {
            self.next();
            let args = if self.data_is(")") || self.st.tok == Tok::End {
                Vec::new()
            } else {
                self.expr_list()?
            };
            if !(self.data_is(")") || self.st.tok == Tok::End) {
                return Err("Expected `)'");
            }
            self.next();
            return Ok(Expr::call(&name, args));
        }
        Ok(Expr::Var(name))
    }

    /// `math-read-brackets`.
    fn brackets(&mut self, space_sep: bool, close: &'static str) -> Res<Expr> {
        let space_sep = space_sep && !self.commas_ahead();
        self.next();
        while self.st.tok == Tok::Space {
            self.next();
        }
        if self.data_is(close) || self.st.tok == Tok::End {
            self.next();
            return Ok(Expr::Vec(Vec::new()));
        }
        let saved = self.st.clone();
        let saved_keep = self.keep_spaces;
        let first = if self.data_is("\\dots") || self.data_is("\\ldots") {
            Ok(vec![Expr::call("neg", vec![Expr::Var("inf".into())])])
        } else {
            self.keep_spaces = space_sep;
            let r = self.vector(close);
            self.keep_spaces = saved_keep;
            r
        };
        let mut vals = match first {
            Ok(v) => v,
            Err(e) if space_sep => {
                // Again without spaces as separators.
                let err_state = self.st.clone();
                self.st = saved;
                self.keep_spaces = false;
                let r = self.vector(close);
                self.keep_spaces = saved_keep;
                match r {
                    Ok(v)
                        if matches!(self.st.data.as_str(), "\\ldots" | "\\dots" | ";")
                            || self.data_is(close)
                            || self.st.tok == Tok::End =>
                    {
                        v
                    }
                    _ => {
                        self.st = err_state;
                        return Err(e);
                    }
                }
            }
            Err(e) => return Err(e),
        };
        if self.data_is("\\dots") || self.data_is("\\ldots") {
            self.next();
            let lo = if vals.len() > 1 {
                Expr::call("mul", vals)
            } else {
                vals.pop().expect("one value")
            };
            let hi = if self.data_is(close) || self.data_is(")") || self.st.tok == Tok::End {
                Expr::Var("inf".into())
            } else {
                self.level(0, None)?
            };
            let closed = if self.data_is(")") { 2 } else { 3 };
            if !(self.data_is(close) || self.data_is(")") || self.st.tok == Tok::End) {
                return Err("Expected `]'");
            }
            if self.st.tok != Tok::End {
                self.next();
            }
            return Ok(Expr::Intv(closed, Box::new(lo), Box::new(hi)));
        }
        if self.data_is(";") {
            // A matrix: rows separated by `;`.
            let mut rows = vec![Expr::Vec(vals)];
            while self.data_is(";") {
                self.next();
                while self.st.tok == Tok::Space {
                    self.next();
                }
                self.keep_spaces = space_sep;
                let r = self.vector(close);
                self.keep_spaces = saved_keep;
                rows.push(Expr::Vec(r?));
            }
            vals = rows;
        }
        if !(self.data_is(close) || self.st.tok == Tok::End) {
            return Err("Expected `]'");
        }
        if self.st.tok != Tok::End {
            self.next();
        }
        Ok(Expr::Vec(vals))
    }

    /// `math-read-vector`.
    fn vector(&mut self, close: &str) -> Res<Vec<Expr>> {
        let mut v = vec![self.level(0, None)?];
        loop {
            while self.st.tok == Tok::Space {
                self.next();
            }
            if self.st.tok == Tok::End
                || self.data_is(";")
                || self.data_is(close)
                || self.data_is("\\dots")
                || self.data_is("\\ldots")
            {
                break;
            }
            if self.data_is(",") {
                self.next();
            }
            while self.st.tok == Tok::Space {
                self.next();
            }
            v.push(self.level(0, None)?);
        }
        Ok(v)
    }

    /// `math-check-for-commas`: whether a comma follows before the
    /// bracket that closes the current one.
    fn commas_ahead(&self) -> bool {
        let b = self.s.as_bytes();
        let mut depth = 0i32;
        let mut i = self.st.pos;
        while i < b.len() {
            match b[i] {
                b'[' | b'{' | b'(' => depth += 1,
                b']' | b'}' | b')' => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                b',' if depth == 0 => return true,
                _ => {}
            }
            i += 1;
        }
        false
    }
}

/// The length of a number token (`math-read-token`'s regexp, without
/// radix and HMS forms).
fn number_len(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'_'));
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    // `[0-9]+:[0-9:]+`
    let d = digits(i);
    if d > 0 && b.get(i + d) == Some(&b':') {
        let more = b[i + d + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit() || **c == b':')
            .count();
        if more > 0 {
            return i + d + 1 + more;
        }
    }
    // `[0-9.]+\([eE][-+_]?[0-9]+\)?`
    let body = b[i..]
        .iter()
        .take_while(|c| c.is_ascii_digit() || **c == b'.')
        .count();
    i += body;
    if body > 0 && matches!(b.get(i), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(i + 1), Some(b'-' | b'+' | b'_')));
        let ed = digits(i + 1 + sign);
        if ed > 0 {
            i += 1 + sign + ed;
        }
    }
    i
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(n) = it.next() {
                out.push(match n {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_inequality(f: &str) -> bool {
    matches!(f, "lt" | "gt" | "leq" | "geq" | "eq" | "neq")
}

fn is_inequality_expr(x: &Expr) -> bool {
    matches!(x, Expr::Call(f, _) if is_inequality(f))
}

/// `a < b < c` as `a < b && b < c`.
fn composite(x: Expr, func: &str, y: Expr) -> Expr {
    let Expr::Call(_, args) = &x else {
        unreachable!("an inequality")
    };
    let middle = args.last().cloned().unwrap_or(Expr::Vec(Vec::new()));
    Expr::call("land", vec![x, Expr::call(func, vec![middle, y])])
}

/// Reads `text` as `math-read-exprs` does: the comma-separated
/// expressions.
pub fn parse(text: &str, prec: &Prec) -> Result<Vec<Expr>, SyntaxError> {
    // `..` becomes `\dots`, except in runs of three or more dots.
    let mut s = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.'
            && b.get(i + 1) == Some(&b'.')
            && (b.get(i + 2).is_some_and(|&c| c != b'.')
                || (b.get(i + 2).is_some() && b.get(i + 3).is_some_and(|&c| c != b'.')))
        {
            s.push_str("\\dots");
            i += 2;
            continue;
        }
        let c = text[i..].chars().next().expect("a character");
        s.push(c);
        i += c.len_utf8();
    }
    let mut p = Parser {
        s: &s,
        st: State {
            pos: 0,
            old_pos: 0,
            tok: Tok::End,
            data: String::new(),
        },
        keep_spaces: false,
        prec: *prec,
    };
    p.next();
    let r = p.expr_list();
    let pos = p.st.old_pos.min(text.len());
    match r {
        Err(message) => Err(SyntaxError { pos, message }),
        Ok(v) if p.st.tok == Tok::End => Ok(v),
        Ok(_) => Err(SyntaxError {
            pos,
            message: "Syntax error",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(s: &str) -> String {
        match parse(s, &Prec::default()) {
            Ok(v) => v.iter().map(|e| e.to_lisp()).collect::<Vec<_>>().join(" "),
            Err(e) => format!("error {} {}", e.pos, e.message),
        }
    }

    #[test]
    fn precedence() {
        assert_eq!(tree("1/7*7"), "(/ 1 (* 7 7))");
        assert_eq!(tree("1*7/7"), "(/ (* 1 7) 7)");
        assert_eq!(tree("2^3^2"), "(^ 2 (^ 3 2))");
        assert_eq!(tree("-2^2"), "(neg (^ 2 2))");
        assert_eq!(tree("-2*3"), "(* -2 3)");
        assert_eq!(tree("2^-1"), "(^ 2 -1)");
        assert_eq!(tree("1+2*3-4"), "(- (+ 1 (* 2 3)) 4)");
        assert_eq!(tree("(5)(3)"), "(* 5 3)");
        assert_eq!(tree("2 x"), "(* 2 (var x))");
        assert_eq!(tree("a/b c"), "(/ (var a) (* (var b) (var c)))");
        assert_eq!(tree("3%2"), "(% 3 2)");
        assert_eq!(tree("50%"), "(percent 50)");
        assert_eq!(tree("5!"), "(fact 5)");
        assert_eq!(
            tree("1<2 && 3>=2 || !x"),
            "(lor (land (lt 1 2) (geq 3 2)) (lnot (var x)))"
        );
        assert_eq!(tree("x ? 1 : 2"), "(if (var x) 1 2)");
        assert_eq!(tree("if(1>2,3,4)"), "(if (gt 1 2) 3 4)");
    }

    #[test]
    fn factors() {
        assert_eq!(tree("[1,2,3]"), "(vec 1 2 3)");
        assert_eq!(tree("[1 2 3]"), "(vec 1 2 3)");
        assert_eq!(tree("[1 + 2]"), "(vec (+ 1 2))");
        assert_eq!(tree("[1 -2]"), "(vec 1 -2)");
        assert_eq!(tree("[]"), "(vec)");
        assert_eq!(tree("vsum([1,2])"), "(vsum (vec 1 2))");
        assert_eq!(tree("sin(30"), "(sin 30)");
        assert_eq!(tree("(1, 2)"), "(cplx 1 2)");
        assert_eq!(tree("[1..5]"), "(intv 3 1 5)");
        assert_eq!(tree("1:3"), "1:3");
        assert_eq!(tree("_5"), "-5");
        assert_eq!(tree("nan"), "(var nan)");
    }

    #[test]
    fn errors() {
        assert_eq!(tree("1+"), "error 2 Expected a number");
        assert_eq!(tree("$1"), "error 0 $'s not allowed in this context");
        assert_eq!(tree("(1"), "1");
        assert_eq!(tree("1)"), "error 1 Syntax error");
        assert_eq!(tree("1.2.3"), "error 5 Bad format");
    }
}
