//! Export options as `ox.el` reads them: `#+OPTIONS:` items with their
//! Lisp values, keywords such as `#+TITLE:` with their behaviors, and the
//! defaults of an Emacs without customizations.

/// A Lisp value read from an `#+OPTIONS:` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// `t`.
    T,
    /// `nil` (or `()`).
    Nil,
    /// An integer.
    Int(i64),
    /// A symbol other than `t` and `nil`.
    Sym(String),
    /// A string.
    Str(String),
    /// A list.
    List(Vec<Value>),
}

impl Value {
    /// Non-nil.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil)
    }

    /// The integer, if it is one.
    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The symbol's name, if it is one.
    pub fn sym(&self) -> Option<&str> {
        match self {
            Value::Sym(s) => Some(s),
            _ => None,
        }
    }

    /// The strings of a list (strings and symbols).
    pub fn strings(&self) -> Vec<String> {
        match self {
            Value::List(v) => v
                .iter()
                .filter_map(|x| match x {
                    Value::Str(s) | Value::Sym(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// Reads one Lisp value from `s`; the rest of the text is ignored.
pub fn read(s: &str) -> Value {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    read_at(&chars, &mut i)
}

fn read_at(c: &[char], i: &mut usize) -> Value {
    while *i < c.len() && c[*i].is_whitespace() {
        *i += 1;
    }
    if *i >= c.len() {
        return Value::Nil;
    }
    match c[*i] {
        '(' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                while *i < c.len() && c[*i].is_whitespace() {
                    *i += 1;
                }
                if *i >= c.len() {
                    break;
                }
                if c[*i] == ')' {
                    *i += 1;
                    break;
                }
                items.push(read_at(c, i));
            }
            if items.is_empty() {
                Value::Nil
            } else {
                Value::List(items)
            }
        }
        '"' => {
            *i += 1;
            let mut out = String::new();
            while *i < c.len() && c[*i] != '"' {
                if c[*i] == '\\' && *i + 1 < c.len() {
                    *i += 1;
                }
                out.push(c[*i]);
                *i += 1;
            }
            *i += 1;
            Value::Str(out)
        }
        '\'' => {
            *i += 1;
            read_at(c, i)
        }
        _ => {
            let start = *i;
            while *i < c.len() && !c[*i].is_whitespace() && c[*i] != '(' && c[*i] != ')' {
                if c[*i] == '\\' {
                    *i += 1;
                }
                *i += 1;
            }
            let word: String = c[start..(*i).min(c.len())]
                .iter()
                .collect::<String>()
                .replace('\\', "");
            match word.as_str() {
                "t" => Value::T,
                "nil" => Value::Nil,
                w => match w.parse::<i64>() {
                    Ok(n) => Value::Int(n),
                    Err(_) => Value::Sym(w.to_string()),
                },
            }
        }
    }
}

/// The items of an `#+OPTIONS:` value: `toc:nil num:2 ^:{}`.
pub fn parse_options(line: &str) -> Vec<(String, Value)> {
    // `\(.+?\):\((.*?)\|\S-+\)?[ \t]*`, searched from each match's end.
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut s = 0;
    while s < b.len() {
        // The key: the shortest run of characters (at least one) before
        // a colon, not across a line feed.
        let rest = &line[s..];
        let Some(colon_rel) = rest
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == ':')
            .map(|(i, _)| i)
        else {
            break;
        };
        if rest[..colon_rel].contains('\n') {
            break;
        }
        let key = rest[..colon_rel].to_string();
        let mut at = s + colon_rel + 1;
        let value = if line[at..].starts_with('(') {
            // `(.*?)`: to the first closing parenthesis.
            match line[at..].find(')') {
                Some(e) => {
                    let v = &line[at..at + e + 1];
                    at += e + 1;
                    Some(v.to_string())
                }
                None => None,
            }
        } else {
            let e = line[at..]
                .find(char::is_whitespace)
                .unwrap_or(line.len() - at);
            if e == 0 {
                None
            } else {
                let v = &line[at..at + e];
                at += e;
                Some(v.to_string())
            }
        };
        while at < b.len() && (b[at] == b' ' || b[at] == b'\t') {
            at += 1;
        }
        if at == s {
            break;
        }
        s = at;
        if let Some(v) = value {
            // The key as matched: it may start with blanks after a value
            // that ended without them.
            out.push((key, read(&v)));
        }
    }
    out
}

/// How repeated keywords combine (`org-export-options-alist`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
    /// The first value.
    First,
    /// The last value.
    Last,
    /// Values joined with spaces.
    Space,
    /// Values joined with line feeds.
    Newline,
    /// Words of all values.
    Split,
    /// Parsed as Org objects (lines joined, line feeds as spaces).
    Parse,
}

/// An export option: its property, keyword, `#+OPTIONS:` item and
/// behavior.
#[derive(Debug, Clone, Copy)]
pub struct OptionSpec {
    /// The property, as in `ox.el` without the colon.
    pub property: &'static str,
    /// The keyword setting it.
    pub keyword: Option<&'static str>,
    /// The `#+OPTIONS:` item setting it.
    pub option: Option<&'static str>,
    /// How keywords combine.
    pub behavior: Behavior,
}

const fn spec(
    property: &'static str,
    keyword: Option<&'static str>,
    option: Option<&'static str>,
    behavior: Behavior,
) -> OptionSpec {
    OptionSpec {
        property,
        keyword,
        option,
        behavior,
    }
}

/// `org-export-options-alist`.
pub const OPTIONS: &[OptionSpec] = &[
    spec("title", Some("TITLE"), None, Behavior::Parse),
    spec("date", Some("DATE"), None, Behavior::Parse),
    spec("author", Some("AUTHOR"), None, Behavior::Parse),
    spec("email", Some("EMAIL"), None, Behavior::Last),
    spec("language", Some("LANGUAGE"), None, Behavior::Last),
    spec("select-tags", Some("SELECT_TAGS"), None, Behavior::Split),
    spec("exclude-tags", Some("EXCLUDE_TAGS"), None, Behavior::Split),
    spec("creator", Some("CREATOR"), None, Behavior::First),
    spec("headline-levels", None, Some("H"), Behavior::First),
    spec("preserve-breaks", None, Some("\\n"), Behavior::First),
    spec("section-numbers", None, Some("num"), Behavior::First),
    spec("time-stamp-file", None, Some("timestamp"), Behavior::First),
    spec("with-archived-trees", None, Some("arch"), Behavior::First),
    spec("with-author", None, Some("author"), Behavior::First),
    spec("expand-links", None, Some("expand-links"), Behavior::First),
    spec(
        "with-broken-links",
        None,
        Some("broken-links"),
        Behavior::First,
    ),
    spec("with-clocks", None, Some("c"), Behavior::First),
    spec("with-creator", None, Some("creator"), Behavior::First),
    spec("with-date", None, Some("date"), Behavior::First),
    spec("with-drawers", None, Some("d"), Behavior::First),
    spec("with-email", None, Some("email"), Behavior::First),
    spec("with-emphasize", None, Some("*"), Behavior::First),
    spec("with-entities", None, Some("e"), Behavior::First),
    spec("with-fixed-width", None, Some(":"), Behavior::First),
    spec("with-footnotes", None, Some("f"), Behavior::First),
    spec("with-inlinetasks", None, Some("inline"), Behavior::First),
    spec("with-latex", None, Some("tex"), Behavior::First),
    spec("with-planning", None, Some("p"), Behavior::First),
    spec("with-priority", None, Some("pri"), Behavior::First),
    spec("with-properties", None, Some("prop"), Behavior::First),
    spec("with-smart-quotes", None, Some("'"), Behavior::First),
    spec("with-special-strings", None, Some("-"), Behavior::First),
    spec(
        "with-statistics-cookies",
        None,
        Some("stat"),
        Behavior::First,
    ),
    spec("with-sub-superscript", None, Some("^"), Behavior::First),
    spec("with-toc", None, Some("toc"), Behavior::First),
    spec("with-tables", None, Some("|"), Behavior::First),
    spec("with-tags", None, Some("tags"), Behavior::First),
    spec("with-tasks", None, Some("tasks"), Behavior::First),
    spec("with-timestamps", None, Some("<"), Behavior::First),
    spec("with-title", None, Some("title"), Behavior::First),
    spec("with-todo-keywords", None, Some("todo"), Behavior::First),
];

/// The default of an option in Emacs without customizations.
pub fn default(property: &str) -> Value {
    match property {
        "headline-levels" => Value::Int(3),
        "section-numbers"
        | "time-stamp-file"
        | "with-author"
        | "expand-links"
        | "with-date"
        | "with-emphasize"
        | "with-entities"
        | "with-fixed-width"
        | "with-footnotes"
        | "with-inlinetasks"
        | "with-latex"
        | "with-special-strings"
        | "with-statistics-cookies"
        | "with-sub-superscript"
        | "with-toc"
        | "with-tables"
        | "with-tags"
        | "with-tasks"
        | "with-timestamps"
        | "with-title"
        | "with-todo-keywords" => Value::T,
        "with-archived-trees" => Value::Sym("headline".into()),
        "with-broken-links" => Value::Sym("mark".into()),
        "with-drawers" => Value::List(vec![Value::Sym("not".into()), Value::Str("LOGBOOK".into())]),
        "select-tags" => Value::List(vec![Value::Str("export".into())]),
        "exclude-tags" => Value::List(vec![Value::Str("noexport".into())]),
        "language" => Value::Str("en".into()),
        "creator" => Value::Str("Emacs 30.1 (Org mode 9.7.11)".into()),
        _ => Value::Nil,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading() {
        let o = parse_options("toc:nil num:2 ^:{} d:(not \"LOGBOOK\") \\n:t H:2 tags:not-in-toc");
        let get = |k: &str| o.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        assert_eq!(get("toc"), Some(Value::Nil));
        assert_eq!(get("num"), Some(Value::Int(2)));
        assert_eq!(get("^"), Some(Value::Sym("{}".into())));
        assert_eq!(
            get("d"),
            Some(Value::List(vec![
                Value::Sym("not".into()),
                Value::Str("LOGBOOK".into())
            ]))
        );
        assert_eq!(get("\\n"), Some(Value::T));
        assert_eq!(get("tags"), Some(Value::Sym("not-in-toc".into())));
    }
}
