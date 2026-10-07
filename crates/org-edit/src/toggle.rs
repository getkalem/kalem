//! `org-toggle-heading` and `org-toggle-item`: headings to text or items
//! and back, over the line at point or the lines of a region.

use org_syntax::{ParseContext, SyntaxKind};

use crate::buffer::{Buf, EditError, column_at};
use crate::headline::stars_at;
use crate::transaction::Transaction;

/// The start of the line after the one at `pos` (the end of the text on
/// the last line).
fn next_line(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1)
}

/// A heading line at `bol`, inlinetasks left out
/// (`org-with-limited-levels`).
fn heading_at(text: &str, bol: usize, ctx: &ParseContext) -> bool {
    stars_at(text, bol).is_some_and(|n| ctx.inlinetask_min_level.is_none_or(|l| n < l))
}

/// `org-comment-regexp` at the line `bol`.
fn comment_at(text: &str, bol: usize) -> bool {
    let line = &text[bol..text[bol..].find('\n').map_or(text.len(), |i| bol + i)];
    let rest = line.trim_start_matches([' ', '\t']);
    rest == "#" || rest.starts_with("# ")
}

/// The end of `org-item-re`'s match at the line `bol` (its bullet and the
/// blanks after it), when `org-at-item-p`: the line also starts an item
/// in the parse.
fn item_at(text: &str, bol: usize, ctx: &ParseContext) -> Option<usize> {
    let eol = text[bol..].find('\n').map_or(text.len(), |i| bol + i);
    let line = &text[bol..eol];
    let b = line.as_bytes();
    let ind = b.iter().take_while(|c| matches!(c, b' ' | b'\t')).count();
    let bullet_end = match b.get(ind) {
        Some(b'-' | b'+') => ind + 1,
        Some(b'*') if ind > 0 => ind + 1,
        Some(c) if c.is_ascii_digit() => {
            let n = b[ind..].iter().take_while(|c| c.is_ascii_digit()).count();
            if matches!(b.get(ind + n), Some(b'.' | b')')) {
                ind + n + 1
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let blanks = b[bullet_end..]
        .iter()
        .take_while(|c| matches!(c, b' ' | b'\t'))
        .count();
    if blanks == 0 && bullet_end < b.len() {
        return None;
    }
    let parse = org_syntax::parse_with(text, ctx);
    let el = crate::narrow::element_at(&parse.syntax(), bol)?;
    matches!(el.kind(), SyntaxKind::ITEM | SyntaxKind::PLAIN_LIST)
        .then_some(bol + bullet_end + blanks)
}

/// `org-reduced-level`.
fn reduced(level: usize, ctx: &ParseContext) -> usize {
    if ctx.odd_levels_only {
        1 + level / 2
    } else {
        level
    }
}

/// The level of the entry at `pos` (`org-current-level`), 0 before the
/// first heading.
fn current_level(text: &str, pos: usize, ctx: &ParseContext) -> usize {
    crate::headline::back_to_heading(text, pos, ctx.inlinetask_min_level).map_or(0, |(_, l)| l)
}

/// The region `mark`..`point`, or the line at `point`, as the two
/// commands take it: from the first line that is not blank (and, for
/// headings, not a comment), to the region's end (its line's end when it
/// is in a line).
fn bounds(
    text: &str,
    point: usize,
    mark: Option<usize>,
    skip_comments: bool,
    whole_last_line: bool,
) -> (usize, usize) {
    let skip = |pos: usize| {
        let mut p = text[..pos].rfind('\n').map_or(0, |i| i + 1);
        while skip_comments && p < text.len() && comment_at(text, p) {
            p = next_line(text, p);
        }
        let q = p + text[p..]
            .bytes()
            .take_while(|c| matches!(c, b' ' | b'\r' | b'\t' | b'\n'))
            .count();
        text[..q].rfind('\n').map_or(0, |i| i + 1)
    };
    let eol = |p: usize| text[p..].find('\n').map_or(text.len(), |i| p + i);
    match mark.filter(|&m| m != point) {
        Some(m) => {
            let (b, e) = (m.min(point), m.max(point));
            let bolp = e == 0 || text.as_bytes()[e - 1] == b'\n';
            let end = if bolp || !whole_last_line { e } else { eol(e) };
            (skip(b), end)
        }
        None => {
            let bol = text[..point].rfind('\n').map_or(0, |i| i + 1);
            let beg = if skip_comments { skip(bol) } else { bol };
            (beg, eol(point))
        }
    }
}

/// `org-toggle-heading`: in the region, or on the line at point,
/// headings become text (their stars go); else items become headings, a
/// checkbox a TODO or DONE keyword; else lines of text become headings
/// one level below the entry they are in.
pub fn toggle_heading(
    text: &str,
    point: usize,
    mark: Option<usize>,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    let (beg, end) = bounds(text, point, mark, true, true);
    let mut buf = Buf::new(text, point);
    let end = buf.add_marker(end);
    let mut p = beg;
    if heading_at(&buf.text, p, ctx) {
        while p < buf.marker(end) {
            if heading_at(&buf.text, p, ctx) {
                let n = stars_at(&buf.text, p).unwrap_or(0);
                buf.delete(p, p + n + 1);
            }
            p = next_line(&buf.text, p);
        }
    } else if item_at(&buf.text, p, ctx).is_some() {
        // The level of the entry the text after the items is in, as Emacs
        // asks once it has taken them out: a heading right after them is
        // that entry.
        let after = next_line(&buf.text, buf.marker(end).max(p));
        let from = if after < buf.text.len() && heading_at(&buf.text, after, ctx) {
            after
        } else {
            p
        };
        let level = match current_level(&buf.text, from, ctx) {
            0 => 1,
            l => reduced(l, ctx) + 1,
        };
        // `org-list-to-subtree` over the items of the region: each item's
        // line a heading, one level deeper for each level of nesting.
        let mut indents: Vec<usize> = Vec::new();
        // Emacs takes the items out and writes the headings in their place:
        // a point among them goes to their start.
        let first = p;
        let inside = point >= first && point <= buf.marker(end);
        while p < buf.marker(end) {
            if let Some(after) = item_at(&buf.text, p, ctx) {
                let ind = column_at(
                    &buf.text,
                    p + buf.text[p..].len() - buf.text[p..].trim_start_matches([' ', '\t']).len(),
                );
                while indents.last().is_some_and(|&i| i >= ind) && indents.len() > 1 {
                    indents.pop();
                }
                if indents.last().is_none_or(|&i| i < ind) {
                    indents.push(ind);
                }
                let depth = indents.len();
                let eol = buf.text[p..].find('\n').map_or(buf.text.len(), |i| p + i);
                let rest = buf.text[after..eol].to_string();
                let (keyword, rest) = checkbox_keyword(&rest, ctx);
                let rest = match rest.split_once(" :: ") {
                    Some((term, desc)) => format!(" {term} {desc}"),
                    None => rest.to_string(),
                };
                let oddeven = level + depth - 1;
                let stars = "*".repeat(if ctx.odd_levels_only {
                    2 * oddeven - 1
                } else {
                    oddeven
                });
                buf.replace(p, eol, &format!("{stars} {keyword}{rest}"));
            }
            p = next_line(&buf.text, p);
        }
        if inside {
            buf.point = first;
        }
    } else {
        let level = current_level(&buf.text, p, ctx);
        let stars = "*".repeat(level);
        let add = if level == 0 || !ctx.odd_levels_only {
            "*"
        } else {
            "**"
        };
        let rpl = format!("{stars}{add} ");
        while p < buf.marker(end) {
            let eol = buf.text[p..].find('\n').map_or(buf.text.len(), |i| p + i);
            let line = &buf.text[p..eol];
            let ind = line.len() - line.trim_start_matches([' ', '\t']).len();
            if !(heading_at(&buf.text, p, ctx)
                || item_at(&buf.text, p, ctx).is_some()
                || comment_at(&buf.text, p))
                && ind < line.len()
            {
                // `replace-match` over the blanks and the first character.
                let c = line[ind..].chars().next().map_or(0, char::len_utf8);
                let first = line[ind..ind + c].to_string();
                buf.replace(p, p + ind + c, &format!("{rpl}{first}"));
            }
            p = next_line(&buf.text, p);
        }
    }
    // Nothing to toggle: Emacs says so and changes nothing (an empty
    // transaction).
    Ok(buf.transaction("Toggle heading"))
}

/// A checkbox at the start of an item's text as the keyword
/// `org-toggle-heading` gives it (`[X]` the first done keyword, `[ ]` and
/// `[-]` the first not done one), and the text after it.
fn checkbox_keyword<'a>(rest: &'a str, ctx: &ParseContext) -> (String, &'a str) {
    let done = ctx.done_keywords.first().map_or("DONE", String::as_str);
    let todo = ctx.todo_keywords.first().map_or("TODO", String::as_str);
    for (b, kw) in [("[X]", done), ("[x]", done), ("[ ]", todo), ("[-]", todo)] {
        if let Some(r) = rest.strip_prefix(b) {
            let r = r.trim_start_matches([' ', '\t']);
            return (format!("{kw} "), r);
        }
    }
    (String::new(), rest)
}

/// `org-toggle-item`: in the region, or on the line at point, items
/// become text (their bullets go); else headings become items, a TODO
/// keyword a checkbox, their text indented under them; else lines of text
/// become items.
pub fn toggle_item(
    text: &str,
    point: usize,
    mark: Option<usize>,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    let (beg, end) = bounds(text, point, mark, false, false);
    let mut buf = Buf::new(text, point);
    let end = buf.add_marker(end);
    let mut p = beg;
    if item_at(&buf.text, p, ctx).is_some() {
        while p < buf.marker(end) {
            if let Some(after) = item_at(&buf.text, p, ctx) {
                let ind = buf.text[p..].len() - buf.text[p..].trim_start_matches([' ', '\t']).len();
                buf.delete(p + ind, after);
            }
            p = next_line(&buf.text, p);
        }
    } else if heading_at(&buf.text, p, ctx) {
        let limit = ctx.inlinetask_min_level;
        delete_metadata(&mut buf, p, ctx);
        let mut ref_level = reduced(stars_at(&buf.text, p).unwrap_or(1), ctx);
        while p < buf.marker(end) {
            let Some(stars) = stars_at(&buf.text, p) else {
                p = next_line(&buf.text, p);
                continue;
            };
            let level = reduced(stars, ctx);
            let delta = level.saturating_sub(ref_level);
            ref_level = ref_level.min(level);
            delete_metadata(&mut buf, p, ctx);
            let eol = buf.text[p..].find('\n').map_or(buf.text.len(), |i| p + i);
            let line = buf.text[p..eol].to_string();
            let c = org_model::complex_heading(&line, ctx);
            let keyword = c
                .as_ref()
                .and_then(|c| c.todo.clone())
                .map(|r| line[r].to_string());
            // `org-todo-line-regexp`'s group 3: the title, from the first
            // word after the stars and the keyword.
            let after_kw = c
                .as_ref()
                .and_then(|c| c.todo.clone())
                .map_or(stars, |r| r.end);
            let rest = &line[after_kw..];
            let title = after_kw + (rest.len() - rest.trim_start_matches(' ').len());
            let title = if line[title..].trim_matches([' ', '\t']).is_empty() {
                eol - p
            } else {
                title
            };
            let checkbox = match &keyword {
                Some(k) if ctx.done_keywords.contains(k) => "[X] ",
                Some(_) => "[ ] ",
                None => "",
            };
            let indent = " ".repeat(delta * 2);
            buf.delete(p, p + title);
            buf.insert_before_point(p, &format!("{indent}- {checkbox}"));
            // The section's text, down to the next heading, indented under
            // the item.
            let section_end = crate::headline::headings(&buf.text, limit)
                .into_iter()
                .map(|(s, _)| s)
                .find(|s| *s > p)
                .unwrap_or(buf.text.len());
            let stop = section_end.min(buf.marker(end));
            p = next_line(&buf.text, p);
            p = shift_text(&mut buf, p, stop, (delta + 1) * 2);
        }
    } else {
        while p < buf.marker(end) {
            if !(heading_at(&buf.text, p, ctx) || item_at(&buf.text, p, ctx).is_some()) {
                let eol = buf.text[p..].find('\n').map_or(buf.text.len(), |i| p + i);
                let line = &buf.text[p..eol];
                let ind = line.len() - line.trim_start_matches([' ', '\t']).len();
                if ind < line.len() {
                    buf.insert_before_point(p + ind, "- ");
                }
            }
            p = next_line(&buf.text, p);
        }
    }
    Ok(buf.transaction("Toggle item"))
}

/// `org-list--delete-metadata`: a heading's tags, planning line and
/// property drawer go, with the blank lines after them.
fn delete_metadata(buf: &mut Buf, h: usize, ctx: &ParseContext) {
    let eol = buf.text[h..].find('\n').map_or(buf.text.len(), |i| h + i);
    let line = buf.text[h..eol].to_string();
    if let Some(tags) = org_model::complex_heading(&line, ctx).and_then(|c| c.tags) {
        let keep = line[..tags.start].trim_end_matches([' ', '\t']).len();
        buf.delete(h + keep, eol);
    }
    let next = next_line(&buf.text, h);
    if next >= buf.text.len() {
        return;
    }
    let mut p = next;
    let line_at =
        |t: &str, p: usize| t[p..t[p..].find('\n').map_or(t.len(), |i| p + i)].to_string();
    let planning = |l: &str| {
        let t = l.trim_start_matches([' ', '\t']);
        ["CLOSED:", "DEADLINE:", "SCHEDULED:"]
            .iter()
            .any(|k| t.starts_with(k))
    };
    if p < buf.text.len() && planning(&line_at(&buf.text, p)) {
        p = next_line(&buf.text, p);
    }
    if p < buf.text.len()
        && line_at(&buf.text, p)
            .trim()
            .eq_ignore_ascii_case(":PROPERTIES:")
    {
        let mut q = next_line(&buf.text, p);
        while q < buf.text.len() {
            let l = line_at(&buf.text, q);
            let l = l.trim();
            if l.eq_ignore_ascii_case(":END:") {
                p = next_line(&buf.text, q);
                break;
            }
            if !(l.starts_with(':') && l[1..].contains(':')) {
                break;
            }
            q = next_line(&buf.text, q);
        }
    }
    // `org-skip-whitespace`, then back to the line's start.
    let q = p + buf.text[p..]
        .bytes()
        .take_while(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
        .count();
    let stop = if q >= buf.text.len() {
        q
    } else {
        buf.text[..q].rfind('\n').map_or(0, |i| i + 1)
    };
    if stop > next {
        buf.delete(next, stop);
    }
}

/// The lambda `shift-text` of `org-toggle-item`: the lines from `p` to
/// `end` indented so that the least indented is at `ind`, blank lines and
/// headings left alone. Returns where it stopped.
fn shift_text(buf: &mut Buf, mut p: usize, end: usize, ind: usize) -> usize {
    let end = buf.add_marker(end);
    let indent_of = |t: &str, p: usize| {
        let l = &t[p..t[p..].find('\n').map_or(t.len(), |i| p + i)];
        let blank = l.trim_matches([' ', '\t']).is_empty();
        let heading = stars_at(t, p).is_some();
        let w = l.len() - l.trim_start_matches([' ', '\t']).len();
        (blank, heading, column_at(t, p + w), w)
    };
    let mut min = 1000usize;
    let mut q = p;
    while q < buf.marker(end) {
        let (blank, heading, col, _) = indent_of(&buf.text, q);
        if !blank && !heading {
            min = min.min(col);
            if col == 0 {
                break;
            }
        }
        q = next_line(&buf.text, q);
    }
    while p < buf.marker(end) {
        let (blank, heading, col, w) = indent_of(&buf.text, p);
        if !blank && !heading {
            let new = (col + ind).saturating_sub(min);
            buf.replace(p, p + w, &" ".repeat(new));
        }
        p = next_line(&buf.text, p);
    }
    p
}
