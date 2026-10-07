//! Insert commands: links (`org-insert-link` with a target),
//! blocks (`org-insert-structure-template`), timestamps (`org-timestamp`
//! with a date), and horizontal rules.

use jiff::civil::DateTime;
use org_model::{Document, time};

use crate::buffer::{Buf, EditError};
use crate::timestamp::timestamp_at;
use crate::transaction::Transaction;

/// `org-link-escape`: backslashes before a bracket or at the end are
/// doubled, and brackets get one.
pub fn link_escape(link: &str) -> String {
    let mut out = String::new();
    let mut run = 0;
    for c in link.chars() {
        if c == '\\' {
            run += 1;
            continue;
        }
        if matches!(c, '[' | ']') {
            out.push_str(&"\\".repeat(2 * run + 1));
        } else {
            out.push_str(&"\\".repeat(run));
        }
        run = 0;
        out.push(c);
    }
    out.push_str(&"\\".repeat(2 * run));
    out
}

/// `org-link-unescape`: a run of backslashes before a bracket or at the
/// end is halved.
pub fn link_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let n = rest[i..].bytes().take_while(|b| *b == b'\\').count();
        let after = &rest[i + n..];
        if after.is_empty() || after.starts_with(['[', ']']) {
            out.push_str(&"\\".repeat(n / 2));
        } else {
            out.push_str(&rest[i..i + n]);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// A bracket link's end, the range of its target and of its description.
type BracketLink = (usize, (usize, usize), Option<(usize, usize)>);

/// The bracket link (`org-link-bracket-re`) starting at `i`.
fn bracket_link_at(text: &str, i: usize) -> Option<BracketLink> {
    let b = text.as_bytes();
    if !text[i..].starts_with("[[") {
        return None;
    }
    let t = i + 2;
    let mut k = t;
    while k < b.len() {
        match b[k] {
            b'[' => return None,
            b']' => break,
            b'\\' => {
                let n = b[k..].iter().take_while(|c| **c == b'\\').count();
                k += n;
                if k < b.len() {
                    k += text[k..].chars().next().map_or(1, char::len_utf8);
                }
            }
            _ => k += text[k..].chars().next().map_or(1, char::len_utf8),
        }
    }
    if k == t || b.get(k) != Some(&b']') {
        return None;
    }
    let target = (t, k);
    match b.get(k + 1) {
        Some(b']') => Some((k + 2, target, None)),
        Some(b'[') => {
            let d = k + 2;
            // `[^z-a]+?` up to the first `]]`.
            let close = text[d..].find("]]").map(|j| d + j)?;
            (close > d).then_some((close + 2, target, Some((d, close))))
        }
        _ => None,
    }
}

/// Doom Emacs's `+org/remove-link`: the bracket link at `point` gives way
/// to its description, or to its target when it has none.
pub fn remove_link(text: &str, point: usize) -> Result<Transaction, EditError> {
    let bol = text[..point].rfind('\n').map_or(0, |i| i + 1);
    let eol = text[point..].find('\n').map_or(text.len(), |i| point + i);
    let mut i = bol;
    while let Some(j) = text[i..eol].find("[[") {
        let s = i + j;
        if s > point {
            break;
        }
        if let Some((end, (ts, te), desc)) = bracket_link_at(text, s) {
            if point <= end {
                let label = match desc {
                    Some((ds, de)) => text[ds..de].to_string(),
                    None => link_unescape(&text[ts..te]),
                };
                let mut buf = Buf::new(text, point);
                buf.replace(s, end, &label);
                buf.point = point.min(s + label.len());
                return Ok(buf.transaction("Remove link"));
            }
            i = end;
        } else {
            i = s + 1;
        }
    }
    Err(EditError::new("No link at point"))
}

/// `org-link-make-string`: `[[LINK][DESCRIPTION]]`, with the description
/// trimmed and zero width spaces keeping `]]` out of it.
pub fn link_string(link: &str, description: Option<&str>) -> Result<String, EditError> {
    let zws = '\u{200B}';
    let desc = description
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            let mut d = d.to_string();
            if d.ends_with(']') {
                d.push(zws);
            }
            d.replace("]]", &format!("]{zws}]"))
        });
    if link.trim().is_empty() {
        return desc.ok_or_else(|| EditError::new("Empty link"));
    }
    Ok(match desc {
        Some(d) => format!("[[{}][{d}]]", link_escape(link)),
        None => format!("[[{}]]", link_escape(link)),
    })
}

/// `org-insert-link` with a target: replaces the region (which becomes the
/// description when none is given) with a bracket link.
pub fn insert_link(
    doc: &Document,
    point: usize,
    mark: Option<usize>,
    link: &str,
    description: Option<&str>,
) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let ctx = doc.parse().context();
    let region = mark
        .filter(|&m| m != point)
        .map(|m| (point.min(m), point.max(m)));
    let desc = description
        .map(str::to_string)
        .or_else(|| region.map(|(b, e)| text[b..e].to_string()));
    // URL-like links lose angle brackets.
    let mut link = link.to_string();
    let inner = link.strip_prefix('<').and_then(|l| l.strip_suffix('>'));
    if let Some(inner) = inner {
        let plain = ctx
            .link_types
            .iter()
            .any(|t| inner.starts_with(&format!("{t}:")));
        let timestamp = time::parse_time_string(inner).is_some()
            && inner.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if plain && !timestamp {
            link = inner.to_string();
        }
    }
    let s = link_string(&link, desc.as_deref())?;
    let mut buf = Buf::new(&text, point);
    if let Some((b, e)) = region {
        buf.delete(b, e);
    }
    buf.insert_at_point(&s);
    Ok(buf.transaction("Insert link"))
}

/// `org-escape-code-in-region`: a comma before lines starting with `*`,
/// `#+`, `,*` or `,#+`.
fn escape_code(buf: &mut Buf, beg: usize, end: usize) {
    let mut starts = Vec::new();
    let mut l = buf.text[..beg].rfind('\n').map_or(0, |i| i + 1);
    while l < end {
        let e = buf.text[l..].find('\n').map_or(buf.text.len(), |i| l + i);
        let line = &buf.text[l..e];
        let ws = line.len() - line.trim_start_matches([' ', '\t']).len();
        let rest = line[ws..].trim_start_matches(',');
        if (rest.starts_with('*') || rest.starts_with("#+")) && l >= beg {
            starts.push(l + ws);
        }
        if e >= buf.text.len() {
            break;
        }
        l = e + 1;
    }
    for s in starts.into_iter().rev() {
        buf.insert_before_point(s, ",");
    }
}

/// `org-insert-structure-template`: a `#+begin_TYPE` … `#+end_TYPE` block
/// around the region, or an empty one. TYPE in capitals gives
/// `#+BEGIN_TYPE`.
pub fn insert_structure_template(
    doc: &Document,
    point: usize,
    mark: Option<usize>,
    block: &str,
) -> Result<Transaction, EditError> {
    if block.trim().is_empty() {
        return Err(EditError::new("Invalid structure type"));
    }
    let text = doc.parse().syntax().to_string();
    let region = mark
        .filter(|&m| m != point)
        .map(|m| (point.min(m), point.max(m)));
    let lower = block.to_lowercase();
    let extended = lower == "src" || lower == "export";
    let verbatim = ["example", "export", "src", "comment"]
        .iter()
        .any(|p| lower.starts_with(p));
    let first = block.split_whitespace().next().unwrap_or("");
    let upcase = first == first.to_uppercase();
    let mut buf = Buf::new(&text, point);
    let end_m = region.map(|(_, e)| buf.add_marker(e));
    if let Some((b, _)) = region {
        buf.point = b;
    }
    let bol = buf.bol(buf.point);
    let column = crate::buffer::column_at(
        &buf.text,
        bol + buf.text[bol..]
            .bytes()
            .take_while(|c| matches!(c, b' ' | b'\t'))
            .count(),
    ) - crate::buffer::column_at(&buf.text, bol);
    let before = &buf.text[bol..buf.point];
    if before.trim_matches([' ', '\t']).is_empty() {
        buf.point = bol;
    } else {
        buf.insert_at_point("\n");
    }
    let start = buf.point;
    let indent = crate::property::indentation(column);
    // Inside `save-excursion`: build the block from START on.
    let mut p = start;
    let begin_line = format!(
        "{indent}#+{}_{block}{}\n",
        if upcase { "BEGIN" } else { "begin" },
        if extended { " " } else { "" }
    );
    buf.insert_before_point(p, &begin_line);
    p += begin_line.len();
    if let Some(m) = end_m {
        let end = buf.marker(m);
        if verbatim {
            escape_code(&mut buf, p, end);
        }
        let end = buf.marker(m);
        // Before blank lines at the end of the region.
        let q = buf.text[..end]
            .trim_end_matches([' ', '\r', '\t', '\n'])
            .len();
        p = buf.eol(q);
    }
    if !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
        buf.insert_before_point(p, "\n");
        p += 1;
    }
    let end_line = format!("{indent}#+{}_{first}", if upcase { "END" } else { "end" });
    buf.insert_before_point(p, &end_line);
    p += end_line.len();
    // A blank rest of line goes; otherwise the block ends its line.
    let eol = buf.eol(p);
    if buf.text[p..eol].trim_matches([' ', '\t']).is_empty() {
        buf.delete(p, eol);
    } else {
        buf.insert_before_point(p, "\n");
        p += 1;
    }
    if p == buf.text.len() && !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
        buf.insert_before_point(p, "\n");
    }
    // Point: after `#+begin_src ` for src and export, else at the text of
    // the first line inside.
    buf.point = if extended {
        buf.eol(start)
    } else {
        let n = buf.text[start..]
            .find('\n')
            .map_or(buf.text.len(), |i| start + i + 1);
        n + buf.text[n..]
            .bytes()
            .take_while(|c| matches!(c, b' ' | b'\t'))
            .count()
    };
    Ok(buf.transaction("Insert block"))
}

/// `org-timestamp` with a date: replaces the timestamp at point, keeping
/// its repeater and delay, or inserts one.
pub fn insert_timestamp(
    doc: &Document,
    point: usize,
    date: DateTime,
    with_time: bool,
    inactive: bool,
) -> Transaction {
    let text = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&text, point);
    let (o, c) = if inactive { ('[', ']') } else { ('<', '>') };
    let stamp = format!("{o}{}{c}", time::format(date, with_time));
    match timestamp_at(&text, point) {
        Some(ts) => {
            let old = &text[ts.start..ts.end];
            let repeater = repeater_of(old);
            // `replace-match`, then `insert-before-markers`.
            buf.replace_before_markers(ts.start, ts.end, &stamp);
            if let Some(r) = repeater {
                let at = ts.start + stamp.len() - 1;
                buf.point = at;
                buf.insert_at_point(&format!(" {r}"));
            }
        }
        None => buf.replace_before_markers(point, point, &stamp),
    }
    buf.transaction("Insert timestamp")
}

/// `\([.+-]+[0-9]+[hdwmy] ?\)+`: the repeater and delay cookies.
fn repeater_of(ts: &str) -> Option<String> {
    let b = ts.as_bytes();
    let cookie = |i: usize| -> Option<usize> {
        let mut j = i;
        while matches!(b.get(j), Some(b'.' | b'+' | b'-')) {
            j += 1;
        }
        if j == i {
            return None;
        }
        let d = b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        if d == 0 || !matches!(b.get(j + d), Some(b'h' | b'd' | b'w' | b'm' | b'y')) {
            return None;
        }
        let mut e = j + d + 1;
        if b.get(e) == Some(&b' ') {
            e += 1;
        }
        Some(e)
    };
    for i in 0..b.len() {
        if let Some(mut e) = cookie(i) {
            while let Some(n) = cookie(e) {
                e = n;
            }
            return Some(ts[i..e].to_string());
        }
    }
    None
}

/// A horizontal rule (`-----`) on the line at point if it is blank, else on
/// a new line after it.
pub fn insert_horizontal_rule(doc: &Document, point: usize) -> Transaction {
    let text = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&text, point);
    let bol = buf.bol(point);
    let eol = buf.eol(point);
    if text[bol..eol].trim().is_empty() {
        buf.replace(bol, eol, "-----");
        buf.point = bol + 5;
    } else {
        buf.point = eol;
        buf.insert_at_point("\n-----");
    }
    buf.transaction("Insert horizontal rule")
}

/// `org-insert-drawer` with a name: an empty drawer on its own lines at
/// point, point inside it; with a region (`mark`), the drawer around its
/// lines, point at the end of the last one. Drawers cannot hold
/// headlines.
pub fn insert_drawer(
    text: &str,
    point: usize,
    mark: Option<usize>,
    name: &str,
) -> Result<Transaction, EditError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(EditError::new("Invalid drawer name"));
    }
    let bol = |t: &str, p: usize| t[..p].rfind('\n').map_or(0, |i| i + 1);
    let mut buf = Buf::new(text, point);
    match mark.filter(|&m| m != point) {
        None => {
            let p = buf.point;
            if !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
                buf.insert_at_point("\n");
            }
            let start = buf.point;
            buf.insert_at_point(&format!(":{name}:\n\n:END:\n"));
            buf.point = start + name.len() + 3;
        }
        Some(m) => {
            let (rbeg, rend) = (point.min(m), point.max(m));
            let rend = buf.add_marker(rend);
            let start = bol(&buf.text, rbeg);
            // A headline in the region.
            let mut l = start;
            while l < buf.marker(rend) {
                let line = &buf.text[l..];
                let stars = line.bytes().take_while(|&b| b == b'*').count();
                if stars > 0
                    && line.as_bytes().get(stars) == Some(&b' ')
                    && l + stars < buf.marker(rend)
                {
                    // Org has moved to the region's first line.
                    return Err(EditError {
                        message: "Drawers cannot contain headlines".into(),
                        point: Some(start),
                    });
                }
                l = buf.text[l..]
                    .find('\n')
                    .map_or(buf.text.len(), |i| l + i + 1);
            }
            // The first non-blank line of the region.
            let mut p = start;
            while p < buf.text.len()
                && matches!(buf.text.as_bytes()[p], b' ' | b'\t' | b'\n' | b'\r')
            {
                p += 1;
            }
            let p = bol(&buf.text, p);
            buf.point = p;
            buf.insert_at_point(&format!(":{name}:\n"));
            // After the last non-blank character of the region.
            let mut e = buf.marker(rend);
            while e > 0 && matches!(buf.text.as_bytes()[e - 1], b' ' | b'\t' | b'\n' | b'\r') {
                e -= 1;
            }
            buf.point = e;
            buf.insert_at_point("\n:END:");
            let q = buf.point;
            if !(q == buf.text.len() || buf.text.as_bytes()[q] == b'\n') {
                buf.insert_at_point("\n");
            }
            let end_at = buf.text[..buf.point].rfind(":END:").unwrap_or(0);
            buf.point = end_at.saturating_sub(1);
        }
    }
    Ok(buf.transaction("Insert Drawer"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn links_removed() {
        let rm = |t: &str, p: usize| super::remove_link(t, p).map(|x| x.apply(t));
        assert_eq!(rm("a [[https://x.org][Org]] b", 5).unwrap(), "a Org b");
        assert_eq!(rm("a [[file:a\\]b.org]] b", 3).unwrap(), "a file:a]b.org b");
        assert_eq!(rm("a [[x][y]] [[z]] b", 13).unwrap(), "a [[x][y]] z b");
        // At either end of the link too; elsewhere no link.
        assert_eq!(rm("[[x][y]]", 8).unwrap(), "y");
        assert!(rm("a [[x][y]] b", 1).is_err());
        assert_eq!(super::link_unescape("a\\]b\\\\\\[c"), "a]b\\[c");
    }

    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(link_escape("a[b]c"), "a\\[b\\]c");
        assert_eq!(link_escape("a\\"), "a\\\\");
        assert_eq!(link_escape("a\\[x"), "a\\\\\\[x");
        assert_eq!(
            link_string("x", Some(" d]] e] ")).unwrap(),
            "[[x][d]\u{200B}] e]\u{200B}]]"
        );
        assert_eq!(
            repeater_of("<2026-01-01 Thu 10:00 +1w -2d>").as_deref(),
            Some("+1w -2d")
        );
    }

    #[test]
    fn rules() {
        let doc = Document::new(org_syntax::parse("text\n\n"));
        assert_eq!(
            insert_horizontal_rule(&doc, 2).apply("text\n\n"),
            "text\n-----\n\n"
        );
        assert_eq!(
            insert_horizontal_rule(&doc, 5).apply("text\n\n"),
            "text\n-----\n"
        );
    }
}
