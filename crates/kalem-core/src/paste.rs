//! Paste (§6.3): plain text is inserted as it is, tab-separated values
//! become a table, and HTML from the clipboard is converted to Org by a
//! simple converter. Inside source blocks and other verbatim text, and in
//! documents that are not Org, everything is plain text.

use std::ops::Range;

use org_model::Document;
use org_syntax::{SyntaxKind, SyntaxNode};

/// What a paste does to the document: `range` is replaced by `text`, and
/// the caret goes to `cursor` bytes into `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insertion {
    /// The replaced range.
    pub range: Range<usize>,
    /// The new text, with `\n` line breaks.
    pub text: String,
    /// The caret, relative to the start of `range`.
    pub cursor: usize,
}

impl Insertion {
    /// `text` in place of `range`, with the caret after it.
    pub fn plain(range: Range<usize>, text: &str) -> Insertion {
        Insertion {
            range,
            text: text.to_string(),
            cursor: text.len(),
        }
    }
}

/// The paste of `text` (with `\n` line breaks) and, when the clipboard has
/// it, `html`, over the selection `sel` of the Org document `doc`.
pub fn plan(doc: &Document, sel: Range<usize>, text: &str, html: Option<&str>) -> Insertion {
    let root = doc.parse().syntax();
    let full = root.to_string();
    if verbatim_at(&root, sel.start) {
        return Insertion::plain(sel, text);
    }
    if let Some(rows) = tsv(text) {
        if sel.is_empty()
            && let Some(ins) = rows_into_table(&root, &full, sel.start, &rows)
        {
            return ins;
        }
        return table_at(&full, sel, &rows);
    }
    match html.and_then(html_to_org) {
        Some(org) => blocks_at(&full, sel, &align_tables(&org)),
        None => Insertion::plain(sel, text),
    }
}

/// Whether `pos` is in text that is taken literally: the contents of a
/// source, example, export or comment block, fixed-width lines, a LaTeX
/// environment, or code and verbatim objects.
fn verbatim_at(root: &SyntaxNode, pos: usize) -> bool {
    use SyntaxKind::*;
    let inside = |n: &SyntaxNode| {
        let r = n.text_range();
        usize::from(r.start()) < pos && pos < usize::from(r.end())
    };
    if let Some(el) = org_edit::narrow::element_at(root, pos)
        && matches!(
            el.kind(),
            SRC_BLOCK
                | EXAMPLE_BLOCK
                | EXPORT_BLOCK
                | COMMENT_BLOCK
                | FIXED_WIDTH
                | LATEX_ENVIRONMENT
        )
        && inside(&el)
    {
        return true;
    }
    root.descendants()
        .filter(|n| matches!(n.kind(), CODE | VERBATIM | INLINE_SRC_BLOCK))
        .any(|n| inside(&n))
}

/// Tab-separated values, as spreadsheets put them on the clipboard: at
/// least two columns, the same number in every row, and not every row
/// starting with a tab (which is indented code). Quoted fields may hold
/// tabs, line breaks and doubled quotes.
pub fn tsv(text: &str) -> Option<Vec<Vec<String>>> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    if !text.contains('\t') {
        return None;
    }
    let rows = split_tsv(text).unwrap_or_else(|| {
        text.split('\n')
            .map(|l| l.split('\t').map(str::to_string).collect())
            .collect()
    });
    let n = rows.first()?.len();
    if n < 2 || rows.iter().any(|r| r.len() != n) || rows.iter().all(|r| r[0].is_empty()) {
        return None;
    }
    Some(
        rows.into_iter()
            .map(|r| r.into_iter().map(|c| cell(&c)).collect())
            .collect(),
    )
}

/// Splits TSV with quoted fields; `None` for an unclosed quote.
fn split_tsv(text: &str) -> Option<Vec<Vec<String>>> {
    let mut rows = vec![Vec::new()];
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut start = true;
    while let Some(c) = chars.next() {
        match c {
            '"' if start => {
                loop {
                    match chars.next()? {
                        '"' if chars.peek() == Some(&'"') => {
                            chars.next();
                            field.push('"');
                        }
                        '"' => break,
                        c => field.push(c),
                    }
                }
                start = false;
            }
            '\t' => {
                rows.last_mut()?.push(std::mem::take(&mut field));
                start = true;
            }
            '\n' => {
                rows.last_mut()?.push(std::mem::take(&mut field));
                rows.push(Vec::new());
                start = true;
            }
            c => {
                field.push(c);
                start = false;
            }
        }
    }
    rows.last_mut()?.push(field);
    Some(rows)
}

/// A table field: one line, trimmed, with `|` written as `\vert{}`.
fn cell(text: &str) -> String {
    let one_line: Vec<&str> = text.split_whitespace().collect();
    one_line.join(" ").replace('|', "\\vert{}")
}

/// Table rows, `indent`ed and not aligned, without a final line feed. A
/// `None` row is a rule.
fn table_text(rows: &[Option<Vec<String>>], indent: &str) -> String {
    rows.iter()
        .map(|r| match r {
            Some(cells) => format!("{indent}| {} |", cells.join(" | ")),
            None => format!("{indent}|-"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Aligns every table in `text` with `org-table-align`.
pub fn align_tables(text: &str) -> String {
    let starts: Vec<usize> = org_syntax::parse(text)
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TABLE)
        .map(|n| usize::from(n.text_range().start()))
        .collect();
    let mut out = text.to_string();
    for &start in starts.iter().rev() {
        let first = start + out[start..].find('|').unwrap_or(0);
        let doc = Document::new(org_syntax::parse(&out));
        if let Ok(t) = org_edit::table::align_table(&doc, first) {
            out = t.apply(&out);
        }
    }
    // Aligning ends a table at the end of the text with a line feed.
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn bol(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

/// The end of the line at `pos`, before a CR LF pair's CR.
fn eol(text: &str, pos: usize) -> usize {
    let e = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    if e > pos && text.as_bytes()[e - 1] == b'\r' {
        e - 1
    } else {
        e
    }
}

fn indentation(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// A new table in place of `sel`, on lines of its own, indented like the
/// line.
fn table_at(full: &str, sel: Range<usize>, rows: &[Vec<String>]) -> Insertion {
    let b = bol(full, sel.start);
    let e = eol(full, sel.end);
    let before = &full[b..sel.start];
    let after = &full[sel.end..e];
    let indent = indentation(&full[b..e]);
    let rows: Vec<Option<Vec<String>>> = rows.iter().cloned().map(Some).collect();
    let table = align_tables(&table_text(&rows, indent));
    let blank_before = before.trim().is_empty();
    let blank_after = after.trim().is_empty();
    let prefix = if blank_before { "" } else { "\n" };
    let suffix = if blank_after { "" } else { "\n" };
    Insertion {
        range: if blank_before { b } else { sel.start }..if blank_after { e } else { sel.end },
        text: format!("{prefix}{table}{suffix}"),
        cursor: prefix.len() + table.len(),
    }
}

/// Rows added below the row at `pos` when it is in an Org table, and the
/// table aligned.
fn rows_into_table(
    root: &SyntaxNode,
    full: &str,
    pos: usize,
    rows: &[Vec<String>],
) -> Option<Insertion> {
    let el = org_edit::narrow::element_at(root, pos)?;
    let table = el.ancestors().find(|n| n.kind() == SyntaxKind::TABLE)?;
    let start = usize::from(table.text_range().start());
    let row_bol = bol(full, pos);
    let row = &full[row_bol..eol(full, pos)];
    if !row.trim_start().starts_with('|') {
        return None;
    }
    // The rows: the lines from the table's start that begin with `|`.
    let mut end = start;
    while end < full.len() && full[end..].trim_start_matches([' ', '\t']).starts_with('|') {
        end = eol(full, end);
        end = full[end..].find('\n').map_or(full.len(), |i| end + i + 1);
    }
    let end = if end > start && full.as_bytes()[end - 1] == b'\n' {
        end - 1 - usize::from(end >= 2 && full.as_bytes()[end - 2] == b'\r')
    } else {
        end
    };
    let before = full[start..eol(full, pos)].replace('\r', "");
    let after = full[eol(full, pos)..end].replace('\r', "");
    let new: Vec<Option<Vec<String>>> = rows.iter().cloned().map(Some).collect();
    let text = format!("{before}\n{}{after}", table_text(&new, indentation(row)));
    let aligned = align_tables(&text);
    let last = before.matches('\n').count() + rows.len();
    let line_start = aligned
        .match_indices('\n')
        .nth(last.checked_sub(1)?)
        .map_or(0, |(i, _)| i + 1);
    let cursor = aligned[line_start..]
        .find('\n')
        .map_or(aligned.len(), |i| line_start + i);
    Some(Insertion {
        range: start..end,
        text: aligned,
        cursor,
    })
}

/// Whether an Org line only means what it says at the start of a line:
/// headlines, items, tables, keywords and blocks, rules, fixed width.
fn block_line(line: &str) -> bool {
    let t = line.trim_start();
    let stars = line.len() - line.trim_start_matches('*').len();
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    (stars > 0 && line[stars..].starts_with(' '))
        || t.starts_with("- ")
        || t.starts_with("+ ")
        || (digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")))
        || t.starts_with('|')
        || t.starts_with("#+")
        || t.starts_with("-----")
        || t.starts_with(": ")
}

/// Converted Org text in place of `sel`, on a line of its own when it
/// starts or ends with something that needs one.
fn blocks_at(full: &str, sel: Range<usize>, org: &str) -> Insertion {
    let b = bol(full, sel.start);
    let e = eol(full, sel.end);
    let first = org.lines().next().unwrap_or("");
    let last = org.lines().last().unwrap_or("");
    let prefix = if !full[b..sel.start].trim().is_empty() && block_line(first) {
        "\n"
    } else {
        ""
    };
    let suffix = if !full[sel.end..e].trim().is_empty() && (org.contains('\n') || block_line(last))
    {
        "\n"
    } else {
        ""
    };
    let text = format!("{prefix}{org}{suffix}");
    Insertion {
        range: sel,
        cursor: prefix.len() + org.len(),
        text,
    }
}

// HTML.

/// A node of parsed HTML.
#[derive(Debug, Clone)]
enum Node {
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
    Text(String),
}

/// How deep elements nest; deeper ones are flattened into their parent.
const MAX_DEPTH: usize = 200;

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

const RAW: &[&str] = &["script", "style", "title", "textarea", "xmp", "template"];

const BLOCK: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "center",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "html",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "ul",
];

const SKIP: &[&str] = &[
    "head", "script", "style", "title", "meta", "link", "template", "noscript", "svg", "button",
    "select", "iframe", "object", "canvas", "textarea",
];

/// `&name;` and `&#N;` references.
fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map_or(rest.len(), |j| j + 1);
        let name = &rest[1..end];
        let c = if let Some(n) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            u32::from_str_radix(n, 16).ok().and_then(char::from_u32)
        } else if let Some(n) = name.strip_prefix('#') {
            n.parse().ok().and_then(char::from_u32)
        } else {
            entity(name)
        };
        match c {
            Some(c) => {
                if c != '\u{ad}' {
                    out.push(if c == '\u{a0}' { ' ' } else { c });
                }
                rest = &rest[end..];
                rest = rest.strip_prefix(';').unwrap_or(rest);
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" | "AMP" => '&',
        "lt" | "LT" => '<',
        "gt" | "GT" => '>',
        "quot" | "QUOT" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "shy" => '\u{ad}',
        "ensp" | "emsp" | "thinsp" => ' ',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "sbquo" => '‚',
        "bdquo" => '„',
        "laquo" => '«',
        "raquo" => '»',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "middot" => '·',
        "bull" => '•',
        "times" => '×',
        "divide" => '÷',
        "plusmn" => '±',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        "zwj" => '\u{200d}',
        "zwnj" => '\u{200c}',
        _ => return None,
    })
}

/// A forgiving HTML parser: unknown end tags are ignored, open elements
/// are closed by the end of their parent, and `li`, `p`, `tr`, `td`, `th`,
/// `dt`, `dd` and `option` close as HTML closes them.
fn parse_html(html: &str) -> Vec<Node> {
    struct Open {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    }
    let mut stack = vec![Open {
        name: String::new(),
        attrs: Vec::new(),
        children: Vec::new(),
    }];
    fn close(stack: &mut Vec<Open>) {
        if stack.len() > 1 {
            let o = stack.pop().expect("an open element");
            stack
                .last_mut()
                .expect("the root")
                .children
                .push(Node::Element {
                    name: o.name,
                    attrs: o.attrs,
                    children: o.children,
                });
        }
    }
    // Closes an open `name` up to the nearest of `scope`.
    fn close_open(stack: &mut Vec<Open>, names: &[&str], scope: &[&str]) {
        let found = stack
            .iter()
            .rposition(|o| names.contains(&o.name.as_str()) || scope.contains(&o.name.as_str()));
        if let Some(i) = found
            && i > 0
            && names.contains(&stack[i].name.as_str())
        {
            while stack.len() > i {
                close(stack);
            }
        }
    }
    let b = html.as_bytes();
    let mut i = 0;
    let mut text = String::new();
    let flush = |stack: &mut Vec<Open>, text: &mut String| {
        if !text.is_empty() {
            let t = decode(text);
            stack
                .last_mut()
                .expect("the root")
                .children
                .push(Node::Text(t));
            text.clear();
        }
    };
    while i < b.len() {
        if b[i] != b'<' {
            let next = html[i..].find('<').map_or(html.len(), |j| i + j);
            text.push_str(&html[i..next]);
            i = next;
            continue;
        }
        let rest = &html[i..];
        if rest.starts_with("<!--") {
            flush(&mut stack, &mut text);
            i = rest.find("-->").map_or(html.len(), |j| i + j + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            flush(&mut stack, &mut text);
            i = rest.find('>').map_or(html.len(), |j| i + j + 1);
            continue;
        }
        let closing = rest.starts_with("</");
        let name_start = i + 1 + usize::from(closing);
        let name_len = html[name_start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == ':' || c == '-'))
            .unwrap_or(html.len() - name_start);
        if name_len == 0 || !b[name_start].is_ascii_alphabetic() {
            text.push('<');
            i += 1;
            continue;
        }
        flush(&mut stack, &mut text);
        let name = html[name_start..name_start + name_len].to_ascii_lowercase();
        let mut j = name_start + name_len;
        if closing {
            i = html[j..].find('>').map_or(html.len(), |k| j + k + 1);
            if let Some(k) = stack.iter().rposition(|o| o.name == name)
                && k > 0
            {
                while stack.len() > k {
                    close(&mut stack);
                }
            }
            continue;
        }
        // Attributes.
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                break;
            }
            if b[j] == b'>' {
                j += 1;
                break;
            }
            if b[j] == b'/' {
                self_closing = html[j + 1..].starts_with('>');
                j += 1;
                continue;
            }
            let an = html[j..]
                .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '>' | '/'))
                .map_or(html.len(), |k| j + k);
            let key = html[j..an].to_ascii_lowercase();
            j = an;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let mut value = String::new();
            if j < b.len() && b[j] == b'=' {
                j += 1;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                    let q = b[j] as char;
                    let e = html[j + 1..].find(q).map_or(html.len(), |k| j + 1 + k);
                    value = decode(&html[j + 1..e]);
                    j = (e + 1).min(html.len());
                } else {
                    let e = html[j..]
                        .find(|c: char| c.is_ascii_whitespace() || c == '>')
                        .map_or(html.len(), |k| j + k);
                    value = decode(&html[j..e]);
                    j = e;
                }
            }
            if key.is_empty() {
                j += 1;
            } else {
                attrs.push((key, value));
            }
        }
        i = j;
        // Implied end tags.
        match name.as_str() {
            "li" => close_open(&mut stack, &["li"], &["ul", "ol"]),
            "dt" | "dd" => close_open(&mut stack, &["dt", "dd"], &["dl"]),
            "tr" => close_open(&mut stack, &["tr", "td", "th"], &["table"]),
            "td" | "th" => close_open(&mut stack, &["td", "th"], &["tr", "table"]),
            "option" => close_open(&mut stack, &["option"], &["select"]),
            _ => {}
        }
        if BLOCK.contains(&name.as_str()) && stack.last().is_some_and(|o| o.name == "p") {
            close(&mut stack);
        }
        if RAW.contains(&name.as_str()) {
            let end = format!("</{name}");
            let lower = html[i..].to_ascii_lowercase();
            let e = lower.find(&end).map_or(html.len(), |k| i + k);
            let children = vec![Node::Text(html[i..e].to_string())];
            stack
                .last_mut()
                .expect("the root")
                .children
                .push(Node::Element {
                    name,
                    attrs,
                    children,
                });
            i = html[e..].find('>').map_or(html.len(), |k| e + k + 1);
            continue;
        }
        // Deeper elements join their parent, which keeps the converter's
        // recursion bounded.
        if stack.len() > MAX_DEPTH {
            continue;
        }
        if self_closing || VOID.contains(&name.as_str()) {
            stack
                .last_mut()
                .expect("the root")
                .children
                .push(Node::Element {
                    name,
                    attrs,
                    children: Vec::new(),
                });
        } else {
            stack.push(Open {
                name,
                attrs,
                children: Vec::new(),
            });
        }
    }
    flush(&mut stack, &mut text);
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().map(|o| o.children).unwrap_or_default()
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Emphasis from a `style` attribute (Google Docs and others write it
/// on spans).
#[derive(Default, Clone, Copy)]
struct Styled {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: bool,
    strike: bool,
    sub: bool,
    sup: bool,
    pre: bool,
}

fn styled(attrs: &[(String, String)]) -> Styled {
    let mut s = Styled::default();
    for decl in attr(attrs, "style").unwrap_or("").split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase());
        match k.as_str() {
            "font-weight" => {
                s.bold = Some(matches!(
                    v.as_str(),
                    "bold" | "bolder" | "600" | "700" | "800" | "900"
                ))
            }
            "font-style" => s.italic = Some(v == "italic" || v == "oblique"),
            "text-decoration" | "text-decoration-line" => {
                s.underline |= v.contains("underline");
                s.strike |= v.contains("line-through");
            }
            "vertical-align" => {
                s.sub |= v == "sub";
                s.sup |= v == "super";
            }
            "white-space" => s.pre |= v.starts_with("pre"),
            _ => {}
        }
    }
    s
}

/// Whether the HTML says more than its plain text: headings, paragraphs,
/// emphasis, links, lists, tables, blocks. Code editors put their text in
/// styled `div`s and `span`s with `white-space: pre`; that is plain text.
fn meaningful(nodes: &[Node]) -> bool {
    fn walk(nodes: &[Node], found: &mut bool, pre: &mut bool) {
        for n in nodes {
            let Node::Element {
                name,
                attrs,
                children,
            } = n
            else {
                continue;
            };
            if SKIP.contains(&name.as_str()) {
                continue;
            }
            let s = styled(attrs);
            *pre |= s.pre && name != "pre";
            let semantic = matches!(
                name.as_str(),
                "h1" | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "p"
                    | "b"
                    | "strong"
                    | "i"
                    | "em"
                    | "u"
                    | "s"
                    | "strike"
                    | "del"
                    | "ins"
                    | "code"
                    | "kbd"
                    | "pre"
                    | "ul"
                    | "ol"
                    | "li"
                    | "dl"
                    | "table"
                    | "blockquote"
                    | "img"
                    | "hr"
                    | "sub"
                    | "sup"
            ) || (name == "a" && attr(attrs, "href").is_some())
                || s.bold == Some(true)
                || s.italic == Some(true)
                || s.underline
                || s.strike;
            *found |= semantic;
            walk(children, found, pre);
        }
    }
    let (mut found, mut pre) = (false, false);
    walk(nodes, &mut found, &mut pre);
    found && !pre
}

/// Converts HTML to Org: headings, paragraphs and line breaks, emphasis,
/// code, links, images, lists (with checkboxes), description lists,
/// tables, preformatted text, quotes and rules. `None` when the HTML says
/// no more than the plain text beside it.
pub fn html_to_org(html: &str) -> Option<String> {
    let nodes = parse_html(html);
    if !meaningful(&nodes) {
        return None;
    }
    let out = join_blocks(&blocks(&nodes, 0));
    let out = out.trim_matches('\n').to_string();
    (!out.trim().is_empty()).then_some(out)
}

/// A converted block: its lines, and whether it is a list (lists that
/// follow each other are not separated by a blank line).
struct Block {
    text: String,
    list: bool,
}

fn join_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str(if b.list && blocks[i - 1].list {
                "\n"
            } else {
                "\n\n"
            });
        }
        out.push_str(&b.text);
    }
    out
}

fn has_block(nodes: &[Node]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Element { name, children, .. } => {
            BLOCK.contains(&name.as_str()) || has_block(children)
        }
        Node::Text(_) => false,
    })
}

/// The blocks of `nodes`; `depth` is the list nesting.
fn blocks(nodes: &[Node], depth: usize) -> Vec<Block> {
    let mut out = Vec::new();
    let mut inline = Vec::new();
    let flush = |inline: &mut Vec<Node>, out: &mut Vec<Block>| {
        if !inline.is_empty() {
            let p = paragraph(inline);
            if !p.is_empty() {
                out.push(Block {
                    text: p,
                    list: false,
                });
            }
            inline.clear();
        }
    };
    for n in nodes {
        let Node::Element { name, children, .. } = n else {
            inline.push(n.clone());
            continue;
        };
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        if !BLOCK.contains(&name.as_str()) && !has_block(children) {
            inline.push(n.clone());
            continue;
        }
        flush(&mut inline, &mut out);
        match name.as_str() {
            h if h.len() == 2 && h.starts_with('h') && h.as_bytes()[1].is_ascii_digit() => {
                let level = usize::from(h.as_bytes()[1] - b'0');
                let title = paragraph(children).replace("\\\\\n", " ");
                if title.is_empty() {
                    continue;
                }
                let text = if depth == 0 {
                    format!("{} {title}", "*".repeat(level))
                } else {
                    wrap('*', &title)
                };
                out.push(Block { text, list: false });
            }
            "ul" | "ol" => out.push(Block {
                text: list(n, depth),
                list: true,
            }),
            "li" => out.push(Block {
                text: item("- ", children, depth),
                list: true,
            }),
            "dl" => out.push(Block {
                text: description_list(children, depth),
                list: true,
            }),
            "table" => out.push(Block {
                text: table(children),
                list: false,
            }),
            "pre" => out.push(Block {
                text: preformatted(n),
                list: false,
            }),
            "hr" => out.push(Block {
                text: "-----".into(),
                list: false,
            }),
            "blockquote" => {
                let inner = join_blocks(&blocks(children, depth));
                if !inner.trim().is_empty() {
                    out.push(Block {
                        text: format!("#+begin_quote\n{inner}\n#+end_quote"),
                        list: false,
                    });
                }
            }
            _ => out.extend(blocks(children, depth)),
        }
    }
    flush(&mut inline, &mut out);
    out
}

fn indent(text: &str, n: usize) -> String {
    let pad = " ".repeat(n);
    text.lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn list(node: &Node, depth: usize) -> String {
    let Node::Element {
        name,
        attrs,
        children,
    } = node
    else {
        return String::new();
    };
    let ordered = name == "ol";
    let mut n: i64 = attr(attrs, "start")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);
    let mut lines: Vec<String> = Vec::new();
    let mut last_bullet = 2;
    for c in children {
        match c {
            Node::Element {
                name, children: ch, ..
            } if name == "li" => {
                let bullet = if ordered {
                    format!("{n}. ")
                } else {
                    "- ".to_string()
                };
                n += 1;
                last_bullet = bullet.len();
                lines.push(item(&bullet, ch, depth));
            }
            Node::Element { name, .. } if name == "ul" || name == "ol" => {
                // A list right inside a list belongs to the item before it.
                let sub = list(c, depth + 1);
                if !sub.is_empty() {
                    lines.push(indent(&sub, last_bullet));
                }
            }
            Node::Text(t) if t.trim().is_empty() => {}
            other => {
                let p = paragraph(std::slice::from_ref(other));
                if !p.is_empty() {
                    lines.push(format!("- {p}"));
                }
            }
        }
    }
    lines.join("\n")
}

/// A list item: `bullet`, a checkbox from an `<input type=checkbox>`, and
/// its blocks, the later ones indented under the first.
fn item(bullet: &str, children: &[Node], depth: usize) -> String {
    let mut checkbox = None;
    let rest: Vec<Node> = children
        .iter()
        .filter(|c| {
            if let Node::Element { name, attrs, .. } = c
                && name == "input"
                && attr(attrs, "type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox"))
            {
                checkbox = Some(attr(attrs, "checked").is_some());
                return false;
            }
            true
        })
        .cloned()
        .collect();
    let box_ = match checkbox {
        Some(true) => "[X] ",
        Some(false) => "[ ] ",
        None => "",
    };
    let bs = blocks(&rest, depth + 1);
    let mut out = format!("{bullet}{box_}");
    for (i, b) in bs.iter().enumerate() {
        if i == 0 && !b.list {
            out.push_str(indent(&b.text, bullet.len()).trim_start());
        } else {
            out.push('\n');
            out.push_str(&indent(&b.text, bullet.len()));
        }
    }
    out.trim_end().to_string()
}

fn description_list(children: &[Node], depth: usize) -> String {
    let mut lines = Vec::new();
    let mut term: Option<String> = None;
    for c in children {
        let Node::Element {
            name, children: ch, ..
        } = c
        else {
            continue;
        };
        match name.as_str() {
            "dt" => {
                if let Some(t) = term.take() {
                    lines.push(format!("- {t} ::"));
                }
                term = Some(paragraph(ch).replace("\\\\\n", " "));
            }
            "dd" => {
                let body = join_blocks(&blocks(ch, depth + 1));
                let t = term.take().unwrap_or_default();
                lines.push(
                    format!("- {t} :: {}", indent(&body, 2).trim_start())
                        .trim_end()
                        .to_string(),
                );
            }
            _ => {}
        }
    }
    if let Some(t) = term {
        lines.push(format!("- {t} ::"));
    }
    lines.join("\n")
}

/// A table's rows, the first followed by a rule when it is made of
/// headers and more rows follow; aligned later.
fn table(children: &[Node]) -> String {
    fn rows(nodes: &[Node], out: &mut Vec<(Vec<String>, bool)>, caption: &mut Option<String>) {
        for n in nodes {
            let Node::Element { name, children, .. } = n else {
                continue;
            };
            match name.as_str() {
                "tr" => {
                    let mut cells = Vec::new();
                    let mut header = true;
                    for c in children {
                        if let Node::Element { name, children, .. } = c
                            && (name == "td" || name == "th")
                        {
                            header &= name == "th";
                            let text = join_blocks(&blocks(children, 1));
                            cells.push(cell(&text.replace("\\\\\n", " ")));
                        }
                    }
                    if !cells.is_empty() {
                        out.push((cells, header));
                    }
                }
                "caption" => *caption = Some(paragraph(children).replace("\\\\\n", " ")),
                "thead" | "tbody" | "tfoot" => rows(children, out, caption),
                _ => {}
            }
        }
    }
    let mut rs = Vec::new();
    let mut caption = None;
    rows(children, &mut rs, &mut caption);
    let mut lines: Vec<Option<Vec<String>>> = Vec::new();
    let header = rs.first().is_some_and(|r| r.1) && rs.len() > 1;
    for (i, (cells, _)) in rs.into_iter().enumerate() {
        lines.push(Some(cells));
        if i == 0 && header {
            lines.push(None);
        }
    }
    let t = table_text(&lines, "");
    match caption.filter(|c| !c.is_empty()) {
        Some(c) => format!("#+caption: {c}\n{t}"),
        None => t,
    }
}

/// The text of `nodes` as it is, with `<br>` as a line break.
fn raw_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::Element { name, .. } if name == "br" => out.push('\n'),
            Node::Element { name, .. } if SKIP.contains(&name.as_str()) => {}
            Node::Element { children, .. } => raw_text(children, out),
        }
    }
}

/// The language of a code block from `class="language-rust"` or
/// `lang-rust` on the `pre` or the `code` in it.
fn language(node: &Node) -> Option<String> {
    let Node::Element {
        attrs, children, ..
    } = node
    else {
        return None;
    };
    let from = |attrs: &[(String, String)]| {
        attr(attrs, "class")?.split_whitespace().find_map(|c| {
            c.strip_prefix("language-")
                .or_else(|| c.strip_prefix("lang-"))
                .map(str::to_string)
        })
    };
    from(attrs).or_else(|| {
        children.iter().find_map(|c| match c {
            Node::Element { name, attrs, .. } if name == "code" => from(attrs),
            _ => None,
        })
    })
}

/// `org-escape-code-in-string`: a comma before lines that start with `*`
/// or `#+` (after blanks and commas).
fn escape_code(text: &str) -> String {
    text.split('\n')
        .map(|l| {
            let ws = indentation(l).len();
            let rest = l[ws..].trim_start_matches(',');
            if rest.starts_with('*') || rest.starts_with("#+") {
                format!("{},{}", &l[..ws], &l[ws..])
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn preformatted(node: &Node) -> String {
    let Node::Element { children, .. } = node else {
        return String::new();
    };
    let mut text = String::new();
    raw_text(children, &mut text);
    let text = text.replace("\r\n", "\n");
    let text = text.strip_prefix('\n').unwrap_or(&text).trim_end();
    let body = escape_code(text);
    match language(node) {
        Some(lang) => format!("#+begin_src {lang}\n{body}\n#+end_src"),
        None => format!("#+begin_example\n{body}\n#+end_example"),
    }
}

/// Emphasis around `s`, with the blanks at its ends kept outside.
fn wrap(m: char, s: &str) -> String {
    let core = s.trim_matches(|c: char| c == ' ' || c == '\n');
    if core.is_empty() {
        return s.to_string();
    }
    let lead = &s[..s.len() - s.trim_start_matches([' ', '\n']).len()];
    let trail = &s[s.trim_end_matches([' ', '\n']).len()..];
    format!("{lead}{m}{core}{m}{trail}")
}

#[derive(Default, Clone)]
struct Inline {
    /// Markers already open around the text.
    open: Vec<char>,
    link: bool,
}

/// Inline HTML as Org, with `\n` for `<br>`.
fn inline(nodes: &[Node], st: &Inline, out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => {
                let mut space = false;
                for c in t.chars() {
                    if c.is_ascii_whitespace() {
                        space = true;
                    } else {
                        if space {
                            out.push(' ');
                            space = false;
                        }
                        out.push(c);
                    }
                }
                if space {
                    out.push(' ');
                }
            }
            Node::Element {
                name,
                attrs,
                children,
            } => {
                if SKIP.contains(&name.as_str()) {
                    continue;
                }
                let s = styled(attrs);
                let marker = match name.as_str() {
                    "b" | "strong" => (s.bold != Some(false)).then_some('*'),
                    "i" | "em" | "cite" | "dfn" | "var" => (s.italic != Some(false)).then_some('/'),
                    "u" | "ins" => Some('_'),
                    "s" | "strike" | "del" => Some('+'),
                    _ if s.bold == Some(true) => Some('*'),
                    _ if s.italic == Some(true) => Some('/'),
                    _ if s.underline && !st.link => Some('_'),
                    _ if s.strike => Some('+'),
                    _ => None,
                };
                match name.as_str() {
                    "br" => out.push('\n'),
                    "img" => {
                        if let Some(src) = attr(attrs, "src").filter(|s| !s.starts_with("data:"))
                            && !st.link
                            && let Ok(l) = org_edit::insert::link_string(src, None)
                        {
                            out.push_str(&l);
                        } else if let Some(alt) = attr(attrs, "alt") {
                            out.push_str(alt);
                        }
                    }
                    "code" | "kbd" | "samp" | "tt" => {
                        let mut t = String::new();
                        raw_text(children, &mut t);
                        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                        let m = if t.contains('~') && !t.contains('=') {
                            '='
                        } else {
                            '~'
                        };
                        out.push_str(&wrap(m, &t));
                    }
                    "sub" | "sup" => {
                        let mut t = String::new();
                        inline(children, st, &mut t);
                        let t = t.trim();
                        if !t.is_empty() {
                            let m = if name == "sub" { '_' } else { '^' };
                            out.push_str(&format!("{m}{{{t}}}"));
                        }
                    }
                    "a" if !st.link => {
                        let mut d = String::new();
                        let inner = Inline {
                            link: true,
                            ..st.clone()
                        };
                        inline(children, &inner, &mut d);
                        let d = d.replace('\n', " ");
                        let href = attr(attrs, "href").unwrap_or("").trim();
                        let usable = !href.is_empty()
                            && !href.starts_with('#')
                            && !href.to_ascii_lowercase().starts_with("javascript:");
                        let desc = d.trim();
                        let link = usable
                            .then(|| {
                                org_edit::insert::link_string(
                                    href,
                                    (desc != href && !desc.is_empty()).then_some(desc),
                                )
                                .ok()
                            })
                            .flatten();
                        match link {
                            Some(l) => {
                                if d.starts_with(' ') {
                                    out.push(' ');
                                }
                                out.push_str(&l);
                                if d.ends_with(' ') && desc != d {
                                    out.push(' ');
                                }
                            }
                            None => out.push_str(&d),
                        }
                    }
                    _ => match marker.filter(|m| !st.open.contains(m)) {
                        Some(m) => {
                            let mut inner = st.clone();
                            inner.open.push(m);
                            let mut t = String::new();
                            inline(children, &inner, &mut t);
                            out.push_str(&wrap(m, &t));
                        }
                        None => {
                            let sub = if s.sub {
                                Some('_')
                            } else if s.sup {
                                Some('^')
                            } else {
                                None
                            };
                            let mut t = String::new();
                            inline(children, st, &mut t);
                            match sub.filter(|_| !t.trim().is_empty()) {
                                Some(m) => out.push_str(&format!("{m}{{{}}}", t.trim())),
                                None => out.push_str(&t),
                            }
                        }
                    },
                }
            }
        }
    }
}

/// A paragraph: inline text with collapsed blanks, `<br>` as `\\`.
fn paragraph(nodes: &[Node]) -> String {
    let mut s = String::new();
    inline(nodes, &Inline::default(), &mut s);
    let lines: Vec<String> = s
        .split('\n')
        .map(|l| {
            l.split(' ')
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    match (first, last) {
        (Some(a), Some(b)) => lines[a..=b].join("\\\\\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org(html: &str) -> String {
        html_to_org(html).unwrap_or_else(|| "<none>".into())
    }

    #[test]
    fn tab_separated() {
        assert_eq!(
            tsv("a\tb\n1\t2\n"),
            Some(vec![
                vec!["a".into(), "b".into()],
                vec!["1".into(), "2".into()]
            ])
        );
        assert_eq!(
            tsv("\"x\ty\"\t\"two\nlines\"\tq|r"),
            Some(vec![vec![
                "x y".into(),
                "two lines".into(),
                "q\\vert{}r".into()
            ]])
        );
        // Not tables: one column, ragged rows, indented code.
        assert_eq!(tsv("a\nb"), None);
        assert_eq!(tsv("a\tb\nc"), None);
        assert_eq!(tsv("\tfoo\n\tbar"), None);
        assert_eq!(tsv("fn main() {\n\tprintln!();\n}"), None);
    }

    #[test]
    fn html() {
        assert_eq!(
            org("<h1>Title</h1><p>Some <b>bold</b> and <em>it</em>.</p>"),
            "* Title\n\nSome *bold* and /it/."
        );
        assert_eq!(
            org("<p>A <a href=\"https://x.org/a]b\">link <b>here</b> </a>now</p>"),
            "A [[https://x.org/a\\]b][link *here*]] now"
        );
        assert_eq!(
            org("<ul><li>one<li>two<ul><li><input type=checkbox checked>sub</li></ul></li></ul>"),
            "- one\n- two\n  - [X] sub"
        );
        assert_eq!(
            org("<ol start=3><li><p>x</p></li><li>y</li></ol>"),
            "3. x\n4. y"
        );
        assert_eq!(
            org("<table><tr><th>A</th><th>Bee</th></tr><tr><td>1</td><td>22</td></tr></table>"),
            "| A | Bee |\n|-\n| 1 | 22 |"
        );
        assert_eq!(
            align_tables(&org(
                "<table><tr><th>A</th><th>Bee</th></tr><tr><td>1</td><td>22</td></tr></table>"
            )),
            "| A | Bee |\n|---+-----|\n| 1 |  22 |"
        );
        assert_eq!(
            org("<pre><code class=\"language-rust\">fn main() {\n* x\n}\n</code></pre>"),
            "#+begin_src rust\nfn main() {\n,* x\n}\n#+end_src"
        );
        assert_eq!(
            org("<p>line one<br>line&nbsp;two &amp; <code>a~b</code></p>"),
            "line one\\\\\nline two & =a~b="
        );
        assert_eq!(
            org("<blockquote><p>q</p></blockquote><hr><dl><dt>T</dt><dd>d</dd></dl>"),
            "#+begin_quote\nq\n#+end_quote\n\n-----\n\n- T :: d"
        );
        // Google Docs: a normal-weight `b` around everything, styled spans.
        assert_eq!(
            org(
                "<meta charset=\"utf-8\"><b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\">\
                 <p dir=\"ltr\"><span style=\"font-weight:700\">Bold</span><span> and </span>\
                 <span style=\"font-style:italic\">italic</span></p></b>"
            ),
            "*Bold* and /italic/"
        );
        assert_eq!(
            org("x<sup>2</sup> H<sub>2</sub>O <s> gone </s>"),
            "x^{2} H_{2}O +gone+"
        );
    }

    #[test]
    fn plain_html() {
        // A browser's copy of plain words, and a code editor's copy.
        assert_eq!(
            html_to_org("<span style=\"font-weight: 400; font-style: normal\">words</span>"),
            None
        );
        assert_eq!(
            html_to_org(
                "<div style=\"white-space: pre;\"><div><span style=\"font-style: italic;\">// c</span></div></div>"
            ),
            None
        );
        assert_eq!(html_to_org("<!-- x --><script>alert(1)</script>"), None);
    }

    proptest::proptest! {
        #[test]
        fn never_panics(
            html in "(<(/?)(p|b|a href=x|ul|li|ol|table|tr|td|th|pre|code|h2|br|img src=\"y\"|span style=\"font-weight:700\"|!--|!doctype)>|&(amp|#x4e2d|#99999999|nbsp);?|[a-zé中 \t\n|*~=\"'<>/&])*",
            text in "[a\t\n|é]*",
        ) {
            let _ = html_to_org(&html);
            let _ = tsv(&text);
            let doc = Document::new(org_syntax::parse("| x |\n#+begin_src\ny\n#+end_src\nz é\n"));
            for pos in [0, 2, 6, 17, 30] {
                let _ = plan(&doc, pos..pos, &text, Some(&html));
            }
        }
    }

    #[test]
    fn deep_nesting() {
        let html = "<div><b>".repeat(100_000) + "x";
        assert_eq!(html_to_org(&html).as_deref(), Some("*x*"));
    }

    fn paste(text: &str, sel: Range<usize>, clip: &str, html: Option<&str>) -> String {
        let doc = Document::new(org_syntax::parse(text));
        let ins = plan(&doc, sel, clip, html);
        let mut out = text.to_string();
        let caret = ins.range.start + ins.cursor;
        out.replace_range(ins.range, &ins.text);
        out.insert(caret, '^');
        out
    }

    #[test]
    fn placing() {
        // A table on lines of its own, indented like the line.
        assert_eq!(
            paste("text here\n", 4..4, "a\tbb\n1\t2\n", None),
            "text\n| a | bb |\n| 1 |  2 |^\n here\n"
        );
        assert_eq!(paste("  \n", 2..2, "a\tb", None), "  | a | b |^\n");
        // Rows go below the row at point, and the table is aligned.
        assert_eq!(
            paste("| x | y |\n| z | w |\n", 3..3, "long\t1\n", None),
            "| x    | y |\n| long | 1 |^\n| z    | w |\n"
        );
        // Verbatim contexts take the text as it is.
        assert_eq!(
            paste("#+begin_src sh\nx\n#+end_src\n", 15..15, "a\tb", None),
            "#+begin_src sh\na\tb^x\n#+end_src\n"
        );
        // HTML blocks start on a line of their own.
        assert_eq!(
            paste("ab\n", 1..1, "x", Some("<ul><li>x</li></ul>")),
            "a\n- x^\nb\n"
        );
        assert_eq!(paste("ab\n", 1..1, "x", Some("<b>x</b>")), "a*x*^b\n");
        assert_eq!(
            paste("ab\n", 1..1, "x\ty", Some("<b>x</b>")),
            "a\n| x | y |^\nb\n"
        );
    }
}
