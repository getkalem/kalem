//! Archiving and refiling within one file: `org-toggle-archive-tag`,
//! `org-archive-to-archive-sibling` and `org-refile` to a heading of the
//! same document, as `org-archive.el` and `org-refile.el` do them.

use org_model::Document;
use org_syntax::ParseContext;

use crate::buffer::{Buf, EditError, user_error};
use crate::headline::{align_tags, headings, paste_in, subtree_range, valid_level};
use crate::transaction::Transaction;

/// `org-archive-tag`.
pub const ARCHIVE_TAG: &str = "ARCHIVE";

/// `org-archive-sibling-heading`.
pub const SIBLING_HEADING: &str = "Archive";

fn text_of(doc: &Document) -> String {
    doc.parse().syntax().to_string()
}

/// The heading at or before `pos` (`org-back-to-heading t`), with its
/// level.
fn heading_at(text: &str, pos: usize) -> Result<(usize, usize), EditError> {
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    headings(text, None)
        .into_iter()
        .rev()
        .find(|(s, _)| *s <= bol)
        .ok_or_else(|| EditError::new("Before first headline at position"))
}

/// `org-end-of-subtree t t` from the heading at `h` of `level`: the next
/// heading of that level or above (inlinetasks are not headings), or the
/// end.
fn subtree_end(text: &str, h: usize, level: usize, limit: Option<usize>) -> usize {
    headings(text, limit)
        .into_iter()
        .find(|(s, l)| *s > h && *l <= level)
        .map_or(text.len(), |(s, _)| s)
}

/// `outline-up-heading 1 t` from the heading at `h`: its parent heading.
fn up_heading(text: &str, h: usize, level: usize) -> Option<usize> {
    headings(text, None)
        .into_iter()
        .rev()
        .find(|(s, l)| *s < h && *l < level)
        .map(|(s, _)| s)
}

/// `org-toggle-archive-tag`: the `ARCHIVE` tag of the heading at `point`
/// set or removed. Returns whether it is set; setting it moves the cursor
/// to the start of its line.
pub fn toggle_archive_tag(doc: &Document, point: usize) -> Result<(Transaction, bool), EditError> {
    let text = text_of(doc);
    let (h, _) = heading_at(&text, point)?;
    let mut buf = Buf::new(&text, point);
    let set = crate::tags::toggle_at(&mut buf, h, ARCHIVE_TAG, None, doc.parse().context());
    if set {
        buf.point = buf.bol(buf.point);
    }
    Ok((
        buf.transaction(if set { "Archive" } else { "Unarchive" }),
        set,
    ))
}

/// `org-archive-to-archive-sibling`: the subtree at `point` moved to the
/// end of its `Archive` sibling (tagged `ARCHIVE`, made at the end of the
/// parent's subtree when missing), with an `ARCHIVE_TIME` property of
/// `now` (`2026-09-28 Mon 10:00`), and the parent's TODO statistics
/// updated.
pub fn archive_to_sibling(
    doc: &Document,
    point: usize,
    now: &str,
) -> Result<Transaction, EditError> {
    let text = text_of(doc);
    let ctx = doc.parse().context();
    let limit = ctx.inlinetask_min_level;
    // Searching back for a heading leaves the cursor at the start.
    let (h, level) = heading_at(&text, point).map_err(|e| EditError::at(&e.message, 0))?;
    let mut buf = Buf::new(&text, point);
    let leader = format!("{} ", "*".repeat(level));
    let pos = buf.add_advancing_marker(h);
    let (b, e) = match up_heading(&buf.text, h, level) {
        Some(p) => {
            let pl = buf.text[p..].bytes().take_while(|c| *c == b'*').count();
            (p, subtree_end(&buf.text, p, pl, limit))
        }
        None => (0, buf.text.len()),
    };
    // `^LEADER[ \t]*Archive[ \t]*:ARCHIVE:`, case-insensitively.
    let found = find_sibling(&buf.text, b, e, &leader);
    let sibling = match found {
        Some(s) => s,
        None => {
            buf.point = e;
            if !(e == 0 || buf.text.as_bytes()[e - 1] == b'\n') {
                buf.insert_at_point("\n");
            }
            let at = buf.point;
            buf.insert_at_point(&format!("{leader}{SIBLING_HEADING}\n"));
            crate::tags::toggle_at(&mut buf, at, ARCHIVE_TAG, Some(true), ctx);
            at
        }
    };
    buf.point = subtree_end(&buf.text, sibling, level, limit);
    // Cut the subtree (point stays where it is, as a marker would).
    let at = buf.advancing_marker(pos);
    let r = subtree_range(&buf.text, at, ctx)?;
    let clip = buf.text[r.clone()].to_string();
    buf.delete(r.start, r.end);
    paste_in(
        &mut buf,
        &clip,
        Some(valid_level(level, 1, ctx.odd_levels_only)),
        limit,
    );
    let pasted = buf.bol(buf.point);
    crate::property::entry_put(&mut buf, Some(pasted), "ARCHIVE_TIME", now, false, limit)?;
    // The statistics of the sibling's parent.
    let sib = heading_at(&buf.text, pasted)
        .ok()
        .and_then(|(p, l)| up_heading(&buf.text, p, l))
        .unwrap_or(pasted);
    let now_doc = Document::with_settings(
        org_syntax::parse_with(&buf.text, ctx),
        std::sync::Arc::new(doc.settings().clone()),
        None,
    );
    crate::todo::update_parent_todo_statistics(&mut buf, &now_doc, sib, level);
    buf.point = buf.advancing_marker(pos);
    // On a blank line: the next heading that shows, the sibling's
    // children being folded.
    let bol = buf.bol(buf.point);
    let eol = buf.eol(buf.point);
    if buf.point == bol && buf.text[bol..eol].trim_matches([' ', '\t']).is_empty() {
        let s = sibling_start(&buf.text, level);
        let folded = s.map(|s| (buf.eol(s), subtree_end(&buf.text, s, level, limit)));
        buf.point = headings(&buf.text, None)
            .into_iter()
            .map(|(s, _)| s)
            .find(|&s| s > buf.point && !folded.is_some_and(|(a, b)| a < s && s < b))
            .unwrap_or(buf.text.len());
    }
    Ok(buf.transaction("Archive to sibling"))
}

/// The start of the first archive sibling of `level` in `text`.
fn sibling_start(text: &str, level: usize) -> Option<usize> {
    let leader = format!("{} ", "*".repeat(level));
    find_sibling(text, 0, text.len(), &leader).map(|s| text[..s].rfind('\n').map_or(0, |i| i + 1))
}

/// The archive sibling line starting with `leader` between `b` and `e`:
/// its start.
fn find_sibling(text: &str, b: usize, e: usize, leader: &str) -> Option<usize> {
    let mut pos = b;
    while pos < e {
        let bol = pos;
        let eol = text[bol..].find('\n').map_or(text.len(), |i| bol + i);
        let line = &text[bol..eol];
        if let Some(rest) = line.strip_prefix(leader) {
            let rest = rest.trim_start_matches([' ', '\t']);
            if rest.len() >= SIBLING_HEADING.len()
                && rest[..SIBLING_HEADING.len()].eq_ignore_ascii_case(SIBLING_HEADING)
            {
                let rest = rest[SIBLING_HEADING.len()..].trim_start_matches([' ', '\t']);
                let tag = format!(":{ARCHIVE_TAG}:");
                if rest.len() >= tag.len()
                    && rest[..tag.len()].eq_ignore_ascii_case(&tag)
                    // The match ends before `e`.
                    && eol.min(bol + line.len() - rest.len() + tag.len()) <= e
                {
                    return Some(bol);
                }
            }
        }
        pos = eol + 1;
    }
    None
}

/// `org-refile` of the subtree at `point` to the heading starting at
/// `target` in the same document: it becomes the target's last child.
/// The cursor goes to where the subtree was.
pub fn refile(doc: &Document, point: usize, target: usize) -> Result<Transaction, EditError> {
    let text = text_of(doc);
    let ctx: &ParseContext = doc.parse().context();
    let limit = ctx.inlinetask_min_level;
    let (h, level) = heading_at(&text, point)?;
    let end = subtree_end(&text, h, level, limit);
    if target >= h && target < end {
        return user_error("Cannot refile to position inside the tree or region");
    }
    let Some(&(_, tl)) = headings(&text, None).iter().find(|(s, _)| *s == target) else {
        return user_error("Not at a headline");
    };
    let clip = text[subtree_range(&text, point, ctx)?].to_string();
    let mut buf = Buf::new(&text, point);
    let origin = buf.add_advancing_marker(point);
    let new_level = valid_level(tl, 1, ctx.odd_levels_only);
    // After the target's subtree: its next sibling, or the end of its
    // subtree.
    buf.point = subtree_end(&buf.text, target, tl, limit);
    if !(buf.point == 0 || buf.text.as_bytes()[buf.point - 1] == b'\n') {
        buf.insert_at_point("\n");
    }
    paste_in(&mut buf, &clip, Some(new_level), limit);
    let bol = buf.bol(buf.point);
    if headings(&buf.text, limit).iter().any(|(s, _)| *s == bol) {
        align_tags(&mut buf, bol);
    }
    buf.point = buf.advancing_marker(origin);
    let (oh, ol) = heading_at(&buf.text, buf.point)?;
    let oe = subtree_end(&buf.text, oh, ol, limit);
    buf.point = oe;
    buf.delete(oh, oe);
    Ok(buf.transaction("Refile"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(t: &str) -> Document {
        Document::new(org_syntax::parse(t))
    }

    fn apply(t: &str, tx: Transaction) -> (String, usize) {
        let p = tx.selection_after.map_or(0, |s| s.head);
        (tx.apply(t), p)
    }

    #[test]
    fn archive_tag() {
        let t = "* A\nbody\n";
        let (tx, set) = toggle_archive_tag(&doc(t), 6).unwrap();
        assert!(set);
        let (t2, p) = apply(t, tx);
        assert!(t2.starts_with("* A") && t2.contains(":ARCHIVE:"), "{t2}");
        assert_eq!(p, t2.find("body").unwrap());
        let (tx, set) = toggle_archive_tag(&doc(&t2), 0).unwrap();
        assert!(!set);
        assert_eq!(apply(&t2, tx).0, t);
        assert!(toggle_archive_tag(&doc("text\n"), 0).is_err());
    }

    #[test]
    fn archive_sibling_and_refile() {
        let t = "* P [0/2]\n** TODO a\n** TODO b\n* Q\n";
        let tx = archive_to_sibling(&doc(t), 12, "2026-09-28 Mon 10:00").unwrap();
        let (t2, _) = apply(t, tx);
        assert!(t2.starts_with("* P [0/1]\n** TODO b\n** Archive"), "{t2}");
        assert!(t2.contains(":ARCHIVE:\n*** TODO a\n:PROPERTIES:\n:ARCHIVE_TIME: 2026-09-28 Mon 10:00\n:END:\n* Q\n"), "{t2}");
        let t = "* A\n** a1\n* B\nb\n* C\n";
        let tx = refile(&doc(t), 11, 0).unwrap();
        let (t2, p) = apply(t, tx);
        assert_eq!(t2, "* A\n** a1\n** B\nb\n* C\n");
        assert_eq!(p, t2.find("* C").unwrap());
        assert!(refile(&doc(t), 1, 4).is_err());
    }
}
