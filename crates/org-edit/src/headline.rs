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
fn back_to_heading(
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
fn valid_level(level: usize, change: isize, odd: bool) -> usize {
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
    let i = hs.iter().position(|(s, _)| *s == bol).expect("heading");
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
fn subtree_end(text: &str, bol: usize, level: usize, limit: Option<usize>) -> usize {
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
        let i = hs.iter().position(|(s, _)| *s == beg).expect("heading");
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
        let level_indicator = (only_stars && buf.text.as_bytes().get(buf.point) != Some(&b'*'))
            .then(|| {
                hs.iter()
                    .rev()
                    .find(|(s, _)| *s <= bol)
                    .map_or(0, |(_, l)| *l)
            });
        let force = level_indicator.or_else(|| {
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
        Ok(())
    })
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
        tx.replace(r.clone(), moved).expect("one edit");
        r.start
    } else if to < r.start {
        tx.replace(to..to, moved).expect("apart");
        tx.replace(r.clone(), "").expect("apart");
        to
    } else {
        let sep = if text.ends_with('\n') || to < text.len() {
            ""
        } else {
            "\n"
        };
        tx.replace(r.clone(), "").expect("apart");
        tx.replace(to..to, format!("{sep}{moved}")).expect("apart");
        to - r.len() + sep.len()
    };
    Ok(tx.select(Selection::caret(point)))
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
