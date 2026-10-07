//! Moving by structure: `org-forward-heading-same-level`,
//! `org-forward-element`, `org-backward-element`, `org-up-element` and
//! `org-down-element`, which evil-org and Doom Emacs give Vim's `]h`,
//! `[h`, `gj`, `gk`, `gh` and `gl`. Each returns where point goes.

use org_syntax::{ParseContext, SyntaxKind, SyntaxNode};

use crate::buffer::{EditError, user_error};
use crate::headline::{back_to_heading, headings, stars_at, subtree_end};
use crate::narrow::element_at;

fn bol(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn start(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().start())
}

fn end(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().end())
}

/// A heading line at the line of `pos`, inlinetasks left out.
fn on_heading(text: &str, pos: usize, ctx: &ParseContext) -> bool {
    stars_at(text, bol(text, pos)).is_some_and(|n| ctx.inlinetask_min_level.is_none_or(|l| n < l))
}

/// `org-element-at-point`'s `:parent`, as org-element has them: sections
/// and headlines too, the document last.
fn parent(n: &SyntaxNode) -> Option<SyntaxNode> {
    n.parent()
}

/// A container's `:contents-end`: for a section, before its trailing
/// blank lines.
fn contents_end(n: &SyntaxNode, text: &str) -> Option<usize> {
    if n.kind() == SyntaxKind::SECTION {
        let e = end(n);
        let s = start(n);
        let q = s + text[s..e].trim_end_matches([' ', '\r', '\t', '\n']).len();
        return Some(
            text[q..]
                .find('\n')
                .map_or(text.len(), |i| q + i + 1)
                .min(e),
        );
    }
    org_syntax::ast::contents_range(n).map(|r| usize::from(r.end()))
}

/// `org-forward-heading-same-level` by `n` (back when negative): the
/// heading `n` siblings away, not past a heading of a higher level; from
/// before the first heading, the first heading (or the start).
pub fn heading_same_level(text: &str, pos: usize, n: i64, ctx: &ParseContext) -> usize {
    let limit = ctx.inlinetask_min_level;
    let Ok((h, level)) = back_to_heading(text, pos, limit) else {
        return if n < 0 {
            0
        } else {
            headings(text, limit)
                .first()
                .map_or(text.len(), |(s, _)| *s)
        };
    };
    // Every heading, inlinetasks too, stops the search at a higher level.
    let all = headings(text, None);
    let mut count = n.unsigned_abs();
    let mut result = h;
    let mut found = |s: usize, l: usize| -> bool {
        if l < level {
            return false;
        }
        if l == level {
            count -= 1;
            result = s;
        }
        count > 0
    };
    if n < 0 {
        for &(s, l) in all.iter().rev().filter(|(s, _)| *s < h) {
            if !found(s, l) {
                break;
            }
        }
    } else {
        for &(s, l) in all.iter().filter(|(s, _)| *s > h) {
            if !found(s, l) {
                break;
            }
        }
    }
    result
}

/// `org-forward-element`: past the element at point, to the next one of
/// the same level when there is one; from a heading, to the next heading
/// of its level or above.
pub fn forward_element(text: &str, pos: usize, ctx: &ParseContext) -> Result<usize, EditError> {
    if pos >= text.len() {
        return user_error("Cannot move further down");
    }
    let limit = ctx.inlinetask_min_level;
    if on_heading(text, pos, ctx) {
        let (h, level) = back_to_heading(text, pos, limit)?;
        let to = subtree_end(text, h, level, limit);
        if to < text.len() && on_heading(text, to, ctx) {
            return Ok(to);
        }
        return user_error("Cannot move further down");
    }
    let parse = org_syntax::parse_with(text, ctx);
    let Some(el) = element_at(&parse.syntax(), pos) else {
        return Ok(pos);
    };
    let e = end(&el);
    Ok(match parent(&el) {
        Some(p) if contents_end(&p, text) == Some(e) => end(&p),
        _ => e,
    })
}

/// `org-backward-element`: to the start of the element at point, or of
/// the one before; from a heading, to the previous heading of its level,
/// else to its parent.
pub fn backward_element(text: &str, pos: usize, ctx: &ParseContext) -> Result<usize, EditError> {
    if pos == 0 {
        return user_error("Cannot move further up");
    }
    let limit = ctx.inlinetask_min_level;
    if on_heading(text, pos, ctx) {
        let to = heading_same_level(text, pos, -1, ctx);
        if to != pos {
            return Ok(to);
        }
        // `org-up-heading-safe`.
        let (h, level) = back_to_heading(text, pos, limit)?;
        return match headings(text, limit)
            .into_iter()
            .rev()
            .find(|(s, l)| *s < h && *l < level)
        {
            Some((s, _)) => Ok(s),
            None => user_error("Cannot move further up"),
        };
    }
    let parse = org_syntax::parse_with(text, ctx);
    let root = parse.syntax();
    let Some(el) = element_at(&root, pos) else {
        return Ok(pos);
    };
    let beg = start(&el);
    if pos != beg {
        return Ok(beg);
    }
    let p = text[..beg].trim_end_matches([' ', '\r', '\t', '\n']).len();
    if p == 0 {
        return Ok(0);
    }
    let Some(mut prev) = element_at(&root, p) else {
        return Ok(p);
    };
    let mut to = start(&prev);
    while let Some(par) = parent(&prev) {
        if par.kind() == SyntaxKind::DOCUMENT || end(&par) > beg {
            break;
        }
        to = start(&par);
        prev = par;
    }
    Ok(to)
}

/// `org-up-element`: to the start of the element around the one at
/// point, else to the heading of the entry.
pub fn up_element(text: &str, pos: usize, ctx: &ParseContext) -> Result<usize, EditError> {
    let limit = ctx.inlinetask_min_level;
    if on_heading(text, pos, ctx) {
        // `org-up-heading-safe`, which goes to the heading's start first.
        let (h, level) = back_to_heading(text, pos, limit)?;
        return match headings(text, limit)
            .into_iter()
            .rev()
            .find(|(s, l)| *s < h && *l < level)
        {
            Some((s, _)) => Ok(s),
            None => Err(EditError::at("No surrounding element", h)),
        };
    }
    let parse = org_syntax::parse_with(text, ctx);
    let el = element_at(&parse.syntax(), pos);
    // Sections are skipped.
    let p = el.as_ref().and_then(parent).and_then(|p| {
        if p.kind() == SyntaxKind::SECTION {
            parent(&p)
        } else {
            Some(p)
        }
    });
    if let Some(p) = p
        && p.kind() != SyntaxKind::DOCUMENT
    {
        return Ok(start(&p));
    }
    match back_to_heading(text, pos, ctx.inlinetask_min_level) {
        Ok((h, _)) => Ok(h),
        Err(_) => user_error("No surrounding element"),
    }
}

/// `org-down-element`: into the contents of the element at point.
pub fn down_element(text: &str, pos: usize, ctx: &ParseContext) -> Result<usize, EditError> {
    use SyntaxKind::*;
    let parse = org_syntax::parse_with(text, ctx);
    let Some(el) = element_at(&parse.syntax(), pos) else {
        return user_error("No inner element");
    };
    let contents = || org_syntax::ast::contents_range(&el).map(|r| usize::from(r.start()));
    match el.kind() {
        PLAIN_LIST | TABLE => {
            contents().map_or_else(|| user_error("No content for this element"), |c| Ok(c + 1))
        }
        HEADLINE | INLINETASK | PROPERTY_DRAWER | DRAWER | ITEM | CENTER_BLOCK | QUOTE_BLOCK
        | SPECIAL_BLOCK | DYNAMIC_BLOCK | FOOTNOTE_DEFINITION => {
            contents().map_or_else(|| user_error("No content for this element"), Ok)
        }
        _ => user_error("No inner element"),
    }
}

/// `org-next-link` (`backward`: `org-previous-link`), Doom's `]l` and
/// `[l`: the start of the next (previous) link outside the one at `pos`;
/// none when there is no further link.
pub fn next_link(text: &str, pos: usize, backward: bool, ctx: &ParseContext) -> Option<usize> {
    let parse = org_syntax::parse_with(text, ctx);
    let mut ranges: Vec<(usize, usize)> = parse
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::LINK)
        .map(|n| (start(&n), end(&n)))
        .collect();
    ranges.sort_unstable();
    let inside = |(s, e): (usize, usize)| s <= pos && pos < e;
    if backward {
        let from = ranges.iter().find(|r| inside(**r)).map_or(pos, |r| r.0);
        ranges.into_iter().rev().find(|r| r.0 < from).map(|r| r.0)
    } else {
        let from = ranges
            .iter()
            .find(|r| inside(**r))
            .map_or(pos, |r| r.1.saturating_sub(1));
        ranges.into_iter().find(|r| r.0 > from).map(|r| r.0)
    }
}

/// `org-babel-next-src-block` (`backward`: the previous one), Doom's `]c`
/// and `[c`: the first line of the next source block starting after the
/// line at `pos` (of the previous one ending before it), past its
/// `#+NAME:` and other affiliated keywords.
pub fn next_src_block(text: &str, pos: usize, backward: bool, ctx: &ParseContext) -> Option<usize> {
    let parse = org_syntax::parse_with(text, ctx);
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let eol = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    // Each block's first line and the end of its `#+end_src`: searching
    // back, Emacs's regular expression must end before the line at `pos`.
    let mut blocks: Vec<(usize, usize)> = parse
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SRC_BLOCK)
        .map(|n| {
            let e = text[..end(&n)].trim_end().len();
            (usize::from(org_syntax::ast::post_affiliated(&n)), e)
        })
        .collect();
    blocks.sort_unstable();
    if backward {
        blocks.into_iter().rev().find(|b| b.1 <= bol).map(|b| b.0)
    } else {
        blocks.into_iter().find(|b| b.0 > eol).map(|b| b.0)
    }
}
