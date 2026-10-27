//! Vim's patterns (`:help pattern`) as Rust regular expressions: the
//! 'magic' syntax by default, `\v` (very magic), `\m`, `\M` and `\V`, the
//! character classes, `\<` and `\>`, `\{n,m}`, `\zs` and `\ze`, `\c` and
//! `\C`, with 'ignorecase' and 'smartcase'.

use std::ops::Range;

/// A compiled pattern.
#[derive(Debug, Clone)]
pub(crate) struct Pattern {
    re: regex::Regex,
    /// The match is this group (`\zs`, `\ze`), not the whole.
    group: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Magic {
    Very,
    Normal,
    No,
    VeryNo,
}

/// Characters a Rust regular expression treats as special outside a
/// class.
const RUST_SPECIAL: &str = "\\.+*?()|[]{}^$#&-~";

fn literal(out: &mut String, c: char) {
    if RUST_SPECIAL.contains(c) {
        out.push('\\');
    }
    out.push(c);
}

/// The class a backslash and `c` stand for, if any.
fn class(c: char) -> Option<&'static str> {
    Some(match c {
        's' => "[ \\t]",
        'S' => "[^ \\t\\n]",
        'd' => "[0-9]",
        'D' => "[^0-9\\n]",
        'w' => "[0-9A-Za-z_]",
        'W' => "[^0-9A-Za-z_\\n]",
        'a' => "[A-Za-z]",
        'A' => "[^A-Za-z\\n]",
        'l' => "[a-z]",
        'L' => "[^a-z\\n]",
        'u' => "[A-Z]",
        'U' => "[^A-Z\\n]",
        'x' => "[0-9A-Fa-f]",
        'X' => "[^0-9A-Fa-f\\n]",
        'o' => "[0-7]",
        'O' => "[^0-7\\n]",
        'h' => "[A-Za-z_]",
        'H' => "[^A-Za-z_\\n]",
        'k' | 'i' | 'f' | 'p' => "[^\\s]",
        'K' | 'I' | 'F' | 'P' => "[^\\s0-9]",
        'n' => "\\n",
        't' => "\\t",
        'e' => "\\x1b",
        'r' => "\\r",
        _ => return None,
    })
}

/// Translates `pat`; the second value says whether case is ignored by a
/// `\c` or `\C` in it (`None`: the options decide).
fn translate(pat: &str) -> (String, Option<bool>, Option<usize>, Option<usize>) {
    let mut out = String::new();
    let mut magic = Magic::Normal;
    let mut case = None;
    let (mut zs, mut ze) = (None, None);
    let chars: Vec<char> = pat.chars().collect();
    let mut i = 0;
    // Whether `^` here is an anchor: at the start, after `\(` or `\|`.
    let mut at_start = true;
    while i < chars.len() {
        let c = chars[i];
        let start_here = at_start;
        at_start = false;
        if c == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            i += 2;
            match n {
                'v' => magic = Magic::Very,
                'm' => magic = Magic::Normal,
                'M' => magic = Magic::No,
                'V' => magic = Magic::VeryNo,
                'c' => case = Some(true),
                'C' => case = Some(false),
                '<' | '>' if magic != Magic::Very => out.push_str("\\b"),
                '(' if magic != Magic::Very => {
                    out.push('(');
                    at_start = true;
                }
                ')' if magic != Magic::Very => out.push(')'),
                '|' if magic != Magic::Very => {
                    out.push('|');
                    at_start = true;
                }
                '+' if magic != Magic::Very => out.push('+'),
                '=' | '?' if magic != Magic::Very => out.push('?'),
                '{' if magic != Magic::Very => i = brace(&chars, i, &mut out),
                '*' if matches!(magic, Magic::No | Magic::VeryNo) => out.push('*'),
                '.' if matches!(magic, Magic::No | Magic::VeryNo) => out.push('.'),
                '[' if matches!(magic, Magic::No | Magic::VeryNo) => {
                    i = collection(&chars, i - 1, &mut out);
                }
                '$' if magic == Magic::VeryNo => out.push('$'),
                '^' if magic == Magic::VeryNo => out.push('^'),
                'z' if i < chars.len() && chars[i] == 's' => {
                    zs = Some(out.len());
                    i += 1;
                }
                'z' if i < chars.len() && chars[i] == 'e' => {
                    ze = Some(out.len());
                    i += 1;
                }
                '%' if i < chars.len() && chars[i] == '(' => {
                    out.push_str("(?:");
                    at_start = true;
                    i += 1;
                }
                '_' if i < chars.len() => {
                    let k = chars[i];
                    i += 1;
                    match k {
                        '.' => out.push_str("(?s:.)"),
                        's' => out.push_str("\\s"),
                        '^' => out.push('^'),
                        '$' => out.push('$'),
                        k => match class(k) {
                            Some(cl) => {
                                // The class with the line feed.
                                let cl = cl.replace("\\n]", "]");
                                if let Some(rest) = cl.strip_prefix('[') {
                                    out.push_str(&format!("[\\n{rest}"));
                                } else {
                                    out.push_str(&format!("(?:\\n|{cl})"));
                                }
                            }
                            None => literal(&mut out, k),
                        },
                    }
                }
                n if n.is_ascii_digit() && n != '0' => {
                    // A back reference: not supported by Rust's regex; it
                    // matches nothing rather than something else.
                    out.push_str("[^\\s\\S]");
                }
                n => match class(n) {
                    Some(cl) => out.push_str(cl),
                    None => literal(&mut out, n),
                },
            }
            continue;
        }
        i += 1;
        match (magic, c) {
            (_, '^') if start_here && magic != Magic::VeryNo => {
                out.push('^');
                at_start = true;
            }
            (m, '$')
                if m != Magic::VeryNo
                    && (i == chars.len()
                        || chars[i..].starts_with(&['\\', '|'])
                        || chars[i..].starts_with(&['\\', ')'])
                        || (m == Magic::Very && matches!(chars[i], '|' | ')'))) =>
            {
                out.push('$')
            }
            (Magic::Very | Magic::Normal, '.') => out.push('.'),
            (Magic::Very | Magic::Normal, '*') if !start_here => out.push('*'),
            (Magic::Very | Magic::Normal, '[') => i = collection(&chars, i - 1, &mut out),
            (Magic::Very, '(') => {
                out.push('(');
                at_start = true;
            }
            (Magic::Very, ')') => out.push(')'),
            (Magic::Very, '|') => {
                out.push('|');
                at_start = true;
            }
            (Magic::Very, '+') => out.push('+'),
            (Magic::Very, '=' | '?') => out.push('?'),
            (Magic::Very, '<' | '>') => out.push_str("\\b"),
            (Magic::Very, '{') => i = brace(&chars, i, &mut out),
            (Magic::Very, '%') if i < chars.len() && chars[i] == '(' => {
                out.push_str("(?:");
                at_start = true;
                i += 1;
            }
            (_, c) => literal(&mut out, c),
        }
    }
    (out, case, zs, ze)
}

/// `\{n,m}` and its kin, from the character after the brace; returns the
/// index after the closing brace.
fn brace(chars: &[char], mut i: usize, out: &mut String) -> usize {
    let lazy = chars.get(i) == Some(&'-');
    if lazy {
        i += 1;
    }
    let mut body = String::new();
    while i < chars.len() && chars[i] != '}' {
        if chars[i] != '\\' {
            body.push(chars[i]);
        }
        i += 1;
    }
    let body = body.trim();
    let q = match body {
        "" => "*".to_string(),
        b if b.starts_with(',') => format!("{{0{b}}}"),
        b => format!("{{{b}}}"),
    };
    out.push_str(&q);
    if lazy {
        out.push('?');
    }
    i + 1
}

/// A `[...]` collection starting at `i`; returns the index after it. An
/// unclosed `[` is a literal.
fn collection(chars: &[char], i: usize, out: &mut String) -> usize {
    let mut j = i + 1;
    if chars.get(j) == Some(&'^') {
        j += 1;
    }
    if chars.get(j) == Some(&']') {
        j += 1;
    }
    while j < chars.len() && chars[j] != ']' {
        if chars[j] == '['
            && chars.get(j + 1) == Some(&':')
            && let Some(end) = (j + 2..chars.len().saturating_sub(1))
                .find(|&k| chars[k] == ':' && chars[k + 1] == ']')
        {
            j = end + 2;
            continue;
        }
        if chars[j] == '\\' {
            j += 1;
        }
        j += 1;
    }
    if j >= chars.len() {
        out.push_str("\\[");
        return i + 1;
    }
    out.push('[');
    let mut k = i + 1;
    if chars[k] == '^' {
        out.push('^');
        k += 1;
    }
    let mut first = true;
    while k < j {
        let c = chars[k];
        if c == '[' && chars.get(k + 1) == Some(&':') {
            let end = (k + 2..j).find(|&e| chars[e] == ':').unwrap_or(j);
            let name: String = chars[k..=end + 1].iter().collect();
            out.push_str(&name);
            k = end + 2;
            first = false;
            continue;
        }
        match c {
            '\\' if k + 1 < j => {
                let n = chars[k + 1];
                match n {
                    'e' => out.push_str("\\x1b"),
                    't' => out.push_str("\\t"),
                    'n' => out.push_str("\\n"),
                    'r' => out.push_str("\\r"),
                    '\\' | ']' | '^' | '-' => {
                        out.push('\\');
                        out.push(n);
                    }
                    n => {
                        out.push_str("\\\\");
                        literal(out, n);
                    }
                }
                k += 2;
            }
            ']' if first => {
                out.push_str("\\]");
                k += 1;
            }
            '[' | '&' | '~' => {
                out.push('\\');
                out.push(c);
                k += 1;
            }
            c => {
                out.push(c);
                k += 1;
            }
        }
        first = false;
    }
    out.push(']');
    j + 1
}

impl Pattern {
    /// Compiles `pat` with 'ignorecase' and 'smartcase'.
    pub(crate) fn new(pat: &str, ignorecase: bool, smartcase: bool) -> Result<Pattern, String> {
        let (mut re, case, zs, ze) = translate(pat);
        let group = zs.is_some() || ze.is_some();
        if group {
            let a = zs.unwrap_or(0);
            let b = ze.unwrap_or(re.len()).max(a);
            re = format!("{}(?P<m>{}){}", &re[..a], &re[a..b], &re[b..]);
        }
        let upper = pat.chars().any(char::is_uppercase);
        let fold = case.unwrap_or(ignorecase && !(smartcase && upper));
        let re = regex::RegexBuilder::new(&re)
            .multi_line(true)
            .case_insensitive(fold)
            .build()
            .map_err(|_| format!("E486: Pattern not found: {pat}"))?;
        Ok(Pattern { re, group })
    }

    /// Every match in `text`, in order; an empty match counts where it
    /// is (`^` finds every line).
    pub(crate) fn find_all(&self, text: &str) -> Vec<Range<usize>> {
        if self.group {
            return self
                .re
                .captures_iter(text)
                .filter_map(|c| c.name("m").map(|m| m.range()))
                .collect();
        }
        self.re.find_iter(text).map(|m| m.range()).collect()
    }

    /// Whether `line` has a match.
    pub(crate) fn is_match(&self, line: &str) -> bool {
        self.re.is_match(line)
    }

    /// The regular expression, for replacing.
    pub(crate) fn regex(&self) -> &regex::Regex {
        &self.re
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, t: &str) -> Vec<String> {
        let pat = Pattern::new(p, false, false).unwrap();
        pat.find_all(t)
            .into_iter()
            .map(|r| t[r].to_string())
            .collect()
    }

    #[test]
    fn magic() {
        assert_eq!(m("a.c", "abc a+c"), ["abc", "a+c"]);
        assert_eq!(m("a+", "aa a+"), ["a+"]);
        assert_eq!(m("a\\+", "aa a+"), ["aa", "a"]);
        assert_eq!(m("\\<the\\>", "the other the"), ["the", "the"]);
        assert_eq!(m("\\(ab\\)\\{2}", "ababab"), ["abab"]);
        assert_eq!(m("x\\|y", "axbyc"), ["x", "y"]);
        assert_eq!(m("(a)", "(a) a"), ["(a)"]);
        assert_eq!(m("\\v(a|b)+", "aab c"), ["aab"]);
        assert_eq!(m("\\Va.c", "abc a.c"), ["a.c"]);
        assert_eq!(m("foo\\zsbar", "foobar bar"), ["bar"]);
        assert_eq!(m("foo\\zebar", "foobar foo"), ["foo"]);
        assert_eq!(m("[a-c]\\+", "xabcx"), ["abc"]);
        assert_eq!(m("^x", "x\nax\nx"), ["x", "x"]);
        assert_eq!(m("a$b", "a$b"), ["a$b"]);
        assert_eq!(m("\\d\\+", "a 42 b 7"), ["42", "7"]);
        assert_eq!(m("a\\{-1,}", "aaa"), ["a", "a", "a"]);
        assert_eq!(m("\\cABC", "abc"), ["abc"]);
        let smart = Pattern::new("Abc", true, true).unwrap();
        assert!(smart.find_all("abc").is_empty());
        let fold = Pattern::new("abc", true, true).unwrap();
        assert_eq!(fold.find_all("ABC").len(), 1);
    }
}
