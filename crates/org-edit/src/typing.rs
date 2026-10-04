//! Typing in Org documents: `org-self-insert-command`,
//! `org-delete-backward-char` and `org-delete-char`. In tables, typing and
//! deleting keep columns aligned by taking or giving a space before the
//! next `|`; the first key after moving to a field blanks it
//! (`org-table-auto-blank-field`); on headlines, tags stay aligned
//! (`org-fix-tags-on-the-fly`).
//!
//! Deletion removes `len` bytes: the editor passes a grapheme cluster,
//! where Emacs removes one character.

use org_model::Document;

use crate::buffer::{Buf, EditError, column_at};
use crate::table::{at_table, hline_at};
use crate::transaction::Transaction;

fn bol(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn eol(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

/// `(save-excursion (skip-chars-backward " \t") (bolp))`.
fn blank_before(text: &str, pos: usize) -> bool {
    text[bol(text, pos)..pos]
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t'))
}

/// `(looking-at "[^|\n]*  |")`.
fn room_in_field(text: &str, pos: usize) -> bool {
    let rest = &text[pos..eol(text, pos)];
    rest.find('|').is_some_and(|i| rest[..i].ends_with("  "))
}

/// `org-table-check-inside-data-field`.
fn check_data_field(doc: &Document, text: &str, pos: usize) -> Result<(), EditError> {
    let inside = at_table(doc, text, pos)
        && !blank_before(text, pos)
        && !hline_at(text, bol(text, pos))
        && !text[pos..eol(text, pos)]
            .trim_matches([' ', '\t'])
            .is_empty();
    if inside {
        Ok(())
    } else {
        Err(EditError::new("Not in table data field"))
    }
}

/// `org-table-blank-field`: the field becomes as many spaces as it is wide,
/// and point goes after its first one.
fn blank_field(buf: &mut Buf) {
    let p = buf.point;
    let start = buf.text[..p].rfind('|').map_or(0, |i| i + 1);
    let bar = start.saturating_sub(1);
    // `|[^|\n]+`
    let end = buf.text[start..]
        .find(['|', '\n'])
        .map_or(buf.text.len(), |i| start + i);
    if end == start || buf.text.as_bytes().get(bar) != Some(&b'|') {
        return;
    }
    let width = column_at(&buf.text, end) - column_at(&buf.text, bar) - 1;
    buf.replace(start, end, &" ".repeat(width));
    buf.point = bar + 2;
}

/// Where the tags of the heading line `line` start
/// (`org-tag-line-re`, group 1).
fn tags_start(line: &str) -> Option<usize> {
    let stars = line.bytes().take_while(|b| *b == b'*').count();
    if stars == 0 || line.as_bytes().get(stars) != Some(&b' ') {
        return None;
    }
    let body = line.trim_end_matches([' ', '\t']);
    let start = body.rfind([' ', '\t']).map_or(0, |i| i + 1).max(stars + 1);
    let tags = &body[start..];
    let ok = tags.len() >= 3
        && tags.starts_with(':')
        && tags.ends_with(':')
        && tags[1..tags.len() - 1]
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%' | ':'));
    ok.then_some(start)
}

/// `org-fix-tags-on-the-fly`: realigns the tags of the heading line at
/// point when point is before them.
fn fix_tags(buf: &mut Buf) {
    let b = buf.bol(buf.point);
    let e = buf.eol(b);
    if let Some(t) = tags_start(&buf.text[b..e])
        && buf.point < b + t
    {
        crate::headline::align_tags(buf, b);
    }
}

/// `org-self-insert-command` with `text` as the typed character.
/// `blank`: the previous command moved to a table field (Tab, Shift+Tab,
/// Enter, or aligned the table), so the field is blanked first.
pub fn self_insert(
    doc: &Document,
    point: usize,
    text: &str,
    blank: bool,
) -> Result<Transaction, EditError> {
    let source = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&source, point);
    if at_table(doc, &source, point) {
        if blank {
            check_data_field(doc, &source, point)?;
            blank_field(&mut buf);
        }
        if room_in_field(&buf.text, buf.point) {
            buf.insert_at_point(text);
            // `room_in_field` found the field's end and the blank before it.
            if let Some(off) = buf.text[buf.point..].find('|') {
                let bar = buf.point + off;
                buf.delete(bar - 2, bar - 1);
            }
            return Ok(buf.transaction("Typing"));
        }
    }
    buf.insert_at_point(text);
    fix_tags(&mut buf);
    Ok(buf.transaction("Typing"))
}

/// `org-delete-char`: deletes the `len` bytes after point.
pub fn delete_forward(doc: &Document, point: usize, len: usize) -> Transaction {
    let source = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&source, point);
    delete_char(doc, &mut buf, len);
    buf.transaction("Delete")
}

fn delete_char(doc: &Document, buf: &mut Buf, len: usize) {
    let p = buf.point;
    let end = (p + len).min(buf.text.len());
    let plain = buf.text.as_bytes().get(p) == Some(&b'|')
        || blank_before(&buf.text, p)
        || !at_table(doc, &buf.text, p);
    if plain {
        buf.delete(p, end);
        fix_tags(buf);
        return;
    }
    // `.\(.*?\)|`: a character, then the next `|` on the line.
    let line_end = eol(&buf.text, p);
    let bar = if end <= line_end && p < line_end {
        buf.text[end..line_end].find('|').map(|i| end + i)
    } else {
        None
    };
    buf.delete(p, end);
    if let Some(bar) = bar {
        // A space before the `|`, keeping the column's width.
        let at = bar - (end - p);
        buf.insert_before_point(at, " ");
        buf.point = p;
    }
}

/// `org-delete-backward-char`: deletes the `len` bytes before point.
pub fn delete_backward(doc: &Document, point: usize, len: usize) -> Transaction {
    let source = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&source, point);
    let p = point;
    let start = p.saturating_sub(len);
    let in_field = p > 0
        && source.as_bytes()[p - 1] != b'|'
        && !blank_before(&source, p)
        && source[p..eol(&source, p)].contains('|')
        && at_table(doc, &source, p);
    if in_field {
        buf.point = start;
        delete_char(doc, &mut buf, p - start);
    } else {
        buf.delete(start, p);
        buf.point = start;
        fix_tags(&mut buf);
    }
    buf.transaction("Delete")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, f: impl Fn(&Document) -> Transaction) -> (String, usize) {
        let doc = Document::new(org_syntax::parse(text));
        let t = f(&doc);
        (t.apply(text), t.selection_after.map_or(0, |s| s.head))
    }

    #[test]
    fn tables_stay_aligned() {
        let t = "| ab   | c |\n|------+---|\n";
        assert_eq!(
            run(t, |d| self_insert(d, 4, "x", false).unwrap()),
            ("| abx  | c |\n|------+---|\n".into(), 5)
        );
        // No room: the column grows.
        assert_eq!(
            run(t, |d| self_insert(d, 10, "y", false).unwrap()).0,
            "| ab   | cy |\n|------+---|\n"
        );
        assert_eq!(
            run(t, |d| delete_backward(d, 4, 1)),
            ("| a    | c |\n|------+---|\n".into(), 3)
        );
        assert_eq!(
            run(t, |d| delete_forward(d, 2, 1)),
            ("| b    | c |\n|------+---|\n".into(), 2)
        );
        // After Tab: the field is blanked first.
        assert_eq!(
            run(t, |d| self_insert(d, 3, "z", true).unwrap()),
            ("| z    | c |\n|------+---|\n".into(), 3)
        );
        assert!(
            Document::new(org_syntax::parse(t))
                .parse()
                .syntax()
                .to_string()
                == t
        );
    }

    #[test]
    fn tags_follow_typing() {
        let t = "* A                                                                   :tag:\n";
        let (out, _) = run(t, |d| self_insert(d, 3, "b", false).unwrap());
        assert!(out.starts_with("* Ab ") && out.ends_with(" :tag:\n"));
        assert_eq!(out.trim_end().len(), 77);
        assert_eq!(tags_start("* A :x:y:"), Some(4));
        assert_eq!(tags_start("* A:x:"), None);
        assert_eq!(tags_start("* :x:"), Some(2));
    }
}
