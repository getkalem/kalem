//! Headline commands: promote and demote (one headline or a subtree), move
//! a subtree up or down. Each follows the Emacs command it is named after,
//! tag alignment and cursor placement included.

use org_syntax::ParseContext;

use crate::buffer::{Buf, EditError, column_at, string_width, user_error};
use crate::transaction::{Selection, Transaction};

/// `org-tags-column`.
pub const TAGS_COLUMN: isize = -77;

/// A heading line: `^\*+ `.
pub(crate) fn stars_at(text: &str, bol: usize) -> Option<usize> {
    let n = text[bol..].bytes().take_while(|b| *b == b'*').count();
    (n > 0 && text.as_bytes().get(bol + n) == Some(&b' ')).then_some(n)
}

/// Heading line starts, with their number of stars; `limit` excludes
/// inlinetask lines (`org-with-limited-levels`).
pub(crate) fn headings(text: &str, limit: Option<usize>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        if let Some(n) = stars_at(text, pos)
            && limit.is_none_or(|l| n < l)
        {
            out.push((pos, n));
        }
        pos += line.len();
    }
    out
}

/// `org-back-to-heading`: the heading line at or before `pos`.
pub(crate) fn back_to_heading(
    text: &str,
    pos: usize,
    limit: Option<usize>,
) -> Result<(usize, usize), EditError> {
    headings(text, limit)
        .into_iter()
        .rev()
        .find(|(start, _)| *start <= pos)
        .ok_or_else(|| EditError::new("Before first headline at position"))
}

/// `org-back-to-heading`: the heading at or before `pos`, where the `END`
/// line of an inlinetask belongs to the inlinetask and a position after it
/// to the enclosing entry.
pub(crate) fn org_back_to_heading(text: &str, pos: usize, ctx: &ParseContext) -> Option<usize> {
    let hs = headings(text, None);
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let mut i = hs.iter().rposition(|(s, _)| *s <= bol)?;
    loop {
        let (s, _) = hs[i];
        if !is_inlinetask_end(text, s, ctx) {
            return Some(s);
        }
        // On the END line: the inlinetask. After it: skip the inlinetask.
        i = i.checked_sub(1)?;
        if s != bol {
            i = i.checked_sub(1)?;
        }
    }
}

/// `org-inlinetask-end-p` at the line `bol`.
pub(crate) fn is_inlinetask_end(text: &str, bol: usize, ctx: &ParseContext) -> bool {
    let Some(min) = ctx.inlinetask_min_level else {
        return false;
    };
    let line = &text[bol..text[bol..].find('\n').map_or(text.len(), |i| bol + i)];
    let stars = line.bytes().take_while(|b| *b == b'*').count();
    let rest = &line[stars..];
    let word = rest.trim_start_matches([' ', '\t']);
    stars >= min
        && word.len() < rest.len()
        && word.len() >= 3
        && word.as_bytes()[..3].eq_ignore_ascii_case(b"end")
        && word[3..].trim_end_matches([' ', '\t']).is_empty()
}

/// `org-get-valid-level`.
pub(crate) fn valid_level(level: usize, change: isize, odd: bool) -> usize {
    if odd {
        let l = level as isize;
        let v = if change == 0 {
            1 + 2 * (l / 2)
        } else if change > 0 {
            1 + 2 * ((l - 1 + 2 * change) / 2)
        } else {
            (1 + 2 * ((l + 2 * change).div_euclid(2))).max(1)
        };
        v.max(1) as usize
    } else {
        (level as isize + change).max(1) as usize
    }
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%' | ':')
}

/// `org--align-tags-here` on the heading line at `bol`.
pub(crate) fn align_tags(buf: &mut Buf, bol: usize) {
    let eol = buf.eol(bol);
    let line = &buf.text[bol..eol];
    // `^\*+ \(?:.*[ \t]\)?\(:\([[:alnum:]_@#%:]+\):\)[ \t]*$`
    let stars = line.bytes().take_while(|b| *b == b'*').count();
    if stars == 0 || line.as_bytes().get(stars) != Some(&b' ') {
        return;
    }
    let trimmed = line.trim_end_matches([' ', '\t']);
    if !trimmed.ends_with(':') {
        return;
    }
    // The tag group: the longest run of tag characters ending the line,
    // starting with a colon and preceded by a blank (or the stars' space).
    let body_start = stars + 1;
    let mut tags_start = None;
    for (i, c) in trimmed.char_indices().rev() {
        if !is_tag_char(c) {
            break;
        }
        if c == ':'
            && i >= body_start
            && (i == body_start || matches!(trimmed.as_bytes()[i - 1], b' ' | b'\t'))
        {
            tags_start = Some(i);
        }
    }
    let Some(ts) = tags_start else { return };
    let tags = &trimmed[ts..];
    if tags.len() < 3 {
        return;
    }
    let tags_start = bol + ts;
    let blank_start = bol + line[..ts].trim_end_matches([' ', '\t']).len();
    let min_col = column_at(&buf.text, blank_start) + 1;
    let to = if TAGS_COLUMN >= 0 {
        TAGS_COLUMN as usize
    } else {
        (TAGS_COLUMN.unsigned_abs()).saturating_sub(string_width(tags))
    };
    let new = to.max(min_col);
    let current = column_at(&buf.text, tags_start);
    if new != current {
        // Point in the blanks before the tags returns to its column.
        let origin = buf.point;
        let in_blank = origin > blank_start && origin <= tags_start;
        let column = column_at(&buf.text, origin);
        let col = column_at(&buf.text, blank_start);
        buf.delete(blank_start, tags_start);
        let spaces = " ".repeat(new - col);
        buf.insert_before_point(blank_start, &spaces);
        if in_blank {
            buf.point = crate::buffer::move_to_column(&buf.text, bol, column);
        }
    }
}

/// `org-fix-position-after-promote`.
fn fix_position(buf: &mut Buf, ctx: &ParseContext) {
    let pos = buf.point;
    let bol = buf.bol(pos);
    let Some(stars) = stars_at(&buf.text, bol).or_else(|| {
        let n = buf.text[bol..].bytes().take_while(|b| *b == b'*').count();
        (n > 0).then_some(n)
    }) else {
        return;
    };
    let end_stars = bol + stars;
    // `\(?: +\(KW\)\)?` followed by ` +...` or `[ \t]*$`.
    let eol = buf.eol(bol);
    let rest = &buf.text[end_stars..eol];
    let after_spaces = rest.trim_start_matches(' ');
    let kw_end = if after_spaces.len() < rest.len() {
        let keywords = ctx.todo_keywords.iter().chain(&ctx.done_keywords);
        keywords
            .filter(|k| {
                after_spaces.strip_prefix(k.as_str()).is_some_and(|r| {
                    r.is_empty() || r.starts_with(' ') || r.trim_matches([' ', '\t']).is_empty()
                })
            })
            .map(|k| end_stars + (rest.len() - after_spaces.len()) + k.len())
            .max()
    } else {
        None
    };
    if pos == end_stars || Some(pos) == kw_end {
        if pos == buf.text.len() || buf.text.as_bytes()[pos] == b'\n' {
            buf.insert_at_point(" ");
        } else if buf.text.as_bytes()[pos] == b' ' {
            buf.point += 1;
        }
    }
}

/// Changes the level of the heading at `bol` (`org-promote` or
/// `org-demote`).
fn change_level(
    buf: &mut Buf,
    bol: usize,
    stars: usize,
    change: isize,
    ctx: &ParseContext,
) -> Result<(), EditError> {
    if change < 0 && stars == 1 {
        return user_error("Cannot promote to level 0.  UNDO to recover if necessary");
    }
    let new = valid_level(stars, change, ctx.odd_levels_only);
    let head = format!("{} ", "*".repeat(new));
    buf.replace(bol, bol + stars + 1, &head);
    align_tags(buf, bol);
    Ok(())
}

fn run(
    text: &str,
    point: usize,
    label: &str,
    f: impl FnOnce(&mut Buf) -> Result<(), EditError>,
) -> Result<Transaction, EditError> {
    let mut buf = Buf::new(text, point);
    f(&mut buf)?;
    Ok(buf.transaction(label))
}

/// `org-do-promote` (without a region).
pub fn promote(text: &str, point: usize, ctx: &ParseContext) -> Result<Transaction, EditError> {
    run(text, point, "Promote", |buf| {
        let (bol, stars) = back_to_heading(&buf.text, buf.point, None)?;
        change_level(buf, bol, stars, -1, ctx)?;
        fix_position(buf, ctx);
        Ok(())
    })
}

/// `org-do-demote` (without a region).
pub fn demote(text: &str, point: usize, ctx: &ParseContext) -> Result<Transaction, EditError> {
    run(text, point, "Demote", |buf| {
        let (bol, stars) = back_to_heading(&buf.text, buf.point, None)?;
        change_level(buf, bol, stars, 1, ctx)?;
        fix_position(buf, ctx);
        Ok(())
    })
}

/// The editor's heading styles (Ctrl+1 to Ctrl+6, Ctrl+0), a Kalem command
/// with no Emacs counterpart: makes the line at `point` a heading of
/// `level` (`2 × level − 1` stars with `org-odd-levels-only`), changing the
/// stars of a heading line and prefixing any other line, or with level 0
/// removes a heading's stars and the blanks after them. Only this line
/// changes; tags are realigned.
pub fn set_level(
    text: &str,
    point: usize,
    level: usize,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    run(text, point, "Set heading level", |buf| {
        let bol = buf.bol(buf.point);
        let current = stars_at(&buf.text, bol);
        let stars = if ctx.odd_levels_only && level > 0 {
            2 * level - 1
        } else {
            level
        };
        match current {
            Some(s) if level == 0 => {
                let blanks = buf.text[bol + s..]
                    .bytes()
                    .take_while(|b| matches!(b, b' ' | b'\t'))
                    .count();
                buf.delete(bol, bol + s + blanks);
            }
            Some(s) => {
                if s != stars {
                    buf.replace(bol, bol + s + 1, &format!("{} ", "*".repeat(stars)));
                    align_tags(buf, bol);
                }
            }
            None if level == 0 => return user_error("Not on a heading"),
            None => {
                let eol = buf.eol(bol);
                let ws = buf.text[bol..eol]
                    .bytes()
                    .take_while(|b| matches!(b, b' ' | b'\t'))
                    .count();
                let head = format!("{} ", "*".repeat(stars));
                let at_start = buf.point <= bol + ws;
                buf.replace(bol, bol + ws, &head);
                if at_start {
                    buf.point = bol + head.len();
                }
            }
        }
        fix_position(buf, ctx);
        Ok(())
    })
}

fn change_subtree(buf: &mut Buf, change: isize, ctx: &ParseContext) -> Result<(), EditError> {
    let limit = ctx.inlinetask_min_level;
    let (bol, level) = back_to_heading(&buf.text, buf.point, limit)?;
    // `org-map-tree`: the heading and the following deeper ones.
    let hs = headings(&buf.text, limit);
    let i = hs
        .iter()
        .position(|(s, _)| *s == bol)
        .ok_or_else(|| EditError::new("Not in a heading"))?;
    let mut targets = vec![hs[i]];
    targets.extend(hs[i + 1..].iter().take_while(|(_, l)| *l > level).copied());
    // The first one decides whether the command fails.
    if change < 0 && targets[0].1 == 1 {
        return user_error("Cannot promote to level 0.  UNDO to recover if necessary");
    }
    for (b, s) in targets.into_iter().rev() {
        change_level(buf, b, s, change, ctx)?;
    }
    fix_position(buf, ctx);
    Ok(())
}

/// `org-promote-subtree`.
pub fn promote_subtree(
    text: &str,
    point: usize,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    run(text, point, "Promote subtree", |buf| {
        change_subtree(buf, -1, ctx)
    })
}

/// `org-demote-subtree`.
pub fn demote_subtree(
    text: &str,
    point: usize,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    run(text, point, "Demote subtree", |buf| {
        change_subtree(buf, 1, ctx)
    })
}

/// The end of the subtree of the headline at `bol`: the next heading of
/// the same or a higher level (inlinetasks are not headings), or the end.
pub(crate) fn subtree_end(text: &str, bol: usize, level: usize, limit: Option<usize>) -> usize {
    headings(text, limit)
        .into_iter()
        .find(|(s, l)| *s > bol && *l <= level)
        .map_or(text.len(), |(s, _)| s)
}

/// `org-move-subtree-down` (`down`) or `org-move-subtree-up`.
pub fn move_subtree(
    text: &str,
    point: usize,
    down: bool,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    let label = if down {
        "Move subtree down"
    } else {
        "Move subtree up"
    };
    run(text, point, label, |buf| {
        let col = column_at(&buf.text, buf.point);
        let limit = ctx.inlinetask_min_level;
        let (beg, level) = back_to_heading(&buf.text, buf.point, None)?;
        // An inlinetask is not a subtree: Emacs fails, with point at the
        // inlinetask's first line (its END line leads there too).
        if limit.is_some_and(|l| level >= l) {
            let eol = buf.eol(beg);
            let is_end = buf.text[beg + level + 1..eol].trim_end_matches([' ', '\t']) == "END";
            let start = if is_end {
                headings(&buf.text, None)
                    .into_iter()
                    .rev()
                    .find(|(s, l)| *s < beg && *l == level)
                    .map_or(beg, |(s, _)| s)
            } else {
                beg
            };
            return Err(EditError::at(
                "Cannot move past superior level or buffer limit",
                start,
            ));
        }
        let end = subtree_end(&buf.text, beg, level, limit);
        let hs = headings(&buf.text, None);
        let i = hs
            .iter()
            .position(|(s, _)| *s == beg)
            .ok_or_else(|| EditError::new("Not in a heading"))?;
        let fail = || {
            Err(EditError::at(
                "Cannot move past superior level or buffer limit",
                beg,
            ))
        };
        let mut ins = if down {
            // `org-get-next-sibling`.
            match hs[i + 1..].iter().find(|(_, l)| *l <= level) {
                Some(&(s, l)) if l == level => s,
                _ => return fail(),
            }
        } else {
            // `org-get-previous-sibling`.
            match hs[..i].iter().rev().find(|(_, l)| *l <= level) {
                Some(&(s, l)) if l == level => s,
                _ => return fail(),
            }
        };
        if down {
            // Over the sibling's subtree, then back over its trailing
            // blank lines to see whether it ends with a line feed.
            let sib_end = subtree_end(&buf.text, ins, level, limit);
            let content_end = buf.text[..sib_end]
                .trim_end_matches([' ', '\t', '\n', '\r'])
                .len();
            let after_line = buf.text[content_end..]
                .find('\n')
                .map_or(buf.text.len(), |j| content_end + j + 1);
            let p = after_line.min(sib_end);
            buf.point = sib_end;
            if !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
                buf.insert_before_point(p, "\n");
            }
            ins = buf.point;
        }
        let txt = buf.text[beg..end].to_string();
        buf.point = ins;
        buf.delete(beg, end);
        let ins = buf.point;
        if !(ins == 0 || buf.text.as_bytes()[ins - 1] == b'\n')
            && buf.text.as_bytes().get(ins) == Some(&b'\n')
        {
            buf.point += 1;
        }
        let bbb = buf.point;
        buf.insert_at_point(&txt);
        if !(buf.point == 0 || buf.text.as_bytes()[buf.point - 1] == b'\n') {
            buf.insert_at_point("\n");
        }
        // Back to the moved heading, then to the original column.
        let mut p = bbb;
        while p < buf.text.len() && matches!(buf.text.as_bytes()[p], b' ' | b'\t' | b'\n') {
            p += 1;
        }
        let bol = buf.bol(p);
        buf.point = crate::buffer::move_to_column(&buf.text, bol, col);
        Ok(())
    })
}

/// The text `org-copy-subtree` copies: the range of the subtree at `point`.
pub fn subtree_range(
    text: &str,
    point: usize,
    ctx: &ParseContext,
) -> Result<std::ops::Range<usize>, EditError> {
    let limit = ctx.inlinetask_min_level;
    let (mut beg, mut level) = back_to_heading(text, point, None)?;
    // An inlinetask is not a subtree: take its headline.
    if limit.is_some_and(|l| level >= l) {
        let (b, l) = back_to_heading(text, beg, limit)?;
        beg = b;
        level = l;
    }
    let end = subtree_end(text, beg, level, limit);
    Ok(beg..end)
}

/// `org-copy-subtree`: the subtree's text.
pub fn copy_subtree(text: &str, point: usize, ctx: &ParseContext) -> Result<String, EditError> {
    let r = subtree_range(text, point, ctx)?;
    Ok(text[r].to_string())
}

/// `org-cut-subtree`: the transaction deleting the subtree, and its text.
pub fn cut_subtree(
    text: &str,
    point: usize,
    ctx: &ParseContext,
) -> Result<(Transaction, String), EditError> {
    let r = subtree_range(text, point, ctx)?;
    let clip = text[r.clone()].to_string();
    let mut buf = Buf::new(text, point);
    if r.end > r.start {
        buf.delete(r.start, r.end);
    }
    Ok((buf.transaction("Cut subtree"), clip))
}

/// `org-kill-is-subtree-p`: the clipboard starts with a heading (after
/// blank lines) and has no heading above that level.
pub fn is_subtree(clip: &str, ctx: &ParseContext) -> bool {
    let limit = ctx.inlinetask_min_level.unwrap_or(usize::MAX);
    // `\`\([ \t\n\r]*?\n\)?\(\*\{1,N\} \)`
    let mut starts = vec![0];
    for (i, _) in clip.match_indices('\n') {
        if clip[..i]
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            starts.push(i + 1);
        } else {
            break;
        }
    }
    let first = starts
        .iter()
        .find_map(|&s| stars_at(clip, s).filter(|&n| n < limit).map(|n| (s, n)));
    let Some((start, level)) = first else {
        return false;
    };
    headings(clip, Some(limit))
        .into_iter()
        .filter(|(s, _)| *s > start)
        .all(|(_, l)| l >= level)
}

/// `org-paste-subtree` with the clipboard `clip`.
pub fn paste_subtree(
    text: &str,
    point: usize,
    clip: &str,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    if !is_subtree(clip, ctx) {
        return user_error("The kill is not a (set of) tree(s).  Use `C-y' to yank anyway");
    }
    let limit = ctx.inlinetask_min_level;
    run(text, point, "Paste subtree", |buf| {
        paste_in(buf, clip, None, limit);
        Ok(())
    })
}

/// `org-paste-subtree` in `buf` at point with the clipboard `clip` (a
/// subtree), at the numeric `level` when given.
pub(crate) fn paste_in(buf: &mut Buf, clip: &str, level: Option<usize>, limit: Option<usize>) {
    {
        let hs = headings(&buf.text, limit);
        let old_level = headings(clip, limit).first().map(|(_, l)| *l);
        let bol = buf.bol(buf.point);
        let eol = buf.eol(bol);
        let line = &buf.text[bol..eol];
        let at_heading = hs.iter().any(|(s, _)| *s == bol);
        // A line of stars only is a level indicator.
        let only_stars = line.starts_with('*')
            && line
                .trim_start_matches('*')
                .trim_matches([' ', '\t'])
                .is_empty();
        // `(org-outline-level)` there: the level of the heading at or
        // before the line (a line of stars alone is not a heading), 0 if
        // none.
        let level_indicator =
            (level.is_none() && only_stars && buf.text.as_bytes().get(buf.point) != Some(&b'*'))
                .then(|| {
                    hs.iter()
                        .rev()
                        .find(|(s, _)| *s <= bol)
                        .map_or(0, |(_, l)| *l)
                });
        let force = level_indicator.or(level).or_else(|| {
            (buf.point == bol && at_heading)
                .then(|| hs.iter().find(|(s, _)| *s == bol).map(|(_, l)| *l))
                .flatten()
        });
        let previous = hs
            .iter()
            .rev()
            .find(|(s, _)| *s <= bol)
            .map_or(1, |(_, l)| *l);
        let next = hs.iter().find(|(s, _)| *s > bol).map_or(1, |(_, l)| *l);
        let new_level = force.unwrap_or(previous.max(next));
        let shift = match old_level {
            Some(o) if o != new_level => new_level as isize - o as isize,
            _ => 0,
        };
        if level_indicator.is_some() {
            let next_bol = if eol < buf.text.len() { eol + 1 } else { eol };
            buf.delete(bol, next_bol);
        }
        let bol = buf.bol(buf.point);
        let at_heading = headings(&buf.text, limit).iter().any(|(s, _)| *s == bol);
        if !(buf.point == bol && at_heading) {
            // `org-next-visible-heading`.
            let next = headings(&buf.text, limit)
                .into_iter()
                .find(|(s, _)| *s > bol)
                .map_or(buf.text.len(), |(s, _)| s);
            buf.point = next;
            if !(buf.point == 0 || buf.text.as_bytes()[buf.point - 1] == b'\n') {
                buf.insert_at_point("\n");
            }
        }
        let beg = buf.point;
        buf.insert_at_point(clip);
        if !clip.ends_with('\n') {
            buf.insert_at_point("\n");
        }
        let end = buf.point;
        let mut beg = beg;
        while beg < end && matches!(buf.text.as_bytes()[beg], b' ' | b'\t' | b'\n' | b'\r') {
            beg += 1;
        }
        if shift != 0 {
            let region: Vec<(usize, usize)> = headings(&buf.text[..end], limit)
                .into_iter()
                .filter(|(s, _)| *s >= beg)
                .collect();
            for (s, l) in region.into_iter().rev() {
                let new = (l as isize + shift).max(1) as usize;
                buf.replace(s, s + l + 1, &format!("{} ", "*".repeat(new)));
                align_tags(buf, s);
            }
        }
        buf.point = beg;
    }
}

/// Moves the subtree at `from` before the heading line starting at `to`,
/// or to the end of the text when `to` is its length, with its top heading
/// at `level`: the stars of all its headings change by the same amount and
/// their tags stay aligned. This is the drag and drop of an outline; Org
/// has no such command (`org-refile` moves under a heading).
pub fn move_subtree_to(
    text: &str,
    from: usize,
    to: usize,
    level: usize,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    let limit = ctx.inlinetask_min_level;
    let r = subtree_range(text, from, ctx)?;
    if to != text.len() && !headings(text, limit).iter().any(|(s, _)| *s == to) {
        return user_error("Not at a headline");
    }
    if to > r.start && to < r.end {
        return user_error("Cannot move a subtree into itself");
    }
    let mut clip = text[r.clone()].to_string();
    if !clip.ends_with('\n') {
        clip.push('\n');
    }
    let old = headings(&clip, limit).first().map_or(1, |(_, l)| *l);
    let shift = level.max(1) as isize - old as isize;
    let mut cb = Buf::new(&clip, 0);
    if shift != 0 {
        for (s, l) in headings(&clip, limit).into_iter().rev() {
            let new = (l as isize + shift).max(1) as usize;
            cb.replace(s, s + l + 1, &format!("{} ", "*".repeat(new)));
            align_tags(&mut cb, s);
        }
    }
    let moved = cb.text;
    // A deletion and an insertion, so that positions outside the subtree
    // map through the move.
    let mut tx = Transaction::new("Move subtree");
    let point = if to == r.start || to == r.end {
        tx.edit(r.clone(), moved);
        r.start
    } else if to < r.start {
        tx.edit(to..to, moved);
        tx.edit(r.clone(), "");
        to
    } else {
        let sep = if text.ends_with('\n') || to < text.len() {
            ""
        } else {
            "\n"
        };
        tx.edit(r.clone(), "");
        tx.edit(to..to, format!("{sep}{moved}"));
        to - r.len() + sep.len()
    };
    Ok(tx.select(Selection::caret(point)))
}

/// Where [`insert_heading`] puts the new heading: `org-insert-heading`
/// without a prefix, with `C-u` (`org-insert-heading-respect-content`),
/// or with `C-u C-u`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingPlace {
    /// At point: above the heading at its start, splitting its title in
    /// it, or after its line.
    Here,
    /// After the subtree at point.
    AfterSubtree,
    /// After the subtree of the parent of the heading at point.
    AfterParent,
}

/// `org-before-first-heading-p` (with limited levels) at `pos`.
fn before_first_heading(text: &str, pos: usize, limit: Option<usize>) -> bool {
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    !headings(text, limit).iter().any(|(s, _)| *s <= bol)
}

/// `outline-next-heading` from `pos`: the next heading line's start after
/// it.
fn next_heading(text: &str, pos: usize, limit: Option<usize>) -> Option<usize> {
    headings(text, limit)
        .into_iter()
        .map(|(s, _)| s)
        .find(|s| *s > pos)
}

/// The start of the line `n` lines away from the line at `pos` (Emacs's
/// `forward-line`, stopping at the ends).
fn forward_line(text: &str, pos: usize, n: isize) -> usize {
    let mut b = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    if n < 0 {
        for _ in 0..n.unsigned_abs() {
            if b == 0 {
                break;
            }
            b = text[..b - 1].rfind('\n').map_or(0, |i| i + 1);
        }
    } else {
        for _ in 0..n {
            match text[b..].find('\n') {
                Some(i) => b += i + 1,
                None => break,
            }
        }
    }
    b
}

/// `org--line-empty-p`: whether the line `n` lines away from the one at
/// `pos` is blank (never at the start of the text).
fn line_empty(text: &str, pos: usize, n: isize) -> bool {
    if pos == 0 {
        return false;
    }
    let b = forward_line(text, pos, n);
    let e = text[b..].find('\n').map_or(text.len(), |i| b + i);
    text[b..e].trim_matches([' ', '\t']).is_empty()
}

/// `org--blank-before-heading-p` with `org-blank-before-new-entry`'s
/// default (`auto`): a new heading gets a blank line before it when the
/// heading at point (its parent with `parent`) has one.
fn blank_before_heading(text: &str, point: usize, parent: bool, limit: Option<usize>) -> bool {
    let mut p = point;
    if before_first_heading(text, p, limit) {
        match next_heading(text, p, limit) {
            Some(s) => p = s,
            None => return false,
        }
    }
    let Ok((h, level)) = back_to_heading(text, p, limit) else {
        return false;
    };
    p = h;
    if parent
        && let Some(&(s, _)) = headings(text, limit)
            .iter()
            .rev()
            .find(|(s, l)| *s < h && *l < level)
    {
        p = s;
    }
    if p != 0 {
        return line_empty(text, p, -1);
    }
    if let Some(n) = next_heading(text, p, limit) {
        return line_empty(text, n, -1);
    }
    let end = text.trim_end_matches([' ', '\t']).len();
    let bolp = end == 0 || text.as_bytes()[end - 1] == b'\n';
    bolp && line_empty(text, end, -1)
}

/// `org-N-empty-lines-before-current`: exactly `n` blank lines before the
/// line at point, the column kept.
fn n_empty_lines_before_current(buf: &mut Buf, n: usize) {
    let col = column_at(&buf.text, buf.point);
    let bol = buf.bol(buf.point);
    buf.point = bol;
    if bol > 0 {
        let q = buf.text[..bol]
            .trim_end_matches([' ', '\r', '\t', '\n'])
            .len();
        let start = buf.eol(q);
        let prev_end = bol - 1;
        if start < prev_end {
            buf.delete(start, prev_end);
        }
    }
    buf.insert_at_point(&"\n".repeat(n));
    let bol = buf.bol(buf.point);
    buf.point = crate::buffer::move_to_column(&buf.text, bol, col);
}

/// The lambda `maybe-add-blank-after` of `org-insert-heading`: a blank
/// line between the new heading and a heading right after it.
fn blank_after(buf: &mut Buf, blank: bool) {
    let e = buf.eol(buf.point);
    if blank && e < buf.text.len() && stars_at(&buf.text, e + 1).is_some() {
        buf.insert_before_point(e + 1, "\n");
    }
}

/// `org-insert-heading` in `buf` with the default settings
/// (`org-M-RET-may-split-line` and `org-blank-before-new-entry`), at
/// `level` when given. Point ends after the new heading's stars.
fn insert_heading_in(buf: &mut Buf, place: HeadingPlace, level: Option<usize>, ctx: &ParseContext) {
    let limit = ctx.inlinetask_min_level;
    let blank = blank_before_heading(
        &buf.text,
        buf.point,
        place == HeadingPlace::AfterParent,
        limit,
    );
    let current = (!before_first_heading(&buf.text, buf.point, limit))
        .then(|| back_to_heading(&buf.text, buf.point, limit).ok())
        .flatten()
        .map(|(_, l)| l);
    let stars = "*".repeat(level.or(current).unwrap_or(1));
    let bolp = |b: &Buf| b.point == b.bol(b.point);
    if place != HeadingPlace::Here {
        match current {
            None => buf.point = next_heading(&buf.text, buf.point, limit).unwrap_or(buf.text.len()),
            Some(_) => {
                let (mut h, mut l) = back_to_heading(&buf.text, buf.point, limit).unwrap_or((0, 1));
                if place == HeadingPlace::AfterParent
                    && let Some(&(s, pl)) = headings(&buf.text, limit)
                        .iter()
                        .rev()
                        .find(|(s, pl)| *s < h && *pl < l)
                {
                    (h, l) = (s, pl);
                }
                buf.point = subtree_end(&buf.text, h, l, limit);
            }
        }
        if !bolp(buf) {
            buf.insert_at_point("\n");
        }
        if blank && buf.point > 0 && before_first_heading(&buf.text, buf.point - 1, limit) {
            buf.insert_at_point("\n");
            buf.point -= 1;
        }
        if current.is_none() && buf.point < buf.text.len() && buf.point > 0 {
            if stars_at(&buf.text, buf.bol(buf.point)).is_some() {
                buf.insert_at_point("\n");
            }
            buf.point -= 1;
        }
        if !(blank && line_empty(&buf.text, buf.point, -1)) {
            n_empty_lines_before_current(buf, usize::from(blank));
        }
        buf.insert_at_point(&format!("{stars} \n"));
        buf.point -= 1;
        blank_after(buf, blank);
        return;
    }
    let bol = buf.bol(buf.point);
    if stars_at(&buf.text, bol).is_some() {
        if bolp(buf) {
            let p = buf.point;
            if blank {
                buf.insert_before_point(p, "\n");
            }
            buf.insert_before_point(p, &format!("{stars} \n"));
            if !(blank && line_empty(&buf.text, buf.point, -1)) {
                n_empty_lines_before_current(buf, usize::from(blank));
            }
            buf.point = buf.eol(buf.point);
            return;
        }
        let eol = buf.eol(bol);
        let title = org_model::complex_heading(&buf.text[bol..eol], ctx)
            .and_then(|c| c.title)
            .map(|r| (bol + r.start, bol + r.end));
        if let Some((_, te)) = title.filter(|(ts, te)| (*ts..=*te).contains(&buf.point)) {
            let split = buf.text[buf.point..te].to_string();
            buf.delete(buf.point, te);
            let eol = buf.eol(buf.point);
            if buf.text[buf.point..eol]
                .trim_matches([' ', '\t'])
                .is_empty()
            {
                buf.delete(buf.point, eol);
            } else {
                align_tags(buf, bol);
            }
            buf.point = buf.eol(buf.point);
            if blank {
                buf.insert_at_point("\n");
            }
            buf.insert_at_point(&format!("\n{stars} "));
            blank_after(buf, blank);
            if !split.trim().is_empty() {
                buf.insert_at_point(&split);
            }
            return;
        }
        buf.point = buf.eol(buf.point);
        if blank {
            buf.insert_at_point("\n");
        }
        buf.insert_at_point(&format!("\n{stars} "));
        blank_after(buf, blank);
        return;
    }
    if bolp(buf) {
        buf.insert_at_point(&format!("{stars} "));
    } else {
        buf.insert_at_point(&format!("\n{stars} "));
    }
    if !(blank && line_empty(&buf.text, buf.point, -1)) {
        n_empty_lines_before_current(buf, usize::from(blank));
    }
    blank_after(buf, blank);
}

/// `org-insert-heading` (M-RET on a heading): a new heading at the level
/// of the one at point. At the start of a heading it goes above it; in
/// its title the rest of the title moves to it; elsewhere on a heading it
/// follows the heading's line; on a line of text it turns the text after
/// point into the heading. [`HeadingPlace::AfterSubtree`] is
/// `org-insert-heading-respect-content` (C-RET).
pub fn insert_heading(
    text: &str,
    point: usize,
    place: HeadingPlace,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    run(text, point, "Insert heading", |buf| {
        insert_heading_in(buf, place, None, ctx);
        Ok(())
    })
}

/// Doom Emacs's `+org/insert-item-below` (`above` false) and `-above` on
/// a heading: one of its level after its subtree, before the blank lines
/// that end it, or right above it, with its TODO keyword (a done one
/// becoming the first keyword). Point ends after the new stars and
/// keyword.
pub fn insert_heading_doom(
    text: &str,
    point: usize,
    above: bool,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    let limit = ctx.inlinetask_min_level;
    let heading = back_to_heading(text, point, limit).ok();
    let level = heading.map_or(1, |(_, l)| l);
    let keyword = heading.and_then(|(h, _)| {
        let line = &text[h..text[h..].find('\n').map_or(text.len(), |i| h + i)];
        let k = org_model::complex_heading(line, ctx)?
            .todo
            .map(|r| line[r].to_string())?;
        if ctx.done_keywords.contains(&k) {
            ctx.todo_keywords.first().cloned()
        } else {
            Some(k)
        }
    });
    let head = format!(
        "{} {}",
        "*".repeat(level),
        keyword.map_or(String::new(), |k| format!("{k} "))
    );
    run(text, point, "Insert heading", |buf| {
        if above {
            let Some((h, _)) = heading else {
                return user_error("Before first headline");
            };
            buf.point = h;
            buf.insert_at_point(&head);
            let p = buf.point;
            buf.insert_before_point(p, "\n");
            return Ok(());
        }
        // `org-end-of-subtree`: before the blank lines ending it.
        let end = heading.map_or(buf.text.len(), |(h, l)| subtree_end(&buf.text, h, l, limit));
        let at = buf.text[..end]
            .trim_end_matches(['\n', '\r', '\t', ' '])
            .len();
        buf.point = at;
        buf.insert_at_point(&format!("\n{head}"));
        Ok(())
    })
}

/// `org-insert-subheading`: a heading one level below the one at point,
/// after its line (even at its start).
pub fn insert_subheading(
    text: &str,
    point: usize,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    run(text, point, "Insert subheading", |buf| {
        let p = buf.point;
        if p == buf.bol(p) && p < buf.text.len() && buf.text.as_bytes()[p] != b'\n' {
            buf.point += buf.text[p..].chars().next().map_or(1, char::len_utf8);
        }
        insert_heading_in(buf, HeadingPlace::Here, None, ctx);
        let bol = buf.bol(buf.point);
        if let Some(stars) = stars_at(&buf.text, bol) {
            change_level(buf, bol, stars, 1, ctx)?;
            fix_position(buf, ctx);
        }
        Ok(())
    })
}

/// `org-insert-todo-heading` on a heading (lists are
/// [`crate::list::insert_item`]'s): [`insert_heading`], with the TODO
/// keyword of the previous heading of its level, or the first keyword
/// when that one has none or is done, or with `first`.
pub fn insert_todo_heading(
    text: &str,
    point: usize,
    place: HeadingPlace,
    first: bool,
    ctx: &ParseContext,
) -> Result<Transaction, EditError> {
    run(text, point, "Insert TODO heading", |buf| {
        insert_heading_in(buf, place, None, ctx);
        let bol = buf.bol(buf.point);
        let Some(level) = stars_at(&buf.text, bol) else {
            return Ok(());
        };
        // `org-forward-heading-same-level -1`: the previous heading of
        // this level, not past a higher one.
        let previous = headings(&buf.text, ctx.inlinetask_min_level)
            .into_iter()
            .rev()
            .filter(|(s, _)| *s < bol)
            .take_while(|(_, l)| *l >= level)
            .find(|(_, l)| *l == level)
            .map_or(bol, |(s, _)| s);
        let line = &buf.text[previous..buf.eol(previous)];
        let keyword = org_model::complex_heading(line, ctx)
            .and_then(|c| c.todo)
            .map(|r| line[r].to_string());
        let mark = match keyword {
            Some(k) if !first && !ctx.done_keywords.contains(&k) => k,
            _ => match ctx.todo_keywords.first() {
                Some(k) => k.clone(),
                None => return Ok(()),
            },
        };
        let at = bol + level + 1;
        buf.insert_before_point(at, &format!("{mark} "));
        buf.point = at + mark.len() + 1;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_subtrees() {
        let ctx = ParseContext::default();
        let t = "* A\na\n** A1\n* B\n* C :tag:\nc\n";
        let mv = |from, to, level| {
            let tx = move_subtree_to(t, from, to, level, &ctx).unwrap();
            let out = tx.apply(t);
            (out, tx.selection_after.map(|s| s.head))
        };
        // Down, before a heading and to the end.
        assert_eq!(mv(0, 16, 1).0, "* B\n* A\na\n** A1\n* C :tag:\nc\n");
        assert_eq!(
            mv(0, t.len(), 1),
            ("* B\n* C :tag:\nc\n* A\na\n** A1\n".into(), Some(16))
        );
        // Up, one level deeper: under A, after A1.
        let (out, head) = mv(20, 12, 2);
        assert!(out.starts_with("* A\na\n** A1\n** C "), "{out}");
        assert!(out.ends_with(" :tag:\nc\n* B\n") && head == Some(12));
        // In place, another level.
        assert_eq!(mv(12, 12, 2).0, "* A\na\n** A1\n** B\n* C :tag:\nc\n");
        assert!(move_subtree_to(t, 0, 6, 1, &ctx).is_err());
        assert!(move_subtree_to(t, 0, 2, 1, &ctx).is_err());
        // Without a final line feed.
        let t = "* A\n* B";
        let tx = move_subtree_to(t, 0, t.len(), 1, &ctx).unwrap();
        assert_eq!(tx.apply(t), "* B\n* A\n");
    }

    #[test]
    fn odd_levels() {
        assert_eq!(valid_level(3, 1, true), 5);
        assert_eq!(valid_level(3, -1, true), 1);
        assert_eq!(valid_level(1, -1, true), 1);
        assert_eq!(valid_level(2, 1, false), 3);
    }

    #[test]
    fn heading_styles() {
        let ctx = ParseContext::default();
        let set = |t: &str, p: usize, l: usize| {
            let tx = set_level(t, p, l, &ctx).unwrap();
            let out = tx.apply(t);
            (out, tx.selection_after.map(|s| s.head))
        };
        assert_eq!(set("* A\n", 2, 3).0, "*** A\n");
        assert_eq!(set("*** A\n", 4, 1).0, "* A\n");
        assert_eq!(set("** A\n", 4, 0).0, "A\n");
        assert_eq!(set("text\n", 0, 2), ("** text\n".into(), Some(3)));
        assert_eq!(set("  text\n", 4, 1), ("* text\n".into(), Some(4)));
        assert_eq!(set("  text\n", 1, 1), ("* text\n".into(), Some(2)));
        assert_eq!(set("\n", 0, 1), ("* \n".into(), Some(2)));
        assert!(set_level("text\n", 0, 0, &ctx).is_err());
        // Tags stay aligned (`org-tags-column` -77: they end at column 77).
        let out = set("* A :x:\n", 0, 2).0;
        assert!(out.starts_with("** A ") && out.ends_with(" :x:\n"));
        assert_eq!(out.trim_end().len(), 77);
    }
}
