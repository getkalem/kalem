//! Tags and property match strings, as `org-make-tags-matcher` compiles
//! them: `+work-boss|TODO="NEXT"`, `LEVEL>1/!`, `{^wo}`,
//! `SCHEDULED<"<today>"`.

use jiff::civil::DateTime;
use regex_automata::meta::Regex;

use crate::info::split_string;
use crate::{Document, EntryId, Inherit};

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

#[derive(Debug, Clone)]
enum Operand {
    Regex(Option<Regex>),
    Str(String),
    Time(f64),
    Num(f64),
}

#[derive(Debug, Clone)]
enum Term {
    Tag(String),
    TagRegex(Option<Regex>),
    Prop {
        name: String,
        op: Op,
        operand: Operand,
        star: bool,
    },
}

#[derive(Debug, Clone)]
struct Item {
    minus: bool,
    term: Term,
}

#[derive(Debug, Clone)]
enum TodoItem {
    Equal(String),
    Regex(Option<Regex>),
}

/// A compiled match string.
#[derive(Debug, Clone)]
pub struct Matcher {
    tags: Option<Vec<Vec<Item>>>,
    todo: Option<Vec<Vec<(bool, TodoItem)>>>,
    todo_only: bool,
    ignored: Vec<String>,
}

/// Translates an Emacs regular expression into `regex` syntax, for the
/// constructs match strings use. Returns `None` for unsupported ones (such
/// as back references).
pub fn emacs_regex(re: &str, case_fold: bool) -> Option<String> {
    let mut out = String::new();
    if case_fold {
        out.push_str("(?i)");
    }
    let c: Vec<char> = re.chars().collect();
    let mut i = 0;
    let mut at_start = true;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' && i + 1 < c.len() {
            let n = c[i + 1];
            i += 2;
            match n {
                '(' => {
                    if c.get(i) == Some(&'?') {
                        // `\(?:` or `\(?N:`
                        let mut j = i + 1;
                        while j < c.len() && c[j].is_ascii_digit() {
                            j += 1;
                        }
                        if c.get(j) == Some(&':') {
                            i = j + 1;
                        }
                        out.push_str("(?:");
                    } else {
                        out.push('(');
                    }
                    at_start = true;
                    continue;
                }
                ')' => out.push(')'),
                '|' => {
                    out.push('|');
                    at_start = true;
                    continue;
                }
                '{' => out.push('{'),
                '}' => out.push('}'),
                '<' | '>' | 'b' => out.push_str("\\b"),
                'B' => out.push_str("\\B"),
                'w' => out.push_str("\\w"),
                'W' => out.push_str("\\W"),
                '`' => out.push_str("\\A"),
                '\'' => out.push_str("\\z"),
                '=' => {}
                '_' if matches!(c.get(i), Some('<') | Some('>')) => {
                    out.push_str("\\b");
                    i += 1;
                }
                's' | 'S' if c.get(i) == Some(&'-') => {
                    out.push_str(if n == 's' { "\\s" } else { "\\S" });
                    i += 1;
                }
                '0'..='9' => return None,
                other => {
                    out.push_str(&regex_syntax::escape(&other.to_string()));
                }
            }
            at_start = false;
            continue;
        }
        match ch {
            '[' => {
                // A bracket expression, copied with Rust escapes.
                let mut j = i + 1;
                out.push('[');
                if c.get(j) == Some(&'^') {
                    out.push('^');
                    j += 1;
                }
                if c.get(j) == Some(&']') {
                    out.push_str("\\]");
                    j += 1;
                }
                while j < c.len() && c[j] != ']' {
                    if c[j] == '[' && c.get(j + 1) == Some(&':') {
                        let end = (j + 2..c.len().saturating_sub(1))
                            .find(|&k| c[k] == ':' && c[k + 1] == ']')?;
                        out.extend(&c[j..end + 2]);
                        j = end + 2;
                        continue;
                    }
                    if matches!(c[j], '\\' | '[' | '&' | '~')
                        || (c[j] == '-' && c.get(j + 1) == Some(&'-'))
                    {
                        out.push('\\');
                    }
                    out.push(c[j]);
                    j += 1;
                }
                if j >= c.len() {
                    return None;
                }
                out.push(']');
                i = j + 1;
            }
            '(' | ')' | '|' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
                i += 1;
            }
            '^' if !at_start => {
                out.push_str("\\^");
                i += 1;
            }
            '*' | '+' | '?' if at_start => {
                out.push('\\');
                out.push(ch);
                i += 1;
            }
            _ => {
                out.push(ch);
                i += 1;
            }
        }
        at_start = false;
    }
    Some(out)
}

fn compile(re: &str) -> Option<Regex> {
    emacs_regex(re, true).and_then(|r| Regex::new(&r).ok())
}

/// Emacs's `string-to-number` in base 10.
pub fn string_to_number(s: &str) -> f64 {
    let s = s.trim_start_matches([' ', '\t', '\n']);
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'-') | Some(b'+')) {
        i += 1;
    }
    let digits = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    let int_end = digits(i);
    let mut end = int_end;
    if b.get(end) == Some(&b'.') {
        let frac_end = digits(end + 1);
        if frac_end > end + 1 {
            end = frac_end;
        }
    }
    if end == i || (end == int_end && int_end == i) {
        return 0.0;
    }
    if matches!(b.get(end), Some(b'e') | Some(b'E')) {
        let mut k = end + 1;
        if matches!(b.get(k), Some(b'-') | Some(b'+')) {
            k += 1;
        }
        let e = digits(k);
        if e > k {
            end = e;
        }
    }
    s[..end].parse().unwrap_or(0.0)
}

/// Seconds of a civil date and time on a uniform scale (time zones are
/// not modeled: comparisons between local times are what matter).
fn seconds(dt: DateTime) -> f64 {
    let days = dt
        .date()
        .since(jiff::civil::date(1970, 1, 1))
        .map_or(0, |s| s.get_days()) as f64;
    days * 86400.0 + dt.hour() as f64 * 3600.0 + dt.minute() as f64 * 60.0 + dt.second() as f64
}

/// `org-2ft`: a time string as seconds, 0 when it is not one.
fn to_ft(s: &str) -> f64 {
    crate::time::parse_time_string(s).map_or(0.0, seconds)
}

/// `org-matcher-time`.
fn matcher_time(s: &str, now: DateTime) -> f64 {
    let today = seconds(now.date().to_datetime(jiff::civil::Time::midnight()));
    match s {
        "<now>" => seconds(now),
        "<today>" => today,
        "<tomorrow>" => today + 86400.0,
        "<yesterday>" => today - 86400.0,
        _ => {
            let inner = s.strip_prefix('<').and_then(|x| x.strip_suffix('>'));
            if let Some(inner) = inner
                && inner.len() >= 3
                && matches!(inner.as_bytes()[0], b'+' | b'-')
                && inner[1..inner.len() - 1]
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            {
                let n: f64 = inner[..inner.len() - 1].parse().unwrap_or(0.0);
                let unit = match inner.as_bytes()[inner.len() - 1] {
                    b'd' => Some(86400.0),
                    b'w' => Some(604800.0),
                    b'm' => Some(2678400.0),
                    b'y' => Some(31557600.0),
                    b'h' => None,
                    _ => return to_ft(s),
                };
                return match unit {
                    Some(u) => today + n * u,
                    // `(* n nil)` signals in Emacs; treat it as now.
                    None => seconds(now),
                };
            }
            to_ft(s)
        }
    }
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%')
}

/// One step of the term regexp `^&?\([-+:]\)?\(TERM\)`: the sign, the
/// term's text and the parsed term, and the length consumed.
fn next_term(s: &str, now: DateTime) -> Option<(bool, String, Term, usize)> {
    let mut i = 0;
    if s.starts_with('&') {
        i += 1;
    }
    let minus = match s[i..].chars().next() {
        Some('-') => {
            i += 1;
            true
        }
        Some('+') | Some(':') => {
            i += 1;
            false
        }
        _ => false,
    };
    let rest = &s[i..];
    // `{[^}]+}`
    if let Some(r) = rest.strip_prefix('{')
        && let Some(end) = r.find('}')
        && end > 0
    {
        let text = &rest[..end + 2];
        return Some((
            minus,
            text.to_string(),
            Term::TagRegex(compile(&r[..end])),
            i + end + 2,
        ));
    }
    // A property comparison.
    if let Some((term, len)) = property_term(rest, now) {
        return Some((minus, rest[..len].to_string(), term, i + len));
    }
    // `[[:alnum:]_@#%]+`
    let len: usize = rest
        .chars()
        .take_while(|c| is_tag_char(*c))
        .map(char::len_utf8)
        .sum();
    (len > 0).then(|| {
        (
            minus,
            rest[..len].to_string(),
            Term::Tag(rest[..len].to_string()),
            i + len,
        )
    })
}

fn property_term(s: &str, now: DateTime) -> Option<(Term, usize)> {
    // Name: `\(?:[[:alnum:]_]+\|\\[^[:space:]]\)+`
    let mut name = String::new();
    let mut i = 0;
    let c: Vec<(usize, char)> = s.char_indices().collect();
    let mut k = 0;
    while k < c.len() {
        let ch = c[k].1;
        if ch.is_alphanumeric() || ch == '_' {
            name.push(ch);
            k += 1;
        } else if ch == '\\' && k + 1 < c.len() && !c[k + 1].1.is_whitespace() {
            name.push(c[k + 1].1);
            k += 2;
        } else {
            break;
        }
    }
    if name.is_empty() {
        return None;
    }
    i += c.get(k).map_or(s.len(), |x| x.0);
    let rest = &s[i..];
    // Operators in the regexp's order of preference.
    let mut ops: Vec<(&str, Op)> = Vec::new();
    if let Some(first) = rest.chars().next()
        && matches!(first, '<' | '=' | '>')
    {
        let base = match first {
            '<' => (Op::Lt, Op::Le),
            '>' => (Op::Gt, Op::Ge),
            _ => (Op::Eq, Op::Eq),
        };
        if rest[1..].starts_with('=') {
            ops.push((&rest[..2], if first == '=' { Op::Eq } else { base.1 }));
        }
        ops.push((&rest[..1], base.0));
    }
    for (p, op) in [("!=", Op::Ne), ("/=", Op::Ne), ("<>", Op::Ne)] {
        if rest.starts_with(p) {
            ops.push((p, op));
        }
    }
    for (optext, op) in ops {
        // `=<` and `=>` are `<=` and `>=`.
        let op = match optext {
            "=<" => Op::Le,
            "=>" => Op::Ge,
            _ => op,
        };
        let mut j = optext.len();
        let star = rest[j..].starts_with('*');
        if star {
            j += 1;
        }
        let v = &rest[j..];
        let (operand_text, len) = if let Some(r) = v.strip_prefix('{') {
            match r.find('}') {
                Some(e) if e > 0 => (&v[..e + 2], e + 2),
                _ => continue,
            }
        } else if let Some(r) = v.strip_prefix('"') {
            match r.find('"') {
                Some(e) => (&v[..e + 2], e + 2),
                None => continue,
            }
        } else {
            // `-?[.0-9]+\(?:[eE][-+]?[0-9]+\)?`
            let b = v.as_bytes();
            let mut e = usize::from(b.first() == Some(&b'-'));
            let d0 = e;
            while e < b.len() && (b[e].is_ascii_digit() || b[e] == b'.') {
                e += 1;
            }
            if e == d0 {
                continue;
            }
            if matches!(b.get(e), Some(b'e') | Some(b'E')) {
                let mut f = e + 1;
                if matches!(b.get(f), Some(b'-') | Some(b'+')) {
                    f += 1;
                }
                let g = f + b[f.min(b.len())..]
                    .iter()
                    .take_while(|x| x.is_ascii_digit())
                    .count();
                if g > f {
                    e = g;
                }
            }
            (&v[..e], e)
        };
        let upper = name.to_uppercase();
        let operand = if operand_text.starts_with('{') {
            Operand::Regex(compile(&operand_text[1..operand_text.len() - 1]))
        } else if let Some(inner) = operand_text
            .strip_prefix('"')
            .and_then(|x| x.strip_suffix('"'))
        {
            if is_time_operand(operand_text) {
                Operand::Time(matcher_time(inner, now))
            } else {
                Operand::Str(inner.to_string())
            }
        } else {
            Operand::Num(string_to_number(operand_text))
        };
        return Some((
            Term::Prop {
                name: upper,
                op,
                operand,
                star,
            },
            i + j + len,
        ));
    }
    None
}

/// `^"[[<]\(?:[0-9]+\|now\|today\|tomorrow\|[+-][0-9]+[dmwy]\).*[]>]"$`
fn is_time_operand(pv: &str) -> bool {
    let Some(inner) = pv.strip_prefix('"').and_then(|x| x.strip_suffix('"')) else {
        return false;
    };
    let b = inner.as_bytes();
    if b.len() < 2
        || !matches!(b[0], b'[' | b'<')
        || !matches!(b[b.len() - 1], b']' | b'>')
        || inner.contains('\n')
    {
        return false;
    }
    let body = &inner[1..];
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    let rel = {
        let bb = body.as_bytes();
        matches!(bb.first(), Some(b'+') | Some(b'-')) && {
            let n = bb[1..].iter().take_while(|x| x.is_ascii_digit()).count();
            n > 0 && matches!(bb.get(1 + n), Some(b'd' | b'm' | b'w' | b'y'))
        }
    };
    // The alternative and `.*` must leave room for the closing bracket.
    let starts = digits > 0
        || rel
        || ["now", "today", "tomorrow"]
            .iter()
            .any(|w| body.starts_with(w));
    starts && body.len() >= 2
}

impl Matcher {
    /// Compiles `match` (`org-make-tags-matcher`); `now` gives the value
    /// of `<today>`, `<now>` and relative times.
    pub fn new(input: &str, now: DateTime) -> Matcher {
        // The TODO part follows the last run of slashes, unless a double
        // quote comes after it.
        let mut tags_part = input;
        let mut todo_part: Option<&str> = None;
        let mut todo_only = false;
        let mut ignored = Vec::new();
        if let Some(start) = last_slash_run(input)
            && !input[start..].contains('"')
        {
            let end = start + input[start..].bytes().take_while(|b| *b == b'/').count();
            tags_part = &input[..start];
            let mut t = &input[end..];
            if let Some(x) = t.strip_prefix('!') {
                todo_only = true;
                t = x;
            }
            todo_part = (!t.trim().is_empty()).then_some(t);
        }
        let tags = (!tags_part.trim().is_empty()).then(|| {
            let mut terms = split_string(tags_part, '|').into_iter();
            let mut or = Vec::new();
            while let Some(mut term) = terms.next() {
                while term.ends_with('\\') {
                    match terms.next() {
                        Some(next) => term = format!("{term}|{next}"),
                        None => break,
                    }
                }
                let mut and = Vec::new();
                let mut t = term.as_str();
                while let Some((minus, _, parsed, len)) = next_term(t, now) {
                    and.push(Item {
                        minus,
                        term: parsed,
                    });
                    t = &t[len..];
                }
                if !t.is_empty() {
                    ignored.push(t.to_string());
                }
                or.push(and);
            }
            or
        });
        let todo = todo_part.map(|t| {
            split_string(t, '|')
                .into_iter()
                .map(|term| {
                    let mut and = Vec::new();
                    let mut t = term.as_str();
                    while let Some((minus, text, _, len)) = next_term(t, now) {
                        let item = match text.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
                            Some(re) => TodoItem::Regex(compile(re)),
                            None => TodoItem::Equal(text),
                        };
                        and.push((minus, item));
                        t = &t[len..];
                    }
                    if !t.is_empty() {
                        ignored.push(t.to_string());
                    }
                    and
                })
                .collect()
        });
        Matcher {
            tags,
            todo,
            todo_only,
            ignored,
        }
    }

    /// The parts of the match string that are no term, which Emacs too
    /// leaves out: the rest of each `|` part from where its terms stop.
    pub fn ignored(&self) -> &[String] {
        &self.ignored
    }

    /// Whether entry `id` matches.
    pub fn matches(&self, doc: &Document, id: EntryId) -> bool {
        let e = doc.entry(id);
        let todo = e.todo.as_deref();
        let ctx = doc.parse().context();
        if self.todo_only && !todo.is_some_and(|t| ctx.todo_keywords.iter().any(|k| k == t)) {
            return false;
        }
        let tags = doc.tags(id);
        let tags_ok = self.tags.as_ref().is_none_or(|or| {
            or.iter().any(|and| {
                and.iter()
                    .all(|item| item.minus != self.term(doc, id, &item.term, &tags))
            })
        });
        let todo_ok = self.todo.as_ref().is_none_or(|or| {
            or.iter().any(|and| {
                !and.is_empty()
                    && and.iter().all(|(minus, item)| {
                        let m = match item {
                            TodoItem::Equal(k) => todo == Some(k.as_str()),
                            TodoItem::Regex(re) => {
                                todo.is_some_and(|t| re.as_ref().is_some_and(|re| re.is_match(t)))
                            }
                        };
                        *minus != m
                    })
            })
        });
        tags_ok && todo_ok
    }

    fn term(&self, doc: &Document, id: EntryId, term: &Term, tags: &[String]) -> bool {
        match term {
            Term::Tag(t) => tags.iter().any(|x| x == t),
            Term::TagRegex(re) => re
                .as_ref()
                .is_some_and(|re| tags.iter().any(|t| re.is_match(t))),
            Term::Prop {
                name,
                op,
                operand,
                star,
            } => {
                let value = match name.as_str() {
                    "LEVEL" => Some(doc.entry(id).level.to_string()),
                    "CATEGORY" => Some(doc.category(Some(id))),
                    "TODO" => doc.entry(id).todo.clone(),
                    p => doc.entry_get(Some(id), p, Inherit::Selective, false),
                };
                if *star && value.is_none() {
                    return false;
                }
                let v = value.unwrap_or_default();
                match operand {
                    Operand::Regex(re) => {
                        let m = re.as_ref().is_some_and(|re| re.is_match(&v));
                        if *op == Op::Ne { !m } else { m }
                    }
                    Operand::Str(s) => cmp(*op, v.as_str().cmp(s.as_str())),
                    // `org-time<` and friends: both times must be positive.
                    Operand::Time(t) => {
                        let a = to_ft(&v);
                        // `org-time<>` is written `(\= a b)`, which Emacs
                        // reads as `(= a b)`: `<>` tests equality on times.
                        let op = if *op == Op::Ne { Op::Eq } else { *op };
                        a > 0.0 && *t > 0.0 && cmp_f(op, a, *t)
                    }
                    Operand::Num(n) => cmp_f(*op, string_to_number(&v), *n),
                }
            }
        }
    }
}

fn last_slash_run(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut start = None;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' {
            start = Some(i);
            while i < b.len() && b[i] == b'/' {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    start
}

fn cmp(op: Op, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        Op::Lt => o == Less,
        Op::Gt => o == Greater,
        Op::Le => o != Greater,
        Op::Ge => o != Less,
        Op::Eq => o == Equal,
        Op::Ne => o != Equal,
    }
}

fn cmp_f(op: Op, a: f64, b: f64) -> bool {
    match op {
        Op::Lt => a < b,
        Op::Gt => a > b,
        Op::Le => a <= b,
        Op::Ge => a >= b,
        Op::Eq => a == b,
        Op::Ne => a != b,
    }
}

impl Document {
    /// The headlines and inlinetasks matching `input` (`org-map-entries`
    /// with a match string), in document order.
    pub fn matching(&self, input: &str, now: DateTime) -> Vec<EntryId> {
        let expanded;
        let input = if self.settings().group_tags {
            expanded = crate::tags::expand_group_tags(input, &self.tag_table().groups());
            expanded.as_str()
        } else {
            input
        };
        let m = Matcher::new(input, now);
        (0..self.outline().entries.len())
            .map(EntryId)
            .filter(|id| m.matches(self, *id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_translation() {
        assert_eq!(emacs_regex("^wo", false).as_deref(), Some("^wo"));
        assert_eq!(emacs_regex("\\(a\\|b\\)", false).as_deref(), Some("(a|b)"));
        assert_eq!(emacs_regex("a(b)", false).as_deref(), Some("a\\(b\\)"));
        assert_eq!(
            emacs_regex("[[:alpha:]\\]x", false).as_deref(),
            Some("[[:alpha:]\\\\]x")
        );
        assert_eq!(emacs_regex("\\<wo\\>", false).as_deref(), Some("\\bwo\\b"));
        assert_eq!(emacs_regex("\\1", false), None);
    }

    #[test]
    fn numbers() {
        assert_eq!(string_to_number("0:30"), 0.0);
        assert_eq!(string_to_number(" 12abc"), 12.0);
        assert_eq!(string_to_number("-1.5e2"), -150.0);
        assert_eq!(string_to_number("x"), 0.0);
        assert_eq!(string_to_number(".5"), 0.5);
    }
}
