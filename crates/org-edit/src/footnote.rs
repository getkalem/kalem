//! Footnotes (`org-footnote.el`): a new footnote with its definition
//! (`org-footnote-new`), renumbering (`org-footnote-renumber-fn:N`),
//! sorting (`org-footnote-sort`), normalizing (`org-footnote-normalize`),
//! deleting (`org-footnote-delete`), and going between a reference and
//! its definition (`org-footnote-action`). Definitions go into the
//! footnote section (`org-footnote-section`, "Footnotes"), or with no
//! section at the end of the reference's section.

use std::ops::Range;

use org_syntax::SyntaxKind::{self, *};
use org_syntax::SyntaxNode;
use org_syntax::ast::{self, AstNode};

use crate::buffer::Buf;
use crate::{EditError, Transaction};

/// How footnotes are laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteSettings {
    /// `org-footnote-section`: the heading definitions go under, or none
    /// (each definition at the end of its reference's section).
    pub section: Option<String>,
    /// `org-footnote-define-inline`: new footnotes defined where they are
    /// referenced, `[fn:1:]`.
    pub define_inline: bool,
    /// `org-footnote-auto-label`: how a new footnote gets its label.
    pub auto_label: AutoLabel,
    /// `org-footnote-auto-adjust`: footnotes renumbered and sorted after
    /// one is added or deleted.
    pub auto_adjust: bool,
}

/// `org-footnote-auto-label`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoLabel {
    /// `t`: the next free number.
    Auto,
    /// `nil` (and `plain`): the user types the label.
    Prompt,
    /// `confirm`: the user confirms the next free number or types another.
    Confirm,
    /// `random`: a random label.
    Random,
    /// `anonymous`: no label, `[fn::]`.
    Anonymous,
}

impl Default for FootnoteSettings {
    fn default() -> FootnoteSettings {
        FootnoteSettings {
            section: Some("Footnotes".into()),
            define_inline: false,
            auto_label: AutoLabel::Auto,
            auto_adjust: false,
        }
    }
}

impl FootnoteSettings {
    /// These settings with the `#+STARTUP` options of `text` applied
    /// (`fninline`, `nofninline`, `fnlocal`, `fnauto`, `fnprompt`,
    /// `fnconfirm`, `fnplain`, `fnanon`, `fnadjust`, `nofnadjust`).
    pub fn for_text(&self, text: &str) -> FootnoteSettings {
        let mut s = self.clone();
        if !text.contains("#+") {
            return s;
        }
        for (k, v) in org_syntax::parse(text).keywords() {
            if !k.eq_ignore_ascii_case("STARTUP") {
                continue;
            }
            for opt in v.split_whitespace() {
                match opt.to_ascii_lowercase().as_str() {
                    "fninline" => s.define_inline = true,
                    "nofninline" => s.define_inline = false,
                    "fnlocal" => s.section = None,
                    "fnauto" => s.auto_label = AutoLabel::Auto,
                    "fnprompt" | "fnplain" => s.auto_label = AutoLabel::Prompt,
                    "fnconfirm" => s.auto_label = AutoLabel::Confirm,
                    "fnanon" => s.auto_label = AutoLabel::Anonymous,
                    "fnadjust" => s.auto_adjust = true,
                    "nofnadjust" => s.auto_adjust = false,
                    _ => {}
                }
            }
        }
        s
    }

    /// Whether a new footnote asks for its label.
    pub fn asks_label(&self) -> bool {
        matches!(self.auto_label, AutoLabel::Prompt | AutoLabel::Confirm)
    }
}

/// The label a new footnote in `text` is offered: the first free number
/// (`org-footnote-unique-label`).
pub fn proposed_label(text: &str) -> String {
    let labels = all_labels(&root(text));
    (1..)
        .map(|n: usize| n.to_string())
        .find(|l| !labels.contains(l))
        // One of the first `labels.len() + 1` numbers is free.
        .unwrap_or_else(|| (labels.len() + 1).to_string())
}

/// `org-footnote-auto-adjust-maybe` after `tx` on `text`: the footnotes
/// renumbered and sorted when the settings ask for it, in one
/// transaction.
fn adjusted(
    text: &str,
    point: usize,
    tx: Transaction,
    settings: &FootnoteSettings,
) -> Result<Transaction, EditError> {
    if !settings.auto_adjust {
        return Ok(tx);
    }
    let label = tx.label.clone();
    let mut t = tx.apply(text);
    let mut p = tx.selection_after.map_or(point, |s| s.head);
    let r = renumber(&t, p)?;
    p = r.selection_after.map_or(p, |s| s.head);
    t = r.apply(&t);
    let at_definition = definition_at(&t, &root(&t), p).map(|(l, _)| l);
    let so = sort(&t, p, settings)?;
    p = so.selection_after.map_or(p, |s| s.head);
    t = so.apply(&t);
    // Back to the definition point was in, one space after its label.
    if let Some(label) = at_definition {
        let head = format!("[fn:{label}]");
        let found = t
            .match_indices(&head)
            .map(|(i, _)| i)
            .find(|&i| i == 0 || t.as_bytes()[i - 1] == b'\n');
        if let Some(i) = found {
            let after = i + head.len();
            let blanks = t[after..].len() - t[after..].trim_start_matches([' ', '\t']).len();
            t.replace_range(after..after + blanks, " ");
            p = after + 1;
        }
    }
    let mut buf = Buf::new(text, point);
    buf.text = t;
    buf.point = p;
    Ok(buf.transaction(&label))
}

fn root(text: &str) -> SyntaxNode {
    org_syntax::parse(text).syntax()
}

fn range(n: &SyntaxNode) -> Range<usize> {
    usize::from(n.text_range().start())..usize::from(n.text_range().end())
}

/// A character of `[-_[:word:]]`.
fn label_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// `org-element-context`: the innermost object at `pos`, else the element.
fn context(root: &SyntaxNode, pos: usize) -> Option<SyntaxNode> {
    let el = crate::narrow::element_at(root, pos)?;
    let mut found = el.clone();
    for d in el.descendants() {
        if d.kind().is_element() && d != el {
            continue;
        }
        let r = range(&d);
        if d.kind().is_object() && r.start <= pos && pos < r.end && d.ancestors().any(|a| a == el) {
            found = d;
        }
    }
    Some(found)
}

fn is_heading_line(line: &str) -> bool {
    let stars = line.bytes().take_while(|&b| b == b'*').count();
    stars > 0 && line.as_bytes().get(stars) == Some(&b' ')
}

fn heading_level(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b'*').count()
}

fn bol(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn eol(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

/// `forward-line`: the start of the next line, or the end of the text.
fn forward_line(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1)
}

fn skip_back(text: &str, mut pos: usize, chars: &[u8]) -> usize {
    while pos > 0 && chars.contains(&text.as_bytes()[pos - 1]) {
        pos -= 1;
    }
    pos
}

/// `count-lines`.
fn count_lines(text: &str, a: usize, b: usize) -> usize {
    let (a, b) = (a.min(b), a.max(b));
    let n = text[a..b].matches('\n').count();
    if a != b && !(b == 0 || text.as_bytes()[b - 1] == b'\n') {
        n + 1
    } else {
        n
    }
}

/// `outline-next-heading`: the next heading line after `pos`.
fn next_heading(text: &str, pos: usize) -> usize {
    let mut start = forward_line(text, pos);
    while start < text.len() {
        if is_heading_line(&text[start..eol(text, start)]) {
            return start;
        }
        start = forward_line(text, start);
    }
    text.len()
}

/// `org-back-over-empty-lines` with `org-blank-before-new-entry`'s
/// `heading` on (`auto`): point to the start of the first empty line
/// before it, and the number of empty lines passed.
fn back_over_empty_lines(buf: &mut Buf) -> usize {
    let pos = buf.point;
    let p = skip_back(&buf.text, pos, b" \t\n\r");
    let p = forward_line(&buf.text, p).min(pos);
    buf.point = p;
    count_lines(&buf.text, p, pos)
}

/// The heading lines of the footnote section: their starts.
fn section_heading(text: &str, section: &str, from: usize) -> Option<(usize, usize)> {
    let mut start = if from == 0 || text.as_bytes()[from - 1] == b'\n' {
        from
    } else {
        forward_line(text, from)
    };
    while start < text.len() {
        let end = eol(text, start);
        let line = &text[start..end];
        let stars = heading_level(line);
        if stars > 0 {
            let rest = &line[stars..];
            let after = rest.trim_start_matches(' ');
            if after.len() < rest.len()
                && let Some(tail) = after.get(..section.len())
                && tail.eq_ignore_ascii_case(section)
                && after[section.len()..].trim_matches([' ', '\t']).is_empty()
            {
                return Some((start, end));
            }
        }
        start = forward_line(text, start);
    }
    None
}

/// `org-end-of-subtree` with `to-heading`: the next heading of the same
/// level or higher after the heading at `start`, or the end.
fn subtree_end(text: &str, start: usize) -> usize {
    let level = heading_level(&text[start..eol(text, start)]);
    let mut p = forward_line(text, start);
    while p < text.len() {
        let line = &text[p..eol(text, p)];
        if is_heading_line(line) && heading_level(line) <= level {
            return p;
        }
        p = forward_line(text, p);
    }
    text.len()
}

/// `org-footnote--clear-footnote-section`: the footnote sections removed
/// and a new one started at the end, point in it.
fn clear_section(buf: &mut Buf, settings: &FootnoteSettings) {
    let Some(section) = &settings.section else {
        return;
    };
    let mut from = 0;
    while let Some((start, _)) = section_heading(&buf.text, section, from) {
        let end = subtree_end(&buf.text, start);
        buf.point = start;
        buf.delete(start, end);
        from = start;
    }
    let len = buf.text.len();
    buf.point = skip_back(&buf.text, len, b" \r\t\n");
    if buf.point != 0 {
        buf.point = forward_line(&buf.text, buf.point);
        let p = buf.point;
        if p == buf.text.len() || buf.text.as_bytes()[p] == b'\n' {
            buf.insert_at_point("\n");
        }
    }
    let (p, len) = (buf.point, buf.text.len());
    buf.delete(p, len);
    let saved = buf.point;
    let empty = back_over_empty_lines(buf) == 0;
    buf.point = saved;
    if empty {
        buf.insert_at_point("\n");
    }
    buf.insert_at_point(&format!("* {section}\n"));
}

/// `org-end-of-meta-data` with `full` t, from the heading line at `start`.
fn end_of_meta_data(text: &str, start: usize) -> usize {
    let mut p = forward_line(text, start);
    let line = |p: usize| &text[p..eol(text, p)];
    let planning = |l: &str| {
        let t = l.trim_start();
        ["CLOSED:", "DEADLINE:", "SCHEDULED:"]
            .iter()
            .any(|k| t.starts_with(k))
    };
    if p < text.len() && planning(line(p)) {
        p = forward_line(text, p);
    }
    if p < text.len() && line(p).trim().eq_ignore_ascii_case(":PROPERTIES:") {
        // A drawer of `:KEY:` lines up to `:END:`.
        let mut q = forward_line(text, p);
        while q < text.len() {
            let l = line(q).trim();
            if l.eq_ignore_ascii_case(":END:") {
                p = forward_line(text, q);
                break;
            }
            if !(l.starts_with(':') && l[1..].contains(':')) {
                break;
            }
            q = forward_line(text, q);
        }
    }
    if p < text.len() && is_heading_line(line(p)) {
        return p;
    }
    let end = next_heading(text, p);
    let drawer = |l: &str| {
        let t = l.trim();
        t.len() > 2
            && t.starts_with(':')
            && t.ends_with(':')
            && t[1..t.len() - 1]
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };
    while p < text.len() {
        let l = line(p);
        if l.trim().is_empty() || l.trim_start().starts_with("CLOCK:") {
            p = forward_line(text, p);
        } else if drawer(l) {
            let mut q = forward_line(text, p);
            let mut found = None;
            while q < end {
                if line(q).trim().eq_ignore_ascii_case(":END:") {
                    found = Some(q);
                    break;
                }
                q = forward_line(text, q);
            }
            match found {
                Some(q) => p = forward_line(text, q),
                None => break,
            }
        } else {
            break;
        }
    }
    p
}

/// `org-footnote--goto-local-insertion-point`.
fn goto_local_insertion_point(buf: &mut Buf) {
    let h = next_heading(&buf.text, buf.point);
    let p = skip_back(&buf.text, h, b" \t\n");
    buf.point = p;
    if p != 0 {
        buf.point = forward_line(&buf.text, p);
    }
    let p = buf.point;
    if !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
        buf.insert_at_point("\n");
    }
}

/// `org-footnote-create-definition`: an empty definition of `label`, in
/// the footnote section or at the end of the section; its start. Point
/// does not move.
fn create_definition(buf: &mut Buf, label: &str, settings: &FootnoteSettings) -> usize {
    let saved = buf.add_marker(buf.point);
    match &settings.section {
        None => goto_local_insertion_point(buf),
        Some(s) => match section_heading(&buf.text, s, 0) {
            Some((start, _)) => {
                buf.point = end_of_meta_data(&buf.text, start);
                let p = buf.point;
                if !(p == 0 || buf.text.as_bytes()[p - 1] == b'\n') {
                    buf.insert_at_point("\n");
                }
            }
            None => clear_section(buf, settings),
        },
    }
    if back_over_empty_lines(buf) == 0 {
        buf.insert_at_point("\n");
    }
    buf.insert_at_point(&format!("[fn:{label}] \n"));
    let at = bol(&buf.text, buf.point.saturating_sub(1));
    buf.point = buf.marker(saved);
    at
}

/// A footnote reference: its label, where it starts, whether it is
/// outside every definition, and the length of its inline definition.
#[derive(Debug, Clone)]
struct Reference {
    label: Option<String>,
    begin: usize,
    top: bool,
    size: Option<usize>,
}

/// `org-footnote--collect-references`: the references in reading order,
/// the ones in a definition after the first reference to it.
fn collect_references(text: &str, anonymous: bool) -> Vec<Reference> {
    let root = root(text);
    let mut refs: Vec<Reference> = Vec::new();
    // The references in each definition: a label, found again as the last
    // reference with it, as Org does; an anonymous one as itself (Org
    // looks for the last anonymous reference of the document, a wrong one
    // when there are several).
    let mut nested: Vec<(String, Vec<Result<String, usize>>)> = Vec::new();
    for n in root
        .descendants()
        .filter(|n| n.kind() == FOOTNOTE_REFERENCE)
    {
        let Some(r) = ast::FootnoteReference::cast(n.clone()) else {
            continue;
        };
        let label = r.label();
        if label.is_none() && !anonymous {
            continue;
        }
        let begin = range(&n).start;
        // A definition `[fn:x]` at a line start is not a reference.
        if begin == bol(text, begin) && !r.is_inline() && text[begin..].starts_with("[fn:") {
            continue;
        }
        let size = r
            .is_inline()
            .then(|| ast::contents_range(&n).map_or(0, |c| usize::from(c.len())));
        let def = n
            .ancestors()
            .find(|a| a.kind() == FOOTNOTE_DEFINITION)
            .and_then(ast::FootnoteDefinition::cast);
        if let Some(d) = &def {
            let dl = d.label();
            let entry = label.clone().ok_or(refs.len());
            match nested.iter_mut().find(|(k, _)| *k == dl) {
                Some((_, v)) => v.push(entry),
                None => nested.push((dl, vec![entry])),
            }
        }
        refs.push(Reference {
            label,
            begin,
            top: def.is_none(),
            size,
        });
    }
    fn add(
        r: &Reference,
        allow_nested: bool,
        refs: &[Reference],
        nested: &[(String, Vec<Result<String, usize>>)],
        out: &mut Vec<usize>,
        depth: usize,
    ) {
        if !(allow_nested || r.top) || depth > 64 {
            return;
        }
        let Some(i) = refs.iter().position(|x| x.begin == r.begin) else {
            return;
        };
        out.push(i);
        let Some(l) = &r.label else { return };
        if let Some((_, labels)) = nested.iter().find(|(k, _)| k == l) {
            for nl in labels {
                // The last reference with that label.
                let x = match nl {
                    Ok(l) => refs.iter().rev().find(|x| x.label.as_ref() == Some(l)),
                    Err(i) => refs.get(*i),
                };
                if let Some(x) = x {
                    add(x, true, refs, nested, out, depth + 1);
                }
            }
        }
    }
    let mut order = Vec::new();
    for r in &refs {
        add(r, false, &refs, &nested, &mut order, 0);
    }
    order.into_iter().map(|i| refs[i].clone()).collect()
}

/// `org-footnote--collect-definitions`: each label's first definition,
/// with the blank lines before it, and its text (trimmed), in reverse
/// order of the document as Org's list is.
fn definitions(text: &str) -> Vec<(String, Range<usize>, String)> {
    let root = root(text);
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for n in root
        .descendants()
        .filter(|n| n.kind() == FOOTNOTE_DEFINITION)
    {
        let Some(d) = ast::FootnoteDefinition::cast(n.clone()) else {
            continue;
        };
        let label = d.label();
        if seen.contains(&label) {
            continue;
        }
        seen.push(label.clone());
        let r = range(&n);
        let b = skip_back(text, r.start, b" \r\t\n");
        let beg = if b == 0 { 0 } else { forward_line(text, b) };
        let e = skip_back(text, r.end, b" \r\t\n");
        let end = forward_line(text, e);
        out.push((label, beg..end, text[beg..end].trim().to_string()));
    }
    out.reverse();
    out
}

/// Takes the definitions out of `buf`: their labels and texts, in
/// reverse order.
fn take_definitions(buf: &mut Buf) -> Vec<(String, String)> {
    let defs = definitions(&buf.text);
    // Deleted from the end, the earlier ones keep their places.
    let mut ranges: Vec<Range<usize>> = defs.iter().map(|d| d.1.clone()).collect();
    ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
    for r in ranges {
        buf.delete(r.start, r.end);
    }
    defs.into_iter().map(|(l, _, t)| (l, t)).collect()
}

/// `org-footnote--set-label` at the reference or definition at `pos`.
fn set_label(buf: &mut Buf, pos: usize, label: &str) {
    let at = pos + 4;
    if at > buf.text.len() || !buf.text.is_char_boundary(at) {
        return;
    }
    buf.point = at;
    if buf.text[at..].starts_with(':') {
        buf.insert_at_point(label);
    } else {
        let len: usize = buf.text[at..]
            .chars()
            .take_while(|c| label_char(*c))
            .map(char::len_utf8)
            .sum();
        if len > 0 {
            buf.replace(at, at + len, label);
            buf.point = at + label.len();
        }
    }
}

/// `org-footnote-all-labels`.
fn all_labels(root: &SyntaxNode) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in root.descendants() {
        let l = match n.kind() {
            FOOTNOTE_REFERENCE => ast::FootnoteReference::cast(n).and_then(|r| r.label()),
            FOOTNOTE_DEFINITION => ast::FootnoteDefinition::cast(n).map(|d| d.label()),
            _ => None,
        };
        if let Some(l) = l
            && !out.contains(&l)
        {
            out.push(l);
        }
    }
    out
}

/// `org-footnote--allow-reference-p`.
fn allow_reference(text: &str, root: &SyntaxNode, pos: usize) -> bool {
    if pos == 0 || text.as_bytes()[pos - 1] == b'\n' {
        return false;
    }
    let Some(ctx) = context(root, pos) else {
        return true;
    };
    let k = ctx.kind();
    let r = range(&ctx);
    if k.is_element() && pos < usize::from(ast::post_affiliated(&ctx)) {
        return false;
    }
    if k == PARAGRAPH {
        return true;
    }
    if k == VERSE_BLOCK {
        return ast::contents_range(&ctx)
            .is_some_and(|c| usize::from(c.start()) <= pos && pos < usize::from(c.end()));
    }
    if matches!(k, HEADLINE | INLINETASK) {
        let line_start = bol(text, pos);
        let line = &text[line_start..eol(text, line_start)];
        if !is_heading_line(line) {
            return true;
        }
        if line[heading_level(line)..]
            .trim()
            .eq_ignore_ascii_case("END")
        {
            return false;
        }
        let Some(h) = ast::Headline::cast(ctx.clone()) else {
            return false;
        };
        let Some(title) = ctx.children().find(|c| c.kind() == HEADLINE_TITLE) else {
            return false;
        };
        let tr = range(&title);
        if tr.start == tr.end {
            return false;
        }
        let tags = if h.tags().is_empty() {
            None
        } else {
            line.rfind(" :").map(|i| line_start + i + 1)
        };
        return pos >= tr.start && tags.is_none_or(|t| pos < t);
    }
    let end = {
        let e = skip_back(text, r.end, b" \r\t\n");
        if k.is_object() {
            e
        } else {
            forward_line(text, e)
        }
    };
    if pos >= end {
        return true;
    }
    if k == FOOTNOTE_DEFINITION {
        return text[pos..].chars().next().is_some_and(char::is_whitespace);
    }
    if k.is_element() {
        return false;
    }
    if pos == r.start {
        return true;
    }
    if k == LINK {
        return false;
    }
    let contents = ast::contents_range(&ctx).map(|c| usize::from(c.start())..usize::from(c.end()));
    if k == TABLE_CELL {
        let p = skip_back(text, pos, b" \t");
        return contents.is_some_and(|c| p <= c.end);
    }
    contents.is_some_and(|c| c.start <= pos && pos <= c.end)
}

/// `org-footnote-new`: a new numbered footnote at `point`, its definition
/// started in the footnote section (or at the end of the section), point
/// in it.
pub fn new(
    text: &str,
    point: usize,
    settings: &FootnoteSettings,
) -> Result<Transaction, EditError> {
    new_labeled(text, point, settings, None)
}

/// [`new`] with the label the user typed when the settings ask for one
/// (`answer`; empty for an anonymous footnote, `fn:` taken off).
pub fn new_labeled(
    text: &str,
    point: usize,
    settings: &FootnoteSettings,
    answer: Option<&str>,
) -> Result<Transaction, EditError> {
    let root = root(text);
    if !allow_reference(text, &root, point) {
        return Err(EditError::new("Cannot insert a footnote here"));
    }
    let labels = all_labels(&root);
    let propose = proposed_label(text);
    let normalize = |l: &str| {
        let l = l.trim();
        let l = l.strip_prefix("fn:").unwrap_or(l);
        (!l.is_empty()).then(|| l.to_string())
    };
    let label = match settings.auto_label {
        AutoLabel::Anonymous => None,
        AutoLabel::Random => {
            use std::hash::{BuildHasher, Hasher};
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_usize(point);
            Some(format!("{:x}", h.finish() >> 1))
        }
        AutoLabel::Auto => Some(propose),
        AutoLabel::Prompt | AutoLabel::Confirm => match answer {
            Some(a) => normalize(a),
            None => Some(propose),
        },
    };
    let mut buf = Buf::new(text, point);
    let Some(label) = label else {
        buf.insert_at_point("[fn::]");
        buf.point -= 1;
        return Ok(buf.transaction("New Footnote"));
    };
    if labels.contains(&label) {
        buf.insert_at_point(&format!("[fn:{label}]"));
        return Ok(buf.transaction("New Footnote"));
    }
    if settings.define_inline {
        buf.insert_at_point(&format!("[fn:{label}:]"));
        buf.point -= 1;
        return adjusted(text, point, buf.transaction("New Footnote"), settings);
    }
    buf.insert_at_point(&format!("[fn:{label}]"));
    let at = create_definition(&mut buf, &label, settings);
    buf.point = at + format!("[fn:{label}]").len();
    adjusted(text, point, buf.transaction("New Footnote"), settings)
}

/// `org-footnote-renumber-fn:N`: numbered footnotes numbered again in the
/// order of their references.
pub fn renumber(text: &str, point: usize) -> Result<Transaction, EditError> {
    let refs: Vec<Reference> = collect_references(text, false)
        .into_iter()
        .filter(|r| {
            r.label
                .as_ref()
                .is_some_and(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    let mut map: Vec<(String, String)> = Vec::new();
    for r in &refs {
        let Some(l) = r.label.clone() else { continue };
        if !map.iter().any(|(k, _)| *k == l) {
            let n = map.len() + 1;
            map.push((l, n.to_string()));
        }
    }
    let mut c = map.len();
    let mut buf = Buf::new(text, point);
    let saved = buf.add_marker(point);
    let markers: Vec<usize> = refs.iter().map(|r| buf.add_marker(r.begin)).collect();
    for (r, m) in refs.iter().zip(markers) {
        let Some(l) = r.label.as_ref() else { continue };
        let Some((_, new)) = map.iter().find(|(k, _)| k == l) else {
            continue;
        };
        let pos = buf.marker(m);
        set_label(&mut buf, pos, new);
    }
    // Definitions at line starts.
    let mut p = 0;
    while p < buf.text.len() {
        let line_end = eol(&buf.text, p);
        let line = buf.text[p..line_end].to_string();
        if let Some(rest) = line.strip_prefix("[fn:") {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && rest.as_bytes().get(digits) == Some(&b']') {
                let old = &rest[..digits];
                let new = match map.iter().find(|(k, _)| k == old) {
                    Some((_, v)) => v.clone(),
                    None => {
                        c += 1;
                        c.to_string()
                    }
                };
                buf.replace(p + 4, p + 4 + digits, &new);
            }
        }
        p = forward_line(&buf.text, p);
    }
    buf.point = buf.marker(saved);
    Ok(buf.transaction("Renumber Footnotes"))
}

/// `org-footnote-sort`: definitions in the order of their references, in
/// the footnote section or at the end of their sections.
pub fn sort(
    text: &str,
    point: usize,
    settings: &FootnoteSettings,
) -> Result<Transaction, EditError> {
    let refs = collect_references(text, false);
    let mut buf = Buf::new(text, point);
    let saved = buf.add_marker(point);
    let markers: Vec<usize> = refs.iter().map(|r| buf.add_marker(r.begin)).collect();
    let defs = take_definitions(&mut buf);
    clear_section(&mut buf, settings);
    let mut inserted: Vec<String> = Vec::new();
    for (r, m) in refs.iter().zip(&markers) {
        let Some(label) = r.label.clone() else {
            continue;
        };
        if inserted.contains(&label) || r.size.is_some() {
            continue;
        }
        inserted.push(label.clone());
        if settings.section.is_none() && r.top {
            buf.point = buf.marker(*m);
            goto_local_insertion_point(&mut buf);
        }
        let def = defs.iter().find(|(l, _)| *l == label).map_or_else(
            || format!("[fn:{label}] DEFINITION NOT FOUND."),
            |d| d.1.clone(),
        );
        buf.insert_at_point(&format!("\n{def}\n"));
    }
    for (label, def) in &defs {
        if !inserted.contains(label) {
            buf.insert_at_point(&format!("\n{def}\n"));
        }
    }
    buf.point = buf.marker(saved);
    Ok(buf.transaction("Sort Footnotes"))
}

/// A footnote while normalizing: its label, or its number when it was
/// anonymous.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Label(String),
    Anonymous(usize),
}

/// `org-footnote-normalize`: every footnote numbered in reading order,
/// inline definitions moved out, definitions sorted.
pub fn normalize(
    text: &str,
    point: usize,
    settings: &FootnoteSettings,
) -> Result<Transaction, EditError> {
    let refs = collect_references(text, true);
    let mut buf = Buf::new(text, point);
    let saved = buf.add_marker(point);
    let markers: Vec<usize> = refs.iter().map(|r| buf.add_marker(r.begin)).collect();
    let mut n = 0usize;
    let mut translations: Vec<(String, String)> = Vec::new();
    // Pushed to the front, as Org's list is.
    let mut defs: Vec<(Key, String)> = Vec::new();
    let mut keys: Vec<Key> = Vec::new();
    for (r, m) in refs.iter().zip(&markers) {
        let (key, new) = match &r.label {
            None => {
                n += 1;
                (Key::Anonymous(n), n.to_string())
            }
            Some(l) => match translations.iter().find(|(k, _)| k == l) {
                Some((_, v)) => (Key::Label(l.clone()), v.clone()),
                None => {
                    n += 1;
                    translations.push((l.clone(), n.to_string()));
                    (Key::Label(l.clone()), n.to_string())
                }
            },
        };
        keys.push(key.clone());
        let pos = buf.marker(*m);
        set_label(&mut buf, pos, &new);
        if let Some(size) = r.size {
            let p = buf.point;
            let mut end = (p + size + 1).min(buf.text.len());
            while !buf.text.is_char_boundary(end) {
                end -= 1;
            }
            let removed = buf.text[p..end].to_string();
            buf.delete(p, end);
            let body = removed.get(1..).unwrap_or("").trim();
            defs.insert(0, (key, format!("[fn:{new}] {body}")));
        }
    }
    for (l, t) in take_definitions(&mut buf) {
        defs.push((Key::Label(l), t));
    }
    clear_section(&mut buf, settings);
    let mut inserted: Vec<Key> = Vec::new();
    for ((r, m), key) in refs.iter().zip(&markers).zip(&keys) {
        if !(settings.section.is_some() || !r.top) {
            buf.point = buf.marker(*m);
            goto_local_insertion_point(&mut buf);
        }
        if inserted.contains(key) {
            continue;
        }
        inserted.push(key.clone());
        let stored = defs.iter().find(|(k, _)| k == key).map(|d| d.1.clone());
        let new = match key {
            Key::Anonymous(i) => i.to_string(),
            Key::Label(l) => translations
                .iter()
                .find(|(k, _)| k == l)
                .map(|t| t.1.clone())
                .unwrap_or_default(),
        };
        let def = match (stored, key) {
            (None, _) => format!("[fn:{new}] DEFINITION NOT FOUND."),
            (Some(s), Key::Anonymous(_)) => s,
            (Some(s), Key::Label(_)) => match s.strip_prefix("[fn:").and_then(|x| x.find(']')) {
                Some(i) => format!("[fn:{new}{}", &s[4 + i..]),
                None => s,
            },
        };
        buf.insert_at_point(&format!("\n{def}\n"));
    }
    for (key, def) in &defs {
        if inserted.contains(key) {
            continue;
        }
        n += 1;
        let renamed = def
            .split_inclusive('\n')
            .map(|l| match l.strip_prefix("[fn:") {
                Some(rest) => {
                    let len: usize = rest
                        .chars()
                        .take_while(|c| label_char(*c))
                        .map(char::len_utf8)
                        .sum();
                    if len > 0 && rest[len..].starts_with(']') {
                        format!("[fn:{n}]{}", &rest[len + 1..])
                    } else {
                        l.to_string()
                    }
                }
                None => l.to_string(),
            })
            .collect::<String>();
        buf.insert_at_point(&format!("\n{renamed}\n"));
    }
    buf.point = buf.marker(saved);
    Ok(buf.transaction("Normalize Footnotes"))
}

/// `org-footnote-at-reference-p`: the reference at `pos`, its label and
/// range (without the blanks after it).
fn reference_at(
    text: &str,
    root: &SyntaxNode,
    pos: usize,
) -> Option<(Option<String>, Range<usize>)> {
    let ctx = context(root, pos)?;
    let r = ast::FootnoteReference::cast(ctx.clone())?;
    let rr = range(&ctx);
    let end = skip_back(text, rr.end, b" \t");
    (pos < end).then(|| (r.label(), rr.start..end))
}

/// `org-footnote-at-definition-p`: the label of the definition at `pos`,
/// from after its affiliated keywords to the line after its text.
fn definition_at(text: &str, root: &SyntaxNode, pos: usize) -> Option<(String, Range<usize>)> {
    let el = crate::narrow::element_at(root, pos)?;
    let d = std::iter::once(el.clone())
        .chain(el.ancestors())
        .find(|a| a.kind() == FOOTNOTE_DEFINITION)?;
    let label = ast::FootnoteDefinition::cast(d.clone())?.label();
    let begin = usize::from(ast::post_affiliated(&d));
    let end = forward_line(text, skip_back(text, range(&d).end, b" \r\t\n"));
    Some((label, begin..end))
}

/// Starts of `[fn:LABEL]` and `[fn:LABEL:` in `text`, from `from`.
fn label_matches(text: &str, label: &str) -> Vec<(usize, usize)> {
    let pat = format!("[fn:{label}");
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(j) = text[i..].find(&pat) {
        let s = i + j;
        let after = s + pat.len();
        if matches!(text.as_bytes().get(after), Some(b']' | b':')) {
            out.push((s, after + 1));
        }
        i = s + 1;
    }
    out
}

/// `org-footnote-get-next-reference` for `label`, from `from`.
fn next_reference(text: &str, label: &str, from: usize) -> Option<Range<usize>> {
    let root = root(text);
    label_matches(text, label)
        .into_iter()
        .filter(|(_, e)| *e > from)
        .find_map(|(_, e)| reference_at(text, &root, e - 1).map(|r| r.1))
}

/// [`delete`], then the footnotes renumbered and sorted when the
/// settings ask for it (`org-footnote-auto-adjust`).
pub fn delete_adjusted(
    text: &str,
    point: usize,
    settings: &FootnoteSettings,
) -> Result<Transaction, EditError> {
    adjusted(text, point, delete(text, point)?, settings)
}

/// `org-footnote-delete`: the footnote at `point`, its references and its
/// definitions.
pub fn delete(text: &str, point: usize) -> Result<Transaction, EditError> {
    let root = root(text);
    let mut buf = Buf::new(text, point);
    let label = match reference_at(text, &root, point) {
        Some((None, r)) => {
            buf.delete(r.start, r.end);
            return Ok(buf.transaction("Delete Footnote"));
        }
        Some((Some(l), _)) => l,
        None => match definition_at(text, &root, point) {
            Some((l, _)) => l,
            None => return Err(EditError::new("Don't know which footnote to remove")),
        },
    };
    let saved = buf.add_marker(point);
    let mut from = 0;
    while let Some(r) = next_reference(&buf.text, &label, from) {
        buf.delete(r.start, r.end);
        from = r.start;
    }
    loop {
        let root = self::root(&buf.text);
        let found = label_matches(&buf.text, &label)
            .into_iter()
            .filter(|(s, e)| {
                buf.text.as_bytes()[e - 1] == b']'
                    && (*s == 0 || buf.text.as_bytes()[s - 1] == b'\n')
            })
            .find_map(|(_, e)| definition_at(&buf.text, &root, e));
        let Some((_, r)) = found else { break };
        let b = skip_back(&buf.text, r.start, b" \r\t\n");
        let start = if b == 0 {
            0
        } else {
            forward_line(&buf.text, b)
        };
        let e = skip_back(&buf.text, r.end, b" \r\t\n");
        let end = if e == 0 {
            0
        } else {
            forward_line(&buf.text, e)
        };
        if end <= start {
            break;
        }
        buf.delete(start, end);
    }
    buf.point = buf.marker(saved);
    Ok(buf.transaction("Delete Footnote"))
}

/// `org-footnote-get-definition`: where `label`'s definition starts (a
/// definition, or an inline footnote with that label).
fn definition_start(text: &str, label: &str) -> Option<usize> {
    let root = root(text);
    for (s, e) in label_matches(text, label) {
        let at_bol = s == 0 || text.as_bytes()[s - 1] == b'\n';
        let close = text.as_bytes()[e - 1];
        if !((at_bol && close == b']') || (!at_bol && close == b':')) {
            continue;
        }
        let ctx = context(&root, e - 1)?;
        if matches!(ctx.kind(), FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE) {
            return Some(range(&ctx).start);
        }
    }
    None
}

/// `org-footnote-action`: from a reference to its definition, from a
/// definition to its closest reference, elsewhere a new footnote.
pub fn action(
    text: &str,
    point: usize,
    settings: &FootnoteSettings,
) -> Result<Transaction, EditError> {
    let root = root(text);
    let ctx = context(&root, point);
    let mut buf = Buf::new(text, point);
    if let Some(c) = &ctx {
        let end = skip_back(text, range(c).end, b" \t");
        if point > end {
            return new(text, point, settings);
        }
    }
    let kind: Option<SyntaxKind> = ctx.as_ref().map(SyntaxNode::kind);
    match kind {
        Some(FOOTNOTE_REFERENCE) => {
            #[expect(
                clippy::expect_used,
                reason = "the match on its kind found a reference"
            )]
            let c = ctx.expect("a context");
            #[expect(
                clippy::expect_used,
                reason = "the match on its kind found a reference"
            )]
            let r = ast::FootnoteReference::cast(c.clone()).expect("a reference");
            match r.label() {
                None => {
                    buf.point =
                        ast::contents_range(&c).map_or(range(&c).start, |x| usize::from(x.start()));
                }
                // Without a definition, Org asks whether to make one; the
                // answer "no" leaves everything as it is.
                Some(l) => {
                    if let Some(p) = definition_start(text, &l) {
                        buf.point = p + format!("[fn:{l}").len() + 1;
                    }
                }
            }
            Ok(buf.transaction("Footnote"))
        }
        Some(FOOTNOTE_DEFINITION) => {
            #[expect(
                clippy::expect_used,
                reason = "the match on its kind found a definition"
            )]
            let l = ast::FootnoteDefinition::cast(ctx.expect("a context"))
                .expect("a definition")
                .label();
            // The closest reference before point, else the next one.
            let matches = label_matches(text, &l);
            let before = matches
                .iter()
                .rev()
                .filter(|(s, _)| *s < point)
                .find_map(|(s, _)| reference_at(text, &root, *s).map(|r| r.1.start));
            let start = before.or_else(|| {
                matches
                    .iter()
                    .filter(|(_, e)| *e > point)
                    .find_map(|(_, e)| reference_at(text, &root, e - 1).map(|r| r.1.start))
            });
            let Some(start) = start else {
                return Err(EditError::new(&format!(
                    "Cannot find reference of footnote {l:?}"
                )));
            };
            buf.point = start;
            Ok(buf.transaction("Footnote"))
        }
        _ => new(text, point, settings),
    }
}

/// The footnote referred to at `pos`: its label (none for an anonymous
/// one) and its definition's text on one line (a label's definition, or
/// an inline footnote's), for previews.
pub fn preview(text: &str, pos: usize) -> Option<(Option<String>, String)> {
    let root = root(text);
    let (label, r) = reference_at(text, &root, pos)?;
    let def = match label.clone() {
        None => {
            let ctx = context(&root, r.start)?;
            let c = ast::contents_range(&ctx)?;
            text[usize::from(c.start())..usize::from(c.end())].to_string()
        }
        Some(l) => {
            let start = definition_start(text, &l)?;
            let ctx = context(&root, start + 1)?;
            match ctx.kind() {
                FOOTNOTE_DEFINITION => {
                    let c = ast::contents_range(&ctx)?;
                    text[usize::from(c.start())..usize::from(c.end())].to_string()
                }
                _ => {
                    let c = ast::contents_range(&ctx)?;
                    text[usize::from(c.start())..usize::from(c.end())].to_string()
                }
            }
        }
    };
    let one_line = def.split_whitespace().collect::<Vec<_>>().join(" ");
    (!one_line.is_empty()).then_some((label, one_line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews() {
        let t = "A[fn:1] and B[fn:: inline one] and C[fn:x: named\n inline].\n\n[fn:1] The   first\nnote.\n";
        let text = |p: usize| preview(t, p).map(|x| x.1);
        assert_eq!(text(2).as_deref(), Some("The first note."));
        assert_eq!(preview(t, 2).and_then(|x| x.0).as_deref(), Some("1"));
        assert_eq!(text(15).as_deref(), Some("inline one"));
        assert_eq!(text(40).as_deref(), Some("named inline"));
        assert_eq!(preview(t, 0), None);
    }
}
