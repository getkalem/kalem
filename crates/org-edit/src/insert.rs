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

#[cfg(test)]
mod tests {
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
