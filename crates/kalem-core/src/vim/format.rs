//! `gq` and `gw`: lines formatted to a width as Vim's internal
//! formatting does with its defaults ('formatoptions' `tcq`, the default
//! 'comments'): the lines of a paragraph joined and broken again at
//! blanks, so that each fits in the width where a blank allows. A
//! comment's leader (`#`, `//`, `%`, `>` nested) begins each of its
//! lines; a list item (`- `) begins a paragraph, its next lines indented
//! under its text; blank lines end paragraphs. In Org and Markdown, as
//! Doom Emacs's filling there, headings, tables, keyword lines, blocks
//! and the lines in them stay as they are, and `+ `, `1. ` and `1) ` (and
//! in Markdown `* `) are list items too.

use unicode_width::UnicodeWidthChar;

/// The kind of text, for what is a list item and what stays as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Text,
    Org,
    Markdown,
}

/// How a line begins: its indent, then a comment's leader or a list
/// item's marker (with the blanks after them).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Prefix {
    indent: String,
    leader: String,
    /// The leader is a list item's marker: only the first line has it.
    item: bool,
}

/// The width `s` shows in from column `col`, tabs every `ts`.
fn width_from(s: &str, col: usize, ts: usize) -> usize {
    s.chars().fold(col, |c, ch| {
        if ch == '\t' {
            (c / ts + 1) * ts
        } else {
            c + ch.width().unwrap_or(0)
        }
    }) - col
}

/// The comment leader or list marker `rest` (a line after its indent)
/// begins with, and its length with the blanks after it.
fn leader(rest: &str, kind: Kind) -> Option<(usize, bool)> {
    let blanks =
        |at: usize| at + (rest[at..].len() - rest[at..].trim_start_matches([' ', '\t']).len());
    let blank_after = |n: usize| rest[n..].starts_with([' ', '\t']);
    // `>` nested: `> > text`.
    if rest.starts_with('>') {
        let mut at = 0;
        while rest[at..].starts_with('>') {
            at = blanks(at + 1);
        }
        return Some((at, false));
    }
    if rest.starts_with("//") {
        return Some((blanks(2), false));
    }
    if rest.starts_with('%') && kind != Kind::Markdown {
        return Some((blanks(1), false));
    }
    if rest.starts_with('#') && blank_after(1) && kind != Kind::Markdown {
        return Some((blanks(1), false));
    }
    // List items.
    let bullets: &[char] = match kind {
        Kind::Text => &['-'],
        Kind::Org => &['-', '+'],
        Kind::Markdown => &['-', '+', '*'],
    };
    if rest.starts_with(bullets) && blank_after(1) {
        return Some((blanks(1), true));
    }
    if kind != Kind::Text {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && rest[digits..].starts_with(['.', ')']) && blank_after(digits + 1) {
            return Some((blanks(digits + 1), true));
        }
    }
    None
}

fn prefix(line: &str, kind: Kind) -> Prefix {
    let rest = line.trim_start_matches([' ', '\t']);
    let indent = line[..line.len() - rest.len()].to_string();
    match leader(rest, kind) {
        Some((n, item)) => Prefix {
            indent,
            leader: rest[..n].to_string(),
            item,
        },
        None => Prefix {
            indent,
            leader: String::new(),
            item: false,
        },
    }
}

/// Whether `line` stays as it is in `kind`, and whether it opens (or
/// closes) a block whose lines stay too.
fn fixed(line: &str, kind: Kind) -> (bool, bool) {
    let t = line.trim_start();
    match kind {
        Kind::Text => (false, false),
        Kind::Org => {
            let lower = t.to_ascii_lowercase();
            let block = lower.starts_with("#+begin") || lower.starts_with("#+end");
            let heading = line.starts_with('*') && line.trim_start_matches('*').starts_with(' ');
            let drawer = t.starts_with(':') && t.trim_end().ends_with(':') && t.len() > 1;
            (
                block || heading || t.starts_with('|') || t.starts_with("#+") || drawer,
                block,
            )
        }
        Kind::Markdown => {
            let fence = t.starts_with("```") || t.starts_with("~~~");
            let rule = matches!(t.trim_end(), "---" | "***" | "___");
            let heading = t.starts_with('#');
            (fence || rule || heading || t.starts_with('|'), fence)
        }
    }
}

/// `line` broken at blanks to fit in `width`, its next lines beginning
/// with `next`: Vim's breaking, at the last blank where the text before
/// it fits, else at the first blank after.
fn break_line(
    line: String,
    keep: usize,
    next: &str,
    width: usize,
    ts: usize,
    out: &mut Vec<String>,
) {
    let mut line = line;
    // No break within a line's prefix (`keep` bytes of the first).
    let mut keep = keep;
    loop {
        if width_from(&line, 0, ts) <= width {
            out.push(line);
            return;
        }
        // The blanks a break may go at, and the width before each.
        let mut best = None;
        let mut first_after = None;
        let mut col = 0;
        let mut prev_blank = true;
        for (i, ch) in line.char_indices() {
            let blank = ch == ' ' || ch == '\t';
            if blank && !prev_blank && i > keep {
                if col <= width {
                    best = Some(i);
                } else if first_after.is_none() {
                    first_after = Some(i);
                }
            }
            prev_blank = blank;
            col += width_from(&line[i..i + ch.len_utf8()], col, ts);
        }
        let Some(at) = best.or(first_after) else {
            out.push(line);
            return;
        };
        let rest = line[at..].trim_start_matches([' ', '\t']);
        if rest.is_empty() {
            out.push(line[..at].to_string());
            return;
        }
        let rest = format!("{next}{rest}");
        line.truncate(at);
        out.push(line.trim_end_matches([' ', '\t']).to_string());
        keep = next.len();
        line = rest;
    }
}

/// `lines` formatted to `width` (tabs every `ts`).
pub(super) fn format(lines: &[&str], width: usize, ts: usize, kind: Kind) -> Vec<String> {
    let ts = ts.max(1);
    let width = width.max(1);
    let mut out = Vec::new();
    let mut in_block = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let (stays, edge) = fixed(line, kind);
        if edge {
            in_block = !in_block;
        }
        if line.trim().is_empty() || stays || in_block {
            out.push(line.to_string());
            i += 1;
            continue;
        }
        let first = prefix(line, kind);
        // The next lines' prefix: a comment's leader again, or blanks
        // under a list item's text.
        let next = if first.item {
            format!(
                "{}{}",
                first.indent,
                " ".repeat(width_from(
                    &first.leader,
                    width_from(&first.indent, 0, ts),
                    ts
                ))
            )
        } else {
            format!("{}{}", first.indent, first.leader)
        };
        let mut joined = line.trim_end_matches([' ', '\t']).to_string();
        i += 1;
        while i < lines.len() {
            let l = lines[i];
            if l.trim().is_empty() || fixed(l, kind).0 {
                break;
            }
            let p = prefix(l, kind);
            if p.item || p.leader.trim() != first.leader.trim() && !first.item {
                break;
            }
            if first.item && !p.leader.is_empty() {
                break;
            }
            let body = l[p.indent.len() + p.leader.len()..].trim();
            if !body.is_empty() {
                // One blank between, none after one already there.
                if !joined.ends_with([' ', '\t']) {
                    joined.push(' ');
                }
                joined.push_str(body);
            }
            i += 1;
        }
        let keep = first.indent.len() + first.leader.len();
        break_line(joined, keep, &next, width, ts, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(text: &str, width: usize, kind: Kind) -> String {
        let lines: Vec<&str> = text.lines().collect();
        format(&lines, width, 8, kind).join("\n")
    }

    #[test]
    fn paragraphs_joined_and_broken() {
        assert_eq!(
            fmt("aaa bbb ccc\nddd eee\n\nfff", 8, Kind::Text),
            "aaa bbb\nccc ddd\neee\n\nfff"
        );
        // A word longer than the width keeps its line.
        assert_eq!(fmt("a verylongword b", 5, Kind::Text), "a\nverylongword\nb");
        // The first line's indent for the next lines.
        assert_eq!(fmt("  aa bb cc", 6, Kind::Text), "  aa\n  bb\n  cc");
        // Blanks within a line stay; a join puts one.
        assert_eq!(fmt("a  b\nc", 20, Kind::Text), "a  b c");
    }

    #[test]
    fn comments_and_items() {
        assert_eq!(fmt("# aa bb cc\n# dd", 8, Kind::Text), "# aa bb\n# cc dd");
        assert_eq!(fmt("> > aa bb cc", 9, Kind::Text), "> > aa bb\n> > cc");
        assert_eq!(
            fmt("- aa bb cc\n- dd", 8, Kind::Text),
            "- aa bb\n  cc\n- dd"
        );
        assert_eq!(fmt("// aa\n// bb", 20, Kind::Text), "// aa bb");
        // A comment's lines and the text's do not join.
        assert_eq!(fmt("# aa\nbb", 20, Kind::Text), "# aa\nbb");
    }

    #[test]
    fn org_and_markdown_structure_stays() {
        let org = "* A heading that is long\nsome text\nmore\n| a | b |\n#+begin_src sh\necho a b c d\n#+end_src\n1. one two\n   three";
        assert_eq!(
            fmt(org, 12, Kind::Org),
            "* A heading that is long\nsome text\nmore\n| a | b |\n#+begin_src sh\necho a b c d\n#+end_src\n1. one two\n   three"
        );
        assert_eq!(fmt("+ aa bb cc", 8, Kind::Org), "+ aa bb\n  cc");
        assert_eq!(
            fmt(
                "## Title words\n* aa bb cc\n```\nx y z\n```",
                8,
                Kind::Markdown
            ),
            "## Title words\n* aa bb\n  cc\n```\nx y z\n```"
        );
    }
}
