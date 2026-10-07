//! Plain lists: the list structure of `org-list.el` (`org-list-struct`)
//! and the commands built on it: indenting items, cycling bullets,
//! checkboxes and their statistics cookies, moving and inserting items,
//! and repairing numbering.
//!
//! As in Emacs, a command reads the structure of the list at point,
//! changes it, fixes bullets, indentation and checkboxes
//! (`org-list-write-struct`), and writes back only what changed.

use org_model::Document;
use org_syntax::{ItemTerminator, ParseContext};

use crate::buffer::{Buf, EditError};
use crate::headline::{align_tags, headings, org_back_to_heading};
use crate::property::indentation;
use crate::transaction::Transaction;

/// An item of a list structure.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    pos: usize,
    /// Indentation; moving a whole list left can make it negative, which
    /// Emacs writes as none.
    ind: isize,
    /// The bullet with its trailing blanks.
    bullet: String,
    counter: Option<String>,
    checkbox: Option<String>,
    tag: Option<String>,
    end: usize,
}

type Struct = Vec<Item>;

fn bol(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn eol(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

/// `(forward-line 1)`.
fn next_line(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1)
}

/// `(forward-line -1)` from the line at `pos`.
fn prev_line(text: &str, pos: usize) -> usize {
    let b = bol(text, pos);
    if b == 0 { 0 } else { bol(text, b - 1) }
}

/// `org-current-text-indentation` of the line at `b`.
fn indent(text: &str, b: usize) -> usize {
    let mut col = 0;
    for c in text[b..].chars() {
        match c {
            ' ' => col += 1,
            '\t' => col = (col / 8 + 1) * 8,
            _ => break,
        }
    }
    col
}

fn blank_line(text: &str, b: usize) -> bool {
    text[b..eol(text, b)]
        .trim_matches([' ', '\t', '\r'])
        .is_empty()
}

fn line(text: &str, b: usize) -> &str {
    &text[b..eol(text, b)]
}

/// `org-item-re` at the start of the line `b`.
fn is_item(text: &str, b: usize, ctx: &ParseContext) -> bool {
    let l = line(text, b);
    let ws = l.len() - l.trim_start_matches([' ', '\t']).len();
    let r = &l[ws..];
    let bytes = r.as_bytes();
    let blen = match bytes.first() {
        Some(b'-' | b'+') => 1,
        Some(b'*') if ws > 0 => 1,
        _ => {
            let digits = bytes.iter().take_while(|c| c.is_ascii_digit()).count();
            let n = if digits > 0 {
                digits
            } else if ctx.list_allow_alphabetical
                && bytes.first().is_some_and(u8::is_ascii_alphabetic)
            {
                1
            } else {
                return false;
            };
            let term_ok = matches!(
                (bytes.get(n), ctx.item_terminator),
                (Some(b'.'), ItemTerminator::Both | ItemTerminator::Dot)
                    | (Some(b')'), ItemTerminator::Both | ItemTerminator::Paren)
            );
            if !term_ok {
                return false;
            }
            n + 1
        }
    };
    matches!(bytes.get(blen), None | Some(b' ' | b'\t'))
}

/// `org-list-full-item-re` at the line `b`, as positions.
#[derive(Debug, Clone, Copy)]
struct FullItem {
    bullet: (usize, usize),
    counter: Option<(usize, usize)>,
    checkbox: Option<(usize, usize)>,
    tag: Option<(usize, usize)>,
    end: usize,
}

fn full_item(text: &str, b: usize) -> Option<FullItem> {
    let e = eol(text, b);
    let t = text.as_bytes();
    let mut i = b;
    while i < e && matches!(t[i], b' ' | b'\t') {
        i += 1;
    }
    let bs = i;
    if i < e && matches!(t[i], b'-' | b'+' | b'*') {
        i += 1;
    } else {
        let d = t[i..e].iter().take_while(|c| c.is_ascii_digit()).count();
        if d > 0 {
            i += d;
        } else if i < e && t[i].is_ascii_alphabetic() {
            i += 1;
        } else {
            return None;
        }
        if i < e && matches!(t[i], b'.' | b')') {
            i += 1;
        } else {
            return None;
        }
    }
    let ws = t[i..e]
        .iter()
        .take_while(|c| matches!(c, b' ' | b'\t'))
        .count();
    if ws == 0 && i < e {
        return None;
    }
    i += ws;
    let bullet = (bs, i);
    // `\[@\(?:start:\)?\([0-9]+\|[A-Za-z]\)\][ \t]*`
    let mut counter = None;
    if text[i..e].starts_with("[@") {
        let mut j = i + 2;
        if text[j..e].starts_with("start:") {
            j += 6;
        }
        let d = t[j..e].iter().take_while(|c| c.is_ascii_digit()).count();
        let n = if d > 0 {
            d
        } else if j < e && t[j].is_ascii_alphabetic() {
            1
        } else {
            0
        };
        if n > 0 && t.get(j + n) == Some(&b']') {
            counter = Some((j, j + n));
            i = j + n + 1;
            while i < e && matches!(t[i], b' ' | b'\t') {
                i += 1;
            }
        }
    }
    // `\(\[[ X-]\]\)\(?:[ \t]+\|$\)`, case-insensitively.
    let mut checkbox = None;
    if i + 3 <= e
        && t[i] == b'['
        && matches!(t[i + 1], b' ' | b'X' | b'x' | b'-')
        && t[i + 2] == b']'
    {
        let after = i + 3;
        let ws = t[after..e]
            .iter()
            .take_while(|c| matches!(c, b' ' | b'\t'))
            .count();
        if ws > 0 || after == e {
            checkbox = Some((i, after));
            i = after + ws;
        }
    }
    // `\(.*\)[ \t]+::\(?:[ \t]+\|$\)`: the last such `::`.
    let mut tag = None;
    let rest = &text[i..e];
    let mut best = None;
    for (k, _) in rest.match_indices("::") {
        let before_ws = rest[..k].len() - rest[..k].trim_end_matches([' ', '\t']).len();
        let after = &rest[k + 2..];
        if before_ws > 0 && (after.is_empty() || after.starts_with([' ', '\t'])) {
            best = Some((
                k,
                before_ws,
                after.len() - after.trim_start_matches([' ', '\t']).len(),
            ));
        }
    }
    if let Some((k, _, aws)) = best {
        // The greedy `.*` keeps all but one blank before `::`.
        tag = Some((i, i + k - 1));
        i = i + k + 2 + aws;
    }
    Some(FullItem {
        bullet,
        counter,
        checkbox,
        tag,
        end: i,
    })
}

/// `org-list-context`: (lim-up, lim-down, context) around the line `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    Default,
    Drawer,
    Block,
    Invalid,
    Inlinetask,
}

fn is_drawer_begin(l: &str) -> bool {
    let t = l
        .trim_start_matches([' ', '\t'])
        .trim_end_matches([' ', '\t']);
    t.len() >= 3
        && t.starts_with(':')
        && t.ends_with(':')
        && t[1..t.len() - 1]
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn is_drawer_end(l: &str) -> bool {
    let t = l.trim_start_matches([' ', '\t']);
    t.len() >= 5 && t.as_bytes()[..5].eq_ignore_ascii_case(b":END:")
}

/// `^[ \t]*#\+\(begin\|end\)_`: Some(true) for begin, Some(false) for end.
fn block_line(l: &str) -> Option<bool> {
    let t = l.trim_start_matches([' ', '\t']);
    let r = t.strip_prefix("#+")?;
    if r.len() >= 6 && r.as_bytes()[..6].eq_ignore_ascii_case(b"begin_") {
        Some(true)
    } else if r.len() >= 4 && r.as_bytes()[..4].eq_ignore_ascii_case(b"end_") {
        Some(false)
    } else {
        None
    }
}

fn inlinetask_line(text: &str, b: usize, ctx: &ParseContext) -> Option<bool> {
    let min = ctx.inlinetask_min_level?;
    let min = if ctx.odd_levels_only {
        2 * min - 1
    } else {
        min
    };
    let l = line(text, b);
    let stars = l.bytes().take_while(|c| *c == b'*').count();
    let rest = &l[stars..];
    let word = rest.trim_start_matches([' ', '\t']);
    if stars < min || word.len() == rest.len() {
        return None;
    }
    let is_end = word.get(..3).is_some_and(|w| w.eq_ignore_ascii_case("end"))
        && word[3..].trim_matches([' ', '\t']).is_empty();
    Some(is_end)
}

/// Line starts from `from` down to `to` (inclusive), backwards.
fn lines_back(text: &str, from: usize, to: usize) -> impl Iterator<Item = usize> + '_ {
    let mut cur = Some(bol(text, from));
    std::iter::from_fn(move || {
        let c = cur?;
        if c < to {
            return None;
        }
        cur = if c == 0 { None } else { Some(bol(text, c - 1)) };
        Some(c)
    })
}

fn list_context(text: &str, pos: usize, ctx: &ParseContext) -> (usize, usize, Context) {
    let b = bol(text, pos);
    let limited: Vec<(usize, usize)> = headings(text, ctx.inlinetask_min_level);
    let mut lim_up = org_back_to_heading(text, b, ctx).unwrap_or(0);
    let mut lim_down = limited
        .iter()
        .find(|(s, _)| *s > b)
        .map_or(text.len(), |(s, _)| *s);
    let mut context = Context::Default;
    // Drawer.
    let l = line(text, b);
    if !is_drawer_begin(l) && !is_drawer_end(l) {
        let beg = lines_back(text, b, lim_up)
            .skip(1)
            .find(|&x| is_drawer_begin(line(text, x)));
        if let Some(db) = beg {
            let beg = eol(text, db) + 1;
            let mut end = lim_down;
            let mut x = db;
            loop {
                x = next_line(text, x);
                if x >= lim_down || x >= text.len() {
                    break;
                }
                if is_drawer_end(line(text, x)) {
                    end = x - 1;
                    break;
                }
            }
            if end >= b {
                lim_up = beg;
                lim_down = end;
                context = Context::Drawer;
            }
        }
    }
    // Block.
    if block_line(line(text, b)).is_none() {
        let found = lines_back(text, b, lim_up)
            .skip(1)
            .find(|&x| block_line(line(text, x)).is_some());
        if let Some(bb) = found
            && block_line(line(text, bb)) == Some(true)
        {
            let t = line(text, bb).trim_start_matches([' ', '\t']);
            let ty: String = t[8..]
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect::<String>()
                .to_lowercase();
            let beg = eol(text, bb) + 1;
            let mut end = lim_down;
            let mut is_end = false;
            let mut x = bb;
            loop {
                x = next_line(text, x);
                if x >= lim_down || x >= text.len() {
                    break;
                }
                if let Some(k) = block_line(line(text, x)) {
                    end = x - 1;
                    is_end = !k;
                    break;
                }
            }
            if end >= b && is_end && !ty.is_empty() {
                lim_up = beg;
                lim_down = end;
                context = if ["example", "verse", "src", "export"].contains(&ty.as_str()) {
                    Context::Invalid
                } else {
                    Context::Block
                };
            }
        }
    }
    // Inlinetask.
    if ctx.inlinetask_min_level.is_some() && !line(text, b).starts_with('*') {
        let found = lines_back(text, b, lim_up)
            .skip(1)
            .find(|&x| inlinetask_line(text, x, ctx).is_some());
        if let Some(ib) = found
            && inlinetask_line(text, ib, ctx) == Some(false)
        {
            let beg = eol(text, ib) + 1;
            let mut x = ib;
            loop {
                x = next_line(text, x);
                if x >= lim_down || x >= text.len() {
                    break;
                }
                if inlinetask_line(text, x, ctx) == Some(true) {
                    if eol(text, x) > b {
                        lim_up = beg;
                        lim_down = x - 1;
                        context = Context::Inlinetask;
                    }
                    break;
                }
            }
        }
    }
    (lim_up, lim_down, context)
}

/// `org-list-struct` for the list around the item at line `start`.
#[expect(
    clippy::expect_used,
    reason = "the item starts come from the scan of the list's own bullets"
)]
fn list_struct(text: &str, start: usize, ctx: &ParseContext) -> Struct {
    let (lim_up, lim_down, _) = list_context(text, start, ctx);
    let assoc = |b: usize, ind: usize| -> Item {
        let ind = ind as isize;
        let f = full_item(text, b).expect("item");
        let bullet = text[f.bullet.0..f.bullet.1].to_string();
        let unordered = bullet.trim_start().starts_with(['-', '+', '*']);
        Item {
            pos: b,
            ind,
            bullet,
            counter: f.counter.map(|(s, e)| text[s..e].to_string()),
            checkbox: f.checkbox.map(|(s, e)| text[s..e].to_string()),
            tag: if unordered {
                f.tag.map(|(s, e)| text[s..e].to_string())
            } else {
                None
            },
            end: 0,
        }
    };
    let is_list_end = |b: usize| -> bool {
        // `^[ \t]*\n[ \t]*\n`
        blank_line(text, b) && eol(text, b) < text.len() && {
            let n = next_line(text, b);
            n < text.len() && blank_line(text, n) && eol(text, n) < text.len()
        }
    };
    let end_before_blank = |p: usize| -> usize {
        let q = text[..p].trim_end_matches([' ', '\r', '\t', '\n']).len();
        (eol(text, q) + 1).min(lim_down)
    };
    let start = bol(text, start);
    let mut text_min_ind = 10000;
    let mut beg_cell = (start, indent(text, start));
    let mut itm_lst: Vec<Item> = Vec::new(); // pushed front-first as Emacs pushes
    let mut end_lst: Vec<(usize, usize)> = Vec::new();
    // 1. Up from the starting item.
    let mut p = start;
    let itm_final: Vec<Item>;
    loop {
        let ind = indent(text, p);
        if p <= lim_up {
            if !is_item(text, p, ctx) {
                itm_final = keep_from(&itm_lst, beg_cell.0);
            } else {
                beg_cell = (p, ind);
                let mut v = vec![assoc(p, ind)];
                v.extend(itm_lst.iter().cloned());
                itm_final = v;
            }
            break;
        }
        if is_list_end(p) {
            itm_final = keep_from(&itm_lst, beg_cell.0);
            break;
        }
        if is_item(text, p, ctx) {
            itm_lst.insert(0, assoc(p, ind));
            end_lst.insert(0, (ind, p));
            if ind < text_min_ind {
                beg_cell = (p, ind);
            }
            p = prev_line(text, p);
            continue;
        }
        let l = line(text, p);
        let t = l.trim_start_matches([' ', '\t']);
        if t.len() >= 6
            && t.as_bytes()[..6].eq_ignore_ascii_case(b"#+end_")
            && let Some(x) = lines_back(text, p, lim_up).skip(1).find(|&x| {
                let t = line(text, x).trim_start_matches([' ', '\t']);
                t.len() >= 8 && t.as_bytes()[..8].eq_ignore_ascii_case(b"#+begin_")
            })
        {
            p = x;
            continue;
        }
        if is_drawer_end(l)
            && let Some(x) = lines_back(text, p, lim_up)
                .skip(1)
                .find(|&x| is_drawer_begin(line(text, x)))
        {
            p = x;
            continue;
        }
        if inlinetask_line(text, p, ctx).is_some() {
            // `org-inlinetask-goto-beginning`, then one line up.
            let b0 = lines_back(text, p, 0)
                .find(|&x| inlinetask_line(text, x, ctx) == Some(false))
                .unwrap_or(p);
            p = prev_line(text, b0);
            if b0 == 0 {
                itm_final = keep_from(&itm_lst, beg_cell.0);
                break;
            }
            continue;
        }
        if blank_line(text, p) {
            if p == 0 {
                itm_final = keep_from(&itm_lst, beg_cell.0);
                break;
            }
            p = prev_line(text, p);
            continue;
        }
        if ind == 0 {
            itm_final = keep_from(&itm_lst, beg_cell.0);
            break;
        }
        if ind < text_min_ind {
            text_min_ind = ind;
        }
        end_lst.insert(0, (ind, p));
        if p == 0 {
            itm_final = keep_from(&itm_lst, beg_cell.0);
            break;
        }
        p = prev_line(text, p);
    }
    // 2. Down from the starting item.
    let mut itm2: Vec<Item> = Vec::new();
    let mut end2: Vec<(usize, usize)> = Vec::new();
    let mut p = start;
    loop {
        let ind = indent(text, p);
        if p >= lim_down || p >= text.len() {
            end2.push((0, end_before_blank(p.min(text.len()))));
            break;
        }
        if is_list_end(p) {
            end2.push((0, p));
            break;
        }
        if is_item(text, p, ctx) {
            itm2.push(assoc(p, ind));
            end2.push((ind, p));
            p = next_line(text, p);
            continue;
        }
        if inlinetask_line(text, p, ctx).is_some() {
            // `org-inlinetask-goto-end`: after the END line.
            let mut x = p;
            while x < text.len() && inlinetask_line(text, x, ctx) != Some(true) {
                x = next_line(text, x);
            }
            p = next_line(text, x);
            continue;
        }
        if blank_line(text, p) {
            p = next_line(text, p);
            continue;
        }
        if ind <= beg_cell.1 {
            end2.push((0, end_before_blank(p)));
            break;
        }
        if let Some(last) = itm2.last()
            && ind as isize <= last.ind
        {
            end2.push((ind, p));
        }
        let t = line(text, p).trim_start_matches([' ', '\t']);
        if t.len() >= 8 && t.as_bytes()[..8].eq_ignore_ascii_case(b"#+begin_") {
            let mut x = p;
            loop {
                x = next_line(text, x);
                if x >= lim_down || x >= text.len() {
                    break;
                }
                let t = line(text, x).trim_start_matches([' ', '\t']);
                if t.len() >= 6 && t.as_bytes()[..6].eq_ignore_ascii_case(b"#+end_") {
                    p = x;
                    break;
                }
            }
        } else if is_drawer_begin(line(text, p)) {
            let mut x = p;
            loop {
                x = next_line(text, x);
                if x >= lim_down || x >= text.len() {
                    break;
                }
                if is_drawer_end(line(text, x)) {
                    p = x;
                    break;
                }
            }
        }
        p = next_line(text, p);
    }
    // The starting item is in both lists: drop it from the second.
    let mut st: Struct = itm_final;
    st.extend(itm2.into_iter().skip(1));
    let mut ends: Vec<(isize, usize)> = end_lst.into_iter().map(|(i, p)| (i as isize, p)).collect();
    ends.extend(end2.into_iter().skip(1).map(|(i, p)| (i as isize, p)));
    assoc_end(&mut st, &ends);
    st
}

/// The items of `lst` from the one at `pos` on.
fn keep_from(lst: &[Item], pos: usize) -> Vec<Item> {
    match lst.iter().position(|i| i.pos == pos) {
        Some(k) => lst[k..].to_vec(),
        None => Vec::new(),
    }
}

/// `org-list-struct-assoc-end`: `ends` are (indentation, position) in
/// order.
fn assoc_end(st: &mut Struct, ends: &[(isize, usize)]) {
    let mut k = 0;
    for it in st.iter_mut() {
        while k < ends.len() && ends[k].1 <= it.pos {
            k += 1;
        }
        // `(assoc-default ind endings '<=)`: the first ending whose
        // indentation is <= the item's.
        it.end = ends[k..]
            .iter()
            .find(|(ind, _)| *ind <= it.ind)
            .map_or(0, |(_, p)| *p);
    }
}

#[expect(
    clippy::expect_used,
    reason = "items are read from the list's own structure, which holds them"
)]
fn get(st: &Struct, pos: usize) -> &Item {
    st.iter().find(|i| i.pos == pos).expect("item in struct")
}

#[expect(
    clippy::expect_used,
    reason = "items are read from the list's own structure, which holds them"
)]
fn get_mut(st: &mut Struct, pos: usize) -> &mut Item {
    st.iter_mut()
        .find(|i| i.pos == pos)
        .expect("item in struct")
}

/// `org-list-prevs-alist`: (item, previous item in the same sub-list).
fn prevs(st: &Struct) -> Vec<(usize, Option<usize>)> {
    st.iter()
        .map(|e| (e.pos, st.iter().find(|x| x.end == e.pos).map(|x| x.pos)))
        .collect()
}

/// `org-list-parents-alist`.
fn parents(st: &Struct) -> Vec<(usize, Option<usize>)> {
    if st.is_empty() {
        return Vec::new();
    }
    let mut ind_to_ori: Vec<(isize, Option<usize>)> = vec![(st[0].ind, None)];
    let mut prev_pos = vec![st[0].pos];
    let mut out = vec![(st[0].pos, None)];
    for item in &st[1..] {
        let (pos, ind) = (item.pos, item.ind);
        let prev_ind = ind_to_ori[0].0;
        prev_pos.insert(0, pos);
        if prev_ind > ind {
            if let Some(k) = ind_to_ori.iter().position(|e| e.0 == ind) {
                ind_to_ori.drain(..k);
            } else if let Some(k) = ind_to_ori.iter().position(|e| e.0 < ind) {
                ind_to_ori.drain(..k);
            } else {
                ind_to_ori = vec![(ind, None)];
            }
            out.push((pos, ind_to_ori[0].1));
        } else if prev_ind < ind {
            let origin = prev_pos.get(1).copied();
            ind_to_ori.insert(0, (ind, origin));
            out.push((pos, origin));
        } else {
            out.push((pos, ind_to_ori[0].1));
        }
    }
    out
}

fn prev_of(prevs: &[(usize, Option<usize>)], item: usize) -> Option<usize> {
    prevs.iter().find(|(p, _)| *p == item).and_then(|(_, v)| *v)
}

fn next_of(prevs: &[(usize, Option<usize>)], item: usize) -> Option<usize> {
    prevs
        .iter()
        .find(|(_, v)| *v == Some(item))
        .map(|(p, _)| *p)
}

fn parent_of(parents: &[(usize, Option<usize>)], item: usize) -> Option<usize> {
    parents
        .iter()
        .find(|(p, _)| *p == item)
        .and_then(|(_, v)| *v)
}

fn children_of(parents: &[(usize, Option<usize>)], item: usize) -> Vec<usize> {
    parents
        .iter()
        .filter(|(_, v)| *v == Some(item))
        .map(|(p, _)| *p)
        .collect()
}

fn first_item(prevs: &[(usize, Option<usize>)], item: usize) -> usize {
    let mut i = item;
    while let Some(p) = prev_of(prevs, i) {
        i = p;
    }
    i
}

fn has_child(st: &Struct, item: usize) -> Option<usize> {
    let k = st.iter().position(|i| i.pos == item)?;
    let next = st.get(k + 1)?;
    (st[k].ind < next.ind).then_some(next.pos)
}

#[expect(
    clippy::expect_used,
    reason = "items are read from the list's own structure, which holds them"
)]
fn subtree(st: &Struct, item: usize) -> Vec<usize> {
    let it = get(st, item);
    let k = st.iter().position(|i| i.pos == item).expect("item");
    st[k + 1..]
        .iter()
        .take_while(|i| i.pos < it.end)
        .map(|i| i.pos)
        .collect()
}

fn end_before_blank(text: &str, end: usize) -> usize {
    let q = text[..end].trim_end_matches([' ', '\r', '\t', '\n']).len();
    eol(text, q)
}

/// `org-list-bullet-string`: the bullet with one space after it.
fn bullet_string(bullet: &str) -> String {
    let t = bullet.trim_start_matches([' ', '\t']);
    match t.find([' ', '\t']) {
        Some(i) => format!("{} ", &t[..i]),
        None if t.is_empty() => bullet.to_string(),
        None => format!("{t} "),
    }
}

fn use_alpha_bul(
    st: &Struct,
    prevs: &[(usize, Option<usize>)],
    first: usize,
    ctx: &ParseContext,
) -> bool {
    if !ctx.list_allow_alphabetical {
        return false;
    }
    let mut ascii = 64u32;
    let mut item = Some(first);
    while let Some(i) = item {
        match get(st, i)
            .counter
            .as_deref()
            .filter(|c| c.chars().any(|x| x.is_ascii_alphabetic()))
        {
            Some(c) => {
                ascii = c
                    .to_ascii_uppercase()
                    .chars()
                    .next()
                    .map_or(ascii, |x| x as u32)
            }
            None => ascii += 1,
        }
        if ascii > 90 {
            return false;
        }
        item = next_of(prevs, i);
    }
    true
}

/// `org-list-inc-bullet-maybe`.
fn inc_bullet(b: &str) -> String {
    if let Some(s) = b.find(|c: char| c.is_ascii_digit()) {
        let e = s + b[s..].bytes().take_while(u8::is_ascii_digit).count();
        let n: u64 = b[s..e].parse().unwrap_or(0);
        return format!("{}{}{}", &b[..s], n + 1, &b[e..]);
    }
    if let Some(s) = b.find(|c: char| c.is_ascii_alphabetic()) {
        let c = b.as_bytes()[s] + 1;
        return format!("{}{}{}", &b[..s], c as char, &b[s + 1..]);
    }
    b.to_string()
}

/// Replaces the first match of the digits or a letter in `b` with `r`.
fn replace_first(b: &str, pred: impl Fn(char) -> bool, run: bool, r: &str) -> String {
    match b.find(&pred) {
        Some(s) => {
            let e = if run {
                s + b[s..]
                    .chars()
                    .take_while(|c| pred(*c))
                    .map(char::len_utf8)
                    .sum::<usize>()
            } else {
                s + 1
            };
            format!("{}{}{}", &b[..s], r, &b[e..])
        }
        None => b.to_string(),
    }
}

/// `org-list-struct-fix-bul`.
#[expect(
    clippy::expect_used,
    reason = "a previous item exists in that arm, and the arm's guard found a letter or digit"
)]
fn fix_bul(st: &mut Struct, prevs: &[(usize, Option<usize>)], ctx: &ParseContext) {
    let items: Vec<usize> = st.iter().map(|i| i.pos).collect();
    let alpha = |c: char| c.is_ascii_alphabetic();
    let digit = |c: char| c.is_ascii_digit();
    for item in items {
        let prev = prev_of(prevs, item);
        let prev_bul = prev.map(|p| get(st, p).bullet.clone());
        let counter = get(st, item).counter.clone();
        let bullet = get(st, item).bullet.clone();
        let alphap = prev.is_none() && use_alpha_bul(st, prevs, item, ctx);
        let has = |s: &str, f: fn(char) -> bool| s.chars().any(f);
        let new = match (&prev_bul, &counter) {
            (Some(pb), Some(c))
                if has(c, char::is_alphanumeric)
                    && c.chars().any(|x| x.is_ascii_alphabetic())
                    && pb.chars().any(|x| x.is_ascii_alphabetic()) =>
            {
                let real = if pb.chars().any(|x| x.is_ascii_lowercase()) {
                    c.to_lowercase()
                } else {
                    c.to_uppercase()
                };
                replace_first(pb, alpha, false, &real)
            }
            (Some(pb), Some(c))
                if c.chars().any(|x| x.is_ascii_digit())
                    && pb.chars().any(|x| x.is_ascii_digit()) =>
            {
                let digits: String = c
                    .chars()
                    .skip_while(|x| !x.is_ascii_digit())
                    .take_while(char::is_ascii_digit)
                    .collect();
                replace_first(pb, digit, true, &digits)
            }
            (Some(_), _) => inc_bullet(&get(st, prev.expect("prev")).bullet),
            (None, Some(c))
                if use_alpha_bul(st, prevs, item, ctx)
                    && c.chars().any(|x| x.is_ascii_alphabetic())
                    && bullet.chars().any(|x| x.is_ascii_alphabetic()) =>
            {
                let real = if bullet.chars().any(|x| x.is_ascii_lowercase()) {
                    c.to_lowercase()
                } else {
                    c.to_uppercase()
                };
                replace_first(&bullet, alpha, false, &real)
            }
            (None, Some(c))
                if c.chars().any(|x| x.is_ascii_digit())
                    && bullet.chars().any(|x| x.is_ascii_digit()) =>
            {
                let digits: String = c
                    .chars()
                    .skip_while(|x| !x.is_ascii_digit())
                    .take_while(char::is_ascii_digit)
                    .collect();
                replace_first(&bullet, digit, true, &digits)
            }
            _ if alphap && bullet.chars().any(|x| x.is_ascii_uppercase()) => {
                replace_first(&bullet, |c| c.is_ascii_uppercase(), false, "A")
            }
            _ if alphap && bullet.chars().any(|x| x.is_ascii_lowercase()) => {
                replace_first(&bullet, |c| c.is_ascii_lowercase(), false, "a")
            }
            _ if bullet.chars().any(|x| x.is_ascii_alphanumeric()) => {
                // `\([0-9]+\|[A-Za-z]\)` → "1".
                let s = bullet
                    .find(|c: char| c.is_ascii_alphanumeric())
                    .expect("alnum");
                let e = if bullet.as_bytes()[s].is_ascii_digit() {
                    s + bullet[s..].bytes().take_while(u8::is_ascii_digit).count()
                } else {
                    s + 1
                };
                format!("{}1{}", &bullet[..s], &bullet[e..])
            }
            _ => bullet.clone(),
        };
        get_mut(st, item).bullet = bullet_string(&new);
    }
}

/// `org-list-struct-fix-ind`.
fn fix_ind(st: &mut Struct, parents: &[(usize, Option<usize>)], bullet_size: Option<usize>) {
    if st.is_empty() {
        return;
    }
    let top_ind = st[0].ind;
    let items: Vec<usize> = st[1..].iter().map(|i| i.pos).collect();
    for item in items {
        let new = match parent_of(parents, item) {
            Some(p) => {
                let pi = get(st, p);
                bullet_size.unwrap_or(pi.bullet.chars().count()) as isize + pi.ind
            }
            None => top_ind,
        };
        get_mut(st, item).ind = new;
    }
}

/// `org-list-struct-fix-box`. Returns the blocking item with `ordered`.
fn fix_box(st: &mut Struct, parents: &[(usize, Option<usize>)], ordered: bool) -> Option<usize> {
    let all: Vec<usize> = st.iter().map(|i| i.pos).collect();
    let mut parent_list: Vec<usize> = Vec::new();
    for &e in &all {
        if let Some(p) = parent_of(parents, e)
            && get(st, p).checkbox.is_some()
            && !parent_list.contains(&p)
        {
            parent_list.insert(0, p);
        }
    }
    // Deepest first; the sort is stable.
    parent_list.sort_by_key(|p| std::cmp::Reverse(get(st, *p).ind));
    for p in parent_list {
        let boxes: Vec<Option<String>> = children_of(parents, p)
            .iter()
            .map(|c| get(st, *c).checkbox.clone())
            .collect();
        let has = |s: &str| boxes.iter().any(|b| b.as_deref() == Some(s));
        let new = if (has("[ ]") && has("[X]")) || has("[-]") {
            Some("[-]".to_string())
        } else if has("[X]") {
            Some("[X]".to_string())
        } else if has("[ ]") {
            Some("[ ]".to_string())
        } else {
            get(st, p).checkbox.clone()
        };
        get_mut(st, p).checkbox = new;
    }
    if ordered {
        let boxes: Vec<Option<String>> = all.iter().map(|e| get(st, *e).checkbox.clone()).collect();
        if let Some(k) = boxes.iter().position(|b| b.as_deref() == Some("[ ]"))
            && boxes[k..].iter().any(|b| b.as_deref() == Some("[X]"))
        {
            for &e in &all[k..] {
                if get(st, e).checkbox.is_some() {
                    get_mut(st, e).checkbox = Some("[ ]".into());
                }
            }
            fix_box(st, parents, false);
            return Some(all[k]);
        }
    }
    None
}

/// `org-list-struct-fix-item-end`.
fn fix_item_end(st: &mut Struct) {
    let mut end_list: Vec<(isize, usize)> = Vec::new();
    let mut acc_end: Vec<(usize, usize)> = Vec::new(); // (end, item), newest first
    for it in st.iter() {
        if !st.iter().any(|x| x.pos == it.end) {
            let item_up = acc_end.iter().find(|(e, _)| *e > it.end).map(|(_, i)| *i);
            let ind = item_up.map_or(0, |u| get(st, u).ind + 2);
            end_list.insert(0, (ind, it.end));
        }
        end_list.insert(0, (it.ind, it.pos));
        acc_end.insert(0, (it.end, it.pos));
    }
    end_list.sort_by_key(|e| e.1);
    assoc_end(st, &end_list);
}

/// `org-list-struct-apply-struct`: writes the difference between `st` and
/// `old` into `buf`.
fn apply_struct(buf: &mut Buf, st: &Struct, old: &Struct, ctx: &ParseContext) {
    // 1. Shifts and endings.
    let mut itm_shift: Vec<(usize, isize)> = Vec::new();
    let mut end_list: Vec<(usize, Option<usize>)> = Vec::new();
    let mut acc_end: Vec<(usize, usize)> = Vec::new();
    for e in old {
        let new = get(st, e.pos);
        let shift = (new.ind + new.bullet.chars().count() as isize)
            - (e.ind + e.bullet.chars().count() as isize);
        itm_shift.insert(0, (e.pos, shift));
        if !old.iter().any(|x| x.pos == e.end) {
            let item_up = acc_end
                .iter()
                .find(|(end, _)| *end > e.end)
                .map(|(_, i)| *i);
            end_list.insert(0, (e.end, item_up));
        }
        acc_end.insert(0, (e.end, e.pos));
    }
    let mut all_ends: Vec<usize> = itm_shift.iter().map(|(p, _)| *p).collect();
    for (e, _) in &end_list {
        if !all_ends.contains(e) {
            all_ends.push(*e);
        }
    }
    all_ends.sort();
    all_ends.dedup();
    acc_end.reverse();
    // 2. Slices (down, up, delta), last first.
    let mut slices: Vec<(usize, usize, isize)> = Vec::new();
    for w in all_ends.windows(2) {
        let (up, down) = (w[0], w[1]);
        let delta = if st.iter().any(|i| i.pos == up) {
            itm_shift
                .iter()
                .find(|(p, _)| *p == up)
                .map_or(0, |(_, d)| *d)
        } else {
            let child = acc_end.iter().find(|(e, _)| *e == up).map(|(_, i)| *i);
            match child {
                Some(c) => {
                    let ind = get(st, c).ind;
                    let mut min_ind = usize::MAX;
                    let t = &buf.text;
                    let mut p = up;
                    while p < down {
                        if !blank_line(t, p) {
                            min_ind = min_ind.min(indent(t, p));
                            let l = line(t, p).trim_start_matches([' ', '\t']);
                            if l.len() >= 7 && l.as_bytes()[..7].eq_ignore_ascii_case(b"#+BEGIN") {
                                let kind =
                                    l[7..].split_whitespace().next().unwrap_or("").to_string();
                                let _ = kind;
                                // Skip to the matching `#+END`.
                                let mut x = p;
                                loop {
                                    x = next_line(t, x);
                                    if x >= down {
                                        break;
                                    }
                                    let l2 = line(t, x).trim_start_matches([' ', '\t']);
                                    if l2.len() >= 5
                                        && l2.as_bytes()[..5].eq_ignore_ascii_case(b"#+END")
                                    {
                                        p = x;
                                        break;
                                    }
                                }
                            } else if is_drawer_begin(line(t, p)) {
                                let mut x = p;
                                loop {
                                    x = next_line(t, x);
                                    if x >= down {
                                        break;
                                    }
                                    if line(t, x).trim().eq_ignore_ascii_case(":END:") {
                                        p = x;
                                        break;
                                    }
                                }
                            }
                        }
                        let n = next_line(t, p);
                        if n == p {
                            break;
                        }
                        p = n;
                    }
                    if min_ind == usize::MAX {
                        0
                    } else {
                        ind - min_ind as isize
                    }
                }
                None => 0,
            }
        };
        slices.insert(0, (down, up, delta));
    }
    // 3. Apply, last slice first.
    for (down, up, delta) in slices {
        if delta != 0 {
            shift_body_ind(buf, down, up, delta, ctx);
        }
        if let Some(cell) = st.iter().find(|i| i.pos == up)
            && old.iter().find(|i| i.pos == up) != Some(cell)
        {
            modify_item(buf, up, st, old);
        }
    }
}

/// Shifts the indentation of the lines between `beg` and `end` by `delta`,
/// from the line before `end` up.
fn shift_body_ind(buf: &mut Buf, end: usize, beg: usize, delta: isize, ctx: &ParseContext) {
    let t = &buf.text;
    let q = t[..end].trim_end_matches([' ', '\r', '\t', '\n']).len();
    let mut p = bol(t, q);
    loop {
        let t = &buf.text;
        if !(p > beg || (p == beg && !is_item(t, p, ctx))) {
            break;
        }
        if inlinetask_line(t, p, ctx).is_some() {
            if let Some(b0) =
                lines_back(t, p, 0).find(|&x| inlinetask_line(t, x, ctx) == Some(false))
            {
                p = b0;
            }
        } else if !blank_line(t, p) {
            let cur = indent(t, p) as isize;
            indent_line_to(buf, p, (cur + delta).max(0) as usize);
        }
        if p == 0 {
            break;
        }
        p = prev_line(&buf.text, p);
    }
}

/// `indent-to` at `p` (column `from`) up to column `col`, with
/// `indent-tabs-mode`: tabs to the last tab stop, then spaces.
fn indent_to(buf: &mut Buf, p: usize, from: usize, col: usize) {
    if col <= from {
        return;
    }
    let tabs = col / 8 - from / 8;
    let s = if tabs > 0 {
        format!("{}{}", "\t".repeat(tabs), " ".repeat(col - (col / 8) * 8))
    } else {
        " ".repeat(col - from)
    };
    buf.insert_before_point(p, &s);
}

/// `indent-line-to` for the line at `b`, with `indent-tabs-mode`.
fn indent_line_to(buf: &mut Buf, b: usize, col: usize) {
    let n = buf.text[b..]
        .bytes()
        .take_while(|c| matches!(c, b' ' | b'\t'))
        .count();
    let p = b + n;
    let cur = indent(&buf.text, b);
    if cur < col {
        let (mut p, mut from) = (p, cur);
        if col - (cur / 8) * 8 >= 8 {
            // Trailing spaces go; tabs stay.
            let spaces = buf.text[b..p]
                .bytes()
                .rev()
                .take_while(|c| *c == b' ')
                .count();
            buf.delete(p - spaces, p);
            p -= spaces;
            from = indent(&buf.text[..p], b);
        }
        indent_to(buf, p, from, col);
    } else if cur > col {
        // `move-to-column` with FORCE splits a tab that spans COL.
        let mut c = 0;
        let mut k = b;
        while k < p {
            let w = if buf.text.as_bytes()[k] == b'\t' {
                (c / 8 + 1) * 8 - c
            } else {
                1
            };
            if c + w > col {
                break;
            }
            c += w;
            k += 1;
        }
        buf.replace(k, p, &" ".repeat(col - c));
    }
}

/// Rewrites the bullet, checkbox and indentation of the item at `item`.
fn modify_item(buf: &mut Buf, item: usize, st: &Struct, old: &Struct) {
    let new = get(st, item);
    let old_bul = &get(old, item).bullet;
    let new_bul = bullet_string(&new.bullet);
    let Some(f) = full_item(&buf.text, item) else {
        return;
    };
    // a. The bullet.
    if *old_bul != new_bul {
        let origin = buf.point;
        let keep_space = if f.bullet.0 <= origin && origin <= f.bullet.1 {
            let t = &buf.text[origin..];
            t[..t.len() - t.trim_start_matches([' ', '\t']).len()].to_string()
        } else {
            String::new()
        };
        buf.replace(f.bullet.0, f.bullet.1, "");
        buf.replace_before_markers(f.bullet.0, f.bullet.0, &new_bul);
        buf.insert_before_point(f.bullet.0 + new_bul.len(), &keep_space);
    }
    let Some(f) = full_item(&buf.text, item) else {
        return;
    };
    // b. The checkbox.
    let cur = f.checkbox.map(|(s, e)| buf.text[s..e].to_string());
    match (&cur, &new.checkbox) {
        (a, b) if a == b => {}
        (Some(_), Some(nb)) => {
            let Some((s, e)) = f.checkbox else { return };
            buf.replace(s, e, nb);
        }
        (Some(_), None) => {
            // `.*?\([ \t]*\[[ X-]\]\)`: the box and the blanks before it.
            let Some((s, e)) = f.checkbox else { return };
            let before = buf.text[..s].trim_end_matches([' ', '\t']).len().max(item);
            buf.replace(before, e, "");
        }
        (None, Some(nb)) => {
            let (at, text) = match f.counter {
                Some((_, ce)) => (ce + 1, nb.clone()),
                None => (f.bullet.1, format!("{nb} ")),
            };
            buf.insert_before_point(at, &text);
        }
        (None, None) => {}
    }
    // c. The indentation.
    let old_ind = indent(&buf.text, item) as isize;
    if new.ind != old_ind {
        // `delete-region` of the blanks, then `indent-to`.
        let n = buf.text[item..]
            .bytes()
            .take_while(|c| matches!(c, b' ' | b'\t'))
            .count();
        buf.delete(item, item + n);
        indent_to(buf, item, 0, new.ind.max(0) as usize);
    }
}

/// `org-list-write-struct`.
fn write_struct(
    buf: &mut Buf,
    st: &mut Struct,
    parents: &[(usize, Option<usize>)],
    old: Option<&Struct>,
    ctx: &ParseContext,
) {
    let old = old.cloned().unwrap_or_else(|| st.clone());
    fix_ind(st, parents, Some(2));
    fix_item_end(st);
    let pv = prevs(st);
    fix_bul(st, &pv, ctx);
    fix_ind(st, parents, None);
    fix_box(st, parents, false);
    apply_struct(buf, st, &old, ctx);
}

/// `org-list-swap-items`.
fn swap_items(buf: &mut Buf, beg_a: usize, beg_b: usize, st: &mut Struct) {
    let t = buf.text.clone();
    let end_a_nb = end_before_blank(&t, get(st, beg_a).end);
    let end_b_nb = end_before_blank(&t, get(st, beg_b).end);
    let end_a = get(st, beg_a).end;
    let end_b = get(st, beg_b).end;
    let size_a = end_a_nb - beg_a;
    let size_b = end_b_nb - beg_b;
    let body_a = &t[beg_a..end_a_nb];
    let body_b = &t[beg_b..end_b_nb];
    let between = &t[end_a_nb..beg_b];
    let mut sub_a = vec![beg_a];
    sub_a.extend(subtree(st, beg_a));
    let mut sub_b = vec![beg_b];
    sub_b.extend(subtree(st, beg_b));
    buf.delete(beg_a, end_b_nb);
    buf.insert_before_point(beg_a, &format!("{body_b}{between}{body_a}"));
    for e in st.iter_mut() {
        let pos = e.pos;
        if pos < beg_a {
            continue;
        }
        if sub_a.contains(&pos) {
            let end_e = e.end;
            e.pos = pos + end_b_nb - end_a_nb;
            e.end = end_e + end_b_nb - end_a_nb;
            if end_e == end_a {
                e.end = end_b;
            }
        } else if sub_b.contains(&pos) {
            let end_e = e.end;
            e.pos = pos + beg_a - beg_b;
            e.end = (end_e as isize + beg_a as isize - beg_b as isize) as usize;
            if end_e == end_b {
                e.end = beg_a + size_b + (end_a - end_a_nb);
            }
        } else if pos < beg_b {
            e.pos = (pos as isize + size_b as isize - size_a as isize) as usize;
            e.end = (e.end as isize + size_b as isize - size_a as isize) as usize;
        }
    }
    st.sort_by_key(|i| i.pos);
}

/// `org-at-item-p`: the start of the line at `point` when the parser has an
/// item there and it matches `org-item-re`.
fn at_item(
    root: &org_syntax::SyntaxNode,
    text: &str,
    point: usize,
    ctx: &ParseContext,
) -> Option<usize> {
    let b = bol(text, point);
    let parsed = root.descendants().any(|n| {
        n.kind() == org_syntax::SyntaxKind::ITEM && usize::from(n.text_range().start()) == b
    });
    (parsed && is_item(text, b, ctx)).then_some(b)
}

/// After the bullet (from `org-item-re`) and a numeric counter, as
/// `org-list-at-regexp-after-bullet-p` looks.
fn after_bullet(text: &str, b: usize, ctx: &ParseContext) -> usize {
    let l = line(text, b);
    let ws = l.len() - l.trim_start_matches([' ', '\t']).len();
    let r = &l[ws..];
    let blen = if r.starts_with(['-', '+', '*']) {
        1
    } else {
        let d = r.bytes().take_while(u8::is_ascii_digit).count();
        (if d > 0 { d } else { 1 }) + 1
    };
    let mut i = b + ws + blen;
    while matches!(text.as_bytes().get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    let rest = &text[i..eol(text, i)];
    if let Some(r) = rest.strip_prefix("[@") {
        let r = r.strip_prefix("start:").unwrap_or(r);
        let d = if ctx.list_allow_alphabetical && r.starts_with(|c: char| c.is_ascii_alphabetic()) {
            1
        } else {
            r.bytes().take_while(u8::is_ascii_digit).count()
        };
        if d > 0 && r.as_bytes().get(d) == Some(&b']') {
            let skipped = rest.len() - r.len() + d + 1;
            i += skipped;
            while matches!(text.as_bytes().get(i), Some(b' ' | b'\t')) {
                i += 1;
            }
        }
    }
    i
}

/// `org-at-item-checkbox-p`: the box, followed by a blank.
fn item_checkbox(text: &str, b: usize, ctx: &ParseContext) -> Option<String> {
    let i = after_bullet(text, b, ctx);
    let t = text.as_bytes();
    (t.get(i) == Some(&b'[')
        && matches!(t.get(i + 1), Some(b'-' | b' ' | b'X' | b'x'))
        && t.get(i + 2) == Some(&b']')
        && matches!(t.get(i + 3), Some(b' ' | b'\t')))
    .then(|| text[i..i + 3].to_string())
}

/// `org-at-item-description-p`: `\S-.+[ \t]+::\([ \t]+\|$\)` after the
/// bullet.
fn item_description(text: &str, b: usize, ctx: &ParseContext) -> bool {
    let i = after_bullet(text, b, ctx);
    let rest = &text[i..eol(text, i)];
    if rest.is_empty() || rest.starts_with([' ', '\t']) {
        return false;
    }
    rest.match_indices("::").any(|(k, _)| {
        let before = &rest[..k];
        let ws = before.len() - before.trim_end_matches([' ', '\t']).len();
        let after = &rest[k + 2..];
        ws > 0 && before.len() - ws >= 2 && (after.is_empty() || after.starts_with([' ', '\t']))
    })
}

fn setup(doc: &Document) -> (String, &ParseContext) {
    (doc.parse().syntax().to_string(), doc.parse().context())
}

/// `org-move-item-down` (`down`) or `org-move-item-up`.
pub fn move_item(doc: &Document, point: usize, down: bool) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc);
    let root = doc.parse().syntax();
    let Some(item) = at_item(&root, &text, point, ctx) else {
        return Err(EditError::new("Not at an item"));
    };
    let col = crate::buffer::column_at(&text, point) - crate::buffer::column_at(&text, item);
    let mut buf = Buf::new(&text, point);
    let mut st = list_struct(&text, item, ctx);
    let pv = prevs(&st);
    if down {
        let Some(next) = next_of(&pv, item) else {
            return Err(EditError::new("Cannot move this item further down"));
        };
        swap_items(&mut buf, item, next, &mut st);
        let pv = prevs(&st);
        buf.point = next_of(&pv, item).unwrap_or(item);
    } else {
        let Some(prev) = prev_of(&pv, item) else {
            return Err(EditError::new("Cannot move this item further up"));
        };
        swap_items(&mut buf, prev, item, &mut st);
        buf.point = prev;
    }
    let pa = parents(&st);
    write_struct(&mut buf, &mut st, &pa, None, ctx);
    let b = bol(&buf.text, buf.point);
    buf.point = crate::buffer::move_to_column(&buf.text, b, col);
    Ok(buf.transaction(if down {
        "Move item down"
    } else {
        "Move item up"
    }))
}

/// `org-list-repair`.
pub fn repair(doc: &Document, point: usize) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc);
    let root = doc.parse().syntax();
    let Some(item) = at_item(&root, &text, point, ctx) else {
        return Err(EditError::new("This is not a list"));
    };
    let mut buf = Buf::new(&text, point);
    let mut st = list_struct(&text, item, ctx);
    let pa = parents(&st);
    write_struct(&mut buf, &mut st, &pa, None, ctx);
    Ok(buf.transaction("Repair list"))
}

/// A bullet for [`cycle_bullet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulletChoice {
    /// The next bullet of the cycle `-`, `+`, `*`, `1.`, `1)`.
    Next,
    /// The previous one.
    Previous,
    /// This bullet (`-`, `1.`, …).
    Bullet(String),
    /// The Nth bullet of the cycle.
    Index(usize),
}

/// `org-cycle-list-bullet`: changes the bullet of every item of the
/// sub-list at point.
pub fn cycle_bullet(
    doc: &Document,
    point: usize,
    which: BulletChoice,
) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc);
    let root = doc.parse().syntax();
    let Some(item) = at_item(&root, &text, point, ctx) else {
        return Err(EditError::new("Not at an item"));
    };
    let mut buf = Buf::new(&text, point);
    let mut st = list_struct(&text, item, ctx);
    let pa = parents(&st);
    let pv = prevs(&st);
    let list_beg = first_item(&pv, item);
    let it = get(&st, item).clone();
    let origin_offset = point as isize - (item as isize + it.ind);
    let origin_offset2 = point as isize - (item as isize + it.ind + it.bullet.len() as isize);
    let bullet = get(&st, list_beg).bullet.clone();
    let alpha_p = use_alpha_bul(&st, &pv, list_beg, ctx);
    let current = if bullet.contains('.') && bullet.chars().any(|c| c.is_ascii_lowercase()) {
        "a.".to_string()
    } else if bullet.contains(')') && bullet.chars().any(|c| c.is_ascii_lowercase()) {
        "a)".to_string()
    } else if bullet.contains('.') && bullet.chars().any(|c| c.is_ascii_uppercase()) {
        "A.".to_string()
    } else if bullet.contains(')') && bullet.chars().any(|c| c.is_ascii_uppercase()) {
        "A)".to_string()
    } else if bullet.contains('.') {
        "1.".to_string()
    } else if bullet.contains(')') {
        "1)".to_string()
    } else {
        bullet.trim().to_string()
    };
    let description = item_description(&text, item, ctx);
    let mut list: Vec<&str> = vec!["-", "+"];
    // `*` bullets are not allowed at column 0: `(looking-at "\\S-")` at the
    // start of the line.
    if text[item..].starts_with([' ', '\t']) {
        list.push("*");
    }
    if !description && ctx.item_terminator != ItemTerminator::Paren {
        list.push("1.");
    }
    if !description && ctx.item_terminator != ItemTerminator::Dot {
        list.push("1)");
    }
    if alpha_p && !description && ctx.item_terminator != ItemTerminator::Paren {
        list.extend(["a.", "A."]);
    }
    if alpha_p && !description && ctx.item_terminator != ItemTerminator::Dot {
        list.extend(["a)", "A)"]);
    }
    let len = list.len() as isize;
    let idx = list
        .iter()
        .position(|b| *b == current)
        .map_or(len, |i| i as isize);
    let get_v = |i: isize| list[i.rem_euclid(len) as usize].to_string();
    let new = match which {
        BulletChoice::Bullet(b) if list.contains(&b.as_str()) => b,
        BulletChoice::Index(n) => get_v(n as isize),
        BulletChoice::Previous => get_v(idx - 1),
        _ => get_v(idx + 1),
    };
    let old = st.clone();
    get_mut(&mut st, list_beg).bullet = bullet_string(&new);
    fix_bul(&mut st, &pv, ctx);
    fix_ind(&mut st, &pa, None);
    // The structure is applied from the start of the line; the cursor
    // follows as a marker.
    let origin_m = buf.add_marker(point);
    buf.point = item;
    apply_struct(&mut buf, &st, &old, ctx);
    buf.point = buf.marker(origin_m);
    // Keep point at the same place relative to the bullet.
    let origin = buf.point;
    let b = bol(&buf.text, origin);
    if is_item(&buf.text, b, ctx) {
        let st2 = list_struct(&buf.text, b, ctx);
        let it2 = get(&st2, b);
        if origin_offset2 >= 0 {
            buf.point =
                (b as isize + it2.ind + it2.bullet.len() as isize + origin_offset2) as usize;
        } else if origin_offset >= 0 {
            buf.point = (b as isize + it2.ind + origin_offset) as usize;
        }
    }
    Ok(buf.transaction("Cycle bullet"))
}

/// How [`toggle_checkbox`] changes boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckboxAction {
    /// Check an unchecked box and uncheck a checked one.
    Toggle,
    /// `C-u`: add a box where there is none, remove it where there is.
    Presence,
    /// `C-u C-u`: set the box to `[-]`.
    Partial,
}

/// `org-toggle-checkbox`: on an item, on a headline (the items of its
/// section), or on the items of the region `mark`..`point`; then the
/// section's statistics cookies are updated.
pub fn toggle_checkbox(
    doc: &Document,
    point: usize,
    mark: Option<usize>,
    action: CheckboxAction,
) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc);
    let root = doc.parse().syntax();
    let mut buf = Buf::new(&text, point);
    let entry = doc.outline().entry_at(point);
    let ordered = doc
        .entry_get(entry, "ORDERED", org_model::Inherit::No, false)
        .is_some();
    let b = bol(&text, point);
    let region = mark.filter(|&m| m != point);
    let limited = headings(&text, ctx.inlinetask_min_level);
    let (lim_up, lim_down, single) = if let Some(m) = region {
        let (rs, re) = (point.min(m), point.max(m));
        match search_item(&text, rs, re, ctx) {
            Some(i) => (i, re, false),
            None => return Err(EditError::new("No item in region")),
        }
    } else if crate::headline::stars_at(&text, b).is_some() {
        let limit = headings(&text, None)
            .into_iter()
            .find(|(s, _)| *s > b)
            .map_or(text.len(), |(s, _)| s);
        let from = end_of_meta_data_full(&text, b);
        match search_item(&text, from, limit, ctx) {
            Some(i) => (i, limit, false),
            None => return Err(EditError::new("No item in subtree")),
        }
    } else if at_item(&root, &text, point, ctx).is_some() {
        (b, eol(&text, b), true)
    } else {
        return Err(EditError::new(
            "Not at an item or heading, and no active region",
        ));
    };
    let _ = &limited;
    let cbox = item_checkbox(&text, lim_up, ctx);
    let reference: Option<String> = match action {
        CheckboxAction::Partial => Some("[-]".into()),
        CheckboxAction::Presence => {
            if cbox.is_none() {
                Some("[ ]".into())
            } else {
                None
            }
        }
        CheckboxAction::Toggle => Some(if cbox.as_deref() == Some("[X]") {
            "[ ]".into()
        } else {
            "[X]".into()
        }),
    };
    let lim_down_m = buf.add_marker(lim_down);
    let mut p = lim_up;
    loop {
        let lim_down = buf.marker(lim_down_m);
        if p >= lim_down {
            break;
        }
        let Some(item) = search_item(&buf.text, p, lim_down, ctx) else {
            break;
        };
        let mut st = list_struct(&buf.text, item, ctx);
        let old = st.clone();
        let pa = parents(&st);
        let bottom = st.iter().map(|i| i.end).max().unwrap_or(item);
        let bottom_m = buf.add_marker(bottom);
        let to_toggle: Vec<usize> = st
            .iter()
            .map(|i| i.pos)
            .filter(|&e| e >= lim_up && e <= lim_down)
            .collect();
        for e in to_toggle {
            let cur = get(&st, e).checkbox.clone();
            get_mut(&mut st, e).checkbox = if cur.is_some() || action == CheckboxAction::Presence {
                reference.clone()
            } else {
                cur
            };
        }
        let block = fix_box(&mut st, &pa, ordered);
        if single
            && let Some(bi) = block
            && lim_up > bi
        {
            let line_no = buf.text[..bi].matches('\n').count() + 1;
            return Err(EditError::new(&format!(
                "Checkbox blocked because of unchecked box at line {line_no}"
            )));
        }
        apply_struct(&mut buf, &st, &old, ctx);
        p = buf.marker(bottom_m);
    }
    update_checkbox_count(&mut buf, ctx, doc.settings());
    Ok(buf.transaction("Toggle checkbox"))
}

/// `org-end-of-meta-data` with FULL: after planning, drawers and clocks.
fn end_of_meta_data_full(text: &str, h: usize) -> usize {
    let mut p = next_line(text, h);
    if p < text.len() && crate::property::is_planning_line(text, p) {
        p = next_line(text, p);
    }
    if let Some(e) = crate::property::property_drawer_at(text, p) {
        p = next_line(text, e);
    }
    let end = headings(text, None)
        .into_iter()
        .find(|(s, _)| *s > h)
        .map_or(text.len(), |(s, _)| s);
    if p < text.len() && crate::headline::stars_at(text, p).is_none() {
        while p < text.len() {
            let l = line(text, p);
            let t = l.trim_start_matches([' ', '\t']);
            if blank_line(text, p) || t.starts_with("CLOCK:") {
                p = next_line(text, p);
            } else if is_drawer_begin(l) {
                let mut x = p;
                let mut found = false;
                loop {
                    x = next_line(text, x);
                    if x >= end || x >= text.len() {
                        break;
                    }
                    if line(text, x).trim().eq_ignore_ascii_case(":END:") {
                        found = true;
                        break;
                    }
                }
                if !found {
                    break;
                }
                p = next_line(text, x);
            } else {
                break;
            }
        }
    }
    p
}

/// `org-list-search-forward` of `org-item-beginning-re`: the first item
/// line in `from..to` in a valid list context.
fn search_item(text: &str, from: usize, to: usize, ctx: &ParseContext) -> Option<usize> {
    let mut p = bol(text, from);
    if p < from {
        p = next_line(text, p);
    }
    while p < to && p < text.len() {
        if is_item(text, p, ctx) && list_context(text, p, ctx).2 != Context::Invalid {
            return Some(p);
        }
        p = next_line(text, p);
    }
    None
}

/// `org-update-checkbox-count` for the section at point.
fn update_checkbox_count(buf: &mut Buf, ctx: &ParseContext, settings: &org_model::Settings) {
    let text = buf.text.clone();
    let point = buf.point;
    let parse = org_syntax::parse_with(&text, ctx);
    let doc = Document::with_settings(parse, std::sync::Arc::new(settings.clone()), None);
    let b = bol(&text, point);
    let in_task = inlinetask_bounds(&text, b, ctx);
    let (from, to) = match in_task {
        Some((s, e)) => (s, e),
        None => {
            let limited = headings(&text, ctx.inlinetask_min_level);
            // `outline-previous-heading`: a heading ending before point.
            let from = limited
                .iter()
                .rev()
                .find(|(s, n)| s + n < point)
                .map_or(0, |(s, _)| *s);
            let to = limited
                .iter()
                .find(|(s, _)| *s > b)
                .map_or(text.len(), |(s, _)| *s);
            (from, to)
        }
    };
    let entry = doc.outline().entry_at(point);
    let data = doc
        .entry_get(entry, "COOKIE_DATA", org_model::Inherit::No, false)
        .unwrap_or_default();
    let cookies = doc.checkbox_cookies(from, to, &data);
    for (s, e, new) in cookies.into_iter().rev() {
        buf.insert_before_point(s, &new);
        buf.delete(s + new.len(), e + new.len());
        let lb = bol(&buf.text, s);
        if crate::headline::stars_at(&buf.text, lb).is_some() {
            align_tags(buf, lb);
        }
    }
}

/// The inlinetask around the line `b`: (its start, its END line).
fn inlinetask_bounds(text: &str, b: usize, ctx: &ParseContext) -> Option<(usize, usize)> {
    ctx.inlinetask_min_level?;
    let start = lines_back(text, b, 0).find(|&x| inlinetask_line(text, x, ctx).is_some())?;
    if inlinetask_line(text, start, ctx) != Some(false) {
        return None;
    }
    let mut x = start;
    loop {
        x = next_line(text, x);
        if x >= text.len() {
            return None;
        }
        if let Some(end) = inlinetask_line(text, x, ctx) {
            return (end && x >= b).then_some((start, x));
        }
        if crate::headline::stars_at(text, x).is_some() {
            return None;
        }
    }
}

/// Indent (`indent` true) or outdent the item at point, with its children
/// unless `no_subtree` (`org-indent-item`, `org-indent-item-tree`,
/// `org-outdent-item`, `org-outdent-item-tree`); with a region, every item
/// in it.
pub fn indent_item(
    doc: &Document,
    point: usize,
    mark: Option<usize>,
    indent: bool,
    no_subtree: bool,
) -> Result<Transaction, EditError> {
    let (text, ctx) = setup(doc);
    let root = doc.parse().syntax();
    let region = mark
        .filter(|&m| m != point)
        .map(|m| (point.min(m), point.max(m)));
    let at = match region {
        Some((rs, _)) => at_item(&root, &text, rs, ctx)
            .ok_or_else(|| EditError::new("Region not starting at an item"))?,
        None => {
            at_item(&root, &text, point, ctx).ok_or_else(|| EditError::new("Not at an item"))?
        }
    };
    let mut buf = Buf::new(&text, point);
    let mut st = list_struct(&text, at, ctx);
    let top = st[0].pos;
    let pa = parents(&st);
    let pv = prevs(&st);
    let b = bol(&text, point);
    let special = region.is_none() && top == b;
    if special && no_subtree {
        return Err(EditError::new(
            "At first item: use S-M-<left/right> to move the whole list",
        ));
    }
    let (beg, end) = match region {
        Some(r) => r,
        None if special => (b, st.iter().map(|i| i.end).max().unwrap_or(b)),
        None if no_subtree => (b, b + 1),
        None => (b, get(&st, b).end),
    };
    if special {
        let skip = if ctx.odd_levels_only { 2 } else { 1 };
        let offset: isize = if indent { skip } else { -skip };
        let top_ind = get(&st, beg).ind;
        if top_ind + offset < 0 {
            return Err(EditError::new("Cannot outdent beyond margin"));
        }
        let old = st.clone();
        if top_ind + offset == 0 && get(&st, beg).bullet.contains('*') {
            get_mut(&mut st, beg).bullet = bullet_string("-");
        }
        for it in st.iter_mut() {
            it.ind += offset;
        }
        fix_bul(&mut st, &pv, ctx);
        apply_struct(&mut buf, &st, &old, ctx);
        return Ok(buf.transaction("Indent list"));
    }
    if !indent {
        let blocked = if no_subtree && region.is_none() {
            has_child(&st, beg).is_some()
        } else {
            st.iter()
                .rev()
                .find(|e| e.pos < end)
                .is_some_and(|last| has_child(&st, last.pos).is_some())
        };
        if blocked {
            return Err(EditError::new(
                "Cannot outdent an item without its children",
            ));
        }
    }
    let old = st.clone();
    let new_parents = if indent {
        struct_indent(beg, end, &st, &pa, &pv)?
    } else {
        struct_outdent(beg, end, &st, &pa)?
    };
    write_struct(&mut buf, &mut st, &new_parents, Some(&old), ctx);
    update_checkbox_count(&mut buf, ctx, doc.settings());
    Ok(buf.transaction(if indent {
        "Indent item"
    } else {
        "Outdent item"
    }))
}

/// `org-list-struct-outdent`.
fn struct_outdent(
    start: usize,
    end: usize,
    st: &Struct,
    parents: &[(usize, Option<usize>)],
) -> Result<Vec<(usize, Option<usize>)>, EditError> {
    let mut acc: Vec<(usize, usize)> = Vec::new();
    let mut out = Vec::new();
    for &(item, parent) in parents {
        let cell = if item < start {
            (item, parent)
        } else if item >= end {
            match parent.and_then(|p| acc.iter().find(|(a, _)| *a == p)) {
                Some(&(_, conv)) => (item, Some(conv)),
                None => (item, parent),
            }
        } else {
            let Some(p) = parent else {
                return Err(EditError::new("Cannot outdent top-level items"));
            };
            if p >= start {
                acc.insert(0, (p, item));
                (item, parent)
            } else {
                let grand = parent_of(parents, p);
                acc.insert(0, (p, item));
                (item, grand)
            }
        };
        out.push(cell);
    }
    let _ = st;
    Ok(out)
}

/// `org-list-struct-indent`.
fn struct_indent(
    start: usize,
    end: usize,
    st: &Struct,
    parents: &[(usize, Option<usize>)],
    prevs: &[(usize, Option<usize>)],
) -> Result<Vec<(usize, Option<usize>)>, EditError> {
    let mut acc: Vec<(usize, Option<usize>)> = Vec::new();
    let mut out = Vec::new();
    for &(item, parent) in parents {
        let cell = if item < start {
            (item, parent)
        } else if item >= end {
            match acc.iter().find(|(a, _)| Some(*a) == parent) {
                Some(&(_, conv)) => (item, conv),
                None => (item, parent),
            }
        } else {
            let prev = prev_of(prevs, item);
            match prev {
                None if parent.is_none_or(|p| p < start) => {
                    return Err(EditError::new("Cannot indent the first item of a list"));
                }
                None => {
                    acc.insert(0, (item, parent));
                    (item, parent)
                }
                Some(pr) if pr < start => {
                    acc.insert(0, (item, Some(pr)));
                    (item, Some(pr))
                }
                Some(pr) => {
                    let conv = acc.iter().find(|(a, _)| *a == pr).and_then(|(_, c)| *c);
                    acc.insert(0, (item, conv));
                    (item, conv)
                }
            }
        };
        out.push(cell);
    }
    let _ = st;
    Ok(out)
}

/// `org-insert-item` at `point`, with a checkbox if `checkbox`. Returns
/// `None` when point is not in a list.
pub fn insert_item(doc: &Document, point: usize, checkbox: bool) -> Option<Transaction> {
    let (text, ctx) = setup(doc);
    let itemp = in_item(&text, point, ctx)?;
    let mut buf = Buf::new(&text, point);
    let mut st = list_struct(&text, itemp, ctx);
    let pv = prevs(&st);
    let desc = (list_type(&st, &pv, itemp) == "descriptive").then_some(" :: ");
    insert_item_struct(&mut buf, point, &mut st, &pv, checkbox, desc.unwrap_or(""));
    let pa = parents(&st);
    write_struct(&mut buf, &mut st, &pa, None, ctx);
    if checkbox {
        update_checkbox_count(&mut buf, ctx, doc.settings());
    }
    let b = bol(&buf.text, buf.point);
    if let Some(f) = full_item(&buf.text, b) {
        let ordered = buf.text[f.bullet.0..f.bullet.1].contains(['.', ')']);
        buf.point = match f.tag {
            Some((ts, _)) if ordered => ts,
            _ => f.end,
        };
        if desc.is_some() {
            buf.point -= 1;
        }
    }
    Some(buf.transaction("Insert item"))
}

fn list_type(st: &Struct, prevs: &[(usize, Option<usize>)], item: usize) -> &'static str {
    let first = get(st, first_item(prevs, item));
    if first.bullet.chars().any(char::is_alphanumeric) {
        "ordered"
    } else if first.tag.is_some() {
        "descriptive"
    } else {
        "unordered"
    }
}

/// The start of the line of the item `pos` is in (`org-beginning-of-item`),
/// with whether that item has a checkbox; none outside lists.
pub fn item_at(doc: &Document, pos: usize) -> Option<(usize, bool)> {
    let (text, ctx) = setup(doc);
    let b = in_item(&text, pos, ctx)?;
    let checkbox = full_item(&text, b).is_some_and(|f| f.checkbox.is_some());
    Some((b, checkbox))
}

/// `org-in-item-p`: the item containing `pos`.
fn in_item(text: &str, pos: usize, ctx: &ParseContext) -> Option<usize> {
    let b = bol(text, pos);
    let (lim_up, _, c) = list_context(text, b, ctx);
    let mut ind_ref = if blank_line(text, b) || inlinetask_line(text, b, ctx).is_some() {
        10000
    } else {
        indent(text, b)
    };
    if c == Context::Invalid {
        return None;
    }
    if is_item(text, b, ctx) {
        return Some(b);
    }
    let list_end_at = |p: usize| {
        blank_line(text, p) && eol(text, p) < text.len() && {
            let n = next_line(text, p);
            n < text.len() && blank_line(text, n) && eol(text, n) < text.len()
        }
    };
    let mut p = b;
    // Inside two blank lines ending a list: start above them.
    for start in [prev_line(text, b), b] {
        if list_end_at(start) && start <= pos && pos < next_line(text, next_line(text, start)) {
            p = prev_line(text, start);
            break;
        }
    }
    loop {
        let ind = indent(text, p);
        if is_item(text, p, ctx) && ind < ind_ref {
            return Some(p);
        }
        if p <= lim_up || list_end_at(p) {
            return None;
        }
        let l = line(text, p);
        let t = l.trim_start_matches([' ', '\t']);
        if t.len() >= 6
            && t.as_bytes()[..6].eq_ignore_ascii_case(b"#+end_")
            && let Some(x) = lines_back(text, p, lim_up).skip(1).find(|&x| {
                let t = line(text, x).trim_start_matches([' ', '\t']);
                t.len() >= 8 && t.as_bytes()[..8].eq_ignore_ascii_case(b"#+begin_")
            })
        {
            p = x;
            continue;
        }
        if is_drawer_end(l)
            && let Some(x) = lines_back(text, p, lim_up)
                .skip(1)
                .find(|&x| is_drawer_begin(line(text, x)))
        {
            p = x;
            continue;
        }
        if inlinetask_line(text, p, ctx).is_some() {
            let b0 = lines_back(text, p, 0)
                .find(|&x| inlinetask_line(text, x, ctx) == Some(false))
                .unwrap_or(p);
            p = prev_line(text, b0);
            continue;
        }
        if !blank_line(text, p) {
            if ind == 0 {
                return None;
            }
            if ind < ind_ref {
                ind_ref = ind;
            }
        }
        if p == 0 {
            return None;
        }
        p = prev_line(text, p);
    }
}

/// `org-list-separating-blank-lines-number` with
/// `org-blank-before-new-entry` set to `auto` for items.
fn separating_blank_lines(
    text: &str,
    pos: usize,
    item: usize,
    st: &Struct,
    prevs: &[(usize, Option<usize>)],
) -> usize {
    let count_blanks = |p: usize| -> usize {
        let b = bol(text, p);
        let q = text[..b].trim_end_matches([' ', '\r', '\t', '\n']).len();
        let after = next_line(text, q);
        let after = if q == 0 && b > 0 && after > b {
            0
        } else {
            after
        };
        text[after.min(b)..b].matches('\n').count()
    };
    if let Some(n) = next_of(prevs, item) {
        return count_blanks(n);
    }
    if prev_of(prevs, item).is_some() {
        return count_blanks(item);
    }
    let end_nb = end_before_blank(text, get(st, item).end);
    if pos > end_nb {
        let usr = count_blanks(pos);
        if usr > 0 {
            return usr;
        }
    }
    let top = st[0].pos;
    let mut p = top;
    while p < end_nb {
        if blank_line(text, p) {
            return 1;
        }
        p = next_line(text, p);
        if p >= text.len() {
            break;
        }
    }
    0
}

/// `org-list-insert-item`.
#[expect(
    clippy::expect_used,
    reason = "items are read from the list's own structure, which holds them"
)]
fn insert_item_struct(
    buf: &mut Buf,
    pos: usize,
    st: &mut Struct,
    prevs: &[(usize, Option<usize>)],
    checkbox: bool,
    after_bullet: &str,
) {
    let text = buf.text.clone();
    let mut pos = pos;
    let item = st
        .iter()
        .try_fold(None, |acc, it| {
            if it.pos > pos {
                Err(acc)
            } else if it.end < pos {
                Ok(acc)
            } else {
                Ok(Some(it.pos))
            }
        })
        .unwrap_or_else(|e| e)
        .unwrap_or_else(|| st.last().expect("item").pos);
    let item_end = get(st, item).end;
    let item_end_nb = end_before_blank(&text, item_end);
    let f = full_item(&text, item).expect("item");
    let limit = match f.tag {
        None => f.end,
        Some((ts, _)) if text[f.bullet.0..f.bullet.1].contains(['.', ')']) => ts,
        Some((_, te)) => {
            let mut x = te;
            while matches!(text.as_bytes().get(x), Some(b' ' | b'\t')) {
                x += 1;
            }
            x
        }
    };
    let beforep = pos <= limit;
    let blank_nb = separating_blank_lines(&text, pos, item, st, prevs);
    let ind = get(st, item).ind.max(0) as usize;
    let ind_size = ind / 8 + ind % 8;
    let bullet = bullet_string(&get(st, item).bullet);
    let boxs = checkbox.then_some("[ ]");
    let mut text_cut: Option<String> = None;
    if !beforep {
        if item_end < pos {
            let e = eol(&buf.text, pos);
            buf.delete(item_end - 1, e);
            pos = item_end - 1;
        }
        let q = buf.text[..pos]
            .trim_end_matches([' ', '\r', '\t', '\n'])
            .len();
        let mut cut_start = q;
        while matches!(buf.text.as_bytes().get(cut_start), Some(b' ' | b'\t')) {
            cut_start += 1;
        }
        pos = cut_start;
        let from = q;
        let to = item_end_nb.max(from);
        let cut = buf.text[from..to].to_string();
        buf.delete(from, to);
        text_cut = Some(cut);
    }
    let cut_body = text_cut
        .as_deref()
        .map(|c| c.trim_start_matches([' ', '\t']).to_string());
    let body = format!(
        "{bullet}{}{after_bullet}{}",
        boxs.map(|b| format!("{b} ")).unwrap_or_default(),
        cut_body.unwrap_or_default()
    );
    let item_sep = "\n".repeat(1 + blank_nb);
    let item_size = ind_size + body.len() + item_sep.len();
    let size_offset = item_size as isize - text_cut.as_ref().map_or(0, String::len) as isize;
    buf.insert_before_point(item, &format!("{}{body}{item_sep}", indentation(ind)));
    let shift = |x: usize, d: isize| (x as isize + d) as usize;
    for e in st.iter_mut() {
        let (p, end) = (e.pos, e.end);
        if p < item {
            if end > item {
                e.end = shift(end, size_offset);
            }
        } else if p == item && !beforep {
            e.pos = p + item_size;
            e.end = shift(end, size_offset);
        } else if !beforep && p >= pos && p <= item_end_nb {
            let offset = pos as isize
                - item as isize
                - ind as isize
                - bullet.len() as isize
                - after_bullet.len() as isize;
            e.pos = shift(p, -offset);
            e.end = shift(end, -offset);
        } else {
            e.pos = shift(p, size_offset);
            e.end = shift(end, size_offset);
        }
    }
    st.push(Item {
        pos: item,
        ind: ind as isize,
        bullet: bullet.clone(),
        counter: None,
        checkbox: boxs.map(str::to_string),
        tag: None,
        end: item + item_size,
    });
    st.sort_by_key(|i| i.pos);
    if beforep {
        buf.point = item;
    } else {
        swap_items(buf, item, item + item_size, st);
        let pv = self::prevs(st);
        buf.point = next_of(&pv, item).unwrap_or(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(text: &str, at: usize) -> Struct {
        let p = org_syntax::parse(text);
        list_struct(text, at, p.context())
    }

    #[test]
    fn structure_like_the_docstring() {
        let t = "- [X] first item\n  1. sub-item 1\n  5. [@5] sub-item 2\n  some other text belonging to first item\n- last item\n  + tag :: description\n\n";
        let s = st(t, 0);
        type Row<'a> = (
            usize,
            isize,
            &'a str,
            Option<&'a str>,
            Option<&'a str>,
            usize,
        );
        let v: Vec<Row<'_>> = s
            .iter()
            .map(|i| {
                (
                    i.pos,
                    i.ind,
                    i.bullet.as_str(),
                    i.counter.as_deref(),
                    i.checkbox.as_deref(),
                    i.end,
                )
            })
            .collect();
        assert_eq!(
            v,
            vec![
                (0, 0, "- ", None, Some("[X]"), 96),
                (17, 2, "1. ", None, None, 33),
                (33, 2, "5. ", Some("5"), None, 54),
                // The docstring's example says 131 in 1-based positions,
                // but its last line starts at 132.
                (96, 0, "- ", None, None, 131),
                (108, 2, "+ ", None, None, 131),
            ]
        );
        let p = parents(&s);
        assert_eq!(p[1], (17, Some(0)));
        assert_eq!(p[4], (108, Some(96)));
    }
}
