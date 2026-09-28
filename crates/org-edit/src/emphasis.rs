//! Emphasis: `org-emphasize`, which wraps the region in markers or inserts
//! a pair of them, and [`toggle_emphasis`], the editor's bold/italic
//! command, which also removes markers and keeps to what the Org syntax
//! can express.

use org_model::Document;
use org_syntax::{SyntaxKind, SyntaxNode};

use crate::buffer::{Buf, EditError};
use crate::transaction::Transaction;

/// The emphasis markers of `org-emphasis-alist`.
pub const MARKERS: &[char] = &['*', '/', '_', '=', '~', '+'];

/// `[:space:]` in Org's syntax table, plus the line feed.
fn is_space(c: char) -> bool {
    c.is_whitespace()
}

/// The PRE characters of `org-emphasis-regexp-components`.
fn is_pre(c: char) -> bool {
    matches!(c, '-' | '(' | '\'' | '"' | '{') || is_space(c)
}

/// The POST characters of `org-emphasis-regexp-components`.
fn is_post(c: char) -> bool {
    matches!(
        c,
        '-' | '.' | ',' | ':' | '!' | '?' | ';' | '\'' | '"' | ')' | '}' | '[' | '\\'
    ) || is_space(c)
}

/// `org-emphasize` on the region `mark`..`point`, or at `point` without a
/// region. `marker` is the emphasis character; `None` (a space in Emacs)
/// removes the region's outer markers.
pub fn emphasize(
    doc: &Document,
    point: usize,
    mark: Option<usize>,
    marker: Option<char>,
) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&text, point);
    if let Some(c) = marker
        && !MARKERS.contains(&c)
    {
        return Err(EditError::new(&format!("No such emphasis marker: \"{c}\"")));
    }
    let region = mark
        .filter(|&m| m != point)
        .map(|m| (point.min(m), point.max(m)));
    let mut string = region.map_or(String::new(), |(b, e)| text[b..e].to_string());
    let s = marker.map_or(String::new(), String::from);
    let mv = region.is_none() && marker.is_some();
    // Strip markers that already surround the text.
    loop {
        let mut chars = string.chars();
        let (Some(first), Some(last)) = (chars.next(), chars.next_back()) else {
            break;
        };
        if string.chars().count() > 1 && first == last && MARKERS.contains(&first) {
            string = string[first.len_utf8()..string.len() - last.len_utf8()].to_string();
        } else {
            break;
        }
    }
    let string = format!("{s}{string}{s}");
    if let Some((b, e)) = region {
        buf.delete(b, e);
    }
    let mut p = buf.point;
    let before = buf.text[..p].chars().next_back();
    if !(before.is_none() || before == Some('\n') || before.is_some_and(is_pre)) {
        buf.insert_at_point(" ");
        p = buf.point;
    }
    let after = buf.text[p..].chars().next();
    if !(after.is_none() || after == Some('\n') || after.is_some_and(is_post)) {
        buf.insert_at_point(" ");
        buf.point -= 1;
    }
    buf.insert_at_point(&string);
    if mv {
        buf.point -= s.len();
    }
    Ok(buf.transaction("Emphasize"))
}

/// The kinds of emphasis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// `*bold*`
    Bold,
    /// `/italic/`
    Italic,
    /// `_underline_`
    Underline,
    /// `+strike-through+`
    StrikeThrough,
    /// `=verbatim=`
    Verbatim,
    /// `~code~`
    Code,
}

impl Emphasis {
    /// The marker character.
    pub fn marker(self) -> char {
        match self {
            Emphasis::Bold => '*',
            Emphasis::Italic => '/',
            Emphasis::Underline => '_',
            Emphasis::StrikeThrough => '+',
            Emphasis::Verbatim => '=',
            Emphasis::Code => '~',
        }
    }

    fn kind(self) -> SyntaxKind {
        match self {
            Emphasis::Bold => SyntaxKind::BOLD,
            Emphasis::Italic => SyntaxKind::ITALIC,
            Emphasis::Underline => SyntaxKind::UNDERLINE,
            Emphasis::StrikeThrough => SyntaxKind::STRIKE_THROUGH,
            Emphasis::Verbatim => SyntaxKind::VERBATIM,
            Emphasis::Code => SyntaxKind::CODE,
        }
    }
}

fn is_emphasis(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::BOLD
            | SyntaxKind::ITALIC
            | SyntaxKind::UNDERLINE
            | SyntaxKind::STRIKE_THROUGH
            | SyntaxKind::VERBATIM
            | SyntaxKind::CODE
    )
}

/// The object's range without trailing blanks, and its contents range.
fn spans(n: &SyntaxNode) -> ((usize, usize), (usize, usize)) {
    let start = usize::from(n.text_range().start());
    let t = n.text().to_string();
    let end = start + t.trim_end_matches([' ', '\t']).len();
    ((start, end), (start + 1, end - 1))
}

/// Bold, italic and the other emphasis for the selection `start..end`, as a
/// word processor does it: if the selection is (inside) that emphasis, the
/// markers go; otherwise it is wrapped with `org-emphasize`, after
/// trimming blanks at its ends. Selections that cross the boundary of
/// other emphasis, or lie in code or verbatim text, are refused, as is a
/// result that would not parse as the emphasis.
pub fn toggle_emphasis(
    doc: &Document,
    start: usize,
    end: usize,
    kind: Emphasis,
) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let root = doc.parse().syntax();
    let objects: Vec<SyntaxNode> = root
        .descendants()
        .filter(|n| is_emphasis(n.kind()))
        .collect();
    // Unwrap: the selection is within an object of this kind.
    let enclosing = objects
        .iter()
        .filter(|n| n.kind() == kind.kind())
        .find(|n| {
            let ((s, e), _) = spans(n);
            start >= s && end <= e
        });
    if let Some(n) = enclosing {
        let ((s, e), _) = spans(n);
        let mut buf = Buf::new(&text, end);
        buf.delete(e - 1, e);
        buf.delete(s, s + 1);
        return Ok(buf.transaction("Remove emphasis"));
    }
    // No formatting inside code or verbatim.
    if objects.iter().any(|n| {
        matches!(n.kind(), SyntaxKind::CODE | SyntaxKind::VERBATIM) && {
            let (_, (cs, ce)) = spans(n);
            start < ce && end > cs
        }
    }) {
        return Err(EditError::new("Code and verbatim text cannot be formatted"));
    }
    // The selection must not cut another object in two.
    for n in &objects {
        let ((s, e), _) = spans(n);
        let crosses = (start > s && start < e && end > e) || (start < s && end > s && end < e);
        if crosses {
            return Err(EditError::new(
                "The selection crosses the boundary of other formatting",
            ));
        }
    }
    let sel = &text[start..end];
    let lead = sel.len() - sel.trim_start().len();
    let trail = sel.len() - sel.trim_end().len();
    let (s, e) = (start + lead, end - trail);
    if s >= e {
        return Err(EditError::new("Nothing to format"));
    }
    if text[s..e].contains("\n\n") {
        return Err(EditError::new("Emphasis cannot span paragraphs"));
    }
    // Wrap like `org-emphasize`, except that the start and end of an
    // enclosing object's contents are fine neighbors: `*bold /word/*`.
    let contents_start = objects.iter().any(|n| spans(n).1.0 == s);
    let contents_end = objects.iter().any(|n| spans(n).1.1 == e);
    let before = text[..s].chars().next_back();
    let after = text[e..].chars().next();
    let pre_ok = before.is_none_or(|c| c == '\n' || is_pre(c)) || contents_start;
    let post_ok = after.is_none_or(|c| c == '\n' || is_post(c)) || contents_end;
    let m = kind.marker();
    let mut buf = Buf::new(&text, e);
    buf.insert_before_point(e, &format!("{m}{}", if post_ok { "" } else { " " }));
    buf.insert_before_point(s, &format!("{}{m}", if pre_ok { "" } else { " " }));
    buf.point = e + 1 + usize::from(!pre_ok) + 1;
    let t = buf.transaction("Emphasize");
    // The result must parse as the emphasis.
    let new_text = t.apply(&text);
    let parse = org_syntax::parse_with(&new_text, doc.parse().context());
    let ok = parse.syntax().descendants().any(|n| {
        n.kind() == kind.kind() && {
            let ((os, _), _) = spans(&n);
            os >= s && os <= s + 1
        }
    });
    if !ok {
        return Err(EditError::new("The selection cannot be formatted here"));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toggle(text: &str, s: usize, e: usize, k: Emphasis) -> Result<String, EditError> {
        let doc = Document::new(org_syntax::parse(text));
        toggle_emphasis(&doc, s, e, k).map(|t| t.apply(text))
    }

    #[test]
    fn toggles() {
        assert_eq!(
            toggle("a word here\n", 2, 6, Emphasis::Bold).unwrap(),
            "a *word* here\n"
        );
        assert_eq!(
            toggle("a *word* here\n", 3, 7, Emphasis::Bold).unwrap(),
            "a word here\n"
        );
        assert_eq!(
            toggle("a *word* here\n", 2, 8, Emphasis::Bold).unwrap(),
            "a word here\n"
        );
        // Blanks at the ends of the selection stay outside.
        assert_eq!(
            toggle("a word here\n", 1, 7, Emphasis::Italic).unwrap(),
            "a /word/ here\n"
        );
        // Nested kinds are fine, crossing boundaries is not.
        assert_eq!(
            toggle("a *bold word* x\n", 8, 12, Emphasis::Italic).unwrap(),
            "a *bold /word/* x\n"
        );
        assert!(toggle("a *bold word* x\n", 5, 15, Emphasis::Italic).is_err());
        assert!(toggle("a ~code here~ x\n", 5, 9, Emphasis::Bold).is_err());
    }
}
