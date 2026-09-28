//! Filling text as Emacs's `fill-region` does in a buffer of plain text
//! (`fill-column`, `sentence-end-double-space`, adaptive fill prefixes and
//! hard newlines), for the plain text back-end.

use unicode_width::UnicodeWidthChar;

/// Marks a hard newline (`hard-newline`): the `\n` after it is kept by
/// [`fill`] and the segments on each side are filled apart.
pub const HARD: char = '\u{E002}';

/// How lines are justified (`justify-current-line`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    /// Left: nothing moves.
    Left,
    /// Centered in the width.
    Center,
    /// Flush right at the width.
    Right,
}

/// The display width of a character, as `char-width` (the hard newline
/// mark has none).
pub fn char_width(c: char) -> usize {
    if c == HARD || c == '\n' {
        0
    } else if c == '\t' {
        8
    } else {
        c.width().unwrap_or(0)
    }
}

/// `string-width`.
pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// `org-ascii--fill-string` (`fill-region` with `use-hard-newlines`): each
/// part between hard newlines filled to `column`.
pub fn fill(s: &str, column: usize, justify: Justify) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut rest = s;
    loop {
        let (segment, hard) = match rest.find(HARD) {
            Some(i) => {
                let after = &rest[i + HARD.len_utf8()..];
                let nl = usize::from(after.starts_with('\n'));
                (&rest[..i], Some(&rest[i..i + HARD.len_utf8() + nl]))
            }
            None => (rest, None),
        };
        out.push_str(&fill_paragraph(segment, column, justify));
        match hard {
            Some(h) => {
                out.push_str(h);
                rest = &rest[segment.len() + h.len()..];
            }
            None => break,
        }
    }
    out
}

/// The sentence-end characters and the closing ones that may follow.
fn sentence_char(c: char) -> bool {
    matches!(c, '.' | '?' | '!' | '…' | '‽')
}

fn closing_char(c: char) -> bool {
    matches!(c, ']' | '"' | '\'' | '”' | '’' | ')' | '}' | '»' | '›')
}

/// The adaptive fill prefix at the start of `line`
/// (`[-–!|#%;>*·•‣⁃◦ \t]*`).
fn adaptive_prefix(line: &str, column: usize) -> String {
    let p: String = line
        .chars()
        .take_while(|c| {
            matches!(
                c,
                '-' | '–'
                    | '!'
                    | '|'
                    | '#'
                    | '%'
                    | ';'
                    | '>'
                    | '*'
                    | '·'
                    | '•'
                    | '‣'
                    | '⁃'
                    | '◦'
                    | ' '
                    | '\t'
            )
        })
        .collect();
    // Death to insanely long prefixes.
    if p.chars().count() >= column {
        String::new()
    } else {
        p
    }
}

/// `fill-context-prefix` for the lines of a paragraph.
fn context_prefix(lines: &[&str], column: usize) -> String {
    let first = adaptive_prefix(lines[0], column);
    if lines.len() > 1 {
        let second = adaptive_prefix(lines[1], column);
        // The second line's prefix if its non-blank parts appear in the
        // first line's, else their common prefix.
        let words: Vec<&str> = second
            .split([' ', '\t'])
            .filter(|w| !w.is_empty())
            .collect();
        let mut at = 0;
        let mut ok = true;
        for w in words {
            match first[at..].find(w) {
                Some(i) => at += i + w.len(),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return second;
        }
        let common = first
            .chars()
            .zip(second.chars())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.len_utf8())
            .sum();
        return first[..common].to_string();
    }
    if first.chars().all(|c| c == ' ' || c == '\t') {
        first
    } else {
        " ".repeat(width(&first))
    }
}

/// `fill-region-as-paragraph` on one segment.
fn fill_paragraph(s: &str, column: usize, justify: Justify) -> String {
    // Leading blank lines stay as they are; so do trailing line feeds but
    // one.
    let start = s.len() - s.trim_start_matches([' ', '\t', '\n']).len();
    let bol = s[..start].rfind('\n').map_or(0, |i| i + 1);
    let lead = &s[..bol];
    let mut end = s.len();
    let mut trailing = String::new();
    if s[bol..].trim().is_empty() {
        return s.to_string();
    }
    while end > bol && s[..end].ends_with('\n') {
        end -= 1;
    }
    if end < s.len() {
        trailing.push('\n');
    }
    let body = &s[bol..end];
    let lines: Vec<&str> = body.split('\n').collect();
    let prefix = context_prefix(&lines, column);
    // The first line keeps its prefix; the others lose theirs.
    let first_indent = if matches!(justify, Justify::Right | Justify::Center) {
        String::new()
    } else {
        let n = lines[0].len() - lines[0].trim_start_matches([' ', '\t']).len();
        lines[0][..n].to_string()
    };
    let mut text = String::new();
    for (i, l) in lines.iter().enumerate() {
        let l = if i == 0 {
            l.trim_start_matches([' ', '\t'])
        } else {
            l.strip_prefix(prefix.as_str()).unwrap_or(l)
        };
        if i > 0 {
            // A sentence ending a line gets a second space.
            let t = text.trim_end_matches(closing_char);
            if t.ends_with(sentence_char) && !text.ends_with([' ', '\t']) {
                text.push(' ');
            }
            text.push(' ');
        }
        text.push_str(l);
    }
    let text = canonical_spaces(&text);
    let text = text.trim_end_matches([' ', '\t']);
    // The filling loop.
    let chars: Vec<char> = text.chars().collect();
    let mut lines_out: Vec<String> = Vec::new();
    let mut pos = 0;
    let mut first = true;
    let base = |first: bool| {
        if first {
            width(&first_indent)
        } else {
            width(&prefix)
        }
    };
    while pos < chars.len() {
        let linebeg = pos;
        // `move-to-column`.
        let mut col = base(first);
        let mut p = pos;
        while p < chars.len() && col + char_width(chars[p]) <= column {
            col += char_width(chars[p]);
            p += 1;
        }
        if p >= chars.len() {
            lines_out.push(chars[linebeg..].iter().collect());
            break;
        }
        // Look at the character at the column too: a space there is a
        // break point.
        let mut q = (p + 1).min(chars.len());
        // `fill-move-to-break-point`.
        let mut found = None;
        while q > linebeg {
            let Some(i) = (linebeg..q).rev().find(|&i| chars[i] == ' ') else {
                break;
            };
            let after = i + 1;
            if nobreak(&chars, after, linebeg) {
                // Skip back over the spaces and look again.
                let mut k = after;
                while k > linebeg && chars[k - 1] == ' ' {
                    k -= 1;
                }
                q = k;
                continue;
            }
            found = Some(after);
            break;
        }
        let mut brk = match found {
            Some(a) => {
                let mut k = a;
                while k > linebeg && chars[k - 1] == ' ' {
                    k -= 1;
                }
                k
            }
            None => linebeg,
        };
        if brk <= linebeg {
            // At least one word, and not after a period followed by one
            // space.
            let mut k = linebeg;
            let mut first_word = true;
            while k < chars.len() && (first_word || nobreak(&chars, k, linebeg)) {
                while k < chars.len() && chars[k] == ' ' {
                    k += 1;
                }
                while k < chars.len() && chars[k] != ' ' {
                    k += 1;
                }
                first_word = false;
            }
            brk = k;
        }
        let mut next = brk;
        while next < chars.len() && chars[next] == ' ' {
            next += 1;
        }
        if next >= chars.len() {
            lines_out.push(
                chars[linebeg..]
                    .iter()
                    .collect::<String>()
                    .trim_end()
                    .to_string(),
            );
            break;
        }
        lines_out.push(chars[linebeg..brk].iter().collect());
        pos = next;
        first = false;
    }
    let mut out = String::from(lead);
    for (i, l) in lines_out.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let indent = if i == 0 { &first_indent } else { &prefix };
        let line = format!("{indent}{l}");
        out.push_str(&justify_line(&line, column, justify, indent, true));
    }
    out.push_str(&trailing);
    out
}

/// `fill-nobreak-p` at `at`, just after a space: no break after a period
/// followed by a single space.
fn nobreak(chars: &[char], at: usize, linebeg: usize) -> bool {
    if at <= linebeg {
        return false;
    }
    let mut k = at;
    while k > linebeg && chars[k - 1] == ' ' {
        k -= 1;
    }
    k > 0
        && chars[k - 1] == '.'
        && chars.get(k) == Some(&' ')
        && chars.get(k + 1).is_some_and(|c| *c != ' ' && *c != '\n')
}

/// `canonically-space-region` with `sentence-end-double-space`: tabs as
/// spaces, two spaces after a sentence, one elsewhere.
fn canonical_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().map(|c| if c == '\t' { ' ' } else { c }).collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != ' ' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j] == ' ' {
            j += 1;
        }
        let n = j - i;
        // What precedes, closing characters skipped.
        let mut k = i;
        while k > 0 && closing_char(chars[k - 1]) {
            k -= 1;
        }
        let after_sentence = k > 0 && sentence_char(chars[k - 1]);
        let keep = if i == 0 {
            // Leading spaces stay.
            n
        } else if n == 1 {
            1
        } else if after_sentence {
            2
        } else {
            1
        };
        out.extend(std::iter::repeat_n(' ', keep));
        i = j;
    }
    out
}

/// Whitespace from column `from` to column `to`, with tabs when `tabs`
/// (`indent-to` with `indent-tabs-mode`).
pub fn indentation(from: usize, to: usize, tabs: bool) -> String {
    if to <= from {
        return String::new();
    }
    if !tabs {
        return " ".repeat(to - from);
    }
    let mut out = String::new();
    let mut col = from;
    while (col / 8 + 1) * 8 <= to {
        out.push('\t');
        col = (col / 8 + 1) * 8;
    }
    out.push_str(&" ".repeat(to - col));
    out
}

/// `justify-current-line` for one line whose fill prefix is `prefix`;
/// the indentation has tabs when `tabs` (`indent-tabs-mode`).
fn justify_line(line: &str, column: usize, how: Justify, prefix: &str, tabs: bool) -> String {
    let line = line.trim_end_matches([' ', '\t']);
    if how == Justify::Left || line.trim().is_empty() {
        return line.to_string();
    }
    let body = line.strip_prefix(prefix).unwrap_or(line);
    let text = body.trim_start_matches([' ', '\t']);
    let pw = width(prefix);
    let indent = pw + width(&body[..body.len() - text.len()]);
    let endcol = indent + width(text);
    let target = match how {
        Justify::Right => (indent + column).saturating_sub(endcol),
        _ => (column.saturating_sub(endcol - indent)) / 2,
    };
    let target = target.max(pw);
    format!("{prefix}{}{text}", indentation(pw, target, tabs))
}

/// `org-ascii--justify-lines`: each line justified in `column`.
pub fn justify_lines(s: &str, column: usize, how: Justify) -> String {
    s.split('\n')
        .map(|l| {
            if how == Justify::Left {
                l.to_string()
            } else {
                justify_line(l, column, how, "", false)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filling() {
        let t = "Special strings -- an en dash --- an em dash ... and a shy hyphen.\nSub and superscripts: H_2O and x^2 and E=mc^{2}.";
        assert_eq!(
            fill(t, 72, Justify::Left),
            "Special strings -- an en dash --- an em dash ... and a shy hyphen.  Sub\nand superscripts: H_2O and x^2 and E=mc^{2}."
        );
        // No break after a period and one space.
        assert_eq!(fill("aaaa e.g. bbbb", 9, Justify::Left), "aaaa\ne.g. bbbb");
        // Hard newlines.
        assert_eq!(
            fill(&format!("a b{HARD}\nc d"), 3, Justify::Left),
            format!("a b{HARD}\nc d")
        );
        assert_eq!(fill("word", 10, Justify::Right), "      word");
        assert_eq!(fill("word", 10, Justify::Center), "   word");
        assert_eq!(fill("word", 30, Justify::Right), "\t\t\t  word");
        assert_eq!(canonical_spaces("a  b.   c"), "a b.  c");
    }
}
