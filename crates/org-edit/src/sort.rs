//! Sorting entries: `org-sort-entries`, with the record handling of
//! Emacs's `sort-subr`.
//!
//! Records are the headings of one level and what follows them up to the
//! next heading of that level. A record ends early, at the end of the
//! sorted text, when a heading of a higher level comes first; the sort
//! is stable, and a reversed sort keeps equal records in document order.

use jiff::civil::DateTime;
use org_model::{Document, Inherit, complex_heading_title, complex_heading_todo, string_to_number};
use org_syntax::ast::{AstNode, Link, LinkFormat};
use org_syntax::{NodeOrToken, ParseContext, SyntaxElement, SyntaxKind};

use crate::buffer::{Buf, EditError};
use crate::headline::{headings, org_back_to_heading as back_to_heading, stars_at};
use crate::transaction::Transaction;

/// What entries are sorted by: the sorting types of `org-sort-entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortBy {
    /// `a`: the heading text without TODO keyword, priority, `COMMENT` and
    /// tags, with emphasis markers removed and links replaced by their
    /// description (or target). Compared by code point, see
    /// `book/part-2/org-known-differences.org`.
    Alpha,
    /// `n`: the number the heading text starts with (0 if none).
    Numeric,
    /// `t`: the first active timestamp in the entry's own text, or else its
    /// first timestamp; entries without one sort as now.
    Time,
    /// `c`: the first inactive timestamp at the start of a line.
    Created,
    /// `s`: the SCHEDULED date.
    Scheduled,
    /// `d`: the DEADLINE date.
    Deadline,
    /// `p`: the priority cookie, or the default priority.
    Priority,
    /// `r`: the value of a property (empty if unset), compared by code
    /// point.
    Property(String),
    /// `o`: the TODO keyword, in the order of the keyword sequences: active
    /// states, then no keyword, then done states.
    TodoOrder,
    /// `k`: the clocked time of the subtree.
    Clocking,
}

/// How to sort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortOptions {
    /// The key.
    pub by: SortBy,
    /// Descending order (the capital letter sorting types).
    pub reverse: bool,
    /// For [`SortBy::Alpha`]: compare with case (the `WITH-CASE`
    /// argument); by default letters are downcased first.
    pub with_case: bool,
}

impl SortOptions {
    /// The options for an `org-sort-entries` sorting type character
    /// (`a`, `A`, `n`, …); `property` is used for `r` and `R`.
    pub fn from_char(c: char, property: Option<&str>) -> Option<SortOptions> {
        let by = match c.to_ascii_lowercase() {
            'a' => SortBy::Alpha,
            'n' => SortBy::Numeric,
            't' => SortBy::Time,
            'c' => SortBy::Created,
            's' => SortBy::Scheduled,
            'd' => SortBy::Deadline,
            'p' => SortBy::Priority,
            'r' => SortBy::Property(property?.to_string()),
            'o' => SortBy::TodoOrder,
            'k' => SortBy::Clocking,
            _ => return None,
        };
        Some(SortOptions {
            by,
            reverse: c.is_ascii_uppercase(),
            with_case: false,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Key {
    Num(f64),
    Str(String),
}

fn less(a: &Key, b: &Key) -> bool {
    match (a, b) {
        (Key::Num(x), Key::Num(y)) => x < y,
        (Key::Str(x), Key::Str(y)) => x < y,
        _ => false,
    }
}

/// `(org-end-of-subtree t t)` from the heading at `h`: the end of the
/// headline there, or of the headline around an inlinetask, or the end of
/// the text.
fn subtree_end(text: &str, h: usize, ctx: &ParseContext) -> usize {
    let limit = ctx.inlinetask_min_level;
    let hs = headings(text, limit);
    let headline = hs.iter().rev().find(|(s, _)| *s <= h);
    match headline {
        Some(&(s, level)) => hs
            .iter()
            .find(|(b, l)| *b > s && *l <= level)
            .map_or(text.len(), |(b, _)| *b),
        None => text.len(),
    }
}

/// `outline-next-heading` from `pos`: the next heading line, or the end.
fn next_heading(text: &str, pos: usize, end: usize) -> usize {
    headings(&text[..end], None)
        .into_iter()
        .find(|(s, _)| *s > pos)
        .map_or(end, |(s, _)| s)
}

fn at_bol(text: &str, pos: usize) -> bool {
    pos == 0 || text.as_bytes()[pos - 1] == b'\n'
}

/// `org-sort-entries`: sorts the entries in the region (`mark` to
/// `point`), else the children of the entry at `point`, else the top-level
/// entries. Positions are byte offsets in the document's text; `now` is
/// the time entries without a date sort as.
pub fn sort_entries(
    doc: &Document,
    point: usize,
    mark: Option<usize>,
    opts: &SortOptions,
    now: DateTime,
) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let ctx = doc.parse().context();
    let mut buf = Buf::new(&text, point);
    let region = mark.filter(|&m| m != point);
    let (start, mut end, beg);
    if let Some(m) = region {
        let (rs, re) = (point.min(m), point.max(m));
        let bol = buf.bol(rs);
        start = if stars_at(&text, bol).is_some() {
            bol
        } else {
            next_heading(&text, rs, text.len())
        };
        end = match back_to_heading(&text, re, ctx) {
            Some(h) => subtree_end(&text, h, ctx),
            None => text.len(),
        };
        beg = start;
    } else if let Some(h) = back_to_heading(&text, point, ctx) {
        start = h;
        let mut e = subtree_end(&text, h, ctx);
        if !at_bol(&buf.text, e) {
            buf.insert_before_point(e, "\n");
            e += 1;
        }
        // `org-back-over-empty-lines`, then one line down if it moved.
        let t = &buf.text;
        let q = t[..e].trim_end_matches([' ', '\t', '\n', '\r']).len();
        let after_q = t[q..].find('\n').map_or(t.len(), |i| q + i + 1);
        let p = after_q.min(e);
        end = if p < e {
            t[p..].find('\n').map_or(t.len(), |i| p + i + 1)
        } else {
            p
        };
        beg = next_heading(&buf.text, start, buf.text.len());
    } else {
        start = if stars_at(&text, 0).is_some() {
            0
        } else {
            next_heading(&text, 0, text.len())
        };
        let last = buf.bol(buf.text.len());
        if buf.text[last..]
            .chars()
            .any(|c| !matches!(c, ' ' | '\t' | '\r' | '\x0c'))
        {
            let len = buf.text.len();
            buf.insert_before_point(len, "\n");
        }
        end = buf.text.len();
        beg = start;
    }
    if beg >= end {
        return Err(EditError::at("Nothing to sort", start));
    }
    let n = buf.text[beg..].bytes().take_while(|b| *b == b'*').count();
    if n > 1 {
        // `^` followed by one star less and a blank, anywhere in the text.
        let mut txt = buf.text[beg..end].to_string();
        if !txt.ends_with('\n') {
            txt.push('\n');
        }
        let b = txt.as_bytes();
        let above = std::iter::once(0)
            .chain(txt.match_indices('\n').map(|(i, _)| i + 1))
            .any(|l| {
                b.len() > l + n - 1
                    && b[l..l + n - 1].iter().all(|c| *c == b'*')
                    && matches!(b[l + n - 1], b' ' | b'\t' | b'\n')
            });
        if above {
            return Err(EditError::at(
                "Region to sort contains a level above the first entry",
                beg,
            ));
        }
    }
    if !at_bol(&buf.text, end) {
        buf.insert_before_point(end, "\n");
        end += 1;
    }

    // The records, in document order.
    let t = buf.text.clone();
    // `outline-next-visible-heading` is `org-next-visible-heading` in Org
    // buffers, which skips inlinetasks.
    let hs: Vec<(usize, usize)> = headings(&t[..end], ctx.inlinetask_min_level)
        .into_iter()
        .filter(|(s, _)| *s >= beg)
        .collect();
    let mut records = Vec::new();
    let mut s = beg;
    while s < end {
        // `outline-forward-same-level`: the next heading of this level or
        // above; a higher one makes the record run to the end.
        let e = match hs.iter().find(|(b, l)| *b > s && *l <= n) {
            Some(&(b, l)) if l == n => b,
            _ => end,
        };
        records.push((s, e));
        s = e;
    }

    let clocks = matches!(opts.by, SortBy::Clocking).then(|| doc.clock_sums());
    let mut keyed = Vec::with_capacity(records.len());
    for &(s, e) in &records {
        let section_end = next_heading(&t, s, end);
        let key = record_key(doc, ctx, &t, s, section_end, opts, now, clocks.as_ref())
            .map_err(|m| EditError::at(&m, beg))?;
        keyed.push((key, s..e));
    }
    // `sort-subr`: records are collected last first, reversed back unless
    // the sort is reversed, sorted stably, and reversed when it is.
    if opts.reverse {
        keyed.reverse();
    }
    keyed.sort_by(|a, b| {
        if less(&a.0, &b.0) {
            std::cmp::Ordering::Less
        } else if less(&b.0, &a.0) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    if opts.reverse {
        keyed.reverse();
    }
    let mut sorted = String::with_capacity(end - beg);
    for (_, r) in &keyed {
        sorted.push_str(&t[r.clone()]);
    }
    buf.replace(beg, end, &sorted);
    buf.point = start;
    Ok(buf.transaction("Sort entries"))
}

#[allow(clippy::too_many_arguments)]
fn record_key(
    doc: &Document,
    ctx: &ParseContext,
    t: &str,
    s: usize,
    section_end: usize,
    opts: &SortOptions,
    now: DateTime,
    clocks: Option<&std::collections::HashMap<usize, i64>>,
) -> Result<Key, String> {
    let eol = t[s..].find('\n').map_or(t.len(), |i| s + i);
    let line = t[s..eol].trim_end_matches('\r');
    let time_key = |dt: Option<DateTime>| Key::Num(seconds_of(dt.unwrap_or(now)));
    Ok(match &opts.by {
        SortBy::Alpha | SortBy::Numeric => {
            // `org-get-heading` goes back to the heading, from the END line
            // of an inlinetask to its start.
            let h = back_to_heading(t, s, ctx).unwrap_or(s);
            let heol = t[h..].find('\n').map_or(t.len(), |i| h + i);
            let line = t[h..heol].trim_end_matches('\r');
            let todo = complex_heading_todo(line, ctx);
            let title = complex_heading_title(line, todo.as_deref()).unwrap_or_default();
            let title = strip_comment(&title);
            let visible = remove_invisible(title, ctx);
            if opts.by == SortBy::Numeric {
                Key::Num(string_to_number(&visible))
            } else if opts.with_case {
                Key::Str(visible)
            } else {
                Key::Str(visible.to_lowercase())
            }
        }
        SortBy::Clocking => Key::Num(clocks.and_then(|c| c.get(&s)).copied().unwrap_or(0) as f64),
        SortBy::Time => {
            let m = find_timestamp(t, s, section_end, &['<'], &['>'])
                .or_else(|| find_timestamp(t, s, section_end, &['<', '['], &['>', ']']));
            time_key(m.and_then(|(a, b)| org_model::time::parse_time_string(&t[a..b])))
        }
        SortBy::Created => time_key(
            find_created(t, s, section_end)
                .and_then(|(a, b)| org_model::time::parse_time_string(&t[a..b])),
        ),
        SortBy::Scheduled | SortBy::Deadline => {
            let word = if opts.by == SortBy::Scheduled {
                "SCHEDULED:"
            } else {
                "DEADLINE:"
            };
            match find_planning(t, s, section_end, word) {
                Some(inner) => match org_model::time::parse_time_string(inner) {
                    Some(dt) => Key::Num(seconds_of(dt)),
                    None => return Err(format!("Not an Org time string: {inner}")),
                },
                None => time_key(None),
            }
        }
        SortBy::Priority => {
            let default = doc.info().priorities.default;
            Key::Num(priority_char(line).map_or(default, |c| c as u32) as f64)
        }
        SortBy::Property(p) => {
            let entry = doc.outline().entry_at(s);
            Key::Str(
                doc.entry_get(entry, p, Inherit::No, false)
                    .unwrap_or_default(),
            )
        }
        SortBy::TodoOrder => {
            let keywords: Vec<&str> = ctx
                .todo_sequences
                .iter()
                .flat_map(|q| q.keywords.iter().map(|k| k.name.as_str()))
                .collect();
            match complex_heading_todo(line, ctx) {
                Some(m) => {
                    let tail = keywords
                        .iter()
                        .position(|k| *k == m.as_str())
                        .map_or(0, |i| keywords.len() - i);
                    let done = ctx.done_keywords.contains(&m);
                    Key::Num(if done {
                        99.0 + tail as f64
                    } else {
                        99.0 - tail as f64
                    })
                }
                None => Key::Num(99.0),
            }
        }
    })
}

/// Seconds of a civil time, for ordering.
fn seconds_of(dt: DateTime) -> f64 {
    dt.to_zoned(jiff::tz::TimeZone::UTC)
        .map_or(0.0, |z| z.timestamp().as_second() as f64)
}

/// `org-get-heading` with NO-COMMENT: `COMMENT` and blanks at the start
/// of the title removed.
fn strip_comment(title: &str) -> &str {
    match title.strip_prefix("COMMENT") {
        Some(r) if r.starts_with([' ', '\t']) => r.trim_start_matches([' ', '\t']),
        _ => title,
    }
}

/// `org-sort-remove-invisible`: the text of `s` with emphasis markers
/// removed, code and verbatim replaced by their value and links by their
/// description or target.
pub(crate) fn remove_invisible(s: &str, ctx: &ParseContext) -> String {
    // A line starting with a letter and a space is a paragraph whose
    // objects are parsed as those of `s` alone.
    let parse = org_syntax::parse_with(&format!("x {s}\n"), ctx);
    let root = parse.syntax();
    let Some(par) = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PARAGRAPH)
    else {
        return s.to_string();
    };
    let mut out = String::new();
    for c in par.children_with_tokens() {
        flatten(&c, ctx, &mut out);
    }
    let out = out.strip_suffix('\n').unwrap_or(&out);
    out.strip_prefix("x ").unwrap_or(out).to_string()
}

fn flatten(el: &SyntaxElement, ctx: &ParseContext, out: &mut String) {
    use SyntaxKind::*;
    let n = match el {
        NodeOrToken::Token(t) => {
            out.push_str(t.text());
            return;
        }
        NodeOrToken::Node(n) => n,
    };
    match n.kind() {
        BOLD | ITALIC | UNDERLINE | STRIKE_THROUGH | CODE | VERBATIM => {
            for c in n.children_with_tokens().filter(|c| c.kind() != MARKER) {
                flatten(&c, ctx, out);
            }
        }
        LINK => {
            let Some(link) = Link::cast(n.clone()) else {
                return;
            };
            let described = link.format() == LinkFormat::Bracket && link.description().is_some();
            for c in n.children_with_tokens().filter(|c| c.kind() != MARKER) {
                if c.kind() == CODE_TEXT {
                    if !described {
                        out.push_str(&link.info(ctx).raw_link);
                    }
                } else {
                    flatten(&c, ctx, out);
                }
            }
        }
        _ => {
            for c in n.children_with_tokens() {
                flatten(&c, ctx, out);
            }
        }
    }
}

/// `YYYY-MM-DD` with ASCII digits at `i`.
fn date_at(b: &[u8], i: usize) -> bool {
    b.len() >= i + 10
        && b[i..i + 10].iter().enumerate().all(|(k, c)| match k {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// The first `OPEN DATE(?: .*?)? CLOSE` in `from..to` (`org-ts-regexp`,
/// `org-ts-regexp-both`).
fn find_timestamp(
    t: &str,
    from: usize,
    to: usize,
    open: &[char],
    close: &[char],
) -> Option<(usize, usize)> {
    let b = t.as_bytes();
    for (i, c) in t[from..to].char_indices() {
        let i = from + i;
        if !open.contains(&c) || !date_at(b, i + 1) {
            continue;
        }
        let after = i + 11;
        let end = match b.get(after) {
            Some(&c) if close.contains(&(c as char)) => Some(after + 1),
            Some(b' ') => {
                let line_end = t[after..].find('\n').map_or(t.len(), |k| after + k);
                t[after + 1..line_end]
                    .find(close)
                    .map(|k| after + 1 + k + 1)
            }
            _ => None,
        };
        if let Some(e) = end.filter(|e| *e <= to) {
            return Some((i, e));
        }
    }
    None
}

/// `^[ \t]*\[` `org-ts-regexp1` `\]` in `from..to`: an inactive date at
/// the start of a line.
fn find_created(t: &str, from: usize, to: usize) -> Option<(usize, usize)> {
    let b = t.as_bytes();
    let mut l = from;
    while l < to {
        let next = t[l..].find('\n').map_or(t.len(), |k| l + k + 1);
        let mut i = l;
        while matches!(b.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        if b.get(i) == Some(&b'[') && date_at(b, i + 1) {
            let mut k = i + 11;
            // ` *[^]+0-9>\r\n -]+`, all of it or nothing.
            let mut j = k;
            while b.get(j) == Some(&b' ') {
                j += 1;
            }
            let name_end = t[j..]
                .char_indices()
                .find(|(_, c)| {
                    matches!(c, ']' | '+' | '>' | '\r' | '\n' | ' ' | '-') || c.is_ascii_digit()
                })
                .map_or(t.len(), |(x, _)| j + x);
            if name_end > j {
                k = name_end;
            }
            // `\( \([0-9]\{1,2\}\):\([0-9]\{2\}\)\)?`
            let time_end = (b.get(k) == Some(&b' ')).then(|| {
                let d = b[k + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                (matches!(d, 1 | 2)
                    && b.get(k + 1 + d) == Some(&b':')
                    && b.get(k + 2 + d).is_some_and(u8::is_ascii_digit)
                    && b.get(k + 3 + d).is_some_and(u8::is_ascii_digit))
                .then_some(k + 4 + d)
            });
            let k = time_end.flatten().unwrap_or(k);
            if b.get(k) == Some(&b']') && k < to {
                return Some((l, k + 1));
            }
        }
        l = next;
    }
    None
}

/// `\<WORD *<\([^>]+\)>` in `from..to`: the text inside the angle brackets.
fn find_planning<'a>(t: &'a str, from: usize, to: usize, word: &str) -> Option<&'a str> {
    let hay = &t[from..to];
    for (i, _) in hay.match_indices(word) {
        let at = from + i;
        if t[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        let mut j = at + word.len();
        while t.as_bytes().get(j) == Some(&b' ') {
            j += 1;
        }
        if j >= to || t.as_bytes()[j] != b'<' {
            continue;
        }
        let Some(k) = t[j + 1..to].find('>') else {
            continue;
        };
        if k > 0 {
            return Some(&t[j + 1..j + 1 + k]);
        }
    }
    None
}

/// The first character of the first `[#X]` cookie (`[A-Z0-9]+`) on the
/// heading line.
fn priority_char(line: &str) -> Option<char> {
    let b = line.as_bytes();
    line.match_indices("[#").find_map(|(i, _)| {
        let n = b[i + 2..]
            .iter()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            .count();
        (n > 0 && b.get(i + 2 + n) == Some(&b']')).then(|| b[i + 2] as char)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_parts_are_removed() {
        let ctx = org_syntax::parse("").context().clone();
        assert_eq!(
            remove_invisible("a *bold*  x [[https://a.b][de *sc*]] ~c d~ [[t]] y", &ctx),
            "a bold  x de sc c d t y"
        );
        assert_eq!(remove_invisible("<https://x.y> z", &ctx), "https://x.y z");
    }

    #[test]
    fn keys() {
        assert_eq!(priority_char("* TODO [#a] [#B2] x"), Some('B'));
        assert_eq!(strip_comment("COMMENT  x"), "x");
        assert_eq!(strip_comment("COMMENTS"), "COMMENTS");
        let t = "x [2026-01-02 Fri] <2026-01-03>\n";
        assert_eq!(
            find_timestamp(t, 0, t.len(), &['<'], &['>']),
            Some((19, 31))
        );
        assert_eq!(
            find_created("  [2026-01-02 Fri 9:30]\n", 0, 24),
            Some((0, 23))
        );
        assert_eq!(find_created("[2026-01-02 Fri 9:30-10:00]\n", 0, 28), None);
    }
}
