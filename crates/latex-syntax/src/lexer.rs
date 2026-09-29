//! Tokens with TeX's standard category codes: `\` escapes, `{` and `}`
//! group, `$` shifts to math, `&` `#` `^` `_` `~` are special, `%` starts
//! a comment to the end of the line, and a blank line ends a paragraph.
//! `@` is a letter in control words after `\makeatletter`.

/// A token as lexed; the parser gives it its syntax kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tok {
    ControlWord,
    ControlSymbol,
    Comment,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Dollar,
    Ampersand,
    Hash,
    Caret,
    Underscore,
    Tilde,
    Whitespace,
    Newline,
    ParBreak,
    Text,
}

/// Whether `b` is a letter in a control word.
pub(crate) fn is_letter(b: u8, at_letter: bool) -> bool {
    b.is_ascii_alphabetic() || (at_letter && b == b'@')
}

/// Bytes that end a run of text.
fn special(b: u8) -> bool {
    matches!(
        b,
        b'\\'
            | b'%'
            | b'{'
            | b'}'
            | b'['
            | b']'
            | b'$'
            | b'&'
            | b'#'
            | b'^'
            | b'_'
            | b'~'
            | b' '
            | b'\t'
            | b'\r'
            | b'\n'
    )
}

/// The length of the line ending at `pos`, if one starts there.
pub(crate) fn newline_at(b: &[u8], pos: usize, limit: usize) -> usize {
    match b.get(pos) {
        Some(b'\n') if pos < limit => 1,
        Some(b'\r') if pos < limit => {
            if pos + 1 < limit && b[pos + 1] == b'\n' {
                2
            } else {
                1
            }
        }
        _ => 0,
    }
}

/// The length of the UTF-8 character starting with byte `b`.
pub(crate) fn char_len(b: u8) -> usize {
    match b {
        0..0x80 => 1,
        0xc0..0xe0 => 2,
        0xe0..0xf0 => 3,
        0xf0.. => 4,
        _ => 1,
    }
}

/// The token at `pos` (before `limit`) and where it ends.
pub(crate) fn next(src: &str, pos: usize, limit: usize, at_letter: bool) -> (Tok, usize) {
    let b = src.as_bytes();
    debug_assert!(pos < limit);
    let c = b[pos];
    match c {
        b'\\' => {
            if pos + 1 >= limit {
                return (Tok::ControlSymbol, pos + 1);
            }
            if is_letter(b[pos + 1], at_letter) {
                let mut i = pos + 1;
                while i < limit && is_letter(b[i], at_letter) {
                    i += 1;
                }
                (Tok::ControlWord, i)
            } else {
                let n = newline_at(b, pos + 1, limit);
                let len = if n > 0 { n } else { char_len(b[pos + 1]) };
                (Tok::ControlSymbol, (pos + 1 + len).min(limit))
            }
        }
        b'%' => {
            let mut i = pos + 1;
            while i < limit && b[i] != b'\n' && b[i] != b'\r' {
                i += 1;
            }
            (Tok::Comment, i)
        }
        b'{' => (Tok::LBrace, pos + 1),
        b'}' => (Tok::RBrace, pos + 1),
        b'[' => (Tok::LBracket, pos + 1),
        b']' => (Tok::RBracket, pos + 1),
        b'$' => (Tok::Dollar, pos + 1),
        b'&' => (Tok::Ampersand, pos + 1),
        b'#' => (Tok::Hash, pos + 1),
        b'^' => (Tok::Caret, pos + 1),
        b'_' => (Tok::Underscore, pos + 1),
        b'~' => (Tok::Tilde, pos + 1),
        b' ' | b'\t' => {
            let mut i = pos + 1;
            while i < limit && matches!(b[i], b' ' | b'\t') {
                i += 1;
            }
            (Tok::Whitespace, i)
        }
        b'\n' | b'\r' => {
            let first = pos + newline_at(b, pos, limit);
            // Blank lines after it: a paragraph break to the last one.
            let mut end = first;
            let mut i = first;
            loop {
                while i < limit && matches!(b[i], b' ' | b'\t') {
                    i += 1;
                }
                let n = newline_at(b, i, limit);
                if n == 0 {
                    break;
                }
                i += n;
                end = i;
            }
            if end > first {
                (Tok::ParBreak, end)
            } else {
                (Tok::Newline, first)
            }
        }
        _ => {
            let mut i = pos + 1;
            while i < limit && !special(b[i]) {
                i += 1;
            }
            (Tok::Text, i)
        }
    }
}

/// Where the line containing `pos` ends (before its line ending).
pub(crate) fn line_end(b: &[u8], pos: usize, limit: usize) -> usize {
    let mut i = pos;
    while i < limit && b[i] != b'\n' && b[i] != b'\r' {
        i += 1;
    }
    i
}

/// For `\verb` and `\lstinline` at `pos` (after the command name): where
/// the verbatim argument ends, on the same line. `lst` allows the options
/// and the braces of `\lstinline`.
pub(crate) fn verb_end(b: &[u8], pos: usize, limit: usize, lst: bool) -> Option<usize> {
    let eol = line_end(b, pos, limit);
    let mut i = pos;
    if !lst && b.get(i) == Some(&b'*') {
        i += 1;
    }
    if lst && i < eol && b[i] == b'[' {
        i = i + 1 + b[i + 1..eol].iter().position(|&c| c == b']')? + 1;
    }
    if i >= eol {
        return None;
    }
    let d = b[i];
    if d == b' ' || d == b'\t' || (!lst && d == b'*') {
        return None;
    }
    if lst && d == b'{' {
        let mut depth = 0usize;
        for (k, &c) in b[i..eol].iter().enumerate() {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + k + 1);
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    let len = char_len(d);
    let delim = &b[i..(i + len).min(eol)];
    let rest = i + len;
    (rest..eol)
        .find(|&k| b[k..].starts_with(delim))
        .map(|k| k + len)
}

/// For `\url` and `\href` at `pos` (after the command name): the braces of
/// the address, taken as they are, on the same line: `(open, close)`.
pub(crate) fn raw_braces(b: &[u8], pos: usize, limit: usize) -> Option<(usize, usize)> {
    let eol = line_end(b, pos, limit);
    let mut i = pos;
    while i < eol && matches!(b[i], b' ' | b'\t') {
        i += 1;
    }
    if b.get(i) != Some(&b'{') || i >= eol {
        return None;
    }
    let close = i + 1 + b[i + 1..eol].iter().position(|&c| c == b'}')?;
    Some((i, close))
}

/// For `\begin` and `\end` at `pos` (after the command name): the braces
/// around the environment name and the name's range.
pub(crate) fn env_name(b: &[u8], pos: usize, limit: usize) -> Option<(usize, usize, usize)> {
    let mut i = pos;
    while i < limit && matches!(b[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= limit || b[i] != b'{' {
        return None;
    }
    let start = i + 1;
    let mut k = start;
    while k < limit
        && (b[k].is_ascii_alphanumeric() || matches!(b[k], b'*' | b'@' | b':' | b'_' | b'-'))
    {
        k += 1;
    }
    (k > start && k < limit && b[k] == b'}').then_some((i, start, k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(s: &str) -> Vec<(Tok, &str)> {
        let mut out = Vec::new();
        let mut p = 0;
        while p < s.len() {
            let (t, e) = next(s, p, s.len(), false);
            out.push((t, &s[p..e]));
            p = e;
        }
        out
    }

    #[test]
    fn tokens() {
        use Tok::*;
        assert_eq!(
            lex("\\emph{a}  b\\\\ %c\n\n  \nx\r\ny\\"),
            vec![
                (ControlWord, "\\emph"),
                (LBrace, "{"),
                (Text, "a"),
                (RBrace, "}"),
                (Whitespace, "  "),
                (Text, "b"),
                (ControlSymbol, "\\\\"),
                (Whitespace, " "),
                (Comment, "%c"),
                (ParBreak, "\n\n  \n"),
                (Text, "x"),
                (Newline, "\r\n"),
                (Text, "y"),
                (ControlSymbol, "\\"),
            ]
        );
        assert_eq!(lex("\\é")[0], (ControlSymbol, "\\é"));
    }

    #[test]
    fn verbatim_arguments() {
        let b = b"\\verb|a{b|c";
        assert_eq!(verb_end(b, 5, b.len(), false), Some(10));
        assert_eq!(verb_end(b"\\verb|abc\n|", 5, 11, false), None);
        let l = b"\\lstinline[x]{a{b}c} d";
        assert_eq!(verb_end(l, 10, l.len(), true), Some(20));
        assert_eq!(raw_braces(b"\\url {a%b}", 4, 10), Some((5, 9)));
        assert_eq!(env_name(b"\\begin {align*}x", 6, 16), Some((7, 8, 14)));
    }
}
