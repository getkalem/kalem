//! Plain list structure, following `org-element--list-struct` and the
//! regexps of `org-list.el`.

use crate::buf::Buf;
use crate::context::{ItemTerminator, ParseContext};
use crate::elements::Parser;
use crate::re::{self, Lazy};

/// One item of a list structure: `(pos ind bullet counter checkbox tag end)`.
#[derive(Debug, Clone)]
pub(crate) struct StructItem {
    pub(crate) pos: usize,
    pub(crate) ind: usize,
    pub(crate) tag: Option<(usize, usize)>,
    pub(crate) end: usize,
}

/// The structure of a plain list and all its sub-lists, sorted by
/// position.
#[derive(Debug, Clone, Default)]
pub(crate) struct ListStruct {
    pub(crate) items: Vec<StructItem>,
}

impl ListStruct {
    /// `assq`: the item starting at `pos`.
    pub(crate) fn get(&self, pos: usize) -> Option<&StructItem> {
        self.items
            .binary_search_by_key(&pos, |i| i.pos)
            .ok()
            .map(|i| &self.items[i])
    }
}

/// `org-list-full-item-re` (letters always allowed), matched with
/// `case-fold-search` on.
static FULL_ITEM: Lazy = Lazy::new(
    r"(?mi)((?:[-+*]|(?:[0-9]+|[A-Za-z])[.)])(?:[ \t]+|$))(?:\[@(?:start:)?([0-9]+|[A-Za-z])\][ \t]*)?(?:(\[[ X-]\])(?:[ \t]+|$))?(?:(.*)[ \t]+::(?:[ \t]+|$))?",
);

static BLOCK_BEGIN: Lazy = Lazy::new(r"(?i)#\+BEGIN(:|_[^{S}]+)");
static DRAWER_LINE: Lazy = Lazy::new(r"(?mi):[-_{W}]+:[ \t]*$");

pub(crate) fn full_item_match(p: &Parser<'_>, pos: usize) -> Option<re::Captures> {
    // The regex omits the leading `^[ \t]*`, see `Parser::at_indented`.
    re::looking_at_line(
        FULL_ITEM.get(),
        p.buf.s,
        p.buf.begv,
        p.buf.zv,
        p.buf.skip_blank(pos),
    )
}

/// `(looking-at-p (org-item-re))` at `p`: returns the match end.
pub(crate) fn item_re_match(b: &Buf<'_>, ctx: &ParseContext, p: usize) -> Option<usize> {
    let q = b.skip_blank(p);
    let bullet_end = match b.byte(q)? {
        b'-' | b'+' => q + 1,
        b'*' if q > p => q + 1,
        c if c.is_ascii_digit() || (ctx.list_allow_alphabetical && c.is_ascii_alphabetic()) => {
            let e = if c.is_ascii_digit() {
                b.skip_fwd(q, b"0123456789", b.zv)
            } else {
                q + 1
            };
            let ok = matches!(
                (b.byte(e), ctx.item_terminator),
                (Some(b'.'), ItemTerminator::Both | ItemTerminator::Dot)
                    | (Some(b')'), ItemTerminator::Both | ItemTerminator::Paren)
            );
            if !ok {
                return None;
            }
            e + 1
        }
        _ => return None,
    };
    match b.byte(bullet_end) {
        None | Some(b'\n') => Some(bullet_end),
        Some(b' ' | b'\t') => Some(b.skip_blank(bullet_end)),
        _ => None,
    }
}

/// `org-current-text-column` after skipping indentation: `string-width`
/// counts every tab as 8 columns.
fn indentation(b: &Buf<'_>, p: usize) -> usize {
    let mut w = 0;
    let mut q = p;
    while let Some(c) = b.byte(q) {
        match c {
            b' ' => w += 1,
            b'\t' => w += 8,
            _ => break,
        }
        q += 1;
    }
    w
}

/// `org-list-end-re`: `^[ \t]*\n[ \t]*\n` at `p`.
fn at_list_end(b: &Buf<'_>, p: usize) -> bool {
    let e1 = b.eol(p);
    if !b.rest_is_blank(p) || e1 >= b.zv {
        return false;
    }
    let e2 = b.eol(e1 + 1);
    b.rest_is_blank(e1 + 1) && e2 < b.zv
}

/// `org-element--list-struct`.
pub(crate) fn list_struct(parser: &Parser<'_>, start: usize, limit: usize) -> ListStruct {
    let b = &parser.buf;
    let mut items: Vec<StructItem> = Vec::new();
    let mut done: Vec<StructItem> = Vec::new();
    let mut p = start;
    let finish = |mut done: Vec<StructItem>, items: Vec<StructItem>| {
        done.extend(items);
        done.sort_by_key(|i| i.pos);
        ListStruct { items: done }
    };
    loop {
        if p >= limit {
            let end = b.lbp2(b.skip_bwd(p, b" \r\t\n", 0));
            for it in items.iter_mut() {
                it.end = end;
            }
            return finish(done, items);
        }
        if at_list_end(b, p) {
            for it in items.iter_mut() {
                it.end = p;
            }
            return finish(done, items);
        }
        if item_re_match(b, parser.ctx, p).is_some() {
            let ind = indentation(b, p);
            while items.last().is_some_and(|last| ind <= last.ind) {
                let mut it = items.pop().expect("non-empty");
                it.end = p;
                done.push(it);
            }
            let m = full_item_match(parser, p);
            let tag = m.as_ref().and_then(|m| {
                let (bs, be) = m.get(1)?;
                let bullet = b.slice(bs, be);
                if bullet.contains(['-', '+', '*']) {
                    m.get(4)
                } else {
                    None
                }
            });
            items.push(StructItem {
                pos: p,
                ind,
                tag,
                end: 0,
            });
            p = b.next_line(p);
            continue;
        }
        if b.rest_is_blank(p) {
            p = b.next_line(p);
            continue;
        }
        // A text line: does it end previous items?
        let ind = indentation(b, p);
        let end = b.lbp2(b.skip_bwd(p, b" \r\t\n", 0));
        while items.last().is_some_and(|last| ind <= last.ind) {
            let mut it = items.pop().expect("non-empty");
            it.end = end;
            done.push(it);
            if items.is_empty() {
                done.sort_by_key(|i| i.pos);
                return ListStruct { items: done };
            }
        }
        // Skip blocks and drawers.
        let mut q = p;
        if let Some(m) = re::looking_at_line(BLOCK_BEGIN.get(), b.s, b.begv, b.zv, b.skip_blank(p))
        {
            let (s, e) = m.get(1).expect("group");
            let marker = format!("#+END{}", b.slice(s, e));
            let mut ls = p;
            while ls < limit && b.eol(ls) <= limit {
                let r = b.skip_blank(ls);
                if b.looking_at_ci(r, &marker) && b.rest_is_blank(r + marker.len()) {
                    q = b.eol(ls);
                    break;
                }
                if b.eol(ls) >= b.zv {
                    break;
                }
                ls = b.eol(ls) + 1;
            }
        } else if re::looking_at_line_p(DRAWER_LINE.get(), b.s, b.begv, b.zv, b.skip_blank(p)) {
            let mut ls = b.next_line(p);
            // The search starts at the drawer line itself, which is not an
            // :END: line unless the drawer is named END.
            ls = if b.is_bol(p) { p } else { ls };
            while ls < limit && b.eol(ls) <= limit {
                let r = b.skip_blank(ls);
                if b.looking_at_ci(r, ":END:") && b.rest_is_blank(r + 5) {
                    q = b.eol(ls);
                    break;
                }
                if b.eol(ls) >= b.zv {
                    break;
                }
                ls = b.eol(ls) + 1;
            }
        }
        p = b.next_line(q);
    }
}

/// An item of a plain list structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListItem {
    /// The start of the item's line.
    pub pos: usize,
    /// Its indentation.
    pub ind: usize,
    /// Where it ends: the next item at its level or above, or the end of
    /// the list before blank lines.
    pub end: usize,
}

/// The structure org-element gives the plain list starting at `start`
/// (`org-element--list-struct`, the list's `:structure`): the items met
/// scanning forward from there up to `limit`. It can reach past the list
/// itself: later items that are less indented than the first one belong to
/// it, while the parser makes them a list of their own.
pub fn list_structure(text: &str, ctx: &ParseContext, start: usize, limit: usize) -> Vec<ListItem> {
    let parser = Parser::elements_only(text, ctx);
    list_struct(&parser, start, limit.min(text.len()))
        .items
        .iter()
        .map(|i| ListItem {
            pos: i.pos,
            ind: i.ind,
            end: i.end,
        })
        .collect()
}
