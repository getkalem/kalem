//! Kalem's formatting beyond Org (design §9.5): the font, size, color and
//! highlight of text, and the alignment of paragraphs, as a word processor
//! has them.
//!
//! Every standard Org file works in Kalem as in Emacs; these are Kalem's
//! own additions, written in Org syntax that Emacs parses and leaves out
//! of its exports, so a Kalem file still opens, edits and exports in Emacs
//! (without the formatting):
//!
//! - A formatted span is an export snippet for the `kalem` back-end that
//!   starts it, `@@kalem:font="Georgia" size=14 color=#c00000 bg=#fff2a8@@`,
//!   and one that ends it, `@@kalem:end@@`. Spans stay inside one paragraph
//!   (or heading); nested spans combine, the inner one winning.
//! - A paragraph aligned right or justified has `#+ATTR_KALEM: :align
//!   right` (or `justify`) above it. Centered paragraphs use Org's own
//!   `#+begin_center` block.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use org_edit::Transaction;
use org_syntax::SyntaxKind::{self, *};
use org_syntax::{SyntaxNode, TextSize, ast};

use crate::theme::Color;

/// The export back-end of Kalem's snippets.
pub const BACKEND: &str = "kalem";

/// The snippet that ends a span.
pub const END: &str = "@@kalem:end@@";

/// A font family, interned so that styles stay `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontName(u32);

static FONTS: Mutex<Vec<Arc<str>>> = Mutex::new(Vec::new());

impl FontName {
    /// The name for `family`.
    pub fn new(family: &str) -> FontName {
        let mut fonts = FONTS.lock().expect("the font names");
        if let Some(i) = fonts.iter().position(|f| &**f == family) {
            return FontName(i as u32);
        }
        fonts.push(family.into());
        FontName(fonts.len() as u32 - 1)
    }

    /// The family.
    pub fn family(self) -> Arc<str> {
        FONTS.lock().expect("the font names")[self.0 as usize].clone()
    }
}

/// The formatting of a span of text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CharFormat {
    /// The font family.
    pub font: Option<FontName>,
    /// The size, in tenths of a point.
    pub size: Option<u16>,
    /// The text color.
    pub color: Option<Color>,
    /// The highlight (background) color.
    pub highlight: Option<Color>,
}

fn hex(c: Color) -> String {
    let (r, g, b) = c.rgb();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// A size in tenths of a point as written: `14`, `10.5`.
pub fn size_text(tenths: u16) -> String {
    if tenths.is_multiple_of(10) {
        (tenths / 10).to_string()
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

/// A size as written (`14`, `10.5`, `14pt`) in tenths of a point.
pub fn parse_size(s: &str) -> Option<u16> {
    let s = s.trim().trim_end_matches("pt");
    let v: f32 = s.parse().ok()?;
    (v > 0. && v <= 1638.).then(|| (v * 10.).round() as u16)
}

/// A color as written: `#rrggbb`, or a name of [`COLORS`].
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(c) = Color::parse(s) {
        return Some(c);
    }
    let named = COLORS
        .iter()
        .chain(HIGHLIGHTS)
        .find(|(n, _)| n.eq_ignore_ascii_case(s))?;
    Color::parse(named.1)
}

/// Text colors offered, by name (shared with the exporters).
pub use org_export::kalem::COLORS;

/// Highlight colors offered, by name.
pub use org_export::kalem::HIGHLIGHTS;

/// Font sizes offered, as a word processor offers them.
pub const SIZES: &[u16] = &[8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];

impl CharFormat {
    /// Nothing set.
    pub fn is_empty(&self) -> bool {
        *self == CharFormat::default()
    }

    /// `self` with what `inner` sets on top.
    pub fn with(self, inner: CharFormat) -> CharFormat {
        CharFormat {
            font: inner.font.or(self.font),
            size: inner.size.or(self.size),
            color: inner.color.or(self.color),
            highlight: inner.highlight.or(self.highlight),
        }
    }

    /// The value of the snippet that starts a span with this format:
    /// `font="Times New Roman" size=14 color=#c00000 bg=#fff2a8`.
    pub fn to_value(&self) -> String {
        let mut parts = Vec::new();
        if let Some(f) = self.font {
            parts.push(format!("font=\"{}\"", f.family().replace('"', "")));
        }
        if let Some(s) = self.size {
            parts.push(format!("size={}", size_text(s)));
        }
        if let Some(c) = self.color {
            parts.push(format!("color={}", hex(c)));
        }
        if let Some(c) = self.highlight {
            parts.push(format!("bg={}", hex(c)));
        }
        parts.join(" ")
    }

    /// The snippet that starts a span with this format.
    pub fn opening(&self) -> String {
        format!("@@{BACKEND}:{}@@", self.to_value())
    }

    /// The format a snippet value sets; `None` for `end`. Unknown keys and
    /// values that do not read are ignored.
    pub fn parse(value: &str) -> Option<CharFormat> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("end") || value == "/" {
            return None;
        }
        let mut f = CharFormat::default();
        let mut rest = value;
        while let Some(eq) = rest.find('=') {
            let key = rest[..eq].trim();
            let after = rest[eq + 1..].trim_start();
            let (val, next) = if let Some(q) = after.strip_prefix('"') {
                match q.find('"') {
                    Some(e) => (&q[..e], &q[e + 1..]),
                    None => (q, ""),
                }
            } else {
                let e = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..e], &after[e..])
            };
            match key.to_ascii_lowercase().as_str() {
                "font" if !val.trim().is_empty() => f.font = Some(FontName::new(val.trim())),
                "size" => f.size = parse_size(val),
                "color" => f.color = parse_color(val),
                "bg" | "highlight" => f.highlight = parse_color(val),
                _ => {}
            }
            rest = next;
        }
        Some(f)
    }
}

fn start(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().start())
}

fn end(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().end())
}

/// A snippet's range without its trailing blanks.
fn marker_range(n: &SyntaxNode) -> Range<usize> {
    let blank = match n.last_child_or_token() {
        Some(org_syntax::NodeOrToken::Token(t)) if t.kind() == WHITESPACE => {
            usize::from(t.text_range().len())
        }
        _ => 0,
    };
    start(n)..end(n) - blank
}

/// Whether `n` is one of Kalem's snippets.
pub fn is_marker(n: &SyntaxNode) -> bool {
    n.kind() == EXPORT_SNIPPET
        && ast::AstNode::cast(n.clone())
            .is_some_and(|s: ast::ExportSnippet| s.backend().eq_ignore_ascii_case(BACKEND))
}

/// Kalem's snippets in `el`, in order: their ranges (without trailing
/// blanks) and the format each starts (`None` for an end).
pub fn markers(el: &SyntaxNode) -> Vec<(Range<usize>, Option<CharFormat>)> {
    el.descendants()
        .filter(is_marker)
        .map(|n| {
            let v = ast::AstNode::cast(n.clone())
                .map(|s: ast::ExportSnippet| s.value())
                .unwrap_or_default();
            (marker_range(&n), CharFormat::parse(&v))
        })
        .collect()
}

/// The end of an element's inline content: before its trailing blank
/// lines and final line break.
fn content_end(el: &SyntaxNode) -> usize {
    let text = el.text().to_string();
    start(el) + text.trim_end_matches(['\n', '\r', ' ', '\t']).len()
}

/// The formatted parts of `el`'s content, in order and not overlapping:
/// each from where a span starts (or an inner span ends) to where it (or an
/// inner span) starts or ends; a span without an end runs to the end of
/// the element.
pub fn spans(el: &SyntaxNode) -> Vec<(Range<usize>, CharFormat)> {
    let mut out = Vec::new();
    let mut stack: Vec<CharFormat> = Vec::new();
    let mut at = 0;
    let merged = |stack: &[CharFormat]| {
        stack
            .iter()
            .fold(CharFormat::default(), |acc, f| acc.with(*f))
    };
    for (r, f) in markers(el) {
        if !stack.is_empty() && at < r.start {
            out.push((at..r.start, merged(&stack)));
        }
        match f {
            Some(f) => stack.push(f),
            None => {
                stack.pop();
            }
        }
        at = r.end;
    }
    let e = content_end(el);
    if !stack.is_empty() && at < e {
        out.push((at..e, merged(&stack)));
    }
    out
}

/// The element holding inline content at `pos`: a paragraph, a
/// headline's title, a table cell's row, an item's paragraph.
pub fn element_at(root: &SyntaxNode, pos: usize) -> Option<SyntaxNode> {
    let len = end(root);
    let tok = root
        .token_at_offset(TextSize::from(pos.min(len) as u32))
        .right_biased()
        .or_else(|| {
            root.token_at_offset(TextSize::from(pos.min(len) as u32))
                .left_biased()
        })?;
    tok.parent_ancestors().find(|a| {
        matches!(
            a.kind(),
            PARAGRAPH | HEADLINE | INLINETASK | TABLE_ROW | VERSE_BLOCK
        )
    })
}

/// Where `el`'s inline content starts.
pub fn format_start(el: &SyntaxNode) -> usize {
    content_range(el).start
}

/// The format at `pos` in `el`.
pub fn format_at(el: &SyntaxNode, pos: usize) -> CharFormat {
    spans(el)
        .into_iter()
        .find(|(r, _)| r.start <= pos && pos < r.end.max(r.start + 1))
        .map(|(_, f)| f)
        .unwrap_or_default()
}

/// A document's own defaults, from its `#+KALEM:` keyword: `font="Georgia"
/// size=12 spacing=1.5`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DocDefaults {
    /// The body font.
    pub font: Option<FontName>,
    /// The body size, in tenths of a point.
    pub size: Option<u16>,
    /// The line spacing, in tenths of a line (15 for 1.5).
    pub spacing: Option<u16>,
}

/// The line spacings offered, in tenths of a line.
pub const SPACINGS: &[u16] = &[10, 12, 15, 20, 25, 30];

impl DocDefaults {
    /// The defaults a `#+KALEM:` value sets.
    pub fn parse(value: &str) -> DocDefaults {
        let f = CharFormat::parse(value).unwrap_or_default();
        let spacing = value.split_whitespace().find_map(|w| {
            let v = w.strip_prefix("spacing=")?;
            let x: f32 = v.parse().ok()?;
            (0.5..=5.).contains(&x).then(|| (x * 10.).round() as u16)
        });
        DocDefaults {
            font: f.font,
            size: f.size,
            spacing,
        }
    }

    /// The defaults of a document's keywords (the last `#+KALEM:` wins,
    /// key by key).
    pub fn of(keywords: &[(String, String)]) -> DocDefaults {
        let mut d = DocDefaults::default();
        for (k, v) in keywords {
            if k.eq_ignore_ascii_case("KALEM") {
                let n = DocDefaults::parse(v);
                d.font = n.font.or(d.font);
                d.size = n.size.or(d.size);
                d.spacing = n.spacing.or(d.spacing);
            }
        }
        d
    }

    /// As a keyword value.
    pub fn to_value(&self) -> String {
        let mut v = CharFormat {
            font: self.font,
            size: self.size,
            ..CharFormat::default()
        }
        .to_value();
        if let Some(s) = self.spacing {
            if !v.is_empty() {
                v.push(' ');
            }
            v.push_str(&format!("spacing={}", size_text(s)));
        }
        v
    }
}

/// The words of a `#+KALEM:` value: `key=value` pairs, values in
/// quotes kept whole.
fn words(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (i, c) in value.char_indices() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if let Some(s) = start.take() {
                    out.push(&value[s..i]);
                }
                continue;
            }
            _ => {}
        }
        if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&value[s..]);
    }
    out
}

/// The keys [`DocDefaults`] reads; other keys of `#+KALEM:` are kept as
/// they are when it changes.
const DEFAULT_KEYS: &[&str] = &["font", "size", "color", "bg", "highlight", "spacing"];

/// The value of `key` in the document's `#+KALEM:` keywords (the last one
/// wins): `recalc=auto` gives `auto` for `recalc`.
pub fn kalem_option(keywords: &[(String, String)], key: &str) -> Option<String> {
    keywords
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("KALEM"))
        .flat_map(|(_, v)| {
            words(v)
                .into_iter()
                .filter_map(|w| w.split_once('='))
                .filter(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim_matches('"').to_string())
                .collect::<Vec<_>>()
        })
        .next_back()
}

/// Sets `key=value` in the document's first `#+KALEM:` line (a new one
/// after the keywords at the top if there is none), the other keys kept.
pub fn set_kalem_option(root: &SyntaxNode, text: &str, key: &str, value: &str) -> Transaction {
    let line = root
        .descendants()
        .filter(|n| n.kind() == KEYWORD)
        .find(|n| {
            ast::AstNode::cast(n.clone())
                .is_some_and(|k: ast::Keyword| k.key().eq_ignore_ascii_case("KALEM"))
        });
    let old = line
        .as_ref()
        .and_then(|n| ast::AstNode::cast(n.clone()))
        .map(|k: ast::Keyword| k.value())
        .unwrap_or_default();
    let mut words: Vec<String> = words(&old)
        .into_iter()
        .filter(|w| {
            !w.split_once('=')
                .is_some_and(|(k, _)| k.eq_ignore_ascii_case(key))
        })
        .map(str::to_string)
        .collect();
    words.push(format!("{key}={value}"));
    let new = format!("#+KALEM: {}", words.join(" "));
    let mut tx = Transaction::new("Document Option");
    match line {
        Some(n) => {
            let s = start(&n);
            let e = s + text[s..].find('\n').unwrap_or(text.len() - s);
            let _ = tx.replace(s..e, new);
        }
        None => {
            let mut at = 0;
            for l in text.split_inclusive('\n') {
                if l.trim_start().starts_with("#+")
                    && !l.trim_start().to_ascii_lowercase().starts_with("#+begin")
                {
                    at += l.len();
                } else {
                    break;
                }
            }
            let _ = tx.insert(at, format!("{new}\n"));
        }
    }
    tx
}

/// Changes the document's `#+KALEM:` defaults with `change` (the first
/// such line, else a new one after the keywords at the top).
pub fn set_defaults(
    root: &SyntaxNode,
    text: &str,
    change: impl FnOnce(&mut DocDefaults),
) -> Transaction {
    let mut tx = Transaction::new("Document Format");
    let line = root
        .descendants()
        .filter(|n| n.kind() == KEYWORD)
        .find(|n| {
            ast::AstNode::cast(n.clone())
                .is_some_and(|k: ast::Keyword| k.key().eq_ignore_ascii_case("KALEM"))
        });
    let old = line
        .as_ref()
        .and_then(|n| ast::AstNode::cast(n.clone()))
        .map(|k: ast::Keyword| k.value())
        .unwrap_or_default();
    let mut d = DocDefaults::parse(&old);
    change(&mut d);
    let mut value = d.to_value();
    // Keys that are not formatting (`recalc=auto`) stay.
    for w in words(&old) {
        let key = w.split_once('=').map_or(w, |(k, _)| k);
        if !DEFAULT_KEYS.iter().any(|d| d.eq_ignore_ascii_case(key)) {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(w);
        }
    }
    match line {
        Some(n) => {
            let r = start(&n)
                ..start(&n)
                    + text[start(&n)..]
                        .find('\n')
                        .unwrap_or(text.len() - start(&n));
            let new = if value.is_empty() {
                String::new()
            } else {
                format!("#+KALEM: {value}")
            };
            let r = if value.is_empty() && r.end < text.len() {
                r.start..r.end + 1
            } else {
                r
            };
            let _ = tx.replace(r, new);
        }
        None if value.is_empty() => {}
        None => {
            // After the keywords at the top of the document.
            let mut at = 0;
            for l in text.split_inclusive('\n') {
                if l.trim_start().starts_with("#+")
                    && !l.trim_start().to_ascii_lowercase().starts_with("#+begin")
                {
                    at += l.len();
                } else {
                    break;
                }
            }
            let _ = tx.insert(at, format!("#+KALEM: {value}\n"));
        }
    }
    tx
}

/// Paragraph alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Flush left, the default.
    #[default]
    Left,
    /// Centered.
    Center,
    /// Flush right.
    Right,
    /// Justified.
    Justify,
}

impl Align {
    /// The alignment a name gives: `left`, `center`, `right`, `justify`.
    pub fn from_name(s: &str) -> Option<Align> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" => Some(Align::Left),
            "center" | "centre" => Some(Align::Center),
            "right" => Some(Align::Right),
            "justify" | "justified" | "both" => Some(Align::Justify),
            _ => None,
        }
    }

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }
}

/// The value of `:key` in an attribute line such as `:align right :x y`.
fn attr(value: &str, key: &str) -> Option<String> {
    let mut words = value.split_whitespace();
    while let Some(w) = words.next() {
        if w.eq_ignore_ascii_case(key) {
            return words.next().map(str::to_string);
        }
    }
    None
}

/// The attributes of an `#+ATTR_KALEM:` value in order, each with its
/// value: `:align right :before 12`.
fn attr_list(value: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for w in value.split_whitespace() {
        if w.starts_with(':') {
            out.push((w.to_string(), String::new()));
        } else if let Some(last) = out.last_mut() {
            if !last.1.is_empty() {
                last.1.push(' ');
            }
            last.1.push_str(w);
        }
    }
    out
}

/// `value` with attribute `key` set to `new` (or taken out), the others
/// kept as they are.
fn with_attr(value: &str, key: &str, new: Option<&str>) -> String {
    let mut list = attr_list(value);
    match (
        list.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)),
        new,
    ) {
        (Some(i), Some(v)) => list[i].1 = v.to_string(),
        (Some(i), None) => {
            list.remove(i);
        }
        (None, Some(v)) => list.push((key.to_string(), v.to_string())),
        (None, None) => {}
    }
    list.iter()
        .map(|(k, v)| {
            if v.is_empty() {
                k.clone()
            } else {
                format!("{k} {v}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The space before and after the paragraph `el`, in tenths of a point:
/// `#+ATTR_KALEM: :before 12 :after 6`.
pub fn spacing(el: &SyntaxNode) -> (Option<u16>, Option<u16>) {
    let values: Vec<String> = ast::affiliated_keywords(el)
        .filter(|k| k.key().eq_ignore_ascii_case("ATTR_KALEM"))
        .map(|k| k.value())
        .collect();
    let get = |key: &str| {
        values
            .iter()
            .filter_map(|v| attr(v, key))
            .filter_map(|v| parse_size(&v))
            .next_back()
    };
    (get(":before"), get(":after"))
}

/// The room to leave above and below line `line` (a range without its
/// line feed), in tenths of a point: the paragraph's space before on its
/// first line and its space after on its last.
pub fn line_spacing(root: &SyntaxNode, line: Range<usize>) -> (u16, u16) {
    let Some(el) = element_at(root, line.start).filter(|e| e.kind() == PARAGRAPH) else {
        return (0, 0);
    };
    let (before, after) = spacing(&el);
    if before.is_none() && after.is_none() {
        return (0, 0);
    }
    let first = usize::from(ast::post_affiliated(&el));
    let last = content_end(&el);
    (
        if line.start <= first && first <= line.end {
            before.unwrap_or(0)
        } else {
            0
        },
        if line.start < last && last <= line.end + 1 {
            after.unwrap_or(0)
        } else {
            0
        },
    )
}

/// Sets the space before (`before`) and after (`after`) the paragraphs
/// in `range`: `Some(None)` takes it away, `None` leaves it; sizes in
/// tenths of a point.
pub fn set_spacing(
    root: &SyntaxNode,
    text: &str,
    range: Range<usize>,
    before: Option<Option<u16>>,
    after: Option<Option<u16>>,
) -> Option<Transaction> {
    let mut tx = Transaction::new("Paragraph Spacing");
    let mut seen: Vec<SyntaxNode> = Vec::new();
    let mut pos = range.start;
    let line_end = |p: usize| text[p..].find('\n').map_or(text.len(), |i| p + i + 1);
    loop {
        if let Some(el) = element_at(root, pos).filter(|e| e.kind() == PARAGRAPH)
            && !seen.contains(&el)
        {
            let indent: String = text[start(&el)..]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let last = ast::affiliated_keywords(&el)
                .filter(|k| k.key().eq_ignore_ascii_case("ATTR_KALEM"))
                .last();
            let mut value = last.as_ref().map(|k| k.value()).unwrap_or_default();
            for (key, v) in [(":before", before), (":after", after)] {
                if let Some(v) = v {
                    value = with_attr(&value, key, v.map(size_text).as_deref());
                }
            }
            let line = if value.trim().is_empty() {
                String::new()
            } else {
                format!("{indent}#+ATTR_KALEM: {}\n", value.trim())
            };
            match last {
                Some(k) => {
                    let n = ast::AstNode::syntax(&k);
                    tx.replace(start(n)..end(n), line).ok()?;
                }
                None if !line.is_empty() => {
                    tx.insert(usize::from(ast::post_affiliated(&el)), line)
                        .ok()?;
                }
                None => {}
            }
            seen.push(el.clone());
            pos = end(&el);
        } else {
            pos = line_end(pos);
        }
        if pos >= range.end.max(range.start + 1) || pos >= text.len() {
            break;
        }
    }
    (!seen.is_empty()).then_some(tx)
}

/// The alignment of the paragraph `el`: `#+ATTR_KALEM: :align …`, or
/// centered in a `#+begin_center` block.
pub fn align(el: &SyntaxNode) -> Align {
    let own = ast::affiliated_keywords(el)
        .filter(|k| k.key().eq_ignore_ascii_case("ATTR_KALEM"))
        .filter_map(|k| attr(&k.value(), ":align"))
        .filter_map(|a| Align::from_name(&a))
        .last();
    own.unwrap_or_else(|| {
        if el.ancestors().any(|a| a.kind() == CENTER_BLOCK) {
            Align::Center
        } else {
            Align::Left
        }
    })
}

/// Whether the line at `line_start` of `text` is an `#+ATTR_KALEM:` line.
pub fn is_attr_line(text: &str, line_start: usize) -> bool {
    let line = text[line_start..].split('\n').next().unwrap_or("");
    let t = line.trim_start();
    t.get(..13)
        .is_some_and(|p| p.eq_ignore_ascii_case("#+attr_kalem:"))
}

/// A change of the character formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The font family (`None`: the document's).
    Font(Option<String>),
    /// The size in tenths of a point (`None`: the document's).
    Size(Option<u16>),
    /// One step up (or down) the list of sizes, from `base` tenths when
    /// no size is set.
    Grow {
        /// Down.
        down: bool,
        /// The document's size.
        base: u16,
    },
    /// The text color.
    Color(Option<Color>),
    /// The highlight color.
    Highlight(Option<Color>),
    /// No Kalem formatting.
    Clear,
}

impl Change {
    fn apply(&self, f: CharFormat) -> CharFormat {
        let mut f = f;
        match self {
            Change::Font(v) => f.font = v.as_deref().map(FontName::new),
            Change::Size(v) => f.size = *v,
            Change::Grow { down, base } => {
                let now = f.size.unwrap_or(*base);
                let pts = now.div_ceil(10);
                let next = if *down {
                    SIZES
                        .iter()
                        .rev()
                        .find(|&&s| s * 10 < now)
                        .copied()
                        .unwrap_or(SIZES[0])
                } else {
                    SIZES
                        .iter()
                        .find(|&&s| s * 10 > now)
                        .copied()
                        .unwrap_or(pts + 10)
                };
                f.size = Some(next * 10);
                if f.size == Some(*base) {
                    f.size = None;
                }
            }
            Change::Color(c) => f.color = *c,
            Change::Highlight(c) => f.highlight = *c,
            Change::Clear => f = CharFormat::default(),
        }
        f
    }
}

/// Objects whose inside cannot hold a snippet: a formatting boundary
/// inside one moves to its edge.
const WHOLE: &[SyntaxKind] = &[
    CODE,
    VERBATIM,
    INLINE_SRC_BLOCK,
    INLINE_BABEL_CALL,
    LATEX_FRAGMENT,
    TIMESTAMP,
    ENTITY,
    EXPORT_SNIPPET,
    LINK,
    FOOTNOTE_REFERENCE,
    TARGET,
    RADIO_TARGET,
    MACRO,
    STATISTICS_COOKIE,
    CITATION,
    LINE_BREAK,
];

/// `pos` moved out of objects that cannot be split: to their start
/// (`back`) or end.
fn snap(root: &SyntaxNode, pos: usize, back: bool) -> usize {
    let len = end(root);
    let Some(tok) = root
        .token_at_offset(TextSize::from(pos.min(len) as u32))
        .right_biased()
    else {
        return pos;
    };
    let mut p = pos;
    for a in tok.parent_ancestors() {
        if WHOLE.contains(&a.kind()) {
            let r = marker_range(&a);
            if r.start < p && p < r.end {
                p = if back { r.start } else { r.end };
            }
        }
    }
    p
}

/// The inline content of `el`: where formatting may go.
fn content_range(el: &SyntaxNode) -> Range<usize> {
    match el.kind() {
        HEADLINE | INLINETASK => {
            // The title: after the stars, keyword and priority, before the
            // tags and the line end.
            let line_end = start(el)
                + el.text()
                    .to_string()
                    .find('\n')
                    .unwrap_or(usize::from(el.text_range().len()));
            let s = el
                .children_with_tokens()
                .filter(|t| t.text_range().start() < TextSize::from(line_end as u32))
                .filter(|t| {
                    !matches!(
                        t.kind(),
                        STARS
                            | TODO_KEYWORD
                            | PRIORITY
                            | WHITESPACE
                            | TAGS
                            | COMMENT_KEYWORD
                            | NEWLINE
                    )
                })
                .map(|t| usize::from(t.text_range().start()))
                .next()
                .unwrap_or(line_end);
            let e = el
                .children_with_tokens()
                .find(|t| t.kind() == TAGS)
                .map_or(line_end, |t| usize::from(t.text_range().start()));
            let text = el.text().to_string();
            let rel = |p: usize| p - start(el);
            let title = &text[rel(s)..rel(e.max(s))];
            s..s + title.trim_end().len()
        }
        _ => {
            let s = usize::from(ast::post_affiliated(el));
            s..content_end(el)
        }
    }
}

/// The source text of `el`'s content as parts: `(source range of text,
/// its format)`, with Kalem's snippets left out.
fn parts(el: &SyntaxNode, content: &Range<usize>) -> Vec<(Range<usize>, CharFormat)> {
    let spans = spans(el);
    let mut cuts: Vec<Range<usize>> = markers(el).into_iter().map(|(r, _)| r).collect();
    cuts.sort_by_key(|r| r.start);
    let mut out = Vec::new();
    let mut at = content.start;
    let mut push = |r: Range<usize>| {
        if r.start >= r.end {
            return;
        }
        // Cut where the format changes.
        let mut s = r.start;
        let mut bounds: Vec<usize> = spans
            .iter()
            .flat_map(|(sr, _)| [sr.start, sr.end])
            .filter(|&b| b > r.start && b < r.end)
            .collect();
        bounds.sort_unstable();
        bounds.dedup();
        bounds.push(r.end);
        for b in bounds {
            let f = spans
                .iter()
                .find(|(sr, _)| sr.start <= s && s < sr.end)
                .map(|(_, f)| *f)
                .unwrap_or_default();
            out.push((s..b, f));
            s = b;
        }
    };
    for c in cuts {
        if c.end <= content.start || c.start >= content.end {
            continue;
        }
        push(at..c.start.max(at));
        at = at.max(c.end);
    }
    push(at..content.end);
    out
}

/// The kinds of emphasis Org reads in `text`, in order.
fn emphasis(text: &str) -> Vec<SyntaxKind> {
    org_syntax::parse(text)
        .syntax()
        .descendants()
        .map(|n| n.kind())
        .filter(|k| {
            matches!(
                k,
                BOLD | ITALIC | UNDERLINE | STRIKE_THROUGH | CODE | VERBATIM
            )
        })
        .collect()
}

/// `pieces` changed so that Org reads the emphasis around them as the
/// editor shows it. A snippet right before a marker that opens (`*bold*`,
/// `/it/`, `=code=` and the like) or right after one that closes takes the
/// place of the character Org wants there, and the emphasis would be read
/// as plain text. Around bold, italic, underline and strike-through the
/// change moves inside (the opening marker takes the format of what comes
/// before it, the closing one of what comes after), where Org reads
/// snippets; code and verbatim, whose text Org does not read, take the
/// characters around them into their format instead, and stay plain at
/// the start or end of the paragraph, where there is no such character.
/// No span goes into an emphasis without going out of it too (HTML nests):
/// one that would stops before the character in front of the marker.
fn around_emphasis(
    el: &SyntaxNode,
    text: &str,
    pieces: Vec<(Range<usize>, CharFormat)>,
) -> Vec<(Range<usize>, CharFormat)> {
    // (opening marker, closing marker, whether its text is read).
    let mut objects = Vec::new();
    for n in el.descendants() {
        let read = matches!(n.kind(), BOLD | ITALIC | UNDERLINE | STRIKE_THROUGH);
        if read || matches!(n.kind(), CODE | VERBATIM) {
            let s = start(&n);
            let e = end(&n) - ast::post_blank(&n);
            if e >= s + 2 {
                objects.push((s, e - 1, read));
            }
        }
    }
    if objects.is_empty() {
        return pieces;
    }
    let before = |p: usize| {
        text[..p]
            .chars()
            .next_back()
            .map_or(p, |c| p - c.len_utf8())
    };
    let after = |p: usize| text[p..].chars().next().map_or(p, |c| p + c.len_utf8());
    // Each marker and each character around one a piece of its own.
    let mut cuts: Vec<usize> = Vec::new();
    for &(o, c, _) in &objects {
        cuts.extend([before(o), o, o + 1, c, c + 1, after(c + 1)]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out: Vec<(Range<usize>, CharFormat)> = Vec::new();
    for (r, f) in pieces {
        let mut s = r.start;
        for c in cuts
            .iter()
            .copied()
            .filter(|&c| c > r.start && c < r.end)
            .chain([r.end])
        {
            out.push((s..c, f));
            s = c;
        }
    }
    type Pieces = [(Range<usize>, CharFormat)];
    let at = |out: &Pieces, p: usize| out.iter().position(|(r, _)| r.start == p);
    let ending = |out: &Pieces, p: usize| out.iter().position(|(r, _)| r.end == p);
    let plain = |out: &mut Pieces, r: Range<usize>| {
        for piece in out
            .iter_mut()
            .filter(|(x, _)| x.start >= r.start && x.end <= r.end)
        {
            piece.1 = CharFormat::default();
        }
    };
    // Code and verbatim: the characters around them go with them.
    for &(o, c, read) in &objects {
        if read {
            continue;
        }
        let Some(i) = at(&out, o) else { continue };
        let f = out[i].1;
        match (ending(&out, o), at(&out, c + 1)) {
            (Some(p), Some(n)) => {
                out[p].1 = f;
                out[n].1 = f;
            }
            _ => plain(&mut out, o..c + 1),
        }
    }
    for _ in 0..4 * objects.len() + 4 {
        let mut changed = false;
        for &(o, c, read) in &objects {
            let (Some(i), Some(j)) = (at(&out, o), at(&out, c)) else {
                continue;
            };
            if read {
                let f = ending(&out, o).map_or_else(CharFormat::default, |k| out[k].1);
                let g = at(&out, c + 1).map_or_else(CharFormat::default, |k| out[k].1);
                changed |= out[i].1 != f || out[j].1 != g;
                out[i].1 = f;
                out[j].1 = g;
            } else {
                // Still with the characters around it, or plain with them.
                let f = out[i].1;
                let fmt = |k: Option<usize>| k.map_or_else(CharFormat::default, |k| out[k].1);
                if fmt(ending(&out, o)) != f || fmt(at(&out, c + 1)) != f {
                    plain(&mut out, before(o)..after(c + 1));
                    changed = true;
                }
            }
        }
        // Formatted runs that go into an emphasis without going out.
        let mut i = 0;
        while i < out.len() {
            let f = out[i].1;
            let mut j = i;
            while j < out.len() && out[j].1 == f {
                j += 1;
            }
            let (a, b) = (out[i].0.start, out[j - 1].0.end);
            i = j;
            if f.is_empty() {
                continue;
            }
            for &(o, c, read) in &objects {
                let (s, e) = (o, c + 1);
                let crosses = a < e && b > s && !(a <= s && b >= e) && !(a > s && b <= c);
                if !read || !crosses {
                    continue;
                }
                if a <= s {
                    plain(&mut out, before(o).max(a)..o + 1);
                } else {
                    plain(&mut out, c..after(e).min(b));
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    out
}

/// Formats `range` of the document's text with `change`: the formatted
/// spans of each paragraph (or heading) it touches are written again, in
/// their simplest form. Returns the transaction and the range covering the
/// same text afterwards; `None` when nothing can be formatted there (a
/// table, a source block).
pub fn apply(
    root: &SyntaxNode,
    text: &str,
    range: Range<usize>,
    change: &Change,
) -> Option<(Transaction, Range<usize>)> {
    let mut els: Vec<SyntaxNode> = Vec::new();
    let mut pos = range.start;
    loop {
        let el = element_at(root, pos)?;
        if el.kind() == TABLE_ROW {
            return None;
        }
        let e = end(&el);
        if !els.iter().any(|x| x == &el) {
            els.push(el);
        }
        if e >= range.end || e >= text.len() {
            break;
        }
        pos = e;
        // Skip what holds no inline content (blank lines, keywords).
        while pos < range.end && element_at(root, pos).is_none_or(|x| els.contains(&x)) {
            let next = text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1);
            if next == pos {
                break;
            }
            pos = next;
        }
        if pos >= range.end {
            break;
        }
    }
    let (sel_start, sel_end) = (snap(root, range.start, true), snap(root, range.end, false));
    let mut edits = Vec::new();
    let (mut new_start, mut new_end): (Option<usize>, Option<usize>) = (None, None);
    let mut shift: isize = 0;
    for el in &els {
        let content = content_range(el);
        if content.start >= content.end {
            continue;
        }
        let ps = parts(el, &content);
        // Split at the selection and change what is inside it.
        let mut changed: Vec<(Range<usize>, CharFormat)> = Vec::new();
        for (r, f) in ps {
            let mut cuts = vec![r.start];
            for b in [sel_start, sel_end] {
                if b > r.start && b < r.end {
                    cuts.push(b);
                }
            }
            cuts.push(r.end);
            for w in cuts.windows(2) {
                let piece = w[0]..w[1];
                let inside =
                    piece.start >= sel_start && piece.end <= sel_end && sel_start < sel_end;
                changed.push((piece.clone(), if inside { change.apply(f) } else { f }));
            }
        }
        let changed = around_emphasis(el, text, changed);
        // Written again: runs of the same format, markers around the
        // formatted ones; the new places of the selection's ends.
        let mut out = String::new();
        // Where each part starts and ends now: (old, new, a start).
        let mut map: Vec<(usize, usize, bool)> = Vec::new();
        let mut i = 0;
        while i < changed.len() {
            let f = changed[i].1;
            let mut j = i;
            while j < changed.len() && changed[j].1 == f {
                j += 1;
            }
            if !f.is_empty() {
                out.push_str(&f.opening());
            }
            for (r, _) in &changed[i..j] {
                map.push((r.start, content.start + out.len(), true));
                out.push_str(&text[r.clone()]);
                map.push((r.end, content.start + out.len(), false));
            }
            if !f.is_empty() {
                out.push_str(END);
            }
            i = j;
        }
        let old = &text[content.clone()];
        if out == old {
            continue;
        }
        // A snippet beside a stray `/` or `*` can still make Org read
        // emphasis that was not there: then the text is not formatted.
        if emphasis(old) != emphasis(&out) {
            return None;
        }
        // The selection's start goes where its part now starts (after an
        // opening snippet), its end where its part ends (before an end).
        let find = |p: usize, start: bool| {
            map.iter()
                .find(|(o, _, st)| *o == p && *st == start)
                .or_else(|| map.iter().find(|(o, _, _)| *o == p))
                .map(|(_, n, _)| (*n as isize + shift) as usize)
        };
        if sel_start >= content.start
            && let Some(n) = find(sel_start, true)
        {
            new_start = Some(n);
        }
        if let Some(n) = find(sel_end, false) {
            new_end = Some(n);
        }
        // The smallest edit: common prefix and suffix left alone.
        let pre = old
            .bytes()
            .zip(out.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let pre = (0..=pre)
            .rev()
            .find(|&p| old.is_char_boundary(p) && out.is_char_boundary(p))
            .unwrap_or(0);
        let max_suf = old.len().min(out.len()) - pre;
        let suf = old
            .bytes()
            .rev()
            .zip(out.bytes().rev())
            .take(max_suf)
            .take_while(|(a, b)| a == b)
            .count();
        let suf = (0..=suf)
            .rev()
            .find(|&s| old.is_char_boundary(old.len() - s) && out.is_char_boundary(out.len() - s))
            .unwrap_or(0);
        edits.push((
            content.start + pre..content.end - suf,
            out[pre..out.len() - suf].to_string(),
        ));
        shift += out.len() as isize - old.len() as isize;
    }
    let mut tx = Transaction::new("Format");
    for (r, t) in edits {
        tx.replace(r, t).ok()?;
    }
    // Ends outside the formatted content move with the edits.
    let s = new_start.unwrap_or_else(|| tx.map(sel_start, org_edit::Assoc::After));
    let e = new_end.unwrap_or_else(|| tx.map(sel_end, org_edit::Assoc::Before));
    Some((tx, s..e.max(s)))
}

/// Sets the alignment of the paragraphs `range` touches. Centering uses
/// Org's own `#+begin_center` block, which Emacs centers too; right and
/// justify an `#+ATTR_KALEM: :align …` line; left takes both away (a
/// center block holding only that paragraph goes, one holding more gets
/// `:align left` for the paragraph).
pub fn set_align(
    root: &SyntaxNode,
    text: &str,
    range: Range<usize>,
    align: Align,
) -> Option<Transaction> {
    let mut tx = Transaction::new("Align");
    let mut seen: Vec<SyntaxNode> = Vec::new();
    let mut pos = range.start;
    let line_end = |p: usize| text[p..].find('\n').map_or(text.len(), |i| p + i + 1);
    loop {
        if let Some(el) = element_at(root, pos).filter(|e| e.kind() == PARAGRAPH)
            && !seen.contains(&el)
        {
            let center = el.ancestors().find(|a| a.kind() == CENTER_BLOCK);
            // The center block holds only this paragraph.
            let alone = center.as_ref().is_some_and(|c| {
                c.descendants()
                    .filter(|d| d.kind().is_element() && d.kind() != CENTER_BLOCK)
                    .all(|d| d == el || d.ancestors().any(|a| a == el))
            });
            let attrs: Vec<ast::AffiliatedKeyword> = ast::affiliated_keywords(&el)
                .filter(|k| k.key().eq_ignore_ascii_case("ATTR_KALEM"))
                .collect();
            let indent: String = text[start(&el)..]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            // The attribute line wanted, and whether the paragraph should be
            // in a center block of its own.
            let (attr, wrap) = match (align, &center) {
                (Align::Center, Some(_)) => (None, None),
                (Align::Center, None) => (None, Some(true)),
                (Align::Left, Some(_)) if alone => (None, Some(false)),
                (a, Some(_)) if alone => (Some(a).filter(|a| *a != Align::Left), Some(false)),
                (a, _) => (
                    Some(a).filter(|a| *a != Align::Left || center.is_some()),
                    None,
                ),
            };
            let line = |a: Align| format!("{indent}#+ATTR_KALEM: :align {}\n", a.name());
            match (attrs.last(), attr) {
                (Some(k), w) => {
                    // The other attributes (`:before`, `:after`) stay.
                    let n = ast::AstNode::syntax(k);
                    let value = with_attr(&k.value(), ":align", w.map(Align::name));
                    let new = if value.trim().is_empty() {
                        String::new()
                    } else {
                        format!("{indent}#+ATTR_KALEM: {}\n", value.trim())
                    };
                    tx.replace(start(n)..end(n), new).ok()?;
                }
                (None, Some(a)) => {
                    let at = usize::from(ast::post_affiliated(&el));
                    tx.insert(at, line(a)).ok()?;
                }
                (None, None) => {}
            }
            match (wrap, &center) {
                (Some(true), _) => {
                    let s = start(&el);
                    let e = line_end(content_end(&el).saturating_sub(1).max(s));
                    tx.insert(s, format!("{indent}#+begin_center\n")).ok()?;
                    let tail = if e > 0 && text.as_bytes()[e - 1] != b'\n' {
                        "\n"
                    } else {
                        ""
                    };
                    tx.insert(e, format!("{tail}{indent}#+end_center\n")).ok()?;
                }
                (Some(false), Some(c)) => {
                    // The block's first and last lines go.
                    let cs = start(c);
                    tx.delete(cs..line_end(cs)).ok()?;
                    let ce = content_end(c);
                    let last = text[..ce].rfind('\n').map_or(0, |i| i + 1);
                    tx.delete(last..line_end(last)).ok()?;
                }
                _ => {}
            }
            seen.push(el.clone());
            pos = end(&el);
        } else {
            pos = line_end(pos);
        }
        if pos >= range.end.max(range.start + 1) || pos >= text.len() {
            break;
        }
    }
    (!seen.is_empty()).then_some(tx)
}

/// The Kalem snippet holding byte `pos`, if any (its range without
/// trailing blanks).
pub fn marker_at(root: &SyntaxNode, pos: usize) -> Option<Range<usize>> {
    if pos >= end(root) {
        return None;
    }
    let tok = root
        .token_at_offset(TextSize::from(pos as u32))
        .right_biased()?;
    let n = tok.parent_ancestors().find(is_marker)?;
    let r = marker_range(&n);
    (r.start <= pos && pos < r.end).then_some(r)
}

/// The smallest element around `range` (and a byte on each side).
fn around(root: &SyntaxNode, range: &Range<usize>) -> SyntaxNode {
    let len = end(root);
    let r = org_syntax::TextRange::new(
        TextSize::from(range.start.saturating_sub(1) as u32),
        TextSize::from((range.end + 1).min(len) as u32),
    );
    let covering = root.covering_element(r);
    let node = match covering {
        org_syntax::NodeOrToken::Node(n) => n,
        org_syntax::NodeOrToken::Token(t) => t.parent().unwrap_or_else(|| root.clone()),
    };
    node.ancestors()
        .find(|a| a.kind().is_element() || a.kind() == DOCUMENT)
        .unwrap_or_else(|| root.clone())
}

/// Deleting `range`: the parts outside Kalem's snippets, so that spans
/// keep their start and end; a span left empty goes with its snippets.
pub fn deletion(root: &SyntaxNode, text: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let scope = around(root, &range);
    let mut keep: Vec<(Range<usize>, bool)> = scope
        .descendants()
        .filter(is_marker)
        .map(|n| {
            let r = marker_range(&n);
            let end = text[r.clone()].eq_ignore_ascii_case(END);
            (r, end)
        })
        .filter(|(r, _)| r.start <= range.end && range.start <= r.end)
        .collect();
    if keep.is_empty() {
        return vec![range];
    }
    keep.sort_by_key(|(r, _)| r.start);
    let mut out = Vec::new();
    let mut at = range.start;
    for (k, _) in &keep {
        if k.start > at {
            out.push(at..k.start.min(range.end));
        }
        at = at.max(k.end);
    }
    if at < range.end {
        out.push(at..range.end);
    }
    // A span whose text all goes: its two snippets go too.
    let covered =
        |r: Range<usize>| r.is_empty() || out.iter().any(|o| o.start <= r.start && r.end <= o.end);
    let mut extra = Vec::new();
    for w in keep.windows(2) {
        let ((a, a_end), (b, b_end)) = (&w[0], &w[1]);
        if !a_end && *b_end && covered(a.end..b.start) {
            extra.push(a.clone());
            extra.push(b.clone());
        }
    }
    out.extend(extra);
    out.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(m) if m.end >= r.start => m.end = m.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> org_syntax::Parse {
        org_syntax::parse(text)
    }

    fn run(text: &str, range: Range<usize>, change: Change) -> (String, Range<usize>) {
        let p = parse(text);
        let (tx, r) = apply(&p.syntax(), text, range, &change).expect("formattable");
        (tx.apply(text), r)
    }

    #[test]
    fn values() {
        let f =
            CharFormat::parse(r#"font="Times New Roman" size=10.5 color=#C00000 bg=yellow x=1"#)
                .unwrap();
        assert_eq!(f.font.unwrap().family().as_ref(), "Times New Roman");
        assert_eq!(f.size, Some(105));
        assert_eq!(
            f.to_value(),
            r#"font="Times New Roman" size=10.5 color=#c00000 bg=#fff2a8"#
        );
        assert_eq!(CharFormat::parse("end"), None);
        assert_eq!(CharFormat::parse("size=big"), Some(CharFormat::default()));
    }

    #[test]
    fn spans_nest() {
        let text = "a @@kalem:size=14@@b @@kalem:color=#ff0000@@c@@kalem:end@@ d@@kalem:end@@ e\n";
        let p = parse(text);
        let el = element_at(&p.syntax(), 0).unwrap();
        let s = spans(&el);
        let shown: Vec<(&str, Option<u16>, bool)> = s
            .iter()
            .map(|(r, f)| (&text[r.clone()], f.size, f.color.is_some()))
            .collect();
        assert_eq!(
            shown,
            [
                ("b ", Some(140), false),
                ("c", Some(140), true),
                (" d", Some(140), false)
            ]
        );
        // An open span runs to the end of the paragraph.
        let text = "a @@kalem:size=14@@b\nc\n\nnext\n";
        let p = parse(text);
        let el = element_at(&p.syntax(), 0).unwrap();
        assert_eq!(&text[spans(&el)[0].0.clone()], "b\nc");
    }

    proptest::proptest! {
        /// Formatting never changes what Org reads as emphasis, and the
        /// text without Kalem's snippets stays the same.
        #[test]
        fn formatting_keeps_emphasis(
            text in "([a-c ]|\\*[ab]+\\*|/[ab ]*a/|=a b=|~c~|\\(|\\)|-){1,12}\n",
            ops in proptest::collection::vec((0usize..40, 0usize..40, 0usize..5), 1..5),
        ) {
            let kinds = emphasis;
            let want = kinds(&text);
            let mut t = text.clone();
            for (a, b, c) in ops {
                let len = t.len();
                let (a, b) = ((a % len).min(b % len), (a % len).max(b % len));
                let change = match c {
                    0 => Change::Color(parse_color("red")),
                    1 => Change::Highlight(parse_color("yellow")),
                    2 => Change::Size(Some(140)),
                    3 => Change::Font(Some("Georgia".into())),
                    _ => Change::Clear,
                };
                let p = parse(&t);
                if let Some((tx, _)) = apply(&p.syntax(), &t, a..b, &change) {
                    t = tx.apply(&t);
                }
                proptest::prop_assert_eq!(kinds(&t), want.clone(), "{}", t);
                proptest::prop_assert_eq!(crate::kinds::strip_markup(&t).0, text.clone());
            }
        }
    }

    #[test]
    fn applying() {
        // A word, then part of it, then the whole paragraph.
        let (t, r) = run("one two three\n", 4..7, Change::Size(Some(140)));
        assert_eq!(t, "one @@kalem:size=14@@two@@kalem:end@@ three\n");
        assert_eq!(&t[r.clone()], "two");
        let (t, _) = run(&t, r.start + 1..r.end, Change::Color(parse_color("red")));
        assert_eq!(
            t,
            "one @@kalem:size=14@@t@@kalem:end@@@@kalem:size=14 color=#c00000@@wo@@kalem:end@@ three\n"
        );
        let (t, _) = run(&t, 0..t.len() - 1, Change::Clear);
        assert_eq!(t, "one two three\n");
        // Growing steps through the sizes from the document's.
        let (t, r) = run(
            "abc\n",
            0..3,
            Change::Grow {
                down: false,
                base: 160,
            },
        );
        assert_eq!(t, "@@kalem:size=18@@abc@@kalem:end@@\n");
        let (t, _) = run(
            &t,
            r,
            Change::Grow {
                down: true,
                base: 160,
            },
        );
        assert_eq!(t, "abc\n");
        // Code and links are not split.
        let (t, _) = run("see =verbatim= x\n", 6..9, Change::Size(Some(200)));
        assert_eq!(t, "see@@kalem:size=20@@ =verbatim= @@kalem:end@@x\n");
        // At the start of a paragraph there is no room for the snippet.
        let (t, _) = run("=verbatim= x\n", 2..5, Change::Size(Some(200)));
        assert_eq!(t, "=verbatim= x\n");
        // Across paragraphs: each has its own span.
        let (t, _) = run(
            "one\n\ntwo\n",
            0..8,
            Change::Highlight(parse_color("yellow")),
        );
        assert_eq!(
            t,
            "@@kalem:bg=#fff2a8@@one@@kalem:end@@\n\n@@kalem:bg=#fff2a8@@two@@kalem:end@@\n"
        );
        // A heading's title.
        let (t, _) = run(
            "* TODO Title :tag:\n",
            7..12,
            Change::Font(Some("Georgia".into())),
        );
        assert_eq!(
            t,
            "* TODO @@kalem:font=\"Georgia\"@@Title@@kalem:end@@ :tag:\n"
        );
        // Snippets stay off emphasis markers, which Org would then not
        // read as emphasis.
        let fmt = |t: &str, r: Range<usize>| {
            run(t, r, Change::Color(Some(parse_color("red").unwrap()))).0
        };
        let red = "@@kalem:color=#c00000@@";
        let end = "@@kalem:end@@";
        assert_eq!(fmt("*bold*\n", 0..6), format!("*{red}bold{end}*\n"));
        assert_eq!(
            fmt("a *b* =c= d\n", 0..11),
            format!("{red}a *b* =c= d{end}\n")
        );
        assert_eq!(fmt("(*bold*) x\n", 1..7), format!("(*{red}bold{end}*) x\n"));
        // Spans nest with emphasis.
        assert_eq!(
            fmt("a *b* /c/\n", 2..9),
            format!("a *{red}b{end}* /{red}c{end}/\n")
        );
        assert_eq!(
            fmt("x *bold* y\n", 0..5),
            format!("{red}x{end} *{red}bo{end}ld* y\n")
        );
        assert_eq!(
            fmt("*a* *b*\n", 1..6),
            format!("*{red}a{end}* *{red}b{end}*\n")
        );
        for t in [fmt("*bold*\n", 0..6), fmt("a *b* /c/\n", 2..9)] {
            let p = parse(&t);
            assert!(p.syntax().descendants().any(|n| n.kind() == BOLD), "{t}");
        }
        // Not in tables.
        let p = parse("| a |\n");
        assert!(apply(&p.syntax(), "| a |\n", 2..3, &Change::Clear).is_none());
    }

    #[test]
    fn aligning() {
        let text = "one\n\ntwo\n";
        let p = parse(text);
        let tx = set_align(&p.syntax(), text, 0..1, Align::Right).unwrap();
        let t = tx.apply(text);
        assert_eq!(t, "#+ATTR_KALEM: :align right\none\n\ntwo\n");
        let p = parse(&t);
        let el = element_at(&p.syntax(), 30).unwrap();
        assert_eq!(align(&el), Align::Right);
        assert!(is_attr_line(&t, 0));
        let tx = set_align(&p.syntax(), &t, 28..29, Align::Left).unwrap();
        assert_eq!(tx.apply(&t), "one\n\ntwo\n");
        let text = "#+begin_center\nmid\n#+end_center\n";
        let p = parse(text);
        let el = element_at(&p.syntax(), 16).unwrap();
        assert_eq!(align(&el), Align::Center);
        // Centering wraps the paragraph in a center block; left unwraps it.
        let text = "one\ntwo\n\nnext\n";
        let p = parse(text);
        let t = set_align(&p.syntax(), text, 0..0, Align::Center)
            .unwrap()
            .apply(text);
        assert_eq!(t, "#+begin_center\none\ntwo\n#+end_center\n\nnext\n");
        let p = parse(&t);
        let el = element_at(&p.syntax(), 16).unwrap();
        assert_eq!(align(&el), Align::Center);
        let t2 = set_align(&p.syntax(), &t, 16..16, Align::Left)
            .unwrap()
            .apply(&t);
        assert_eq!(t2, text);
        let t3 = set_align(&p.syntax(), &t, 16..16, Align::Right)
            .unwrap()
            .apply(&t);
        assert_eq!(t3, "#+ATTR_KALEM: :align right\none\ntwo\n\nnext\n");
        // A center block with more paragraphs keeps them centered.
        let text = "#+begin_center\na\n\nb\n#+end_center\n";
        let p = parse(text);
        let t = set_align(&p.syntax(), text, 15..15, Align::Left)
            .unwrap()
            .apply(text);
        assert_eq!(
            t,
            "#+begin_center\n#+ATTR_KALEM: :align left\na\n\nb\n#+end_center\n"
        );
    }

    #[test]
    fn document_defaults() {
        let d = DocDefaults::of(&[(
            "KALEM".into(),
            r#"font="Georgia" size=12 spacing=1.5"#.into(),
        )]);
        assert_eq!(d.font.unwrap().family().as_ref(), "Georgia");
        assert_eq!((d.size, d.spacing), (Some(120), Some(15)));
        assert_eq!(d.to_value(), r#"font="Georgia" size=12 spacing=1.5"#);
        let text = "#+TITLE: T\nbody\n";
        let p = parse(text);
        let t = set_defaults(&p.syntax(), text, |d| d.size = Some(130)).apply(text);
        assert_eq!(t, "#+TITLE: T\n#+KALEM: size=13\nbody\n");
        let p = parse(&t);
        let t2 = set_defaults(&p.syntax(), &t, |d| d.spacing = Some(20)).apply(&t);
        assert_eq!(t2, "#+TITLE: T\n#+KALEM: size=13 spacing=2\nbody\n");
        let p = parse(&t2);
        let t3 = set_defaults(&p.syntax(), &t2, |d| *d = DocDefaults::default()).apply(&t2);
        assert_eq!(t3, "#+TITLE: T\nbody\n");
    }

    #[test]
    fn paragraph_spacing() {
        let text = "Intro.\n\nFirst line\nsecond line.\n";
        let p = parse(text);
        let at = text.find("First").unwrap();
        let t = set_spacing(&p.syntax(), text, at..at, Some(Some(120)), Some(Some(60)))
            .unwrap()
            .apply(text);
        assert_eq!(
            t,
            "Intro.\n\n#+ATTR_KALEM: :before 12 :after 6\nFirst line\nsecond line.\n"
        );
        // Alignment keeps the spacing, and the spacing the alignment.
        let p = parse(&t);
        let at = t.find("First").unwrap();
        let t = set_align(&p.syntax(), &t, at..at, Align::Right)
            .unwrap()
            .apply(&t);
        assert!(
            t.contains("#+ATTR_KALEM: :before 12 :after 6 :align right\n"),
            "{t}"
        );
        let p = parse(&t);
        let at = t.find("First").unwrap();
        let t = set_spacing(&p.syntax(), &t, at..at, Some(None), None)
            .unwrap()
            .apply(&t);
        assert!(t.contains("#+ATTR_KALEM: :after 6 :align right\n"), "{t}");
        // Space above the first line, below the last.
        let p = parse(&t);
        let root = p.syntax();
        let line = |s: &str| {
            let a = t.find(s).unwrap();
            a..a + t[a..].find('\n').unwrap()
        };
        assert_eq!(line_spacing(&root, line("First")), (0, 0));
        assert_eq!(line_spacing(&root, line("second")), (0, 60));
        let p = parse("#+ATTR_KALEM: :before 6\nOne\nTwo\n");
        let root = p.syntax();
        assert_eq!(line_spacing(&root, 24..27), (60, 0));
        assert_eq!(line_spacing(&root, 28..31), (0, 0));
        // Taking both away takes the line away.
        let text = "#+ATTR_KALEM: :before 6\nOne\n";
        let p = parse(text);
        let t = set_spacing(&p.syntax(), text, 25..25, Some(None), Some(None))
            .unwrap()
            .apply(text);
        assert_eq!(t, "One\n");
    }

    #[test]
    fn other_kalem_keys_stay() {
        let text = "#+KALEM: recalc=auto font=\"Times New Roman\"\nbody\n";
        let p = parse(text);
        assert_eq!(
            kalem_option(&p.keywords(), "recalc").as_deref(),
            Some("auto")
        );
        assert_eq!(kalem_option(&p.keywords(), "nothing"), None);
        let t = set_defaults(&p.syntax(), text, |d| d.size = Some(120)).apply(text);
        assert_eq!(
            t,
            "#+KALEM: font=\"Times New Roman\" size=12 recalc=auto\nbody\n"
        );
        let p = parse(&t);
        let t = set_defaults(&p.syntax(), &t, |d| *d = DocDefaults::default()).apply(&t);
        assert_eq!(t, "#+KALEM: recalc=auto\nbody\n");
    }

    #[test]
    fn deleting_keeps_markers() {
        let text = "a @@kalem:size=14@@bc@@kalem:end@@ d\n";
        let p = parse(text);
        let del = |r: Vec<Range<usize>>| {
            let mut tx = Transaction::new("");
            for range in r {
                tx.delete(range).unwrap();
            }
            tx.apply(text)
        };
        // Deleting across the end marker keeps it.
        assert_eq!(
            del(deletion(&p.syntax(), text, 20..36)),
            "a @@kalem:size=14@@b@@kalem:end@@\n"
        );
        // Deleting the whole span takes its markers.
        assert_eq!(del(deletion(&p.syntax(), text, 2..34)), "a  d\n");
        assert_eq!(del(deletion(&p.syntax(), text, 19..21)), "a  d\n");
        // Elsewhere, as it is.
        assert_eq!(deletion(&p.syntax(), text, 0..1), vec![0..1]);
    }
}
