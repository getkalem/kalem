//! When-clauses: conditions on the editor's context that decide whether a
//! key binding or a command applies (`editorFocus && inTable`,
//! `editorMode == org`, `!readOnly`).
//!
//! Grammar: `||` and `&&` (with `&&` binding tighter), `!`, parentheses,
//! comparisons `==` and `!=` against a string, number or `true`/`false`,
//! and bare context keys, which are true when set to a truthy value.

use std::collections::HashMap;

/// A value in the context.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A flag.
    Bool(bool),
    /// A string, such as a mode name.
    Str(String),
    /// A number.
    Num(f64),
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Str(s) => !s.is_empty(),
            Value::Num(n) => *n != 0.0,
        }
    }
}

/// The context keys a when-clause is evaluated against.
#[derive(Debug, Clone, Default)]
pub struct Context {
    values: HashMap<String, Value>,
}

impl Context {
    /// Sets a key.
    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        self.values.insert(key.to_string(), value);
        self
    }

    /// Sets a flag.
    pub fn flag(&mut self, key: &str, on: bool) -> &mut Self {
        self.set(key, Value::Bool(on))
    }

    /// A key's value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
}

/// A parsed when-clause.
#[derive(Debug, Clone, PartialEq)]
pub enum WhenClause {
    /// A context key.
    Key(String),
    /// `key == value`.
    Eq(String, Value),
    /// `key != value`.
    Ne(String, Value),
    /// `!e`
    Not(Box<WhenClause>),
    /// `a && b`
    And(Box<WhenClause>, Box<WhenClause>),
    /// `a || b`
    Or(Box<WhenClause>, Box<WhenClause>),
    /// `true` or `false`.
    Const(bool),
}

impl WhenClause {
    /// The strings the clause compares context key `key` with.
    pub fn values(&self, key: &str) -> Vec<String> {
        match self {
            WhenClause::Eq(k, Value::Str(v)) | WhenClause::Ne(k, Value::Str(v)) if k == key => {
                vec![v.clone()]
            }
            WhenClause::Not(e) => e.values(key),
            WhenClause::And(a, b) | WhenClause::Or(a, b) => {
                let mut v = a.values(key);
                v.extend(b.values(key));
                v
            }
            _ => Vec::new(),
        }
    }

    /// Whether the clause looks at context key `key`.
    pub fn mentions(&self, key: &str) -> bool {
        match self {
            WhenClause::Key(k) | WhenClause::Eq(k, _) | WhenClause::Ne(k, _) => k == key,
            WhenClause::Not(e) => e.mentions(key),
            WhenClause::And(a, b) | WhenClause::Or(a, b) => a.mentions(key) || b.mentions(key),
            WhenClause::Const(_) => false,
        }
    }
}

/// A syntax error in a when-clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenError {
    /// What is wrong.
    pub message: String,
    /// Byte offset in the clause.
    pub at: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(f64),
    And,
    Or,
    Not,
    Eq,
    Ne,
    Open,
    Close,
}

fn lex(s: &str) -> Result<Vec<(Tok, usize)>, WhenError> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let two = s.get(i..i + 2);
        match c {
            b' ' | b'\t' => i += 1,
            b'(' => {
                out.push((Tok::Open, i));
                i += 1;
            }
            b')' => {
                out.push((Tok::Close, i));
                i += 1;
            }
            _ if two == Some("&&") => {
                out.push((Tok::And, i));
                i += 2;
            }
            _ if two == Some("||") => {
                out.push((Tok::Or, i));
                i += 2;
            }
            _ if two == Some("==") => {
                out.push((Tok::Eq, i));
                i += 2;
            }
            _ if two == Some("!=") => {
                out.push((Tok::Ne, i));
                i += 2;
            }
            b'!' => {
                out.push((Tok::Not, i));
                i += 1;
            }
            b'\'' | b'"' => {
                let end = s[i + 1..].find(c as char).ok_or(WhenError {
                    message: "unterminated string".into(),
                    at: i,
                })?;
                out.push((Tok::Str(s[i + 1..i + 1 + end].to_string()), i));
                i += end + 2;
            }
            _ if c.is_ascii_digit()
                || (c == b'-' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                let len = s[i + 1..]
                    .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
                    .map_or(s.len() - i, |k| k + 1);
                let n = s[i..i + len].parse().map_err(|_| WhenError {
                    message: "bad number".into(),
                    at: i,
                })?;
                out.push((Tok::Num(n), i));
                i += len;
            }
            _ if c.is_ascii_alphabetic() || c == b'_' => {
                let len = s[i..]
                    .find(|ch: char| {
                        !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-'))
                    })
                    .unwrap_or(s.len() - i);
                out.push((Tok::Ident(s[i..i + len].to_string()), i));
                i += len;
            }
            _ => {
                return Err(WhenError {
                    message: format!("unexpected `{}`", c as char),
                    at: i,
                });
            }
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    len: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }

    fn at(&self) -> usize {
        self.toks.get(self.pos).map_or(self.len, |t| t.1)
    }

    fn or(&mut self) -> Result<WhenClause, WhenError> {
        let mut l = self.and()?;
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            let r = self.and()?;
            l = WhenClause::Or(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<WhenClause, WhenError> {
        let mut l = self.unary()?;
        while self.peek() == Some(&Tok::And) {
            self.pos += 1;
            let r = self.unary()?;
            l = WhenClause::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn unary(&mut self) -> Result<WhenClause, WhenError> {
        match self.peek().cloned() {
            Some(Tok::Not) => {
                self.pos += 1;
                Ok(WhenClause::Not(Box::new(self.unary()?)))
            }
            Some(Tok::Open) => {
                self.pos += 1;
                let e = self.or()?;
                if self.peek() != Some(&Tok::Close) {
                    return Err(WhenError {
                        message: "expected `)`".into(),
                        at: self.at(),
                    });
                }
                self.pos += 1;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                match name.as_str() {
                    "true" => return Ok(WhenClause::Const(true)),
                    "false" => return Ok(WhenClause::Const(false)),
                    _ => {}
                }
                let op = self.peek().cloned();
                if matches!(op, Some(Tok::Eq | Tok::Ne)) {
                    self.pos += 1;
                    let v = match self.peek().cloned() {
                        Some(Tok::Str(s)) => Value::Str(s),
                        Some(Tok::Num(n)) => Value::Num(n),
                        Some(Tok::Ident(i)) if i == "true" || i == "false" => {
                            Value::Bool(i == "true")
                        }
                        // A bare word is a string: `editorMode == org`.
                        Some(Tok::Ident(i)) => Value::Str(i),
                        _ => {
                            return Err(WhenError {
                                message: "expected a value".into(),
                                at: self.at(),
                            });
                        }
                    };
                    self.pos += 1;
                    return Ok(if op == Some(Tok::Eq) {
                        WhenClause::Eq(name, v)
                    } else {
                        WhenClause::Ne(name, v)
                    });
                }
                Ok(WhenClause::Key(name))
            }
            _ => Err(WhenError {
                message: "expected a condition".into(),
                at: self.at(),
            }),
        }
    }
}

impl WhenClause {
    /// Parses a clause.
    pub fn parse(s: &str) -> Result<WhenClause, WhenError> {
        let mut p = Parser {
            toks: lex(s)?,
            pos: 0,
            len: s.len(),
        };
        let e = p.or()?;
        if p.pos != p.toks.len() {
            return Err(WhenError {
                message: "unexpected text".into(),
                at: p.at(),
            });
        }
        Ok(e)
    }

    /// Evaluates against `ctx`; missing keys are false.
    pub fn eval(&self, ctx: &Context) -> bool {
        match self {
            WhenClause::Key(k) => ctx.get(k).is_some_and(Value::truthy),
            WhenClause::Eq(k, v) => ctx.get(k).is_some_and(|x| loose_eq(x, v)),
            WhenClause::Ne(k, v) => !ctx.get(k).is_some_and(|x| loose_eq(x, v)),
            WhenClause::Not(e) => !e.eval(ctx),
            WhenClause::And(a, b) => a.eval(ctx) && b.eval(ctx),
            WhenClause::Or(a, b) => a.eval(ctx) || b.eval(ctx),
            WhenClause::Const(b) => *b,
        }
    }
}

impl WhenClause {
    /// Whether the clause can hold in `ctx` when the keys `ctx` lacks may
    /// take any value: `false` only when what `ctx` has already rules it
    /// out. Menus ask this with the document's keys alone, so a command
    /// that depends on the cursor stays in them.
    pub fn possible(&self, ctx: &Context) -> bool {
        self.eval3(ctx) != Some(false)
    }

    /// Three-valued evaluation: `None` when it turns on a key `ctx` lacks.
    fn eval3(&self, ctx: &Context) -> Option<bool> {
        match self {
            WhenClause::Key(k) => ctx.get(k).map(Value::truthy),
            WhenClause::Eq(k, v) => ctx.get(k).map(|x| loose_eq(x, v)),
            WhenClause::Ne(k, v) => ctx.get(k).map(|x| !loose_eq(x, v)),
            WhenClause::Not(e) => e.eval3(ctx).map(|b| !b),
            WhenClause::And(a, b) => match (a.eval3(ctx), b.eval3(ctx)) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            },
            WhenClause::Or(a, b) => match (a.eval3(ctx), b.eval3(ctx)) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            },
            WhenClause::Const(b) => Some(*b),
        }
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Str(s)) | (Value::Str(s), Value::Num(x)) => {
            s.parse::<f64>().is_ok_and(|y| y == *x)
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clauses() {
        let mut ctx = Context::default();
        ctx.flag("editorFocus", true)
            .flag("inTable", false)
            .set("editorMode", Value::Str("org".into()))
            .set("level", Value::Num(2.0));
        let t = |s: &str| WhenClause::parse(s).unwrap().eval(&ctx);
        assert!(t("editorFocus"));
        assert!(!t("editorFocus && inTable"));
        assert!(t("editorFocus && !inTable"));
        assert!(t("inTable || editorMode == org"));
        assert!(t("editorMode != 'markdown'"));
        assert!(t("level == 2 && (inTable || editorFocus)"));
        assert!(!t("missing"));
        // With keys unknown: possible unless the known ones rule it out.
        let mut doc = Context::default();
        doc.set("editorMode", Value::Str("latex".into()));
        let p = |s: &str| WhenClause::parse(s).unwrap().possible(&doc);
        assert!(!p("editorMode == org"));
        assert!(!p("editorMode == org && onHeadline"));
        assert!(p("onHeadline"));
        assert!(p("!onHeadline"));
        assert!(p("editorMode == latex && inTable"));
        assert!(p("editorMode == org || hasSelection"));
        assert!(!p("editorMode == org || editorMode == markdown"));
        assert!(t("a || b && c || true"));
        assert!(WhenClause::parse("a &&").is_err());
        assert!(WhenClause::parse("(a").is_err());
        assert_eq!(WhenClause::parse("a b").unwrap_err().at, 2);
    }
}
