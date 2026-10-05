//! Tags: `org-set-tags`, `org-toggle-tag`, `org-change-tag-in-region`,
//! `org-align-tags`, and the tag selection of `org-set-tags-command` with
//! mutually exclusive groups.

use org_model::{Document, TagTable, complex_heading};
use org_syntax::ParseContext;

use crate::buffer::{Buf, EditError};
use crate::headline::{align_tags, org_back_to_heading, stars_at};
use crate::transaction::Transaction;

/// The end of the heading line at `pos`: before its line feed, and before
/// a CR LF file's carriage return (no tag ends with one).
fn line_end(text: &str, pos: usize) -> usize {
    let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    if end > pos && text.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

/// The local tags of the heading line at `h`.
fn local_tags(text: &str, h: usize, ctx: &ParseContext) -> Vec<String> {
    let line = &text[h..line_end(text, h)];
    complex_heading(line, ctx)
        .and_then(|c| c.tags)
        .map(|r| {
            line[r]
                .split(':')
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `org-tag-line-re`: the start of the tags of the heading line at `h`.
fn tags_start(text: &str, h: usize) -> Option<usize> {
    let eol = line_end(text, h);
    let line = &text[h..eol];
    let body = line.trim_end_matches([' ', '\t']);
    let start = body.rfind([' ', '\t']).map_or(0, |i| i + 1);
    let tags = &body[start..];
    let stars = line.bytes().take_while(|b| *b == b'*').count();
    let valid = tags.len() >= 3
        && tags.starts_with(':')
        && tags.ends_with(':')
        && tags[1..tags.len() - 1]
            .chars()
            .all(|c| c == ':' || c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%'))
        && start > stars;
    valid.then_some(h + start)
}

/// `org-set-tags` on the heading at `h`.
fn set_tags_at(buf: &mut Buf, h: usize, tags: &[String], ctx: &ParseContext) {
    if local_tags(&buf.text, h, ctx) == tags {
        // Unchanged tags are aligned all the same.
        if !tags.is_empty() {
            align_tags(buf, h);
        }
        return;
    }
    let eol = line_end(&buf.text, h);
    let start = tags_start(&buf.text, h).unwrap_or(eol);
    let p = buf.text[h..start].trim_end_matches([' ', '\t']).len() + h;
    buf.delete(p, eol);
    let mut p = p;
    if stars_at(&buf.text, h).is_none() {
        buf.insert_before_point(p, " ");
        p += 1;
    }
    if !tags.is_empty() {
        buf.insert_before_point(p, &format!(" :{}:", tags.join(":")));
        align_tags(buf, h);
    }
}

fn heading(doc: &Document, point: usize) -> Result<(String, usize), EditError> {
    let text = doc.parse().syntax().to_string();
    let ctx = doc.parse().context();
    match org_back_to_heading(&text, point, ctx) {
        Some(h) => Ok((text, h)),
        None => Err(EditError::new(&format!(
            "Before first headline at position {}",
            point + 1
        ))),
    }
}

/// `org-set-tags` on the headline at `point`: replaces its tags.
pub fn set_tags(doc: &Document, point: usize, tags: &[String]) -> Result<Transaction, EditError> {
    let (text, h) = heading(doc, point)?;
    let mut buf = Buf::new(&text, point);
    set_tags_at(&mut buf, h, tags, doc.parse().context());
    Ok(buf.transaction("Set tags"))
}

/// Toggle one tag (`org-toggle-tag`) on the heading at `h`; `on` forces
/// a state. Returns whether the tag is set.
pub(crate) fn toggle_at(
    buf: &mut Buf,
    h: usize,
    tag: &str,
    on: Option<bool>,
    ctx: &ParseContext,
) -> bool {
    let mut current = local_tags(&buf.text, h, ctx);
    let present = current.iter().any(|t| t == tag);
    let add = match on {
        Some(v) => v,
        None => !present,
    };
    if add {
        if !present {
            current.push(tag.to_string());
        }
    } else {
        current.retain(|t| t != tag);
    }
    set_tags_at(buf, h, &current, ctx);
    add
}

/// `org-toggle-tag` on the headline at `point`. `on` forces the tag on or
/// off. Returns the change and whether the tag is now set.
pub fn toggle_tag(
    doc: &Document,
    point: usize,
    tag: &str,
    on: Option<bool>,
) -> Result<(Transaction, bool), EditError> {
    let (text, h) = heading(doc, point)?;
    let mut buf = Buf::new(&text, point);
    let set = toggle_at(&mut buf, h, tag, on, doc.parse().context());
    Ok((buf.transaction("Toggle tag"), set))
}

/// `org-change-tag-in-region`: sets (or with `off` removes) `tag` on each
/// heading line from the line of `beg` to the line before the one of
/// `end`.
pub fn change_tag_in_region(
    doc: &Document,
    point: usize,
    beg: usize,
    end: usize,
    tag: &str,
    off: bool,
) -> Transaction {
    let text = doc.parse().syntax().to_string();
    let ctx = doc.parse().context();
    let mut buf = Buf::new(&text, point);
    let (beg, end) = (beg.min(text.len()), end.min(text.len()));
    let first = text[..beg].rfind('\n').map_or(0, |i| i + 1);
    let last = text[..end].rfind('\n').map_or(0, |i| i + 1);
    // Line starts, as markers, since tags change line lengths.
    let mut lines = Vec::new();
    let mut l = first;
    while l < last {
        lines.push(buf.add_marker(l));
        l = text[l..].find('\n').map_or(text.len(), |i| l + i + 1);
    }
    let last_line = lines.last().copied();
    for m in lines {
        let bol = buf.marker(m);
        if stars_at(&buf.text, bol).is_some()
            && let Some(h) = org_back_to_heading(&buf.text, bol, ctx)
        {
            toggle_at(&mut buf, h, tag, Some(!off), ctx);
        }
    }
    // The loop leaves point on the last line it visited.
    buf.point = match last_line {
        Some(m) => buf.marker(m),
        None => beg,
    };
    buf.transaction("Change tag in region")
}

/// `(org-align-tags t)`: aligns the tags of every headline.
pub fn align_all_tags(doc: &Document, point: usize) -> Transaction {
    let text = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&text, point);
    let starts: Vec<usize> = crate::headline::headings(&text, None)
        .into_iter()
        .map(|(s, _)| s)
        .collect();
    for s in starts.into_iter().rev() {
        if tags_start(&buf.text, s).is_some() {
            align_tags(&mut buf, s);
        }
    }
    buf.transaction("Align tags")
}

/// [`align_all_tags`] as one replacement per changed headline line, for
/// callers that map positions through it.
pub(crate) fn align_tags_by_line(text: &str) -> Transaction {
    let mut tx = Transaction::new("Align tags");
    for (s, _) in crate::headline::headings(text, None) {
        if tags_start(text, s).is_none() {
            continue;
        }
        let eol = line_end(text, s);
        let line = &text[s..eol];
        let mut buf = Buf::new(line, 0);
        align_tags(&mut buf, 0);
        if buf.text != line {
            // One edit per line: they cannot overlap.
            let _ = tx.replace(s..eol, buf.text.clone());
        }
    }
    tx
}

/// The tag list after choosing `tag` in the tag selection of
/// `org-set-tags-command`: the tag is removed if present, else added and
/// the other tags of its mutually exclusive groups removed; tags are kept
/// in the order of the table.
pub fn select_tag(current: &[String], tag: &str, table: &TagTable) -> Vec<String> {
    let mut tags: Vec<String> = current.to_vec();
    if tags.iter().any(|t| t == tag) {
        tags.retain(|t| t != tag);
    } else {
        for g in table.exclusive_groups() {
            if g.iter().any(|t| t == tag) {
                tags.retain(|t| !g.contains(t));
            }
        }
        tags.insert(0, tag.to_string());
    }
    // A stable insertion sort by table position; tags outside the table
    // stay where they are relative to others.
    let order: Vec<&str> = table.tags().map(|(n, _)| n).collect();
    let pos = |t: &str| order.iter().position(|x| *x == t);
    let less = |a: &str, b: &str| matches!((pos(a), pos(b)), (Some(x), Some(y)) if x < y);
    for i in 1..tags.len() {
        let mut j = i;
        while j > 0 && less(&tags[j], &tags[j - 1]) {
            tags.swap(j, j - 1);
            j -= 1;
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CR LF file's heading: its tags found before the CR, replaced and
    /// aligned with the CR kept at the end of the line.
    #[test]
    fn tags_of_a_crlf_heading() {
        let text = "* Heading :a:\r\nbody\r\n";
        let doc = Document::new(org_syntax::parse(text));
        let tx = set_tags(&doc, 0, &["b".to_string()]).unwrap();
        let out = tx.apply(text);
        assert!(
            out.starts_with("* Heading") && out.contains(":b:\r\nbody\r\n"),
            "{out:?}"
        );
        assert!(!out.contains(":a:"), "{out:?}");
    }

    #[test]
    fn selection_with_exclusive_groups() {
        let table = TagTable::parse("{ @office @home } laptop phone");
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            select_tag(&s(&["phone", "@office"]), "@home", &table),
            s(&["@home", "phone"])
        );
        assert_eq!(
            select_tag(&s(&["phone"]), "laptop", &table),
            s(&["laptop", "phone"])
        );
        assert_eq!(
            select_tag(&s(&["laptop", "phone"]), "laptop", &table),
            s(&["phone"])
        );
    }
}
