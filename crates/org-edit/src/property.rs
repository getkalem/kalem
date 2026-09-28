//! Properties: `org-entry-put` for drawer properties, with the property
//! drawer created where Org creates it and lines aligned by
//! `org-property-format` (`%-10s %s`).

use org_model::Document;
use org_syntax::ParseContext;

use crate::buffer::{Buf, EditError};
use crate::headline::org_back_to_heading;

/// Properties that `org-entry-put` refuses (`org-special-properties`
/// without TODO, PRIORITY, SCHEDULED and DEADLINE, which are commands of
/// their own).
const SPECIAL: &[&str] = &[
    "ALLTAGS",
    "BLOCKED",
    "CLOCKSUM",
    "CLOCKSUM_T",
    "CLOSED",
    "FILE",
    "ITEM",
    "TAGS",
    "TIMESTAMP",
    "TIMESTAMP_IA",
];

fn line_end(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

fn next_line(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1)
}

fn indent_of(text: &str, bol: usize) -> usize {
    let n = text[bol..]
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    crate::buffer::column_at(text, bol + n) - crate::buffer::column_at(text, bol)
}

/// `indent-to-column` with `indent-tabs-mode`.
pub(crate) fn indentation(col: usize) -> String {
    format!("{}{}", "\t".repeat(col / 8), " ".repeat(col % 8))
}

/// `org-planning-line-re` at `bol`.
pub(crate) fn is_planning_line(text: &str, bol: usize) -> bool {
    let rest = text[bol..].trim_start_matches([' ', '\t']);
    ["CLOSED:", "DEADLINE:", "SCHEDULED:"]
        .iter()
        .any(|w| rest.starts_with(w))
}

/// `org-property-drawer-re` at `p`: the end of its `:END:` line.
pub(crate) fn property_drawer_at(text: &str, p: usize) -> Option<usize> {
    if p >= text.len() || (p > 0 && text.as_bytes()[p - 1] != b'\n') {
        return None;
    }
    let first_eol = line_end(text, p);
    if text[p..first_eol].trim_matches([' ', '\t']) != ":PROPERTIES:" || first_eol == text.len() {
        return None;
    }
    let mut l = first_eol + 1;
    loop {
        let eol = line_end(text, l);
        let line = text[l..eol].trim_start_matches([' ', '\t']);
        if line.trim_end_matches([' ', '\t']) == ":END:" {
            return Some(eol);
        }
        // `[ \t]*:\S-+:\(?:[ \t].*\)?[ \t]*\n`
        let word = &line[..line.find([' ', '\t']).unwrap_or(line.len())];
        if !(word.len() >= 3 && word.starts_with(':') && word.ends_with(':')) || eol == text.len() {
            return None;
        }
        l = eol + 1;
    }
}

/// Where the property drawer of the entry at `h` is or goes: after the
/// heading and its planning line.
fn drawer_position(text: &str, h: Option<usize>) -> usize {
    match h {
        Some(h) => {
            let mut p = next_line(text, h);
            if p > h && text.as_bytes()[p - 1] == b'\n' && is_planning_line(text, p) {
                p = next_line(text, p);
            }
            p
        }
        None => {
            // Before the first headline: after comment lines at the top.
            let mut p = 0;
            while p < text.len() {
                let eol = line_end(text, p);
                let l = text[p..eol].trim_start_matches([' ', '\t']);
                if !(l == "#" || l.starts_with("# ")) || eol == text.len() {
                    break;
                }
                p = eol + 1;
            }
            p
        }
    }
}

/// `org-indent-line` for a new line at `pos` in the entry at `h`: like the
/// element before it (its first line; for a drawer, the drawer's first
/// line), or, right after the headline, at the headline's text with
/// `org-adapt-indentation` and at the margin without.
pub(crate) fn sibling_indent(
    text: &str,
    h: Option<usize>,
    pos: usize,
    adapt: bool,
    inline_min: Option<usize>,
) -> usize {
    let before = text[..pos].trim_end_matches([' ', '\t', '\n', '\r']).len();
    let heading_end = h.map(|h| line_end(text, h));
    if h.is_none() || before <= heading_end.unwrap_or(0) {
        return match h {
            // `org-current-level`, which skips inlinetasks.
            Some(h) if adapt => crate::headline::headings(&text[..line_end(text, h)], inline_min)
                .last()
                .map_or(0, |(_, l)| l + 1),
            _ => 0,
        };
    }
    let bol = text[..before].rfind('\n').map_or(0, |i| i + 1);
    if text[bol..before].trim().eq_ignore_ascii_case(":END:") {
        // The start of the drawer.
        let mut l = bol;
        while l > 0 {
            l = text[..l - 1].rfind('\n').map_or(0, |i| i + 1);
            let line = text[l..line_end(text, l)].trim();
            let name = line.strip_prefix(':').and_then(|r| r.strip_suffix(':'));
            if name.is_some_and(|n| {
                !n.is_empty()
                    && n.chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            }) {
                return indent_of(text, l);
            }
            if heading_end.is_some_and(|e| l <= e) {
                break;
            }
        }
    }
    indent_of(text, bol)
}

/// `org-get-property-block` with FORCE: the body of the property drawer of
/// the entry at `h` (or of the document), created if needed.
fn property_block(
    buf: &mut Buf,
    h: Option<usize>,
    adapt: bool,
    inline_min: Option<usize>,
) -> (usize, usize) {
    let p = drawer_position(&buf.text, h);
    if let Some(end) = property_drawer_at(&buf.text, p) {
        let body = next_line(&buf.text, p);
        let end_bol = buf.text[..end].rfind('\n').map_or(0, |i| i + 1);
        return (body, end_bol);
    }
    // `org-insert-property-drawer`.
    let t = &buf.text;
    let mut q = p;
    if q > 0 && t.as_bytes()[q - 1] == b'\n' {
        q -= 1;
    }
    let at_bob = q == 0;
    let prev_bol = (!at_bob).then(|| t[..q].rfind('\n').map_or(0, |i| i + 1));
    let _ = prev_bol;
    let indent = indentation(sibling_indent(t, h, q, adapt, inline_min));
    let mut ins = String::new();
    if !at_bob {
        ins.push('\n');
    }
    ins.push_str(&format!("{indent}:PROPERTIES:\n{indent}:END:"));
    let eob = q == t.len();
    if eob || at_bob {
        ins.push('\n');
    }
    buf.insert_before_point(q, &ins);
    let drawer = q + usize::from(!at_bob);
    let end_bol = next_line(&buf.text, drawer);
    (end_bol, end_bol)
}

/// `org-entry-put` for a drawer property of the entry at `h` (`None`: the
/// document's top property drawer).
pub(crate) fn entry_put(
    buf: &mut Buf,
    h: Option<usize>,
    key: &str,
    value: &str,
    adapt: bool,
    inline_min: Option<usize>,
) -> Result<(), EditError> {
    if key.is_empty() || key.contains(char::is_whitespace) {
        return Err(EditError::new(&format!("Invalid property name: \"{key}\"")));
    }
    if SPECIAL.iter().any(|s| s.eq_ignore_ascii_case(key)) {
        return Err(EditError::new(&format!(
            "The {key} property cannot be set with `org-entry-put'"
        )));
    }
    let (start, end) = property_block(buf, h, adapt, inline_min);
    // An existing line for the key, case-insensitively.
    let mut l = start;
    let mut line_start = None;
    while l < end {
        let eol = line_end(&buf.text, l);
        let line = buf.text[l..eol].trim_start_matches([' ', '\t']);
        let matches = line.len() > key.len() + 1
            && line.as_bytes()[0] == b':'
            && line[1..]
                .get(..key.len())
                .is_some_and(|k| k.eq_ignore_ascii_case(key))
            && line.as_bytes()[key.len() + 1] == b':'
            && (line.len() == key.len() + 2
                || matches!(line.as_bytes()[key.len() + 2], b' ' | b'\t'));
        if matches {
            line_start = Some(l);
            break;
        }
        l = eol + 1;
    }
    // Indentation of the previous property line, or of `:PROPERTIES:`.
    let bol = match line_start {
        Some(l) => {
            let eol = line_end(&buf.text, l);
            buf.delete(l, eol);
            l
        }
        None => {
            buf.insert_before_point(end, "\n");
            end
        }
    };
    let prev = buf.text[..bol - 1].rfind('\n').map_or(0, |i| i + 1);
    let indent = indent_of(&buf.text, prev);
    let formatted = format!("{:<10} {value}", format!(":{key}:"));
    let line = format!(
        "{}{}",
        indentation(indent),
        formatted.trim_matches([' ', '\t', '\n', '\r'])
    );
    // The line is empty now; the text goes after a cursor at its start.
    buf.insert_before_point(bol, &line);
    Ok(())
}

/// `org-entry-delete` (`org-delete-property`): takes `key` (and `KEY+`)
/// out of the property drawer of the entry at `point`, and the drawer when
/// nothing is left in it. Returns `None` when there was no such property.
pub fn delete_property(doc: &Document, point: usize, key: &str) -> Option<crate::Transaction> {
    let text = doc.parse().syntax().to_string();
    let ctx: &ParseContext = doc.parse().context();
    let mut buf = Buf::new(&text, point);
    let h = org_back_to_heading(&text, point, ctx);
    let p = drawer_position(&text, h);
    let end = property_drawer_at(&text, p)?;
    let begin = next_line(&text, p);
    let end_bol = text[..end].rfind('\n').map_or(0, |i| i + 1);
    // `^[ \t]*:KEY\+?:\(?:[ \t]+.*\)?[ \t]*$`, case-insensitively.
    let is_key = |line: &str| {
        let l = line.trim_start_matches([' ', '\t']);
        let Some(rest) = l.strip_prefix(':') else {
            return false;
        };
        let Some(rest) = rest
            .get(..key.len())
            .filter(|k| k.eq_ignore_ascii_case(key))
            .map(|_| &rest[key.len()..])
        else {
            return false;
        };
        let rest = rest.strip_prefix('+').unwrap_or(rest);
        rest.strip_prefix(':')
            .is_some_and(|r| r.is_empty() || r.starts_with([' ', '\t']))
    };
    let mut removed = Vec::new();
    let mut l = begin;
    while l < end_bol {
        let n = next_line(&text, l);
        if is_key(&text[l..line_end(&text, l)]) {
            removed.push(l..n);
        }
        l = n;
    }
    if removed.is_empty() {
        return None;
    }
    let left = end_bol - begin - removed.iter().map(|r| r.len()).sum::<usize>();
    if left == 0 {
        // The whole drawer, `:PROPERTIES:` to `:END:`.
        buf.delete(p, next_line(&text, end_bol));
    } else {
        for r in removed.into_iter().rev() {
            buf.delete(r.start, r.end);
        }
    }
    Some(buf.transaction("Delete property"))
}

/// `org-toggle-ordered-property`: sets `ORDERED` to `t` on the entry at
/// `point`, or takes it away. Returns the change and whether the entry's
/// tasks are now ordered.
pub fn toggle_ordered(
    doc: &Document,
    point: usize,
) -> Result<(crate::Transaction, bool), EditError> {
    let text = doc.parse().syntax().to_string();
    if org_back_to_heading(&text, point, doc.parse().context()).is_none() {
        return Err(EditError::new(&format!(
            "Before first headline at position {}",
            point + 1
        )));
    }
    let entry = doc.outline().entry_at(point);
    if doc
        .entry_get(entry, "ORDERED", org_model::Inherit::No, false)
        .is_some()
        && let Some(t) = delete_property(doc, point, "ORDERED")
    {
        return Ok((t, false));
    }
    Ok((set_property(doc, point, "ORDERED", "t", false)?, true))
}

/// `org-set-property` for a drawer property of the entry at `point`.
pub fn set_property(
    doc: &Document,
    point: usize,
    key: &str,
    value: &str,
    adapt: bool,
) -> Result<crate::Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let ctx: &ParseContext = doc.parse().context();
    let mut buf = Buf::new(&text, point);
    let h = org_back_to_heading(&text, point, ctx);
    entry_put(&mut buf, h, key, value, adapt, ctx.inlinetask_min_level)?;
    Ok(buf.transaction("Set property"))
}
