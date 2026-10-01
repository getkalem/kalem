//! The parser of the Kalem format (RFC 0003, Part III of the Book; task
//! T2.13.3): a lossless tree with the byte range of every node, recovery
//! from ill-formed input exactly as §15 says, incremental reparsing from
//! the enclosing top-level block, the canonical form of §14 and a plain
//! semantic HTML. It grew out of the spike of T2.13.2, whose findings are
//! appendix B of the RFC.

use std::collections::HashSet;

use serde_json::{Value, json};

/// A byte range of the source.
pub type Range = (usize, usize);

/// An attribute of a command (§4.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Attr {
    /// `#id`.
    Id(String),
    /// `.style`.
    Style(String),
    /// `key=value`.
    Key(String, String),
    /// `key`, a boolean.
    Flag(String),
    /// A positional value.
    Positional(String),
}

/// What a command holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// No braces.
    None,
    /// Blocks: paragraphs and block commands.
    Blocks(Vec<Node>),
    /// Inline content.
    Inline(Vec<Inline>),
    /// Verbatim text, as written.
    Verbatim(String),
    /// One record a line (`\props`, `\formulas`), trimmed.
    Lines(Vec<String>),
}

/// A command.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    /// Its name.
    pub name: String,
    /// Where the name is, its backslash included.
    pub name_range: Range,
    /// Its attributes, in the order written.
    pub attrs: Vec<Attr>,
    /// Where each attribute is written, in the order of `attrs`.
    pub attr_ranges: Vec<Range>,
    /// The attribute list, its brackets included.
    pub attrs_range: Option<Range>,
    /// Its content.
    pub body: Body,
    /// The content between the braces.
    pub body_range: Option<Range>,
    /// Where it is.
    pub range: Range,
}

/// A block.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A paragraph.
    Paragraph(Vec<Inline>, Range),
    /// A block command.
    Block(Command),
}

/// Inline content.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Text, escapes resolved, and where it is written.
    Text(String, Range),
    /// `$…$`: the formula, and where it is with its dollars.
    Math(String, Range),
    /// An inline command.
    Command(Command),
}

/// A problem found while parsing, and how the parser went on (§15).
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// Where.
    pub range: Range,
    /// Which rule: `unclosed-inline`, `unclosed-block`, `unclosed-math`,
    /// `unclosed-verbatim`, `stray-brace`, `unknown-command`,
    /// `duplicate-id`, `lone-backslash`, `unclosed-attributes`,
    /// `missing-version`, `block-command-inline`, `text-after-block`.
    pub code: &'static str,
    /// For people.
    pub message: String,
}

/// A parsed document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The version of `\klm[…]`; `None` for a fragment (the body of a
    /// document, as the RFC's examples are).
    pub version: Option<String>,
    /// The blocks, `\meta` first when present.
    pub blocks: Vec<Node>,
    /// What the parser recovered from.
    pub diagnostics: Vec<Diagnostic>,
    /// The `#id`s of each top-level block, with where they are written,
    /// for the duplicate check after an incremental parse.
    ids: Vec<Vec<(String, Range)>>,
}

/// The `#id`s in a block, in order.
fn block_ids(n: &Node) -> Vec<(String, Range)> {
    let mut out = Vec::new();
    visit_commands(std::slice::from_ref(n), &mut |c| {
        for (a, r) in c.attrs.iter().zip(&c.attr_ranges) {
            if let Attr::Id(id) = a {
                out.push((id.clone(), *r));
            }
        }
    });
    out
}

/// How a command's content is read and written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A block holding blocks: `\ul{…}` over several lines.
    Container,
    /// A block holding inline content on its line (and, for `\li`, blocks
    /// after it): headings, `\p`, `\li`, `\caption`, `\tr`.
    Line,
    /// A block without content: `\toc`, `\hr`.
    Bare,
    /// Verbatim, as a block or inline: `\code`, `\eq`.
    Verbatim,
    /// One record a line: `\props`, `\formulas`.
    Records,
    /// An inline command.
    Inline,
    /// Not in the specification.
    Unknown,
}

const CONTAINERS: &[&str] = &[
    "meta", "ul", "ol", "dl", "table", "tfoot", "figure", "block", "box", "columns", "log",
];
const LINES: &[&str] = &[
    "part", "h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "dt", "dd", "caption", "tr", "entry",
    "abstract",
];
const BARE: &[&str] = &[
    "klm",
    "toc",
    "lof",
    "lot",
    "printindex",
    "pagebreak",
    "columnbreak",
    "pagesetup",
    "hr",
    "include",
    "bibliography",
    "vspace",
    "linenumbers",
    "clock",
];
const VERBATIM: &[&str] = &["code", "raw", "comment", "eq", "macros", "results"];
const RECORDS: &[&str] = &["props", "formulas"];
const INLINE: &[&str] = &[
    "b", "i", "u", "del", "ins", "hl", "sup", "sub", "sc", "span", "lang", "q", "br", "nbsp",
    "shy", "thinsp", "zwsp", "date", "var", "sym", "index", "gloss", "link", "target", "ref",
    "cite", "fn", "fnref", "note", "img", "th", "td", "noweb",
];
/// Commands whose first bare attribute is a value, not a flag.
const POSITIONAL: &[&str] = &[
    "klm",
    "img",
    "link",
    "ref",
    "cite",
    "date",
    "var",
    "sym",
    "index",
    "gloss",
    "lang",
    "include",
    "columns",
    "vspace",
    "linenumbers",
    "noweb",
    "fnref",
];
/// Inline commands whose content is not optional: empty, they keep their
/// braces (`\td{}`, an empty cell); the others drop them (§14.4).
const CONTENT: &[&str] = &[
    "b", "i", "u", "del", "ins", "hl", "sup", "sub", "sc", "span", "lang", "q", "gloss", "fn",
    "note", "td", "th", "code", "comment", "raw",
];
/// Blocks that follow their owner without a blank line (§14.3).
const ATTACHED: &[&str] = &["props", "log", "results", "formulas", "caption", "tfoot"];

/// How `name` is read.
pub fn kind(name: &str) -> Kind {
    if CONTAINERS.contains(&name) {
        Kind::Container
    } else if LINES.contains(&name) {
        Kind::Line
    } else if BARE.contains(&name) {
        Kind::Bare
    } else if VERBATIM.contains(&name) {
        Kind::Verbatim
    } else if RECORDS.contains(&name) {
        Kind::Records
    } else if INLINE.contains(&name) {
        Kind::Inline
    } else {
        Kind::Unknown
    }
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_lowercase()
}

fn is_name(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    /// Where parsing stops (an unclosed block's recovered end).
    end: usize,
    diags: Vec<Diagnostic>,
    /// A paragraph just ended at the `}` that closes its block.
    pending_close: bool,
    /// Where the last closing brace of a block was.
    last_close: usize,
}

/// What one step of the block loop found.
enum Step {
    /// A block, or nothing (blank content).
    Node(Option<Node>),
    /// The `}` closing the enclosing block.
    Closed,
}

/// Where an inline starts.
fn inline_start(i: &Inline) -> usize {
    match i {
        Inline::Text(_, r) | Inline::Math(_, r) => r.0,
        Inline::Command(c) => c.range.0,
    }
}

/// Where an inline ends.
fn inline_end(i: &Inline) -> usize {
    match i {
        Inline::Text(_, r) | Inline::Math(_, r) => r.1,
        Inline::Command(c) => c.range.1,
    }
}

/// Parses `src`.
pub fn parse(src: &str) -> Document {
    let mut p = Parser {
        src,
        b: src.as_bytes(),
        pos: 0,
        end: src.len(),
        diags: Vec::new(),
        pending_close: false,
        last_close: 0,
    };
    let version = if let Some(rest) = src.strip_prefix("\\klm[") {
        let close = rest.find(']').map(|i| i + 5);
        let v = close.map(|c| src[5..c].trim().to_string());
        p.pos = close.map_or(src.len(), |c| c + 1);
        p.rest_of_line();
        v
    } else {
        // A fragment: the body of a document (the RFC's examples).
        None
    };
    let (blocks, _) = p.blocks(false, 0);
    let ids = blocks.iter().map(block_ids).collect();
    let mut doc = Document {
        version,
        blocks,
        diagnostics: p.diags,
        ids,
    };
    finish(&mut doc);
    doc
}

/// The checks over the whole document, after its blocks are parsed each
/// on its own: a duplicate `#id` (the first keeps it, §15); diagnostics
/// in order.
fn finish(doc: &mut Document) {
    doc.diagnostics.retain(|d| d.code != "duplicate-id");
    let mut seen: HashSet<&str> = HashSet::new();
    let mut found = Vec::new();
    for (id, r) in doc.ids.iter().flatten() {
        if !seen.insert(id.as_str()) {
            found.push(Diagnostic {
                range: *r,
                code: "duplicate-id",
                message: format!("#{id} is used before; the first keeps it"),
            });
        }
    }
    doc.diagnostics.extend(found);
    doc.diagnostics.sort_by_key(|d| d.range);
}

/// Calls `f` on every command, in document order.
pub fn visit_commands<'a>(blocks: &'a [Node], f: &mut dyn FnMut(&'a Command)) {
    fn inl<'a>(i: &'a [Inline], f: &mut dyn FnMut(&'a Command)) {
        for x in i {
            if let Inline::Command(c) = x {
                cmd(c, f);
            }
        }
    }
    fn cmd<'a>(c: &'a Command, f: &mut dyn FnMut(&'a Command)) {
        f(c);
        match &c.body {
            Body::Blocks(b) => visit_commands(b, f),
            Body::Inline(i) => inl(i, f),
            _ => {}
        }
    }
    for b in blocks {
        match b {
            Node::Paragraph(i, _) => inl(i, f),
            Node::Block(c) => cmd(c, f),
        }
    }
}

impl Node {
    /// Where the block is.
    pub fn range(&self) -> Range {
        match self {
            Node::Paragraph(_, r) => *r,
            Node::Block(c) => c.range,
        }
    }
}

/// `doc`, the tree of `old`, after `old[edit]` was replaced so that the
/// text is now `new`: reparsed from the top-level block before the one
/// the edit touches up to the first unchanged top-level block, the rest
/// moved. The same as `parse(new)`.
pub fn reparse(doc: Document, old: &str, edit: (usize, usize), new: &str) -> Document {
    let header = old.find('\n').unwrap_or(old.len());
    if edit.0 <= header
        || doc.version.is_none() && old.starts_with("\\klm[") != new.starts_with("\\klm[")
    {
        return parse(new);
    }
    let delta = new.len() as isize - old.len() as isize;
    let line_start = |i: usize| old[..i].rfind('\n').map_or(0, |n| n + 1);
    let n = doc.blocks.len();
    // The blocks whose line starts at or before the edit, by bisection.
    let mut touched = doc.blocks.partition_point(|b| b.range().0 <= edit.0);
    if touched < n && line_start(doc.blocks[touched].range().0) <= edit.0 {
        touched += 1;
    }
    let first = touched.saturating_sub(2);
    if first == 0 && doc.version.is_some() {
        return parse(new);
    }
    let from = if first == 0 {
        0
    } else {
        line_start(doc.blocks[first].range().0)
    };
    let mut p = Parser {
        src: new,
        b: new.as_bytes(),
        pos: from,
        end: new.len(),
        diags: Vec::new(),
        pending_close: false,
        last_close: 0,
    };
    let Document {
        version,
        blocks: mut old_blocks,
        diagnostics: old_diags,
        ids: mut old_ids,
    } = doc;
    let mut rest = old_blocks.split_off(first);
    let mut rest_ids = old_ids.split_off(first);
    let mut blocks = old_blocks;
    // Where the new parse joins an old block that starts after the edit:
    // its index among `rest`.
    let edit_end_new = (edit.1 as isize + delta) as usize;
    let joins = |pos: usize, rest: &[Node]| -> Option<usize> {
        if pos <= edit_end_new {
            return None;
        }
        let q = (pos as isize - delta) as usize;
        let k = rest.partition_point(|b| b.range().0 < q);
        // The block whose line starts at `q`: the one at or after it.
        [k.checked_sub(1), Some(k)]
            .into_iter()
            .flatten()
            .filter(|&k| k < rest.len())
            .find(|&k| {
                let r = rest[k].range().0;
                r >= q && old[q..r].bytes().all(|b| b == b' ' || b == b'\t') && line_start(r) == q
            })
    };
    let mut reused = None;
    loop {
        p.skip_blank_lines();
        if let Some(k) = joins(p.pos, &rest) {
            reused = Some(first + k);
            break;
        }
        let (mut one, _) = p.blocks_one();
        if one.is_empty() {
            break;
        }
        blocks.append(&mut one);
    }
    let starts_at = |i: usize, rest: &[Node]| line_start(rest[i - first].range().0);
    // The duplicate ids change only when a block with an id is parsed
    // again or goes away.
    let gone = reused.map_or(rest.len(), |i| i - first);
    let mut ids = old_ids;
    ids.extend(blocks[first..].iter().map(block_ids));
    let ids_changed = ids[first..].iter().any(|v| !v.is_empty())
        || rest_ids[..gone].iter().any(|v| !v.is_empty());
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut later_diags = Vec::new();
    let cut = reused.map(|i| starts_at(i, &rest));
    for d in old_diags {
        if d.code == "duplicate-id" && ids_changed {
            continue;
        }
        if d.range.1 <= from && d.range.0 < from {
            diagnostics.push(d);
        } else if cut.is_some_and(|c| d.range.0 >= c) {
            later_diags.push(d);
        }
    }
    diagnostics.extend(p.diags);
    if let Some(i) = reused {
        for mut b in rest.drain(i - first..) {
            if delta != 0 {
                shift_node(&mut b, delta);
            }
            blocks.push(b);
        }
        for mut v in rest_ids.drain(i - first..) {
            for (_, r) in &mut v {
                shift_range(r, delta);
            }
            ids.push(v);
        }
        for mut d in later_diags {
            d.range = (shift(d.range.0, delta), shift(d.range.1, delta));
            diagnostics.push(d);
        }
    }
    let mut out = Document {
        version,
        blocks,
        diagnostics,
        ids,
    };
    if ids_changed {
        finish(&mut out);
    } else {
        out.diagnostics.sort_by_key(|d| d.range);
    }
    out
}

fn shift(i: usize, delta: isize) -> usize {
    (i as isize + delta) as usize
}

fn shift_range(r: &mut Range, delta: isize) {
    *r = (shift(r.0, delta), shift(r.1, delta));
}

fn shift_command(c: &mut Command, delta: isize) {
    shift_range(&mut c.range, delta);
    shift_range(&mut c.name_range, delta);
    for r in &mut c.attr_ranges {
        shift_range(r, delta);
    }
    if let Some(r) = &mut c.attrs_range {
        shift_range(r, delta);
    }
    if let Some(r) = &mut c.body_range {
        shift_range(r, delta);
    }
    match &mut c.body {
        Body::Blocks(b) => b.iter_mut().for_each(|n| shift_node(n, delta)),
        Body::Inline(i) => shift_inlines(i, delta),
        _ => {}
    }
}

fn shift_inlines(i: &mut [Inline], delta: isize) {
    for x in i {
        match x {
            Inline::Text(_, r) | Inline::Math(_, r) => shift_range(r, delta),
            Inline::Command(c) => shift_command(c, delta),
        }
    }
}

fn shift_node(n: &mut Node, delta: isize) {
    match n {
        Node::Paragraph(i, r) => {
            shift_range(r, delta);
            shift_inlines(i, delta);
        }
        Node::Block(c) => shift_command(c, delta),
    }
}

impl Parser<'_> {
    fn diag(&mut self, range: Range, code: &'static str, message: impl Into<String>) {
        self.diags.push(Diagnostic {
            range,
            code,
            message: message.into(),
        });
    }

    fn at(&self, i: usize) -> Option<u8> {
        (i < self.end).then(|| self.b[i])
    }

    fn eof(&self) -> bool {
        self.pos >= self.end
    }

    /// The end of the line holding `i` (at its line feed, or the end).
    fn line_end(&self, i: usize) -> usize {
        self.src[i..self.end].find('\n').map_or(self.end, |n| i + n)
    }

    fn line_start(&self, i: usize) -> usize {
        self.src[..i].rfind('\n').map_or(0, |n| n + 1)
    }

    /// Whether the line starting at `i` is blank.
    fn blank_line(&self, i: usize) -> bool {
        self.src[i..self.line_end(i)].trim().is_empty()
    }

    /// Past spaces and tabs.
    fn skip_blanks(&mut self) {
        while matches!(self.at(self.pos), Some(b' ' | b'\t' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Past the rest of a line (blanks and its line feed); `true` if only
    /// blanks were there.
    fn rest_of_line(&mut self) -> bool {
        self.skip_blanks();
        match self.at(self.pos) {
            Some(b'\n') => {
                self.pos += 1;
                true
            }
            None => true,
            _ => false,
        }
    }

    fn name_at(&self, i: usize) -> Option<(String, usize)> {
        if self.at(i) != Some(b'\\') || !self.at(i + 1).is_some_and(is_name_start) {
            return None;
        }
        let mut j = i + 1;
        while self.at(j).is_some_and(is_name) {
            j += 1;
        }
        Some((self.src[i + 1..j].to_string(), j))
    }

    /// The end of an attribute list starting at `i` (at `[`), past `]`;
    /// timestamps (`<…>`, `[…]`) and quoted values are tokens.
    fn attrs_end(&self, i: usize) -> usize {
        let mut j = i + 1;
        let mut depth = 0usize;
        let mut quoted = false;
        while let Some(c) = self.at(j) {
            match c {
                b'\n' => return j,
                b'\\' if quoted => j += 1,
                b'"' => quoted = !quoted,
                b'[' | b'<' if !quoted => depth += 1,
                b'>' if !quoted && depth > 0 => depth -= 1,
                b']' if !quoted => {
                    if depth == 0 {
                        return j + 1;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            j += 1;
        }
        j
    }

    /// Whether the line at `i` (after its indentation) starts a block
    /// command.
    fn block_command_at(&self, i: usize) -> bool {
        let Some((name, after)) = self.name_at(i) else {
            return false;
        };
        let mut j = after;
        if self.at(j) == Some(b'[') {
            j = self.attrs_end(j);
        }
        let rest = self.src[j..self.line_end(j)].trim_end();
        match kind(&name) {
            Kind::Container | Kind::Line | Kind::Bare | Kind::Records => true,
            // Verbatim commands are blocks at line start in block form.
            Kind::Verbatim => {
                rest == "{"
                    || rest.is_empty()
                    || matches!(name.as_str(), "eq" | "macros" | "results")
            }
            // A picture alone on its line is a block.
            Kind::Inline => name == "img" && rest.is_empty(),
            Kind::Unknown => rest == "{" || rest.is_empty(),
        }
    }

    /// Past blank lines.
    fn skip_blank_lines(&mut self) {
        while !self.eof() && self.blank_line(self.pos) {
            let e = self.line_end(self.pos);
            self.pos = (e + 1).min(self.end);
        }
    }

    /// One block at the top level (after blank lines): none at the end.
    fn blocks_one(&mut self) -> (Vec<Node>, bool) {
        self.skip_blank_lines();
        if self.eof() {
            return (Vec::new(), false);
        }
        match self.block_step(false, 0) {
            Step::Node(n) => (n.into_iter().collect(), false),
            Step::Closed => (Vec::new(), true),
        }
    }

    /// Blocks up to the end, or up to the `}` that closes the enclosing
    /// command when `closing`; whether that `}` came. `indent` is the
    /// enclosing command's indentation (for recovery).
    fn blocks(&mut self, closing: bool, indent: usize) -> (Vec<Node>, bool) {
        let mut out = Vec::new();
        loop {
            self.skip_blank_lines();
            if self.eof() {
                return (out, false);
            }
            match self.block_step(closing, indent) {
                Step::Node(n) => out.extend(n),
                Step::Closed => return (out, true),
            }
            if std::mem::take(&mut self.pending_close) {
                return (out, true);
            }
        }
    }

    /// The block at the position (not blank, not at the end).
    fn block_step(&mut self, closing: bool, _indent: usize) -> Step {
        let start = self.pos;
        self.skip_blanks();
        let first = self.pos;
        if closing && self.at(first) == Some(b'}') {
            self.last_close = first;
            self.pos += 1;
            if !self.rest_of_line() {
                let e = self.line_end(self.pos);
                self.diag(
                    (self.pos, e),
                    "text-after-block",
                    "text after a closing brace",
                );
            }
            return Step::Closed;
        }
        if self.block_command_at(first) {
            let col = first - self.line_start(first);
            return Step::Node(Some(Node::Block(self.block_command(col))));
        }
        self.pos = start;
        let (inl, closed) = self.inlines(Stop::Paragraph { closing });
        let begin = inl.first().map_or(first, inline_start).min(first);
        let end = inl.last().map_or(first, inline_end).max(first);
        let range = (begin, end);
        let node = (!inl.is_empty()).then_some(Node::Paragraph(inl, range));
        if closed {
            self.last_close = self.pos - 1;
            self.rest_of_line();
            if let Some(n) = node {
                // The paragraph, then the close: the caller sees the close
                // next time round at the same place.
                self.pending_close = true;
                return Step::Node(Some(n));
            }
            return Step::Closed;
        }
        Step::Node(node)
    }

    fn block_command(&mut self, col: usize) -> Command {
        let start = self.pos;
        let (name, after) = self.name_at(start).expect("a command");
        self.pos = after;
        let (attrs, attr_ranges, attrs_range) = self.attributes(&name);
        let k = kind(&name);
        let mut body_range = None;
        let body = if self.at(self.pos) == Some(b'{') {
            match k {
                Kind::Verbatim | Kind::Records => {
                    let open = self.pos;
                    let (v, closed) = self.verbatim_closed(true);
                    let close = if closed { self.pos - 1 } else { self.pos };
                    body_range = Some((open + 1, close.max(open + 1)));
                    if k == Kind::Verbatim {
                        Body::Verbatim(v)
                    } else {
                        Body::Lines(
                            v.lines()
                                .map(str::trim)
                                .filter(|l| !l.is_empty())
                                .map(str::to_string)
                                .collect(),
                        )
                    }
                }
                _ => {
                    self.pos += 1;
                    let content = self.pos;
                    let (blocks, closed) = self.blocks(true, col);
                    if closed {
                        body_range = Some((content, self.last_close.max(content)));
                        Body::Blocks(blocks)
                    } else {
                        let b = self.recover_block(start, content, col);
                        body_range = Some((content, self.pos.max(content)));
                        Body::Blocks(b)
                    }
                }
            }
        } else {
            Body::None
        };
        if !matches!(body, Body::Blocks(_)) && !self.rest_of_line() {
            let e = self.line_end(self.pos);
            self.diag(
                (self.pos, e),
                "text-after-block",
                "text after a block command",
            );
        }
        if k == Kind::Unknown {
            self.diag(
                (start, after),
                "unknown-command",
                format!("\\{name} is not in the specification; read as a generic block"),
            );
        }
        // A block ends with its closing brace or its content, not with the
        // line feed after it.
        let end = body_range
            .map(|r| {
                if self.b.get(r.1) == Some(&b'}') {
                    r.1 + 1
                } else {
                    r.1
                }
            })
            .or(attrs_range.map(|r| r.1))
            .unwrap_or(after);
        Command {
            name,
            name_range: (start, after),
            attrs,
            attr_ranges,
            attrs_range,
            body,
            body_range,
            range: (start, end.max(after)),
        }
    }

    /// An unclosed block (§15): it ends before the next block command at
    /// the same or a lower indentation that starts a line, else at the
    /// end. Reparsed up to there.
    fn recover_block(&mut self, start: usize, content: usize, col: usize) -> Vec<Node> {
        let mut cut = self.end;
        let mut i = self.line_end(content);
        while i < self.end {
            let ls = i + 1;
            let mut j = ls;
            while matches!(self.at(j), Some(b' ' | b'\t')) {
                j += 1;
            }
            if j - ls <= col && self.block_command_at(j) {
                cut = ls;
                break;
            }
            i = self.line_end(ls);
        }
        self.diag(
            (start, content),
            "unclosed-block",
            "a block command without its closing brace; it ends before the next block at its indentation",
        );
        let saved = self.end;
        self.end = cut;
        self.pos = content;
        let (blocks, _) = self.blocks(false, col);
        self.end = saved;
        self.pos = cut;
        blocks
    }

    fn attributes(&mut self, name: &str) -> (Vec<Attr>, Vec<Range>, Option<Range>) {
        let mut out = Vec::new();
        let mut ranges = Vec::new();
        if self.at(self.pos) != Some(b'[') {
            return (out, ranges, None);
        }
        let open = self.pos;
        let end = self.attrs_end(open);
        let closed = self.b.get(end - 1) == Some(&b']') && end > open + 1;
        let inner_end = if closed { end - 1 } else { end };
        if !closed {
            self.diag(
                (open, end),
                "unclosed-attributes",
                "an attribute list without `]`",
            );
        }
        let mut i = open + 1;
        let mut positional_taken = false;
        while i < inner_end {
            while i < inner_end && self.b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= inner_end {
                break;
            }
            let at = i;
            let (tok, next) = self.token(i, inner_end);
            i = next;
            if let Some(id) = tok.strip_prefix('#').filter(|_| !tok.contains('=')) {
                out.push(Attr::Id(id.to_string()));
            } else if let Some(s) = tok
                .strip_prefix('.')
                .filter(|s| !s.is_empty() && s.bytes().all(is_name))
            {
                out.push(Attr::Style(s.to_string()));
            } else if i < inner_end && self.b[i] == b'=' {
                let (v, next) = self.token(i + 1, inner_end);
                i = next;
                out.push(Attr::Key(tok, unquote(&v)));
            } else if !positional_taken
                && (POSITIONAL.contains(&name) || !is_key(&tok) || tok.starts_with('"'))
            {
                positional_taken = true;
                out.push(Attr::Positional(unquote(&tok)));
            } else {
                out.push(Attr::Flag(tok));
            }
            ranges.push((at, i));
        }
        self.pos = end;
        (out, ranges, Some((open, end)))
    }

    /// A token of an attribute list from `i`: a quoted string, a
    /// timestamp, or a bare run; returned as written.
    fn token(&self, i: usize, end: usize) -> (String, usize) {
        let mut j = i;
        match self.b.get(i) {
            Some(b'"') => {
                j += 1;
                while j < end && self.b[j] != b'"' {
                    if self.b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                j = (j + 1).min(end);
            }
            Some(&open @ (b'<' | b'[')) => {
                let close = if open == b'<' { b'>' } else { b']' };
                let mut depth = 0;
                while j < end {
                    if self.b[j] == open {
                        depth += 1;
                    } else if self.b[j] == close {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    j += 1;
                }
            }
            _ => {
                while j < end
                    && !self.b[j].is_ascii_whitespace()
                    && !matches!(self.b[j], b'=' | b'"')
                {
                    j += 1;
                }
            }
        }
        (self.src[i..j].to_string(), j)
    }

    /// Verbatim content from `{` at the position: as written, braces
    /// balanced, a backslash taking the next character with it. As a
    /// block, the content is the lines between the opening line and the
    /// line of the closing brace. With whether the closing brace came.
    fn verbatim_closed(&mut self, block: bool) -> (String, bool) {
        let open = self.pos;
        let mut j = open + 1;
        let mut depth = 1;
        let mut close = None;
        while let Some(c) = self.at(j) {
            match c {
                b'\\' => j += 1,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(j);
                        break;
                    }
                }
                b'\n' if !block && self.blank_line(j + 1) => break,
                _ => {}
            }
            j += 1;
        }
        let Some(close) = close else {
            // Unclosed: to the end (a block) or of the paragraph.
            let (code, until) = if block {
                ("unclosed-verbatim", self.end)
            } else {
                ("unclosed-inline", j.min(self.end))
            };
            self.diag(
                (open, until),
                code,
                "verbatim content without its closing brace",
            );
            self.pos = until;
            return (self.src[open + 1..until].to_string(), false);
        };
        self.pos = close + 1;
        let inner = &self.src[open + 1..close];
        if block && inner.starts_with('\n') {
            // From the next line to the start of the closing brace's line.
            let body = &inner[1..];
            let line = body.rfind('\n').map_or(0, |n| n + 1);
            if body[line..].trim().is_empty() {
                return (body[..line].to_string(), true);
            }
            return (body.to_string(), true);
        }
        (inner.to_string(), true)
    }

    /// Inline content until `stop`; whether a closing brace ended it.
    fn inlines(&mut self, stop: Stop) -> (Vec<Inline>, bool) {
        let mut out: Vec<Inline> = Vec::new();
        let mut text = String::new();
        // Where the text being gathered starts.
        let mut from = self.pos;
        let flush = |text: &mut String, out: &mut Vec<Inline>, from: &mut usize, to: usize| {
            if !text.is_empty() {
                out.push(Inline::Text(std::mem::take(text), (*from, to)));
            }
            *from = to;
        };
        while let Some(c) = self.at(self.pos) {
            if text.is_empty() {
                from = self.pos;
            }
            match c {
                b'\\' => {
                    let n = self.at(self.pos + 1);
                    if let Some(e @ (b'\\' | b'{' | b'}' | b'$')) = n {
                        text.push(e as char);
                        self.pos += 2;
                    } else if n.is_some_and(is_name_start) {
                        flush(&mut text, &mut out, &mut from, self.pos);
                        let (name, _) = self.name_at(self.pos).expect("a name");
                        if !matches!(kind(&name), Kind::Inline | Kind::Verbatim | Kind::Unknown)
                            && name != "img"
                        {
                            let p = self.pos;
                            self.diag(
                                (p, p + name.len() + 1),
                                "block-command-inline",
                                format!("\\{name} is a block command, here inside a paragraph"),
                            );
                        }
                        out.push(Inline::Command(self.inline_command()));
                    } else {
                        let p = self.pos;
                        self.diag(
                            (p, p + 1),
                            "lone-backslash",
                            "a backslash that starts nothing",
                        );
                        text.push('\\');
                        self.pos += 1;
                    }
                }
                b'$' => {
                    flush(&mut text, &mut out, &mut from, self.pos);
                    out.push(self.math());
                }
                b'{' => {
                    let p = self.pos;
                    self.diag((p, p + 1), "stray-brace", "an opening brace in text");
                    text.push('{');
                    self.pos += 1;
                }
                b'}' => match stop {
                    Stop::Brace => {
                        flush(&mut text, &mut out, &mut from, self.pos);
                        self.pos += 1;
                        return (out, true);
                    }
                    Stop::Paragraph { closing: true } => {
                        flush(&mut text, &mut out, &mut from, self.pos);
                        self.pos += 1;
                        trim_end(&mut out);
                        return (out, true);
                    }
                    Stop::Paragraph { closing: false } => {
                        let p = self.pos;
                        self.diag(
                            (p, p + 1),
                            "stray-brace",
                            "a closing brace that closes nothing",
                        );
                        text.push('}');
                        self.pos += 1;
                    }
                },
                b'\n' => {
                    // A paragraph ends at a blank line or before a block
                    // command that starts a line.
                    let next = self.pos + 1;
                    let mut j = next;
                    while matches!(self.at(j), Some(b' ' | b'\t')) {
                        j += 1;
                    }
                    let ends = next >= self.end
                        || self.blank_line(next)
                        || self.block_command_at(j)
                        || (matches!(stop, Stop::Paragraph { closing: true })
                            && self.at(j) == Some(b'}'));
                    if ends {
                        if matches!(stop, Stop::Brace) {
                            let p = self.pos;
                            self.diag(
                                (p, p),
                                "unclosed-inline",
                                "an inline command without its closing brace; it ends with its paragraph",
                            );
                            flush(&mut text, &mut out, &mut from, self.pos);
                            return (out, false);
                        }
                        flush(&mut text, &mut out, &mut from, self.pos);
                        self.pos = next;
                        trim_end(&mut out);
                        return (out, false);
                    }
                    text.push('\n');
                    self.pos += 1;
                }
                _ => {
                    let ch = self.src[self.pos..].chars().next().expect("a character");
                    text.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        if matches!(stop, Stop::Brace) {
            let p = self.pos;
            self.diag(
                (p, p),
                "unclosed-inline",
                "an inline command without its closing brace",
            );
        }
        flush(&mut text, &mut out, &mut from, self.pos);
        trim_end(&mut out);
        (out, false)
    }

    fn inline_command(&mut self) -> Command {
        let start = self.pos;
        let (name, after) = self.name_at(start).expect("a command");
        self.pos = after;
        let (attrs, attr_ranges, attrs_range) = self.attributes(&name);
        let mut body_range = None;
        let body = if self.at(self.pos) == Some(b'{') {
            let content = self.pos + 1;
            let (b, closed) = if kind(&name) == Kind::Verbatim {
                let (v, closed) = self.verbatim_closed(false);
                (Body::Verbatim(v), closed)
            } else {
                self.pos += 1;
                let (inl, closed) = self.inlines(Stop::Brace);
                (Body::Inline(inl), closed)
            };
            let close = if closed { self.pos - 1 } else { self.pos };
            body_range = Some((content, close.max(content)));
            b
        } else {
            Body::None
        };
        if kind(&name) == Kind::Unknown {
            self.diag(
                (start, after),
                "unknown-command",
                format!("\\{name} is not in the specification; read as a generic span"),
            );
        }
        Command {
            name,
            name_range: (start, after),
            attrs,
            attr_ranges,
            attrs_range,
            body,
            body_range,
            range: (start, self.pos),
        }
    }

    fn math(&mut self) -> Inline {
        let open = self.pos;
        let mut j = open + 1;
        while let Some(c) = self.at(j) {
            match c {
                b'\\' => j += 1,
                b'$' => {
                    self.pos = j + 1;
                    return Inline::Math(self.src[open + 1..j].to_string(), (open, j + 1));
                }
                b'\n' if self.blank_line(j + 1) || j + 1 >= self.end => break,
                _ => {}
            }
            j += 1;
        }
        let until = j.min(self.end);
        self.diag(
            (open, until),
            "unclosed-math",
            "`$` without its pair; the formula ends with its paragraph",
        );
        self.pos = until;
        Inline::Math(self.src[open + 1..until].to_string(), (open, until))
    }
}

#[derive(Debug, Clone, Copy)]
enum Stop {
    /// A paragraph; `closing` when a `}` closes the enclosing block.
    Paragraph { closing: bool },
    /// An inline command's content.
    Brace,
}

fn trim_end(out: &mut Vec<Inline>) {
    if let Some(Inline::Text(t, r)) = out.last_mut() {
        let n = t.trim_end().len();
        // Blanks are written as they are, so the range shrinks as much.
        r.1 -= t.len() - n;
        t.truncate(n);
        if t.is_empty() {
            out.pop();
        }
    }
}

fn is_key(s: &str) -> bool {
    s.split('.').all(|p| {
        p.as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic())
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

fn unquote(v: &str) -> String {
    match v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => v.to_string(),
    }
}

// ---------------------------------------------------------------------
// The model: what two parses must agree on (no ranges, whitespace of text
// collapsed, empty content the same as none).

/// The document as JSON, for `parse(fmt(x)) = parse(x)` and the suite.
pub fn model(doc: &Document) -> Value {
    SEEN_IDS.with(|s| s.borrow_mut().clear());
    json!({
        "version": doc.version,
        "blocks": doc.blocks.iter().map(block_model).collect::<Vec<_>>(),
    })
}

fn block_model(n: &Node) -> Value {
    match n {
        Node::Paragraph(inl, _) => json!({ "p": inlines_model(inl) }),
        Node::Block(c) => command_model(c),
    }
}

thread_local! {
    /// The ids the model has seen: a later duplicate is not in it (§15).
    static SEEN_IDS: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
}

fn command_model(c: &Command) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("cmd".into(), json!(c.name));
    let attrs: Vec<&Attr> = c
        .attrs
        .iter()
        .filter(|a| match a {
            Attr::Id(id) => SEEN_IDS.with(|s| s.borrow_mut().insert(id.clone())),
            _ => true,
        })
        .collect();
    if !attrs.is_empty() {
        // The order of kinds is the serializer's; within a kind, as written.
        let mut attrs = attrs;
        attrs.sort_by_key(|a| rank(a));
        m.insert(
            "attrs".into(),
            Value::Array(attrs.into_iter().map(attr_model).collect()),
        );
    }
    match &c.body {
        Body::None => {}
        Body::Blocks(b) if b.is_empty() => {}
        Body::Inline(i) if inlines_model(i).is_empty() => {}
        Body::Blocks(b) => {
            m.insert(
                "blocks".into(),
                Value::Array(b.iter().map(block_model).collect()),
            );
        }
        Body::Inline(i) => {
            m.insert("inline".into(), Value::Array(inlines_model(i)));
        }
        Body::Verbatim(v) => {
            // A block's last line feed is the closing brace's line.
            m.insert("verbatim".into(), json!(v.strip_suffix('\n').unwrap_or(v)));
        }
        Body::Lines(l) => {
            m.insert("lines".into(), json!(l));
        }
    }
    Value::Object(m)
}

fn attr_model(a: &Attr) -> Value {
    match a {
        Attr::Id(v) => json!({ "id": v }),
        Attr::Style(v) => json!({ "style": v }),
        Attr::Key(k, v) => json!({ "key": k, "value": v }),
        Attr::Flag(k) => json!({ "flag": k }),
        Attr::Positional(v) => json!({ "value": v }),
    }
}

fn inlines_model(inl: &[Inline]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for i in inl {
        match i {
            Inline::Text(t, _) => {
                let t = collapse(t);
                if let Some(Value::String(prev)) = out.last_mut() {
                    prev.push_str(&t);
                } else if !t.is_empty() {
                    out.push(Value::String(t));
                }
            }
            Inline::Math(m, _) => out.push(json!({ "math": m })),
            Inline::Command(c) => out.push(command_model(c)),
        }
    }
    // Blanks at the edges of a paragraph are not content.
    if let Some(Value::String(s)) = out.first_mut() {
        *s = s.trim_start().to_string();
    }
    if let Some(Value::String(s)) = out.last_mut() {
        *s = s.trim_end().to_string();
    }
    out.retain(|v| v.as_str() != Some(""));
    out
}

fn collapse(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut blank = false;
    for c in t.chars() {
        if c.is_whitespace() {
            if !blank {
                out.push(' ');
            }
            blank = true;
        } else {
            out.push(c);
            blank = false;
        }
    }
    out
}

// ---------------------------------------------------------------------
// The canonical form (§14).

/// The canonical bytes of `doc`.
pub fn fmt(doc: &Document) -> String {
    let mut out = String::new();
    let mut blocks = doc.blocks.as_slice();
    if let Some(v) = &doc.version {
        out.push_str(&format!("\\klm[{v}]\n"));
        if let Some(Node::Block(c)) = blocks.first()
            && c.name == "meta"
        {
            fmt_block(&blocks[0], 0, &mut out);
            blocks = &blocks[1..];
        }
        if !blocks.is_empty() {
            out.push('\n');
        }
    }
    fmt_blocks(blocks, 0, true, &mut out);
    out
}

fn attached(n: &Node) -> bool {
    matches!(n, Node::Block(c) if ATTACHED.contains(&c.name.as_str()))
}

fn fmt_blocks(nodes: &[Node], indent: usize, top: bool, out: &mut String) {
    for (i, n) in nodes.iter().enumerate() {
        if i > 0 {
            let prev = &nodes[i - 1];
            let para = matches!(n, Node::Paragraph(..)) || matches!(prev, Node::Paragraph(..));
            if !attached(n) && (top || para) {
                out.push('\n');
            }
        }
        fmt_block(n, indent, out);
    }
}

fn fmt_block(n: &Node, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match n {
        Node::Paragraph(inl, _) => {
            out.push_str(&pad);
            out.push_str(fmt_inlines(inl).trim());
            out.push('\n');
        }
        Node::Block(c) => {
            out.push_str(&pad);
            out.push('\\');
            out.push_str(&c.name);
            out.push_str(&fmt_attrs(&c.attrs));
            match &c.body {
                Body::None => out.push('\n'),
                Body::Verbatim(v) => {
                    out.push_str("{\n");
                    out.push_str(v);
                    if !v.is_empty() && !v.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&pad);
                    out.push_str("}\n");
                }
                Body::Lines(lines) => {
                    out.push_str("{\n");
                    for l in lines {
                        out.push_str(&pad);
                        out.push_str("  ");
                        out.push_str(l);
                        out.push('\n');
                    }
                    out.push_str(&pad);
                    out.push_str("}\n");
                }
                Body::Inline(inl) => {
                    out.push('{');
                    out.push_str(&fmt_inlines(inl));
                    out.push_str("}\n");
                }
                Body::Blocks(children) => {
                    let line = kind(&c.name) == Kind::Line;
                    match children.split_first() {
                        None => out.push_str("{}\n"),
                        Some((Node::Paragraph(inl, _), [])) if line => {
                            out.push('{');
                            out.push_str(fmt_inlines(inl).trim());
                            out.push_str("}\n");
                        }
                        Some((Node::Paragraph(inl, _), rest)) if line => {
                            out.push('{');
                            out.push_str(fmt_inlines(inl).trim());
                            out.push('\n');
                            fmt_blocks(rest, indent + 2, false, out);
                            out.push_str(&pad);
                            out.push_str("}\n");
                        }
                        _ => {
                            out.push_str("{\n");
                            fmt_blocks(children, indent + 2, false, out);
                            out.push_str(&pad);
                            out.push_str("}\n");
                        }
                    }
                }
            }
        }
    }
}

fn fmt_inlines(inl: &[Inline]) -> String {
    let mut out = String::new();
    for (i, x) in inl.iter().enumerate() {
        match x {
            Inline::Text(t, _) => {
                // Paragraph text is one line (§14.5).
                let t = join_lines(t);
                for ch in t.chars() {
                    match ch {
                        '\\' | '{' | '}' | '$' => {
                            out.push('\\');
                            out.push(ch);
                        }
                        c => out.push(c),
                    }
                }
            }
            Inline::Math(m, _) => {
                out.push('$');
                out.push_str(m);
                out.push('$');
            }
            Inline::Command(c) => {
                out.push('\\');
                out.push_str(&c.name);
                out.push_str(&fmt_attrs(&c.attrs));
                let empty = match &c.body {
                    Body::None => true,
                    Body::Inline(v) => inlines_model(v).is_empty(),
                    Body::Blocks(v) => v.is_empty(),
                    _ => false,
                };
                match &c.body {
                    Body::Verbatim(v) => {
                        out.push('{');
                        out.push_str(v);
                        out.push('}');
                    }
                    Body::Inline(v) if !empty => {
                        out.push('{');
                        out.push_str(&fmt_inlines(v));
                        out.push('}');
                    }
                    _ if CONTENT.contains(&c.name.as_str()) => out.push_str("{}"),
                    _ => {
                        // Without braces, unless what follows would be read
                        // as more of the name or as its attributes.
                        let next = match inl.get(i + 1) {
                            Some(Inline::Text(t, _)) => join_lines(t).chars().next(),
                            _ => None,
                        };
                        let needs = next.is_some_and(|ch| {
                            ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '['
                        });
                        if needs {
                            out.push_str("{}");
                        }
                    }
                }
            }
        }
    }
    out
}

fn join_lines(t: &str) -> String {
    if !t.contains('\n') {
        return t.to_string();
    }
    let mut out = String::new();
    for (i, l) in t.split('\n').enumerate() {
        let l = if i == 0 { l.trim_end() } else { l.trim() };
        if i > 0 && !out.is_empty() && !l.is_empty() {
            out.push(' ');
        }
        out.push_str(l);
    }
    out
}

fn fmt_attrs(attrs: &[Attr]) -> String {
    if attrs.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&Attr> = attrs.iter().collect();
    sorted.sort_by_key(|a| rank(a));
    let items: Vec<String> = sorted
        .into_iter()
        .map(|a| match a {
            Attr::Id(v) => format!("#{v}"),
            Attr::Style(v) => format!(".{v}"),
            Attr::Key(k, v) => format!("{k}={}", value(v)),
            Attr::Flag(k) => k.clone(),
            Attr::Positional(v) => value(v),
        })
        .collect();
    format!("[{}]", items.join(" "))
}

/// Where an attribute goes (§14.6): `#id`, `.style`s, the positional
/// value, then keys as written.
fn rank(a: &Attr) -> u8 {
    match a {
        Attr::Id(_) => 0,
        Attr::Style(_) => 1,
        Attr::Positional(_) => 2,
        _ => 3,
    }
}

/// A value as written: bare when it can be, a timestamp as it is, else
/// quoted.
fn value(v: &str) -> String {
    let timestamp = (v.starts_with('<') && v.ends_with('>'))
        || (v.starts_with('[') && v.ends_with(']') && v.len() > 1);
    if timestamp && !v.contains('"') && !v.contains('\n') {
        return v.to_string();
    }
    let bare = !v.is_empty()
        && !v.starts_with(['<', '[', '#', '.'])
        && !v
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ']' | '"' | '=' | '\\'));
    if bare {
        v.to_string()
    } else {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

// ---------------------------------------------------------------------
// A plain semantic HTML, the suite's third file.

/// The document as HTML.
pub fn html(doc: &Document) -> String {
    let mut out = String::from("<article class=\"klm\">\n");
    for b in &doc.blocks {
        html_block(b, &mut out);
    }
    out.push_str("</article>\n");
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn id_attr(c: &Command) -> String {
    c.attrs
        .iter()
        .find_map(|a| match a {
            Attr::Id(v) => Some(format!(" id=\"{}\"", esc(v))),
            _ => None,
        })
        .unwrap_or_default()
}

fn positional(c: &Command) -> Option<&str> {
    c.attrs.iter().find_map(|a| match a {
        Attr::Positional(v) => Some(v.as_str()),
        _ => None,
    })
}

fn html_block(n: &Node, out: &mut String) {
    match n {
        Node::Paragraph(inl, _) => {
            out.push_str("<p>");
            html_inlines(inl, out);
            out.push_str("</p>\n");
        }
        Node::Block(c) => {
            let tag = match c.name.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "dl" | "dt"
                | "dd" | "table" | "tr" | "tfoot" | "figure" | "p" => c.name.as_str(),
                "caption" => "figcaption",
                "part" => "h1",
                "hr" => {
                    out.push_str("<hr>\n");
                    return;
                }
                "meta" | "props" | "log" | "formulas" | "comment" | "klm" => return,
                "eq" => {
                    if let Body::Verbatim(v) = &c.body {
                        out.push_str(&format!(
                            "<div class=\"math display\"{}>\\[{}\\]</div>\n",
                            id_attr(c),
                            esc(v.trim())
                        ));
                    }
                    return;
                }
                "code" | "results" | "raw" | "macros" => {
                    if let Body::Verbatim(v) = &c.body {
                        out.push_str(&format!("<pre data-kind=\"{}\">{}</pre>\n", c.name, esc(v)));
                    }
                    return;
                }
                "img" => {
                    out.push_str(&format!(
                        "<img src=\"{}\">\n",
                        esc(positional(c).unwrap_or_default())
                    ));
                    return;
                }
                _ => "div",
            };
            let kind = if tag == "div" {
                format!(" data-kind=\"{}\"", c.name)
            } else {
                String::new()
            };
            let styles: Vec<&str> = c
                .attrs
                .iter()
                .filter_map(|a| match a {
                    Attr::Style(v) => Some(v.as_str()),
                    _ => None,
                })
                .collect();
            let class = if styles.is_empty() {
                String::new()
            } else {
                format!(" class=\"{}\"", esc(&styles.join(" ")))
            };
            out.push_str(&format!("<{tag}{}{class}{kind}>", id_attr(c)));
            match &c.body {
                Body::Blocks(b) => {
                    // A line command's one paragraph is its content.
                    if let [Node::Paragraph(inl, _)] = b.as_slice()
                        && kind_of_line(c)
                    {
                        html_inlines(inl, out);
                    } else {
                        out.push('\n');
                        for x in b {
                            html_block(x, out);
                        }
                    }
                }
                Body::Inline(inl) => html_inlines(inl, out),
                _ => {}
            }
            out.push_str(&format!("</{tag}>\n"));
        }
    }
}

fn kind_of_line(c: &Command) -> bool {
    kind(&c.name) == Kind::Line
}

fn html_inlines(inl: &[Inline], out: &mut String) {
    for i in inl {
        match i {
            Inline::Text(t, _) => out.push_str(&esc(&collapse(t))),
            Inline::Math(m, _) => {
                out.push_str(&format!("<span class=\"math\">\\({}\\)</span>", esc(m)))
            }
            Inline::Command(c) => {
                let (open, close) = match c.name.as_str() {
                    "b" => ("<strong>".to_string(), "</strong>"),
                    "i" => ("<em>".into(), "</em>"),
                    "u" => ("<u>".into(), "</u>"),
                    "del" => ("<del>".into(), "</del>"),
                    "ins" => ("<ins>".into(), "</ins>"),
                    "sup" => ("<sup>".into(), "</sup>"),
                    "sub" => ("<sub>".into(), "</sub>"),
                    "hl" => ("<mark>".into(), "</mark>"),
                    "q" => ("“".into(), "”"),
                    "td" => ("<td>".into(), "</td>"),
                    "th" => ("<th>".into(), "</th>"),
                    "br" => ("<br>".into(), ""),
                    "link" => (
                        format!("<a href=\"{}\">", esc(positional(c).unwrap_or_default())),
                        "</a>",
                    ),
                    "ref" => (
                        format!("<a href=\"#{}\">", esc(positional(c).unwrap_or_default())),
                        "</a>",
                    ),
                    "fn" => ("<span class=\"footnote\">".into(), "</span>"),
                    _ => (format!("<span data-kind=\"{}\">", c.name), "</span>"),
                };
                out.push_str(&open);
                match &c.body {
                    Body::Inline(v) => html_inlines(v, out),
                    Body::Verbatim(v) => out.push_str(&esc(v)),
                    Body::None => {
                        if let Some(v) = positional(c).filter(|_| !close.is_empty()) {
                            out.push_str(&esc(v));
                        }
                    }
                    _ => {}
                }
                out.push_str(close);
            }
        }
    }
}

// ---------------------------------------------------------------------
// The examples of the RFC.

/// The examples of an Org chapter written in the Kalem format: its `klm`
/// source blocks (Part III of the Book).
pub fn org_examples(org: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut lines = org.lines().enumerate();
    while let Some((n, l)) = lines.next() {
        if !l.trim().eq_ignore_ascii_case("#+begin_src klm") {
            continue;
        }
        let mut body = String::new();
        for (_, l) in lines.by_ref() {
            if l.trim().eq_ignore_ascii_case("#+end_src") {
                break;
            }
            // Org's escape of lines that would read as its own syntax.
            let l = l
                .strip_prefix(',')
                .filter(|r| r.starts_with(['*', '#']))
                .unwrap_or(l);
            body.push_str(l);
            body.push('\n');
        }
        out.push((n + 1, body));
    }
    out
}

/// The examples of a Markdown file written in the Kalem format: the
/// fenced blocks without a language (or `klm`) that hold a command.
pub fn examples(markdown: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut lines = markdown.lines().enumerate();
    while let Some((n, l)) = lines.next() {
        let Some(lang) = l.strip_prefix("```") else {
            continue;
        };
        let mut body = String::new();
        for (_, l) in lines.by_ref() {
            if l.starts_with("```") {
                break;
            }
            body.push_str(l);
            body.push('\n');
        }
        let lang = lang.trim();
        let ebnf = body.contains(" = ") && body.contains(" ;\n");
        if (lang.is_empty() || lang == "klm") && body.trim_start().starts_with('\\') && !ebnf {
            out.push((n + 1, body));
        }
    }
    out
}
