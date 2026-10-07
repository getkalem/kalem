//! Sentences and paragraphs as Vim finds them (`:help sentence`, `:help
//! paragraph`): its `findsent()`, `findpar()`, `current_sent()` and
//! `current_par()` (search.c, textobject.c) over Kalem's text, a place at
//! a line feed being Vim's end-of-line NUL. Ported step for step, as the
//! results hang on the details (where a sentence at the end of a line
//! ends, what a count beyond the text does).

use super::{
    DocumentState, blank_line, char_at, char_before, last_line, line_end, line_of, line_start,
};

/// nroff macros that start a paragraph ('paragraphs') or a section
/// ('sections'), Vim's defaults.
const PARAGRAPHS: &[u8] = b"IPLPPPQPP TPHPLIPpLpItpplpipbp";
const SECTIONS: &[u8] = b"SHNHH HUnhsh";

/// The character at `p`; none at the end of a line (Vim's NUL).
fn gchar(doc: &DocumentState, p: usize) -> Option<char> {
    char_at(doc, p).filter(|c| *c != '\n')
}

fn white(c: Option<char>) -> bool {
    matches!(c, Some(' ' | '\t'))
}

fn col(doc: &DocumentState, p: usize) -> usize {
    p - line_start(doc, line_of(doc, p))
}

fn empty(doc: &DocumentState, line: usize) -> bool {
    line_start(doc, line) == line_end(doc, line)
}

/// Vim's `inc()`: a character on; 0 within the line, 2 when that is its
/// end, 1 onto the next line, -1 at the end of the text (not moved).
fn inc(doc: &DocumentState, p: &mut usize) -> i32 {
    if let Some(c) = gchar(doc, *p) {
        *p += c.len_utf8();
        return if gchar(doc, *p).is_some() { 0 } else { 2 };
    }
    let line = line_of(doc, *p);
    if line < last_line(doc) {
        *p = line_start(doc, line + 1);
        1
    } else {
        -1
    }
}

/// Vim's `dec()`: a character back; 1 onto the end of the line before,
/// -1 at the start of the text.
fn dec(doc: &DocumentState, p: &mut usize) -> i32 {
    let line = line_of(doc, *p);
    if *p > line_start(doc, line) {
        *p -= char_before(doc, *p).map_or(1, char::len_utf8);
        return 0;
    }
    if line > 0 {
        *p = line_end(doc, line - 1);
        return 1;
    }
    -1
}

/// `inc()` past the end of a line that is not empty.
pub(super) fn incl(doc: &DocumentState, p: &mut usize) -> i32 {
    let mut r = inc(doc, p);
    if r >= 1 && col(doc, *p) != 0 {
        r = inc(doc, p);
    }
    r
}

/// `dec()` past the end of a line that is not empty.
pub(super) fn decl(doc: &DocumentState, p: &mut usize) -> i32 {
    let mut r = dec(doc, p);
    if r == 1 && col(doc, *p) != 0 {
        r = dec(doc, p);
    }
    r
}

/// Vim's `inmacro()`: `s` (after a line's `.`) names a macro of `opt`,
/// two letters each, a space matching a space or the line's end.
fn in_macro(opt: &[u8], s: &[u8]) -> bool {
    let s0 = s.first().copied().unwrap_or(0);
    let s1 = s.get(1).copied().unwrap_or(0);
    opt.chunks(2).any(|m| {
        let (m0, m1) = (m[0], m.get(1).copied().unwrap_or(0));
        (m0 == s0 || (m0 == b' ' && (s0 == 0 || s0 == b' ')))
            && (m1 == s1 || ((m1 == 0 || m1 == b' ') && (s0 == 0 || s1 == 0 || s1 == b' ')))
    })
}

/// Vim's `startPS()`: `line` starts a paragraph (`para` none: it is
/// empty) or a section (`para` `{` or `}`: it starts with that), or it
/// starts with a form feed or an nroff macro (`both`: a `}` too).
fn start_ps_of(doc: &DocumentState, line: usize, para: Option<char>, both: bool) -> bool {
    let t = &doc.text().as_str()[line_start(doc, line)..line_end(doc, line)];
    let first = t.chars().next();
    if first == para || first == Some('\u{c}') || (both && first == Some('}')) {
        return true;
    }
    t.strip_prefix('.').is_some_and(|r| {
        in_macro(SECTIONS, r.as_bytes()) || (para.is_none() && in_macro(PARAGRAPHS, r.as_bytes()))
    })
}

fn start_ps(doc: &DocumentState, line: usize) -> bool {
    start_ps_of(doc, line, None, false)
}

/// Vim's `findsent()`: the start of the sentence `count` on (`)`) or
/// back (`(`) from `pos`; none when the text ends first.
pub(super) fn findsent(
    doc: &DocumentState,
    pos: usize,
    forward: bool,
    count: usize,
) -> Option<usize> {
    let step = |p: &mut usize| if forward { incl(doc, p) } else { decl(doc, p) };
    let mut pos = pos;
    let mut noskip = false;
    let mut count = count;
    while count > 0 {
        count -= 1;
        let mut found = false;
        if gchar(doc, pos).is_none() {
            // On an empty line: on to one that is not.
            loop {
                if step(&mut pos) == -1 || gchar(doc, pos).is_some() {
                    break;
                }
            }
            found = forward;
        } else if forward && col(doc, pos) == 0 && start_ps(doc, line_of(doc, pos)) {
            // At the start of a paragraph: the next line.
            let line = line_of(doc, pos);
            if line == last_line(doc) {
                return None;
            }
            pos = line_start(doc, line + 1);
            found = true;
        } else if !forward {
            decl(doc, &mut pos);
        }
        if !found {
            // Back to the last character that is neither blank nor
            // punctuation.
            let mut found_dot = false;
            while let Some(c) = gchar(doc, pos) {
                if !(c == ' ' || c == '\t' || ".!?)]\"'".contains(c)) {
                    break;
                }
                let mut t = pos;
                if decl(doc, &mut t) == -1 || (empty(doc, line_of(doc, t)) && forward) {
                    break;
                }
                if found_dot {
                    break;
                }
                if ".!?".contains(c) {
                    found_dot = true;
                }
                if ")]\"'".contains(c) && !gchar(doc, t).is_some_and(|t| ".!?)]\"'".contains(t)) {
                    break;
                }
                decl(doc, &mut pos);
            }
            // The end of the sentence.
            let start_line = line_of(doc, pos);
            loop {
                let c = gchar(doc, pos);
                if c.is_none() || (col(doc, pos) == 0 && start_ps(doc, line_of(doc, pos))) {
                    if !forward && line_of(doc, pos) != start_line {
                        pos = line_start(doc, line_of(doc, pos) + 1);
                    }
                    break;
                }
                if c.is_some_and(|c| ".!?".contains(c)) {
                    let mut t = pos;
                    let after = loop {
                        if inc(doc, &mut t) == -1 {
                            break None;
                        }
                        let g = gchar(doc, t);
                        if !g.is_some_and(|g| ")]\"'".contains(g)) {
                            break Some(g);
                        }
                    };
                    // At the text's end, or before a blank or a line's end.
                    if matches!(after, None | Some(None | Some(' ' | '\t'))) {
                        pos = t;
                        if gchar(doc, pos).is_none() {
                            inc(doc, &mut pos);
                        }
                        break;
                    }
                }
                if step(&mut pos) == -1 {
                    if count > 0 {
                        return None;
                    }
                    noskip = true;
                    break;
                }
            }
        }
        // Over blanks.
        while !noskip && white(gchar(doc, pos)) {
            if incl(doc, &mut pos) == -1 {
                break;
            }
        }
    }
    Some(pos)
}

/// Vim's `find_first_blank()`: back to the first of the blanks before
/// `p`.
fn find_first_blank(doc: &DocumentState, p: &mut usize) {
    while decl(doc, p) != -1 {
        if !white(gchar(doc, *p)) {
            incl(doc, p);
            break;
        }
    }
}

/// Vim's `findsent_forward()`: `count` sentences or the blanks between
/// them on, from the start of one (`at_start`) or from within.
fn findsent_forward(doc: &DocumentState, p: &mut usize, count: usize, at_start: bool) {
    let mut at_start = at_start;
    for i in (0..count).rev() {
        if let Some(n) = findsent(doc, *p, true, 1) {
            *p = n;
        }
        if at_start {
            find_first_blank(doc, p);
        }
        if i == 0 || at_start {
            decl(doc, p);
        }
        at_start = !at_start;
    }
}

/// What `is` and `as` take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Taken {
    /// For an operator: from `start` to `end`, `end` itself included or
    /// not (Vim's rules for an exclusive end at a line's start apply).
    Range {
        start: usize,
        end: usize,
        inclusive: bool,
    },
    /// In visual mode: the selection's new anchor and cursor.
    Visual { anchor: usize, cursor: usize },
}

/// Vim's `current_sent()`: `count` sentences at `cursor` (`as` with the
/// blanks after them, or else before), or the visual selection from
/// `anchor` made that much bigger.
pub(super) fn current_sent(
    doc: &DocumentState,
    cursor: usize,
    anchor: Option<usize>,
    count: usize,
    include: bool,
) -> Taken {
    let mut start = cursor;
    let mut pos = start;
    let mut cur = findsent(doc, cursor, true, 1).unwrap_or(cursor);
    let mut count = count;
    let mut extending = anchor.is_some_and(|a| a != start);
    loop {
        if extending {
            #[expect(clippy::expect_used, reason = "extending only with an anchor")]
            let visual = anchor.expect("an anchor");
            if start < visual {
                // At the start of the selection: it grows back.
                let mut at_start = true;
                let mut p = start;
                while p < cur {
                    if !white(gchar(doc, p)) {
                        at_start = false;
                        break;
                    }
                    incl(doc, &mut p);
                }
                if !at_start {
                    cur = findsent(doc, cur, false, 1).unwrap_or(cur);
                    if cur == start {
                        at_start = true;
                    } else {
                        cur = findsent(doc, cur, true, 1).unwrap_or(cur);
                    }
                }
                if include {
                    count *= 2;
                }
                for _ in 0..count {
                    if at_start {
                        find_first_blank(doc, &mut cur);
                    }
                    if !at_start || (!include && !white(gchar(doc, cur))) {
                        cur = findsent(doc, cur, false, 1).unwrap_or(cur);
                    }
                    at_start = !at_start;
                }
            } else {
                // At its end: it grows on.
                let mut p = pos;
                incl(doc, &mut p);
                let mut at_start = true;
                if p != cur {
                    at_start = false;
                    while p < cur {
                        if !white(gchar(doc, p)) {
                            at_start = true;
                            break;
                        }
                        incl(doc, &mut p);
                    }
                    if at_start {
                        cur = findsent(doc, cur, false, 1).unwrap_or(cur);
                    } else {
                        cur = start;
                    }
                }
                if include {
                    count *= 2;
                }
                findsent_forward(doc, &mut cur, count, at_start);
            }
            return Taken::Visual {
                anchor: visual,
                cursor: cur,
            };
        }
        // Started on blanks just before a sentence?
        while white(gchar(doc, pos)) {
            incl(doc, &mut pos);
        }
        let mut start_pos = start;
        let start_blank = pos == cur;
        if start_blank {
            find_first_blank(doc, &mut start_pos);
        } else {
            cur = findsent(doc, cur, false, 1).unwrap_or(cur);
            start_pos = cur;
        }
        let ncount = if include {
            count * 2
        } else if start_blank {
            count - 1
        } else {
            count
        };
        if ncount > 0 {
            findsent_forward(doc, &mut cur, ncount, true);
        } else {
            decl(doc, &mut cur);
        }
        if include {
            // Blanks before the sentence taken: none after it; none
            // after it: those before it.
            if start_blank {
                find_first_blank(doc, &mut cur);
                if white(gchar(doc, cur)) {
                    decl(doc, &mut cur);
                }
            } else if !white(gchar(doc, cur)) {
                find_first_blank(doc, &mut start_pos);
            }
        }
        if anchor.is_some() {
            // "is" on a single blank before a sentence would stay put:
            // grow it instead.
            if start_pos == cur {
                extending = true;
                start = start_pos;
                continue;
            }
            return Taken::Visual {
                anchor: start_pos,
                cursor: cur,
            };
        }
        // The line feed after the sentence too, if there is one.
        let inclusive = incl(doc, &mut cur) == -1;
        return Taken::Range {
            start: start_pos,
            end: cur,
            inclusive,
        };
    }
}

/// Vim's `findpar()`: the line `count` paragraphs on (`}`) or back (`{`)
/// from `pos`, at its start, or the last character of the last line
/// (then the motion is inclusive); none when the text ends first. Only
/// empty lines part paragraphs here, not blank ones.
pub(super) fn findpar(
    doc: &DocumentState,
    pos: usize,
    forward: bool,
    count: usize,
) -> Option<(usize, bool)> {
    findpar_of(doc, pos, forward, count, None, false)
}

/// Vim's `findpar()` for sections too (`[[`, `]]`, `[]`, `][`: `what`
/// the brace a section's line starts with; `both`, `]]` after an
/// operator, stops at a `}` too, on the line after it).
pub(super) fn findpar_of(
    doc: &DocumentState,
    pos: usize,
    forward: bool,
    count: usize,
    what: Option<char>,
    both: bool,
) -> Option<(usize, bool)> {
    let last = last_line(doc);
    let mut curr = line_of(doc, pos);
    for remaining in (0..count).rev() {
        let mut did_skip = false;
        let mut first = true;
        loop {
            if !empty(doc, curr) {
                did_skip = true;
            }
            if !first && did_skip && start_ps_of(doc, curr, what, both) {
                break;
            }
            first = false;
            let next = if forward {
                (curr < last).then_some(curr + 1)
            } else {
                curr.checked_sub(1)
            };
            match next {
                Some(n) => curr = n,
                None if remaining > 0 => return None,
                None => break,
            }
        }
    }
    // `]]` after an operator takes the `}` line; past the last line it
    // ends at the start of that line.
    if both && doc.text().as_str()[line_start(doc, curr)..].starts_with('}') {
        if curr >= last {
            return Some((line_start(doc, curr), false));
        }
        curr += 1;
    }
    // The last line (a single line too, even going back): its last
    // character, the motion inclusive, as Vim does.
    if curr == last && what != Some('}') {
        let (s, e) = (line_start(doc, curr), line_end(doc, curr));
        if e > s {
            return Some((e - char_before(doc, e).map_or(1, char::len_utf8), true));
        }
    }
    Some((line_start(doc, curr), false))
}

/// `[(`, `[{`, `])` and `]}`: the `count`th bracket `want` that is not
/// matched, back or on from `pos`.
pub(super) fn unmatched(
    doc: &DocumentState,
    pos: usize,
    want: char,
    count: usize,
) -> Option<usize> {
    let (open, close, forward) = match want {
        '(' => ('(', ')', false),
        '{' => ('{', '}', false),
        ')' => ('(', ')', true),
        _ => ('{', '}', true),
    };
    let text = doc.text().as_str();
    let mut at = pos;
    // As many as there are, the last one found (none: the motion fails).
    let mut found_any = false;
    for _ in 0..count.max(1) {
        let mut depth = 0usize;
        let found = if forward {
            let from = (at + char_at(doc, at).map_or(0, char::len_utf8)).min(text.len());
            text[from..].char_indices().find_map(|(i, c)| {
                if c == open {
                    depth += 1;
                } else if c == close {
                    if depth == 0 {
                        return Some(from + i);
                    }
                    depth -= 1;
                }
                None
            })
        } else {
            text[..at].char_indices().rev().find_map(|(i, c)| {
                if c == close {
                    depth += 1;
                } else if c == open {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                None
            })
        };
        match found {
            Some(f) => {
                at = f;
                found_any = true;
            }
            None => break,
        }
    }
    found_any.then_some(at)
}

/// Vim's `current_par()`: the lines of `count` paragraphs (`ap` with the
/// blank lines after them, or else before) from `line`, first and last;
/// in visual mode with the selection from `anchor` (a line), its first
/// line stays and the last moves. None past the text's end.
pub(super) fn current_par(
    doc: &DocumentState,
    line: usize,
    anchor: Option<usize>,
    count: usize,
    include: bool,
) -> Option<(usize, usize)> {
    let last = last_line(doc);
    let white = |l: usize| blank_line(doc, l);
    let mut start = line;
    if let Some(visual) = anchor.filter(|a| *a != line) {
        // A selection over lines grows a paragraph (and its blank lines)
        // at a time, the way the cursor is.
        let forward = start > visual;
        let at_end = |l: usize| if forward { l == last } else { l == 0 };
        let mut ok = true;
        for _ in 0..count {
            if at_end(start) {
                ok = false;
                break;
            }
            let mut prev_white = None;
            for _ in 0..2 {
                start = if forward { start + 1 } else { start - 1 };
                let is_white = white(start);
                if prev_white == Some(is_white) {
                    start = if forward { start - 1 } else { start + 1 };
                    break;
                }
                loop {
                    if at_end(start) {
                        break;
                    }
                    let next = if forward { start + 1 } else { start - 1 };
                    if is_white != white(next)
                        || (!is_white && start_ps(doc, if forward { start + 1 } else { start }))
                    {
                        break;
                    }
                    start = next;
                }
                if !include || at_end(start) {
                    break;
                }
                prev_white = Some(is_white);
            }
        }
        return ok.then_some((visual, start));
    }
    // Back to the first line of the paragraph or of the blank lines.
    let white_in_front = white(start);
    while start > 0 {
        if white_in_front {
            if !white(start - 1) {
                break;
            }
        } else if white(start - 1) || start_ps(doc, start) {
            break;
        }
        start -= 1;
    }
    // Past the blank lines.
    let mut end = start;
    while end <= last && white(end) {
        end += 1;
    }
    let mut end = end as isize - 1;
    let mut i = count;
    if !include && white_in_front {
        i -= 1;
    }
    for left in (0..i).rev() {
        if end == last as isize {
            return None;
        }
        let do_white = !include && white((end + 1) as usize);
        if include || !do_white {
            end += 1;
            // To the paragraph's end.
            while (end as usize) < last
                && !white(end as usize + 1)
                && !start_ps(doc, end as usize + 1)
            {
                end += 1;
            }
        }
        if left == 0 && white_in_front && include {
            break;
        }
        // To the end of the blank lines after it.
        if include || do_white {
            while (end as usize) < last && white(end as usize + 1) {
                end += 1;
            }
        }
    }
    let end = end.max(start as isize) as usize;
    // No blank lines at the end: those before, if any.
    if !white_in_front && !white(end) && include {
        while start > 0 && white(start - 1) {
            start -= 1;
        }
    }
    Some((start, end))
}

/// Vim's `find_next_quote()`: the next `q` in `line` from `col`; with
/// `escape`, a character after a backslash is skipped.
fn next_quote(line: &str, mut col: usize, q: u8, escape: bool) -> Option<usize> {
    let b = line.as_bytes();
    let len = |c: usize| line[c..].chars().next().map_or(1, char::len_utf8);
    loop {
        let c = *b.get(col)?;
        if escape && c == b'\\' {
            col += 1;
            if col >= b.len() {
                return None;
            }
        } else if c == q {
            return Some(col);
        }
        col += len(col);
    }
}

/// Vim's `find_prev_quote()`: the `q` before `col` that no odd number of
/// backslashes escapes, else 0.
fn prev_quote(line: &str, mut col: usize, q: u8) -> usize {
    let b = line.as_bytes();
    while col > 0 {
        col -= 1;
        while !line.is_char_boundary(col) {
            col -= 1;
        }
        let mut n = 0;
        while col > n && b[col - n - 1] == b'\\' {
            n += 1;
        }
        if n % 2 == 1 {
            col -= n;
        } else if b[col] == q {
            break;
        }
    }
    col
}

/// Vim's `current_quote()` (outside visual mode): the quoted text `q` at
/// `pos` on its line (`i"`), or with its quotes and the blanks after
/// them or else before (`a"`); with the quotes from a count of two
/// (`2i"`). The string is the one the cursor is in, or the one starting
/// at the quote before it, or the next one.
pub(super) fn current_quote(
    doc: &DocumentState,
    pos: usize,
    q: char,
    include: bool,
    count: usize,
) -> Option<std::ops::Range<usize>> {
    let line = line_of(doc, pos);
    let s = line_start(doc, line);
    let text = &doc.text().as_str()[s..line_end(doc, line)];
    let q = u8::try_from(q).ok()?;
    let first = pos - s;
    let b = text.as_bytes();
    let (mut start, mut end);
    if b.get(first) == Some(&q) {
        // On a quote: which string it is, counted from the line's start.
        start = 0;
        loop {
            start = next_quote(text, start, q, false)?;
            if start > first {
                return None;
            }
            end = next_quote(text, start + 1, q, true)?;
            if start <= first && first <= end {
                break;
            }
            start = end + 1;
        }
    } else {
        start = prev_quote(text, first, q);
        if b.get(start) != Some(&q) {
            start = next_quote(text, start, q, false)?;
        }
        end = next_quote(text, start + 1, q, true)?;
    }
    if include {
        let white = |i: usize| matches!(b.get(i), Some(b' ' | b'\t'));
        if white(end + 1) {
            while white(end + 1) {
                end += 1;
            }
        } else {
            while start > 0 && white(start - 1) {
                start -= 1;
            }
        }
    }
    Some(if !include && count < 2 {
        s + start + 1..s + end
    } else {
        s + start..s + end + 1
    })
}
